//! Headcodes of trains in terminal platforms stay shown: a train running
//! into a road that ends at buffers keeps its headcode in the berth of the
//! platform starter (the first signal facing back out), which becomes the
//! next working's headcode in place when it forms. Entries waiting at a
//! boundary each take their own headcode in.

mod common;

use common::*;
use signalbox_core::robot::commands;
use signalbox_core::sim::Sim;

/// Run terminus under the robot for `secs`, calling `check` after each tick.
fn run(sim: &mut Sim, secs: f64, mut check: impl FnMut(&Sim)) {
    for i in 0..(secs * 10.0) as u64 {
        if i % 10 == 0 {
            for c in commands(sim) {
                sim.submit(c);
            }
        }
        sim.step();
        check(sim);
    }
}

fn in_platform(sim: &Sim, headcode: &str, seg: &str) -> bool {
    let w = sim.world();
    sim.trains().iter().any(|t| t.headcode == headcode && t.head().0 == w.net.segment(seg).unwrap())
}

#[test]
fn a_terminating_train_keeps_its_headcode_through_forming() {
    let mut sim = Sim::new(world("terminus"), 1);
    let w = sim.world().clone();
    let (b3, b4) = (w.net.berth("B3").unwrap(), w.net.berth("B4").unwrap());
    let mut seen_b3: Vec<String> = Vec::new();
    let mut stabled_shown = false;
    run(&mut sim, 1800.0, |sim| {
        let d = sim.describer();
        for (hc, seg, b) in [("1A01", "p1", b3), ("1A02", "p1", b3), ("1A03", "p2", b4)] {
            if in_platform(sim, hc, seg) {
                assert_eq!(d.get(b), Some(hc), "{hc} in {seg} at {}: {:?}", sim.now_s(), d.berths);
            }
        }
        if let Some(h) = d.get(b3) {
            if seen_b3.last().map(String::as_str) != Some(h) {
                seen_b3.push(h.to_string());
            }
        }
        stabled_shown |= sim.trains().iter().any(|t| t.headcode == "1A03" && t.stabled) && d.get(b4) == Some("1A03");
    });
    assert_eq!(seen_b3, ["1A01", "1A02"], "1A01 shown in the platform, then 1A02 in its place");
    assert!(stabled_shown, "1A03 stabled with its headcode shown");
}

/// A service booked to exit whose last call is a terminal platform cannot
/// leave (trains only reverse to form a new service): it is shown like a
/// stabled train, headcode kept, and the robot asks nothing for it.
#[test]
fn an_exit_service_ending_at_buffers_keeps_its_headcode_and_gets_no_routes() {
    let w = load_with("terminus", |v| {
        v["services"][2]["end"] = serde_json::json!({"kind": "exit"});
        v["entries"] = serde_json::json!([{"service": "1A03", "boundary": "W", "time": "06:10"}]);
    })
    .unwrap();
    let mut sim = Sim::new(w, 1);
    let b4 = sim.world().net.berth("B4").unwrap();
    run(&mut sim, 1800.0, |_| {});
    assert!(in_platform(&sim, "1A03", "p2"), "{:?}", sim.trains());
    assert!(sim.trains()[0].dwell.is_none(), "its dwell is over");
    assert_eq!(sim.describer().get(b4), Some("1A03"));
    assert_eq!(commands(&sim), vec![]);
}

/// Two entries waiting at one boundary: the fringe berth shows the first;
/// when it enters it takes its own headcode in, and the fringe berth then
/// shows the second (it used to step whatever the berth held, so a later
/// offer overwrote the first and the first entered with no headcode).
#[test]
fn queued_entries_at_one_boundary_each_keep_their_headcode() {
    let w = load_with("terminus", |v| v["entries"][1]["time"] = "06:00".into()).unwrap();
    let mut sim = Sim::new(w, 1);
    let (bw, b1) = (sim.world().net.berth("BW").unwrap(), sim.world().net.berth("B1").unwrap());
    sim.step();
    assert_eq!(sim.trains().len(), 1);
    assert_eq!(sim.describer().get(b1), Some("1A01"));
    assert_eq!(sim.describer().get(bw), Some("1A03"));
}
