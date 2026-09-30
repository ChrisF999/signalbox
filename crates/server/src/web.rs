//! HTTP routes (spec §8): the browser client (or C2's placeholder page
//! when it is not installed), the WebSocket, and the auth routes. Nothing
//! but the login flow answers without a session.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRef, Path, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH, X_CONTENT_TYPE_OPTIONS};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::get;
use axum_extra::extract::cookie::{Cookie, Key, SignedCookieJar};
use protocol::{GameInfo, LayoutInfo, codes};
use serde::Deserialize;

use crate::assets::{Asset, WebAssets};
use crate::limit::RateLimit;
use crate::oidc::{Denied, LOGIN_COOKIE, LoginError, Oidc, PENDING_TTL};
use crate::session::{SESSION_COOKIE, SESSION_TTL, Sessions, cookie};
use crate::supervisor::Supervisor;

/// Largest message or frame a client may send.
pub const MAX_CLIENT_MESSAGE: usize = 64 * 1024;
/// A client that takes longer than this to accept one frame is dropped.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// WebSocket close code for too many messages ("policy violation").
pub const CLOSE_POLICY: u16 = 1008;

#[derive(Clone)]
pub struct AppState {
    pub sup: Arc<Supervisor>,
    pub sessions: Arc<Sessions>,
    pub key: Key,
    /// `None` only in a `dev-auth` build started without OIDC settings.
    pub oidc: Option<Arc<Oidc>>,
    /// The browser client; `None` serves the placeholder page.
    pub web: Option<Arc<WebAssets>>,
}

impl FromRef<AppState> for Key {
    fn from_ref(s: &AppState) -> Key {
        s.key.clone()
    }
}

pub fn router(state: AppState) -> Router {
    let r = Router::new()
        .route("/", get(index))
        .route("/app/{file}", get(app_file))
        .route("/ws", get(ws))
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", get(logout));
    #[cfg(feature = "dev-auth")]
    let r = r.route("/auth/dev", get(dev::login));
    r.with_state(state)
}

/// The logged-in user, from the signed session cookie.
pub fn user_of(state: &AppState, jar: &SignedCookieJar) -> Option<String> {
    let c = jar.get(SESSION_COOKIE)?;
    state.sessions.user(c.value())
}

pub fn escape_html(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '"' => "&quot;".into(),
            '\'' => "&#39;".into(),
            c => c.to_string(),
        })
        .collect()
}

/// The page served when the browser client is not installed.
pub fn index_page(user: &str, games: &[GameInfo], layouts: &[LayoutInfo]) -> String {
    let e = escape_html;
    let mut rows = String::new();
    for g in games {
        let holders: Vec<String> =
            g.areas.iter().map(|a| format!("{}: {}", e(&a.name), e(a.holder.as_deref().unwrap_or("robot")))).collect();
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{:?}</td><td>{}</td><td>{}</td></tr>\n",
            e(&g.id),
            e(&g.layout),
            g.state,
            holders.join(", "),
            e(g.error.as_deref().unwrap_or("")),
        ));
    }
    let layouts: Vec<String> = layouts.iter().map(|l| e(&l.name)).collect();
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>signalbox</title></head><body>\n\
         <h1>signalbox</h1>\n<p>Signed in as {}. <a href=\"/auth/logout\">Sign out</a></p>\n\
         <p>The browser client is not installed on this server (<code>SIGNALBOX_WEB</code>); bots play over <code>/ws</code>.</p>\n\
         <h2>Games</h2>\n<table><tr><th>Game</th><th>Layout</th><th>State</th><th>Areas</th><th>Error</th></tr>\n{}</table>\n\
         <h2>Layouts</h2>\n<p>{}</p>\n</body></html>\n",
        e(user),
        rows,
        layouts.join(", "),
    )
}

async fn index(State(state): State<AppState>, jar: SignedCookieJar, headers: HeaderMap) -> Response {
    let Some(user) = user_of(&state, &jar) else { return Redirect::to("/auth/login").into_response() };
    match &state.web {
        Some(w) => serve(&w.index, &headers),
        None => Html(index_page(&user, &state.sup.list_games(), &state.sup.layouts().infos())).into_response(),
    }
}

/// `/app/{file}`: only names loaded at start; 401 without a session.
async fn app_file(State(state): State<AppState>, jar: SignedCookieJar, Path(file): Path<String>, headers: HeaderMap) -> Response {
    if user_of(&state, &jar).is_none() {
        return (StatusCode::UNAUTHORIZED, "sign in first").into_response();
    }
    match state.web.as_ref().and_then(|w| w.app.get(&file)) {
        Some(a) => serve(a, &headers),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// An asset from memory; 304 when the browser already has this version.
/// `no-cache`: browsers revalidate every load, so a new build is picked up
/// at once while an unchanged one costs a 304.
fn serve(a: &Asset, headers: &HeaderMap) -> Response {
    let fresh = headers
        .get(IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == a.etag));
    let common = [(ETAG, a.etag.clone()), (CACHE_CONTROL, "no-cache".to_string())];
    if fresh {
        return (StatusCode::NOT_MODIFIED, common).into_response();
    }
    let typed = [(CONTENT_TYPE, a.content_type.to_string()), (X_CONTENT_TYPE_OPTIONS, "nosniff".to_string())];
    (common, typed, a.body.clone()).into_response()
}

async fn login(State(state): State<AppState>, jar: SignedCookieJar) -> Response {
    let Some(oidc) = &state.oidc else { return (StatusCode::NOT_FOUND, "no login provider is configured").into_response() };
    match oidc.begin().await {
        Ok((url, login_state)) => (jar.add(cookie(LOGIN_COOKIE, &login_state, PENDING_TTL.as_secs())), Redirect::to(&url)).into_response(),
        Err(e) => {
            eprintln!("signalbox-server: login: {e}");
            (StatusCode::SERVICE_UNAVAILABLE, "the login service is unavailable; try again shortly").into_response()
        }
    }
}

#[derive(Deserialize)]
struct CallbackQuery {
    state: Option<String>,
    code: Option<String>,
}

/// The provider sends the browser back here. The `state` must be the one
/// this browser's login cookie holds (login CSRF) and a live, unused login.
async fn callback(State(state): State<AppState>, jar: SignedCookieJar, Query(q): Query<CallbackQuery>) -> Response {
    let Some(oidc) = &state.oidc else { return (StatusCode::NOT_FOUND, "no login provider is configured").into_response() };
    let started = jar.get(LOGIN_COOKIE).map(|c| c.value().to_string());
    let jar = jar.remove(Cookie::build(LOGIN_COOKIE).path("/"));
    let (Some(login_state), Some(code)) = (q.state, q.code) else {
        return (StatusCode::BAD_REQUEST, jar, "the login was cancelled or failed; start again at /auth/login").into_response();
    };
    if started.as_deref() != Some(login_state.as_str()) {
        return (StatusCode::BAD_REQUEST, jar, "this login was not started in this browser").into_response();
    }
    match oidc.finish(&login_state, &code).await {
        Ok(user) => {
            let id = state.sessions.create(&user);
            (jar.add(cookie(SESSION_COOKIE, &id, SESSION_TTL.as_secs())), Redirect::to("/")).into_response()
        }
        Err(LoginError::BadState) => (StatusCode::BAD_REQUEST, jar, "unknown or expired login; start again").into_response(),
        Err(LoginError::Denied(Denied::NotInGroup)) => {
            (StatusCode::FORBIDDEN, jar, "your account is not in signalbox-users").into_response()
        }
        Err(LoginError::Denied(Denied::NoUsername)) => {
            (StatusCode::FORBIDDEN, jar, "your account has no username").into_response()
        }
        Err(LoginError::Denied(Denied::ReservedName)) => {
            (StatusCode::FORBIDDEN, jar, "`robot` is reserved for the automatic signaller").into_response()
        }
        Err(e @ LoginError::Rejected(_)) => {
            eprintln!("signalbox-server: callback: {e}");
            (StatusCode::FORBIDDEN, jar, "the login could not be verified").into_response()
        }
        Err(e @ LoginError::Unavailable(_)) => {
            eprintln!("signalbox-server: callback: {e}");
            (StatusCode::BAD_GATEWAY, jar, "the login service is unavailable; try again shortly").into_response()
        }
    }
}

async fn logout(State(state): State<AppState>, jar: SignedCookieJar) -> Response {
    if let Some(c) = jar.get(SESSION_COOKIE) {
        state.sessions.remove(c.value());
    }
    let jar = jar.remove(Cookie::build(SESSION_COOKIE).path("/"));
    (jar, Html("<!doctype html><p>Signed out. <a href=\"/auth/login\">Sign in</a></p>")).into_response()
}

async fn ws(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    let Some(user) = user_of(&state, &jar) else { return (StatusCode::UNAUTHORIZED, "sign in first").into_response() };
    let upgrade = match upgrade {
        Ok(u) => u,
        Err(e) => return e.into_response(),
    };
    upgrade
        .max_message_size(MAX_CLIENT_MESSAGE)
        .max_frame_size(MAX_CLIENT_MESSAGE)
        .on_upgrade(move |socket| client_loop(state.sup, user, socket))
}

async fn send(socket: &mut WebSocket, m: Message) -> bool {
    matches!(tokio::time::timeout(SEND_TIMEOUT, socket.send(m)).await, Ok(Ok(())))
}

/// One client socket: frames in go to the supervisor, the outbox goes out.
async fn client_loop(sup: Arc<Supervisor>, user: String, mut socket: WebSocket) {
    let me = sup.attach(&user);
    let mut limit = RateLimit::new(Instant::now());
    loop {
        tokio::select! {
            m = socket.recv() => {
                let text = match m {
                    Some(Ok(Message::Text(t))) => Some(t),
                    Some(Ok(Message::Binary(_))) => None,
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                };
                if !limit.allow(Instant::now()) {
                    let close = CloseFrame { code: CLOSE_POLICY, reason: "too many messages".into() };
                    let _ = send(&mut socket, Message::Close(Some(close))).await;
                    break;
                }
                match text {
                    Some(t) => sup.handle_text(&user, me.conn, t.as_str()),
                    None => sup.reply(&user, me.conn, protocol::ServerFrame::error(codes::BAD_MESSAGE, "binary frames are not used")),
                }
            }
            f = me.outbox.pop() => match f {
                Some(f) => {
                    if !send(&mut socket, Message::Text(f.to_json().into())).await {
                        break;
                    }
                }
                None => {
                    let _ = send(&mut socket, Message::Close(None)).await;
                    break;
                }
            },
        }
    }
    sup.detach(&user, me.conn);
}

#[cfg(feature = "dev-auth")]
pub mod dev {
    //! `/auth/dev?user=<name>`: a session for anyone, for tests and bots.
    //! Compiled only with the `dev-auth` feature.

    use axum::extract::{Query, State};
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Redirect, Response};
    use axum_extra::extract::cookie::SignedCookieJar;

    use super::AppState;
    use crate::session::{SESSION_COOKIE, SESSION_TTL, cookie};

    #[derive(serde::Deserialize)]
    pub struct DevQuery {
        user: String,
    }

    /// 1–32 of `A-Z a-z 0-9 _ . -`, and not `robot` in any case.
    pub fn valid_dev_user(s: &str) -> bool {
        (1..=32).contains(&s.len())
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            && !crate::supervisor::is_robot(s)
    }

    pub async fn login(State(state): State<AppState>, jar: SignedCookieJar, Query(q): Query<DevQuery>) -> Response {
        if !valid_dev_user(&q.user) {
            return (StatusCode::BAD_REQUEST, "user: 1 to 32 of A-Z a-z 0-9 _ . -, not robot").into_response();
        }
        let id = state.sessions.create(&q.user);
        (jar.add(cookie(SESSION_COOKIE, &id, SESSION_TTL.as_secs())), Redirect::to("/")).into_response()
    }
}
