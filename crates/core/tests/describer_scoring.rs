mod common;

use common::*;
use signalbox_core::describer::Describer;
use signalbox_core::events::Event;
use signalbox_core::ids::*;
use signalbox_core::scoring::Scores;

#[test]
fn headcode_steps_between_berths() {
    let w = world("plain_line");
    let (b1, b2) = (w.net.berth("B1").unwrap(), w.net.berth("B2").unwrap());
    let mut d = Describer::new(&w.net);
    assert_eq!(d.interpose(b1, "2A01"), vec![Event::BerthChanged { berth: b1, headcode: Some("2A01".into()) }]);
    let ev = d.step(b1, Some(b2));
    assert_eq!(
        ev,
        vec![
            Event::BerthChanged { berth: b1, headcode: None },
            Event::BerthChanged { berth: b2, headcode: Some("2A01".into()) },
        ]
    );
    assert_eq!(d.get(b1), None);
    assert_eq!(d.get(b2), Some("2A01"));
}

#[test]
fn stepping_an_empty_berth_does_nothing() {
    let w = world("plain_line");
    let mut d = Describer::new(&w.net);
    assert!(d.step(w.net.berth("B1").unwrap(), w.net.berth("B2")).is_empty());
}

#[test]
fn stepping_with_nowhere_to_go_clears_the_berth() {
    let w = world("plain_line");
    let b2 = w.net.berth("B2").unwrap();
    let mut d = Describer::new(&w.net);
    d.interpose(b2, "2A01");
    assert_eq!(d.step(b2, None), vec![Event::BerthChanged { berth: b2, headcode: None }]);
}

#[test]
fn cancel_clears_only_filled_berths() {
    let w = world("plain_line");
    let b1 = w.net.berth("B1").unwrap();
    let mut d = Describer::new(&w.net);
    assert_eq!(d.cancel(b1), None);
    d.interpose(b1, "2A01");
    assert_eq!(d.cancel(b1), Some(Event::BerthChanged { berth: b1, headcode: None }));
}

#[test]
fn penalties_by_event() {
    let w = world("terminus");
    let mut s = Scores::new(&w.net);
    let t = TrainId(0);
    let p = PlatformId(0);
    s.apply(&w, &Event::TrainArrived { train: t, platform: p, late_s: 125 });
    assert_eq!(s.total(), 2, "two whole minutes late");
    s.apply(&w, &Event::TrainArrived { train: t, platform: p, late_s: -30 });
    assert_eq!(s.total(), 2, "early is free");
    s.apply(&w, &Event::WrongPlatform { train: t, platform: p, expected: "2".into() });
    assert_eq!(s.total(), 7);
    s.apply(&w, &Event::SignalPassedAtDanger { signal: sig(&w, "S1"), train: t });
    assert_eq!(s.total(), 57);
    s.apply(&w, &Event::Collision { train: t, other: TrainId(1), section: sec(&w, "TP") });
    assert_eq!(s.total(), 557);
    s.apply(&w, &Event::TrainEntered { train: t, headcode: "1A01".into() });
    assert_eq!(s.total(), 557);
    assert_eq!(s.by_area, vec![557]);
}
