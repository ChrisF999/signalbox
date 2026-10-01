//! Weighted delay bands (TS2's `[lo, hi, weight]` generators) for entry
//! delays and minimum dwells, and the golden hash that keeps worlds without
//! bands bit-identical to before bands existed.

mod common;

use common::*;
use serde_json::json;
use signalbox_core::robot::soak;
use signalbox_core::sim::Sim;

/// Worlds without bands draw exactly as they did before bands existed.
/// (Re-pinned once, deliberately, when terminating trains began to keep
/// their headcodes in the platform starter's berth: that changes describer
/// state, not the draws; the pre-change hash was 0x2cfa948fb4334461.)
#[test]
fn worlds_without_bands_hash_as_before() {
    let w = load_with("terminus", |v| {
        v["options"]["entry_delay_s"] = json!([0, 240]);
        v["options"]["min_dwell_s"] = json!([20, 90]);
    })
    .unwrap();
    let mut sim = Sim::new(w, 11);
    soak(&mut sim, 3600.0);
    assert_eq!(sim.state_hash(), 0xf18e905cfc5053e9);
}

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use signalbox_core::events::Event;
use signalbox_core::timetable::{DelayBand, draw_delay};

fn band(lo_s: i32, hi_s: i32, weight: u32) -> DelayBand {
    DelayBand { lo_s, hi_s, weight }
}

/// Liverpool Street's `defaultDelayAtEntry`.
fn liverpool_bands() -> Vec<DelayBand> {
    vec![band(-120, -60, 15), band(-60, 180, 50), band(120, 300, 30), band(300, 3600, 5)]
}

#[test]
fn draws_stay_in_the_bands_and_follow_the_weights() {
    let bands = liverpool_bands();
    let mut rng = ChaCha8Rng::seed_from_u64(5);
    let n = 100_000;
    let draws: Vec<i32> = (0..n).map(|_| draw_delay(&bands, (0, 0), &mut rng)).collect();
    assert!(draws.iter().all(|&d| (-120..=3600).contains(&d)));
    let share = |f: &dyn Fn(i32) -> bool| draws.iter().filter(|&&d| f(d)).count() as f64 / f64::from(n);
    // Only the first band reaches below -60, only the last above 300.
    assert!((share(&|d| d < -60) - 0.15 * 59.0 / 61.0).abs() < 0.01, "{}", share(&|d| d < -60));
    assert!((share(&|d| d > 300) - 0.05 * 3299.0 / 3301.0).abs() < 0.005, "{}", share(&|d| d > 300));
    // (180, 300) is the third band only.
    assert!((share(&|d| d > 180 && d < 300) - 0.30 * 119.0 / 181.0).abs() < 0.01);
    let mean = draws.iter().map(|&d| f64::from(d)).sum::<f64>() / f64::from(n);
    let want = 0.15 * -90.0 + 0.50 * 60.0 + 0.30 * 210.0 + 0.05 * 1950.0;
    assert!((mean - want).abs() < 10.0, "mean {mean} want {want}");
}

#[test]
fn without_bands_the_range_is_drawn_as_before() {
    let mut a = ChaCha8Rng::seed_from_u64(9);
    let mut b = ChaCha8Rng::seed_from_u64(9);
    use rand::Rng;
    for _ in 0..1000 {
        let old = b.random_range(10u32..=500);
        assert_eq!(draw_delay(&[], (10, 500), &mut a), old as i32);
    }
}

/// Terminus with only 1A03 (booked 06:10) and the given entry options.
fn lone_entry(options: serde_json::Value) -> Sim {
    let w = load_with("terminus", |v| {
        v["entries"] = json!([{"service": "1A03", "boundary": "W", "time": "06:10"}]);
        for (k, val) in options.as_object().unwrap() {
            v["options"][k] = val.clone();
        }
    })
    .unwrap();
    Sim::new(w, 1)
}

fn entered_at(sim: &mut Sim) -> f64 {
    run_until(sim, 3600.0, |e| matches!(e, Event::TrainEntered { .. }));
    sim.now_s() - 0.1
}

#[test]
fn a_negative_band_enters_early() {
    let mut sim = lone_entry(json!({"entry_delay_bands": [{"lo_s": -120, "hi_s": -120, "weight": 1}]}));
    let t = entered_at(&mut sim);
    assert!((t - (6.0 * 3600.0 + 480.0)).abs() < 0.11, "entered at {t}");
}

#[test]
fn a_positive_band_enters_late() {
    let mut sim = lone_entry(json!({"entry_delay_bands": [{"lo_s": 600, "hi_s": 600, "weight": 3}]}));
    let t = entered_at(&mut sim);
    assert!((t - (6.0 * 3600.0 + 1200.0)).abs() < 0.11, "entered at {t}");
}

#[test]
fn dwell_bands_set_the_minimum_dwell() {
    // 1A03 is booked 06:15 / 06:20; arriving late, it stands the band's 400 s.
    let mut sim = lone_entry(json!({
        "entry_delay_bands": [{"lo_s": 900, "hi_s": 900, "weight": 1}],
        "min_dwell_bands": [{"lo_s": 400, "hi_s": 400, "weight": 1}],
    }));
    for c in signalbox_core::robot::commands(&sim) {
        sim.submit(c);
    }
    let mut arrived = None;
    for _ in 0..36_000 {
        if sim.tick() % 10 == 0 {
            for c in signalbox_core::robot::commands(&sim) {
                sim.submit(c);
            }
        }
        let now = sim.now_s();
        for e in sim.step() {
            if let Event::TrainArrived { .. } = e {
                arrived = Some(now);
            }
        }
        if arrived.is_some() {
            break;
        }
    }
    let a = arrived.expect("arrives");
    let t = sim.trains()[0].dwell.expect("dwelling").depart_at_s;
    assert!((t - (a + 400.0)).abs() < 0.11, "arrived {a} departs {t}");
}

#[test]
fn bad_bands_are_rejected() {
    for (k, bands) in [
        ("entry_delay_bands", json!([{"lo_s": 60, "hi_s": 0, "weight": 1}])),
        ("entry_delay_bands", json!([{"lo_s": 0, "hi_s": 60, "weight": 0}])),
        ("min_dwell_bands", json!([{"lo_s": -10, "hi_s": 60, "weight": 1}])),
    ] {
        let r = load_with("terminus", |v| v["options"][k] = bands.clone());
        assert!(r.is_err(), "{k} {bands}");
    }
}
