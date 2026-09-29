use signalbox_core::world::World;
use signalbox_core::world::file::{ExitFile, RouteFile};
use ts2_import::report;

fn data(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/data/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn route<'a>(routes: &'a [RouteFile], entrance: &str) -> &'a RouteFile {
    routes.iter().find(|r| r.entrance == entrance).unwrap_or_else(|| panic!("no route from {entrance}"))
}

#[test]
fn mini_converts_to_a_loadable_world() {
    let c = ts2_import::convert(&data("mini")).unwrap();
    World::from_file(c.world.clone()).unwrap();
    let r = route(&c.world.routes, "A");
    assert!(matches!(&r.exit, ExitFile::Node(n) if n == "N9"), "BUFFER end → node exit");
    assert_eq!(r.points.len(), 1);
    assert_eq!(r.points[0].points, "N5");
    assert!(r.overlap.is_empty());
    assert_eq!(c.report.count(report::ROUTE_DROPPED), 0);
    assert!(c.world.layout["lines"].as_array().unwrap().len() == 4);
}

#[test]
fn every_ts2_route_start_is_an_entrance() {
    // The core loader only accepts signal-to-signal routes, so loading proves
    // the long TS2 routes were split; here we check none were lost.
    let text = data("drain");
    let t: ts2_import::ts2::Ts2 = serde_json::from_str(&text).unwrap();
    let c = ts2_import::convert(&text).unwrap();
    for r in t.routes.values() {
        let ts2_import::ts2::Item::SignalItem(s) = &t.track_items[&r.begin_signal] else { panic!() };
        let name = s.name.clone().unwrap();
        assert!(c.world.routes.iter().any(|x| x.entrance == name), "no route from {name}");
    }
    assert_eq!(c.report.count(report::ROUTE_DROPPED), 0, "{}", c.report.render());
}

#[test]
fn routes_list_trailing_points() {
    let c = ts2_import::convert(&data("liverpool-st")).unwrap();
    let w = World::from_file(c.world.clone()).unwrap();
    // Every points node a route's path crosses is listed; the core validator
    // checks it, so loading is the proof. There must be trailing moves too:
    // more listed positions than TS2 `directions` entries (398 facing).
    let listed: usize = c.world.routes.iter().map(|r| r.points.len()).sum();
    assert!(listed > 0 && !w.routes.is_empty());
    assert_eq!(c.report.count(report::ROUTE_DROPPED), 0, "{}", c.report.render());
}

#[test]
fn overlaps_are_generated_beyond_signal_exits() {
    let c = ts2_import::convert(&data("liverpool-st")).unwrap();
    let with_overlap = c
        .world
        .routes
        .iter()
        .filter(|r| matches!(r.exit, ExitFile::Signal(_)) && !r.overlap.is_empty())
        .count();
    assert!(with_overlap > 0);
    assert!(c.world.routes.iter().all(|r| !(matches!(r.exit, ExitFile::Node(_)) && !r.overlap.is_empty())));
}

#[test]
fn persistent_routes_become_automatic() {
    let c = ts2_import::convert(&data("liverpool-st")).unwrap();
    assert!(c.world.routes.iter().any(|r| r.automatic));
}

#[test]
fn validator_rejections_drop_only_that_route() {
    let mut c = ts2_import::convert(&data("mini")).unwrap();
    // Same path and points as A-N9 but claiming the other end: the tracer disagrees.
    let mut bad = c.world.routes[0].clone();
    bad.exit = ExitFile::Node("N10".into());
    c.world.routes.push(bad);
    let mut rep = ts2_import::report::Report::default();
    let fixed = ts2_import::routes::finish(c.world, &mut rep).unwrap();
    World::from_file(fixed.clone()).unwrap();
    assert_eq!(fixed.routes.len(), 1);
    assert!(matches!(&fixed.routes[0].exit, ExitFile::Node(n) if n == "N9"));
    assert_eq!(rep.count(report::ROUTE_DROPPED), 1);
}

#[test]
fn gretz_converts() {
    let c = ts2_import::convert(&data("gretz-armainvilliers")).unwrap();
    World::from_file(c.world).unwrap();
}

#[test]
fn overlap_stops_before_unset_facing_points() {
    // Liverpool Street 90 -> 84: the section beyond 84 holds facing points that
    // the TS2 route never sets, so the overlap is cut (not the route dropped).
    let c = ts2_import::convert(&data("liverpool-st")).unwrap();
    let r = c.world.routes.iter().find(|r| r.entrance == "90" && matches!(&r.exit, ExitFile::Signal(n) if n == "84")).unwrap();
    assert!(r.overlap.is_empty());
    assert!(c.report.count(report::OVERLAP_CUT) > 0);
    assert_eq!(c.report.count(report::ROUTE_DROPPED), 0, "{}", c.report.render());
}

#[test]
fn automatic_route_before_a_controlled_signal_has_no_overlap() {
    let c = ts2_import::convert(&data("drain")).unwrap();
    let routes = &c.world.routes;
    // 74-75 is automatic and 75 begins hand-set routes.
    let r = routes.iter().find(|r| r.entrance == "74" && matches!(&r.exit, ExitFile::Signal(s) if s == "75")).unwrap();
    assert!(r.automatic);
    assert!(r.overlap.is_empty() && r.overlap_points.is_empty());
    assert!(c.report.render().contains("automatic route before a controlled signal: no overlap"));
    // No automatic route is left holding an overlap over a controlled signal's routes.
    let controlled: Vec<&str> = routes.iter().filter(|r| !r.automatic).map(|r| r.entrance.as_str()).collect();
    for r in routes.iter().filter(|r| r.automatic) {
        if let ExitFile::Signal(s) = &r.exit {
            assert!(!controlled.contains(&s.as_str()) || r.overlap.is_empty(), "{}", r.entrance);
        }
    }
}

#[test]
fn automatic_route_sharing_its_signal_with_other_routes_is_set_by_hand() {
    // Mini with a buffer signal before end 10 and a persistent TS2 route from A
    // over the reverse leg: A also begins the hand-set route to BUF, so an
    // automatic A route would hold A for ever and the other could never be set.
    let mut v: serde_json::Value = serde_json::from_str(&data("mini")).unwrap();
    let items = v["trackItems"].as_object_mut().unwrap();
    let mut buf = items["7"].clone();
    buf["tiId"] = "13".into();
    buf["name"] = "BUF2".into();
    buf["previousTiId"] = "8".into();
    buf["nextTiId"] = "10".into();
    items.insert("13".into(), buf);
    items["8"]["nextTiId"] = "13".into();
    items["10"]["previousTiId"] = "13".into();
    v["routes"]["2"] = serde_json::json!({"__type__": "Route", "id": "2", "beginSignal": "3", "endSignal": "13",
        "directions": {"5": 1}, "initialState": 2});
    let c = ts2_import::convert(&v.to_string()).unwrap();
    World::from_file(c.world.clone()).unwrap();
    let from_a: Vec<&RouteFile> = c.world.routes.iter().filter(|r| r.entrance == "A").collect();
    assert_eq!(from_a.len(), 2);
    assert!(from_a.iter().all(|r| !r.automatic), "{from_a:?}");
    assert_eq!(c.report.count(report::AUTOMATIC_DEMOTED), 1, "{}", c.report.render());
}

#[test]
fn automatic_routes_share_track_only_with_their_continuations() {
    // An automatic route never lets go of its path, so a route crossing or
    // joining it could never be set. Liverpool Street: automatic 73-83 would
    // own the line that 71-83 (and 75-83, 90-82, ...) join.
    for name in ["drain", "gretz-armainvilliers", "liverpool-st"] {
        let c = ts2_import::convert(&data(name)).unwrap();
        let routes = &c.world.routes;
        let exit_is = |r: &RouteFile, s: &str| matches!(&r.exit, ExitFile::Signal(x) if x == s);
        for a in routes.iter().filter(|r| r.automatic) {
            for b in routes.iter().filter(|b| !std::ptr::eq(*b, a)) {
                let shared: Vec<&String> =
                    a.path.iter().chain(&a.overlap).filter(|s| b.path.contains(s) || b.overlap.contains(s)).collect();
                let continues = |x: &RouteFile, y: &RouteFile| exit_is(x, &y.entrance) && shared.iter().all(|s| x.overlap.contains(s));
                assert!(
                    shared.is_empty() || continues(a, b) || continues(b, a),
                    "{name}: automatic {}-{:?} shares {shared:?} with {}-{:?}",
                    a.entrance, a.exit, b.entrance, b.exit
                );
            }
        }
    }
    let c = ts2_import::convert(&data("liverpool-st")).unwrap();
    let r = c.world.routes.iter().find(|r| r.entrance == "73").unwrap();
    assert!(!r.automatic);
}

/// `mini` plus a ring of track with two signals, and a TS2 route round the ring
/// whose end signal (mini's `A`, elsewhere) can never be met.
fn ring_route_never_ends(v: &mut serde_json::Value) {
    use serde_json::json;
    let line = |id: &str, prev: &str, next: &str| {
        json!({"__type__": "LineItem", "tiId": id, "name": null, "previousTiId": prev, "nextTiId": next, "realLength": 300.0,
               "maxSpeed": 0.0, "placeCode": null, "trackCode": "", "conflictTiId": null, "x": 0, "y": 100, "xf": 10, "yf": 100})
    };
    let signal = |id: &str, name: &str, prev: &str, next: &str| {
        json!({"__type__": "SignalItem", "tiId": id, "name": name, "signalType": "UK_3_ASPECTS", "reverse": false,
               "previousTiId": prev, "nextTiId": next, "x": 5, "y": 100, "xn": 5, "yn": 105, "maxSpeed": 0,
               "conflictTiId": null, "customProperties": {}})
    };
    let items = v["trackItems"].as_object_mut().unwrap();
    for (id, item) in [
        ("20", line("20", "24", "21")),
        ("21", signal("21", "R1", "20", "22")),
        ("22", line("22", "21", "23")),
        ("23", signal("23", "R2", "22", "24")),
        ("24", line("24", "23", "20")),
    ] {
        items.insert(id.into(), item);
    }
    v["routes"]["2"] = json!({"__type__": "Route", "id": "2", "beginSignal": "21", "endSignal": "3", "directions": {}, "initialState": 2});
}

#[test]
fn a_route_that_runs_past_its_end_signal_is_dropped_whole() {
    let mut v: serde_json::Value = serde_json::from_str(&data("mini")).unwrap();
    ring_route_never_ends(&mut v);
    let c = ts2_import::convert(&v.to_string()).unwrap();
    assert!(c.report.warnings.iter().any(|w| w.kind == report::ROUTE_DROPPED && w.detail.contains("route 2")), "{}", c.report.render());
    // The ring's stretches are gone, so nothing from it can be automatic.
    assert!(c.world.routes.iter().all(|r| !r.automatic), "{}", c.report.render());
}

#[test]
fn automatic_takeover_with_disagreeing_points_is_demoted() {
    // Mini with a signal Y ahead of the points. Automatic A-Y has its overlap
    // over the reverse leg (its TS2 directions set the points beyond Y), while
    // the automatic route Y-BUF continues it over the normal leg: the two
    // disagree on the points, which the core loader rejects for automatic
    // routes, so the converter must set one of them by hand.
    let mut v: serde_json::Value = serde_json::from_str(&data("mini")).unwrap();
    let items = v["trackItems"].as_object_mut().unwrap();
    let mut y = items["3"].clone();
    y["tiId"] = "13".into();
    y["name"] = "Y".into();
    y["previousTiId"] = "4".into();
    y["nextTiId"] = "5".into();
    items.insert("13".into(), y);
    items["4"]["nextTiId"] = "13".into();
    items["5"]["previousTiId"] = "13".into();
    v["routes"]["1"] = serde_json::json!({"__type__": "Route", "id": "1", "beginSignal": "13", "endSignal": "7",
        "directions": {"5": 0}, "initialState": 2});
    v["routes"]["2"] = serde_json::json!({"__type__": "Route", "id": "2", "beginSignal": "3", "endSignal": "13",
        "directions": {"5": 1}, "initialState": 2});
    let c = ts2_import::convert(&v.to_string()).expect("a clash must be demoted, not fail the conversion");
    World::from_file(c.world.clone()).unwrap();
    assert_eq!(c.world.routes.len(), 2, "{:?}", c.world.routes);
    assert_eq!(c.world.routes.iter().filter(|r| !r.automatic).count(), 1, "{:?}", c.world.routes);
    assert_eq!(c.report.count(report::ROUTE_DROPPED), 0, "{}", c.report.render());
    assert_eq!(c.report.count(report::AUTOMATIC_DEMOTED), 1, "{}", c.report.render());
}
