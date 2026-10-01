//! Clicks, menus, hover text and the train list (spec D1 §3.2, §6)
//! against a real twobox game in process.

mod common;

use std::collections::BTreeMap;

use client_core::Target;
use client_core::app::REFUSED_S;
use client_core::select::{self, Click, MenuItem};
use client_core::trains::train_list;
use common::*;
use protocol::*;

fn sig(n: &str) -> Target {
    Target::Signal(s(n))
}

fn set_route(entrance: &str, exit: ExitName) -> PlayerCommand {
    PlayerCommand::SetRoute { entrance: s(entrance), exit }
}

#[test]
fn entrance_then_exit_sets_the_route() {
    let mut t = Table::new("ann", Some("West"));
    t.app.click(&sig("W1"));
    assert_eq!(t.app.game().unwrap().selected(), Some("W1"));
    assert_eq!(t.app.valid_exits(), [ExitName::Signal(s("A"))], "only exits of routes from W1 light up");
    t.app.click(&sig("A"));
    assert_eq!(t.app.game().unwrap().selected(), None);
    assert_eq!(t.h.take_sent(), [ClientFrame::Game(ClientMsg::Command { cmd: set_route("W1", ExitName::Signal(s("A"))) })]);
    let out = t.game.handle("ann", ClientMsg::Command { cmd: set_route("W1", ExitName::Signal(s("A"))) });
    t.deliver(out);
    t.run(1.0);
    assert!(t.view().routes.contains_key("W1-A"), "{:?}", t.view().routes);
}

#[test]
fn routes_to_a_boundary_end_at_its_exit_marker() {
    let mut t = Table::new("ann", Some("West"));
    t.app.click(&sig("A"));
    assert_eq!(t.app.valid_exits(), [ExitName::Node(s("E")), ExitName::Node(s("N"))]);
    t.app.click(&Target::Exit(s("N")));
    t.pump();
    t.run(10.0);
    assert!(t.view().routes.contains_key("A-N"), "{:?}", t.view().routes);
    assert_eq!(t.view().points["P"].position, PointsPos::Reverse);
}

#[test]
fn the_selection_clears_on_the_entrance_again_esc_or_a_dead_click() {
    let mut t = Table::new("ann", Some("West"));
    t.app.click(&sig("W1"));
    t.app.click(&sig("W1"));
    assert_eq!(t.app.game().unwrap().selected(), None, "the entrance again");
    t.app.click(&sig("W1"));
    t.app.escape();
    assert_eq!(t.app.game().unwrap().selected(), None, "Esc");
    t.app.click(&sig("W1"));
    t.app.click(&sig("W2"));
    assert_eq!(t.app.game().unwrap().selected(), Some("W2"), "another entrance takes over");
    t.app.click(&Target::Exit(s("E")));
    assert_eq!(t.app.game().unwrap().selected(), None, "not an exit of W2's routes");
    for dead in [Target::Points(s("P")), Target::Berth(s("BA")), Target::Section(s("TW1")), sig("C")] {
        t.app.click(&sig("W1"));
        t.app.click(&dead);
        assert_eq!(t.app.game().unwrap().selected(), None, "a dead click on {dead:?}");
    }
    assert!(t.h.take_sent().is_empty(), "nothing was sent");
}

#[test]
fn fringe_and_spectators_get_hover_only() {
    let mut t = Table::new("ann", Some("West"));
    t.app.click(&sig("C"));
    assert_eq!(t.app.game().unwrap().selected(), None, "C is East's, seen on West's fringe");
    assert!(t.app.menu(&Target::Points(s("P"))).is_empty());
    assert_eq!(t.app.describe(&Target::Points(s("P"))), "Points P (East): normal");
    let mut spec = Table::new("sam", None);
    spec.app.click(&sig("W1"));
    assert_eq!(spec.app.game().unwrap().selected(), None);
    assert!(spec.app.menu(&sig("W1")).is_empty());
    assert!(!spec.app.can_interpose("BA"));
    spec.app.interpose("BA", "2Z99");
    assert!(spec.h.take_sent().is_empty(), "a spectator's interpose sends nothing");
    t.app.interpose("BC", "2Z99");
    assert!(t.h.take_sent().is_empty(), "nor does one on the fringe");
    assert_eq!(spec.app.describe(&sig("W1")), "Signal TAW1 (West): red", "twobox: box T, West is A");
}

#[test]
fn right_click_cancels_a_route_and_swings_points() {
    let mut t = Table::new("eve", Some("East"));
    assert_eq!(
        t.app.menu(&Target::Points(s("P"))),
        [MenuItem { label: s("Swing P reverse"), cmd: PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse } }]
    );
    assert!(t.app.menu(&sig("C")).is_empty(), "no route set from C");
    t.app.click(&sig("C"));
    t.app.click(&sig("W2"));
    t.pump();
    t.run(10.0);
    assert!(t.view().routes.contains_key("C-W2"));
    assert_eq!(
        t.app.menu(&sig("C")),
        [MenuItem { label: s("Cancel route TBC to TAW2"), cmd: PlayerCommand::CancelRoute { entrance: s("C") } }],
        "C is East's (B), W2 West's (A)"
    );
    assert_eq!(t.app.describe(&sig("C")), "Signal TBC: yellow; route to TAW2 set");
    let MenuItem { cmd, .. } = t.app.menu(&sig("C")).remove(0);
    t.app.command(cmd);
    t.pump();
    t.run(60.0);
    assert!(!t.view().routes.contains_key("C-W2"));
}

#[test]
fn a_refused_command_outlines_its_entrance_and_raises_an_alarm() {
    let mut t = Table::new("eve", Some("East"));
    t.app.click(&sig("C"));
    t.app.click(&sig("W2"));
    t.pump();
    t.run(0.5);
    t.app.click(&sig("D"));
    t.app.click(&sig("W2"));
    t.pump();
    t.run(0.2);
    let g = t.app.game().unwrap();
    assert_eq!(g.refused(), Some("D"));
    assert_eq!(
        t.log_lines().last().unwrap(),
        &(s("Refused: set route TBD to TAW2 (conflicts with a route already set)"), true)
    );
    t.run(REFUSED_S);
    assert_eq!(t.app.game().unwrap().refused(), None);
}

#[test]
fn berths_interpose_a_typed_headcode_and_cancel_it() {
    let mut t = Table::new("ann", Some("West"));
    assert!(t.app.can_interpose("BA"));
    assert!(t.app.menu(&Target::Berth(s("BA"))).is_empty(), "nothing to cancel yet");
    t.app.interpose("BA", "   ");
    assert!(t.h.take_sent().is_empty(), "a blank headcode sends nothing");
    t.app.interpose("BA", " 2Z99 ");
    t.pump();
    t.run(0.5);
    assert_eq!(t.view().berths.get("BA").map(String::as_str), Some("2Z99"));
    assert_eq!(t.app.describe(&Target::Berth(s("BA"))), "Berth BA: 2Z99");
    assert_eq!(
        t.app.menu(&Target::Berth(s("BA"))),
        [MenuItem { label: s("Cancel 2Z99"), cmd: PlayerCommand::CancelBerth { berth: s("BA") } }]
    );
}

fn auto_layout() -> Layout {
    let route = |name: &str, exit: &str, automatic: bool| RouteInfo {
        name: s(name),
        entrance: s("S1"),
        exit: ExitName::Signal(s(exit)),
        automatic,
        operable: true,
    };
    Layout {
        title: s("t"),
        you: s("ann"),
        area: Some(s("A")),
        areas: vec![s("A")],
        sections: vec![],
        segments: vec![],
        signals: vec![SignalInfo {
            name: s("S1"),
            area: s("A"),
            segment: s("x"),
            offset_m: 0.0,
            direction: Dir::Up,
            aspects: 3,
            operable: true,
        }],
        points: vec![],
        berths: vec![],
        platforms: vec![],
        routes: vec![route("S1-S2", "S2", true), route("S1-S3", "S3", false)],
        geometry: None,
        box_prefix: String::new(),
        workstations: BTreeMap::new(),
        simplifier: vec![],
    }
}

fn empty_view() -> View {
    View {
        seq: 1,
        sim_time: 0.0,
        speed: 1,
        paused: false,
        vote: None,
        holders: BTreeMap::new(),
        score: Some(0),
        signals: BTreeMap::new(),
        routes: BTreeMap::new(),
        points: BTreeMap::new(),
        sections: BTreeMap::new(),
        berths: BTreeMap::new(),
        trains: BTreeMap::new(),
    }
}

#[test]
fn automatic_routes_offer_auto_working_on_and_off() {
    let l = auto_layout();
    let mut v = empty_view();
    assert!(select::signal_menu(&l, &v, "S1").is_empty());
    v.routes.insert(s("S1-S2"), RouteView { state: RouteState::Locked, auto_working: true });
    let labels: Vec<String> = select::signal_menu(&l, &v, "S1").into_iter().map(|m| m.label).collect();
    assert_eq!(labels, ["Cancel route S1 to S2", "Auto-working off"]);
    v.routes.insert(s("S1-S2"), RouteView { state: RouteState::Locked, auto_working: false });
    assert_eq!(
        select::signal_menu(&l, &v, "S1")[1].cmd,
        PlayerCommand::SetAutoWorking { entrance: s("S1"), on: true }
    );
    assert_eq!(select::click(&l, None, &ExitName::Signal(s("S9"))), Click::Ignore, "no routes from S9");
    assert_eq!(select::click(&l, Some("S1"), &ExitName::Node(s("Z"))), Click::Clear);
}

/// The ○A button sends exactly what the signal menu's auto-working entry
/// would, and nothing when the menu has none.
#[test]
fn the_auto_button_is_the_menus_auto_working_command() {
    let l = auto_layout();
    let mut v = empty_view();
    assert_eq!(select::auto_toggle(&l, &v, "S1"), None, "no route set from S1");
    assert_eq!(select::describe_auto(&l, &v, "S1"), "Auto-working S1: off");
    v.routes.insert(s("S1-S2"), RouteView { state: RouteState::Locked, auto_working: false });
    assert_eq!(select::auto_toggle(&l, &v, "S1"), Some(PlayerCommand::SetAutoWorking { entrance: s("S1"), on: true }));
    v.routes.insert(s("S1-S2"), RouteView { state: RouteState::Locked, auto_working: true });
    assert_eq!(select::auto_toggle(&l, &v, "S1"), Some(PlayerCommand::SetAutoWorking { entrance: s("S1"), on: false }));
    assert!(select::auto_working(&l, &v, "S1"));
    assert_eq!(select::describe_auto(&l, &v, "S1"), "Auto-working S1: on");
    let mut theirs = l.clone();
    theirs.signals[0].operable = false;
    assert_eq!(select::auto_toggle(&theirs, &v, "S1"), None, "not on the fringe or for a spectator");
}

#[test]
fn clicking_an_auto_button_never_touches_the_selection() {
    let mut t = Table::new("ann", Some("West"));
    t.app.click(&sig("W1"));
    t.app.click(&Target::Auto(s("W1")));
    assert_eq!(t.app.game().unwrap().selected(), Some("W1"));
    assert!(t.h.take_sent().is_empty(), "twobox has no automatic routes: nothing to toggle");
    assert_eq!(t.app.describe(&Target::Auto(s("W1"))), "Auto-working TAW1: off");
    assert!(t.app.menu(&Target::Auto(s("W1"))).is_empty());
}

#[test]
fn the_train_list_puts_platforms_first_then_by_booked_time() {
    let row = |state, booked: Option<f64>| TrainRow { next_place: None, next_platform: None, booked, late_s: 0, state };
    let mut v = empty_view();
    v.trains.insert(s("1A"), row(TrainState::Due, Some(100.0)));
    v.trains.insert(s("1B"), row(TrainState::InArea, Some(300.0)));
    v.trains.insert(s("1C"), row(TrainState::InArea, Some(200.0)));
    v.trains.insert(s("1D"), row(TrainState::AtPlatform, None));
    v.trains.insert(s("1E"), row(TrainState::InArea, None));
    v.trains.insert(s("1F"), row(TrainState::Approaching, Some(50.0)));
    let order: Vec<&str> = train_list(&v).into_iter().map(|(h, _)| h).collect();
    assert_eq!(order, ["1D", "1C", "1B", "1E", "1F", "1A"]);
}

#[test]
fn hover_describes_track_and_names_other_areas() {
    let t = Table::new("ann", Some("West"));
    assert_eq!(t.app.describe(&Target::Section(s("TW1"))), "Track TW1: clear");
    assert_eq!(t.app.describe(&Target::Section(s("TP"))), "Track TP (East): clear");
    assert_eq!(t.app.describe(&Target::Exit(s("W"))), "Exit W");
    assert_eq!(t.app.describe(&sig("nowhere")), "Signal nowhere");
}

#[test]
fn a_new_layout_drops_an_entrance_you_can_no_longer_work() {
    let mut t = Table::new("ann", Some("West"));
    t.app.click(&sig("W1"));
    t.app.release();
    t.pump();
    assert_eq!(t.app.game().unwrap().area(), None);
    assert_eq!(t.app.game().unwrap().selected(), None);
    assert!(t.app.valid_exits().is_empty());
}

#[test]
fn interpose_validates_like_the_server() {
    for bad in ["2Z-99", "12345678901", "é", "", "  "] {
        assert_eq!(select::interpose("BA", bad), None, "{bad:?}");
    }
    for ok in ["2z99", "1A01", " 1A01 ", "1234567890"] {
        let h = ok.trim();
        assert_eq!(
            select::interpose("BA", ok),
            Some(PlayerCommand::Interpose { berth: s("BA"), headcode: s(h) }),
            "{ok:?}"
        );
    }
}

#[test]
fn cancelling_routes_and_busy_points_offer_no_menu() {
    let l = auto_layout();
    let mut v = empty_view();
    v.routes.insert(s("S1-S2"), RouteView { state: RouteState::Cancelling, auto_working: true });
    assert!(select::signal_menu(&l, &v, "S1").is_empty());

    let mut l = l;
    l.points.push(PointsInfo { name: s("P"), section: s("x"), area: s("A"), operable: true });
    assert!(select::points_menu(&l, &v, "P").is_empty(), "no entry in the view");
    for (moving, locked) in [(true, false), (false, true)] {
        v.points.insert(s("P"), PointsView { position: PointsPos::Normal, moving, locked });
        assert!(select::points_menu(&l, &v, "P").is_empty(), "moving {moving} locked {locked}");
    }
    v.points.insert(s("P"), PointsView { position: PointsPos::Normal, moving: false, locked: false });
    assert_eq!(select::points_menu(&l, &v, "P").len(), 1);
}
