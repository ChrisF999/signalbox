//! Display names (realism spec §2, owner decision 11).

mod common;

use std::collections::BTreeMap;

use client_core::{Names, select};
use client_core::text::{command_text, notice_text};
use common::*;
use protocol::*;

fn one_box(prefix: &str, letters: &[(&str, &str)]) -> Layout {
    let signal = |name: &str, area: &str| SignalInfo {
        name: s(name),
        area: s(area),
        segment: s("x"),
        offset_m: 0.0,
        direction: Dir::Up,
        aspects: 3,
        operable: true,
    };
    Layout {
        title: s("t"),
        you: s("ann"),
        area: Some(s("A")),
        areas: vec![s("A")],
        sections: vec![],
        segments: vec![],
        signals: vec![signal("121", "A"), signal("39,1V1", "A")],
        points: vec![],
        berths: vec![],
        platforms: vec![],
        routes: vec![],
        geometry: None,
        box_prefix: s(prefix),
        workstations: letters.iter().map(|(a, l)| (s(a), s(l))).collect::<BTreeMap<_, _>>(),
        simplifier: vec![],
        display_headcodes: Default::default(),
        places: Default::default(),
    }
}

#[test]
fn signals_get_the_box_and_their_areas_workstation() {
    let t = Table::new("eve", Some("East"));
    let n = t.app.game().unwrap().names();
    assert_eq!((n.signal("C"), n.signal("A"), n.signal("W2")), (s("TBC"), s("TAA"), s("TAW2")), "twobox: T, West A, East B");
    assert_eq!(n.signal("W1"), "W1", "West's W1 is not in East's layout: shown as it is");
    assert_eq!(n.exit(&ExitName::Signal(s("W2"))), "TAW2");
    assert_eq!(n.exit(&ExitName::Node(s("E"))), "E", "nodes keep their names");
    assert_eq!((n.workstation("West"), n.workstation("East"), n.workstation("Nowhere")), (Some("A"), Some("B"), None));
}

#[test]
fn a_single_area_layout_has_no_workstation_letter() {
    let n = Names::new(&one_box("L", &[("A", "A")]));
    assert_eq!((n.signal("121"), n.signal("39,1V1")), (s("L121"), s("L39,1V1")));
    assert_eq!(n.workstation("A"), None);
}

#[test]
fn a_layout_from_an_older_server_shows_plain_names() {
    let n = Names::new(&one_box("", &[]));
    assert_eq!(n.signal("121"), "121");
    assert_eq!(Names::default().signal("121"), "121", "before any layout");
}

#[test]
fn alarms_and_commands_use_the_shown_names() {
    let t = Table::new("eve", Some("East"));
    let n = t.app.game().unwrap().names();
    let cmd = PlayerCommand::SetRoute { entrance: s("A"), exit: ExitName::Node(s("E")) };
    assert_eq!(command_text(&cmd, n), "set route TAA to E");
    assert_eq!(command_text(&PlayerCommand::CancelRoute { entrance: s("C") }, n), "cancel route from TBC");
    assert_eq!(
        command_text(&PlayerCommand::SetAutoWorking { entrance: s("D"), on: true }, n),
        "auto-working on at TBD"
    );
    assert_eq!(
        notice_text(&Notice::Spad { signal: s("C"), train: s("1E01") }, n),
        (s("SPAD: 1E01 passed TBC at danger"), true)
    );
}

fn wtt_box() -> Layout {
    let mut l = one_box("W", &[]);
    l.display_headcodes = [(s("301/1"), s("301")), (s("301/2"), s("301"))].into_iter().collect();
    l
}

/// A service with a display headcode is shown by it (polish spec P18,
/// amended: a WTT trip `301/1` shows its train number); any other text,
/// such as a headcode a player interposed, is shown as it is.
#[test]
fn headcodes_are_shown_by_their_display_headcode() {
    let n = Names::new(&wtt_box());
    assert_eq!((n.headcode("301/1"), n.headcode("301/2"), n.headcode("1A01")), ("301", "301", "1A01"));
    assert_eq!(Names::default().headcode("301/1"), "301/1", "before any layout");
    assert_eq!(notice_text(&Notice::Spad { signal: s("121"), train: s("301/1") }, &n).0, "SPAD: 301 passed W121 at danger");
    let late = Notice::Late { train: s("301/1"), place: s("BNK"), platform: s("7"), late_s: 120 };
    assert_eq!(notice_text(&late, &n).0, "301 at BNK 7, 2 min late");
    let wrong = Notice::WrongPlatform { train: s("301/2"), place: s("BNK"), platform: s("8"), expected: s("7") };
    assert_eq!(notice_text(&wrong, &n).0, "301 at BNK platform 8, booked 7");
    let handover = Notice::Handover { headcode: s("301/1"), from_area: s("West") };
    assert_eq!(notice_text(&handover, &n).0, "301 offered from West");
}

/// Polish spec M1, M2: points, berths and track are never shown by their
/// converter ids, and place codes read as their names.
#[test]
fn points_berths_track_and_places_have_display_names() {
    use client_core::names::points_number;
    assert_eq!((points_number("N153"), points_number("P1"), points_number("P"), points_number("X9")), (s("P153"), s("P1"), s("P"), s("X9")));
    let mut l = one_box("L", &[]);
    l.points.push(PointsInfo { name: s("N153"), section: s("T7"), area: s("A"), operable: true });
    l.berths.push(BerthInfo { name: s("B121"), signal: Some(s("121")), boundary: None, area: s("A"), operable: true });
    l.berths.push(BerthInfo { name: s("BX"), signal: None, boundary: Some(s("N9")), area: s("A"), operable: true });
    l.segments.push(SegmentInfo { name: s("L5"), from: s("N1"), to: s("N2"), length_m: 100.0, section: s("T7") });
    l.platforms.push(PlatformInfo { place: s("LIVST"), platform: s("10"), segment: s("L5"), from_m: 0.0, to_m: 90.0 });
    l.places.insert(s("LIVST"), s("LIVERPOOL STREET"));
    let n = Names::new(&l);
    assert_eq!((n.points("N153"), n.points("unknown")), (s("LP153"), s("unknown")));
    assert_eq!((n.berth("B121"), n.berth("BX"), n.berth("B0")), (s("L121"), s("edge"), s("B0")));
    assert_eq!((n.track("T7"), n.track("T8")), (s("Track at LIVERPOOL STREET 10"), s("Track")));
    assert_eq!((n.place("LIVST"), n.place("BNK")), ("LIVERPOOL STREET", "BNK"));
    let late = Notice::Late { train: s("1P02"), place: s("LIVST"), platform: s("10"), late_s: 60 };
    assert_eq!(notice_text(&late, &n).0, "1P02 at LIVERPOOL STREET 10, 1 min late");
    assert_eq!(command_text(&PlayerCommand::SwingPoints { points: s("N153"), to: PointsPos::Reverse }, &n), "swing LP153 reverse");
    assert_eq!(notice_text(&Notice::Collision { section: s("T7") }, &n).0, "COLLISION: Track at LIVERPOOL STREET 10");
    // Ruling D3: a boundary berth reads "the edge berth" in command texts.
    let interpose = |berth: &str| command_text(&PlayerCommand::Interpose { berth: s(berth), headcode: s("2Z99") }, &n);
    assert_eq!((interpose("B121"), interpose("BX")), (s("interpose 2Z99 at L121"), s("interpose 2Z99 at the edge berth")));
    assert_eq!(command_text(&PlayerCommand::CancelBerth { berth: s("BX") }, &n), "cancel the headcode at the edge berth");
    assert_eq!(select::describe_berth(&l, &empty_view(), "BX"), "Edge berth: empty");
}

fn empty_view() -> View {
    View {
        seq: 0,
        sim_time: 0.0,
        speed: 1,
        paused: false,
        vote: None,
        holders: Default::default(),
        score: None,
        signals: Default::default(),
        routes: Default::default(),
        points: Default::default(),
        sections: Default::default(),
        berths: Default::default(),
        trains: Default::default(),
    }
}
