use ts2_import::ts2::{Item, Port, Ts2};

fn load(name: &str) -> Ts2 {
    let path = format!("{}/tests/data/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn real_files_parse_with_the_surveyed_counts() {
    for (name, items, routes, services, trains, types) in [
        ("drain", 91, 22, 16, 3, 1),
        ("liverpool-st", 608, 119, 1373, 692, 17),
        ("gretz-armainvilliers", 459, 121, 73, 43, 10),
    ] {
        let t = load(name);
        assert_eq!(
            (t.track_items.len(), t.routes.len(), t.services.len(), t.trains.len(), t.train_types.len()),
            (items, routes, services, trains, types),
            "{name}"
        );
    }
}

#[test]
fn links_and_ports() {
    let t = load("mini");
    let p = &t.track_items["5"];
    assert_eq!(p.link(Port::Prev), Some("4"));
    assert_eq!(p.link(Port::Rev), Some("8"));
    assert_eq!(p.port_to("6"), Some(Port::Next));
    assert!(matches!(t.track_items["7"], Item::SignalItem(ref s) if s.signal_type == "BUFFER"));
    assert_eq!(t.trains[0].train_head.previous_ti, "1");
    assert_eq!(t.routes["1"].directions["5"], 0);
}

#[test]
fn report_groups_warnings() {
    let mut r = ts2_import::report::Report::default();
    r.warn(ts2_import::report::ROUTE_DROPPED, "a");
    r.warn(ts2_import::report::ROUTE_DROPPED, "b");
    r.warn(ts2_import::report::CALL_DROPPED, "c");
    assert_eq!(r.count(ts2_import::report::ROUTE_DROPPED), 2);
    assert_eq!(r.summary().len(), 2);
    assert!(r.render().contains("route dropped"));
}
