//! The owner's real WTT as Drain's timetable (polish spec §4), run for a
//! whole day under the robot. Needs the git-ignored
//! `external/wtt/wtt.bbox.html` (deploy/README.md says how to make it) and
//! is skipped without it; slow in debug builds:
//! `scripts/cargo test --release -p ts2-import --test wtt_day -- --ignored --nocapture`
//!
//! Without the robot's standing rule (`robot::may_stand`, polish spec P22)
//! the morning peak gridlocks at about 07:00 on every seed; with it the
//! whole day runs.

use std::collections::BTreeMap;

use signalbox_core::events::Event;
use signalbox_core::robot::{self, ROBOT_EVERY_TICKS, STUCK_S};
use signalbox_core::sim::{Sim, TICK_S};
use signalbox_core::time::fmt_hms;
use signalbox_core::world::World;
use signalbox_core::world::file::WorldFile;
use ts2_import::wtt;

const WTT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../external/wtt/wtt.bbox.html");

fn drain_with_wtt() -> Option<WorldFile> {
    let text = std::fs::read_to_string(WTT).ok()?;
    let dir = env!("CARGO_MANIFEST_DIR");
    let mut w = ts2_import::convert(&std::fs::read_to_string(format!("{dir}/tests/data/drain.json")).unwrap()).unwrap().world;
    let areas = ts2_import::areas::parse(&std::fs::read_to_string(format!("{dir}/../../layouts/drain.areas.json")).unwrap()).unwrap();
    ts2_import::areas::apply(&mut w, &areas).unwrap();
    let day = wtt::on_day(&wtt::parse(&text).unwrap(), wtt::DAY).unwrap();
    wtt::check(&day, &wtt::Checks::waterloo_city()).unwrap();
    wtt::apply(&mut w, &day).unwrap();
    Some(w)
}

#[derive(Debug, Default)]
struct Day {
    spads: usize,
    collisions: usize,
    violations: usize,
    wrong_platform: usize,
    stuck: Vec<String>,
    running: Vec<String>,
    stabled: usize,
    /// Lateness in seconds at each booked stop and each departure, with headcode and time.
    arrivals: Vec<(i64, String, f64)>,
    departures: Vec<(i64, String, f64)>,
}

/// Until 01:00, as `robot::soak` runs a world, also timing every call.
fn run_day(w: WorldFile, seed: u64) -> Day {
    let mut sim = Sim::new(World::from_file(w).unwrap(), seed);
    let ticks = ((25.0 * 3600.0 - sim.now_s()) / TICK_S).round() as u64;
    let mut d = Day::default();
    let mut still: BTreeMap<u32, (usize, f64, f64)> = BTreeMap::new();
    for i in 0..ticks {
        if i % ROBOT_EVERY_TICKS == 0 {
            for c in robot::commands(&sim) {
                sim.submit(c);
            }
            let now = sim.now_s();
            for t in sim.trains() {
                let here = (t.head().0.idx(), t.head_m);
                if still.get(&t.id.0).is_none_or(|s| (s.0, s.1) != here) {
                    still.insert(t.id.0, (here.0, here.1, now));
                }
            }
        }
        let now = sim.now_s();
        for e in sim.step() {
            match e {
                Event::SignalPassedAtDanger { .. } => d.spads += 1,
                Event::Collision { .. } => d.collisions += 1,
                Event::InvariantViolated { .. } => d.violations += 1,
                Event::WrongPlatform { .. } => d.wrong_platform += 1,
                Event::TrainArrived { train, late_s, .. } | Event::TrainPassed { train, late_s, .. } => {
                    let h = sim.trains().iter().find(|t| t.id == train).map(|t| t.headcode.clone()).unwrap_or_default();
                    d.arrivals.push((late_s, h, now));
                }
                Event::TrainDeparted { train, .. } => {
                    // (A train that formed its next service in the same tick has next_call 0.)
                    if let Some(t) = sim.trains().iter().find(|t| t.id == train && t.next_call > 0) {
                        if let Some(dep) = sim.world().services[t.service.idx()].calls[t.next_call - 1].dep_s {
                            d.departures.push(((now - dep).round() as i64, t.headcode.clone(), now));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let now = sim.now_s();
    let standing = |t: &&signalbox_core::trains::Train| still.get(&t.id.0).is_some_and(|s| now - s.2 >= STUCK_S);
    d.stuck = sim.trains().iter().filter(|t| !t.stabled && t.dwell.is_none()).filter(standing).map(|t| t.headcode.clone()).collect();
    d.running = sim.trains().iter().filter(|t| !t.stabled).map(|t| t.headcode.clone()).collect();
    d.stabled = sim.trains().iter().filter(|t| t.stabled).count();
    d
}

fn worst(v: &[(i64, String, f64)]) -> (i64, String) {
    v.iter().max_by_key(|x| x.0).map(|x| (x.0, format!("{} at {}", x.1, fmt_hms(x.2)))).unwrap_or_default()
}

/// No SPADs, collisions or stuck trains; every train stabled by 01:00; no
/// stop more than 3 minutes late, over five seeds (the dwell times differ).
#[test]
#[ignore = "needs the owner's WTT in external/wtt/ (wtt.bbox.html); run in release"]
fn the_real_wtt_runs_a_whole_day() {
    let Some(w) = drain_with_wtt() else {
        eprintln!("no {WTT}: skipped");
        return;
    };
    let mut failed = Vec::new();
    for seed in [1, 2, 3, 7, 42] {
        let d = run_day(w.clone(), seed);
        let mut hourly: BTreeMap<u32, (i64, usize, usize)> = BTreeMap::new();
        for (late, _, at) in &d.arrivals {
            let e = hourly.entry((*at / 3600.0) as u32).or_default();
            e.0 = e.0.max(*late);
            e.1 += usize::from(*late > 60);
            e.2 += 1;
        }
        eprintln!(
            "seed {seed}: spads {} collisions {} violations {} wrong platform {} stuck {:?} running {:?} stabled {}; {} stops, {} over 1 min late, worst {:?}; {} departures, {} over 1 min late, worst {:?}",
            d.spads,
            d.collisions,
            d.violations,
            d.wrong_platform,
            d.stuck,
            d.running,
            d.stabled,
            d.arrivals.len(),
            d.arrivals.iter().filter(|x| x.0 > 60).count(),
            worst(&d.arrivals),
            d.departures.len(),
            d.departures.iter().filter(|x| x.0 > 60).count(),
            worst(&d.departures),
        );
        eprintln!(
            "  by hour (worst s / stops over 1 min late / stops): {}",
            hourly.iter().map(|(h, (m, l, n))| format!("{h:02}h {m}/{l}/{n}")).collect::<Vec<_>>().join(", ")
        );
        let clean = (d.spads, d.collisions, d.violations) == (0, 0, 0)
            && d.stuck.is_empty()
            && d.running.is_empty()
            && d.stabled == 5
            && worst(&d.arrivals).0 <= 180
            && worst(&d.departures).0 <= 180;
        if !clean {
            failed.push(seed);
        }
    }
    // Every seed is reported before the verdict.
    assert!(failed.is_empty(), "seeds {failed:?} failed");
}
