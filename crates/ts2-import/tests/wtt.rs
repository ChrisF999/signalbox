//! Reading a Working Timetable (polish spec §4) from `pdftotext -bbox` text,
//! on a hand-made synthetic WTT (`tests/data/wtt-synthetic.bbox.html`,
//! written by `wtt-synthetic.py`: fictional trains 301–303 in the real WTT's
//! layout and notation), and putting it into Drain.

use signalbox_core::robot::soak;
use signalbox_core::sim::Sim;
use signalbox_core::time::parse_hms;
use signalbox_core::world::World;
use signalbox_core::world::file::{EndFile, WorldFile};
use ts2_import::wtt::{self, Bound, Checks, Day, Trip, WttError};

const SYNTHETIC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/wtt-synthetic.bbox.html");
const DRAIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/drain.json");

fn trips() -> Vec<Trip> {
    wtt::parse(&std::fs::read_to_string(SYNTHETIC).unwrap()).unwrap()
}

fn find(ts: &[Trip], train: u16, trip: u16) -> &Trip {
    ts.iter().find(|t| t.train == train && t.trip == trip).unwrap_or_else(|| panic!("no {train}/{trip}"))
}

fn at(s: &str) -> Option<u32> {
    parse_hms(s)
}

/// What the synthetic WTT says about itself (fictional figures, like the
/// rest of it).
fn checks() -> Checks {
    Checks {
        running: vec![("7", Bound::West, 195), ("8", Bound::West, 225), ("7", Bound::East, 270), ("8", Bound::East, 225)],
        snapshots: vec![(at("05:51").unwrap(), 0), (at("06:10").unwrap(), 2), (at("06:36").unwrap(), 1)],
        intervals: vec![(at("06:00").unwrap(), at("06:40").unwrap(), 590)],
    }
}

fn drain() -> WorldFile {
    ts2_import::convert(&std::fs::read_to_string(DRAIN).unwrap()).unwrap().world
}

#[test]
fn every_monday_to_friday_column_is_read() {
    let ts = trips();
    // Six westbound columns, three eastbound; the contents and Saturday pages are not timetables.
    assert_eq!(ts.len(), 9);
    assert!(ts.iter().all(|t| t.train != 309));
    let t = find(&ts, 301, 2);
    assert_eq!(t.bound, Bound::West);
    assert_eq!(t.platform.as_deref(), Some("7"));
    assert_eq!((t.bank, t.arr, t.dep, t.siding, t.to_form), (at("06:09"), at("06:12:15"), at("06:13:30"), at("06:14:30"), at("06:16")));
    // Fractions: `12` = ½, `14` = ¼, `34` = ¾, and a numerator stacked on its denominator.
    assert_eq!(find(&ts, 302, 3).arr, at("06:12:45"));
    assert_eq!(find(&ts, 302, 5).bank, at("06:34:30"));
    assert_eq!(find(&ts, 302, 5).arr, at("06:38:15"));
    assert_eq!(find(&ts, 302, 3).to_form, at("06:34:30"));
    assert_eq!(find(&ts, 302, 3).bank, at("06:17:15"));
    // `06z30` is 06:30 with the train-wash mark; `Stop` ends a working.
    let t = find(&ts, 301, 4);
    assert_eq!((t.depot, t.wash, t.to_form), (at("06:30:30"), true, None));
    assert!(t.has("Shed") && t.has("Rd"));
    // `Pfm 26`: a move that starts standing in a Waterloo platform.
    let t = find(&ts, 303, 1);
    assert_eq!((t.starts_in.as_deref(), t.dep, t.depot), (Some("26"), at("05:50"), at("05:52")));
    assert!(t.has("Start") && t.has("Ety"));
    let t = find(&ts, 301, 1);
    assert_eq!((t.bound, t.depot, t.arr, t.dep, t.bank), (Bound::East, at("06:00"), at("06:01:30"), at("06:03"), at("06:07:30")));
}

#[test]
fn day_codes_pick_one_weekday() {
    let ts = trips();
    let wed: Vec<(u16, u16)> = wtt::on_day(&ts, Day::Wed).unwrap().iter().map(|t| (t.train, t.trip)).collect();
    assert!(wed.contains(&(302, 1)) && wed.contains(&(302, 5)) && !wed.contains(&(302, 2)), "{wed:?}");
    let tue: Vec<(u16, u16)> = wtt::on_day(&ts, Day::Tue).unwrap().iter().map(|t| (t.train, t.trip)).collect();
    assert!(tue.contains(&(302, 2)) && !tue.contains(&(302, 1)) && !tue.contains(&(302, 5)), "{tue:?}");
    let mut odd = ts.clone();
    odd[0].notes.push("QO".into());
    assert_eq!(wtt::on_day(&odd, Day::Wed), Err(WttError::DayCode("QO".into(), odd[0].train, odd[0].trip)));
}

#[test]
fn a_day_is_checked_against_what_the_wtt_says() {
    let day = wtt::on_day(&trips(), Day::Wed).unwrap();
    let r = wtt::check(&day, &checks()).unwrap();
    assert_eq!((r.trips, r.trains, r.running_exact, r.running_longer, r.links), (8, 3, 7, 0, 5));
    assert_eq!(r.intervals, vec![(at("06:00").unwrap(), at("06:40").unwrap(), 590)]);
    let mut c = checks();
    c.snapshots[1].1 = 3;
    assert!(matches!(wtt::check(&day, &c), Err(WttError::Check(m)) if m.contains("2 trains in service at 06:10:00")));
    let mut fast = day.clone();
    fast.iter_mut().find(|t| (t.train, t.trip) == (301, 2)).unwrap().arr = at("06:12");
    assert!(matches!(wtt::check(&fast, &checks()), Err(WttError::Check(m)) if m.contains("under the published")));
    let mut broken = day.clone();
    broken.iter_mut().find(|t| (t.train, t.trip) == (301, 2)).unwrap().to_form = at("06:17");
    assert!(matches!(wtt::check(&broken, &checks()), Err(WttError::Check(m)) if m.contains("301 trip 2 forms")));
}

#[test]
fn a_day_goes_into_drain() {
    let day = wtt::on_day(&trips(), Day::Wed).unwrap();
    let mut w = drain();
    let r = wtt::apply(&mut w, &day).unwrap();
    assert_eq!((r.services, r.entries, r.start_time.as_str()), (7, 2, "05:50:00"));
    assert_eq!((r.dropped_empty, r.dropped_trains), (vec!["303/1".to_string()], vec![303]));
    let heads: Vec<&str> = w.services.iter().map(|s| s.headcode.as_str()).collect();
    assert_eq!(heads, ["301/1", "301/2", "301/3", "301/4", "302/1", "302/3", "302/5"]);
    let shown: Vec<&str> = w.services.iter().map(|s| s.display.as_deref().unwrap_or_default()).collect();
    assert_eq!(shown, ["301", "301", "301", "301", "302", "302", "302"], "the panel shows the train number");
    let svc = |h: &str| w.services.iter().find(|s| s.headcode == h).unwrap();
    let calls = |h: &str| svc(h).calls.iter().map(|c| format!("{} {}", c.place, c.platform.clone().unwrap_or_default())).collect::<Vec<_>>();
    assert_eq!(calls("302/1"), ["BNK 8", "WTL 26", "DPT 6"]);
    assert_eq!(calls("302/3"), ["DPT 6", "WTL 25", "BNK 8"]);
    assert_eq!(calls("301/1"), ["DPT 5", "WTL 25", "BNK 7"]);
    assert_eq!(calls("301/4"), ["BNK 7", "WTL 26", "DPT 5"]);
    assert!(matches!(&svc("301/1").end, EndFile::Form { service } if service == "301/2"));
    assert!(matches!(svc("301/4").end, EndFile::Stable), "to the depot for the night");
    let last = svc("302/5").calls.last().unwrap();
    assert_eq!((last.place.as_str(), last.stop, last.arr.as_deref()), ("WTL", false, Some("06:38:15")));
    assert!(matches!(svc("302/5").end, EndFile::Stable), "the last train in stables in platform 26");
    assert_eq!(w.options.start_time, "05:50:00");
    // P20: 20–30 s dwell, and nothing drawn from TS2's delay bands (they would
    // replace it); the WTT's trains enter on time.
    let o = &w.options;
    assert_eq!((o.min_dwell_s, o.entry_delay_s), ([20, 30], [0, 0]));
    assert!(o.min_dwell_bands.is_empty() && o.entry_delay_bands.is_empty(), "{o:?}");
    let entry = |h: &str| w.entries.iter().find(|e| e.service == h).unwrap();
    assert_eq!((entry("301/1").time.as_str(), entry("301/1").at.as_ref().unwrap().segment.as_str()), ("05:50:00", "L1000021"));
    assert_eq!((entry("302/1").time.as_str(), entry("302/1").at.as_ref().unwrap().segment.as_str()), ("05:50:00", "L8"));
    let again = {
        let mut w2 = drain();
        wtt::apply(&mut w2, &day).unwrap();
        serde_json::to_string(&w2).unwrap()
    };
    assert_eq!(serde_json::to_string(&w).unwrap(), again, "byte-identical");
    World::from_file(w).unwrap();
}

#[test]
fn the_synthetic_day_runs_under_the_robot() {
    let mut w = drain();
    wtt::apply(&mut w, &wtt::on_day(&trips(), Day::Wed).unwrap()).unwrap();
    let mut sim = Sim::new(World::from_file(w).unwrap(), 7);
    let r = soak(&mut sim, 3600.0);
    assert_eq!((r.spads, r.collisions, r.invariant_violations), (0, 0, 0), "{r:?}");
    assert!(r.stuck.is_empty() && r.still_running.is_empty(), "{r:?}");
    assert_eq!((r.entered, r.stabled), (2, 2), "{r:?}");
}

#[test]
fn a_wrong_file_is_refused() {
    assert_eq!(wtt::parse("<html><body>not a timetable</body></html>"), Err(WttError::NoPages));
    let text = std::fs::read_to_string(SYNTHETIC).unwrap().replacen(">06</word>", ">6a</word>", 1);
    assert!(matches!(wtt::parse(&text), Err(WttError::Format(..))), "a garbled time");
}

/// A word tag cut short is an error naming its page, not a panic.
#[test]
fn a_malformed_word_is_refused() {
    let text = std::fs::read_to_string(SYNTHETIC).unwrap();
    let bad = text.replacen(r#"">WESTBOUND</word>"#, r#""</word>"#, 1);
    assert_ne!(bad, text);
    assert!(matches!(wtt::parse(&bad), Err(WttError::Format(2, m)) if m.contains("malformed")), "{:?}", wtt::parse(&bad));
}

/// Minutes past 59 and hours past 27 are errors naming the page; hours
/// 24–27 are how a WTT may write times after midnight.
#[test]
fn out_of_range_times_are_refused() {
    let text = std::fs::read_to_string(SYNTHETIC).unwrap();
    // 303/1 leaves at 05:50: its minutes are the first `50` word.
    let minutes = text.replacen(">50</word>", ">75</word>", 1);
    assert!(matches!(wtt::parse(&minutes), Err(WttError::Format(2, m)) if m.contains("minutes 75")), "{:?}", wtt::parse(&minutes));
    assert!(wtt::parse(&text.replacen(">06</word>", ">24</word>", 1)).is_ok(), "24:05 is after midnight");
    let hours = text.replacen(">06</word>", ">29</word>", 1);
    assert!(matches!(wtt::parse(&hours), Err(WttError::Format(_, m)) if m.contains("hours 29")), "{:?}", wtt::parse(&hours));
    let wash = text.replacen(">06z30</word>", ">06z61</word>", 1);
    assert_ne!(wash, text);
    assert!(matches!(wtt::parse(&wash), Err(WttError::Format(_, m)) if m.contains("minutes 61")), "{:?}", wtt::parse(&wash));
}
