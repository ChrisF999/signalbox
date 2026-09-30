mod common;

use common::*;
use serde_json::json;
use signalbox_core::events::{Command, Event};
use signalbox_core::routes::Exit;
use signalbox_core::sim::{Sim, SimState};

fn busy_terminus(seed: u64) -> Sim {
    let w = load_with("terminus", |v| v["options"]["entry_delay_s"] = json!([0, 60])).unwrap();
    let mut sim = Sim::new(w, seed);
    let w = sim.world().clone();
    sim.submit(Command::SetRoute { entrance: sig(&w, "S1"), exit: Exit::Node(node(&w, "E1")) });
    sim
}

#[test]
fn snapshot_survives_json() {
    let mut sim = busy_terminus(3);
    sim.run_for(200.0);
    let s = sim.snapshot();
    let back: SimState = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(s, back);
}

#[test]
fn restored_sim_continues_identically() {
    let mut a = busy_terminus(3);
    a.run_for(200.0);
    let mut b = Sim::restore(a.world().clone(), a.snapshot()).unwrap();
    a.run_for(600.0);
    b.run_for(600.0);
    assert_eq!(a.snapshot(), b.snapshot());
}

#[test]
fn replaying_the_log_reproduces_the_run() {
    let mut sim = busy_terminus(7);
    run_until(&mut sim, 900.0, |e| matches!(e, Event::TrainFormed { .. }));
    let w = sim.world().clone();
    sim.submit(Command::SetRoute { entrance: sig(&w, "S3"), exit: Exit::Node(node(&w, "W")) });
    sim.run_for(300.0);
    let again = Sim::replay(w, 7, sim.log(), sim.tick());
    assert_eq!(again.snapshot(), sim.snapshot());
}

#[test]
fn different_seeds_can_differ() {
    let mut a = busy_terminus(1);
    let mut b = busy_terminus(2);
    a.run_for(120.0);
    b.run_for(120.0);
    assert_ne!(a.snapshot().rng_seed, b.snapshot().rng_seed);
}

#[test]
fn restore_rejects_a_snapshot_from_another_world() {
    let mut sim = busy_terminus(3);
    sim.run_for(10.0);
    let err = Sim::restore(world("plain_line"), sim.snapshot()).err().expect("size mismatch is rejected");
    assert!(err.contains("does not match"), "{err}");
}

#[test]
fn replay_accepts_an_unsorted_log() {
    let mut sim = busy_terminus(7);
    sim.run_for(50.0);
    let w = sim.world().clone();
    sim.submit(Command::SetRoute { entrance: sig(&w, "S3"), exit: Exit::Node(node(&w, "W")) });
    sim.run_for(50.0);
    let mut log = sim.log().to_vec();
    assert!(log.len() >= 2 && log[0].0 < log[1].0, "need commands at different ticks");
    log.reverse();
    let again = Sim::replay(w, 7, &log, sim.tick());
    assert_eq!(again.snapshot(), sim.snapshot());
}

#[test]
fn state_hash_is_fnv1a_of_the_snapshot_json() {
    let mut sim = busy_terminus(3);
    sim.run_for(60.0);
    let json = serde_json::to_string(&sim.snapshot()).unwrap();
    let want = json.bytes().fold(0xcbf29ce484222325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3));
    assert_eq!(sim.state_hash(), want);
}

#[test]
fn state_hash_follows_the_state() {
    let mut a = busy_terminus(3);
    let mut b = busy_terminus(3);
    a.run_for(100.0);
    b.run_for(100.0);
    assert_eq!(a.state_hash(), b.state_hash());
    b.step();
    assert_ne!(a.state_hash(), b.state_hash());
}

#[test]
fn robot_cadence_is_public() {
    assert_eq!(signalbox_core::robot::ROBOT_EVERY_TICKS, 10);
}
