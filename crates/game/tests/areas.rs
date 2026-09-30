//! Command subjects, the fringe and what each player sees.

mod common;

use std::collections::BTreeSet;

use common::*;
use game::areas::{AreaMap, Visibility, fringe, signal_node};
use signalbox_core::events::Command;
use signalbox_core::ids::*;
use signalbox_core::network::PointsPos;
use signalbox_core::routes::Exit;
use signalbox_core::world::World;

fn names<T: Copy>(ids: &[T], name: impl Fn(T) -> String) -> Vec<String> {
    ids.iter().map(|&i| name(i)).collect()
}

#[test]
fn the_fixture_loads_with_two_areas() {
    let w = twobox();
    assert_eq!(w.net.areas.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["West", "East"]);
    assert_eq!(w.routes.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["W1-A", "A-E", "A-N", "C-W2", "D-W2", "W2-W"]);
}

#[test]
fn every_command_maps_to_its_subjects_area() {
    let w = twobox();
    let m = AreaMap::new(&w);
    let net = &w.net;
    let (west, east) = (area(&w, "West"), area(&w, "East"));
    let sig = |x: &str| net.signal(x).unwrap();
    let node = |x: &str| net.node(x).unwrap();
    let berth = |x: &str| net.berth(x).unwrap();
    assert_eq!(m.subject(&Command::SetRoute { entrance: sig("A"), exit: Exit::Node(node("E")) }), Some(west));
    assert_eq!(m.subject(&Command::CancelRoute { entrance: sig("C") }), Some(east));
    assert_eq!(m.subject(&Command::SetAutoWorking { entrance: sig("W1"), on: true }), Some(west));
    assert_eq!(m.subject(&Command::SwingPoints { points: node("P"), to: PointsPos::Reverse }), Some(east));
    assert_eq!(m.subject(&Command::SwingPoints { points: node("J1"), to: PointsPos::Reverse }), None);
    assert_eq!(m.subject(&Command::Interpose { berth: berth("BW"), headcode: "1A01".into() }), Some(west));
    assert_eq!(m.subject(&Command::CancelBerth { berth: berth("BE") }), Some(east));
    assert_eq!(m.subject(&Command::CancelBerth { berth: berth("BA") }), Some(west));
    assert_eq!(m.subject(&Command::CancelRoute { entrance: SignalId(99) }), None);
}

#[test]
fn signals_stand_at_the_nearer_end_of_their_segment() {
    let w = twobox();
    let net = &w.net;
    let at = |s: &str| net.nodes[signal_node(net, net.signal(s).unwrap()).idx()].name.clone();
    assert_eq!([at("W1"), at("A"), at("W2"), at("C"), at("D")], ["J0", "J1", "J0", "J2", "J3"]);
}

#[test]
fn the_fringe_runs_to_the_first_signal_beyond_the_boundary() {
    let w = twobox();
    let names = |set: BTreeSet<SectionId>| set.into_iter().map(|s| w.net.sections[s.idx()].name.clone()).collect::<Vec<_>>();
    // West sees the junction up to C and D; East sees back to W1/W2's node.
    assert_eq!(names(fringe(&w, area(&w, "West"))), ["TP"]);
    assert_eq!(names(fringe(&w, area(&w, "East"))), ["TW2"]);
}

#[test]
fn each_player_sees_their_area_and_its_fringe() {
    let w = twobox();
    let m = AreaMap::new(&w);
    let net = &w.net;
    let sec = |s: SectionId| net.sections[s.idx()].name.clone();
    let sig = |s: SignalId| net.signals[s.idx()].name.clone();
    let node = |n: NodeId| net.nodes[n.idx()].name.clone();
    let berth = |b: BerthId| net.berths[b.idx()].name.clone();
    let route = |r: RouteId| w.routes[r.idx()].name.clone();

    let west = Visibility::of_area(&w, &m, area(&w, "West"));
    assert_eq!(west.area, Some(area(&w, "West")));
    assert_eq!(names(&west.sections, sec), ["TW1", "TW2", "TP"]);
    assert_eq!(west.fringe.iter().copied().map(sec).collect::<Vec<_>>(), ["TP"]);
    assert_eq!(names(&west.signals, sig), ["W1", "A", "W2"]);
    assert_eq!(names(&west.points, node), ["P"]);
    assert_eq!(names(&west.berths, berth), ["BW", "BW1", "BA", "BW2"]);
    assert_eq!(names(&west.routes, route), ["W1-A", "A-E", "A-N", "W2-W"]);
    assert!(west.operable(area(&w, "West")) && !west.operable(area(&w, "East")));

    let east = Visibility::of_area(&w, &m, area(&w, "East"));
    assert_eq!(names(&east.sections, sec), ["TW2", "TP", "TE", "TN"]);
    assert_eq!(names(&east.signals, sig), ["A", "W2", "C", "D"]);
    assert_eq!(names(&east.berths, berth), ["BE", "BN", "BA", "BW2", "BC", "BD"]);
    assert_eq!(names(&east.routes, route), ["A-E", "A-N", "C-W2", "D-W2", "W2-W"]);

    let all = Visibility::spectator(&w, &m);
    assert_eq!(all.area, None);
    assert!(all.fringe.is_empty());
    assert_eq!((all.sections.len(), all.signals.len(), all.points.len(), all.berths.len(), all.routes.len()), (5, 5, 1, 8, 6));
    assert!(!all.operable(area(&w, "West")));
}

#[test]
fn liverpool_street_fringes() {
    let w = World::from_json(&liverpool_json()).unwrap();
    let m = AreaMap::new(&w);
    let sizes: Vec<(String, usize)> =
        w.net.areas.iter().enumerate().map(|(i, a)| (a.name.clone(), fringe(&w, AreaId::from_idx(i)).len())).collect();
    assert_eq!(
        sizes,
        [("Liverpool Street".to_string(), 16), ("Bethnal Green".to_string(), 37), ("Hackney & Bow".to_string(), 33)]
    );
    let seen_by = |s: &str| -> Vec<String> {
        let id = w.net.signal(s).unwrap();
        (0..w.net.areas.len())
            .filter(|&i| Visibility::of_area(&w, &m, AreaId::from_idx(i)).signals.contains(&id))
            .map(|i| w.net.areas[i].name.clone())
            .collect()
    };
    for s in ["61", "63", "65", "64", "66", "68"] {
        assert_eq!(seen_by(s), ["Liverpool Street", "Bethnal Green"], "{s}");
    }
    for s in ["91", "93", "95", "90", "92", "94"] {
        assert_eq!(seen_by(s), ["Bethnal Green", "Hackney & Bow"], "{s}");
    }
}
