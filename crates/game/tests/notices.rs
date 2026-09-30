//! Notices derived from one tick's events.

mod common;

use common::*;
use game::areas::AreaMap;
use game::notices::area_notices;
use protocol::Notice;
use signalbox_core::events::Event;
use signalbox_core::ids::*;
use signalbox_core::sim::Sim;

fn setup() -> (Sim, AreaMap) {
    let w = twobox();
    let m = AreaMap::new(&w);
    (Sim::new(w, 1), m)
}

fn berth(sim: &Sim, name: &str) -> BerthId {
    sim.world().net.berth(name).unwrap()
}

/// Describer contents at the start of a tick.
fn berths_with(sim: &Sim, filled: &[(&str, &str)]) -> Vec<Option<String>> {
    let mut v = vec![None; sim.world().net.berths.len()];
    for (b, h) in filled {
        v[berth(sim, b).idx()] = Some(h.to_string());
    }
    v
}

fn changed(sim: &Sim, name: &str, headcode: Option<&str>) -> Event {
    Event::BerthChanged { berth: berth(sim, name), headcode: headcode.map(str::to_string) }
}

#[test]
fn a_step_across_the_boundary_is_a_handover() {
    let (sim, m) = setup();
    let before = berths_with(&sim, &[("BC", "2W03")]);
    let ev = vec![changed(&sim, "BC", None), changed(&sim, "BW2", Some("2W03"))];
    let west = area(sim.world(), "West");
    assert_eq!(
        area_notices(&sim, &m, &before, &ev),
        vec![(west, Notice::Handover { headcode: "2W03".into(), from_area: "East".into() })]
    );
}

#[test]
fn steps_inside_an_area_and_interposing_are_not_handovers() {
    let (sim, m) = setup();
    let inside = vec![changed(&sim, "BW1", None), changed(&sim, "BA", Some("1E01"))];
    assert!(area_notices(&sim, &m, &berths_with(&sim, &[("BW1", "1E01")]), &inside).is_empty());
    let interposed = vec![changed(&sim, "BW2", Some("2W03"))];
    assert!(area_notices(&sim, &m, &berths_with(&sim, &[]), &interposed).is_empty());
    let other_train = vec![changed(&sim, "BC", None), changed(&sim, "BW2", Some("2W03"))];
    assert!(area_notices(&sim, &m, &berths_with(&sim, &[("BC", "1A01")]), &other_train).is_empty());
}

#[test]
fn incidents_go_to_the_area_they_happen_in() {
    let (sim, m) = setup();
    let net = &sim.world().net;
    let (west, east) = (area(sim.world(), "West"), area(sim.world(), "East"));
    let ev = vec![
        Event::SignalPassedAtDanger { signal: net.signal("A").unwrap(), train: TrainId(9) },
        Event::Collision { train: TrainId(9), other: TrainId(8), section: net.section("TP").unwrap() },
        Event::TrainArrived { train: TrainId(9), platform: PlatformId(0), late_s: 59 },
        Event::TrainArrived { train: TrainId(9), platform: PlatformId(0), late_s: 60 },
        Event::TrainPassed { train: TrainId(9), platform: PlatformId(1), late_s: 300 },
        Event::WrongPlatform { train: TrainId(9), platform: PlatformId(0), expected: "2".into() },
    ];
    let train = || "train 9".to_string();
    assert_eq!(
        area_notices(&sim, &m, &berths_with(&sim, &[]), &ev),
        vec![
            (west, Notice::Spad { signal: "A".into(), train: train() }),
            (east, Notice::Collision { section: "TP".into() }),
            (east, Notice::Late { train: train(), place: "EST".into(), platform: "1".into(), late_s: 60 }),
            (east, Notice::Late { train: train(), place: "NST".into(), platform: "1".into(), late_s: 300 }),
            (east, Notice::WrongPlatform { train: train(), place: "EST".into(), platform: "1".into(), expected: "2".into() }),
        ]
    );
}

#[test]
fn trains_are_named_by_headcode() {
    let (mut sim, m) = setup();
    sim.run_for(1.0);
    let w1 = sim.world().net.signal("W1").unwrap();
    let west = area(sim.world(), "West");
    let ev = vec![Event::SignalPassedAtDanger { signal: w1, train: TrainId(0) }];
    assert_eq!(
        area_notices(&sim, &m, &berths_with(&sim, &[]), &ev),
        vec![(west, Notice::Spad { signal: "W1".into(), train: "1E01".into() })]
    );
}
