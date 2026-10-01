//! Display data (realism spec §2.1, §3): prefixes and workstation letters
//! with their defaults, and the simplifier per area and for spectators.

mod common;

use common::*;
use game::display::{Display, default_box_prefix, default_workstation, prefixes, simplifier};
use game::{Game, GameMeta};
use protocol::{ServerMsg, SimplifierCall, SimplifierRow};
use serde_json::{Value, json};
use signalbox_core::world::World;

fn twobox_mut(f: impl FnOnce(&mut Value)) -> World {
    let mut json: Value = serde_json::from_str(&twobox_json()).unwrap();
    f(&mut json);
    World::from_json(&json.to_string()).unwrap()
}

fn s(x: &str) -> String {
    x.to_string()
}

fn call(place: &str, arr: Option<f64>, dep: Option<f64>) -> SimplifierCall {
    SimplifierCall { place: s(place), platform: Some(s("1")), arr, dep, stops: true }
}

#[test]
fn prefixes_default_to_the_title_and_the_area_order() {
    let (b, ws) = prefixes(&twobox());
    assert_eq!(b, "T", "Two boxes");
    assert_eq!(ws, map(&[("West", s("A")), ("East", s("B"))]));
    assert_eq!(default_box_prefix("42 — été"), "T");
    assert_eq!(default_box_prefix("½"), "");
    assert_eq!((default_workstation(2), default_workstation(26)), (s("C"), s("")));
}

#[test]
fn prefixes_come_from_the_worlds_layout_and_bad_ones_fall_back() {
    let w = twobox_mut(|j| j["layout"] = json!({"box_prefix": "XY", "workstations": {"East": "Q", "West": "ab"}}));
    assert_eq!(prefixes(&w), (s("XY"), map(&[("West", s("A")), ("East", s("Q"))])), "West's `ab` is not a letter");
    let w = twobox_mut(|j| j["layout"] = json!({"box_prefix": "", "workstations": {}}));
    assert_eq!(prefixes(&w).0, "", "an empty prefix is a choice: no prefix");
    for bad in [json!("TOOLONG"), json!("lc"), json!(7), json!(null)] {
        let w = twobox_mut(|j| j["layout"] = json!({"box_prefix": bad}));
        assert_eq!(prefixes(&w).0, "T", "{bad}");
    }
    let w = twobox_mut(|j| j["layout"] = json!("not an object"));
    assert_eq!(prefixes(&w), (s("T"), map(&[("West", s("A")), ("East", s("B"))])));
}

/// twobox: 1E01 calls at EST 1 07:04–07:05 and 1N02 at NST 1 07:18–07:19,
/// both platforms in East; 2W03 and 2W04 have no calls.
#[test]
fn each_area_lists_the_services_calling_at_its_platforms() {
    let w = twobox();
    let east = simplifier(&w, w.net.area("East"));
    assert_eq!(
        east,
        [
            SimplifierRow {
                headcode: s("1E01"),
                origin: Some(s("EST")),
                destination: Some(s("EST")),
                calls: vec![call("EST", Some(25_440.0), Some(25_500.0))],
            },
            SimplifierRow {
                headcode: s("1N02"),
                origin: Some(s("NST")),
                destination: Some(s("NST")),
                calls: vec![call("NST", Some(26_280.0), Some(26_340.0))],
            },
        ]
    );
    assert!(simplifier(&w, w.net.area("West")).is_empty(), "West has no platforms");
    let all: Vec<String> = simplifier(&w, None).into_iter().map(|r| r.headcode).collect();
    assert_eq!(all, ["1E01", "1N02", "2W03", "2W04"], "spectators: every service, untimed ones last");
}

#[test]
fn calls_count_by_platform_or_by_place_and_rows_run_in_time_order() {
    let w = twobox_mut(|j| {
        j["services"].as_array_mut().unwrap().extend([
            json!({"headcode": "9Z99", "train_type": "EMU", "calls": [
                {"place": "EST", "dep": "06:59:30", "stop": false},
                {"place": "NST", "platform": "1", "arr": "07:30"}
            ]}),
            json!({"headcode": "0A00", "train_type": "EMU", "calls": [{"place": "EST", "platform": "1", "arr": "07:04"}]}),
        ]);
    });
    let east = simplifier(&w, w.net.area("East"));
    let order: Vec<&str> = east.iter().map(|r| r.headcode.as_str()).collect();
    assert_eq!(order, ["9Z99", "0A00", "1E01", "1N02"], "by first listed call, then headcode");
    let z = &east[0];
    assert_eq!((z.origin.as_deref(), z.destination.as_deref()), (Some("EST"), Some("NST")));
    assert_eq!(
        z.calls,
        [
            SimplifierCall { place: s("EST"), platform: None, arr: None, dep: Some(25_170.0), stops: false },
            SimplifierCall { place: s("NST"), platform: Some(s("1")), arr: Some(27_000.0), dep: None, stops: true },
        ],
        "a call without a platform counts when a platform of its place is in the area"
    );
    assert!(simplifier(&w, w.net.area("West")).is_empty());
}

#[test]
fn the_game_puts_them_in_every_layout() {
    let mut g = Game::new(twobox(), GameMeta { layout: s("twobox"), seed: 1 });
    let out = g.connect("sam");
    let ServerMsg::Layout(l) = &out[0].1 else { panic!("{out:?}") };
    assert_eq!((l.box_prefix.as_str(), l.workstations.len(), l.simplifier.len()), ("T", 2, 4));
    let out = g.handle("sam", protocol::ClientMsg::Claim { area: s("West") });
    let ServerMsg::Layout(l) = &out[0].1 else { panic!("{out:?}") };
    assert!(l.simplifier.is_empty(), "the claim's resync carries West's (empty) simplifier");
    assert_eq!(g.layout_of("sam").unwrap(), *l);
    let d = Display::from_world(g.sim().world());
    assert_eq!(d.simplifier(g.sim().world().net.area("East")).len(), 2);
}

/// Every layout carries the display headcodes that differ from the
/// headcode, for the whole world (a berth can hold any train), and none
/// when there are none.
#[test]
fn display_headcodes_go_into_every_layout() {
    let mut g = Game::new(twobox(), GameMeta { layout: s("twobox"), seed: 1 });
    let ServerMsg::Layout(l) = &g.connect("sam")[0].1 else { panic!() };
    assert!(l.display_headcodes.is_empty());
    let w = twobox_mut(|j| {
        j["services"][0]["display"] = json!("E1");
        j["services"][2]["display"] = json!("1N02");
    });
    let mut g = Game::new(w, GameMeta { layout: s("twobox"), seed: 1 });
    let ServerMsg::Layout(l) = &g.connect("sam")[0].1 else { panic!() };
    assert_eq!(l.display_headcodes, map(&[("1E01", s("E1"))]), "1N02 shows itself");
    let ServerMsg::Layout(l) = &g.handle("sam", protocol::ClientMsg::Claim { area: s("West") })[0].1 else { panic!() };
    assert_eq!(l.display_headcodes, map(&[("1E01", s("E1"))]), "whatever the area");
}

/// The three boxes of Liverpool Street, as shipped.
#[test]
fn liverpool_street_has_its_prefixes_and_a_simplifier_per_box() {
    let w = World::from_json(&liverpool_json()).unwrap();
    let (b, ws) = prefixes(&w);
    assert_eq!(b, "L");
    assert_eq!(ws, map(&[("Liverpool Street", s("A")), ("Bethnal Green", s("B")), ("Hackney & Bow", s("C"))]));
    let d = Display::from_world(&w);
    assert_eq!(d.simplifier(None).len(), w.services.len());
    for a in ["Liverpool Street", "Bethnal Green", "Hackney & Bow"] {
        let rows = d.simplifier(w.net.area(a));
        assert!(!rows.is_empty(), "{a}");
        let times: Vec<f64> = rows.iter().filter_map(|r| r.calls.iter().find_map(|c| c.arr.or(c.dep))).collect();
        assert!(times.windows(2).all(|p| p[0] <= p[1]), "{a} runs in time order");
    }
    let mut g = Game::new(w, GameMeta { layout: s("liverpool-st"), seed: 1 });
    let out = g.connect("sam");
    let size = serde_json::to_string(&out[0].1).unwrap().len();
    assert!(size < 1 << 20, "a spectator's layout is {size} bytes; the ipc frame limit is 4 MiB");
}
