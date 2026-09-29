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
