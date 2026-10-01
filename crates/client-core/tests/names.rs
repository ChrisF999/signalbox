//! Display names (realism spec §2, owner decision 11).

mod common;

use std::collections::BTreeMap;

use client_core::Names;
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
