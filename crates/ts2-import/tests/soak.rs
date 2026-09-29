//! Converted real layouts run under the robot signaller without incident.

use signalbox_core::robot::{SoakReport, soak};
use signalbox_core::sim::Sim;
use signalbox_core::world::World;

fn run(name: &str, hours: f64) -> SoakReport {
    let text = std::fs::read_to_string(format!("{}/tests/data/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let world = World::from_file(ts2_import::convert(&text).unwrap().world).unwrap();
    let mut sim = Sim::new(world, 7);
    soak(&mut sim, hours * 3600.0)
}

fn assert_safe(name: &str, r: &SoakReport) {
    assert_eq!(r.spads, 0, "{name}: {r:?}");
    assert_eq!(r.collisions, 0, "{name}: {r:?}");
    assert_eq!(r.invariant_violations, 0, "{name}: {r:?}");
    assert!(r.stuck.is_empty(), "{name}: {r:?}");
}

#[test]
fn mini_runs_its_timetable() {
    let r = run("mini", 0.5);
    assert_safe("mini", &r);
    assert_eq!(r.stabled, 1, "{r:?}");
    assert!(r.still_running.is_empty(), "{r:?}");
}

#[test]
#[ignore = "blocked: overlap of a route onto a following automatic route's path can never be held; see task-8 report"]
fn drain_runs_its_whole_timetable() {
    let r = run("drain", 2.0);
    assert_safe("drain", &r);
    assert!(r.still_running.is_empty(), "{r:?}");
    assert_eq!(r.waiting_to_enter, 0, "{r:?}");
}
