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

#[test]
fn dwelling_train_waits_for_a_red_starter() {
    let w = load_with("plain_line", |v| {
        v["platforms"] = json!([{"place": "MID", "platform": "1", "segment": "b", "from_m": 700, "to_m": 950}]);
        v["services"][0]["calls"] = json!([{"place": "MID", "platform": "1", "arr": "06:01", "dep": "06:02"}]);
    })
    .unwrap();
    let mut sim = Sim::new(w, 1);
    let w = sim.world().clone();
    sim.submit(Command::SetRoute { entrance: sig(&w, "S1"), exit: Exit::Signal(sig(&w, "S2")) });
    run_until(&mut sim, 600.0, |e| matches!(e, Event::TrainArrived { .. }));
    let ev = sim.run_for(240.0);
    assert_eq!(count(&ev, |e| matches!(e, Event::TrainDeparted { .. })), 0, "starter S2 is red");
    assert!(sim.now_s() > 6.0 * 3600.0 + 2.0 * 60.0, "booked departure has passed");
    sim.submit(Command::SetRoute { entrance: sig(&w, "S2"), exit: Exit::Node(node(&w, "E")) });
    let ev = run_until(&mut sim, 60.0, |e| matches!(e, Event::TrainDeparted { .. }));
    assert_eq!(count(&ev, |e| matches!(e, Event::TrainDeparted { .. })), 1);
}

#[test]
fn offered_entries_wait_in_pending_until_they_enter() {
    let w = load_with("terminus", |j| j["options"]["entry_delay_s"] = json!([120, 120])).unwrap();
    let mut sim = Sim::new(w, 1);
    assert_eq!((sim.next_entry(), sim.pending_entries().len()), (0, 0));
    sim.step();
    assert_eq!(sim.next_entry(), 1, "06:00's entry is offered");
    assert_eq!(sim.pending_entries().len(), 1);
    assert_eq!((sim.pending_entries()[0].entry, sim.pending_entries()[0].due_s), (0, 6.0 * 3600.0 + 120.0));
    assert!(sim.trains().is_empty());
    sim.run_for(130.0);
    assert!(sim.pending_entries().is_empty());
    assert_eq!(sim.trains().len(), 1);
}

/// plain_line with its one entry offered only on demand.
fn on_demand_line() -> signalbox_core::world::World {
    load_with("plain_line", |v| v["entries"][0]["on_demand"] = json!(true)).unwrap()
}

#[test]
fn an_on_demand_entry_waits_until_it_is_offered() {
    let mut sim = Sim::new(on_demand_line(), 1);
    let ev = sim.run_for(600.0);
    assert_eq!(count(&ev, |e| matches!(e, Event::TrainEntered { .. })), 0, "never at its time");
    assert!(sim.pending_entries().is_empty());
    let bw = sim.world().net.berth("BW").unwrap();
    let ev = sim.offer_entry(0).unwrap();
    assert_eq!(ev, vec![Event::BerthChanged { berth: bw, headcode: Some("2A01".into()) }]);
    assert_eq!(sim.pending_entries().len(), 1);
    let ev = sim.step();
    assert!(ev.iter().any(|e| matches!(e, Event::TrainEntered { headcode, .. } if headcode == "2A01")));
    assert_eq!(sim.trains().len(), 1);
}

#[test]
fn only_on_demand_entries_can_be_offered() {
    let mut sim = Sim::new(world("plain_line"), 1);
    assert_eq!(sim.offer_entry(0).unwrap_err(), "entry 0 is not on demand");
    assert_eq!(sim.offer_entry(7).unwrap_err(), "no entry 7");
}

#[test]
fn an_offered_entry_survives_a_snapshot() {
    let mut sim = Sim::new(on_demand_line(), 1);
    sim.offer_entry(0).unwrap();
    let snap = sim.snapshot();
    let mut back = Sim::restore(sim.world().clone(), snap).unwrap();
    back.step();
    assert_eq!(back.trains().len(), 1);
}

#[test]
fn on_demand_is_left_out_of_a_written_world_unless_set() {
    use signalbox_core::world::file::WorldFile;
    let f: WorldFile = serde_json::from_value(fixture_json("plain_line")).unwrap();
    let text = serde_json::to_string(&f).unwrap();
    assert!(!text.contains("on_demand"), "converted worlds stay byte-identical");
    let mut f = f;
    f.entries[0].on_demand = true;
    assert!(serde_json::to_string(&f).unwrap().contains(r#""on_demand":true"#));
}

#[test]
fn passing_a_signal_is_an_event() {
    let mut sim = Sim::new(world("plain_line"), 1);
    let w = sim.world().clone();
    let train = signalbox_core::ids::TrainId(0);
    sim.submit(Command::SetRoute { entrance: sig(&w, "S1"), exit: Exit::Signal(sig(&w, "S2")) });
    let ev = run_until(&mut sim, 600.0, |e| matches!(e, Event::SignalPassed { .. }));
    let passed: Vec<&Event> = ev.iter().filter(|e| matches!(e, Event::SignalPassed { .. })).collect();
    assert_eq!(passed, vec![&Event::SignalPassed { signal: sig(&w, "S1"), train }]);
    let ev = sim.run_for(300.0);
    assert_eq!(count(&ev, |e| matches!(e, Event::SignalPassed { .. })), 0, "it stands at S2, which is red");
    sim.submit(Command::SetRoute { entrance: sig(&w, "S2"), exit: Exit::Node(node(&w, "E")) });
    let ev = run_until(&mut sim, 600.0, |e| matches!(e, Event::SignalPassed { .. }));
    assert!(ev.contains(&Event::SignalPassed { signal: sig(&w, "S2"), train }));
    assert_eq!(count(&ev, |e| matches!(e, Event::SignalPassedAtDanger { .. })), 0);
}
