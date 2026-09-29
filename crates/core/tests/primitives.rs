mod common;

use common::*;
use serde_json::json;
use signalbox_core::aspect::{Aspect::*, cleared_aspect, expected_after};
use signalbox_core::events::Command;
use signalbox_core::ids::*;
use signalbox_core::network::{PointsPos, PointsView};
use signalbox_core::occupancy::Occupancy;
use signalbox_core::points::{PointsState, PointsTable};
use signalbox_core::routes::Exit;

#[test]
fn cleared_aspect_table() {
    assert_eq!(cleared_aspect(4, Red), Yellow);
    assert_eq!(cleared_aspect(4, Yellow), DoubleYellow);
    assert_eq!(cleared_aspect(4, DoubleYellow), Green);
    assert_eq!(cleared_aspect(4, Green), Green);
    assert_eq!(cleared_aspect(3, Red), Yellow);
    assert_eq!(cleared_aspect(3, Yellow), Green);
    assert_eq!(cleared_aspect(3, Green), Green);
    assert_eq!(cleared_aspect(2, Red), Green);
    assert_eq!(cleared_aspect(2, Yellow), Green);
}

#[test]
fn expected_aspect_after_passing_one() {
    assert_eq!(expected_after(Red), Red);
    assert_eq!(expected_after(Yellow), Red);
    assert_eq!(expected_after(DoubleYellow), Yellow);
    assert_eq!(expected_after(Green), Green);
}

#[test]
fn points_start_normal_and_swing_over_time() {
    let w = world("terminus");
    let p = node(&w, "P");
    let mut pt = PointsTable::new(&w.net);
    assert_eq!(pt.detected(p), Some(PointsPos::Normal));
    assert_eq!(pt.state(node(&w, "J")), None);
    pt.start_swing(p, PointsPos::Reverse, 5.0);
    assert_eq!(pt.detected(p), None);
    assert_eq!(pt.position(p), None);
    let mut done = vec![];
    for _ in 0..45 {
        done.extend(pt.tick(0.1));
    }
    assert!(done.is_empty(), "still moving after 4.5 s");
    for _ in 0..15 {
        done.extend(pt.tick(0.1));
    }
    assert_eq!(done, vec![(p, PointsPos::Reverse)]);
    assert_eq!(pt.state(p), Some(PointsState::Set(PointsPos::Reverse)));
}

#[test]
fn swinging_to_current_position_does_nothing() {
    let w = world("terminus");
    let p = node(&w, "P");
    let mut pt = PointsTable::new(&w.net);
    pt.start_swing(p, PointsPos::Normal, 5.0);
    assert_eq!(pt.detected(p), Some(PointsPos::Normal));
}

#[test]
fn occupancy_tracks_trains_and_movement() {
    let mut o = Occupancy::new(3);
    let s = SectionId(1);
    assert!(!o.occupied(s));
    o.add(s, TrainId(4), false);
    o.add(s, TrainId(4), false);
    assert!(o.occupied(s));
    assert!(o.stationary(s));
    assert_eq!(o.trains_in(s), &[TrainId(4)]);
    o.add(s, TrainId(5), true);
    assert!(!o.stationary(s));
    assert!(!o.stationary(SectionId(0)), "an empty section is not 'stationary'");
}

#[test]
fn command_json_shape() {
    let c = Command::SetRoute { entrance: SignalId(1), exit: Exit::Node(NodeId(2)) };
    let v = serde_json::to_value(&c).unwrap();
    assert_eq!(v, json!({"cmd": "set_route", "entrance": 1, "exit": {"kind": "node", "id": 2}}));
    let back: Command = serde_json::from_value(v).unwrap();
    assert_eq!(back, c);
}
