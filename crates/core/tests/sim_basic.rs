mod common;

use common::*;
use serde_json::json;
use signalbox_core::events::{Command, Event};
use signalbox_core::ids::*;
use signalbox_core::network::Dir;
use signalbox_core::routes::Exit;
use signalbox_core::sim::Sim;
use signalbox_core::trains::Train;
use signalbox_core::world::World;

fn set_route(sim: &mut Sim, entrance: &str, exit: Exit) {
    let s = sig(sim.world(), entrance);
    sim.submit(Command::SetRoute { entrance: s, exit });
}

fn plain_routes(sim: &mut Sim) {
    let w = sim.world().clone();
    set_route(sim, "S1", Exit::Signal(sig(&w, "S2")));
    set_route(sim, "S2", Exit::Node(node(&w, "E")));
}

#[test]
fn train_runs_through_and_describer_follows() {
    let mut sim = Sim::new(world("plain_line"), 1);
    plain_routes(&mut sim);
    let w = sim.world().clone();
    let (bw, b1, b2) = (w.net.berth("BW").unwrap(), w.net.berth("B1").unwrap(), w.net.berth("B2").unwrap());
    let ev = run_until(&mut sim, 400.0, |e| matches!(e, Event::TrainExited { .. }));
    assert_eq!(count(&ev, |e| matches!(e, Event::TrainEntered { .. })), 1);
    assert_eq!(count(&ev, |e| matches!(e, Event::SignalPassedAtDanger { .. })), 0);
    let berth_trail: Vec<_> = ev
        .iter()
        .filter_map(|e| match e {
            Event::BerthChanged { berth, headcode: Some(h) } => Some((*berth, h.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(berth_trail, vec![(bw, "2A01".into()), (b1, "2A01".into()), (b2, "2A01".into())]);
    assert!(sim.trains().is_empty());
    assert_eq!(sim.describer().get(b2), None);
}

#[test]
fn signal_goes_red_behind_the_train() {
    let mut sim = Sim::new(world("plain_line"), 1);
    plain_routes(&mut sim);
    let w = sim.world().clone();
    let s1 = sig(&w, "S1");
    run_until(&mut sim, 400.0, |e| matches!(e, Event::BerthChanged { headcode: Some(_), berth } if *berth == w.net.berth("B2").unwrap()));
    sim.run_for(1.0);
    assert_eq!(sim.aspect(s1), signalbox_core::aspect::Aspect::Red);
}

#[test]
fn red_signal_holds_the_train() {
    let mut sim = Sim::new(world("plain_line"), 1);
    let ev = sim.run_for(300.0);
    assert_eq!(count(&ev, |e| matches!(e, Event::SignalPassedAtDanger { .. })), 0);
    let t = &sim.trains()[0];
    assert_eq!(t.head().0, seg(sim.world(), "a"));
    assert!(t.head_m > 985.0 && t.head_m < 1000.0, "stopped at {}", t.head_m);
    assert_eq!(t.speed, 0.0);
}

#[test]
fn cancelling_in_front_of_a_fast_train_causes_a_spad() {
    let mut sim = Sim::new(world("plain_line"), 1);
    plain_routes(&mut sim);
    let a = seg(sim.world(), "a");
    for _ in 0..3000 {
        sim.step();
        if sim.trains().first().is_some_and(|t| t.head().0 == a && t.head_m > 850.0) {
            break;
        }
    }
    let s1 = sig(sim.world(), "S1");
    sim.submit(Command::CancelRoute { entrance: s1 });
    let ev = sim.run_for(60.0);
    assert!(ev.contains(&Event::SignalPassedAtDanger { signal: s1, train: TrainId(0) }), "{ev:?}");
    assert!(sim.scores().total() >= 50);
}

#[test]
fn rejected_commands_come_back_as_events() {
    let mut sim = Sim::new(world("plain_line"), 1);
    let s2 = sig(sim.world(), "S2");
    let bad = Command::SetRoute { entrance: s2, exit: Exit::Signal(sig(sim.world(), "S1")) };
    sim.submit(bad.clone());
    sim.submit(Command::CancelRoute { entrance: SignalId(99) });
    let ev = sim.step();
    assert!(ev.contains(&Event::CommandRejected { cmd: bad, reason: signalbox_core::events::Rejection::NoSuchRoute }));
    assert!(ev.iter().any(|e| matches!(e, Event::CommandRejected { reason: signalbox_core::events::Rejection::UnknownId, .. })));
}

#[test]
fn swing_points_command() {
    let mut sim = Sim::new(world("terminus"), 1);
    let p = node(sim.world(), "P");
    sim.submit(Command::SwingPoints { points: p, to: signalbox_core::network::PointsPos::Reverse });
    let ev = sim.run_for(6.0);
    assert!(ev.contains(&Event::PointsMoved { points: p, to: signalbox_core::network::PointsPos::Reverse }));
    sim.submit(Command::SwingPoints { points: node(sim.world(), "J"), to: signalbox_core::network::PointsPos::Reverse });
    let ev = sim.step();
    assert!(ev.iter().any(|e| matches!(e, Event::CommandRejected { reason: signalbox_core::events::Rejection::NotPoints, .. })));
}

#[test]
fn entries_wait_for_owned_section() {
    let mut sim = Sim::new(world("terminus"), 1);
    let w = sim.world().clone();
    set_route(&mut sim, "S3", Exit::Node(node(&w, "W")));
    let ev = sim.run_for(60.0);
    assert_eq!(count(&ev, |e| matches!(e, Event::TrainEntered { .. })), 0, "outbound route owns the entry section");
    sim.submit(Command::CancelRoute { entrance: sig(&w, "S3") });
    let ev = sim.run_for(5.0);
    assert_eq!(count(&ev, |e| matches!(e, Event::TrainEntered { .. })), 1);
}

#[test]
fn same_tick_entries_do_not_collide() {
    let w = load_with("plain_line", |v| {
        v["services"].as_array_mut().unwrap().push(json!({"headcode": "2A02", "train_type": "EMU"}));
        v["entries"].as_array_mut().unwrap().push(json!({"service": "2A02", "boundary": "W", "time": "06:00:00"}));
    })
    .unwrap();
    let mut sim = Sim::new(w, 1);
    plain_routes(&mut sim);
    let (s1, s2) = (sig(sim.world(), "S1"), sig(sim.world(), "S2"));
    sim.submit(Command::SetAutoWorking { entrance: s1, on: true });
    sim.submit(Command::SetAutoWorking { entrance: s2, on: true });
    let first = sim.step();
    assert_eq!(count(&first, |e| matches!(e, Event::TrainEntered { .. })), 1, "only one fits in TA");
    let ev = sim.run_for(900.0);
    assert_eq!(count(&ev, |e| matches!(e, Event::Collision { .. })), 0);
    assert_eq!(count(&ev, |e| matches!(e, Event::SignalPassedAtDanger { .. })), 0);
    assert_eq!(count(&ev, |e| matches!(e, Event::TrainExited { .. })), 2);
}

#[test]
fn running_into_an_occupied_section_is_a_collision() {
    let w: World = world("plain_line");
    let svc = w.service("2A01").unwrap();
    let (a, b) = (seg(&w, "a"), seg(&w, "b"));
    let mut sim = Sim::new(w, 1);
    let mut standing = Train::new(TrainId(0), svc, "2A01", TrainTypeId(0), 100.0, b, Dir::Up, 0.0);
    standing.head_m = 500.0;
    let mut runner = Train::new(TrainId(1), svc, "2A09", TrainTypeId(0), 100.0, a, Dir::Up, 20.0);
    runner.head_m = 950.0;
    runner.last_passed_aspect = Some(signalbox_core::aspect::Aspect::Green);
    sim.insert_train(standing);
    sim.insert_train(runner);
    let ev = sim.run_for(10.0);
    let tb = sec(sim.world(), "TB");
    assert!(ev.contains(&Event::Collision { train: TrainId(1), other: TrainId(0), section: tb }), "{ev:?}");
    assert!(sim.trains().iter().all(|t| t.emergency || t.speed == 0.0));
}

#[test]
fn time_advances_by_ticks() {
    let mut sim = Sim::new(world("plain_line"), 1);
    assert_eq!(sim.now_s(), 6.0 * 3600.0);
    sim.run_for(10.0);
    assert_eq!(sim.tick(), 100);
    assert!((sim.now_s() - (6.0 * 3600.0 + 10.0)).abs() < 1e-9);
}
