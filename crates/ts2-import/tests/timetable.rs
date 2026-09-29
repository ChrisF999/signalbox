use std::collections::BTreeSet;

use signalbox_core::network::Dir;
use signalbox_core::world::file::EndFile;
use ts2_import::report::{self, Report};
use ts2_import::ts2::Ts2;
use ts2_import::{graph, timetable};

fn load(name: &str) -> Ts2 {
    let path = format!("{}/tests/data/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn convert(name: &str) -> (timetable::Timetable, Report) {
    let t = load(name);
    let mut r = Report::default();
    let ends = timetable::entry_ends(&t);
    let g = graph::build(&t, &ends, &mut r).unwrap();
    (timetable::build(&t, &g, &mut r), r)
}

#[test]
fn mini_timetable() {
    let (tt, r) = convert("mini");
    assert_eq!(tt.train_types.len(), 1);
    assert!((tt.train_types[0].max_speed_kmh - 72.0).abs() < 1e-9);
    let s = &tt.services[0];
    assert!(matches!(s.end, EndFile::Stable), "REVERSE alone stables");
    assert_eq!(s.calls[0].arr.as_deref(), Some("06:05:00"));
    assert_eq!(s.calls[0].dep, None);
    assert_eq!(tt.entries[0].boundary.as_deref(), Some("N1"));
    assert_eq!(tt.entries[0].time, "06:00:00");
    assert_eq!(tt.options.start_time, "06:00:00");
    assert_eq!(tt.options.min_dwell_s, [20, 120]);
    assert_eq!(r.count(report::DELAY), 1, "two dwell bands merged");
}

#[test]
fn entry_ends_are_the_ends_trains_start_from() {
    assert_eq!(timetable::entry_ends(&load("mini")), BTreeSet::from(["1".to_string()]));
    assert!(timetable::entry_ends(&load("drain")).is_empty(), "drain trains start in platforms");
}

#[test]
fn drain_trains_start_in_their_platforms() {
    let (tt, _) = convert("drain");
    assert_eq!(tt.entries.len(), 3);
    for e in &tt.entries {
        let at = e.at.as_ref().expect("positional start");
        assert!(at.segment.starts_with('L'));
        assert!(matches!(at.direction, Dir::Up | Dir::Down));
        assert!(e.boundary.is_none());
    }
}

#[test]
fn chains_become_form_and_exit() {
    let (tt, r) = convert("liverpool-st");
    let forms = tt.services.iter().filter(|s| matches!(s.end, EndFile::Form { .. })).count();
    assert_eq!(forms, 681);
    let exits = tt.services.iter().filter(|s| matches!(s.end, EndFile::Exit)).count();
    assert!(exits >= 691, "{exits}");
    assert_eq!(r.count(report::FORM_NO_REVERSE), 0);
}

#[test]
fn calls_at_missing_tracks_are_dropped() {
    let (tt, r) = convert("liverpool-st");
    assert_eq!(r.count(report::CALL_DROPPED), 207);
    assert!(tt.services.iter().all(|s| s.calls.iter().all(|c| !(c.place == "BTHNLGR" && c.platform.as_deref() == Some("FL_DN")))));
}

#[test]
fn gretz_rename_without_reverse_is_warned() {
    let (_, r) = convert("gretz-armainvilliers");
    assert_eq!(r.count(report::FORM_NO_REVERSE), 1);
}

/// Convert `mini` after `edit`ing its JSON: the conversion must succeed.
fn convert_edited(edit: impl FnOnce(&mut serde_json::Value)) -> ts2_import::Conversion {
    let path = format!("{}/tests/data/mini.json", env!("CARGO_MANIFEST_DIR"));
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    edit(&mut v);
    ts2_import::convert(&v.to_string()).expect("bad timetable input must warn, not fail")
}

#[test]
fn huge_initial_delay_skips_the_train() {
    let c = convert_edited(|v| v["trains"][0]["initialDelay"] = serde_json::json!(i64::MAX));
    assert!(c.world.entries.is_empty());
    assert_eq!(c.report.count(report::TRAIN_SKIPPED), 1);
}

#[test]
fn delay_past_48_hours_skips_the_train() {
    let c = convert_edited(|v| v["trains"][0]["initialDelay"] = serde_json::json!(48 * 3600));
    assert!(c.world.entries.is_empty());
    assert_eq!(c.report.count(report::TRAIN_SKIPPED), 1);
}

#[test]
fn unknown_train_type_skips_the_service_and_its_trains() {
    let c = convert_edited(|v| v["services"]["S1"]["plannedTrainType"] = serde_json::json!("NOPE"));
    assert!(c.world.services.is_empty());
    assert!(c.world.entries.is_empty());
    assert_eq!(c.report.count(report::SERVICE_SKIPPED), 1);
    assert_eq!(c.report.count(report::TRAIN_SKIPPED), 1);
}

#[test]
fn malformed_call_time_drops_the_call() {
    let c = convert_edited(|v| v["services"]["S1"]["lines"][0]["scheduledArrivalTime"] = serde_json::json!("6pm"));
    assert!(c.world.services[0].calls.is_empty());
    assert_eq!(c.report.count(report::CALL_DROPPED), 1);
}

#[test]
fn duplicate_train_type_code_keeps_the_first() {
    let c = convert_edited(|v| {
        let t = v["trainTypes"]["T"].clone();
        v["trainTypes"]["U"] = t;
    });
    assert_eq!(c.world.train_types.len(), 1);
    assert_eq!(c.report.count(report::TRAIN_TYPE_SKIPPED), 1);
}

#[test]
fn non_positive_train_type_numbers_skip_the_type_and_its_services() {
    let c = convert_edited(|v| v["trainTypes"]["T"]["length"] = serde_json::json!(-5.0));
    assert!(c.world.train_types.is_empty());
    assert!(c.world.services.is_empty());
    assert_eq!(c.report.count(report::TRAIN_TYPE_SKIPPED), 1);
    assert_eq!(c.report.count(report::SERVICE_SKIPPED), 1);
    let c = convert_edited(|v| v["trainTypes"]["T"]["maxSpeed"] = serde_json::json!(0.0));
    assert!(c.world.train_types.is_empty());
}

#[test]
fn negative_initial_speed_skips_the_train() {
    let c = convert_edited(|v| v["trains"][0]["initialSpeed"] = serde_json::json!(-1.0));
    assert!(c.world.entries.is_empty());
    assert_eq!(c.report.count(report::TRAIN_SKIPPED), 1);
}

#[test]
fn form_into_a_skipped_service_stables_instead() {
    let c = convert_edited(|v| {
        let mut s2 = v["services"]["S1"].clone();
        s2["serviceCode"] = serde_json::json!("S2");
        s2["plannedTrainType"] = serde_json::json!("NOPE");
        v["services"]["S2"] = s2;
        v["services"]["S1"]["postActions"] = serde_json::json!([
            {"actionCode": "SET_SERVICE", "actionParam": "S2"},
            {"actionCode": "REVERSE", "actionParam": null}
        ]);
    });
    assert_eq!(c.world.services.len(), 1);
    assert!(matches!(c.world.services[0].end, EndFile::Stable));
    assert_eq!(c.report.count(report::ACTION), 1);
}
