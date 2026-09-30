//! Views are built from the sim's state for each player's visible set.

mod common;

use std::collections::BTreeMap;

use common::*;
use game::areas::{AreaMap, Visibility};
use game::view::{Shared, build_view};
use protocol::{Aspect, Held, PointsPos, PointsView, RouteState, RouteView, SectionView, View};
use signalbox_core::events::Command;
use signalbox_core::routes::Exit;
use signalbox_core::sim::Sim;

struct Rig {
    sim: Sim,
    west: Visibility,
    east: Visibility,
    all: Visibility,
}

fn rig() -> Rig {
    let w = twobox();
    let m = AreaMap::new(&w);
    let west = Visibility::of_area(&w, &m, area(&w, "West"));
    let east = Visibility::of_area(&w, &m, area(&w, "East"));
    let all = Visibility::spectator(&w, &m);
    Rig { sim: Sim::new(w, 1), west, east, all }
}

impl Rig {
    fn view(&self, vis: &Visibility) -> View {
        let shared = Shared {
            sim_time: self.sim.now_s(),
            speed: 2,
            paused: false,
            vote: None,
            holders: map(&[("East", "robot".to_string()), ("West", "alice".to_string())]),
        };
        build_view(&self.sim, vis, &shared, 4)
    }
}

#[test]
fn a_fresh_railway() {
    let r = rig();
    let v = r.view(&r.west);
    assert_eq!((v.seq, v.sim_time, v.speed, v.paused, v.vote.clone()), (4, 7.0 * 3600.0, 2, false, None));
    assert_eq!(v.holders["West"], "alice");
    assert_eq!(v.score, Some(0));
    assert_eq!(v.signals, map(&[("A", Aspect::Red), ("W1", Aspect::Red), ("W2", Aspect::Red)]));
    assert_eq!(v.points, map(&[("P", PointsView { position: PointsPos::Normal, moving: false, locked: false })]));
    let free = SectionView { occupied: false, held: Held::Free };
    assert_eq!(v.sections, map(&[("TP", free), ("TW1", free), ("TW2", free)]));
    assert!(v.routes.is_empty() && v.berths.is_empty());
    let all = r.view(&r.all);
    assert_eq!(all.score, None);
    assert_eq!((all.signals.len(), all.sections.len()), (5, 5));
}

#[test]
fn routes_points_and_holding_show_in_every_view_that_sees_them() {
    let mut r = rig();
    let w = r.sim.world().clone();
    let a = w.net.signal("A").unwrap();
    r.sim.submit(Command::SetRoute { entrance: a, exit: Exit::Node(w.net.node("N").unwrap()) });
    r.sim.step();
    let v = r.view(&r.west);
    assert_eq!(v.routes, map(&[("A-N", RouteView { state: RouteState::Setting, auto_working: false })]));
    assert_eq!(v.points["P"], PointsView { position: PointsPos::Reverse, moving: true, locked: true });
    assert_eq!(v.sections["TP"], SectionView { occupied: false, held: Held::Path });
    let e = r.view(&r.east);
    assert_eq!(e.routes, v.routes);
    assert_eq!(e.sections["TN"].held, Held::Path);

    r.sim.run_for(6.0);
    let v = r.view(&r.west);
    assert_eq!(v.routes["A-N"].state, RouteState::Locked);
    assert_eq!(v.points["P"], PointsView { position: PointsPos::Reverse, moving: false, locked: true });
    assert_eq!(v.signals["A"], Aspect::Green);

    // A cancelled route still under approach locking shows as cancelling.
    let mut st = r.sim.snapshot();
    let an = w.route_by_name("A-N").unwrap();
    st.il.routes[an.idx()].cancel = Some(120.0);
    r.sim = Sim::restore(w, st).unwrap();
    assert_eq!(r.view(&r.east).routes["A-N"].state, RouteState::Cancelling);
}

#[test]
fn occupancy_and_berths() {
    let mut r = rig();
    r.sim.run_for(1.0);
    let v = r.view(&r.west);
    assert!(v.sections["TW1"].occupied);
    assert_eq!(v.berths, map(&[("BW1", "1E01".to_string())]));
    assert!(r.view(&r.east).berths.is_empty(), "BW1 is not visible from East");
    assert_eq!(r.view(&r.all).berths, v.berths);
    assert_eq!(r.view(&r.all).sections["TW1"], SectionView { occupied: true, held: Held::Free });
}

#[test]
fn views_are_a_function_of_state() {
    let mut r = rig();
    r.sim.run_for(30.0);
    let restored = Sim::restore(r.sim.world().clone(), r.sim.snapshot()).unwrap();
    let a = r.view(&r.all);
    r.sim = restored;
    assert_eq!(r.view(&r.all), a);
    let empty: BTreeMap<String, String> = BTreeMap::new();
    assert_ne!(a.berths, empty);
}
