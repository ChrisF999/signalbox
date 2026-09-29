mod common;

use common::*;
use serde_json::json;
use signalbox_core::ids::NodeId;
use signalbox_core::network::*;

/// Every points node reports the same position (or `None` = moving).
struct Pts(Option<PointsPos>);

impl PointsView for Pts {
    fn position(&self, _: NodeId) -> Option<PointsPos> {
        self.0
    }
}

const NORMAL: Pts = Pts(Some(PointsPos::Normal));

#[test]
fn next_follows_joints_and_stops_at_boundaries() {
    let w = world("plain_line");
    let (a, b, c) = (seg(&w, "a"), seg(&w, "b"), seg(&w, "c"));
    assert_eq!(w.net.next(a, Dir::Up, &NORMAL), Some((b, Dir::Up)));
    assert_eq!(w.net.next(b, Dir::Down, &NORMAL), Some((a, Dir::Down)));
    assert_eq!(w.net.next(c, Dir::Up, &NORMAL), None);
}

#[test]
fn next_through_points_follows_their_position() {
    let w = world("terminus");
    let (app, n, r) = (seg(&w, "app"), seg(&w, "n"), seg(&w, "r"));
    assert_eq!(w.net.next(app, Dir::Up, &NORMAL), Some((n, Dir::Up)));
    assert_eq!(w.net.next(app, Dir::Up, &Pts(Some(PointsPos::Reverse))), Some((r, Dir::Up)));
    assert_eq!(w.net.next(app, Dir::Up, &Pts(None)), None, "moving points block the way");
    assert_eq!(w.net.next(n, Dir::Down, &NORMAL), Some((app, Dir::Down)));
    assert_eq!(w.net.next(n, Dir::Down, &Pts(Some(PointsPos::Reverse))), None, "trailing the wrong leg");
}

#[test]
fn walk_ahead_reports_distances_and_end() {
    let w = world("plain_line");
    let (steps, end) = w.net.walk_ahead(seg(&w, "a"), Dir::Up, 400.0, 5000.0, &NORMAL);
    let got: Vec<_> = steps.iter().map(|s| (s.seg, s.d_start, s.from_along)).collect();
    assert_eq!(got, vec![(seg(&w, "a"), 0.0, 400.0), (seg(&w, "b"), 600.0, 0.0), (seg(&w, "c"), 1600.0, 0.0)]);
    assert_eq!(end, Some(WalkEnd { node: node(&w, "E"), d: 2600.0 }));
}

#[test]
fn walk_ahead_stops_at_max_distance() {
    let w = world("plain_line");
    let (steps, end) = w.net.walk_ahead(seg(&w, "a"), Dir::Up, 0.0, 1500.0, &NORMAL);
    assert_eq!(steps.len(), 2);
    assert_eq!(end, None);
}

#[test]
fn first_signal_ahead_skips_signals_behind() {
    let w = world("plain_line");
    let a = seg(&w, "a");
    assert_eq!(w.net.first_signal_ahead(a, Dir::Up, 0.0, 5000.0, &NORMAL), Some((sig(&w, "S1"), 1000.0)));
    assert_eq!(w.net.first_signal_ahead(a, Dir::Up, 1000.0, 5000.0, &NORMAL), Some((sig(&w, "S2"), 1000.0)));
    assert_eq!(w.net.first_signal_ahead(a, Dir::Up, 0.0, 500.0, &NORMAL), None);
    assert_eq!(w.net.first_signal_ahead(a, Dir::Down, 500.0, 5000.0, &NORMAL), None, "signals face up");
}

#[test]
fn sections_in_rear_of_signal() {
    let w = world("plain_line");
    let s2 = &w.net.signals[sig(&w, "S2").idx()];
    assert_eq!(w.net.sections_in_rear(s2.at, 200.0, &NORMAL), vec![sec(&w, "TB")]);
    assert_eq!(w.net.sections_in_rear(s2.at, 1500.0, &NORMAL), vec![sec(&w, "TB"), sec(&w, "TA")]);
}

#[test]
fn sections_touch_when_they_share_a_node() {
    let w = world("plain_line");
    assert!(w.net.sections_touch(sec(&w, "TA"), sec(&w, "TB")));
    assert!(!w.net.sections_touch(sec(&w, "TA"), sec(&w, "TC")));
}

#[test]
fn platform_along_flips_for_down() {
    let w = load_with("terminus", |v| v["platforms"][0]["from_m"] = json!(50)).unwrap();
    let p = signalbox_core::ids::PlatformId(0);
    assert_eq!(w.net.platform_along(p, Dir::Up), (50.0, 250.0));
    assert_eq!(w.net.platform_along(p, Dir::Down), (0.0, 200.0));
}
