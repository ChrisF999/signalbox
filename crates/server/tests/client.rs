//! The browser client's logic end to end (spec D1 §6): a real
//! `client_core::App` over a WebSocket to the real front with dev login and
//! real game processes. The transport here is test-only (tokio-tungstenite
//! through `bot::net::Conn`); the browser's is web-sys, the app the same.

mod common;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bot::net::{Conn, NetError, dev_login};
use client_core::{App, ConnState, Link, Target, Transport};
use common::*;
use protocol::*;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

const WAIT: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Shared {
    state: ConnState,
    inbox: VecDeque<String>,
    /// Bumped by every connect; a replaced socket's task stops touching state.
    generation: u64,
    /// Every frame the app sent, in order.
    sent: Vec<ClientFrame>,
    task: Option<AbortHandle>,
}

/// `Transport` over `bot::net::Conn` on a tokio task.
struct NetTransport {
    base: String,
    cookie: Option<String>,
    shared: Arc<Mutex<Shared>>,
    out: Option<mpsc::UnboundedSender<String>>,
}

/// The test's handle on the transport the app owns.
#[derive(Clone)]
struct NetHandle(Arc<Mutex<Shared>>);

impl NetTransport {
    fn new(base: &str, cookie: Option<String>) -> (NetTransport, NetHandle) {
        let shared = Arc::new(Mutex::new(Shared::default()));
        (NetTransport { base: base.to_string(), cookie, shared: shared.clone(), out: None }, NetHandle(shared))
    }
}

fn set(shared: &Mutex<Shared>, generation: u64, state: ConnState) {
    let mut s = shared.lock().unwrap();
    if s.generation == generation {
        s.state = state;
    }
}

impl Transport for NetTransport {
    fn connect(&mut self) {
        let generation = {
            let mut s = self.shared.lock().unwrap();
            if let Some(t) = s.task.take() {
                t.abort();
            }
            s.generation += 1;
            s.state = ConnState::Connecting;
            s.inbox.clear();
            s.generation
        };
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        self.out = Some(tx);
        let (base, cookie, shared) = (self.base.clone(), self.cookie.clone(), self.shared.clone());
        let task = tokio::spawn(async move {
            let mut conn = match Conn::connect(&base, cookie.as_deref()).await {
                Ok(c) => c,
                Err(NetError::Status(401)) => return set(&shared, generation, ConnState::Unauthorized),
                Err(_) => return set(&shared, generation, ConnState::Closed),
            };
            set(&shared, generation, ConnState::Open);
            loop {
                tokio::select! {
                    f = conn.recv() => match f {
                        Ok(Some(frame)) => {
                            let mut s = shared.lock().unwrap();
                            if s.generation == generation {
                                s.inbox.push_back(frame.to_json());
                            }
                        }
                        _ => break,
                    },
                    t = rx.recv() => match t {
                        Some(text) => {
                            if conn.send_text(text).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    },
                }
            }
            set(&shared, generation, ConnState::Closed);
        });
        self.shared.lock().unwrap().task = Some(task.abort_handle());
    }

    fn state(&self) -> ConnState {
        self.shared.lock().unwrap().state
    }

    fn send(&mut self, text: String) {
        if self.state() != ConnState::Open {
            return;
        }
        if let Ok(f) = ClientFrame::from_json(&text) {
            self.shared.lock().unwrap().sent.push(f);
        }
        if let Some(out) = &self.out {
            let _ = out.send(text);
        }
    }

    fn poll(&mut self) -> Vec<String> {
        self.shared.lock().unwrap().inbox.drain(..).collect()
    }
}

impl NetHandle {
    /// The network goes away under the client (the TCP connection drops).
    /// The generation moves on too, so the aborted task can no longer touch
    /// the state or the inbox even if it is mid-poll on another thread.
    fn drop_connection(&self) {
        let mut s = self.0.lock().unwrap();
        if let Some(t) = s.task.take() {
            t.abort();
        }
        s.generation += 1;
        s.state = ConnState::Closed;
    }

    fn take_sent(&self) -> Vec<ClientFrame> {
        std::mem::take(&mut self.0.lock().unwrap().sent)
    }
}

/// Tick the app every 20 ms on a real clock until `done` holds.
async fn drive(app: &mut App, clock: Instant, what: &str, mut done: impl FnMut(&App) -> bool) {
    let deadline = Instant::now() + WAIT;
    loop {
        app.tick(clock.elapsed().as_secs_f64());
        if done(app) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn logged_in(f: &Front, user: &str) -> (App, NetHandle, Instant) {
    let cookie = dev_login(&f.base, user).await.unwrap();
    let (t, h) = NetTransport::new(&f.base, Some(cookie));
    let clock = Instant::now();
    (App::new(Box::new(t), 0.0), h, clock)
}

fn liverpool_json() -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    let ts2 = std::fs::read_to_string(format!("{dir}/../ts2-import/tests/data/liverpool-st.json")).unwrap();
    let text = std::fs::read_to_string(format!("{dir}/../../layouts/liverpool-st.areas.json")).unwrap();
    let mut w = ts2_import::convert(&ts2).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&text).unwrap()).unwrap();
    serde_json::to_string(&w).unwrap()
}

/// Spec D1 §6: log in, create Liverpool Street, claim it, click an
/// entrance then its exit, and see the route set in the view.
#[tokio::test]
async fn a_client_sets_a_route_on_liverpool_street() {
    let f = front_with("client-lst", &[("liverpool-st", liverpool_json())]).await;
    let (mut app, _h, clock) = logged_in(&f, "ann").await;
    drive(&mut app, clock, "the lobby", |a| a.layouts().iter().any(|l| l.name == "liverpool-st")).await;
    app.create_game("liverpool-st", Some(7), None);
    drive(&mut app, clock, "the first view", |a| a.game().is_some_and(|g| g.view().is_some())).await;
    app.claim("Liverpool Street");
    drive(&mut app, clock, "the area", |a| a.game().and_then(|g| g.area()) == Some("Liverpool Street")).await;
    let layout = app.game().unwrap().layout().unwrap().clone();
    assert!(layout.geometry.as_ref().is_some_and(|geo| !geo.lines.is_empty()), "the diagram came with the layout");
    // The view is sent separately from the layout: wait for it, not assert on it.
    drive(&mut app, clock, "trains due at 07:00 to be listed", |a| {
        a.game().and_then(|g| g.view()).is_some_and(|v| !v.trains.is_empty())
    })
    .await;
    let candidates: Vec<RouteInfo> = layout.routes.iter().filter(|r| r.operable && !r.automatic).cloned().collect();
    let mut set = None;
    for r in candidates.iter().take(8) {
        let refusals = app.game().unwrap().log().entries().filter(|e| e.text.starts_with("Refused")).count();
        app.click(&Target::Signal(r.entrance.clone()));
        assert_eq!(app.game().unwrap().selected(), Some(r.entrance.as_str()));
        assert!(app.valid_exits().contains(&r.exit));
        app.click(&match &r.exit {
            ExitName::Signal(s) => Target::Signal(s.clone()),
            ExitName::Node(n) => Target::Exit(n.clone()),
        });
        drive(&mut app, clock, "the route or a refusal", |a| {
            let g = a.game().unwrap();
            g.view().unwrap().routes.contains_key(&r.name) || g.log().entries().filter(|e| e.text.starts_with("Refused")).count() > refusals
        })
        .await;
        if app.game().unwrap().view().unwrap().routes.contains_key(&r.name) {
            set = Some(r.name.clone());
            break;
        }
    }
    let name = set.expect("one of the first routes could be set");
    drive(&mut app, clock, "the route to lock", |a| {
        a.game().unwrap().view().unwrap().routes.get(&name).is_some_and(|rv| rv.state == RouteState::Locked)
    })
    .await;
    f.running.stop().await;
}

/// Spec D1 §5: a dropped connection comes back by itself with exactly one
/// `join`, no `resync`, and the area is still yours.
#[tokio::test]
async fn a_dropped_connection_rejoins_with_one_join() {
    let f = front("client-drop").await;
    let (mut app, h, clock) = logged_in(&f, "ann").await;
    drive(&mut app, clock, "the lobby", |a| !a.layouts().is_empty()).await;
    app.create_game("twobox", Some(1), None);
    drive(&mut app, clock, "the first view", |a| a.game().is_some_and(|g| g.view().is_some())).await;
    app.claim("West");
    drive(&mut app, clock, "the area", |a| a.game().and_then(|g| g.area()) == Some("West")).await;
    let game = app.game().unwrap().id.clone();
    let layouts_before = app.game().unwrap().layout_gen();
    h.take_sent();
    h.drop_connection();
    drive(&mut app, clock, "the loss to show", |a| matches!(a.link(), Link::Waiting { .. })).await;
    assert!(app.banner().unwrap().starts_with("Connection lost"));
    drive(&mut app, clock, "the rejoin's layout", |a| a.link() == Link::Open && a.game().is_some_and(|g| g.layout_gen() > layouts_before))
        .await;
    assert_eq!(h.take_sent(), [ClientFrame::Lobby(LobbyMsg::Join { game })], "one join, no resync");
    let g = app.game().unwrap();
    assert_eq!((g.area(), g.resyncs()), (Some("West"), 0));
    f.running.stop().await;
}

/// Spec D1 §5: without a session the socket is refused and the client asks
/// the shell to log in, rather than retrying.
#[tokio::test]
async fn without_a_session_the_client_asks_for_a_login() {
    let f = front("client-401").await;
    let (t, _h) = NetTransport::new(&f.base, None);
    let clock = Instant::now();
    let mut app = App::new(Box::new(t), 0.0);
    drive(&mut app, clock, "the refusal", |a| a.wants_login()).await;
    f.running.stop().await;
}
