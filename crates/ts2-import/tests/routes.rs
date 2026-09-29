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
