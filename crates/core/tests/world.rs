mod common;

use common::*;
use serde_json::json;
use signalbox_core::network::NodeKind;
use signalbox_core::world::LoadError;

#[test]
fn fixtures_load() {
    let w = world("plain_line");
    assert_eq!(w.title, "Plain line");
    assert_eq!(w.net.segments.len(), 3);
    assert_eq!(w.net.signals.len(), 2);
    let t = world("terminus");
    assert!(matches!(t.net.nodes[node(&t, "P").idx()].kind, NodeKind::Points { .. }));
    assert_eq!(t.net.platforms.len(), 2);
}

#[test]
fn line_speed_is_converted_to_metres_per_second() {
    let w = world("plain_line");
    let a = &w.net.segments[seg(&w, "a").idx()];
    assert!((a.line_speed - 100.0 / 3.6).abs() < 1e-9);
}

#[test]
fn nodes_know_their_segments_and_sections_theirs() {
    let w = world("plain_line");
    assert_eq!(w.net.nodes[node(&w, "J1").idx()].segments, vec![seg(&w, "a"), seg(&w, "b")]);
    assert_eq!(w.net.sections[sec(&w, "TB").idx()].segments, vec![seg(&w, "b")]);
    assert_eq!(w.net.signals_on[seg(&w, "a").idx()], vec![sig(&w, "S1")]);
}

#[test]
fn berth_links_to_signal() {
    let w = world("plain_line");
    assert_eq!(w.net.signals[sig(&w, "S1").idx()].berth, w.net.berth("B1"));
    assert_eq!(w.net.boundary_berth(node(&w, "W")), w.net.berth("BW"));
}

#[test]
fn points_section_is_the_toe_segments_section() {
    let w = world("terminus");
    assert_eq!(w.net.points_section(node(&w, "P")), Some(sec(&w, "TP")));
    assert_eq!(w.net.points_section(node(&w, "J")), None);
    assert_eq!(w.net.swing_s(node(&w, "P")), 5.0);
}

#[test]
fn rejects_wrong_schema() {
    let e = load_with("plain_line", |v| v["schema"] = json!(99)).unwrap_err();
    assert_eq!(e, LoadError::Schema(99));
}

#[test]
fn rejects_bad_json() {
    assert!(matches!(signalbox_core::world::World::from_json("{"), Err(LoadError::Json(_))));
}

#[test]
fn rejects_duplicate_segment() {
    let e = load_with("plain_line", |v| {
        let dup = v["segments"][0].clone();
        v["segments"].as_array_mut().unwrap().push(dup);
    })
    .unwrap_err();
    assert_eq!(e, LoadError::Duplicate { kind: "segment", name: "a".into() });
}

#[test]
fn rejects_unknown_section() {
    let e = load_with("plain_line", |v| v["segments"][0]["section"] = json!("NOPE")).unwrap_err();
    assert_eq!(e, LoadError::UnknownRef { kind: "section", name: "NOPE".into(), from: "a".into() });
}

#[test]
fn rejects_joint_with_one_segment() {
    let e = load_with("plain_line", |v| {
        v["segments"].as_array_mut().unwrap().pop();
    })
    .unwrap_err();
    assert!(matches!(e, LoadError::BadNode { ref node, .. } if node == "J2"), "{e:?}");
}

#[test]
fn rejects_empty_section() {
    let e = load_with("plain_line", |v| {
        v["sections"].as_array_mut().unwrap().push(json!({"name": "TX", "area": "Main"}));
    })
    .unwrap_err();
    assert_eq!(e, LoadError::EmptySection("TX".into()));
}

#[test]
fn rejects_signal_off_track() {
    let e = load_with("plain_line", |v| v["signals"][0]["offset_m"] = json!(5000)).unwrap_err();
    assert_eq!(e, LoadError::SignalOffTrack("S1".into()));
}

#[test]
fn rejects_unanchored_berth() {
    let e = load_with("plain_line", |v| {
        v["berths"].as_array_mut().unwrap().push(json!({"name": "BX"}));
    })
    .unwrap_err();
    assert_eq!(e, LoadError::BerthUnanchored("BX".into()));
}

#[test]
fn rejects_bad_platform_extent() {
    let e = load_with("terminus", |v| v["platforms"][0]["to_m"] = json!(900)).unwrap_err();
    assert!(matches!(e, LoadError::Other(_)), "{e:?}");
}

#[test]
fn rejects_non_positive_segment_length_and_speed() {
    for bad in [0.0, -5.0] {
        let e = load_with("plain_line", |v| v["segments"][0]["length_m"] = json!(bad)).unwrap_err();
        assert!(matches!(e, LoadError::Other(ref m) if m.contains("segment `a`")), "{e:?}");
        let e = load_with("plain_line", |v| v["segments"][0]["line_speed_kmh"] = json!(bad)).unwrap_err();
        assert!(matches!(e, LoadError::Other(ref m) if m.contains("segment `a`")), "{e:?}");
    }
}

#[test]
fn rejects_negative_sighting_and_swing() {
    let e = load_with("plain_line", |v| v["signals"][0]["sighting_m"] = json!(-1)).unwrap_err();
    assert!(matches!(e, LoadError::Other(ref m) if m.contains("signal `S1`")), "{e:?}");
    let e = load_with("terminus", |v| v["nodes"][2]["swing_s"] = json!(-1)).unwrap_err();
    assert!(matches!(e, LoadError::Other(ref m) if m.contains("points `P`")), "{e:?}");
}
