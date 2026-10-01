//! The front's pure parts: configuration, sessions, the rate limit, the
//! placeholder page and the web client's files.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use protocol::{AreaHolder, GameInfo, GameState, LayoutInfo};
use server::assets::{WebAssets, content_type, etag, valid_asset_name};
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
    assert_eq!(c.web_dir, PathBuf::from("/opt/signalbox/web"));
    assert!(c.admins.is_empty(), "nobody is an admin unless named");
    let c = cfg(&with(&[("SIGNALBOX_ADMINS", " skye, ,ann ,")])).unwrap();
    assert_eq!(c.admins, ["skye", "ann"]);
    let c = cfg(&with(&[
        ("SIGNALBOX_ADDR", "127.0.0.1:1"),
        ("SIGNALBOX_DATA", "/d"),
        ("SIGNALBOX_LAYOUTS", "/l"),
        ("SIGNALBOX_GAME_BIN", "/bin/g"),
        ("SIGNALBOX_WEB", "/w"),
    ]))
    .unwrap();
    assert_eq!(c.web_dir, PathBuf::from("/w"));
    assert_eq!((c.addr.to_string(), c.data_dir, c.layouts_dir, c.game_bin), (
        "127.0.0.1:1".to_string(),
        PathBuf::from("/d"),
        PathBuf::from("/l"),
        PathBuf::from("/bin/g")
    ));
}

#[test]
fn the_lessons_directory_defaults_to_the_images() {
    assert_eq!(cfg(&with(&[])).unwrap().lessons_dir, PathBuf::from("/opt/signalbox/lessons"));
    assert_eq!(cfg(&with(&[("SIGNALBOX_LESSONS", "/x")])).unwrap().lessons_dir, PathBuf::from("/x"));
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
    assert!(!l.allow(t0 + Duration::from_millis(1900)), "the oldest is exactly a second old: still counted");
    assert!(l.allow(t0 + Duration::from_millis(1901)), "more than a second after the oldest");
}

#[test]
fn the_rate_limit_window_slides() {
    let t0 = Instant::now();
    let mut l = RateLimit::new(t0);
    assert!(l.allow(t0));
    for _ in 1..MAX_MSGS_PER_S {
        assert!(l.allow(t0 + Duration::from_millis(999)));
    }
    assert!(!l.allow(t0 + Duration::from_secs(1)), "20 fall within the last second");
    let mut l = RateLimit::new(t0);
    for _ in 0..MAX_MSGS_PER_S {
        assert!(l.allow(t0 + Duration::from_millis(999)));
    }
    assert!(!l.allow(t0 + Duration::from_millis(1500)), "no burst across a window edge");
    assert!(l.allow(t0 + Duration::from_millis(2000)), "the oldest is more than a second old");
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
        creator: None,
        can_delete: false,
        preparing: None,
    }];
    let page = index_page("a<b", &games, &[LayoutInfo { name: "drain".into(), areas: vec![] }]);
    assert!(!page.contains("<script>") && !page.contains("<b>bad"), "{page}");
    assert!(page.contains("Hackney &amp; Bow: robot") && page.contains("Signed in as a&lt;b"), "{page}");
}

#[test]
fn asset_names_are_plain_file_names() {
    for ok in ["index.html", "signalbox_web.js", "signalbox_web_bg.wasm", "a-b.c_d"] {
        assert!(valid_asset_name(ok), "{ok}");
    }
    let long = "a".repeat(101);
    for bad in ["", ".", "..", ".hidden", "../x", "a/b", "a\\b", "%2e%2e", "a b", "é.js", long.as_str()] {
        assert!(!valid_asset_name(bad), "{bad}");
    }
    assert_eq!(content_type("x.wasm"), "application/wasm");
    assert_eq!(content_type("x.js"), "text/javascript; charset=utf-8");
    assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
    assert_eq!(content_type("README"), "application/octet-stream");
    assert_eq!(etag(b"abc"), etag(b"abc"));
    assert_ne!(etag(b"abc"), etag(b"abd"));
    assert!(etag(b"").starts_with('"') && etag(b"").ends_with('"'));
}

#[test]
fn web_assets_load_index_and_plain_app_files_only() {
    let dir = std::env::temp_dir().join(format!("sbx-web-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(WebAssets::load(&dir), Ok(None), "no directory: the placeholder stays");
    std::fs::create_dir_all(dir.join("app/sub")).unwrap();
    let err = WebAssets::load(&dir).unwrap_err();
    assert!(err.contains("index.html"), "{err}");
    std::fs::write(dir.join("index.html"), "<canvas>").unwrap();
    std::fs::write(dir.join("app/signalbox_web.js"), "import x").unwrap();
    std::fs::write(dir.join("app/signalbox_web_bg.wasm"), b"\0asm").unwrap();
    std::fs::write(dir.join("app/.hidden"), "no").unwrap();
    std::fs::write(dir.join("app/sub/deep.js"), "no").unwrap();
    std::fs::write(dir.join("elsewhere.js"), "no").unwrap();
    let outside = std::env::temp_dir().join(format!("sbx-web-outside-{}", std::process::id()));
    std::fs::write(&outside, "secret").unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("app/link.js")).unwrap();
    let w = WebAssets::load(&dir).unwrap().unwrap();
    let _ = std::fs::remove_file(&outside);
    assert_eq!(&w.index.body[..], b"<canvas>");
    assert_eq!(w.app.keys().collect::<Vec<_>>(), ["signalbox_web.js", "signalbox_web_bg.wasm"], "symlinks are not loaded");
    assert_eq!(w.app["signalbox_web_bg.wasm"].content_type, "application/wasm");
    assert_eq!(w.app["signalbox_web.js"].etag, etag(b"import x"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn web_assets_attach_precompressed_copies_to_their_files() {
    let dir = std::env::temp_dir().join(format!("sbx-web-enc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("app")).unwrap();
    std::fs::write(dir.join("index.html"), "<canvas>").unwrap();
    std::fs::write(dir.join("index.html.br"), "index-br").unwrap();
    std::fs::write(dir.join("app/a.wasm"), "wasm").unwrap();
    std::fs::write(dir.join("app/a.wasm.br"), "wasm-br").unwrap();
    std::fs::write(dir.join("app/a.wasm.gz"), "wasm-gz").unwrap();
    std::fs::write(dir.join("app/b.js"), "js").unwrap();
    std::fs::write(dir.join("app/orphan.js.gz"), "no base").unwrap();
    let outside = std::env::temp_dir().join(format!("sbx-web-enc-outside-{}", std::process::id()));
    std::fs::write(&outside, "secret").unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("app/b.js.br")).unwrap();
    let w = WebAssets::load(&dir).unwrap().unwrap();
    assert_eq!(w.app.keys().collect::<Vec<_>>(), ["a.wasm", "b.js"], "copies are not files of their own");
    let a = &w.app["a.wasm"];
    assert_eq!((&a.body[..], a.etag.as_str()), (&b"wasm"[..], etag(b"wasm").as_str()));
    let br = a.br.as_ref().unwrap();
    assert_eq!((&br.body[..], br.etag.clone()), (&b"wasm-br"[..], etag(b"wasm-br")));
    assert_eq!(&a.gzip.as_ref().unwrap().body[..], b"wasm-gz");
    assert_eq!((w.app["b.js"].br.as_ref(), w.app["b.js"].gzip.as_ref()), (None, None), "a symlinked copy is not loaded");
    assert_eq!(&w.index.br.as_ref().unwrap().body[..], b"index-br");
    assert!(w.index.gzip.is_none());
    // A symlinked copy of the index is refused like the index itself.
    std::fs::remove_file(dir.join("index.html.br")).unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("index.html.br")).unwrap();
    let err = WebAssets::load(&dir).unwrap_err();
    assert!(err.contains("index.html.br") && err.contains("symlink"), "{err}");
    let _ = std::fs::remove_file(&outside);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn accept_encoding_picks_brotli_then_gzip_then_identity() {
    use server::assets::{Coding, pick_coding};
    for (ae, has_br, has_gz, want) in [
        (None, true, true, Coding::Identity),
        (Some("gzip, deflate, br"), true, true, Coding::Br),
        (Some("gzip, deflate, br"), false, true, Coding::Gzip),
        (Some("gzip, deflate, br"), false, false, Coding::Identity),
        (Some("br;q=0, gzip;q=0.1"), true, true, Coding::Gzip),
        (Some("Br ; Q=1"), true, true, Coding::Br),
        (Some("*"), true, true, Coding::Br),
        (Some("*, br;q=0"), true, true, Coding::Gzip),
        (Some("*;q=0"), true, true, Coding::Identity),
        (Some("x-gzip"), true, true, Coding::Gzip),
        (Some("br;q=bogus"), true, true, Coding::Identity),
        (Some("gzip;q=0.000"), true, true, Coding::Identity),
        (Some("brotli, gzipx"), true, true, Coding::Identity),
    ] {
        assert_eq!(pick_coding(ae, has_br, has_gz), want, "{ae:?} br={has_br} gz={has_gz}");
    }
}

#[test]
fn web_assets_refuse_a_symlinked_index_and_unreadable_dirs() {
    let dir = std::env::temp_dir().join(format!("sbx-web-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("real.html");
    std::fs::write(&target, "secret").unwrap();
    std::os::unix::fs::symlink(&target, dir.join("index.html")).unwrap();
    let err = WebAssets::load(&dir).unwrap_err();
    assert!(err.contains("index.html") && err.contains("symlink"), "{err}");
    // Not NotFound (a path below a file): start must fail, not serve the placeholder.
    assert!(WebAssets::load(&target.join("web")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
