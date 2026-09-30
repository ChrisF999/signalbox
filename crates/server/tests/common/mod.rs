//! A front running in process with dev login, real game processes, and a
//! temp data directory; plus WebSocket helpers.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use bot::net::{Conn, dev_login};
use protocol::{ClientFrame, LobbyMsg, LobbyReply, ServerFrame, ServerMsg};
use server::Running;
use server::config::Config;
use tokio::time::timeout;

pub const TWOBOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../game/tests/fixtures/twobox.json");
pub const GAME_BIN: &str = env!("CARGO_BIN_EXE_signalbox-game");

pub fn s(x: &str) -> String {
    x.to_string()
}

pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sbx-front-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A dev-auth config serving on a free local port, without OIDC.
pub fn dev_config(root: &Path, layouts_dir: PathBuf) -> Config {
    Config {
        addr: "127.0.0.1:0".parse().unwrap(),
        data_dir: root.join("data"),
        layouts_dir,
        public_url: s("http://127.0.0.1"),
        oidc: None,
        session_key: vec![7; 64],
        game_bin: PathBuf::from(GAME_BIN),
    }
}

pub struct Front {
    pub running: Running,
    pub base: String,
    pub root: PathBuf,
}

/// A front whose layouts directory holds each `(name, world JSON)`.
pub async fn front_with(name: &str, layouts: &[(&str, String)]) -> Front {
    let root = temp_dir(name);
    let dir = root.join("layouts");
    std::fs::create_dir_all(&dir).unwrap();
    for (layout, json) in layouts {
        std::fs::write(dir.join(format!("{layout}.json")), json).unwrap();
    }
    let running = server::start(dev_config(&root, dir)).await.unwrap();
    let base = running.base();
    Front { running, base, root }
}

/// A front offering the twobox layout.
pub async fn front(name: &str) -> Front {
    front_with(name, &[("twobox", std::fs::read_to_string(TWOBOX).unwrap())]).await
}

impl Front {
    pub fn saves(&self) -> PathBuf {
        self.root.join("data/saves")
    }

    /// A logged-in WebSocket for `user`.
    pub async fn connect(&self, user: &str) -> Conn {
        let cookie = dev_login(&self.base, user).await.unwrap();
        Conn::connect(&self.base, Some(&cookie)).await.unwrap()
    }
}

/// The next frame (10 s); `None` once the server closed the socket.
pub async fn next(c: &mut Conn) -> Option<ServerFrame> {
    timeout(Duration::from_secs(10), c.recv()).await.expect("no frame within 10 s").ok().flatten()
}

/// Frames until one matches `stop` (included).
pub async fn until(c: &mut Conn, stop: impl Fn(&ServerFrame) -> bool) -> Vec<ServerFrame> {
    let mut got = Vec::new();
    loop {
        let f = next(c).await.unwrap_or_else(|| panic!("closed; got {got:?}"));
        let done = stop(&f);
        got.push(f);
        if done {
            return got;
        }
    }
}

pub fn is_view(f: &ServerFrame) -> bool {
    matches!(f, ServerFrame::Game(ServerMsg::View(_)))
}

pub fn lobby(msg: LobbyMsg) -> ClientFrame {
    ClientFrame::Lobby(msg)
}

/// Create a game of `layout` and wait for its first view; returns its id.
pub async fn create(c: &mut Conn, layout: &str) -> String {
    c.send(&lobby(LobbyMsg::CreateGame { layout: s(layout), seed: Some(5), start: None })).await.unwrap();
    let got = until(c, is_view).await;
    match got.first() {
        Some(ServerFrame::Lobby(LobbyReply::Joined { game, .. })) => game.clone(),
        other => panic!("{other:?}"),
    }
}
