mod common;

use common::*;
use serde_json::json;
use signalbox_core::events::{Event, Refused, Rejection};
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

/// Polish spec M4: the route in the way of a refused route, from the same
/// checks `check_set_route` makes.
fn conflict(rig: &Rig, name: &str) -> Result<(), Refused> {
    rig.il.conflict(&rig.w, &rig.pts, &rig.empty(), route(&rig.w, name))
}

fn refused(reason: Rejection, by: Option<signalbox_core::ids::RouteId>) -> Result<(), Refused> {
    Err(Refused { reason, by })
}

#[test]
fn a_route_already_set_from_the_entrance_is_in_the_way() {
    let mut rig = Rig::new("terminus");
    let empty = rig.empty();
    rig.set("S1-E1", &empty).unwrap();
    assert_eq!(conflict(&rig, "S1-E2"), refused(Rejection::ConflictingRoute, Some(route(&rig.w, "S1-E1"))));
    assert_eq!(conflict(&rig, "S1-E1"), refused(Rejection::AlreadySet, None));
}

#[test]
fn a_route_holding_track_is_in_the_way() {
    let mut rig = Rig::new("terminus");
    let empty = rig.empty();
    rig.set("S1-E1", &empty).unwrap();
    assert_eq!(conflict(&rig, "S3-W"), refused(Rejection::ConflictingRoute, Some(route(&rig.w, "S1-E1"))));
}

#[test]
fn a_route_holding_points_the_wrong_way_is_in_the_way() {
    let mut rig = terminus_with_s0();
    let empty = rig.empty();
    rig.set("S0-S1", &empty).unwrap();
    rig.run(0.1, &empty);
    assert_eq!(conflict(&rig, "S1-E2"), refused(Rejection::PointsLocked, Some(route(&rig.w, "S0-S1"))));
    assert_eq!(conflict(&rig, "S1-E1"), Ok(()));
}

/// S0 → S1 → S2 → E3 on one line: S0-S1's overlap runs over P (in S1-S2's
/// path, lying normal as both need), S1-S2's overlap over Q, which S2-E3
/// needs reverse. With the routes either side set, S1-S2 is refused for Q:
/// the route in the way is the one ahead, not the one behind.
fn neighbours() -> Rig {
    let w = load_with("plain_line", |v| {
        *v = json!({
            "schema": 1, "title": "Neighbours", "areas": [{"name": "Main"}],
            "sections": [
                {"name": "TA", "area": "Main"}, {"name": "TB", "area": "Main"},
                {"name": "TC", "area": "Main"}, {"name": "TD", "area": "Main"}
            ],
            "nodes": [
                {"name": "W", "kind": "boundary"}, {"name": "J0", "kind": "joint"}, {"name": "J1", "kind": "joint"},
                {"name": "P", "kind": "points", "toe": "c", "normal": "pn", "reverse": "pr"},
                {"name": "J2", "kind": "joint"}, {"name": "E1", "kind": "buffer_stop"},
                {"name": "Q", "kind": "points", "toe": "d", "normal": "qn", "reverse": "qr"},
                {"name": "E2", "kind": "boundary"}, {"name": "E3", "kind": "boundary"}
            ],
            "segments": [
                {"name": "a", "from": "W", "to": "J0", "length_m": 1000, "line_speed_kmh": 100, "section": "TA"},
                {"name": "b", "from": "J0", "to": "J1", "length_m": 1000, "line_speed_kmh": 100, "section": "TB"},
                {"name": "c", "from": "J1", "to": "P", "length_m": 50, "line_speed_kmh": 40, "section": "TC"},
                {"name": "pn", "from": "P", "to": "J2", "length_m": 200, "line_speed_kmh": 40, "section": "TC"},
                {"name": "pr", "from": "P", "to": "E1", "length_m": 200, "line_speed_kmh": 40, "section": "TC"},
                {"name": "d", "from": "J2", "to": "Q", "length_m": 50, "line_speed_kmh": 40, "section": "TD"},
                {"name": "qn", "from": "Q", "to": "E2", "length_m": 500, "line_speed_kmh": 40, "section": "TD"},
                {"name": "qr", "from": "Q", "to": "E3", "length_m": 500, "line_speed_kmh": 40, "section": "TD"}
            ],
            "signals": [
                {"name": "S0", "area": "Main", "segment": "a", "offset_m": 1000, "direction": "up", "aspects": 3},
                {"name": "S1", "area": "Main", "segment": "b", "offset_m": 1000, "direction": "up", "aspects": 3},
                {"name": "S2", "area": "Main", "segment": "pn", "offset_m": 200, "direction": "up", "aspects": 3}
            ],
            "berths": [],
            "routes": [
                {"entrance": "S0", "exit": {"kind": "signal", "name": "S1"}, "path": ["TB"],
                 "overlap": ["TC"], "overlap_points": [{"points": "P", "position": "normal"}]},
                {"entrance": "S1", "exit": {"kind": "signal", "name": "S2"}, "path": ["TC"],
                 "points": [{"points": "P", "position": "normal"}],
                 "overlap": ["TD"], "overlap_points": [{"points": "Q", "position": "normal"}]},
                {"entrance": "S2", "exit": {"kind": "node", "name": "E3"}, "path": ["TD"],
                 "points": [{"points": "Q", "position": "reverse"}]}
            ],
            "train_types": [], "services": [], "entries": [], "options": {"start_time": "06:00:00"}
        });
    })
    .unwrap();
    Rig::from_world(w)
}

#[test]
fn with_both_neighbours_set_the_route_ahead_is_in_the_way() {
    let mut rig = neighbours();
    let empty = rig.empty();
    rig.set("S0-S1", &empty).unwrap();
    rig.set("S2-E3", &empty).unwrap();
    rig.run(10.0, &empty);
    assert_eq!(rig.pts.detected(node(&rig.w, "P")), Some(PointsPos::Normal), "P already lies S1-S2's way");
    assert_eq!(conflict(&rig, "S1-S2"), refused(Rejection::PointsLocked, Some(route(&rig.w, "S2-E3"))));
    assert_eq!(rig.set("S1-S2", &empty), Err(Rejection::PointsLocked));
}
