//! The simplifier and the headcode enquiry (realism spec §3).

mod common;

use client_core::Target;
use client_core::simplifier::{Line, enquiry, fmt_wtt, lateness, lines, rows};
use common::*;
use protocol::*;

fn call(place: &str, platform: Option<&str>, arr: Option<f64>, dep: Option<f64>, stops: bool) -> SimplifierCall {
    SimplifierCall { place: s(place), platform: platform.map(s), arr, dep, stops }
}

#[test]
fn times_are_hours_and_minutes_with_half_minutes() {
    assert_eq!(fmt_wtt(25_440.0), "07:04");
    assert_eq!(fmt_wtt(25_170.0), "06:59½");
    assert_eq!(fmt_wtt(25_199.9), "06:59½");
    assert_eq!(fmt_wtt(86_400.0 + 30.0), "00:00½", "wraps at midnight");
    for bad in [f64::NAN, -1.0, f64::INFINITY] {
        assert_eq!(fmt_wtt(bad), "00:00");
    }
}

#[test]
fn a_row_is_a_line_per_call_like_the_working_timetable() {
    let r = SimplifierRow {
        headcode: s("1A07"),
        origin: Some(s("BOWJ")),
        destination: Some(s("LIVST")),
        calls: vec![
            call("WSJ", Some("ML_UP"), None, Some(34_170.0), false),
            call("LIVST", Some("12"), Some(34_380.0), Some(34_410.0), true),
        ],
    };
    let line = |h: &str, f: &str, t: &str, p: &str, pf: &str, a: &str, d: &str| Line {
        headcode: s(h),
        from: s(f),
        to: s(t),
        place: s(p),
        platform: s(pf),
        arr: s(a),
        dep: s(d),
    };
    assert_eq!(
        lines(&r),
        [
            line("1A07", "BOWJ", "LIVST", "WSJ", "ML_UP", "pass", "09:29½"),
            line("", "", "", "LIVST", "12", "09:33", "09:33½"),
        ]
    );
    let bare = SimplifierRow { headcode: s("2W03"), origin: None, destination: None, calls: vec![] };
    assert_eq!(lines(&bare), [line("2W03", "", "", "", "", "", "")], "a service with no calls still shows");
}

#[test]
fn search_filters_by_headcode_and_rows_run_in_time_order() {
    let mut t = Table::new("sam", None);
    let order: Vec<&str> = rows(t.layout(), "").iter().map(|r| r.headcode.as_str()).collect();
    assert_eq!(order, ["1E01", "1N02", "2W03", "2W04"]);
    let found: Vec<&str> = rows(t.layout(), " 2w ").iter().map(|r| r.headcode.as_str()).collect();
    assert_eq!(found, ["2W03", "2W04"], "trimmed, any case");
    assert!(rows(t.layout(), "nothing like it").is_empty());
    // A server that sent them out of order is put right.
    t.run(0.1);
    let mut l = t.layout().clone();
    l.simplifier.reverse();
    let order: Vec<&str> = rows(&l, "").iter().map(|r| r.headcode.as_str()).collect();
    assert_eq!(order, ["1E01", "1N02", "2W03", "2W04"]);
}

#[test]
fn a_running_train_shows_its_lateness() {
    let mut t = Table::new("eve", Some("East"));
    assert_eq!(lateness(Some(t.view()), "1E01"), None, "due, not running");
    assert_eq!(lateness(None, "1E01"), None);
    t.run(3.0);
    assert!(t.view().trains.get("1E01").is_some_and(|r| r.state != TrainState::Due), "{:?}", t.view().trains);
    assert_eq!(lateness(Some(t.view()), "1E01").as_deref(), Some("OT"));
    let mut v = t.view().clone();
    v.trains.get_mut("1E01").unwrap().late_s = 180;
    assert_eq!(lateness(Some(&v), "1E01").as_deref(), Some("3L"));
}

#[test]
fn the_enquiry_has_the_rows_and_the_live_state_and_never_routes() {
    let mut t = Table::new("sam", None);
    let e = enquiry(t.layout(), Some(t.view()), "1E01");
    assert_eq!((e.headcode.as_str(), e.rows.len()), ("1E01", 1));
    assert_eq!(e.live_text(), "due", "a spectator lists every train due");
    assert_eq!(enquiry(t.layout(), None, "9Z99").live_text(), "not in your train list");
    t.run(3.0);
    let berth = t.view().berths.iter().find(|(_, h)| *h == "1E01").map(|(b, _)| b.clone()).expect("1E01 is described");
    assert_eq!(t.app.headcode_at(&Target::Berth(berth)).as_deref(), Some("1E01"));
    assert_eq!(t.app.headcode_at(&Target::Berth(s("BD"))), None, "an empty berth");
    assert_eq!(t.app.headcode_at(&Target::Signal(s("C"))), None);
    let e = enquiry(t.layout(), Some(t.view()), "1E01");
    assert!(e.live_text().ends_with(", OT"), "{}", e.live_text());
    assert!(t.h.take_sent().is_empty(), "looking a headcode up sends nothing");
}
