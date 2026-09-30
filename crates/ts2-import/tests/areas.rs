//! Area files: flood fill from seeds, boundary signals, hard errors, and the shipped layouts.

use serde_json::json;
use signalbox_core::world::World;
use signalbox_core::world::file::WorldFile;
use ts2_import::areas::{self, AreaCount, AreaSpec, AreasError, AreasFile};

const PLAIN_LINE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/fixtures/plain_line.json");

fn plain_line() -> WorldFile {
    serde_json::from_str(&std::fs::read_to_string(PLAIN_LINE).unwrap()).unwrap()
}

fn spec(boundaries: &[&str], areas: &[(&str, Vec<&str>)]) -> AreasFile {
    AreasFile {
        schema: 1,
        prefix: None,
        boundaries: boundaries.iter().map(|s| s.to_string()).collect(),
        areas: areas
            .iter()
            .map(|(n, seeds)| AreaSpec {
                name: n.to_string(),
                seeds: seeds.iter().map(|s| s.to_string()).collect(),
                workstation: None,
            })
            .collect(),
    }
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn section_area(w: &WorldFile, s: &str) -> String {
    w.sections.iter().find(|x| x.name == s).unwrap_or_else(|| panic!("no section {s}")).area.clone()
}

fn signal_area(w: &WorldFile, s: &str) -> String {
    w.signals.iter().find(|x| x.name == s).unwrap_or_else(|| panic!("no signal {s}")).area.clone()
}

/// plain_line: W -a- J1 -b- J2 -c- E, sections TA TB TC, S1 at the end of a (J1),
/// S2 at the end of b (J2).
#[test]
fn splits_plain_line_at_a_boundary_signal() {
    let mut w = plain_line();
    let counts = areas::apply(&mut w, &spec(&["S2"], &[("West", vec!["TA"]), ("East", vec!["TC"])])).unwrap();
    assert_eq!(
        counts,
        vec![
            AreaCount { name: "West".into(), sections: 2, signals: 2 },
            AreaCount { name: "East".into(), sections: 1, signals: 0 },
        ]
    );
    assert_eq!(w.areas.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["West", "East"]);
    assert_eq!([section_area(&w, "TA"), section_area(&w, "TB"), section_area(&w, "TC")], ["West", "West", "East"]);
    assert_eq!([signal_area(&w, "S1"), signal_area(&w, "S2")], ["West", "West"]);
    World::from_file(w).unwrap();
}

#[test]
fn signal_and_berth_seeds_resolve_to_sections() {
    for west in ["S1", "BW", "B2", "TB"] {
        let mut w = plain_line();
        areas::apply(&mut w, &spec(&["S2"], &[("West", vec![west]), ("East", vec!["TC"])]))
            .unwrap_or_else(|e| panic!("seed {west}: {e}"));
        assert_eq!(section_area(&w, "TA"), "West", "seed {west}");
    }
}

#[test]
fn unreached_sections_are_listed() {
    let mut w = plain_line();
    let e = areas::apply(&mut w, &spec(&["S1", "S2"], &[("West", vec!["TA"]), ("East", vec!["TC"])])).unwrap_err();
    assert_eq!(e, AreasError::Unreached(names(&["TB"])));
}

#[test]
fn doubly_reached_sections_are_listed() {
    let mut w = plain_line();
    let e = areas::apply(&mut w, &spec(&[], &[("West", vec!["TA"]), ("East", vec!["TC"])])).unwrap_err();
    assert_eq!(e, AreasError::DoublyReached(names(&["TA", "TB", "TC"])));
}

#[test]
fn unknown_names_are_listed_sorted() {
    let mut w = plain_line();
    let e = areas::apply(&mut w, &spec(&["ZZ"], &[("West", vec!["TA", "NOPE"]), ("East", vec!["TC"])])).unwrap_err();
    assert_eq!(e, AreasError::UnknownNames(names(&["NOPE", "ZZ"])));
}

#[test]
fn boundaries_must_be_signals() {
    let mut w = plain_line();
    let e = areas::apply(&mut w, &spec(&["TB", "B1"], &[("West", vec!["TA"]), ("East", vec!["TC"])])).unwrap_err();
    assert_eq!(e, AreasError::NotSignals(names(&["B1", "TB"])));
}

#[test]
fn boundary_signals_must_stand_at_a_segment_end() {
    let mut w = plain_line();
    w.signals[1].offset_m = 500.0;
    let e = areas::apply(&mut w, &spec(&["S2"], &[("West", vec!["TA"]), ("East", vec!["TC"])])).unwrap_err();
    assert_eq!(e, AreasError::NotAtSegmentEnd(names(&["S2"])));
}

#[test]
fn the_area_list_is_checked_first() {
    let mut w = plain_line();
    assert_eq!(areas::apply(&mut w, &spec(&["S2"], &[])), Err(AreasError::NoAreas));
    let dup = spec(&["S2"], &[("West", vec!["TA"]), ("West", vec!["TC"])]);
    assert_eq!(areas::apply(&mut w, &dup), Err(AreasError::DuplicateAreas(names(&["West"]))));
    let seedless = spec(&["S2"], &[("West", vec!["TA"]), ("East", vec![])]);
    assert_eq!(areas::apply(&mut w, &seedless), Err(AreasError::NoSeeds(names(&["East"]))));
}

#[test]
fn failed_apply_leaves_the_world_untouched() {
    let mut w = plain_line();
    let before = serde_json::to_string(&w).unwrap();
    assert!(areas::apply(&mut w, &spec(&["S1", "S2"], &[("West", vec!["TA"]), ("East", vec!["TC"])])).is_err());
    assert_eq!(serde_json::to_string(&w).unwrap(), before);
}

#[test]
fn names_are_opaque_strings() {
    let text = std::fs::read_to_string(PLAIN_LINE)
        .unwrap()
        .replace("\"S2\"", "\"39,1V1\"")
        .replace("\"TC\"", "\"T#3 & more\"");
    let mut w: WorldFile = serde_json::from_str(&text).unwrap();
    areas::apply(&mut w, &spec(&["39,1V1"], &[("Hackney & Bow", vec!["TA"]), ("East, far", vec!["T#3 & more"])]))
        .unwrap();
    assert_eq!(section_area(&w, "T#3 & more"), "East, far");
    assert_eq!(signal_area(&w, "39,1V1"), "Hackney & Bow");
    let e = areas::apply(&mut w, &spec(&["39,1V2"], &[("A", vec!["TA"])])).unwrap_err();
    assert_eq!(e, AreasError::UnknownNames(names(&["39,1V2"])));
    assert!(e.to_string().contains("`39,1V2`"), "{e}");
}

#[test]
fn parse_checks_schema_and_fields() {
    assert_eq!(areas::parse(r#"{"schema": 2, "areas": []}"#), Err(AreasError::Schema(2)));
    assert!(matches!(areas::parse(r#"{"schema": 1, "areas": [], "extra": 1}"#), Err(AreasError::Parse(_))));
    assert!(matches!(areas::parse("not json"), Err(AreasError::Parse(_))));
    let ok = areas::parse(r#"{"schema": 1, "boundaries": ["S2"], "areas": [{"name": "West", "seeds": ["TA"]}]}"#).unwrap();
    assert_eq!(ok, spec(&["S2"], &[("West", vec!["TA"])]));
    let full = areas::parse(r#"{"schema": 1, "prefix": "L", "areas": [{"name": "West", "seeds": ["TA"], "workstation": "B"}]}"#)
        .unwrap();
    assert_eq!((full.prefix.as_deref(), full.areas[0].workstation.as_deref()), (Some("L"), Some("B")));
}

/// plain_line split West | East, with an (empty) drawing to carry the prefixes.
fn drawn_split(prefix: Option<&str>, east: Option<&str>) -> Result<WorldFile, AreasError> {
    let mut w = plain_line();
    w.layout = json!({"lines": []});
    let mut sp = spec(&["S2"], &[("West", vec!["TA"]), ("East", vec!["TC"])]);
    sp.prefix = prefix.map(str::to_string);
    sp.areas[1].workstation = east.map(str::to_string);
    areas::apply(&mut w, &sp).map(|_| w)
}

#[test]
fn prefixes_default_to_the_title_and_the_area_order() {
    let w = drawn_split(None, None).unwrap();
    assert_eq!(w.title, "Plain line");
    assert_eq!(w.layout["box_prefix"], "P");
    assert_eq!(w.layout["workstations"], json!({"West": "A", "East": "B"}));
    assert_eq!(w.layout["lines"], json!([]), "the drawing is kept");
    World::from_file(w).unwrap();
}

#[test]
fn prefixes_from_the_file_win() {
    let w = drawn_split(Some("XYZ"), Some("Q")).unwrap();
    assert_eq!(w.layout["box_prefix"], "XYZ");
    assert_eq!(w.layout["workstations"], json!({"West": "A", "East": "Q"}));
}

#[test]
fn a_world_without_a_drawing_gets_no_prefixes() {
    let mut w = plain_line();
    areas::apply(&mut w, &spec(&["S2"], &[("West", vec!["TA"]), ("East", vec!["TC"])])).unwrap();
    assert!(w.layout.is_null(), "the game's defaults apply: {}", w.layout);
}

#[test]
fn bad_prefixes_and_letters_are_hard_errors() {
    for bad in ["", "ab", "ABCD", "L1", "É"] {
        assert_eq!(drawn_split(Some(bad), None).unwrap_err(), AreasError::BadPrefix(bad.to_string()), "{bad:?}");
    }
    for bad in ["", "b", "BB", "7"] {
        assert_eq!(drawn_split(None, Some(bad)).unwrap_err(), AreasError::BadWorkstations(names(&["East"])), "{bad:?}");
    }
    let e = drawn_split(None, Some("A")).unwrap_err();
    assert_eq!(e, AreasError::DuplicateWorkstations(names(&["A"])), "West has A by default");
    assert!(e.to_string().contains("`A`"), "{e}");
}

#[test]
fn prefix_defaults() {
    assert_eq!(areas::default_prefix("London Liverpool Street Station"), "L");
    assert_eq!(areas::default_prefix("2 boxes, été"), "B");
    assert_eq!(areas::default_prefix("42 — ½"), "");
    assert_eq!(areas::default_workstation(0), "A");
    assert_eq!(areas::default_workstation(25), "Z");
    assert_eq!(areas::default_workstation(26), "");
    assert!(areas::valid_prefix("LST") && !areas::valid_prefix("LSTX"));
    assert!(areas::valid_workstation("C") && !areas::valid_workstation("c"));
}

fn check_shipped(name: &str, want: &[(&str, usize, usize)]) -> WorldFile {
    let dir = env!("CARGO_MANIFEST_DIR");
    let ts2 = std::fs::read_to_string(format!("{dir}/tests/data/{name}.json")).unwrap();
    let text = std::fs::read_to_string(format!("{dir}/../../layouts/{name}.areas.json")).unwrap();
    let mut w = ts2_import::convert(&ts2).unwrap().world;
    let counts = areas::apply(&mut w, &areas::parse(&text).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
    let want: Vec<AreaCount> =
        want.iter().map(|&(n, sections, signals)| AreaCount { name: n.into(), sections, signals }).collect();
    assert_eq!(counts, want, "{name}");
    World::from_file(w.clone()).unwrap();
    w
}

#[test]
fn liverpool_street_has_three_boxes() {
    let w = check_shipped(
        "liverpool-st",
        &[("Liverpool Street", 185, 33), ("Bethnal Green", 77, 19), ("Hackney & Bow", 54, 23)],
    );
    for s in ["61", "63", "65"] {
        assert_eq!(signal_area(&w, s), "Liverpool Street", "{s}");
    }
    for s in ["64", "66", "68", "91", "93", "95"] {
        assert_eq!(signal_area(&w, s), "Bethnal Green", "{s}");
    }
    for s in ["90", "92", "94"] {
        assert_eq!(signal_area(&w, s), "Hackney & Bow", "{s}");
    }
    assert_eq!(w.layout["box_prefix"], "L");
    assert_eq!(w.layout["workstations"], json!({"Liverpool Street": "A", "Bethnal Green": "B", "Hackney & Bow": "C"}));
}

#[test]
fn drain_has_two_boxes() {
    let w = check_shipped("drain", &[("Bank", 11, 4), ("Waterloo", 24, 11)]);
    for s in ["72", "73", "82", "83"] {
        assert_eq!(signal_area(&w, s), "Bank", "{s}");
    }
    assert_eq!(w.layout["box_prefix"], "W", "the Waterloo & City, not the title's L");
    assert_eq!(w.layout["workstations"], json!({"Bank": "A", "Waterloo": "B"}));
}

#[test]
fn gretz_has_three_boxes() {
    let w = check_shipped(
        "gretz-armainvilliers",
        &[("Gretz", 123, 47), ("Tournan & Marles", 68, 26), ("Mortcerf & Coulommiers", 36, 22)],
    );
    assert_eq!(w.layout["box_prefix"], "G");
    assert_eq!(w.layout["workstations"], json!({"Gretz": "A", "Tournan & Marles": "B", "Mortcerf & Coulommiers": "C"}));
}
