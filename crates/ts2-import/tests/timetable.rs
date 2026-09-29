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
