//! The simplifier and the headcode enquiry (realism spec §3).

mod common;

use client_core::Target;
use client_core::simplifier::{Line, enquiry, fmt_wtt, lateness, lines, now_line, resolve, rows};
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

/// The search finds a service by its display headcode too (a WTT train
/// number finds all its trips).
#[test]
fn search_finds_display_headcodes() {
    let t = Table::new("sam", None);
    let mut l = t.layout().clone();
    l.display_headcodes = [(s("1N02"), s("77")), (s("2W04"), s("77"))].into_iter().collect();
    let found: Vec<&str> = rows(&l, "77").iter().map(|r| r.headcode.as_str()).collect();
    assert_eq!(found, ["1N02", "2W04"]);
}

/// A berth holding what a player typed (a train number, say) opens the
/// enquiry of the one running train shown by it; anything else is looked up
/// as it is.
#[test]
fn a_typed_display_headcode_finds_its_running_train() {
    let mut t = Table::new("sam", None);
    t.run(3.0);
    let mut l = t.layout().clone();
    l.display_headcodes = [(s("1E01"), s("E1")), (s("1N02"), s("E1"))].into_iter().collect();
    let v = t.view();
    assert_ne!(v.trains["1E01"].state, TrainState::Due);
    assert_eq!(v.trains.get("1N02").map(|r| r.state), Some(TrainState::Due));
    assert_eq!(resolve(&l, Some(v), "E1"), "1E01", "1N02 is only due");
    assert_eq!(resolve(&l, Some(v), "1N02"), "1N02", "a headcode is itself");
    assert_eq!(resolve(&l, Some(v), "9Z99"), "9Z99");
    assert_eq!(resolve(&l, None, "E1"), "E1");
}

/// Polish spec §7: the simplifier opens at the first train not yet finished.
#[test]
fn the_simplifier_opens_at_the_first_train_not_yet_finished() {
    let row = |h: &str, times: &[f64]| SimplifierRow {
        headcode: s(h),
        origin: None,
        destination: None,
        calls: times.iter().map(|&t| call("X", None, Some(t), Some(t), true)).collect(),
    };
    let (a, b, c, u) = (row("1A01", &[100.0, 200.0]), row("1A02", &[150.0]), row("1A03", &[300.0, 400.0, 500.0]), row("1A04", &[]));
    let rows = vec![&a, &b, &c, &u];
    assert_eq!(now_line(&rows, 0.0), 0);
    assert_eq!(now_line(&rows, 160.0), 0, "1A01 is still running");
    assert_eq!(now_line(&rows, 200.0), 0, "a call at exactly now has not finished");
    assert_eq!(now_line(&rows, 250.0), 3, "after 1A01's two lines and 1A02's one");
    assert_eq!(now_line(&rows, 600.0), 6, "a row with no times never finishes");
}

/// Polish spec M7: the enquiry says what the train does next.
#[test]
fn the_enquiry_says_what_the_train_does_next() {
    let t = Table::new("eve", Some("East"));
    let row = |state, arr, dep| TrainRow {
        next_place: Some(s("EST")),
        next_platform: Some(s("1")),
        booked: arr,
        arr,
        dep,
        late_s: 0,
        state,
    };
    let names = client_core::Names::default();
    let l = t.layout().clone();
    let mut v = t.view().clone();
    v.trains.insert(s("1E01"), row(TrainState::AtPlatform, Some(25_440.0), Some(25_500.0)));
    assert_eq!(enquiry(&l, Some(&v), "1E01").next_text(&names).as_deref(), Some("depart EST 1 at 07:05"));
    v.trains.insert(s("1E01"), row(TrainState::InArea, Some(25_440.0), Some(25_500.0)));
    assert_eq!(enquiry(&l, Some(&v), "1E01").next_text(&names).as_deref(), Some("arrive EST 1 at 07:04"));
    v.trains.insert(s("1E01"), row(TrainState::Approaching, None, Some(25_530.0)));
    assert_eq!(enquiry(&l, Some(&v), "1E01").next_text(&names).as_deref(), Some("pass EST 1 at 07:05½"));
    v.trains.insert(s("1E01"), row(TrainState::Due, Some(25_440.0), None));
    assert_eq!(enquiry(&l, Some(&v), "1E01").next_text(&names), None);
    // Review I2: standing where it terminates there is nothing to depart.
    v.trains.insert(s("1E01"), row(TrainState::AtPlatform, Some(25_440.0), None));
    assert_eq!(enquiry(&l, Some(&v), "1E01").next_text(&names).as_deref(), Some("terminates at EST 1"));
}
