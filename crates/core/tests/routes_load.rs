mod common;

use common::*;
use serde_json::json;
use signalbox_core::network::PointsPos;
use signalbox_core::routes::Exit;
use signalbox_core::timetable::EndAction;
use signalbox_core::world::LoadError;

#[test]
fn routes_resolve() {
    let w = world("terminus");
    assert_eq!(w.routes.len(), 4);
    let r = w.find_route(sig(&w, "S1"), Exit::Node(node(&w, "E1"))).unwrap();
    assert_eq!(r, route(&w, "S1-E1"));
    let def = &w.routes[r.idx()];
    assert_eq!(def.path, vec![sec(&w, "TP"), sec(&w, "TP1")]);
    assert_eq!(def.points, vec![(node(&w, "P"), PointsPos::Normal)]);
    assert!(def.overlap.is_empty());
    assert_eq!(w.routes_from[sig(&w, "S1").idx()].len(), 2);
}

#[test]
fn plain_line_route_has_overlap() {
    let w = world("plain_line");
    let def = &w.routes[route(&w, "S1-S2").idx()];
    assert_eq!(def.exit, Exit::Signal(sig(&w, "S2")));
    assert_eq!(def.overlap, vec![sec(&w, "TC")]);
}

#[test]
fn services_and_entries_resolve() {
    let w = world("terminus");
    let a01 = &w.services[w.service("1A01").unwrap().idx()];
    assert_eq!(a01.end, EndAction::Form(w.service("1A02").unwrap()));
    assert_eq!(a01.calls[0].arr_s, Some((6 * 3600 + 5 * 60) as f64));
    assert_eq!(a01.calls[0].platform.as_deref(), Some("1"));
    assert!(a01.calls[0].stop);
    assert_eq!(w.entries.len(), 2);
    assert!(w.entries[0].time_s < w.entries[1].time_s);
    assert_eq!(w.entries[0].start, signalbox_core::timetable::EntryStart::Boundary(node(&w, "W")));
    assert!((w.train_types[0].max_speed - 120.0 / 3.6).abs() < 1e-9);
}

#[test]
fn options_defaults() {
    let w = world("terminus");
    assert_eq!(w.options.start_s, 6.0 * 3600.0);
    assert_eq!(w.options.overlap_release_s, 60.0);
    assert_eq!(w.options.approach_lock_s, 120.0);
    assert_eq!(w.options.min_dwell_s, (30, 30));
    assert_eq!(w.options.entry_delay_s, (0, 0));
}

#[test]
fn rejects_non_contiguous_route() {
    let e = load_with("plain_line", |v| v["routes"][0]["path"] = json!(["TA", "TC"])).unwrap_err();
    assert!(matches!(e, LoadError::BadRoute { ref route, .. } if route == "S1-S2"), "{e:?}");
}

#[test]
fn rejects_route_missing_points() {
    let e = load_with("terminus", |v| v["routes"][0]["points"] = json!([])).unwrap_err();
    assert!(matches!(e, LoadError::BadRoute { ref route, .. } if route == "S1-E1"), "{e:?}");
}

#[test]
fn rejects_exit_on_a_joint() {
    let e = load_with("plain_line", |v| v["routes"][1]["exit"] = json!({"kind": "node", "name": "J2"})).unwrap_err();
    assert!(matches!(e, LoadError::BadRoute { .. }), "{e:?}");
}

#[test]
fn rejects_duplicate_route() {
    let e = load_with("plain_line", |v| {
        let dup = v["routes"][0].clone();
        v["routes"].as_array_mut().unwrap().push(dup);
    })
    .unwrap_err();
    assert_eq!(e, LoadError::Duplicate { kind: "route", name: "S1-S2".into() });
}

#[test]
fn rejects_form_without_a_stopping_call() {
    let e = load_with("terminus", |v| v["services"][0]["calls"] = json!([])).unwrap_err();
    assert!(matches!(e, LoadError::Other(_)), "{e:?}");
}

#[test]
fn rejects_call_at_unknown_platform() {
    let e = load_with("terminus", |v| v["services"][0]["calls"][0]["platform"] = json!("9")).unwrap_err();
    assert!(matches!(e, LoadError::Other(_)), "{e:?}");
}

#[test]
fn rejects_entry_at_non_boundary() {
    let e = load_with("terminus", |v| v["entries"][0]["boundary"] = json!("E1")).unwrap_err();
    assert!(matches!(e, LoadError::Other(_)), "{e:?}");
}

#[test]
fn rejects_bad_time_and_bad_ranges() {
    let e = load_with("terminus", |v| v["entries"][0]["time"] = json!("6am")).unwrap_err();
    assert!(matches!(e, LoadError::Other(_)), "{e:?}");
    let e = load_with("terminus", |v| v["options"]["min_dwell_s"] = json!([60, 30])).unwrap_err();
    assert!(matches!(e, LoadError::Other(_)), "{e:?}");
}

fn bad_route(e: LoadError) -> String {
    match e {
        LoadError::BadRoute { problem, .. } => problem,
        other => panic!("expected BadRoute, got {other:?}"),
    }
}

#[test]
fn rejects_path_that_skips_the_points_section() {
    let e = load_with("junction", |v| v["routes"][0]["path"] = json!(["TE"])).unwrap_err();
    assert!(matches!(e, LoadError::BadRoute { .. }), "{e:?}");
}

#[test]
fn rejects_points_set_against_the_declared_path() {
    let e = load_with("junction", |v| v["routes"][0]["points"][0]["position"] = json!("reverse")).unwrap_err();
    assert!(matches!(e, LoadError::BadRoute { .. }), "{e:?}");
}

#[test]
fn rejects_signal_in_the_middle_of_a_section() {
    let e = load_with("plain_line", |v| v["signals"][0]["offset_m"] = json!(500)).unwrap_err();
    assert!(bad_route(e).contains("section boundary"));
}

#[test]
fn rejects_points_required_outside_path_and_overlap() {
    let e = load_with("junction", |v| v["routes"][2]["path"] = json!(["TW"])).unwrap_err();
    assert!(bad_route(e).contains("neither path nor overlap"));
}

#[test]
fn rejects_overlap_on_a_node_exit() {
    let e = load_with("plain_line", |v| v["routes"][1]["overlap"] = json!(["TA"])).unwrap_err();
    assert!(bad_route(e).contains("cannot have an overlap"));
}

#[test]
fn rejects_self_looping_segment() {
    let e = load_with("plain_line", |v| v["segments"][1]["to"] = json!("J1")).unwrap_err();
    assert!(matches!(e, LoadError::Other(_)), "{e:?}");
}

#[test]
fn rejects_bad_train_types_entries_and_options() {
    for field in ["max_speed_kmh", "accel", "service_brake", "emergency_brake", "length_m"] {
        for bad in [0.0, -1.0] {
            let e = load_with("terminus", |v| v["train_types"][0][field] = json!(bad)).unwrap_err();
            assert!(matches!(e, LoadError::Other(ref m) if m.contains(field)), "{field} {bad}: {e:?}");
        }
    }
    let e = load_with("terminus", |v| v["entries"][0]["speed_kmh"] = json!(-5.0)).unwrap_err();
    assert!(matches!(e, LoadError::Other(ref m) if m.contains("speed_kmh")), "{e:?}");
    for field in ["overlap_release_s", "approach_lock_s"] {
        let e = load_with("terminus", |v| v["options"][field] = json!(-1.0)).unwrap_err();
        assert!(matches!(e, LoadError::Other(ref m) if m.contains(field)), "{field}: {e:?}");
    }
}

#[test]
fn rejects_clashing_automatic_routes() {
    let e = load_with("junction", |v| {
        v["routes"][0]["automatic"] = json!(true);
        v["routes"][1]["automatic"] = json!(true);
    })
    .unwrap_err();
    assert!(bad_route(e).contains("both use section"));
}

#[test]
fn continuing_automatic_routes_may_share_an_overlap() {
    load_with("plain_line", |v| {
        v["routes"][0]["automatic"] = json!(true);
        v["routes"][1]["automatic"] = json!(true);
    })
    .unwrap();
}

/// Junction with a new signal `X` behind `A`; automatic route `X-A` overlaps over
/// the points normal, and automatic `A-N` (which continues it) needs them reverse.
fn continuing_pair_disagreeing() -> Result<signalbox_core::world::World, LoadError> {
    load_with("junction", |v| {
        v["sections"].as_array_mut().unwrap().push(json!({"name": "TW0", "area": "Jn"}));
        v["nodes"].as_array_mut().unwrap().push(json!({"name": "J0", "kind": "joint"}));
        v["segments"][0] = json!({"name": "w0", "from": "W", "to": "J0", "length_m": 1000, "line_speed_kmh": 100, "section": "TW0"});
        v["segments"].as_array_mut().unwrap().push(
            json!({"name": "w", "from": "J0", "to": "J1", "length_m": 500, "line_speed_kmh": 100, "section": "TW"}),
        );
        v["signals"].as_array_mut().unwrap().push(
            json!({"name": "X", "area": "Jn", "segment": "w0", "offset_m": 1000, "direction": "up", "aspects": 3}),
        );
        v["signals"][0]["offset_m"] = json!(500);
        let overlap = json!({
            "entrance": "X", "exit": {"kind": "signal", "name": "A"}, "path": ["TW"],
            "overlap": ["TP", "TE"], "overlap_points": [{"points": "P", "position": "normal"}],
            "automatic": true
        });
        // The westbound routes would now run on over TW0; they play no part here.
        let a_n = json!({"entrance": "A", "exit": {"kind": "node", "name": "N"}, "path": ["TP", "TN"],
                         "points": [{"points": "P", "position": "reverse"}], "automatic": true});
        v["routes"] = json!([a_n, overlap]);
    })
}

#[test]
fn rejects_continuing_automatic_routes_that_disagree_on_points() {
    // A-N runs over P reverse, X-A's overlap wants it normal: setting both at start would fail.
    let e = continuing_pair_disagreeing().unwrap_err();
    let m = bad_route(e);
    assert!(m.contains("disagree on points"), "{m}");
}
