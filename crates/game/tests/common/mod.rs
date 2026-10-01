#![allow(dead_code)]

use std::collections::BTreeMap;

use signalbox_core::ids::AreaId;
use signalbox_core::world::World;

pub const TWOBOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/twobox.json");

/// A drawing of twobox, for the world's `layout` field.
pub const TWOBOX_LAYOUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/twobox-layout.json");

pub fn twobox_json() -> String {
    std::fs::read_to_string(TWOBOX).unwrap()
}

pub fn twobox() -> World {
    World::from_json(&twobox_json()).unwrap()
}

pub fn area(w: &World, name: &str) -> AreaId {
    w.net.area(name).unwrap_or_else(|| panic!("no area {name}"))
}

pub fn map<V: Clone>(pairs: &[(&str, V)]) -> BTreeMap<String, V> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

/// Liverpool Street converted from TS2 and split by its shipped area file.
pub fn liverpool_json() -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    let ts2 = std::fs::read_to_string(format!("{dir}/../ts2-import/tests/data/liverpool-st.json")).unwrap();
    let text = std::fs::read_to_string(format!("{dir}/../../layouts/liverpool-st.areas.json")).unwrap();
    let mut w = ts2_import::convert(&ts2).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&text).unwrap()).unwrap();
    serde_json::to_string(&w).unwrap()
}

/// Drain converted from TS2, split by its shipped area file, with the
/// synthetic Waterloo & City WTT as its timetable (the real WTT's shape:
/// trains enter from the depot from the timetable's start onwards).
pub fn drain_wtt_json() -> String {
    use ts2_import::wtt;
    let dir = env!("CARGO_MANIFEST_DIR");
    let ts2 = std::fs::read_to_string(format!("{dir}/../ts2-import/tests/data/drain.json")).unwrap();
    let areas = std::fs::read_to_string(format!("{dir}/../../layouts/drain.areas.json")).unwrap();
    let bbox = std::fs::read_to_string(format!("{dir}/../ts2-import/tests/data/wtt-synthetic.bbox.html")).unwrap();
    let mut w = ts2_import::convert(&ts2).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&areas).unwrap()).unwrap();
    let day = wtt::on_day(&wtt::parse(&bbox).unwrap(), wtt::DAY).unwrap();
    wtt::apply(&mut w, &day).unwrap();
    serde_json::to_string(&w).unwrap()
}

use game::areas::AreaMap;
use game::names::to_player_command;
use game::{Game, GameMeta, Out};
use protocol::{ClientMsg, ExitName, Layout, Notice, PlayerCommand, ServerMsg, View};
use signalbox_core::robot;

pub fn meta() -> GameMeta {
    GameMeta { layout: "twobox".into(), seed: 1 }
}

pub fn game() -> Game {
    Game::new(twobox(), meta())
}

/// Connect `player` and, if given, claim `area`; everything sent back.
pub fn join(g: &mut Game, player: &str, area: Option<&str>) -> Vec<Out> {
    let mut out = g.connect(player);
    if let Some(a) = area {
        out.extend(g.handle(player, ClientMsg::Claim { area: a.to_string() }));
    }
    out
}

pub fn send(g: &mut Game, player: &str, msg: ClientMsg) -> Vec<Out> {
    g.handle(player, msg)
}

pub fn command(g: &mut Game, player: &str, cmd: PlayerCommand) -> Vec<Out> {
    g.handle(player, ClientMsg::Command { cmd })
}

pub fn set_route(entrance: &str, exit: ExitName) -> PlayerCommand {
    PlayerCommand::SetRoute { entrance: entrance.to_string(), exit }
}

pub fn notices(outs: &[Out], player: &str) -> Vec<Notice> {
    outs.iter()
        .filter(|(p, _)| p == player)
        .filter_map(|(_, m)| match m {
            ServerMsg::Notice(n) => Some(n.clone()),
            _ => None,
        })
        .collect()
}

pub fn error_codes(outs: &[Out], player: &str) -> Vec<String> {
    notices(outs, player)
        .into_iter()
        .filter_map(|n| match n {
            Notice::Error { code, .. } => Some(code),
            _ => None,
        })
        .collect()
}

/// Advance `real_s` seconds of real time in steps of `dt`.
pub fn run(g: &mut Game, real_s: f64, dt: f64) -> Vec<Out> {
    let mut out = Vec::new();
    for _ in 0..(real_s / dt).round() as u64 {
        out.extend(g.advance(dt));
    }
    out
}

/// At 1x, advance one tick at a time until the sim reaches `tick`.
pub fn run_to_tick(g: &mut Game, tick: u64) -> Vec<Out> {
    assert_eq!(g.clock().speed, 1, "run_to_tick steps one tick per 0.1 s");
    assert!(!g.clock().paused, "run_to_tick on a paused game would never return");
    let mut out = Vec::new();
    while g.sim().tick() < tick {
        out.extend(g.advance(0.1));
    }
    out
}

/// A minimal client: the layout and view it has been sent, deltas applied.
#[derive(Debug, Default)]
pub struct Client {
    pub layout: Option<Layout>,
    pub view: Option<View>,
}

impl Client {
    pub fn take(&mut self, outs: &[Out], me: &str) {
        for (p, m) in outs {
            if p != me {
                continue;
            }
            match m {
                ServerMsg::Layout(l) => self.layout = Some(l.clone()),
                ServerMsg::View(v) => self.view = Some(v.clone()),
                ServerMsg::Delta(d) => self.view.as_mut().expect("a view before any delta").apply(d).expect("deltas in order"),
                ServerMsg::Notice(_) | ServerMsg::Lesson(_) => {}
            }
        }
    }
}

/// Play `player`'s area as the robot would, sending its commands by name.
pub fn play_as_robot(g: &mut Game, player: &str) -> Vec<Out> {
    let w = g.sim().world();
    let Some(a) = g.area_of(player).and_then(|name| w.net.area(name)) else { return vec![] };
    let map = AreaMap::new(w);
    let cmds: Vec<PlayerCommand> = robot::commands(g.sim())
        .iter()
        .filter(|c| map.subject(c) == Some(a))
        .map(|c| to_player_command(w, c))
        .collect();
    cmds.into_iter().flat_map(|cmd| g.handle(player, ClientMsg::Command { cmd })).collect()
}
