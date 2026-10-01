mod common;

use common::*;
use signalbox_core::events::Command;
use signalbox_core::robot::{choose_route, commands, soak};
use signalbox_core::routes::Exit;
use signalbox_core::sim::Sim;

#[test]
fn robot_picks_the_route_to_the_booked_platform() {
    let mut sim = Sim::new(world("terminus"), 1);
    sim.step();
    let w = sim.world().clone();
    let t = &sim.trains()[0];
    assert_eq!(choose_route(&w, t, sig(&w, "S1")), Some(route(&w, "S1-E1")));
}

#[test]
fn robot_finds_a_multi_route_path_to_the_exit() {
    let mut sim = Sim::new(world("plain_line"), 1);
    sim.step();
    let w = sim.world().clone();
    assert_eq!(choose_route(&w, &sim.trains()[0], sig(&w, "S1")), Some(route(&w, "S1-S2")));
}

fn assert_clean(name: &str, secs: f64, exited: usize, stabled: usize) {
    let mut sim = Sim::new(world(name), 42);
    let r = soak(&mut sim, secs);
    assert_eq!(r.spads, 0, "{name}: {r:?}");
    assert_eq!(r.collisions, 0, "{name}: {r:?}");
    assert_eq!(r.invariant_violations, 0, "{name}: {r:?}");
    assert!(r.still_running.is_empty(), "{name}: {r:?}");
    assert_eq!(r.waiting_to_enter, 0, "{name}: {r:?}");
    assert_eq!(r.exited, exited, "{name}: {r:?}");
    assert_eq!(r.stabled, stabled, "{name}: {r:?}");
}

#[test]
fn soak_plain_line() {
    assert_clean("plain_line", 600.0, 1, 0);
}

#[test]
fn soak_terminus() {
    assert_clean("terminus", 3600.0, 1, 1);
}

#[test]
fn soak_junction() {
    assert_clean("junction", 3600.0, 4, 0);
}

/// A train standing in the platform of its current call has that call as
/// `next_call` until it departs; the robot must aim past it, or it never
/// asks for the route that lets the train leave.
#[test]
fn robot_aims_past_the_call_a_dwelling_train_stands_at() {
    let w = load_with("terminus", |v| {
        v["entries"][0] = serde_json::json!({"service": "1A01", "at": {"segment": "p1", "offset_m": 2, "direction": "down"}, "time": "06:00"});
        v["services"][0]["calls"] = serde_json::json!([{"place": "TRM", "platform": "1", "dep": "06:08"}]);
        v["services"][0]["end"] = serde_json::json!({"kind": "exit"});
    })
    .unwrap();
    let mut sim = Sim::new(w, 1);
    for _ in 0..5 {
        sim.step();
    }
    let w = sim.world().clone();
    let t = &sim.trains()[0];
    assert!(t.dwell.is_some() && t.next_call == 0, "{t:?}");
    assert_eq!(choose_route(&w, t, sig(&w, "S3")), Some(route(&w, "S3-W")));
}

/// Three one-way lines. X's and Y's lines cross on the flat in section TS1;
/// X's line and line O cross in TS2. A route set on line O (no train) holds
/// TS2, so X's route cannot be set; the robot must not let that doomed
/// request hold TS1 back from Y, or neither train ever moves.
#[test]
fn robot_does_not_reserve_track_for_a_route_the_interlocking_would_reject() {
    let line = |l: &str, secs: [&str; 3]| {
        serde_json::json!([
            {"name": format!("a{l}"), "from": format!("W{l}"), "to": format!("J{l}1"), "length_m": 1000, "line_speed_kmh": 100, "section": secs[0]},
            {"name": format!("b{l}"), "from": format!("J{l}1"), "to": format!("J{l}2"), "length_m": 100, "line_speed_kmh": 100, "section": secs[1]},
            {"name": format!("c{l}"), "from": format!("J{l}2"), "to": format!("E{l}"), "length_m": 100, "line_speed_kmh": 100, "section": secs[2]},
        ])
    };
    let mut segments = Vec::new();
    let mut nodes = Vec::new();
    for (l, secs) in [("X", ["TAX", "TS1", "TS2"]), ("Y", ["TAY", "TS1", "TCY"]), ("O", ["TAO", "TS2", "TCO"])] {
        segments.extend(line(l, secs).as_array().unwrap().clone());
        for (n, k) in [("W", "boundary"), ("J1", "joint"), ("J2", "joint"), ("E", "boundary")] {
            let name = if n.starts_with('J') { format!("J{l}{}", &n[1..]) } else { format!("{n}{l}") };
            nodes.push(serde_json::json!({"name": name, "kind": k}));
        }
    }
    let sections: Vec<_> = ["TAX", "TAY", "TAO", "TS1", "TS2", "TCY", "TCO"].iter().map(|s| serde_json::json!({"name": s, "area": "A"})).collect();
    let signal = |l: &str| serde_json::json!({"name": format!("S{l}"), "area": "A", "segment": format!("a{l}"), "offset_m": 1000, "direction": "up", "aspects": 3});
    let w = serde_json::json!({
        "schema": 1, "areas": [{"name": "A"}], "sections": sections, "nodes": nodes, "segments": segments,
        "signals": [signal("X"), signal("Y"), signal("O")],
        "routes": [
            {"entrance": "SX", "exit": {"kind": "node", "name": "EX"}, "path": ["TS1", "TS2"]},
            {"entrance": "SY", "exit": {"kind": "node", "name": "EY"}, "path": ["TS1", "TCY"]},
            {"entrance": "SO", "exit": {"kind": "node", "name": "EO"}, "path": ["TS2", "TCO"]},
        ],
        "train_types": [{"code": "EMU", "max_speed_kmh": 120, "accel": 0.8, "service_brake": 0.7, "emergency_brake": 1.2, "length_m": 100}],
        "services": [{"headcode": "1X01", "train_type": "EMU"}, {"headcode": "1Y01", "train_type": "EMU"}],
        "entries": [{"service": "1X01", "boundary": "WX", "time": "06:00"}, {"service": "1Y01", "boundary": "WY", "time": "06:00"}],
        "options": {"start_time": "06:00", "entry_delay_s": [0, 0]},
    });
    let w = signalbox_core::world::World::from_json(&w.to_string()).unwrap();
    let mut sim = Sim::new(w, 1);
    sim.submit(Command::SetRoute { entrance: sig(sim.world(), "SO"), exit: Exit::Node(node(sim.world(), "EO")) });
    sim.step();
    sim.step();
    assert_eq!(sim.trains().len(), 2);
    let w = sim.world().clone();
    assert_eq!(commands(&sim), vec![Command::SetRoute { entrance: sig(&w, "SY"), exit: Exit::Node(node(&w, "EY")) }]);
}

/// Line P crosses line Q on the flat (section TX), and signal SB stands only
/// 50 m past the crossing, so a train waiting at SB would stand across it.
/// `blocker`: a third train stands on P beyond SB.
fn crossing(blocker: bool) -> Sim {
    let seg = |name: &str, from: &str, to: &str, len: u32, sec: &str| {
        serde_json::json!({"name": name, "from": from, "to": to, "length_m": len, "line_speed_kmh": 100, "section": sec})
    };
    let nodes: Vec<_> = [("WP", "boundary"), ("P1", "joint"), ("P2", "joint"), ("P3", "joint"), ("EP", "boundary"),
        ("WQ", "boundary"), ("Q1", "joint"), ("Q2", "joint"), ("EQ", "boundary")]
        .iter()
        .map(|(n, k)| serde_json::json!({"name": n, "kind": k}))
        .collect();
    let sections: Vec<_> =
        ["TPA", "TX", "TPB", "TPC", "TQA", "TQB"].iter().map(|s| serde_json::json!({"name": s, "area": "A"})).collect();
    let signal = |name: &str, segment: &str, at: u32| {
        serde_json::json!({"name": name, "area": "A", "segment": segment, "offset_m": at, "direction": "up", "aspects": 3})
    };
    let mut services = vec![serde_json::json!({"headcode": "1X01", "train_type": "EMU"}), serde_json::json!({"headcode": "1Y01", "train_type": "EMU"})];
    let mut entries = vec![
        serde_json::json!({"service": "1X01", "boundary": "WP", "time": "06:00"}),
        serde_json::json!({"service": "1Y01", "boundary": "WQ", "time": "06:00"}),
    ];
    if blocker {
        services.push(serde_json::json!({"headcode": "1Z01", "train_type": "EMU"}));
        entries.push(serde_json::json!({"service": "1Z01", "at": {"segment": "pc", "offset_m": 500, "direction": "up"}, "time": "06:00"}));
    }
    let w = serde_json::json!({
        "schema": 1, "areas": [{"name": "A"}], "sections": sections, "nodes": nodes,
        "segments": [
            seg("pa", "WP", "P1", 1000, "TPA"), seg("px", "P1", "P2", 50, "TX"), seg("pb", "P2", "P3", 50, "TPB"),
            seg("pc", "P3", "EP", 1000, "TPC"), seg("qa", "WQ", "Q1", 1000, "TQA"), seg("qx", "Q1", "Q2", 50, "TX"),
            seg("qb", "Q2", "EQ", 1000, "TQB"),
        ],
        "signals": [signal("SA", "pa", 1000), signal("SB", "pb", 50), signal("SQ", "qa", 1000)],
        "routes": [
            {"entrance": "SA", "exit": {"kind": "signal", "name": "SB"}, "path": ["TX", "TPB"]},
            {"entrance": "SB", "exit": {"kind": "node", "name": "EP"}, "path": ["TPC"]},
            {"entrance": "SQ", "exit": {"kind": "node", "name": "EQ"}, "path": ["TX", "TQB"]},
        ],
        "train_types": [{"code": "EMU", "max_speed_kmh": 120, "accel": 0.8, "service_brake": 0.7, "emergency_brake": 1.2, "length_m": 100}],
        "services": services, "entries": entries,
        "options": {"start_time": "06:00", "entry_delay_s": [0, 0]},
    });
    let mut sim = Sim::new(signalbox_core::world::World::from_json(&w.to_string()).unwrap(), 1);
    sim.step();
    sim.step();
    assert_eq!(sim.trains().len(), if blocker { 3 } else { 2 });
    sim
}

fn set(w: &signalbox_core::world::World, entrance: &str, exit: Exit) -> Command {
    Command::SetRoute { entrance: sig(w, entrance), exit }
}

/// A train is only routed to a signal where it would stand across a junction
/// when the road beyond that signal can be set too; otherwise it could stand
/// there blocking the train that the road beyond is waiting for.
#[test]
fn robot_routes_a_train_over_a_junction_only_with_the_road_beyond() {
    let sim = crossing(true);
    let w = sim.world().clone();
    assert_eq!(commands(&sim), vec![set(&w, "SQ", Exit::Node(node(&w, "EQ")))]);

    let sim = crossing(false);
    let w = sim.world().clone();
    assert_eq!(
        commands(&sim),
        vec![set(&w, "SA", Exit::Signal(sig(&w, "SB"))), set(&w, "SB", Exit::Node(node(&w, "EP")))]
    );
}

/// A train standing in a platform until its booked departure does not need
/// its road yet: set early, the road would hold the station throat against
/// arrivals for the whole wait.
#[test]
fn robot_sets_a_departure_road_only_shortly_before_departure() {
    let w = load_with("terminus", |v| {
        v["entries"][0] = serde_json::json!({"service": "1A01", "at": {"segment": "p1", "offset_m": 2, "direction": "down"}, "time": "06:00"});
        v["services"][0]["calls"] = serde_json::json!([{"place": "TRM", "platform": "1", "dep": "06:08"}]);
        v["services"][0]["end"] = serde_json::json!({"kind": "exit"});
    })
    .unwrap();
    let mut sim = Sim::new(w, 1);
    sim.run_for(1.0);
    assert!(sim.trains()[0].dwell.is_some());
    assert_eq!(commands(&sim), vec![]);
    sim.run_for(7.0 * 60.0 + 28.0);
    assert_eq!(commands(&sim), vec![]);
    sim.run_for(2.0);
    let w = sim.world().clone();
    assert_eq!(commands(&sim), vec![set(&w, "S3", Exit::Node(node(&w, "W")))]);
}

/// The 5O02 case in miniature. 1S01 passes MID (platform 1) and stops in
/// END platform S. Both routes from S1 run through MID 1, then points Q
/// split them: to SF (only END F beyond) or to SS (END S beyond). Taking
/// the first route that reaches MID strands the train on the fast line
/// and in the wrong platform; the robot must plan the whole journey.
fn split_after_platform() -> Sim {
    let seg = |name: &str, from: &str, to: &str, len: u32, sec: &str| {
        serde_json::json!({"name": name, "from": from, "to": to, "length_m": len, "line_speed_kmh": 60, "section": sec})
    };
    let signal = |name: &str, segment: &str, at: u32| {
        serde_json::json!({"name": name, "area": "A", "segment": segment, "offset_m": at, "direction": "up", "aspects": 3})
    };
    let nodes: Vec<_> = [("W", "boundary"), ("J1", "joint"), ("J2", "joint"), ("F1", "joint"), ("S1n", "joint"), ("EF", "buffer_stop"), ("ES", "buffer_stop")]
        .iter()
        .map(|(n, k)| serde_json::json!({"name": n, "kind": k}))
        .chain([serde_json::json!({"name": "Q", "kind": "points", "toe": "q", "normal": "f1", "reverse": "s1"})])
        .collect();
    let sections: Vec<_> = ["TA", "TM", "TQ", "TF", "TS"].iter().map(|s| serde_json::json!({"name": s, "area": "A"})).collect();
    let w = serde_json::json!({
        "schema": 1, "areas": [{"name": "A"}], "sections": sections, "nodes": nodes,
        "segments": [
            seg("a", "W", "J1", 1000, "TA"), seg("m", "J1", "J2", 200, "TM"), seg("q", "J2", "Q", 50, "TQ"),
            seg("f1", "Q", "F1", 30, "TQ"), seg("s1", "Q", "S1n", 30, "TQ"),
            seg("fp", "F1", "EF", 250, "TF"), seg("sp", "S1n", "ES", 250, "TS"),
        ],
        "signals": [signal("S1", "a", 1000), signal("SF", "f1", 30), signal("SS", "s1", 30)],
        "berths": [{"name": "BW", "boundary": "W"}],
        "platforms": [
            {"place": "MID", "platform": "1", "segment": "m", "from_m": 0, "to_m": 200},
            {"place": "END", "platform": "F", "segment": "fp", "from_m": 0, "to_m": 250},
            {"place": "END", "platform": "S", "segment": "sp", "from_m": 0, "to_m": 250},
        ],
        "routes": [
            {"entrance": "S1", "exit": {"kind": "signal", "name": "SF"}, "path": ["TM", "TQ"], "points": [{"points": "Q", "position": "normal"}]},
            {"entrance": "S1", "exit": {"kind": "signal", "name": "SS"}, "path": ["TM", "TQ"], "points": [{"points": "Q", "position": "reverse"}]},
            {"entrance": "SF", "exit": {"kind": "node", "name": "EF"}, "path": ["TF"]},
            {"entrance": "SS", "exit": {"kind": "node", "name": "ES"}, "path": ["TS"]},
        ],
        "train_types": [{"code": "EMU", "max_speed_kmh": 120, "accel": 0.8, "service_brake": 0.7, "emergency_brake": 1.2, "length_m": 100}],
        "services": [{"headcode": "1S01", "train_type": "EMU", "calls": [
            {"place": "MID", "platform": "1", "dep": "06:02", "stop": false},
            {"place": "END", "platform": "S", "arr": "06:05"},
        ], "end": {"kind": "stable"}}],
        "entries": [{"service": "1S01", "boundary": "W", "time": "06:00"}],
        "options": {"start_time": "06:00", "entry_delay_s": [0, 0]},
    });
    let mut sim = Sim::new(signalbox_core::world::World::from_json(&w.to_string()).unwrap(), 1);
    sim.step();
    assert_eq!(sim.trains().len(), 1);
    sim
}

#[test]
fn robot_keeps_the_booked_platform_when_an_earlier_leg_could_strand_it() {
    let sim = split_after_platform();
    let w = sim.world().clone();
    let t = &sim.trains()[0];
    assert_eq!(choose_route(&w, t, sig(&w, "S1")), Some(route(&w, "S1-SS")));
    // SS is clear of other routes' track, so the robot stops planning there.
    assert_eq!(commands(&sim), vec![set(&w, "S1", Exit::Signal(sig(&w, "SS")))]);
    let mut sim = sim;
    let r = soak(&mut sim, 900.0);
    assert_eq!(r.wrong_platforms, 0, "{r:?}");
    assert_eq!(r.stabled, 1, "{r:?}");
}

/// A route further along the chain that is already set (e.g. left behind
/// when the route before it was refused as the chain was applied) is used
/// as it is: the robot asks only for the rest, instead of never asking.
#[test]
fn robot_uses_a_route_already_set_on_the_chain() {
    let mut sim = crossing(false);
    let w = sim.world().clone();
    sim.submit(set(&w, "SB", Exit::Node(node(&w, "EP"))));
    sim.step();
    let cmds = commands(&sim);
    assert!(cmds.contains(&set(&w, "SA", Exit::Signal(sig(&w, "SB")))), "{cmds:?}");
    assert!(!cmds.contains(&set(&w, "SB", Exit::Node(node(&w, "EP")))), "{cmds:?}");
}

/// A train standing at its first call's platform before its dwell starts
/// (e.g. just formed) is not given its departure road: the call it stands
/// at cannot be skipped as unreachable.
#[test]
fn robot_does_not_skip_the_call_a_train_stands_at() {
    let w = load_with("terminus", |v| {
        v["entries"][0] = serde_json::json!({"service": "1A01", "at": {"segment": "p1", "offset_m": 2, "direction": "down"}, "time": "06:00"});
        v["services"][0]["calls"] = serde_json::json!([{"place": "TRM", "platform": "1", "dep": "06:08"}]);
        v["services"][0]["end"] = serde_json::json!({"kind": "exit"});
    })
    .unwrap();
    let mut sim = Sim::new(w, 1);
    sim.step();
    let w = sim.world().clone();
    let mut t = sim.trains()[0].clone();
    t.dwell = None;
    assert_eq!(t.next_call, 0);
    assert_eq!(choose_route(&w, &t, sig(&w, "S3")), None);
}
