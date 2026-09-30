//! Diagram geometry (spec D1 §4.1): read once from the world's `layout`,
//! cut down to each player's visible set.

mod common;

use common::*;
use game::areas::{AreaMap, Visibility};
use game::geometry::WorldGeometry;
use game::layout::build_layout;
use game::{Game, GameMeta};
use protocol::{Geometry, LabelGeom, LineGeom, NodeGeom, PlatformGeom, PointsGeom, ServerMsg, SignalGeom};
use serde_json::{Value, json};
use signalbox_core::world::World;

/// Twobox drawn left to right (`fixtures/twobox-layout.json`): W at x 0,
/// the points at x 207.5 with their legs undrawn (like TS2's), E and N on
/// the right; plus an unknown segment, a joint named as points, a signal
/// the world lacks and a label far away.
fn twobox_layout() -> Value {
    serde_json::from_str(&std::fs::read_to_string(TWOBOX_LAYOUT).unwrap()).unwrap()
}

fn twobox_with(layout: Value) -> World {
    let mut json: Value = serde_json::from_str(&twobox_json()).unwrap();
    json["layout"] = layout;
    World::from_json(&json.to_string()).unwrap()
}

fn geometry_for(w: &World, area_name: Option<&str>) -> Option<Geometry> {
    let m = AreaMap::new(w);
    let vis = match area_name {
        Some(a) => Visibility::of_area(w, &m, area(w, a)),
        None => Visibility::spectator(w, &m),
    };
    build_layout(w, &m, &vis, "alice", WorldGeometry::from_world(w).as_ref()).geometry
}

fn line(segment: &str, x1: f64, y1: f64, x2: f64, y2: f64) -> LineGeom {
    LineGeom { segment: segment.into(), x1, y1, x2, y2 }
}

fn node(name: &str, x: f64, y: f64) -> NodeGeom {
    NodeGeom { node: name.into(), x, y }
}

#[test]
fn an_area_sees_its_own_drawing_and_the_fringe() {
    let g = geometry_for(&twobox_with(twobox_layout()), Some("West")).unwrap();
    assert_eq!(g.lines, [line("w1", 0.0, 0.0, 100.0, 0.0), line("w2", 100.0, 0.0, 200.0, 0.0)]);
    assert_eq!(
        g.points,
        [PointsGeom {
            node: "P".into(),
            x: 207.5,
            y: 0.0,
            toe: Some([200.0, 0.0]),
            normal: Some([215.0, 0.0]),
            reverse: Some([215.0, 10.0]),
        }],
        "each leg ends where the next drawn line starts"
    );
    let signals: Vec<(&str, Option<[f64; 2]>)> = g.signals.iter().map(|s| (s.signal.as_str(), s.facing)).collect();
    assert_eq!(signals, [("W1", Some([100.0, 0.0])), ("A", Some([100.0, 0.0])), ("W2", Some([-100.0, 0.0]))]);
    assert_eq!(
        g.signals[1],
        SignalGeom { signal: "A".into(), x: 200.0, y: -5.0, berth_x: 190.0, berth_y: -15.0, facing: Some([100.0, 0.0]) }
    );
    assert!(g.platforms.is_empty(), "EST and NST are on East's track, not in West's fringe");
    assert_eq!(g.labels, [LabelGeom { text: "West".into(), x: 50.0, y: -30.0, arrow: None }]);
    assert_eq!(
        g.nodes,
        [node("W", 0.0, 0.0), node("E", 400.0, 0.0), node("N", 400.0, 60.0)],
        "the exits of West's routes and its boundary berth, found along the track"
    );
}

#[test]
fn a_spectator_sees_the_whole_drawing_and_labels_near_it() {
    let g = geometry_for(&twobox_with(twobox_layout()), None).unwrap();
    let lines: Vec<&str> = g.lines.iter().map(|l| l.segment.as_str()).collect();
    assert_eq!(lines, ["w1", "w2", "e", "n"], "unknown segments are dropped");
    assert_eq!(g.points.len(), 1, "a joint named as points is dropped");
    let signals: Vec<&str> = g.signals.iter().map(|s| s.signal.as_str()).collect();
    assert_eq!(signals, ["W1", "A", "W2", "C", "D"]);
    assert_eq!(g.signals[4].facing, Some([-185.0, -50.0]));
    assert_eq!(
        g.platforms[0],
        PlatformGeom { place: "EST".into(), platform: "1".into(), x1: 300.0, y1: -8.0, x2: 360.0, y2: -3.0 }
    );
    assert_eq!(g.platforms.len(), 2);
    let labels: Vec<&str> = g.labels.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(labels, ["West", "East"]);
}

#[test]
fn a_signal_on_an_undrawn_segment_faces_along_the_line_behind_it() {
    let mut layout = twobox_layout();
    layout["lines"].as_array_mut().unwrap().retain(|l| l["segment"] != "w2");
    let g = geometry_for(&twobox_with(layout), Some("West")).unwrap();
    let a = g.signals.iter().find(|s| s.signal == "A").unwrap();
    assert_eq!(a.facing, Some([100.0, 0.0]), "from w1, the line in rear of A");
    let w2 = g.signals.iter().find(|s| s.signal == "W2").unwrap();
    assert_eq!(w2.facing, Some([-185.0, 0.0]), "through P's undrawn legs to e, the nearest line in rear");
}

#[test]
fn no_or_unreadable_layout_means_no_geometry() {
    assert_eq!(geometry_for(&twobox(), Some("West")), None);
    assert_eq!(geometry_for(&twobox_with(json!({"lines": 5})), Some("West")), None);
    assert_eq!(geometry_for(&twobox_with(json!("a picture")), None), None);
    let empty = geometry_for(&twobox_with(json!({})), None).unwrap();
    assert_eq!(empty, Geometry::default());
}

#[test]
fn the_game_sends_geometry_with_every_layout() {
    let mut g = Game::new(twobox_with(twobox_layout()), GameMeta { layout: "twobox".into(), seed: 1 });
    let out = join(&mut g, "alice", Some("East"));
    let layouts: Vec<&protocol::Layout> = out
        .iter()
        .filter_map(|(_, m)| match m {
            ServerMsg::Layout(l) => Some(l),
            _ => None,
        })
        .collect();
    assert_eq!(layouts.len(), 2, "one on connect, one on claim");
    assert_eq!(layouts[0].geometry.as_ref().unwrap().signals.len(), 5, "the spectator's");
    let east = layouts[1].geometry.as_ref().unwrap();
    let signals: Vec<&str> = east.signals.iter().map(|s| s.signal.as_str()).collect();
    assert_eq!(signals, ["A", "W2", "C", "D"]);
    assert_eq!(g.layout_of("alice").unwrap().geometry.as_ref(), Some(east));
}

/// Every signal, points and route exit a player of converted Liverpool
/// Street sees is drawn: signals face a way, points have all three legs.
#[test]
fn liverpool_street_draws_everything_each_player_sees() {
    let w = World::from_json(&liverpool_json()).unwrap();
    let m = AreaMap::new(&w);
    let geo = WorldGeometry::from_world(&w).expect("ts2-import writes a layout");
    let mut visions: Vec<Visibility> = (0..w.net.areas.len())
        .map(|a| Visibility::of_area(&w, &m, signalbox_core::ids::AreaId::from_idx(a)))
        .collect();
    visions.push(Visibility::spectator(&w, &m));
    for vis in &visions {
        let l = build_layout(&w, &m, vis, "ann", Some(&geo));
        let g = l.geometry.as_ref().unwrap();
        for s in &l.signals {
            let sg = g.signals.iter().find(|x| x.signal == s.name).unwrap_or_else(|| panic!("signal {} undrawn", s.name));
            assert!(sg.facing.is_some(), "signal {} faces nowhere", s.name);
        }
        for p in &l.points {
            let pg = g.points.iter().find(|x| x.node == p.name).unwrap_or_else(|| panic!("points {} undrawn", p.name));
            assert!(pg.toe.is_some() && pg.normal.is_some() && pg.reverse.is_some(), "{pg:?}");
        }
        for r in &l.routes {
            if let protocol::ExitName::Node(n) = &r.exit {
                assert!(g.nodes.iter().any(|x| x.node == *n), "exit {n} of {} has no position", r.name);
            }
        }
        assert!(!g.lines.is_empty() && !g.labels.is_empty());
        let bytes = serde_json::to_string(&l).unwrap().len();
        assert!(bytes < 512 * 1024, "{bytes} bytes");
    }
}
