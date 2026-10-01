//! The scene built from a layout's geometry and lists.

mod common;

use client_ui::scene::{BOUNDARY_BERTH_OFFSET_PX, Scene};
use common::*;
use egui::{Rect, Vec2, pos2, vec2};

#[test]
fn an_area_scene_marks_its_fringe_and_what_it_can_work() {
    let sc = Scene::build(&layout_for(Some("West"))).unwrap();
    let tracks: Vec<(&str, &str, bool)> = sc.tracks.iter().map(|t| (t.segment.as_str(), t.section.as_str(), t.fringe)).collect();
    assert_eq!(tracks, [("w1", "TW1", false), ("w2", "TW2", false)]);
    assert_eq!((sc.tracks[1].a, sc.tracks[1].b), (pos2(100.0, 0.0), pos2(200.0, 0.0)));
    let p = &sc.points[0];
    assert_eq!((p.name.as_str(), p.section.as_str(), p.fringe, p.operable), ("P", "TP", true, false));
    assert_eq!((p.toe, p.normal, p.reverse), (Some(pos2(200.0, 0.0)), Some(pos2(215.0, 0.0)), Some(pos2(215.0, 10.0))));
    let signals: Vec<(&str, Vec2, bool, bool)> = sc.signals.iter().map(|s| (s.name.as_str(), s.facing, s.fringe, s.operable)).collect();
    assert_eq!(
        signals,
        [("W1", vec2(1.0, 0.0), false, true), ("A", vec2(1.0, 0.0), false, true), ("W2", vec2(-1.0, 0.0), false, true)]
    );
    let berths: Vec<(&str, Vec2)> = sc.berths.iter().map(|b| (b.name.as_str(), b.offset_px)).collect();
    assert_eq!(
        berths,
        [("BW1", Vec2::ZERO), ("BA", Vec2::ZERO), ("BW2", Vec2::ZERO), ("BW", BOUNDARY_BERTH_OFFSET_PX)],
        "signal berths at their boxes, the boundary berth above its exit"
    );
    let exits: Vec<(&str, egui::Pos2)> = sc.exits.iter().map(|e| (e.node.as_str(), e.at)).collect();
    assert_eq!(exits, [("E", pos2(400.0, 0.0)), ("N", pos2(400.0, 60.0)), ("W", pos2(0.0, 0.0))]);
    assert_eq!(sc.labels.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["West"]);
    assert_eq!(sc.own, Some(Rect::from_min_max(pos2(0.0, -5.0), pos2(200.0, 5.0))), "own track and signals");
    assert_eq!(sc.fit_bounds(), sc.own);
    assert_eq!(sc.all, Some(Rect::from_min_max(pos2(0.0, -5.0), pos2(207.5, 5.0))));
}

#[test]
fn the_neighbours_signals_are_fringe() {
    let sc = Scene::build(&layout_for(Some("East"))).unwrap();
    let a = sc.signals.iter().find(|s| s.name == "A").unwrap();
    assert!(a.fringe && !a.operable);
    let c = sc.signals.iter().find(|s| s.name == "C").unwrap();
    assert!(!c.fringe && c.operable);
    assert!(sc.points[0].operable && !sc.points[0].fringe);
    assert_eq!(sc.platforms.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), ["EST 1", "NST 1"]);
}

#[test]
fn a_spectator_fits_everything() {
    let sc = Scene::build(&layout_for(None)).unwrap();
    assert_eq!(sc.own, None);
    assert_eq!(sc.fit_bounds(), sc.all);
    assert!(sc.signals.iter().all(|s| !s.fringe && !s.operable));
}

#[test]
fn no_geometry_no_scene() {
    let mut l = layout_for(Some("West"));
    l.geometry = None;
    assert_eq!(Scene::build(&l), None);
}

#[test]
fn a_scene_skips_what_the_drawing_lacks() {
    let mut l = layout_for(Some("West"));
    let g = l.geometry.as_mut().unwrap();
    g.signals.retain(|s| s.signal != "A");
    g.points[0].reverse = None;
    g.lines[0].x1 = f64::NAN;
    g.nodes.retain(|n| n.node != "E");
    g.signals[0].facing = None;
    g.labels.push(protocol::LabelGeom { text: s("far"), x: f64::INFINITY, y: 0.0, arrow: None });
    let sc = Scene::build(&l).unwrap();
    assert_eq!(sc.tracks.len(), 1, "the line with a NaN end is skipped");
    assert!(sc.signals.iter().all(|s| s.name != "A"));
    assert!(sc.berths.iter().all(|b| b.name != "BA"), "A's berth goes with A");
    assert_eq!(sc.points[0].reverse, None);
    assert_eq!(sc.signals[0].facing, Vec2::ZERO);
    assert!(sc.exits.iter().all(|e| e.node != "E"));
    assert_eq!(sc.labels.len(), 1);
}

#[test]
fn automatic_routes_are_listed_on_their_entrance() {
    let mut l = layout_for(Some("West"));
    l.routes[0].automatic = true;
    let sc = Scene::build(&l).unwrap();
    assert_eq!(sc.signals[0].auto_routes, [s("W1-A")]);
    assert!(sc.signals[1].auto_routes.is_empty());
}

#[test]
fn exits_know_their_fringe_and_routes() {
    let sc = Scene::build(&layout_for(Some("West"))).unwrap();
    let exits: Vec<(&str, bool, bool)> = sc.exits.iter().map(|e| (e.node.as_str(), e.fringe, e.route_exit)).collect();
    assert_eq!(exits, [("E", true, true), ("N", true, true), ("W", false, true)]);
    let a = sc.signals.iter().find(|s| s.name == "A").unwrap();
    assert!(a.route_exit, "W1-A ends at A");
    let sc = Scene::build(&layout_for(None)).unwrap();
    assert!(sc.exits.iter().all(|e| !e.fringe && !e.route_exit));
    assert!(sc.signals.iter().all(|s| !s.route_exit));
}

#[test]
fn absurd_coordinates_are_left_out() {
    let mut l = layout_for(Some("West"));
    let g = l.geometry.as_mut().unwrap();
    g.signals.iter_mut().find(|s| s.signal == "A").unwrap().x = 3e38;
    g.lines[0].y2 = -3e38;
    g.labels.push(protocol::LabelGeom { text: s("far"), x: 0.0, y: 1.0e7 + 1.0, arrow: None });
    let sc = Scene::build(&l).unwrap();
    assert!(sc.signals.iter().all(|s| s.name != "A"));
    assert_eq!(sc.tracks.len(), 1);
    assert!(sc.labels.iter().all(|l| l.text != "far"));
    let all = sc.all.unwrap();
    assert!(all.min.is_finite() && all.max.is_finite() && all.center().is_finite());
    assert!(sc.fit_bounds().unwrap().width() <= 1.0e7);
}

#[test]
fn signals_stand_on_their_line_and_know_their_routes() {
    let sc = Scene::build(&layout_for(Some("West"))).unwrap();
    let a = sc.signals.iter().find(|s| s.name == "A").unwrap();
    assert_eq!((a.at, a.base), (pos2(200.0, -5.0), pos2(200.0, 0.0)), "drawn 5 above w2, its post starts on it");
    assert_eq!(a.routes, [s("A-E"), s("A-N")]);
    let mut l = layout_for(Some("West"));
    l.geometry.as_mut().unwrap().lines.retain(|g| g.segment != "w2");
    let sc = Scene::build(&l).unwrap();
    let a = sc.signals.iter().find(|s| s.name == "A").unwrap();
    assert_eq!(a.base, a.at, "no line of its own drawn: the post starts at the signal");
}
