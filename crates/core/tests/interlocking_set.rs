mod common;

use common::*;
use serde_json::json;
use signalbox_core::events::{Event, Rejection};
use signalbox_core::interlocking::{Owner, RouteState};
use signalbox_core::network::PointsPos;

#[test]
fn route_with_points_in_place_locks_on_next_update() {
    let mut rig = Rig::new("terminus");
    let empty = rig.empty();
    let ev = rig.set("S1-E1", &empty).unwrap();
    let r = route(&rig.w, "S1-E1");
    assert_eq!(ev, vec![Event::RouteSetting { route: r }]);
    assert_eq!(rig.il.routes[r.idx()].state, RouteState::Setting);
    let ev = rig.run(0.1, &empty);
    assert_eq!(ev, vec![Event::RouteLocked { route: r }]);
    assert_eq!(rig.il.owner[sec(&rig.w, "TP").idx()], Some(Owner::Path(r)));
    assert_eq!(rig.il.owner[sec(&rig.w, "TP1").idx()], Some(Owner::Path(r)));
}

#[test]
fn route_needing_points_moved_locks_once_detected() {
    let mut rig = Rig::new("terminus");
    let empty = rig.empty();
    let ev = rig.set("S1-E2", &empty).unwrap();
    let p = node(&rig.w, "P");
    assert!(ev.contains(&Event::PointsMoving { points: p, to: PointsPos::Reverse }));
    let r = route(&rig.w, "S1-E2");
    rig.run(4.0, &empty);
    assert_eq!(rig.il.routes[r.idx()].state, RouteState::Setting);
    rig.run(2.0, &empty);
    assert_eq!(rig.il.routes[r.idx()].state, RouteState::Locked);
    assert_eq!(rig.pts.detected(p), Some(PointsPos::Reverse));
}

#[test]
fn conflicting_route_is_rejected() {
    let mut rig = Rig::new("terminus");
    let empty = rig.empty();
    rig.set("S1-E1", &empty).unwrap();
    assert_eq!(rig.set("S3-W", &empty), Err(Rejection::ConflictingRoute));
}

#[test]
fn second_route_from_same_entrance_is_rejected() {
    let mut rig = Rig::new("terminus");
    let empty = rig.empty();
    rig.set("S1-E1", &empty).unwrap();
    assert_eq!(rig.set("S1-E2", &empty), Err(Rejection::ConflictingRoute));
}

#[test]
fn setting_twice_is_rejected() {
    let mut rig = Rig::new("terminus");
    let empty = rig.empty();
    rig.set("S1-E1", &empty).unwrap();
    assert_eq!(rig.set("S1-E1", &empty), Err(Rejection::AlreadySet));
}

#[test]
fn points_in_occupied_section_cannot_move() {
    let mut rig = Rig::new("terminus");
    let busy = occ(&rig.w, &["TP"], false);
    assert_eq!(rig.set("S1-E2", &busy), Err(Rejection::PointsOccupied));
    assert!(rig.set("S1-E1", &busy).is_ok(), "points already normal: no movement needed");
}

#[test]
fn next_route_takes_over_the_overlap() {
    let mut rig = Rig::new("plain_line");
    let empty = rig.empty();
    rig.set("S1-S2", &empty).unwrap();
    let r1 = route(&rig.w, "S1-S2");
    assert_eq!(rig.il.owner[sec(&rig.w, "TC").idx()], Some(Owner::Overlap(r1)));
    rig.set("S2-E", &empty).unwrap();
    let r2 = route(&rig.w, "S2-E");
    assert_eq!(rig.il.owner[sec(&rig.w, "TC").idx()], Some(Owner::Path(r2)));
}

/// Terminus plus signal S0 in rear of S1, whose route overlap runs over P normal.
fn terminus_with_s0() -> Rig {
    let w = load_with("terminus", |v| {
        // Split the approach so S0 stands on a section boundary.
        v["sections"].as_array_mut().unwrap().push(json!({"name": "TIN0", "area": "Box"}));
        v["nodes"].as_array_mut().unwrap().push(json!({"name": "J0", "kind": "joint"}));
        v["segments"][0] = json!(
            {"name": "in", "from": "J0", "to": "J", "length_m": 1500, "line_speed_kmh": 100, "section": "TIN"}
        );
        v["segments"].as_array_mut().unwrap().push(json!(
            {"name": "in0", "from": "W", "to": "J0", "length_m": 500, "line_speed_kmh": 100, "section": "TIN0"}
        ));
        for i in [2, 3] {
            v["routes"][i]["path"] = json!(["TP", "TIN", "TIN0"]);
        }
        v["signals"].as_array_mut().unwrap().push(json!(
            {"name": "S0", "area": "Box", "segment": "in0", "offset_m": 500, "direction": "up", "aspects": 3}
        ));
        v["routes"].as_array_mut().unwrap().push(json!({
            "entrance": "S0", "exit": {"kind": "signal", "name": "S1"}, "path": ["TIN"],
            "overlap": ["TP"], "overlap_points": [{"points": "P", "position": "normal"}]
        }));
    })
    .unwrap();
    Rig::from_world(w)
}

#[test]
fn takeover_needing_points_moved_is_points_locked() {
    let mut rig = terminus_with_s0();
    let empty = rig.empty();
    rig.set("S0-S1", &empty).unwrap();
    rig.run(0.1, &empty);
    assert_eq!(rig.set("S1-E2", &empty), Err(Rejection::PointsLocked));
    assert_eq!(rig.pts.detected(node(&rig.w, "P")), Some(PointsPos::Normal), "locked points never swing");
    assert!(rig.set("S1-E1", &empty).is_ok(), "same points position: takeover allowed");
}

#[test]
fn active_route_from_signal() {
    let mut rig = Rig::new("terminus");
    let empty = rig.empty();
    let s1 = sig(&rig.w, "S1");
    assert_eq!(rig.il.active_route_from(&rig.w, s1), None);
    rig.set("S1-E1", &empty).unwrap();
    assert_eq!(rig.il.active_route_from(&rig.w, s1), Some(route(&rig.w, "S1-E1")));
}
