mod common;

use common::*;
use serde_json::json;
use signalbox_core::events::{Command, Event};
use signalbox_core::routes::Exit;
use signalbox_core::sim::{Outcome, Sim};

fn route_to(sim: &mut Sim, entrance: &str, exit_node: &str) {
    let w = sim.world().clone();
    sim.submit(Command::SetRoute { entrance: sig(&w, entrance), exit: Exit::Node(node(&w, exit_node)) });
}

#[test]
fn arrives_dwells_departs_forms_and_leaves() {
    let mut sim = Sim::new(world("terminus"), 1);
    route_to(&mut sim, "S1", "E1");
    let ev = run_until(&mut sim, 600.0, |e| matches!(e, Event::TrainArrived { .. }));
    let Some(Event::TrainArrived { platform, late_s, .. }) = ev.iter().find(|e| matches!(e, Event::TrainArrived { .. })) else {
        unreachable!()
    };
    assert_eq!(sim.world().net.platforms[platform.idx()].platform, "1");
    assert!(*late_s < 0, "arrives early (at {late_s})");
    assert_eq!(count(&ev, |e| matches!(e, Event::WrongPlatform { .. })), 0);

    let ev = run_until(&mut sim, 900.0, |e| matches!(e, Event::TrainDeparted { .. }));
    assert!(sim.now_s() >= 6.0 * 3600.0 + 8.0 * 60.0, "not before booked departure");
    assert!(ev.contains(&Event::TrainFormed { train: signalbox_core::ids::TrainId(0), headcode: "1A02".into() }));
    let b3 = sim.world().net.berth("B3").unwrap();
    assert_eq!(sim.describer().get(b3), Some("1A02"));

    route_to(&mut sim, "S3", "W");
    run_until(&mut sim, 600.0, |e| matches!(e, Event::TrainExited { .. }));
    assert_eq!(sim.finished()[0].1, Outcome::Exited);
}

#[test]
fn wrong_platform_is_reported() {
    let mut sim = Sim::new(world("terminus"), 1);
    route_to(&mut sim, "S1", "E2");
    let ev = run_until(&mut sim, 600.0, |e| matches!(e, Event::TrainArrived { .. }));
    assert!(ev.iter().any(|e| matches!(e, Event::WrongPlatform { expected, .. } if expected == "1")));
    assert!(sim.scores().total() >= 5);
}

#[test]
fn stabling_service_stays_put() {
    let mut sim = Sim::new(world("terminus"), 1);
    route_to(&mut sim, "S1", "E1");
    run_until(&mut sim, 900.0, |e| matches!(e, Event::TrainFormed { .. }));
    route_to(&mut sim, "S3", "W");
    run_until(&mut sim, 600.0, |e| matches!(e, Event::TrainExited { .. }));
    route_to(&mut sim, "S1", "E2");
    run_until(&mut sim, 1800.0, |e| matches!(e, Event::TrainStabled { .. }));
    assert_eq!(sim.trains().len(), 1);
    assert!(sim.trains()[0].stabled);
    assert!(sim.finished().iter().any(|&(_, o)| o == Outcome::Stabled));
    let ev = sim.run_for(60.0);
    assert_eq!(count(&ev, |e| matches!(e, Event::TrainDeparted { .. })), 0);
}

#[test]
fn passing_call_is_recorded() {
    let w = load_with("plain_line", |v| {
        v["platforms"] = json!([{"place": "MID", "platform": "1", "segment": "b", "from_m": 400, "to_m": 600}]);
        v["services"][0]["calls"] = json!([{"place": "MID", "stop": false, "dep": "06:02"}]);
    })
    .unwrap();
    let mut sim = Sim::new(w, 1);
    let w = sim.world().clone();
    sim.submit(Command::SetRoute { entrance: sig(&w, "S1"), exit: Exit::Signal(sig(&w, "S2")) });
    sim.submit(Command::SetRoute { entrance: sig(&w, "S2"), exit: Exit::Node(node(&w, "E")) });
    let ev = run_until(&mut sim, 400.0, |e| matches!(e, Event::TrainExited { .. }));
    assert_eq!(count(&ev, |e| matches!(e, Event::TrainPassed { .. })), 1);
    assert_eq!(count(&ev, |e| matches!(e, Event::TrainArrived { .. })), 0);
}

#[test]
fn dwell_is_at_least_the_minimum_when_no_departure_is_booked() {
    let w = load_with("terminus", |v| {
        v["services"][0]["calls"][0] = json!({"place": "TRM", "platform": "1", "arr": "06:01"});
    })
    .unwrap();
    let mut sim = Sim::new(w, 1);
    route_to(&mut sim, "S1", "E1");
    run_until(&mut sim, 600.0, |e| matches!(e, Event::TrainArrived { .. }));
    let arrived = sim.now_s();
    run_until(&mut sim, 600.0, |e| matches!(e, Event::TrainDeparted { .. }));
    let dwell = sim.now_s() - arrived;
    assert!((dwell - 30.0).abs() < 0.3, "dwell was {dwell}");
}
