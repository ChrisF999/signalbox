//! The train list in each view (spec D1 §4.2), built from sim state.

mod common;

use std::collections::BTreeSet;

use common::*;
use game::areas::{AreaMap, Visibility};
use game::view::{DUE_WINDOW_S, build_trains, late_s};
use game::{Game, GameMeta};
use protocol::{TrainRow, TrainState};
use signalbox_core::sim::Sim;

fn states(rows: &std::collections::BTreeMap<String, TrainRow>) -> Vec<(&str, TrainState)> {
    rows.iter().map(|(h, r)| (h.as_str(), r.state)).collect()
}

#[test]
fn late_is_whole_minutes_and_never_negative() {
    assert_eq!(late_s(25_300.0, Some(25_200.0)), 60);
    assert_eq!(late_s(25_259.9, Some(25_200.0)), 0, "59.9 s is not a minute");
    assert_eq!(late_s(25_200.0, Some(25_200.0)), 0);
    assert_eq!(late_s(25_000.0, Some(25_200.0)), 0, "early");
    assert_eq!(late_s(25_000.0, None), 0);
    assert_eq!(late_s(26_000.0, Some(25_200.0)), 780);
}

/// At 07:00 nothing has entered: each side sees the entries due at its own
/// boundaries within the window, a spectator sees them all.
#[test]
fn before_anything_enters_each_area_sees_what_is_due_at_its_boundaries() {
    let w = twobox();
    let m = AreaMap::new(&w);
    let west = Visibility::of_area(&w, &m, area(&w, "West"));
    let east = Visibility::of_area(&w, &m, area(&w, "East"));
    let all = Visibility::spectator(&w, &m);
    let sim = Sim::new(w, 1);
    let rows = build_trains(&sim, &west);
    assert_eq!(states(&rows), [("1E01", TrainState::Due), ("1N02", TrainState::Due)]);
    assert_eq!(
        rows["1E01"],
        TrainRow {
            next_place: Some("EST".into()),
            next_platform: Some("1".into()),
            booked: Some(25_440.0),
            arr: Some(25_440.0),
            dep: Some(25_500.0),
            late_s: 0,
            state: TrainState::Due,
        }
    );
    assert_eq!(states(&build_trains(&sim, &east)), [("2W03", TrainState::Due), ("2W04", TrainState::Due)]);
    assert_eq!(build_trains(&sim, &east)["2W03"].next_place, None, "no calls");
    assert_eq!(build_trains(&sim, &all).len(), 4);
}

#[test]
fn the_window_is_thirty_sim_minutes() {
    assert_eq!(DUE_WINDOW_S, 1800.0);
    let mut json: serde_json::Value = serde_json::from_str(&twobox_json()).unwrap();
    json["entries"][2]["time"] = "07:31".into();
    let w = signalbox_core::world::World::from_json(&json.to_string()).unwrap();
    let m = AreaMap::new(&w);
    let west = Visibility::of_area(&w, &m, area(&w, "West"));
    let mut sim = Sim::new(w, 1);
    assert_eq!(states(&build_trains(&sim, &west)), [("1E01", TrainState::Due)], "1N02 at 07:31 is beyond 07:30");
    sim.run_for(60.0);
    assert!(build_trains(&sim, &west).contains_key("1N02"), "at 07:01 it is inside the window");
}

/// 1E01 enters at W, runs through West, is handed to East and stops at
/// EST 1. West lists it before it enters and while it is on West's track;
/// East lists it from the moment it runs, because its next call is at
/// East's platform, although West's track is not East's to see.
#[test]
fn a_train_is_due_then_in_area_then_approaching_then_at_the_platform() {
    let mut g = Game::new(twobox(), GameMeta { layout: "twobox".into(), seed: 1 });
    join(&mut g, "west", Some("West"));
    join(&mut g, "east", Some("East"));
    let first = |p: &str| g.view_of(p).unwrap().trains.get("1E01").map(|r| r.state);
    assert_eq!((first("west"), first("east")), (Some(TrainState::Due), None), "it enters at West's boundary");
    let mut seen_west: BTreeSet<TrainState> = BTreeSet::from([TrainState::Due]);
    let mut seen_east: Vec<TrainState> = Vec::new();
    for _ in 0..900 {
        play_as_robot(&mut g, "west");
        play_as_robot(&mut g, "east");
        g.advance(1.0);
        if let Some(r) = g.view_of("west").and_then(|v| v.trains.get("1E01").cloned()) {
            seen_west.insert(r.state);
        }
        if let Some(r) = g.view_of("east").and_then(|v| v.trains.get("1E01").cloned()) {
            if seen_east.last() != Some(&r.state) {
                seen_east.push(r.state);
            }
            if r.state == TrainState::AtPlatform {
                break;
            }
        }
    }
    assert_eq!(
        seen_west,
        BTreeSet::from([TrainState::Due, TrainState::Approaching, TrainState::InArea]),
        "approaching: on West's fringe with its head in East"
    );
    assert_eq!(seen_east, [TrainState::Approaching, TrainState::InArea, TrainState::AtPlatform]);
}

/// Rows follow the sim only, so a client applying deltas keeps them
/// exactly (the consistency tests in game.rs and the bot soak compare
/// whole views, trains included).
#[test]
fn trains_are_a_function_of_state() {
    let mut g = Game::new(twobox(), GameMeta { layout: "twobox".into(), seed: 1 });
    join(&mut g, "sam", None);
    let mut c = Client::default();
    c.take(&g.resync("sam"), "sam");
    let mut ever: BTreeSet<String> = BTreeSet::new();
    for _ in 0..2400 {
        let out = g.advance(0.5);
        c.take(&out, "sam");
        c.take(&g.flush(), "sam");
        assert_eq!(c.view, g.view_of("sam"));
        ever.extend(c.view.as_ref().unwrap().trains.keys().cloned());
    }
    assert_eq!(ever.len(), 4, "{ever:?}");
}

/// An entry offered at the fringe but held there (here by a 40-minute entry
/// delay; a blocked entry section holds it the same way) stays `Due` in the
/// area's list however long it waits: the 30-minute window only limits
/// entries not yet offered.
#[test]
fn an_entry_held_at_the_fringe_stays_due_past_the_window() {
    let mut json: serde_json::Value = serde_json::from_str(&twobox_json()).unwrap();
    json["options"]["entry_delay_s"] = serde_json::json!([2400, 2400]);
    let w = signalbox_core::world::World::from_json(&json.to_string()).unwrap();
    let mut g = Game::new(w, GameMeta { layout: "twobox".into(), seed: 1 });
    join(&mut g, "west", Some("West"));
    let due = |g: &Game| g.view_of("west").unwrap().trains.get("1E01").map(|r| r.state);
    run(&mut g, 60.0, 1.0);
    assert!(g.sim().pending_entries().iter().any(|p| p.entry == 0), "1E01 is offered and held");
    assert_eq!(due(&g), Some(TrainState::Due));
    run(&mut g, 1860.0, 1.0);
    assert!(g.sim().now_s() > 25_200.0 + DUE_WINDOW_S, "held for more than 30 sim minutes");
    assert!(g.sim().trains().is_empty() && g.sim().pending_entries().iter().any(|p| p.entry == 0), "still held");
    assert_eq!(due(&g), Some(TrainState::Due), "still listed, still due");
}

/// Tutorial spec §2: an on-demand entry is not due until it is offered.
#[test]
fn an_on_demand_entry_is_due_only_once_offered() {
    let mut json: serde_json::Value = serde_json::from_str(&twobox_json()).unwrap();
    json["entries"][0]["on_demand"] = true.into();
    let w = signalbox_core::world::World::from_json(&json.to_string()).unwrap();
    let m = AreaMap::new(&w);
    let west = Visibility::of_area(&w, &m, area(&w, "West"));
    let mut sim = Sim::new(w, 1);
    assert_eq!(states(&build_trains(&sim, &west)), [("1N02", TrainState::Due)]);
    sim.offer_entry(0).unwrap();
    assert_eq!(states(&build_trains(&sim, &west)), [("1E01", TrainState::Due), ("1N02", TrainState::Due)]);
}

/// A train standing at a call (here its first, booked in at 06:09 and out at
/// 07:51, as TS2 books trains that start in a platform) is not late while it
/// waits for its booked departure.
#[test]
fn a_train_standing_at_a_call_is_late_only_after_its_booked_departure() {
    let mut json: serde_json::Value = serde_json::from_str(&twobox_json()).unwrap();
    json["services"][0]["calls"] = serde_json::json!([{"place": "EST", "platform": "1", "arr": "06:09", "dep": "07:51"}]);
    json["entries"][0] = serde_json::json!({"service": "1E01", "at": {"segment": "e", "offset_m": 880, "direction": "up"}, "time": "06:00", "speed_kmh": 0});
    let w = signalbox_core::world::World::from_json(&json.to_string()).unwrap();
    let m = AreaMap::new(&w);
    let all = Visibility::spectator(&w, &m);
    let mut sim = Sim::new(w, 1);
    sim.run_for(60.0);
    assert!(sim.trains().iter().any(|t| t.headcode == "1E01" && t.dwell.is_some()), "1E01 stands in EST 1: {:?}", sim.trains());
    let row = build_trains(&sim, &all)["1E01"].clone();
    assert_eq!(row.state, TrainState::AtPlatform);
    assert_eq!(row.booked, Some(28_260.0), "booked against its departure");
    assert_eq!(row.late_s, 0, "07:01, waiting for 07:51: {row:?}");
    sim.run_for(49.0 * 60.0);
    assert_eq!(build_trains(&sim, &all)["1E01"].late_s, 0, "07:50");
}

/// Held at its platform past its booked departure (here by a 10-minute
/// minimum dwell), a train is late against the departure.
#[test]
fn a_train_held_past_its_booked_departure_is_late_against_it() {
    let mut json: serde_json::Value = serde_json::from_str(&twobox_json()).unwrap();
    json["services"][0]["calls"] = serde_json::json!([{"place": "EST", "platform": "1", "arr": "06:09", "dep": "07:00:30"}]);
    json["entries"][0] = serde_json::json!({"service": "1E01", "at": {"segment": "e", "offset_m": 880, "direction": "up"}, "time": "06:00", "speed_kmh": 0});
    json["options"]["min_dwell_s"] = serde_json::json!([600, 600]);
    let w = signalbox_core::world::World::from_json(&json.to_string()).unwrap();
    let m = AreaMap::new(&w);
    let all = Visibility::spectator(&w, &m);
    let mut sim = Sim::new(w, 1);
    sim.run_for(350.0);
    let row = build_trains(&sim, &all)["1E01"].clone();
    assert_eq!(row.state, TrainState::AtPlatform, "{row:?}");
    assert_eq!(row.booked, Some(25_230.0));
    assert_eq!(row.late_s, 300, "07:05:50 against 07:00:30: {row:?}");
}
