mod common;

use common::*;
use serde_json::json;
use signalbox_core::events::Event;
use signalbox_core::ids::*;
use signalbox_core::network::{Dir, PointsPos, PointsView, Position};
use signalbox_core::sim::Sim;
use signalbox_core::timetable::EntryStart;
use signalbox_core::trains::Train;
use signalbox_core::world::LoadError;

struct Normal;

impl PointsView for Normal {
    fn position(&self, _: NodeId) -> Option<PointsPos> {
        Some(PointsPos::Normal)
    }
}

#[test]
fn entry_at_a_position_loads() {
    let w = load_with("terminus", |v| {
        v["entries"][0] = json!({"service": "1A01", "at": {"segment": "p1", "offset_m": 245, "direction": "up"}, "time": "06:00"});
    })
    .unwrap();
    let e = w.entries.iter().find(|e| e.service == w.service("1A01").unwrap()).unwrap();
    assert_eq!(e.start, EntryStart::At(Position { segment: seg(&w, "p1"), offset_m: 245.0, dir: Dir::Up }));
}

#[test]
fn entry_needs_exactly_one_start() {
    let both = load_with("terminus", |v| {
        v["entries"][0]["at"] = json!({"segment": "p1", "offset_m": 10, "direction": "up"});
    });
    assert!(matches!(both, Err(LoadError::Other(_))), "{both:?}");
    let neither = load_with("terminus", |v| {
        v["entries"][0].as_object_mut().unwrap().remove("boundary");
    });
    assert!(matches!(neither, Err(LoadError::Other(_))), "{neither:?}");
    let off = load_with("terminus", |v| {
        v["entries"][0] = json!({"service": "1A01", "at": {"segment": "p1", "offset_m": 900, "direction": "up"}, "time": "06:00"});
    });
    assert!(matches!(off, Err(LoadError::Other(_))), "{off:?}");
}

#[test]
fn placed_train_is_laid_back_along_the_track() {
    let w = world("terminus");
    let at = Position { segment: seg(&w, "app"), offset_m: 10.0, dir: Dir::Up };
    let t = Train::placed(TrainId(0), ServiceId(0), "X", TrainTypeId(0), 100.0, at, 0.0, &w.net, &Normal);
    assert_eq!(t.head(), (seg(&w, "app"), Dir::Up));
    assert_eq!(t.head_m, 10.0);
    assert_eq!(t.segments().collect::<Vec<_>>(), vec![seg(&w, "in"), seg(&w, "app")]);
    assert_eq!(t.off_network_m(&w.net), 0.0);
}

#[test]
fn entry_at_position_arrives_and_departs() {
    let w = load_with("terminus", |v| {
        v["entries"][0] = json!({"service": "1A01", "at": {"segment": "p1", "offset_m": 245, "direction": "up"}, "time": "05:00"});
    })
    .unwrap();
    let mut sim = Sim::new(w, 1);
    let ev = run_until(&mut sim, 5.0, |e| matches!(e, Event::TrainArrived { .. }));
    assert_eq!(count(&ev, |e| matches!(e, Event::TrainEntered { .. })), 1);
    assert_eq!(sim.trains()[0].head().0, seg(sim.world(), "p1"));
    run_until(&mut sim, 900.0, |e| matches!(e, Event::TrainDeparted { .. }));
    assert!(sim.now_s() >= 6.0 * 3600.0 + 8.0 * 60.0);
}

#[test]
fn entries_at_the_same_spot_wait_for_each_other() {
    let w = load_with("terminus", |v| {
        v["entries"][0] = json!({"service": "1A01", "at": {"segment": "p1", "offset_m": 245, "direction": "up"}, "time": "06:00"});
        v["entries"][1] = json!({"service": "1A03", "at": {"segment": "p1", "offset_m": 245, "direction": "up"}, "time": "06:00"});
    })
    .unwrap();
    let mut sim = Sim::new(w, 1);
    let ev = sim.run_for(60.0);
    assert_eq!(count(&ev, |e| matches!(e, Event::TrainEntered { .. })), 1);
    assert_eq!(count(&ev, |e| matches!(e, Event::Collision { .. })), 0);
}

/// Terminus rebuilt so platform 1 hangs directly off P's normal leg, split in
/// two sections, with signal S8 in rear of S3; route S8-S3 has no overlap and
/// the track beyond S3 enters P from its normal leg.
#[test]
fn exit_signal_just_before_trailing_points_loads() {
    let w = load_with("terminus", |v| {
        v["sections"].as_array_mut().unwrap().push(json!({"name": "TP1B", "area": "Box"}));
        let nodes = v["nodes"].as_array_mut().unwrap();
        nodes.retain(|n| n["name"] != "X1");
        nodes.push(json!({"name": "Y", "kind": "joint"}));
        for n in nodes.iter_mut() {
            if n["name"] == "P" {
                n["normal"] = json!("p1a");
            }
        }
        let segs = v["segments"].as_array_mut().unwrap();
        segs.retain(|s| s["name"] != "n" && s["name"] != "p1");
        segs.push(json!({"name": "p1a", "from": "P", "to": "Y", "length_m": 125, "line_speed_kmh": 30, "section": "TP1"}));
        segs.push(json!({"name": "p1b", "from": "Y", "to": "E1", "length_m": 125, "line_speed_kmh": 30, "section": "TP1B"}));
        v["signals"][1] = json!({"name": "S3", "area": "Box", "segment": "p1a", "offset_m": 0, "direction": "down", "aspects": 3});
        v["signals"].as_array_mut().unwrap().push(json!({"name": "S8", "area": "Box", "segment": "p1b", "offset_m": 0, "direction": "down", "aspects": 3}));
        v["platforms"][0] = json!({"place": "TRM", "platform": "1", "segment": "p1b", "from_m": 0, "to_m": 125});
        v["routes"][0]["path"] = json!(["TP", "TP1", "TP1B"]);
        v["routes"].as_array_mut().unwrap().push(json!({"entrance": "S8", "exit": {"kind": "signal", "name": "S3"}, "path": ["TP1"]}));
    });
    let w = w.expect("route S8-S3 must load");
    assert!(w.route_by_name("S8-S3").is_some());
}
