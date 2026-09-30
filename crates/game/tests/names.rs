//! Commands travel by name; names are opaque.

mod common;

use common::*;
use game::areas::AreaMap;
use game::names::{resolve, to_player_command, valid_headcode};
use protocol::{ExitName, PlayerCommand, PointsPos};
use signalbox_core::events::Command;
use signalbox_core::routes::Exit;
use signalbox_core::world::World;

fn s(x: &str) -> String {
    x.to_string()
}

fn every_kind() -> Vec<PlayerCommand> {
    vec![
        PlayerCommand::SetRoute { entrance: s("W1"), exit: ExitName::Signal(s("A")) },
        PlayerCommand::SetRoute { entrance: s("A"), exit: ExitName::Node(s("E")) },
        PlayerCommand::CancelRoute { entrance: s("C") },
        PlayerCommand::SetAutoWorking { entrance: s("C"), on: true },
        PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse },
        PlayerCommand::Interpose { berth: s("BA"), headcode: s("1A01") },
        PlayerCommand::CancelBerth { berth: s("BC") },
    ]
}

#[test]
fn names_resolve_to_ids_and_back() {
    let w = twobox();
    let net = &w.net;
    assert_eq!(
        resolve(&w, &every_kind()[1]),
        Some(Command::SetRoute { entrance: net.signal("A").unwrap(), exit: Exit::Node(net.node("E").unwrap()) })
    );
    for pc in every_kind() {
        let c = resolve(&w, &pc).unwrap_or_else(|| panic!("{pc:?}"));
        assert_eq!(to_player_command(&w, &c), pc);
    }
}

#[test]
fn unknown_names_resolve_to_nothing() {
    let w = twobox();
    for pc in [
        PlayerCommand::SetRoute { entrance: s("Z9"), exit: ExitName::Signal(s("A")) },
        PlayerCommand::SetRoute { entrance: s("W1"), exit: ExitName::Signal(s("Z9")) },
        PlayerCommand::SetRoute { entrance: s("A"), exit: ExitName::Node(s("A")) },
        PlayerCommand::CancelRoute { entrance: s("BA") },
        PlayerCommand::SetAutoWorking { entrance: s(""), on: false },
        PlayerCommand::SwingPoints { points: s("TP"), to: PointsPos::Normal },
        PlayerCommand::Interpose { berth: s("A"), headcode: s("1A01") },
        PlayerCommand::CancelBerth { berth: s("bc") },
    ] {
        assert_eq!(resolve(&w, &pc), None, "{pc:?}");
    }
}

#[test]
fn names_with_commas_and_hashes_resolve() {
    let text = twobox_json()
        .replace("\"C\"", "\"39,1V1\"")
        .replace("\"BC\"", "\"512#113\"")
        .replace("\"East\"", "\"Hackney & Bow\"");
    let w = World::from_json(&text).unwrap();
    let pc = PlayerCommand::SetRoute { entrance: s("39,1V1"), exit: ExitName::Signal(s("W2")) };
    let c = resolve(&w, &pc).unwrap();
    assert_eq!(to_player_command(&w, &c), pc);
    let m = AreaMap::new(&w);
    assert_eq!(w.net.areas[m.subject(&c).unwrap().idx()].name, "Hackney & Bow");
    let berth = PlayerCommand::CancelBerth { berth: s("512#113") };
    assert_eq!(to_player_command(&w, &resolve(&w, &berth).unwrap()), berth);
    assert!(w.route_by_name("39,1V1-W2").is_some());
}

#[test]
fn headcodes_are_short_and_plain() {
    for ok in ["1A01", "2W03", "X", "ABCDEFGHIJ"] {
        assert!(valid_headcode(ok), "{ok}");
    }
    for bad in ["", "1A 01", "ABCDEFGHIJK", "1A-01", "<b>", "é1"] {
        assert!(!valid_headcode(bad), "{bad}");
    }
}
