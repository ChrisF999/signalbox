//! Converted real layouts run under the robot signaller without incident.

use std::time::{Duration, Instant};

use signalbox_core::robot::{self, ROBOT_EVERY_TICKS, SoakReport, commands, soak, soak_with};
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

/// The robot lets a train wait at an automatic signal on plain line where
/// routes from two platforms meet before it (polish spec P22, §4.6): a
/// westbound train leaves Bank for signal 73 while platform 26 is still
/// occupied, instead of waiting at Bank until it clears.
#[test]
fn drain_trains_wait_at_automatic_signal_73() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let mut w = ts2_import::convert(&std::fs::read_to_string(format!("{dir}/tests/data/drain.json")).unwrap()).unwrap().world;
    // A runs from Bank platform 8 into platform 26 and stands there; B leaves platform 7 behind it.
    let (services, entries) = (
        r#"[{"headcode": "A", "train_type": "UT", "calls": [{"place": "BNK", "platform": "8", "dep": "06:00:00"},
                {"place": "WTL", "platform": "26", "arr": "06:03:00", "dep": "23:00:00"}], "end": {"kind": "stable"}},
            {"headcode": "B", "train_type": "UT", "calls": [{"place": "BNK", "platform": "7", "dep": "06:04:00"},
                {"place": "WTL", "platform": "26", "arr": "06:08:00"}], "end": {"kind": "stable"}}]"#,
        r#"[{"service": "A", "at": {"segment": "L8", "offset_m": 79.0, "direction": "up"}, "time": "06:00:00"},
            {"service": "B", "at": {"segment": "L7", "offset_m": 79.0, "direction": "up"}, "time": "06:00:00"}]"#,
    );
    w.services = serde_json::from_str(services).unwrap();
    w.entries = serde_json::from_str(entries).unwrap();
    let mut sim = Sim::new(World::from_file(w).unwrap(), 7);
    for i in 0..(8 * 600) {
        if i % ROBOT_EVERY_TICKS == 0 {
            for c in robot::commands(&sim) {
                sim.submit(c);
            }
        }
        sim.step();
    }
    let b = sim.trains().iter().find(|t| t.headcode == "B").unwrap();
    let net = &sim.world().net;
    assert_eq!(net.segments[b.head().0.idx()].name, "L1000003", "B waits on the plain line at 73");
    assert_eq!(b.speed, 0.0);
    let a = sim.trains().iter().find(|t| t.headcode == "A").unwrap();
    assert_eq!(net.segments[a.head().0.idx()].name, "L1000009", "A still in platform 26");
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
    /// "headcode@time" of trains seen standing in a platform facing buffers
    /// with their headcode in no berth at all (checked every robot round).
    unlabelled: Vec<String>,
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
            "unlabelled {} | entered {} exited {} stabled {} stuck {} | longest fringe wait {:.0} s | due {} entered {} never {} | entry late p50 {} p90 {} max {} | arrivals {} late {} (late p50 {} p90 {}) all p50 {} p90 {} | \
             wrong platforms {} | penalties {} | robot rounds {} mean {:.1} us max {:.1} ms",
            self.unlabelled.len(),
            r.entered,
            r.exited,
            r.stabled,
            r.stuck.len(),
            r.max_fringe_wait_s,
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
    let mut unlabelled = Vec::new();
    let report = soak_with(&mut sim, hours * 3600.0, |s| {
        let net = &s.world().net;
        for t in s.trains() {
            let (seg, dir) = t.head();
            if t.speed != 0.0 || net.platforms_on[seg.idx()].is_empty() {
                continue;
            }
            // Facing buffers (no signal ahead): a terminal road.
            let terminal = net.first_signal_ahead(seg, dir, t.head_m, 3000.0, s.points()).is_none();
            if terminal && !s.describer().berths.iter().any(|b| b.as_deref() == Some(t.headcode.as_str())) {
                unlabelled.push(format!("{}@{}", t.headcode, s.now_s()));
            }
        }
        let t0 = Instant::now();
        let c = commands(s);
        let dt = t0.elapsed();
        rounds += 1;
        total += dt;
        max = max.max(dt);
        c
    });
    Metrics { report, rounds, robot_total: total, robot_max: max, unlabelled }
}

/// The owner's seeded three-hour Liverpool Street run and three more seeds,
/// printed (no asserts): `-- --ignored --nocapture liverpool_street_metrics`.
#[test]
#[ignore]
fn liverpool_street_metrics() {
    for seed in [LIVERPOOL_SEED, 7, 1, 2] {
        let m = measure(liverpool_world(), seed, 3.0);
        println!("liverpool-st seed {seed}: {}", m.summary());
    }
}

/// Upper bounds for a Liverpool Street robot run.
struct Bounds {
    never_entered: usize,
    entry_late_p90_s: i64,
    arrival_late_p50_s: i64,
    arrival_late_p90_s: i64,
    wrong_platforms: usize,
    penalties: i64,
}

fn assert_bounds(what: &str, m: &Metrics, b: &Bounds) {
    let r = &m.report;
    let s = m.summary();
    assert_safe(what, r);
    let late: Vec<i64> = r.arrival_late_s.iter().map(|&l| l.max(0)).collect();
    assert!(r.entries_due - r.entries_due_entered <= b.never_entered, "{what}: never entered: {s}");
    assert!(pct(&r.entry_late_s, 0.9) <= b.entry_late_p90_s, "{what}: entry lateness: {s}");
    assert!(pct(&late, 0.5) <= b.arrival_late_p50_s, "{what}: arrival lateness p50: {s}");
    assert!(pct(&late, 0.9) <= b.arrival_late_p90_s, "{what}: arrival lateness p90: {s}");
    assert!(r.wrong_platforms <= b.wrong_platforms, "{what}: wrong platforms: {s}");
    assert!(r.penalties <= b.penalties, "{what}: penalties: {s}");
    assert!(m.unlabelled.is_empty(), "{what}: trains at buffers without a headcode {:?}", &m.unlabelled[..m.unlabelled.len().min(10)]);
}

/// Three sim-hours from 05:00:15 under the robot, on the owner's seed and
/// on seed 7: safe, and trains run near time on their booked platforms.
/// Measured after the robot fixes (owner's seed; seed 7): never entered
/// 3; 5, entry lateness p90 273; 290 s, arrival lateness (early = 0) p50
/// 16; 44 s and p90 413; 478 s, wrong platforms 0; 0, penalties 1206; 1271.
/// Before them the owner's seed gave 27 never entered, entry p90 3320 s,
/// arrival p50 972 s / p90 2742 s, 8 wrong platforms, penalties 7778.
/// Slow in debug builds: run with
/// `scripts/cargo test --release -p ts2-import --test soak -- --ignored`.
#[test]
#[ignore]
fn liverpool_street_runs_three_hours() {
    let b = Bounds {
        never_entered: 10,
        entry_late_p90_s: 600,
        arrival_late_p50_s: 180,
        arrival_late_p90_s: 900,
        wrong_platforms: 2,
        penalties: 2500,
    };
    for seed in [LIVERPOOL_SEED, 7] {
        let m = measure(liverpool_world(), seed, 3.0);
        assert_bounds(&format!("liverpool-st seed {seed}"), &m, &b);
        assert!(m.report.max_fringe_wait_s < 1800.0, "{}", m.summary());
        assert!(m.report.exited + m.report.stabled > 0, "{}", m.summary());
    }
}

/// The first 45 sim minutes of the owner's run, fast enough for every test
/// run (measured: 2 of 20 due not yet entered, entry p90 160 s, arrival
/// p50 10 s / p90 180 s, no wrong platforms, penalties 77).
#[test]
fn liverpool_street_runs_its_first_45_minutes_to_time() {
    let m = measure(liverpool_world(), LIVERPOOL_SEED, 0.75);
    let b = Bounds {
        never_entered: 5,
        entry_late_p90_s: 600,
        arrival_late_p50_s: 120,
        arrival_late_p90_s: 600,
        wrong_platforms: 0,
        penalties: 400,
    };
    assert_bounds("liverpool-st 45 min", &m, &b);
}

/// Liverpool Street with its options stripped to what converters wrote
/// before delay bands: it must run bit-identically to before bands existed.
/// No signaller (the robot's choices may change; saves replay commands).
/// Re-pinned once, deliberately, when entries queued at a boundary began to
/// keep their own headcodes (describer state only; was 0x6437f3c3617813e5).
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
    assert_eq!(sim.state_hash(), 0x6bb8e97d75323f5e);
}
