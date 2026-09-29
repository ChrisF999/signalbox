use std::collections::BTreeSet;

use signalbox_core::network::Dir;
use signalbox_core::world::file::NodeKindFile;
use ts2_import::graph::{self, Graph};
use ts2_import::report::{self, Report};
use ts2_import::ts2::Ts2;

fn load(name: &str) -> Ts2 {
    let path = format!("{}/tests/data/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn mini() -> (Graph, Report) {
    let mut r = Report::default();
    let ends = BTreeSet::from(["1".to_string()]);
    (graph::build(&load("mini"), &ends, &mut r).unwrap(), r)
}

fn node<'a>(g: &'a Graph, name: &str) -> &'a NodeKindFile {
    &g.nodes.iter().find(|n| n.name == name).unwrap_or_else(|| panic!("no node {name}")).kind
}

fn section_of(g: &Graph, seg: &str) -> String {
    g.segments.iter().find(|s| s.name == seg).unwrap_or_else(|| panic!("no segment {seg}")).section.clone()
}

#[test]
fn ends_are_classified() {
    let (g, r) = mini();
    assert!(matches!(node(&g, "N1"), NodeKindFile::Boundary), "trains enter at 1");
    assert!(matches!(node(&g, "N9"), NodeKindFile::BufferStop), "platform line within 400 m");
    assert!(matches!(node(&g, "N10"), NodeKindFile::Boundary), "no platform within 400 m");
    assert_eq!(g.boundaries, BTreeSet::from(["N1".to_string(), "N10".to_string()]));
    assert_eq!(r.count(report::END_CLASS), 3);
}

#[test]
fn points_get_legs_and_their_own_section() {
    let (g, _) = mini();
    assert!(matches!(node(&g, "N5"), NodeKindFile::Points { toe, normal, reverse, .. }
        if toe == "P5p" && normal == "P5n" && reverse == "P5r"));
    let legs = section_of(&g, "P5p");
    assert_eq!(section_of(&g, "P5n"), legs);
    assert_eq!(section_of(&g, "P5r"), legs);
    assert_ne!(section_of(&g, "L4"), legs);
    assert_ne!(section_of(&g, "L6"), legs);
}

#[test]
fn signals_split_sections_and_buffers_do_not() {
    let (g, _) = mini();
    assert_ne!(section_of(&g, "L2"), section_of(&g, "L4"));
    assert_eq!(section_of(&g, "L6"), section_of(&g, "S7.n-9.p"), "the buffer signal is dropped");
    assert_eq!(g.sections.len(), 5);
}

#[test]
fn signal_sits_at_the_end_it_is_approached_from() {
    let (g, _) = mini();
    assert_eq!(g.signals.len(), 1, "BUFFER is not a signal");
    let a = &g.signals[0];
    assert_eq!((a.name.as_str(), a.segment.as_str(), a.offset_m, a.direction), ("A", "L2", 500.0, Dir::Up));
    assert_eq!(a.aspects, 3);
    assert_eq!(a.sighting_m, 100.0);
    assert_eq!(g.buffer_ends["7"], "N9");
    assert!(g.berths.iter().any(|b| b.name == "BA" && b.signal.as_deref() == Some("A")));
    assert!(g.berths.iter().any(|b| b.name == "FN1" && b.boundary.as_deref() == Some("N1")));
}

#[test]
fn speeds_come_from_line_place_or_default() {
    let (g, _) = mini();
    let kmh = |s: &str| g.segments.iter().find(|x| x.name == s).unwrap().line_speed_kmh;
    assert!((kmh("L2") - 72.0).abs() < 1e-9, "default 20 m/s");
    assert!((kmh("L6") - 36.0).abs() < 1e-9, "place STA 10 m/s");
    assert!((kmh("L8") - 54.0).abs() < 1e-9, "own 15 m/s");
    assert!((kmh("P5r") - 54.0).abs() < 1e-9, "leg takes its neighbour's speed");
}

#[test]
fn platforms_are_the_lines_with_place_and_track() {
    let (g, _) = mini();
    assert_eq!(g.platforms.len(), 1);
    let p = &g.platforms[0];
    assert_eq!((p.place.as_str(), p.platform.as_str(), p.segment.as_str(), p.to_m), ("STA", "1", "L6", 200.0));
}

#[test]
fn duplicate_signal_names_are_made_unique() {
    let mut r = Report::default();
    let t = load("gretz-armainvilliers");
    let g = graph::build(&t, &BTreeSet::new(), &mut r).unwrap();
    let names: BTreeSet<&str> = g.signals.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names.len(), g.signals.len(), "names are unique");
    assert!(g.signals.iter().any(|s| s.name.starts_with("512#")));
    assert!(r.count(report::SIGNAL_TYPE) > 0, "French types are approximated");
}

#[test]
fn real_graphs_build_and_flat_crossings_merge() {
    for (name, crossings) in [("drain", 1), ("liverpool-st", 23), ("gretz-armainvilliers", 3)] {
        let mut r = Report::default();
        graph::build(&load(name), &BTreeSet::new(), &mut r).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(r.count(report::CROSSING), crossings, "{name}");
    }
}

#[test]
fn signals_sit_on_section_boundaries() {
    for name in ["drain", "liverpool-st"] {
        let mut r = Report::default();
        graph::build(&load(name), &BTreeSet::new(), &mut r).unwrap();
        assert_eq!(r.count(report::SIGNAL_OFF_BOUNDARY), 0, "{name}");
    }
}
