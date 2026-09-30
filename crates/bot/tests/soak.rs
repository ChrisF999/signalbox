//! Two bots and the robot play Liverpool Street through `Game`, with a
//! spectator watching (spec §1 success criteria 1–3, in process; C2 repeats
//! it through the real front and game processes).
//!
//! Honest label: bot decisions come from the robot signaller's logic reading
//! the game's sim (`robot::commands(game.sim())`), filtered to the bot's area
//! and sent by name through `Game::handle`, so area checks, command logging
//! and notices are exercised. A strategy that reads only the bot's own view
//! is future work.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bot::Bot;
use game::areas::AreaMap;
use game::names::to_player_command;
use game::{Game, GameMeta, Out};
use protocol::{ClientMsg, Notice, PlayerCommand, Proposal, ServerMsg};
use signalbox_core::robot::{self, ROBOT_EVERY_TICKS};
use signalbox_core::world::World;

const SPEED: u8 = 8;
/// At 8x, 0.125 s of real time is exactly one robot period (10 ticks), so the
/// bots decide on the very state the game's robot sees (amendment 12).
const DT: f64 = 0.125;

fn liverpool_json() -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    let ts2 = std::fs::read_to_string(format!("{dir}/../ts2-import/tests/data/liverpool-st.json")).unwrap();
    let text = std::fs::read_to_string(format!("{dir}/../../layouts/liverpool-st.areas.json")).unwrap();
    let mut w = ts2_import::convert(&ts2).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&text).unwrap()).unwrap();
    serde_json::to_string(&w).unwrap()
}

fn meta() -> GameMeta {
    GameMeta { layout: "liverpool-st".into(), seed: 7 }
}

fn temp_save(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("signalbox-bot-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.sqlite"));
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }
    path
}

/// A game and its clients, wired together in process.
struct Table {
    game: Game,
    map: AreaMap,
    bots: BTreeMap<String, Bot>,
    periods: u64,
    handovers: usize,
    not_your_area: usize,
    /// Skip the bots' first decision: a resumed game already holds it queued.
    skip_decide: bool,
}

impl Table {
    fn new(game: Game) -> Table {
        let map = AreaMap::new(game.sim().world());
        Table { game, map, bots: BTreeMap::new(), periods: 0, handovers: 0, not_your_area: 0, skip_decide: false }
    }

    /// Hand the game's messages to the bots, and the bots' answers back.
    fn deliver(&mut self, mut outs: Vec<Out>) {
        while !outs.is_empty() {
            let mut replies: Vec<(String, ClientMsg)> = Vec::new();
            for (name, msg) in outs {
                match &msg {
                    ServerMsg::Notice(Notice::Handover { .. }) => self.handovers += 1,
                    ServerMsg::Notice(Notice::NotYourArea { .. }) => self.not_your_area += 1,
                    _ => {}
                }
                if let Some(bot) = self.bots.get_mut(&name) {
                    replies.extend(bot.receive(msg).map(|r| (name.clone(), r)));
                }
            }
            outs = replies.into_iter().flat_map(|(name, m)| self.game.handle(&name, m)).collect();
        }
    }

    fn join(&mut self, name: &str, area: Option<&str>) {
        self.bots.insert(name.to_string(), Bot::new());
        let outs = self.game.connect(name);
        self.deliver(outs);
        if let Some(a) = area {
            self.send(name, ClientMsg::Claim { area: a.to_string() });
        }
    }

    fn send(&mut self, name: &str, msg: ClientMsg) {
        let outs = self.game.handle(name, msg);
        self.deliver(outs);
    }

    /// Every bot holding an area votes for `proposal`.
    fn vote_all(&mut self, proposal: Proposal) {
        let holders: Vec<String> = self.bots.iter().filter(|(_, b)| b.area().is_some()).map(|(n, _)| n.clone()).collect();
        for h in holders {
            self.send(&h, ClientMsg::Vote { proposal });
        }
    }

    /// Each holding bot sends the robot's commands for its own area, by name.
    fn decide(&mut self) {
        let world = self.game.sim().world();
        let cmds = robot::commands(self.game.sim());
        let mut sends: Vec<(String, PlayerCommand)> = Vec::new();
        for (name, bot) in &self.bots {
            let Some(area) = bot.area().and_then(|a| world.net.area(a)) else { continue };
            for c in cmds.iter().filter(|c| self.map.subject(c) == Some(area)) {
                sends.push((name.clone(), to_player_command(world, c)));
            }
        }
        for (name, cmd) in sends {
            self.send(&name, ClientMsg::Command { cmd });
        }
    }

    /// One robot period; every second one flushes (4 Hz) and checks every
    /// client's delta-built view and layout against a fresh full one.
    fn period(&mut self) {
        assert_eq!(self.game.sim().tick() % ROBOT_EVERY_TICKS, 0, "periods start on robot ticks");
        if !std::mem::take(&mut self.skip_decide) {
            self.decide();
        }
        let outs = self.game.advance(DT);
        self.deliver(outs);
        self.periods += 1;
        if self.periods % 2 == 0 {
            let outs = self.game.flush();
            self.deliver(outs);
            for (name, bot) in &self.bots {
                assert_eq!(bot.view(), self.game.view_of(name).as_ref(), "{name}'s view at tick {}", self.game.sim().tick());
                assert_eq!(bot.layout(), self.game.layout_of(name).as_ref(), "{name}'s layout");
            }
        }
    }

    fn run_to(&mut self, tick: u64) {
        while self.game.sim().tick() < tick {
            self.period();
        }
    }
}

/// Two bots hold Liverpool Street and Hackney & Bow, a spectator watches,
/// and the robot keeps Bethnal Green.
fn seat(game: Game) -> Table {
    let mut t = Table::new(game);
    t.join("ann", Some("Liverpool Street"));
    t.join("hal", Some("Hackney & Bow"));
    t.join("sam", None);
    t
}

fn play_liverpool(minutes: u64) {
    let mut t = seat(Game::new(World::from_json(&liverpool_json()).unwrap(), meta()));
    t.vote_all(Proposal::Speed { x: SPEED });
    assert_eq!(t.game.clock().speed, SPEED);
    t.run_to(minutes * 600);
    let s = t.game.stats().clone();
    assert_eq!((s.spads, s.collisions, s.invariant_violations), (0, 0, 0), "{s:?}");
    assert!(s.player_commands > 0 && s.robot_commands > 0, "{s:?}");
    assert_eq!(t.not_your_area, 0, "bots only work their own areas");
    assert!(t.handovers >= 1, "no handover notice in {minutes} minutes");
    assert_eq!(t.game.holder("Bethnal Green"), None);
    assert!(t.bots.values().all(|b| b.resyncs() == 0), "in process, no delta is ever lost");
}

/// The first handovers into the bots' areas come at about 13 and 15 sim
/// minutes (prototype run), so 20 minutes sees at least one.
#[test]
fn liverpool_street_twenty_minutes_with_two_bots() {
    play_liverpool(20);
}

/// Three sim-hours at 8x. Slow in debug builds: run with
/// `scripts/cargo test --release -p signalbox-bot --test soak -- --ignored`.
#[test]
#[ignore]
fn liverpool_street_three_hours_with_two_bots() {
    play_liverpool(180);
}

/// Spec §1 success criterion 2 on the real layout: save at 2 minutes, drop
/// at 4, resume, and at 6 minutes the state matches an uninterrupted run.
#[test]
fn liverpool_street_save_and_resume_match_an_uninterrupted_run() {
    let json = liverpool_json();
    let mut reference = seat(Game::new(World::from_json(&json).unwrap(), meta()));
    reference.vote_all(Proposal::Speed { x: SPEED });
    reference.run_to(3600);

    let path = temp_save("liverpool");
    let mut saved = seat(Game::create(&path, &json, meta()).unwrap());
    saved.vote_all(Proposal::Speed { x: SPEED });
    saved.run_to(1200);
    assert!(saved.game.save_now().is_empty());
    saved.run_to(2400);
    drop(saved);

    let game = Game::resume(&path).unwrap();
    let at = game.sim().tick();
    assert!((1200..=2400).contains(&at), "resumed at {at}");
    let queued = !game.sim().snapshot().queue.is_empty();
    let mut resumed = seat(game);
    resumed.skip_decide = queued;
    resumed.vote_all(Proposal::Resume);
    resumed.vote_all(Proposal::Speed { x: SPEED });
    assert!(!resumed.game.clock().paused);
    resumed.run_to(3600);
    assert_eq!(resumed.game.sim().tick(), reference.game.sim().tick());
    assert_eq!(resumed.game.sim().state_hash(), reference.game.sim().state_hash());
}
