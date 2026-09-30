//! The front's pure parts: configuration, sessions, the rate limit and the
//! placeholder page.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use protocol::{AreaHolder, GameInfo, GameState, LayoutInfo};
use server::config::Config;
use server::limit::{MAX_MSGS_PER_S, RateLimit};
use server::session::{SESSION_TTL, Sessions};
use server::web::{escape_html, index_page};

const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\
                   0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn cfg(vars: &[(&str, &str)]) -> Result<Config, String> {
    let m: BTreeMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    Config::from_lookup(|k| m.get(k).cloned())
}

const OIDC: [(&str, &str); 4] = [
    ("OIDC_ISSUER", "https://auth.example/application/o/signalbox/"),
    ("OIDC_CLIENT_ID", "sbx"),
    ("OIDC_CLIENT_SECRET", "s3cret"),
    ("SIGNALBOX_PUBLIC_URL", "https://ra.example:50160/"),
];

fn with(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut v = vec![("SIGNALBOX_SESSION_KEY", KEY)];
    v.extend(OIDC);
    v.extend(extra);
    v
}

#[test]
fn config_defaults_and_overrides() {
    let c = cfg(&with(&[])).unwrap();
    assert_eq!(c.addr.to_string(), "0.0.0.0:9160");
    assert_eq!((c.data_dir, c.layouts_dir), (PathBuf::from("/data"), PathBuf::from("/opt/signalbox/layouts")));
    assert_eq!(c.public_url, "https://ra.example:50160", "no trailing slash");
    assert_eq!(c.session_key.len(), 64);
    let o = c.oidc.unwrap();
    assert_eq!((o.issuer.as_str(), o.client_id.as_str(), o.client_secret.as_str()), (OIDC[0].1, "sbx", "s3cret"));
    assert!(c.game_bin.ends_with("signalbox-game"), "next to the running binary: {}", c.game_bin.display());
    let c = cfg(&with(&[
        ("SIGNALBOX_ADDR", "127.0.0.1:1"),
        ("SIGNALBOX_DATA", "/d"),
        ("SIGNALBOX_LAYOUTS", "/l"),
        ("SIGNALBOX_GAME_BIN", "/bin/g"),
    ]))
    .unwrap();
    assert_eq!((c.addr.to_string(), c.data_dir, c.layouts_dir, c.game_bin), (
        "127.0.0.1:1".to_string(),
        PathBuf::from("/d"),
        PathBuf::from("/l"),
        PathBuf::from("/bin/g")
    ));
}

#[test]
fn config_problems_are_one_clear_line() {
    let err = |vars: &[(&str, &str)]| cfg(vars).unwrap_err();
    let mut no_key: Vec<(&str, &str)> = OIDC.to_vec();
    assert_eq!(err(&no_key), "SIGNALBOX_SESSION_KEY is required (hex, at least 64 bytes)");
    no_key.push(("SIGNALBOX_SESSION_KEY", "zz"));
    assert_eq!(err(&no_key), "SIGNALBOX_SESSION_KEY is not hex");
    let short = &KEY[..126];
    assert_eq!(err(&with(&[("SIGNALBOX_SESSION_KEY", short)])[1..]), "SIGNALBOX_SESSION_KEY has 63 bytes; it needs at least 64");
    assert_eq!(err(&with(&[("SIGNALBOX_ADDR", "nowhere")])), "SIGNALBOX_ADDR `nowhere` is not host:port");
    assert_eq!(
        err(&with(&[("SIGNALBOX_PUBLIC_URL", "ra.example")])[..]),
        "SIGNALBOX_PUBLIC_URL `ra.example` must start with https:// or http://"
    );
    let partial = [("SIGNALBOX_SESSION_KEY", KEY), OIDC[0], OIDC[3]];
    assert_eq!(err(&partial), "OIDC_ISSUER, OIDC_CLIENT_ID and OIDC_CLIENT_SECRET are all required");
    let no_url = [("SIGNALBOX_SESSION_KEY", KEY), OIDC[0], OIDC[1], OIDC[2]];
    assert_eq!(err(&no_url), "SIGNALBOX_PUBLIC_URL is required with OIDC");
    let bare = [("SIGNALBOX_SESSION_KEY", KEY)];
    if cfg!(feature = "dev-auth") {
        let c = cfg(&bare).unwrap();
        assert_eq!((c.oidc, c.public_url.as_str()), (None, "http://0.0.0.0:9160"));
    } else {
        assert_eq!(err(&bare), "OIDC_ISSUER, OIDC_CLIENT_ID and OIDC_CLIENT_SECRET are all required");
    }
}

#[test]
fn sessions_live_twelve_hours_and_can_be_ended() {
    let s = Sessions::new();
    let t0 = Instant::now();
    let id = s.create_at("ann", t0);
    assert_eq!(id.len(), 64);
    assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_ne!(s.create_at("ann", t0), id, "every login gets its own id");
    assert_eq!(s.user_at(&id, t0 + SESSION_TTL - Duration::from_secs(1)).as_deref(), Some("ann"));
    assert_eq!(s.user_at(&id, t0 + SESSION_TTL), None, "expired");
    assert_eq!(s.user_at(&id, t0), None, "and forgotten");
    let id = s.create_at("bob", t0);
    s.remove(&id);
    assert_eq!(s.user_at(&id, t0), None);
    assert_eq!(s.user_at("not-an-id", t0), None);
}

#[test]
fn the_rate_limit_counts_a_one_second_window() {
    let t0 = Instant::now();
    let mut l = RateLimit::new(t0);
    for _ in 0..MAX_MSGS_PER_S {
        assert!(l.allow(t0 + Duration::from_millis(900)));
    }
    assert!(!l.allow(t0 + Duration::from_millis(999)), "the 21st in one second");
    assert!(l.allow(t0 + Duration::from_secs(1)), "a new window");
}

#[test]
fn the_placeholder_page_escapes_every_name() {
    assert_eq!(escape_html(r#"<a href="x">&'"#), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
    let games = [GameInfo {
        id: "g-abcdefgh2345".into(),
        layout: "<script>".into(),
        state: GameState::Crashed,
        sim_time: 0.0,
        areas: vec![AreaHolder { name: "Hackney & Bow".into(), holder: None }],
        players: vec![],
        error: Some("<b>bad</b>".into()),
    }];
    let page = index_page("a<b", &games, &[LayoutInfo { name: "drain".into(), areas: vec![] }]);
    assert!(!page.contains("<script>") && !page.contains("<b>bad"), "{page}");
    assert!(page.contains("Hackney &amp; Bow: robot") && page.contains("Signed in as a&lt;b"), "{page}");
}
