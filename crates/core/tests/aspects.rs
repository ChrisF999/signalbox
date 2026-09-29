mod common;

use common::*;
use serde_json::json;
use signalbox_core::aspect::Aspect::*;
use signalbox_core::events::Event;

fn plain_with_both_routes() -> Rig {
    let mut rig = Rig::new("plain_line");
    let empty = rig.empty();
    rig.set("S1-S2", &empty).unwrap();
    rig.set("S2-E", &empty).unwrap();
    rig.run(0.1, &empty);
    rig
}

#[test]
fn signals_are_red_without_routes() {
    let mut rig = Rig::new("plain_line");
    let empty = rig.empty();
    assert_eq!(rig.aspect("S1", &empty), Red);
    assert_eq!(rig.aspect("S2", &empty), Red);
}

#[test]
fn exit_to_boundary_shows_green_and_rear_signal_follows() {
    let mut rig = plain_with_both_routes();
    let empty = rig.empty();
    assert_eq!(rig.aspect("S2", &empty), Green);
    assert_eq!(rig.aspect("S1", &empty), Green, "overlap taken over by S2-E still counts");
}

#[test]
fn signal_before_a_red_shows_yellow() {
    let mut rig = Rig::new("plain_line");
    let empty = rig.empty();
    rig.set("S1-S2", &empty).unwrap();
    rig.run(0.1, &empty);
    assert_eq!(rig.aspect("S1", &empty), Yellow);
    assert_eq!(rig.aspect("S2", &empty), Red);
}

#[test]
fn two_aspect_signal_shows_green_before_a_red() {
    let w = load_with("plain_line", |v| v["signals"][0]["aspects"] = json!(2)).unwrap();
    let mut rig = Rig::from_world(w);
    let empty = rig.empty();
    rig.set("S1-S2", &empty).unwrap();
    rig.run(0.1, &empty);
    assert_eq!(rig.aspect("S1", &empty), Green);
}

#[test]
fn occupied_path_or_overlap_keeps_signal_red() {
    let mut rig = Rig::new("plain_line");
    let empty = rig.empty();
    rig.set("S1-S2", &empty).unwrap();
    rig.run(0.1, &empty);
    assert_eq!(rig.aspect("S1", &occ(&rig.w, &["TB"], true)), Red);
    let rig_w = rig.w.clone();
    assert_eq!(rig.aspect("S1", &occ(&rig_w, &["TC"], true)), Red);
}

#[test]
fn signal_stays_red_until_points_detected() {
    let mut rig = Rig::new("terminus");
    let empty = rig.empty();
    rig.set("S1-E2", &empty).unwrap();
    rig.run(1.0, &empty);
    assert_eq!(rig.aspect("S1", &empty), Red);
    rig.run(5.0, &empty);
    assert_eq!(rig.aspect("S1", &empty), Yellow, "exit is a buffer stop");
}

#[test]
fn refresh_reports_only_changes() {
    let mut rig = Rig::new("plain_line");
    let empty = rig.empty();
    assert!(rig.il.refresh_aspects(&rig.w, &rig.pts, &empty).is_empty());
    rig.set("S1-S2", &empty).unwrap();
    rig.run(0.1, &empty);
    let ev = rig.il.refresh_aspects(&rig.w, &rig.pts, &empty);
    assert_eq!(ev, vec![Event::SignalAspect { signal: sig(&rig.w, "S1"), aspect: Yellow }]);
    assert!(rig.il.refresh_aspects(&rig.w, &rig.pts, &empty).is_empty());
}
