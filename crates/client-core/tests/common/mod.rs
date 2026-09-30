//! A front in miniature: the app's game frames go to an in-process twobox
//! `Game`, and its answers come back through a `MemTransport`.

#![allow(dead_code)]

use client_core::{App, MemHandle, MemTransport};
use game::{Game, GameMeta, Out};
use protocol::*;
use signalbox_core::world::World;

pub const TWOBOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../game/tests/fixtures/twobox.json");

pub fn s(x: &str) -> String {
    x.to_string()
}

/// An app whose first connection is open, its lobby requests taken.
pub fn open_app() -> (App, MemHandle) {
    let (t, h) = MemTransport::new();
    let mut app = App::new(Box::new(t), 0.0);
    h.open();
    app.tick(0.0);
    h.take_sent();
    (app, h)
}

pub struct Table {
    pub game: Game,
    pub app: App,
    pub h: MemHandle,
    pub me: String,
    pub now: f64,
}

impl Table {
    /// `me` has joined a twobox game (as the front would tell it) and, if
    /// given, claimed `area`.
    pub fn new(me: &str, area: Option<&str>) -> Table {
        let world = World::from_json(&std::fs::read_to_string(TWOBOX).unwrap()).unwrap();
        let game = Game::new(world, GameMeta { layout: s("twobox"), seed: 1 });
        let (app, h) = open_app();
        let mut t = Table { game, app, h, me: s(me), now: 0.0 };
        t.h.push(ServerFrame::Lobby(LobbyReply::Joined { game: s("g-test"), you: s(me) }));
        let out = t.game.connect(me);
        t.deliver(out);
        if let Some(a) = area {
            t.app.claim(a);
            t.pump();
        }
        t
    }

    /// The game's messages for me, to the app.
    pub fn deliver(&mut self, out: Vec<Out>) {
        for (p, m) in out {
            if p == self.me {
                self.h.push(ServerFrame::Game(m));
            }
        }
        self.app.tick(self.now);
    }

    /// The app's game messages to the game and the answers back, until quiet.
    pub fn pump(&mut self) {
        loop {
            let sent = self.h.take_sent();
            if sent.is_empty() {
                return;
            }
            for f in sent {
                if let ClientFrame::Game(m) = f {
                    let me = self.me.clone();
                    let out = self.game.handle(&me, m);
                    self.deliver(out);
                }
            }
        }
    }

    /// Run the game for `secs` of real time at 1x, flushing to the app.
    pub fn run(&mut self, secs: f64) {
        for _ in 0..(secs / 0.1).round() as u64 {
            self.now += 0.1;
            let mut out = self.game.advance(0.1);
            out.extend(self.game.flush());
            self.deliver(out);
            self.pump();
        }
    }

    pub fn layout(&self) -> &Layout {
        self.app.game().unwrap().layout().unwrap()
    }

    pub fn view(&self) -> &View {
        self.app.game().unwrap().view().unwrap()
    }

    pub fn log_lines(&self) -> Vec<(String, bool)> {
        self.app.game().unwrap().log().entries().map(|e| (e.text.clone(), e.alarm)).collect()
    }
}
