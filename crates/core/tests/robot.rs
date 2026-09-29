mod common;

use common::*;
use signalbox_core::robot::{choose_route, soak};
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
