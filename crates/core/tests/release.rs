mod common;

use common::*;
use serde_json::json;
use signalbox_core::aspect::Aspect::*;
use signalbox_core::events::{Event, Rejection};
use signalbox_core::interlocking::{Owner, Progress, RouteState};
use signalbox_core::network::PointsPos;

fn locked(name: &str, fixture: &str) -> Rig {
    let mut rig = Rig::new(fixture);
    let empty = rig.empty();
    rig.set(name, &empty).unwrap();
    rig.run(6.0, &empty);
    rig
}

#[test]
fn sections_release_in_running_order() {
    let mut rig = locked("S1-E1", "terminus");
    let r = route(&rig.w, "S1-E1");
    let (tp, tp1) = (sec(&rig.w, "TP"), sec(&rig.w, "TP1"));
    let w = rig.w.clone();
    rig.run(0.1, &occ(&w, &["TP"], true));
    assert_eq!(rig.il.routes[r.idx()].progress, vec![Progress::Occupied, Progress::Untouched]);
    rig.run(0.1, &occ(&w, &["TP", "TP1"], true));
    rig.run(0.1, &occ(&w, &["TP1"], true));
    assert_eq!(rig.il.owner[tp.idx()], None);
    assert_eq!(rig.il.owner[tp1.idx()], Some(Owner::Path(r)));
    let ev = rig.run(0.1, &rig.empty());
    assert!(ev.contains(&Event::RouteReleased { route: r }));
    assert_eq!(rig.il.routes[r.idx()].state, RouteState::Idle);
    assert_eq!(rig.il.owner[tp1.idx()], None);
}

#[test]
fn out_of_sequence_occupation_releases_nothing() {
    let mut rig = locked("S1-E1", "terminus");
    let r = route(&rig.w, "S1-E1");
    let w = rig.w.clone();
    rig.run(0.1, &occ(&w, &["TP1"], true));
    rig.run(0.1, &rig.empty());
    assert_eq!(rig.il.routes[r.idx()].progress, vec![Progress::Untouched, Progress::Untouched]);
    assert_eq!(rig.il.owner[sec(&w, "TP1").idx()], Some(Owner::Path(r)));
}

#[test]
fn overlap_releases_after_train_stands_at_exit_signal() {
    let mut rig = locked("S1-S2", "plain_line");
    let r = route(&rig.w, "S1-S2");
    let tc = sec(&rig.w, "TC");
    let standing = occ(&rig.w, &["TB"], false);
    rig.run(59.0, &standing);
    assert_eq!(rig.il.owner[tc.idx()], Some(Owner::Overlap(r)));
    let ev = rig.run(2.0, &standing);
    assert!(ev.contains(&Event::OverlapReleased { route: r }));
    assert_eq!(rig.il.owner[tc.idx()], None);
}

#[test]
fn moving_train_does_not_release_overlap() {
    let mut rig = locked("S1-S2", "plain_line");
    let moving = occ(&rig.w, &["TB"], true);
    rig.run(120.0, &moving);
    assert!(rig.il.owner[sec(&rig.w, "TC").idx()].is_some());
}

#[test]
fn cancel_with_no_train_approaching_releases_at_once() {
    let mut rig = locked("S1-E1", "terminus");
    let r = route(&rig.w, "S1-E1");
    let empty = rig.empty();
    let ev = rig.il.cancel_route(&rig.w, &rig.pts, &empty, r).unwrap();
    assert_eq!(ev, vec![Event::RouteCancelled { route: r, approach_locked: false }]);
    rig.run(0.1, &empty);
    assert_eq!(rig.il.routes[r.idx()].state, RouteState::Idle);
    assert_eq!(rig.il.owner[sec(&rig.w, "TP").idx()], None);
}

#[test]
fn cancel_with_train_approaching_is_approach_locked() {
    let mut rig = locked("S1-E1", "terminus");
    let r = route(&rig.w, "S1-E1");
    let approaching = occ(&rig.w, &["TIN"], true);
    let ev = rig.il.cancel_route(&rig.w, &rig.pts, &approaching, r).unwrap();
    assert_eq!(ev, vec![Event::RouteCancelled { route: r, approach_locked: true }]);
    assert_eq!(rig.aspect("S1", &approaching), Red, "signal replaced immediately");
    rig.run(119.0, &approaching);
    assert_eq!(rig.il.routes[r.idx()].state, RouteState::Locked);
    rig.run(2.0, &approaching);
    assert_eq!(rig.il.routes[r.idx()].state, RouteState::Idle);
}

#[test]
fn cancel_keeps_section_under_train() {
    let mut rig = locked("S1-E1", "terminus");
    let r = route(&rig.w, "S1-E1");
    let on_route = occ(&rig.w, &["TP"], false);
    rig.run(0.1, &on_route);
    rig.il.cancel_route(&rig.w, &rig.pts, &on_route, r).unwrap();
    rig.run(121.0, &on_route);
    assert_eq!(rig.il.owner[sec(&rig.w, "TP").idx()], Some(Owner::Path(r)), "train still on it");
    assert_eq!(
        rig.il.owner[sec(&rig.w, "TP1").idx()],
        Some(Owner::Path(r)),
        "the section ahead of the train stays held while it is on the route"
    );
    rig.run(0.1, &rig.empty());
    assert_eq!(rig.il.routes[r.idx()].state, RouteState::Idle);
}

#[test]
fn cancelled_route_releases_behind_a_train_running_on() {
    let mut rig = locked("S1-E1", "terminus");
    let r = route(&rig.w, "S1-E1");
    let on_tp = occ(&rig.w, &["TP"], true);
    rig.run(0.1, &on_tp);
    rig.il.cancel_route(&rig.w, &rig.pts, &on_tp, r).unwrap();
    rig.run(121.0, &on_tp);
    let (tp, tp1) = (sec(&rig.w, "TP").idx(), sec(&rig.w, "TP1").idx());
    assert_eq!(rig.il.owner[tp1], Some(Owner::Path(r)));
    let both = occ(&rig.w, &["TP", "TP1"], true);
    rig.run(0.1, &both);
    let on_tp1 = occ(&rig.w, &["TP1"], true);
    rig.run(0.1, &on_tp1);
    assert_eq!(rig.il.owner[tp], None, "TP released once the train has left it");
    assert_eq!(rig.il.owner[tp1], Some(Owner::Path(r)), "train is on TP1");
    rig.run(0.1, &rig.empty());
    assert_eq!(rig.il.routes[r.idx()].state, RouteState::Idle);
    assert_eq!(rig.il.owner[tp1], None);
}

#[test]
fn cancelled_route_keeps_its_overlap_while_a_train_is_on_it() {
    let mut rig = locked("S1-S2", "plain_line");
    let r = route(&rig.w, "S1-S2");
    let on_tb = occ(&rig.w, &["TB"], true);
    rig.run(0.1, &on_tb);
    rig.il.cancel_route(&rig.w, &rig.pts, &on_tb, r).unwrap();
    rig.run(121.0, &on_tb);
    assert_eq!(rig.il.owner[sec(&rig.w, "TC").idx()], Some(Owner::Overlap(r)));
    rig.run(0.1, &rig.empty());
    assert_eq!(rig.il.routes[r.idx()].state, RouteState::Idle);
}

#[test]
fn taken_over_overlap_returns_to_its_rear_route() {
    let mut rig = Rig::new("plain_line");
    let empty = rig.empty();
    rig.set("S1-S2", &empty).unwrap();
    rig.set("S2-E", &empty).unwrap();
    let (r1, r2) = (route(&rig.w, "S1-S2"), route(&rig.w, "S2-E"));
    let tc = sec(&rig.w, "TC").idx();
    assert_eq!(rig.il.owner[tc], Some(Owner::Path(r2)));
    rig.il.cancel_route(&rig.w, &rig.pts, &empty, r2).unwrap();
    rig.run(0.2, &empty);
    assert_eq!(rig.il.routes[r2.idx()].state, RouteState::Idle);
    assert_eq!(rig.il.owner[tc], Some(Owner::Overlap(r1)));
}

#[test]
fn cancel_rejects_unset_and_automatic_routes() {
    let mut rig = Rig::new("terminus");
    let empty = rig.empty();
    let r = route(&rig.w, "S1-E1");
    assert_eq!(rig.il.cancel_route(&rig.w, &rig.pts, &empty, r), Err(Rejection::RouteNotSet));

    let w = load_with("plain_line", |v| v["routes"][1]["automatic"] = json!(true)).unwrap();
    let mut rig = Rig::from_world(w);
    rig.set("S2-E", &empty_for(&rig)).unwrap();
    let r = route(&rig.w, "S2-E");
    assert_eq!(rig.il.cancel_route(&rig.w, &rig.pts, &empty_for(&rig), r), Err(Rejection::RouteIsAutomatic));
}

fn empty_for(rig: &Rig) -> signalbox_core::occupancy::Occupancy {
    rig.empty()
}

#[test]
fn auto_working_route_stays_set_and_clears_again() {
    let mut rig = locked("S1-S2", "plain_line");
    let r = route(&rig.w, "S1-S2");
    rig.il.set_auto_working(r, true).unwrap();
    let w = rig.w.clone();
    let train = occ(&w, &["TB"], true);
    rig.run(1.0, &train);
    assert_eq!(rig.aspect("S1", &train), Red);
    let empty = rig.empty();
    rig.run(1.0, &empty);
    assert_eq!(rig.il.routes[r.idx()].state, RouteState::Locked);
    assert_eq!(rig.il.routes[r.idx()].progress, vec![Progress::Untouched]);
    assert_eq!(rig.aspect("S1", &empty), Yellow);
}

#[test]
fn auto_working_needs_a_set_route() {
    let mut rig = Rig::new("plain_line");
    let r = route(&rig.w, "S1-S2");
    assert_eq!(rig.il.set_auto_working(r, true), Err(Rejection::RouteNotSet));
}

#[test]
fn points_moving_under_a_locked_route_is_reported() {
    let mut rig = locked("S1-E2", "terminus");
    let empty = rig.empty();
    rig.pts.start_swing(node(&rig.w, "P"), PointsPos::Normal, 5.0);
    let ev = rig.run(0.1, &empty);
    assert!(ev.iter().any(|e| matches!(e, Event::InvariantViolated { .. })), "{ev:?}");
}

/// S2-E is set first, so TC is its path; S1-S2's overlap is TC.
fn overlap_over_a_set_route() -> Rig {
    let mut rig = Rig::new("plain_line");
    let empty = rig.empty();
    rig.set("S2-E", &empty).unwrap();
    rig.set("S1-S2", &empty).unwrap();
    rig.run(6.0, &empty);
    rig
}

#[test]
fn overlap_may_lie_over_the_path_of_the_route_set_first() {
    let mut rig = overlap_over_a_set_route();
    let (r, x) = (route(&rig.w, "S1-S2"), route(&rig.w, "S2-E"));
    let empty = rig.empty();
    assert_eq!(rig.il.routes[r.idx()].state, RouteState::Locked);
    assert_eq!(rig.il.owner[sec(&rig.w, "TC").idx()], Some(Owner::Path(x)));
    assert_eq!(rig.aspect("S1", &empty), Green);
}

#[test]
fn releasing_the_rear_route_leaves_the_continuing_path_alone() {
    let mut rig = overlap_over_a_set_route();
    let (r, x) = (route(&rig.w, "S1-S2"), route(&rig.w, "S2-E"));
    let (tc, w) = (sec(&rig.w, "TC"), rig.w.clone());
    let empty = rig.empty();
    rig.il.cancel_route(&rig.w, &rig.pts, &empty, r).unwrap();
    rig.run(0.2, &empty);
    assert_eq!(rig.il.routes[r.idx()].state, RouteState::Idle);
    assert_eq!(rig.il.owner[tc.idx()], Some(Owner::Path(x)));
    // and a train running through S1-S2 then S2-E does too
    let mut rig = overlap_over_a_set_route();
    rig.run(0.1, &occ(&w, &["TB"], true));
    rig.run(0.1, &occ(&w, &["TB", "TC"], true));
    rig.run(0.1, &occ(&w, &["TC"], true));
    assert_eq!(rig.il.routes[r.idx()].state, RouteState::Idle);
    assert_eq!(rig.il.owner[tc.idx()], Some(Owner::Path(x)));
}
