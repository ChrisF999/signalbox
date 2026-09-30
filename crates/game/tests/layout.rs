//! The static layout each player is sent.

mod common;

use common::*;
use game::areas::{AreaMap, Visibility};
use game::layout::{build_layout, exit_name};
use protocol::{ExitName, Layout};
use signalbox_core::routes::Exit;

fn layout_for(area_name: Option<&str>) -> Layout {
    let w = twobox();
    let m = AreaMap::new(&w);
    let vis = match area_name {
        Some(a) => Visibility::of_area(&w, &m, area(&w, a)),
        None => Visibility::spectator(&w, &m),
    };
    build_layout(&w, &m, &vis, "alice", None)
}

#[test]
fn an_area_layout_marks_operable_and_fringe_elements() {
    let l = layout_for(Some("West"));
    assert_eq!((l.title.as_str(), l.you.as_str(), l.area.as_deref()), ("Two boxes", "alice", Some("West")));
    assert_eq!(l.areas, ["West", "East"]);
    let sections: Vec<(&str, &str, bool)> = l.sections.iter().map(|s| (s.name.as_str(), s.area.as_str(), s.fringe)).collect();
    assert_eq!(sections, [("TW1", "West", false), ("TW2", "West", false), ("TP", "East", true)]);
    let segments: Vec<&str> = l.segments.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(segments, ["w1", "w2", "pa", "pe", "pn"]);
    let signals: Vec<(&str, bool)> = l.signals.iter().map(|s| (s.name.as_str(), s.operable)).collect();
    assert_eq!(signals, [("W1", true), ("A", true), ("W2", true)]);
    let points: Vec<(&str, &str, bool)> = l.points.iter().map(|p| (p.name.as_str(), p.area.as_str(), p.operable)).collect();
    assert_eq!(points, [("P", "East", false)]);
    let berths: Vec<(&str, bool)> = l.berths.iter().map(|b| (b.name.as_str(), b.operable)).collect();
    assert_eq!(berths, [("BW", true), ("BW1", true), ("BA", true), ("BW2", true)]);
    assert_eq!(l.berths[0].boundary.as_deref(), Some("W"));
    assert_eq!(l.berths[2].signal.as_deref(), Some("A"));
    assert!(l.platforms.is_empty());
    let routes: Vec<(&str, bool)> = l.routes.iter().map(|r| (r.name.as_str(), r.operable)).collect();
    assert_eq!(routes, [("W1-A", true), ("A-E", true), ("A-N", true), ("W2-W", true)]);
    assert_eq!(l.routes[1].exit, ExitName::Node("E".into()));
    assert_eq!(l.routes[0].exit, ExitName::Signal("A".into()));
}

#[test]
fn the_neighbour_sees_the_boundary_signal_but_cannot_work_it() {
    let l = layout_for(Some("East"));
    let a = l.signals.iter().find(|s| s.name == "A").unwrap();
    assert!(!a.operable);
    assert_eq!(a.area, "West");
    let routes: Vec<(&str, bool)> = l.routes.iter().map(|r| (r.name.as_str(), r.operable)).collect();
    assert_eq!(routes, [("A-E", false), ("A-N", false), ("C-W2", true), ("D-W2", true), ("W2-W", false)]);
    assert_eq!(l.platforms.iter().map(|p| p.place.as_str()).collect::<Vec<_>>(), ["EST", "NST"]);
    assert!(l.points[0].operable);
}

#[test]
fn a_spectator_sees_everything_and_works_nothing() {
    let l = layout_for(None);
    assert_eq!(l.area, None);
    assert_eq!((l.sections.len(), l.segments.len(), l.signals.len(), l.berths.len(), l.routes.len()), (5, 7, 5, 8, 6));
    assert!(l.sections.iter().all(|s| !s.fringe));
    assert!(l.signals.iter().all(|s| !s.operable) && l.points.iter().all(|p| !p.operable));
    assert!(l.berths.iter().all(|b| !b.operable) && l.routes.iter().all(|r| !r.operable));
}

#[test]
fn exits_are_named() {
    let w = twobox();
    assert_eq!(exit_name(&w, Exit::Signal(w.net.signal("W2").unwrap())), ExitName::Signal("W2".into()));
    assert_eq!(exit_name(&w, Exit::Node(w.net.node("N").unwrap())), ExitName::Node("N".into()));
}
