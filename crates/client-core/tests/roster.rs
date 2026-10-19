//! The players panel's data: area holders, presence and who watches.

use std::collections::BTreeMap;

use client_core::roster::{AreaRow, Holder, Person, Roster};
use protocol::View;

fn s(x: &str) -> String {
    x.to_string()
}

fn areas() -> Vec<String> {
    vec![s("East"), s("West")]
}

fn ws() -> BTreeMap<String, String> {
    BTreeMap::from([(s("East"), s("B")), (s("West"), s("A"))])
}

fn view(holders: &[(&str, &str)], players: &[(&str, bool)]) -> View {
    View {
        seq: 1,
        sim_time: 0.0,
        speed: 1,
        paused: false,
        vote: None,
        holders: holders.iter().map(|(a, h)| (s(a), s(h))).collect(),
        players: players.iter().map(|(n, up)| (s(n), *up)).collect(),
        score: None,
        signals: Default::default(),
        routes: Default::default(),
        points: Default::default(),
        sections: Default::default(),
        berths: Default::default(),
        trains: Default::default(),
    }
}

fn person(name: &str, you: bool, away: bool) -> Person {
    Person { name: s(name), you, away }
}

#[test]
fn rows_follow_the_layout_order_with_holder_workstation_and_robot() {
    let v = view(&[("East", "robot"), ("West", "ann")], &[("ann", true)]);
    let r = Roster::new(&areas(), &ws(), &v, "ann");
    assert_eq!(
        r.areas,
        [
            AreaRow { area: s("East"), workstation: Some(s("B")), holder: Holder::Robot },
            AreaRow { area: s("West"), workstation: Some(s("A")), holder: Holder::Player(person("ann", true, false)) },
        ]
    );
    assert!(r.watching.is_empty());
    assert_eq!((r.online(), r.robot_areas(), r.count_text().as_str()), (1, 1, "1 player"));
}

#[test]
fn a_holder_who_dropped_is_away_and_not_counted() {
    let v = view(&[("East", "bob"), ("West", "ann")], &[("ann", true), ("bob", false)]);
    let r = Roster::new(&areas(), &ws(), &v, "ann");
    assert_eq!(r.areas[0].holder, Holder::Player(person("bob", false, true)));
    assert_eq!((r.online(), r.count_text().as_str()), (1, "1 player"));
}

#[test]
fn spectators_are_listed_by_name_and_you_are_among_them() {
    let v = view(&[("East", "robot"), ("West", "ann")], &[("ann", true), ("zed", true), ("cy", true)]);
    let r = Roster::new(&areas(), &ws(), &v, "zed");
    assert_eq!(r.watching, [person("cy", false, false), person("zed", true, false)]);
    assert_eq!((r.online(), r.count_text().as_str()), (3, "3 players"));
}

#[test]
fn a_holder_is_never_also_listed_as_watching() {
    let v = view(&[("East", "ann"), ("West", "robot")], &[("ann", true), ("bob", true)]);
    let r = Roster::new(&areas(), &ws(), &v, "ann");
    assert_eq!(r.watching, [person("bob", false, false)]);
}

#[test]
fn a_server_without_a_players_list_still_shows_holders_and_you() {
    let v = view(&[("East", "bob"), ("West", "robot")], &[]);
    let r = Roster::new(&areas(), &ws(), &v, "ann");
    assert_eq!(r.areas[0].holder, Holder::Player(person("bob", false, false)));
    assert_eq!(r.watching, [person("ann", true, false)], "you are always in the list");
    assert_eq!(r.online(), 2);
}

#[test]
fn a_single_area_layout_has_no_workstation_letters() {
    let one = vec![s("Drain")];
    let w = BTreeMap::from([(s("Drain"), s("A"))]);
    let r = Roster::new(&one, &w, &view(&[("Drain", "robot")], &[("ann", true)]), "ann");
    assert_eq!(r.areas[0].workstation, None);
}

#[test]
fn everything_robot_and_nobody_else() {
    let r = Roster::new(&areas(), &ws(), &view(&[], &[("ann", true)]), "ann");
    assert!(r.areas.iter().all(|a| a.holder == Holder::Robot), "an area missing from the holders is the robot's");
    assert_eq!((r.robot_areas(), r.online()), (2, 1));
}
