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
    assert_eq!(t.app.describe(&Target::Points(s("P"))), "Points TBP (East): normal");
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
        [MenuItem { label: s("Swing TBP reverse"), cmd: PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse } }]
    );
    assert!(t.app.menu(&sig("C")).is_empty(), "no route set from C");
    t.app.click(&sig("C"));
    t.app.click(&sig("W2"));
    t.pump();
    t.run(10.0);
    assert!(t.view().routes.contains_key("C-W2"));
    assert_eq!(
        t.app.menu(&sig("C")),
        [
            MenuItem { label: s("Cancel route TBC to TAW2"), cmd: PlayerCommand::CancelRoute { entrance: s("C") } },
            MenuItem { label: s("Auto-working on"), cmd: PlayerCommand::SetAutoWorking { entrance: s("C"), on: true } },
        ],
        "C is East's (B), W2 West's (A); a controlled route can be auto-worked"
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
    assert_eq!(t.app.describe(&Target::Berth(s("BA"))), "Berth TAA: 2Z99");
    assert_eq!(
        t.app.menu(&Target::Berth(s("BA"))),
        [MenuItem { label: s("Cancel 2Z99"), cmd: PlayerCommand::CancelBerth { berth: s("BA") } }]
    );
}

/// A berth holding a service with a display headcode shows that in its
/// hover text and menu (polish spec P18, amended).
#[test]
fn berths_show_display_headcodes() {
    let mut t = Table::new("ann", Some("West"));
    t.app.interpose("BA", "2Z99");
    t.pump();
    t.run(0.5);
    let mut l = t.layout().clone();
    l.display_headcodes.insert(s("2Z99"), s("299"));
    assert_eq!(select::describe_berth(&l, t.view(), "BA"), "Berth TAA: 299");
    assert_eq!(select::berth_menu(&l, t.view(), "BA")[0].label, "Cancel 299");
}

fn auto_layout() -> Layout {
    let route = |entrance: &str, name: &str, exit: &str, automatic: bool| RouteInfo {
        name: s(name),
        entrance: s(entrance),
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
        signals: ["S1", "S4"]
            .map(|n| SignalInfo {
                name: s(n),
                area: s("A"),
                segment: s("x"),
                offset_m: 0.0,
                direction: Dir::Up,
                aspects: 3,
                operable: true,
            })
            .to_vec(),
        points: vec![],
        berths: vec![],
        platforms: vec![],
        routes: vec![route("S1", "S1-S2", "S2", false), route("S1", "S1-S3", "S3", false), route("S4", "S4-S1", "S1", true)],
        geometry: None,
        box_prefix: String::new(),
        workstations: BTreeMap::new(),
        simplifier: vec![],
        display_headcodes: Default::default(),
        places: Default::default(),
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

/// Real IECC auto-working: any live route from a controlled signal can be
/// auto-worked; a permanently automatic signal (S4) never offers it.
#[test]
fn controlled_routes_offer_auto_working_on_and_off() {
    let l = auto_layout();
    let mut v = empty_view();
    assert!(select::signal_menu(&l, &v, "S1").is_empty());
    v.routes.insert(s("S1-S2"), RouteView { state: RouteState::Locked, auto_working: true });
    let labels: Vec<String> = select::signal_menu(&l, &v, "S1").into_iter().map(|m| m.label).collect();
    assert_eq!(labels, ["Cancel route S1 to S2", "Auto-working off"]);
    v.routes.insert(s("S1-S2"), RouteView { state: RouteState::Setting, auto_working: false });
    assert_eq!(
        select::signal_menu(&l, &v, "S1")[1].cmd,
        PlayerCommand::SetAutoWorking { entrance: s("S1"), on: true },
        "a route still setting can be auto-worked too, as the core allows"
    );
    v.routes.clear();
    v.routes.insert(s("S1-S3"), RouteView { state: RouteState::Locked, auto_working: false });
    assert_eq!(select::auto_toggle(&l, &v, "S1"), Some(PlayerCommand::SetAutoWorking { entrance: s("S1"), on: true }), "any route");
    v.routes.insert(s("S4-S1"), RouteView { state: RouteState::Locked, auto_working: false });
    let labels: Vec<String> = select::signal_menu(&l, &v, "S4").into_iter().map(|m| m.label).collect();
    assert_eq!(labels, ["Cancel route S4 to S1"], "S4 is permanently automatic: no auto-working");
    assert_eq!(select::auto_toggle(&l, &v, "S4"), None);
    assert_eq!(select::click(&l, None, &ExitName::Signal(s("S9"))), Click::Ignore, "no routes from S9");
    assert_eq!(select::click(&l, Some("S1"), &ExitName::Node(s("Z"))), Click::Clear);
}

/// Only a controlled signal that starts a route gets a ○A button, and only
/// in your own area (or for a spectator, everywhere).
#[test]
fn auto_buttons_go_beside_controlled_signals_with_routes() {
    let l = auto_layout();
    assert!(select::has_auto_button(&l, "S1"));
    assert!(!select::has_auto_button(&l, "S4"), "permanently automatic");
    assert!(!select::has_auto_button(&l, "S9"), "starts no route");
    let mut theirs = l.clone();
    theirs.signals[0].operable = false;
    theirs.signals[0].area = s("B");
    assert!(!select::has_auto_button(&theirs, "S1"), "none on the fringe");
    theirs.area = None;
    assert!(select::has_auto_button(&theirs, "S1"), "a spectator sees it (grey, read-only)");
}

/// The ○A button sends exactly what the signal menu's auto-working entry
/// would, and nothing when the menu has none.
#[test]
fn the_auto_button_is_the_menus_auto_working_command() {
    let l = auto_layout();
    let mut v = empty_view();
    assert_eq!(select::auto_toggle(&l, &v, "S1"), None, "no route set from S1");
    assert!(!select::auto_working(&l, &v, "S1"));
    assert_eq!(select::describe_auto(&l, &v, "S1"), "Auto-working S1: set a route first");
    v.routes.insert(s("S1-S2"), RouteView { state: RouteState::Locked, auto_working: false });
    assert_eq!(select::auto_toggle(&l, &v, "S1"), Some(PlayerCommand::SetAutoWorking { entrance: s("S1"), on: true }));
    assert_eq!(select::describe_auto(&l, &v, "S1"), "Auto-working S1: off");
    v.routes.insert(s("S1-S2"), RouteView { state: RouteState::Locked, auto_working: true });
    assert_eq!(select::auto_toggle(&l, &v, "S1"), Some(PlayerCommand::SetAutoWorking { entrance: s("S1"), on: false }));
    assert!(select::auto_working(&l, &v, "S1"));
    assert_eq!(select::describe_auto(&l, &v, "S1"), "Auto-working S1: on");
    let mut theirs = l.clone();
    theirs.signals[0].operable = false;
    theirs.signals[0].area = s("B");
    assert_eq!(select::auto_toggle(&theirs, &v, "S1"), None, "not on the fringe or for a spectator");
    assert_eq!(select::describe_auto(&theirs, &v, "S1"), "Auto-working S1 (B): on", "whose it is, as a signal's hover says");
    theirs.area = None;
    assert_eq!(select::describe_auto(&theirs, &v, "S1"), "Auto-working S1 (B): on", "a spectator's too");
    v.routes.insert(s("S1-S2"), RouteView { state: RouteState::Cancelling, auto_working: true });
    assert!(!select::auto_working(&l, &v, "S1"), "a cancelling route is not live");
    assert_eq!(select::auto_toggle(&l, &v, "S1"), None);
    assert_eq!(select::describe_auto(&theirs, &v, "S1"), "Auto-working S1 (B): its route is cancelling");
}

#[test]
fn clicking_an_auto_button_never_touches_the_selection() {
    let mut t = Table::new("ann", Some("West"));
    t.app.click(&sig("W1"));
    t.app.click(&Target::Auto(s("W1")));
    assert_eq!(t.app.game().unwrap().selected(), Some("W1"));
    assert!(t.h.take_sent().is_empty(), "no route set from W1: nothing to toggle");
    assert_eq!(t.app.describe(&Target::Auto(s("W1"))), "Auto-working TAW1: set a route first");
    assert!(t.app.menu(&Target::Auto(s("W1"))).is_empty());

    t.app.click(&sig("A"));
    t.pump();
    t.run(1.0);
    assert!(t.view().routes.contains_key("W1-A"), "{:?}", t.view().routes);
    t.app.click(&sig("W2"));
    t.app.click(&Target::Auto(s("W1")));
    assert_eq!(t.app.game().unwrap().selected(), Some("W2"), "the selection is untouched");
    assert_eq!(
        t.h.take_sent(),
        [ClientFrame::Game(ClientMsg::Command { cmd: PlayerCommand::SetAutoWorking { entrance: s("W1"), on: true } })]
    );
    assert_eq!(t.app.describe(&Target::Auto(s("W1"))), "Auto-working TAW1: off", "until the game says so");
}

/// Real auto-working through the game: W1-A, auto-working on, stays set
/// behind 1E01 once it has passed.
#[test]
fn an_auto_worked_route_stays_set_after_a_train_passes() {
    let mut t = Table::new("ann", Some("West"));
    t.app.click(&sig("W1"));
    t.app.click(&sig("A"));
    t.app.click(&sig("A"));
    t.app.click(&Target::Exit(s("E")));
    t.pump();
    t.run(10.0);
    assert!(t.view().routes.contains_key("A-E"), "{:?}", t.view().routes);
    t.app.click(&Target::Auto(s("W1")));
    t.pump();
    t.run(0.5);
    assert!(t.view().routes["W1-A"].auto_working);
    assert!(select::auto_working(t.layout(), t.view(), "W1"));
    assert_eq!(t.app.describe(&Target::Auto(s("W1"))), "Auto-working TAW1: on");
    // 1E01 enters at W at 07:00 and runs W1 -> A -> E.
    let mut seen_in_tw2 = false;
    for _ in 0..600 {
        t.run(1.0);
        seen_in_tw2 |= t.view().sections["TW2"].occupied;
        if seen_in_tw2 && !t.view().sections["TW2"].occupied && !t.view().sections["TW1"].occupied {
            break;
        }
    }
    assert!(seen_in_tw2, "1E01 ran through W1-A");
    assert!(!t.view().sections["TW2"].occupied, "and has left it");
    let rv = &t.view().routes["W1-A"];
    assert_eq!((rv.state, rv.auto_working), (RouteState::Locked, true), "still set, still auto-working");
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
    assert_eq!(t.app.describe(&Target::Section(s("TW1"))), "Track: clear");
    assert_eq!(t.app.describe(&Target::Section(s("TP"))), "Track (East): clear");
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

/// Polish spec H2: a click that chooses nothing says why, once.
#[test]
fn a_click_that_chooses_nothing_says_why_once() {
    let mut spec = Table::new("sam", None);
    spec.app.click(&sig("W1"));
    spec.app.click(&sig("W1"));
    spec.app.click(&Target::Auto(s("W1")));
    assert_eq!(spec.log_lines(), [(s("You are watching: claim an area to signal"), false)]);
    let mut t = Table::new("ann", Some("West"));
    t.app.click(&sig("C"));
    t.app.click(&Target::Auto(s("W1")));
    assert_eq!(
        t.log_lines(),
        [(s("C is not in your area"), false), (s("Auto-working TAW1: set a route from it first"), false)]
    );
}

/// Fix round 1: the dead ○A click on a route being cancelled says so, not
/// that no route is set.
#[test]
fn a_dead_auto_click_on_a_cancelling_route_says_it_is_cancelling() {
    let t = Table::new("ann", Some("West"));
    let mut l = t.layout().clone();
    let mut v = t.view().clone();
    let r = l.routes.iter().find(|r| r.entrance == "W1").unwrap().name.clone();
    v.routes.insert(r, RouteView { state: RouteState::Cancelling, auto_working: false });
    l.signals.iter_mut().for_each(|s| s.operable = s.operable || s.name == "W1");
    assert_eq!(select::describe_auto(&l, &v, "W1"), "Auto-working TAW1: its route is cancelling");
}

/// Polish spec M3: hover text ends with what a click would do, and says
/// nothing where clicks do nothing for you.
#[test]
fn hints_say_what_a_click_does() {
    let mut t = Table::new("ann", Some("West"));
    assert_eq!(t.app.hint(&sig("W1")), Some("click: choose as entrance"));
    t.app.click(&sig("W1"));
    assert_eq!(t.app.hint(&sig("A")), Some("click: set the route to here"));
    assert_eq!(t.app.hint(&sig("W1")), Some("click again or Esc: forget the entrance"));
    assert_eq!(t.app.hint(&Target::Berth(s("BA"))), Some("right-click: interpose or cancel a headcode"));
    assert_eq!(t.app.hint(&Target::Section(s("TW1"))), None);
    assert_eq!(t.app.hint(&Target::Points(s("P"))), None, "East's points");
    let e = Table::new("eve", Some("East"));
    assert_eq!(e.app.hint(&Target::Points(s("P"))), Some("click: open the menu to swing them"));
    let spec = Table::new("sam", None);
    assert_eq!(spec.app.hint(&sig("W1")), None);
}

/// Review fix: points you work that cannot be swung now say why.
#[test]
fn busy_points_say_why_they_cannot_be_swung() {
    let mut e = Table::new("eve", Some("East"));
    let p = Target::Points(s("P"));
    e.app.command(PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse });
    e.pump();
    e.run(0.2);
    assert!(e.view().points["P"].moving, "{:?}", e.view().points);
    assert_eq!(e.app.hint(&p), Some("points moving: wait"));
    e.run(10.0);
    e.app.click(&sig("C"));
    e.app.click(&sig("W2"));
    e.pump();
    e.run(1.0);
    assert!(e.view().points["P"].locked, "{:?}", e.view().points);
    assert_eq!(e.app.hint(&p), Some("points locked by a route or train: they cannot be swung"));
}
