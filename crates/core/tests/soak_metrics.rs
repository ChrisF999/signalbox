mod common;

use common::*;
use serde_json::json;
use signalbox_core::robot::soak;
use signalbox_core::sim::Sim;

#[test]
fn train_with_no_route_is_reported_stuck_and_the_next_waits_at_the_fringe() {
    let w = load_with("plain_line", |v| {
        v["routes"] = json!([]);
        v["services"].as_array_mut().unwrap().push(json!({"headcode": "2A02", "train_type": "EMU"}));
        v["entries"].as_array_mut().unwrap().push(json!({"service": "2A02", "boundary": "W", "time": "06:00:00"}));
    })
    .unwrap();
    let mut sim = Sim::new(w, 1);
    let r = soak(&mut sim, 2100.0);
    assert_eq!(r.stuck, vec!["2A01".to_string()]);
    assert!(r.max_fringe_wait_s >= 2000.0, "{r:?}");
}

#[test]
fn clean_soak_has_no_stuck_trains() {
    let mut sim = Sim::new(world("junction"), 42);
    let r = soak(&mut sim, 3600.0);
    assert!(r.stuck.is_empty(), "{r:?}");
    assert!(r.max_fringe_wait_s < 1800.0, "{r:?}");
}
