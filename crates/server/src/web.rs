//! HTTP routes (spec §8): the placeholder page, the WebSocket, and the
//! auth routes. Nothing but the login flow answers without a session.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRef, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::get;
use axum_extra::extract::cookie::{Cookie, Key, SignedCookieJar};
use protocol::{GameInfo, LayoutInfo, codes};

use crate::limit::RateLimit;
use crate::session::{SESSION_COOKIE, Sessions};
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
}

impl FromRef<AppState> for Key {
    fn from_ref(s: &AppState) -> Key {
        s.key.clone()
    }
}

pub fn router(state: AppState) -> Router {
    let r = Router::new().route("/", get(index)).route("/ws", get(ws)).route("/auth/logout", get(logout));
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

/// The placeholder page until the browser client (sub-project D).
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
         <p>The browser client is not built yet; bots play over <code>/ws</code>.</p>\n\
         <h2>Games</h2>\n<table><tr><th>Game</th><th>Layout</th><th>State</th><th>Areas</th><th>Error</th></tr>\n{}</table>\n\
         <h2>Layouts</h2>\n<p>{}</p>\n</body></html>\n",
        e(user),
        rows,
        layouts.join(", "),
    )
}

async fn index(State(state): State<AppState>, jar: SignedCookieJar) -> Response {
    let Some(user) = user_of(&state, &jar) else { return Redirect::to("/auth/login").into_response() };
    Html(index_page(&user, &state.sup.list_games(), &state.sup.layouts().infos())).into_response()
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

    /// 1–32 of `A-Z a-z 0-9 _ . -`.
    pub fn valid_dev_user(s: &str) -> bool {
        (1..=32).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    }

    pub async fn login(State(state): State<AppState>, jar: SignedCookieJar, Query(q): Query<DevQuery>) -> Response {
        if !valid_dev_user(&q.user) {
            return (StatusCode::BAD_REQUEST, "user: 1 to 32 of A-Z a-z 0-9 _ . -").into_response();
        }
        let id = state.sessions.create(&q.user);
        (jar.add(cookie(SESSION_COOKIE, &id, SESSION_TTL.as_secs())), Redirect::to("/")).into_response()
    }
}
