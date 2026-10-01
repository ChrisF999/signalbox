//! Login (spec §8): the pure admission rules, the pending-login table, and
//! the whole authorization code + PKCE + nonce + state flow against a small
//! OpenID provider run inside the test (discovery, JWKS, token endpoint,
//! RS256 ID tokens signed with a test-only key). Runs in both builds: this
//! is the release login path.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{Form, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use bot::net::{Conn, HttpResponse, NetError, http_get};
use openidconnect::core::{
    CoreGenderClaim, CoreJweContentEncryptionAlgorithm, CoreJwsSigningAlgorithm, CoreRsaPrivateSigningKey,
};
use openidconnect::url::Url;
use openidconnect::{
    AdditionalClaims, IdToken, IdTokenClaims, JsonWebKeyId, JsonWebKeySet, PkceCodeChallenge, PkceCodeVerifier,
    PrivateSigningKey,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use server::config::{Config, OidcConfig};
use server::oidc::*;

/// TEST-ONLY RSA key: the in-test provider signs its ID tokens with it. It
/// is public in the repository and trusted by nothing outside this file.
const KEY_PEM: &str = include_str!("fixtures/oidc-test-key.pem");
/// TEST-ONLY too: a second key, never published in the provider's JWKS.
const OTHER_KEY_PEM: &str = include_str!("fixtures/oidc-test-key-2.pem");
const CLIENT_ID: &str = "signalbox";
const CLIENT_SECRET: &str = "test-secret";

fn s(x: &str) -> String {
    x.to_string()
}

// ---- the pure parts ----

#[test]
fn admit_requires_the_group_and_a_username() {
    let groups = |g: &[&str]| g.iter().map(|x| s(x)).collect::<Vec<String>>();
    assert_eq!(admit(Some("ann"), &groups(&["staff", "signalbox-users"])), Ok(s("ann")));
    assert_eq!(admit(Some("Hackney & Bow"), &groups(&["signalbox-users"])), Ok(s("Hackney & Bow")), "names are opaque");
    assert_eq!(admit(Some("ann"), &groups(&[])), Err(Denied::NotInGroup));
    assert_eq!(admit(Some("ann"), &groups(&["Signalbox-Users", "signalbox-users-old"])), Err(Denied::NotInGroup));
    assert_eq!(admit(None, &groups(&["signalbox-users"])), Err(Denied::NoUsername));
    assert_eq!(admit(Some(""), &groups(&["signalbox-users"])), Err(Denied::NoUsername));
}

#[test]
fn admit_refuses_the_robot_name_in_any_case() {
    let members = vec![s("signalbox-users")];
    for name in ["robot", "Robot", "ROBOT", "rObOt"] {
        assert_eq!(admit(Some(name), &members), Err(Denied::ReservedName), "{name}");
    }
    assert_eq!(admit(Some("robot2"), &members), Ok(s("robot2")), "only the exact name is reserved");
}

fn jwt_with(payload: serde_json::Value) -> String {
    format!("eyJhbGciOiJub25lIn0.{}.sig", URL_SAFE_NO_PAD.encode(payload.to_string()))
}

#[test]
fn groups_come_from_the_token_payload() {
    assert_eq!(groups_from_id_token(&jwt_with(json!({"groups": ["a", "signalbox-users"]}))), Ok(vec![s("a"), s("signalbox-users")]));
    assert_eq!(groups_from_id_token(&jwt_with(json!({"sub": "x"}))), Ok(vec![]), "no claim, no groups");
    assert_eq!(groups_from_id_token(&jwt_with(json!({"groups": null}))), Ok(vec![]));
    assert!(groups_from_id_token(&jwt_with(json!({"groups": "signalbox-users"}))).is_err(), "a string is not a list");
    assert!(groups_from_id_token(&jwt_with(json!({"groups": [1]}))).is_err());
    assert!(groups_from_id_token("not a jwt").is_err());
    assert!(groups_from_id_token("a.!!!.c").is_err());
}

#[test]
fn pending_logins_are_one_shot_bounded_and_expire() {
    let p = PendingLogins::new();
    let t0 = Instant::now();
    p.insert_at(s("st"), s("n"), s("v"), t0);
    let got = p.take_at("st", t0).unwrap();
    assert_eq!((got.nonce.as_str(), got.pkce_verifier.as_str()), ("n", "v"));
    assert!(p.take_at("st", t0).is_none(), "taken once");
    p.insert_at(s("old"), s("n"), s("v"), t0);
    assert!(p.take_at("old", t0 + PENDING_TTL).is_none(), "expired");
    for i in 0..MAX_PENDING_LOGINS + 5 {
        p.insert_at(format!("s{i}"), s("n"), s("v"), t0 + Duration::from_millis(i as u64));
    }
    assert_eq!(p.len(), MAX_PENDING_LOGINS);
    assert!(p.take_at("s0", t0).is_none(), "the oldest went first");
    assert!(p.take_at(&format!("s{}", MAX_PENDING_LOGINS + 4), t0).is_some());
}

// ---- a small OpenID provider ----

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Groups {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    groups: Option<Vec<String>>,
}

impl AdditionalClaims for Groups {}

type TestIdToken = IdToken<Groups, CoreGenderClaim, CoreJweContentEncryptionAlgorithm, CoreJwsSigningAlgorithm>;

/// Who "logs in" at the provider, and what the token says.
#[derive(Clone, Debug)]
struct Login {
    username: Option<String>,
    groups: Option<Vec<String>>,
    audience: String,
    /// `None` = the nonce the front asked for.
    nonce: Option<String>,
    /// `None` = the provider's own issuer.
    issuer: Option<String>,
    /// Sign with the second key under the first key's kid.
    forged: bool,
    /// Seconds from now until the token expires (negative: already expired).
    expires_in: i64,
}

fn member(name: &str) -> Login {
    Login {
        username: Some(s(name)),
        groups: Some(vec![s("signalbox-users")]),
        audience: s(CLIENT_ID),
        nonce: None,
        issuer: None,
        forged: false,
        expires_in: 300,
    }
}

struct Issued {
    login: Login,
    nonce: String,
    challenge: String,
    redirect_uri: String,
}

struct ProviderState {
    issuer: String,
    key: CoreRsaPrivateSigningKey,
    other_key: CoreRsaPrivateSigningKey,
    codes: Mutex<BTreeMap<String, Issued>>,
    next_code: Mutex<u32>,
}

struct Provider {
    state: Arc<ProviderState>,
}

fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

async fn discovery(State(p): State<Arc<ProviderState>>) -> Json<serde_json::Value> {
    Json(json!({
        "issuer": p.issuer,
        "authorization_endpoint": format!("{}authorize", p.issuer),
        "token_endpoint": format!("{}token", p.issuer),
        "jwks_uri": format!("{}jwks", p.issuer),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
    }))
}

async fn jwks(State(p): State<Arc<ProviderState>>) -> Json<serde_json::Value> {
    Json(serde_json::to_value(JsonWebKeySet::new(vec![p.key.as_verification_key()])).unwrap())
}

async fn token(State(p): State<Arc<ProviderState>>, headers: HeaderMap, Form(f): Form<BTreeMap<String, String>>) -> Response {
    let bad = |why: &str| (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_grant", "error_description": why}))).into_response();
    let basic = format!("Basic {}", STANDARD.encode(format!("{CLIENT_ID}:{CLIENT_SECRET}")));
    if headers.get("authorization").and_then(|v| v.to_str().ok()) != Some(basic.as_str()) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "invalid_client"}))).into_response();
    }
    if f.get("grant_type").map(String::as_str) != Some("authorization_code") {
        return bad("grant_type");
    }
    let Some(issued) = f.get("code").and_then(|c| p.codes.lock().unwrap().remove(c)) else { return bad("unknown code") };
    let verifier = PkceCodeVerifier::new(f.get("code_verifier").cloned().unwrap_or_default());
    if PkceCodeChallenge::from_code_verifier_sha256(&verifier).as_str() != issued.challenge {
        return bad("PKCE");
    }
    if f.get("redirect_uri") != Some(&issued.redirect_uri) {
        return bad("redirect_uri");
    }
    let l = &issued.login;
    let mut claims = json!({
        "iss": l.issuer.clone().unwrap_or_else(|| p.issuer.clone()),
        "aud": [l.audience],
        "sub": "user-1",
        "iat": if l.expires_in < 0 { now_s() - 600 } else { now_s() },
        "exp": now_s().saturating_add_signed(l.expires_in),
        "nonce": l.nonce.clone().unwrap_or(issued.nonce),
    });
    if let Some(u) = &l.username {
        claims["preferred_username"] = json!(u);
    }
    if let Some(g) = &l.groups {
        claims["groups"] = json!(g);
    }
    let claims: IdTokenClaims<Groups, CoreGenderClaim> = serde_json::from_value(claims).unwrap();
    let key = if l.forged { &p.other_key } else { &p.key };
    let id_token = TestIdToken::new(claims, key, CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256, None, None).unwrap();
    Json(json!({"access_token": "at", "token_type": "bearer", "expires_in": 300, "id_token": id_token.to_string()})).into_response()
}

impl Provider {
    async fn start() -> Provider {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}/", listener.local_addr().unwrap());
        let key = CoreRsaPrivateSigningKey::from_pem(KEY_PEM, Some(JsonWebKeyId::new(s("test-1")))).unwrap();
        let other_key = CoreRsaPrivateSigningKey::from_pem(OTHER_KEY_PEM, Some(JsonWebKeyId::new(s("test-1")))).unwrap();
        let state = Arc::new(ProviderState { issuer, key, other_key, codes: Mutex::new(BTreeMap::new()), next_code: Mutex::new(0) });
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/jwks", get(jwks))
            .route("/token", post(token))
            .with_state(state.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Provider { state }
    }

    fn issuer(&self) -> String {
        self.state.issuer.clone()
    }

    /// The user signs in at the provider's authorize URL; returns the code
    /// the provider would hand back through the browser.
    fn sign_in(&self, authorize_url: &str, login: Login) -> (String, String) {
        let url = Url::parse(authorize_url).unwrap();
        assert!(authorize_url.starts_with(&format!("{}authorize?", self.state.issuer)), "{authorize_url}");
        let q: BTreeMap<String, String> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["client_id"], CLIENT_ID);
        assert_eq!(q["code_challenge_method"], "S256");
        assert!(q["scope"].split(' ').any(|x| x == "openid"), "{}", q["scope"]);
        let mut n = self.state.next_code.lock().unwrap();
        *n += 1;
        let code = format!("code-{n}");
        let issued = Issued {
            login,
            nonce: q["nonce"].clone(),
            challenge: q["code_challenge"].clone(),
            redirect_uri: q["redirect_uri"].clone(),
        };
        self.state.codes.lock().unwrap().insert(code.clone(), issued);
        (code, q["state"].clone())
    }
}

// ---- the front against it ----

fn config(name: &str, issuer: &str) -> Config {
    let root = std::env::temp_dir().join(format!("sbx-oidc-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("layouts")).unwrap();
    Config {
        addr: "127.0.0.1:0".parse().unwrap(),
        data_dir: root.join("data"),
        layouts_dir: root.join("layouts"),
        lessons_dir: root.join("lessons"),
        public_url: s("https://signalbox.test:50160"),
        oidc: Some(OidcConfig { issuer: s(issuer), client_id: s(CLIENT_ID), client_secret: s(CLIENT_SECRET) }),
        session_key: vec![9; 64],
        game_bin: PathBuf::from(env!("CARGO_BIN_EXE_signalbox-game")),
        web_dir: root.join("web"),
        admins: vec![],
    }
}

fn set_cookies(r: &HttpResponse) -> Vec<&str> {
    r.headers.iter().filter(|(k, _)| k == "set-cookie").map(|(_, v)| v.as_str()).collect()
}

/// `name=value` of the cookie called `name` that `r` sets.
fn cookie_of(r: &HttpResponse, name: &str) -> Option<String> {
    set_cookies(r)
        .into_iter()
        .map(|c| c.split(';').next().unwrap().trim().to_string())
        .find(|c| c.starts_with(&format!("{name}=")) && c.len() > name.len() + 1)
}

/// `/auth/login`: the provider URL it redirects to and the login cookie.
async fn start_login(base: &str) -> (String, String) {
    let r = http_get(base, "/auth/login", None).await.unwrap();
    assert_eq!(r.status, 303, "{}", r.body);
    let login_cookie = cookie_of(&r, LOGIN_COOKIE).expect("a login cookie");
    let c = set_cookies(&r).join(" | ");
    for attr in ["HttpOnly", "SameSite=Lax", "Secure", "Max-Age=600"] {
        assert!(c.contains(attr), "{attr} in {c}");
    }
    (r.header("location").unwrap().to_string(), login_cookie)
}

async fn callback(base: &str, state: &str, code: &str, cookie: Option<&str>) -> HttpResponse {
    http_get(base, &format!("/auth/callback?state={state}&code={code}"), cookie).await.unwrap()
}

#[tokio::test]
async fn a_member_logs_in_through_the_provider() {
    let p = Provider::start().await;
    let running = server::start(config("member", &p.issuer())).await.unwrap();
    let base = running.base();
    let (to, login_cookie) = start_login(&base).await;
    assert!(to.contains("redirect_uri=https%3A%2F%2Fsignalbox.test%3A50160%2Fauth%2Fcallback"), "{to}");
    let (code, state) = p.sign_in(&to, member("ann"));
    let r = callback(&base, &state, &code, Some(&login_cookie)).await;
    assert_eq!((r.status, r.header("location")), (303, Some("/")), "{}", r.body);
    let session = cookie_of(&r, "signalbox_session").expect("a session");
    let page = http_get(&base, "/", Some(&session)).await.unwrap();
    assert!(page.body.contains("Signed in as ann"), "{}", page.body);
    assert!(Conn::connect(&base, Some(&session)).await.is_ok(), "the socket opens");
    assert_eq!(running.sessions.len(), 1);
    running.stop().await;
}

#[tokio::test]
async fn callback_rejects_bad_state_replay_and_foreign_audience() {
    let p = Provider::start().await;
    let running = server::start(config("reject", &p.issuer())).await.unwrap();
    let base = running.base();
    let no_session = |r: &HttpResponse| cookie_of(r, "signalbox_session").is_none();

    // A state that is not this browser's.
    let (to, login_cookie) = start_login(&base).await;
    let (code, _) = p.sign_in(&to, member("ann"));
    let r = callback(&base, "someone-elses-state", &code, Some(&login_cookie)).await;
    assert!(r.status == 400 && no_session(&r), "{} {}", r.status, r.body);

    // No login cookie at all.
    let (to, _) = start_login(&base).await;
    let (code, state) = p.sign_in(&to, member("ann"));
    let r = callback(&base, &state, &code, None).await;
    assert!(r.status == 400 && no_session(&r), "{} {}", r.status, r.body);

    // The provider said no, or the user cancelled.
    let r = http_get(&base, "/auth/callback?error=access_denied", None).await.unwrap();
    assert!(r.status == 400 && no_session(&r), "{}", r.status);

    // A good login once; the same callback again is refused.
    let (to, login_cookie) = start_login(&base).await;
    let (code, state) = p.sign_in(&to, member("ann"));
    assert_eq!(callback(&base, &state, &code, Some(&login_cookie)).await.status, 303);
    let r = callback(&base, &state, &code, Some(&login_cookie)).await;
    assert!(r.status == 400 && no_session(&r), "replayed: {} {}", r.status, r.body);

    // Tokens the front must not accept.
    let refuse = [
        (Login { audience: s("another-app"), ..member("ann") }, 403),
        (Login { groups: Some(vec![s("staff")]), ..member("ann") }, 403),
        (Login { groups: None, ..member("ann") }, 403),
        (Login { username: None, ..member("ann") }, 403),
        (Login { nonce: Some(s("not-the-nonce")), ..member("ann") }, 403),
        // Signed with another key under the provider's kid, from another
        // issuer, or already expired: the ID token fails verification.
        (Login { forged: true, ..member("ann") }, 403),
        (Login { issuer: Some(s("https://auth.example.test/application/o/signalbox/")), ..member("ann") }, 403),
        (Login { expires_in: -120, ..member("ann") }, 403),
    ];
    for (login, status) in refuse {
        let (to, login_cookie) = start_login(&base).await;
        let (code, state) = p.sign_in(&to, login.clone());
        let r = callback(&base, &state, &code, Some(&login_cookie)).await;
        assert!(r.status == status && no_session(&r), "{login:?}: {} {}", r.status, r.body);
        if login.forged || login.issuer.is_some() || login.expires_in < 0 {
            assert!(r.body.contains("could not be verified"), "{login:?}: {}", r.body);
        }
    }
    assert_eq!(running.sessions.len(), 1, "only the one good login");
    running.stop().await;
}

#[tokio::test]
async fn login_answers_503_while_the_provider_is_down() {
    // Nothing listens on port 9; the front still starts.
    let running = server::start(config("down", "http://127.0.0.1:9/")).await.unwrap();
    let r = http_get(&running.base(), "/auth/login", None).await.unwrap();
    assert_eq!(r.status, 503);
    assert!(set_cookies(&r).is_empty());
    let e = Conn::connect(&running.base(), None).await.err().expect("refused");
    assert!(matches!(e, NetError::Status(401)), "{e}");
    running.stop().await;
}

#[tokio::test]
async fn an_authentik_user_called_robot_gets_no_session() {
    let p = Provider::start().await;
    let running = server::start(config("robot", &p.issuer())).await.unwrap();
    let base = running.base();
    for name in ["robot", "Robot"] {
        let (to, login_cookie) = start_login(&base).await;
        let (code, state) = p.sign_in(&to, member(name));
        let r = callback(&base, &state, &code, Some(&login_cookie)).await;
        assert!(r.status == 403 && cookie_of(&r, "signalbox_session").is_none(), "{name}: {} {}", r.status, r.body);
    }
    assert!(running.sessions.is_empty());
    running.stop().await;
}
