//! `Greedy` decides from a bot's own layout and view: the rules on a
//! hand-made screen, then Liverpool Street in process (no network) to show
//! its commands stay in the bot's area and the game stays safe.

use std::collections::BTreeMap;

use bot::Bot;
use bot::strategy::{Greedy, MAX_PER_DECISION, RETRY_S};
use game::{Game, GameMeta, Out};
use protocol::*;
use signalbox_core::world::World;

fn s(x: &str) -> String {
    x.to_string()
}

fn berth(name: &str, signal: Option<&str>, operable: bool) -> BerthInfo {
    BerthInfo { name: s(name), signal: signal.map(s), boundary: None, area: s("A"), operable }
}

fn route(entrance: &str, exit: &str, operable: bool) -> RouteInfo {
    RouteInfo {
        name: format!("{entrance}-{exit}"),
        entrance: s(entrance),
        exit: ExitName::Signal(s(exit)),
        automatic: false,
        operable,
    }
}

fn screen(berths: Vec<BerthInfo>, routes: Vec<RouteInfo>) -> Layout {
    Layout {
        title: s("test"),
        you: s("ann"),
        area: Some(s("A")),
        areas: vec![s("A"), s("B")],
        sections: vec![],
        segments: vec![],
        signals: vec![],
        points: vec![],
        berths,
        platforms: vec![],
        routes,
        geometry: None,
        box_prefix: String::new(),
        workstations: BTreeMap::new(),
        simplifier: vec![],
        display_headcodes: Default::default(),
    }
}

fn view(t: f64, reds: &[&str], berths: &[(&str, &str)]) -> View {
    View {
        seq: 1,
        sim_time: t,
        speed: 1,
        paused: false,
        vote: None,
        holders: BTreeMap::new(),
        score: Some(0),
        signals: reds.iter().map(|r| (s(r), Aspect::Red)).collect(),
        routes: BTreeMap::new(),
        points: BTreeMap::new(),
        sections: BTreeMap::new(),
        berths: berths.iter().map(|(b, h)| (s(b), s(h))).collect(),
        trains: BTreeMap::new(),
    }
}

fn entrances(cmds: &[PlayerCommand]) -> Vec<String> {
    cmds.iter()
        .map(|c| match c {
            PlayerCommand::SetRoute { entrance, exit: ExitName::Signal(x) } => format!("{entrance}-{x}"),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn greedy_routes_a_waiting_train_and_rotates_after_thirty_seconds() {
    let l = screen(vec![berth("B1", Some("S1"), true)], vec![route("S1", "X", true), route("S1", "Y", true), route("S2", "X", true)]);
    let mut g = Greedy::new();
    let v = view(100.0, &["S1"], &[("B1", "1A01")]);
    assert_eq!(entrances(&g.decide(&l, &v)), ["S1-X"]);
    assert!(g.decide(&l, &v).is_empty(), "no second attempt at once");
    let later = view(100.0 + RETRY_S - 0.1, &["S1"], &[("B1", "1A01")]);
    assert!(g.decide(&l, &later).is_empty());
    let later = view(100.0 + RETRY_S, &["S1"], &[("B1", "1A01")]);
    assert_eq!(entrances(&g.decide(&l, &later)), ["S1-Y"], "the next route after a failed attempt");
    let mut set = view(200.0, &["S1"], &[("B1", "1A01")]);
    set.routes.insert(s("S1-X"), RouteView { state: RouteState::Setting, auto_working: false });
    assert!(g.decide(&l, &set).is_empty(), "a route from it is already set");
}

#[test]
fn greedy_leaves_alone_what_is_not_waiting_or_not_its_own() {
    let mut g = Greedy::new();
    let l = screen(vec![berth("B1", Some("S1"), true)], vec![route("S1", "X", true)]);
    assert!(g.decide(&l, &view(0.0, &["S1"], &[])).is_empty(), "no headcode waiting");
    assert!(g.decide(&l, &view(0.0, &[], &[("B1", "1A01")])).is_empty(), "the signal is not at red");
    let fringe = screen(vec![berth("B1", Some("S1"), false)], vec![route("S1", "X", true)]);
    assert!(g.decide(&fringe, &view(0.0, &["S1"], &[("B1", "1A01")])).is_empty(), "a fringe berth");
    let theirs = screen(vec![berth("B1", Some("S1"), true)], vec![route("S1", "X", false)]);
    assert!(g.decide(&theirs, &view(0.0, &["S1"], &[("B1", "1A01")])).is_empty(), "no route it may set");
    let bare = screen(vec![berth("B1", None, true)], vec![]);
    assert!(g.decide(&bare, &view(0.0, &[], &[("B1", "1A01")])).is_empty(), "a berth at no signal");
}

#[test]
fn greedy_sends_at_most_two_commands_a_decision() {
    let names = ["S1", "S2", "S3"];
    let l = screen(
        names.iter().enumerate().map(|(i, n)| berth(&format!("B{i}"), Some(n), true)).collect(),
        names.iter().map(|n| route(n, "X", true)).collect(),
    );
    let v = view(0.0, &names, &[("B0", "1A01"), ("B1", "1A02"), ("B2", "1A03")]);
    let mut g = Greedy::new();
    assert_eq!(g.decide(&l, &v).len(), MAX_PER_DECISION);
    assert_eq!(entrances(&g.decide(&l, &v)), ["S3-X"], "the rest next time");
}

fn liverpool_json() -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    let ts2 = std::fs::read_to_string(format!("{dir}/../ts2-import/tests/data/liverpool-st.json")).unwrap();
    let text = std::fs::read_to_string(format!("{dir}/../../layouts/liverpool-st.areas.json")).unwrap();
    let mut w = ts2_import::convert(&ts2).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&text).unwrap()).unwrap();
    serde_json::to_string(&w).unwrap()
}

/// Deliver the game's messages to the bots; the bots' replies go back.
fn deliver(g: &mut Game, bots: &mut BTreeMap<String, Bot>, mut outs: Vec<Out>, not_your_area: &mut usize) {
    while !outs.is_empty() {
        let mut replies = Vec::new();
        for (name, msg) in outs {
            if matches!(msg, ServerMsg::Notice(Notice::NotYourArea { .. })) {
                *not_your_area += 1;
            }
            if let Some(b) = bots.get_mut(&name) {
                replies.extend(b.receive(msg).map(|r| (name.clone(), r)));
            }
        }
        outs = replies.into_iter().flat_map(|(n, m)| g.handle(&n, m)).collect();
    }
}

#[test]
fn greedy_bots_play_liverpool_street_in_their_own_areas() {
    let mut g = Game::new(World::from_json(&liverpool_json()).unwrap(), GameMeta { layout: s("liverpool-st"), seed: 7 });
    let seats = [("ann", "Liverpool Street"), ("hal", "Hackney & Bow")];
    let mut bots: BTreeMap<String, Bot> = BTreeMap::new();
    let mut greedy: BTreeMap<String, Greedy> = BTreeMap::new();
    let mut nya = 0;
    for (name, area) in seats {
        bots.insert(s(name), Bot::new());
        greedy.insert(s(name), Greedy::new());
        let outs = g.connect(name);
        deliver(&mut g, &mut bots, outs, &mut nya);
        let outs = g.handle(name, ClientMsg::Claim { area: s(area) });
        deliver(&mut g, &mut bots, outs, &mut nya);
    }
    for (name, _) in seats {
        let outs = g.handle(name, ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
        deliver(&mut g, &mut bots, outs, &mut nya);
    }
    assert_eq!(g.clock().speed, 8);
    // 20 sim minutes: a decision and a flush every 0.5 s of real time (40 ticks).
    while g.sim().tick() < 12_000 {
        for (name, _) in seats {
            let (l, v) = (bots[name].layout().unwrap().clone(), bots[name].view().unwrap().clone());
            for cmd in greedy.get_mut(name).unwrap().decide(&l, &v) {
                let outs = g.handle(name, ClientMsg::Command { cmd });
                deliver(&mut g, &mut bots, outs, &mut nya);
            }
        }
        let outs = g.advance(0.5);
        deliver(&mut g, &mut bots, outs, &mut nya);
        let outs = g.flush();
        deliver(&mut g, &mut bots, outs, &mut nya);
    }
    let st = g.stats().clone();
    assert_eq!((st.spads, st.collisions, st.invariant_violations), (0, 0, 0), "{st:?}");
    assert!(st.player_commands > 0, "the bots' commands reached the sim: {st:?}");
    assert_eq!(nya, 0, "Greedy only works its own area");
    for (name, _) in seats {
        assert_eq!(bots[name].view(), g.view_of(name).as_ref(), "{name}");
    }
    eprintln!("greedy, 20 sim minutes: {st:?}");
}
