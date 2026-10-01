//! Converted real layouts run under the robot signaller without incident.

use std::time::{Duration, Instant};

use signalbox_core::robot::{SoakReport, commands, soak, soak_with};
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
fn drain_runs_its_whole_timetable() {
    let r = run("drain", 2.0);
    assert_safe("drain", &r);
    assert!(r.still_running.is_empty(), "{r:?}");
    assert_eq!(r.waiting_to_enter, 0, "{r:?}");
}

/// Three sim-hours from 05:00:15. Slow in debug builds: run with
/// `scripts/cargo test --release -p ts2-import --test soak -- --ignored`.
#[test]
#[ignore]
fn liverpool_street_runs_three_hours() {
    let r = run("liverpool-st", 3.0);
    assert_safe("liverpool-st", &r);
    assert!(r.max_fringe_wait_s < 1800.0, "{r:?}");
    assert!(r.exited + r.stabled > 0, "{r:?}");
}

/// The seed of the owner's Liverpool Street run the robot fixes were measured on.
const LIVERPOOL_SEED: u64 = 8036132600083564156;

/// Liverpool Street as the game runs it (areas applied).
fn liverpool_world() -> World {
    let dir = env!("CARGO_MANIFEST_DIR");
    let ts2 = std::fs::read_to_string(format!("{dir}/tests/data/liverpool-st.json")).unwrap();
    let areas = std::fs::read_to_string(format!("{dir}/../../layouts/liverpool-st.areas.json")).unwrap();
    let mut w = ts2_import::convert(&ts2).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&areas).unwrap()).unwrap();
    World::from_file(w).unwrap()
}

/// Lateness and robot cost of a robot run.
struct Metrics {
    report: SoakReport,
    rounds: u32,
    robot_total: Duration,
    robot_max: Duration,
}

fn pct(v: &[i64], p: f64) -> i64 {
    if v.is_empty() {
        return 0;
    }
    let mut v = v.to_vec();
    v.sort_unstable();
    v[((v.len() - 1) as f64 * p).round() as usize]
}

impl Metrics {
    fn summary(&self) -> String {
        let r = &self.report;
        let late: Vec<i64> = r.arrival_late_s.iter().map(|&l| l.max(0)).collect();
        let late_only: Vec<i64> = r.arrival_late_s.iter().copied().filter(|&l| l > 0).collect();
        format!(
            "due {} entered {} never {} | entry late p50 {} p90 {} max {} | arrivals {} late {} (late p50 {} p90 {}) all p50 {} p90 {} | \
             wrong platforms {} | penalties {} | robot rounds {} mean {:.1} us max {:.1} ms",
            r.entries_due,
            r.entries_due_entered,
            r.entries_due - r.entries_due_entered,
            pct(&r.entry_late_s, 0.5),
            pct(&r.entry_late_s, 0.9),
            pct(&r.entry_late_s, 1.0),
            r.arrival_late_s.len(),
            late_only.len(),
            pct(&late_only, 0.5),
            pct(&late_only, 0.9),
            pct(&late, 0.5),
            pct(&late, 0.9),
            r.wrong_platforms,
            r.penalties,
            self.rounds,
            self.robot_total.as_secs_f64() * 1e6 / f64::from(self.rounds.max(1)),
            self.robot_max.as_secs_f64() * 1e3,
        )
    }
}

fn measure(world: World, seed: u64, hours: f64) -> Metrics {
    let mut sim = Sim::new(world, seed);
    let (mut rounds, mut total, mut max) = (0u32, Duration::ZERO, Duration::ZERO);
    let report = soak_with(&mut sim, hours * 3600.0, |s| {
        let t0 = Instant::now();
        let c = commands(s);
        let dt = t0.elapsed();
        rounds += 1;
        total += dt;
        max = max.max(dt);
        c
    });
    Metrics { report, rounds, robot_total: total, robot_max: max }
}

/// The owner's seeded three-hour Liverpool Street run, printed (no asserts).
#[test]
#[ignore]
fn liverpool_street_metrics() {
    let m = measure(liverpool_world(), LIVERPOOL_SEED, 3.0);
    println!("liverpool-st seed {LIVERPOOL_SEED}: {}", m.summary());
    println!("{:?}", m.report);
}

/// Liverpool Street with its options stripped to what converters wrote
/// before delay bands: it must run bit-identically to before bands existed.
/// No signaller (the robot's choices may change; saves replay commands).
#[test]
fn liverpool_street_without_bands_hashes_as_before() {
    let text = std::fs::read_to_string(format!("{}/tests/data/liverpool-st.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut v = serde_json::to_value(ts2_import::convert(&text).unwrap().world).unwrap();
    let o = v["options"].as_object_mut().unwrap();
    o.remove("entry_delay_bands");
    o.remove("min_dwell_bands");
    let world = World::from_json(&v.to_string()).unwrap();
    let mut sim = Sim::new(world, LIVERPOOL_SEED);
    sim.run_for(3600.0);
    assert_eq!(sim.state_hash(), 0x6437f3c3617813e5);
}
