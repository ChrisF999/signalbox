//! Line-name files: one label per named line, hard errors, and the shipped files.

use serde_json::{Value, json};
use signalbox_core::network::Dir;
use signalbox_core::world::World;
use signalbox_core::world::file::WorldFile;
use ts2_import::lines::{self, LABEL_ABOVE, LineSpec, LinesError};

fn converted(name: &str) -> WorldFile {
    let dir = env!("CARGO_MANIFEST_DIR");
    ts2_import::convert(&std::fs::read_to_string(format!("{dir}/tests/data/{name}.json")).unwrap()).unwrap().world
}

fn shipped(name: &str) -> Vec<LineSpec> {
    let text = std::fs::read_to_string(format!("{}/../../layouts/{name}.lines.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    lines::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn line(name: &str, direction: Dir, through: &[&str]) -> LineSpec {
    LineSpec { name: name.to_string(), direction, through: through.iter().map(|s| s.to_string()).collect() }
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// The labels carrying an arrow (the converter's own labels have none).
fn line_labels(w: &WorldFile) -> Vec<Value> {
    w.layout["labels"].as_array().unwrap().iter().filter(|l| l.get("arrow").is_some()).cloned().collect()
}

/// Drain (the Waterloo & City): Bank at the left, Waterloo at the right. The
/// top road's signals 72–75 face right (the world's `up`), the bottom
/// road's 83–86 face left.
#[test]
fn each_line_gets_one_label_where_its_trains_run_to() {
    let mut w = converted("drain");
    let before = w.layout["labels"].as_array().unwrap().len();
    lines::apply(&mut w, &[line("WESTBOUND", Dir::Up, &["73", "74", "75"]), line("EASTBOUND", Dir::Down, &["T22", "84"])])
        .unwrap();
    assert_eq!(w.layout["labels"].as_array().unwrap().len(), before + 2);
    assert_eq!(
        line_labels(&w),
        [
            json!({"text": "WESTBOUND", "x": 1070.0, "y": 100.0 - LABEL_ABOVE, "arrow": [1.0, 0.0]}),
            json!({"text": "EASTBOUND", "x": 260.0, "y": 150.0 - LABEL_ABOVE, "arrow": [-1.0, 0.0]}),
        ],
        "WESTBOUND at the Waterloo end of 73–75, EASTBOUND at the Bank end of 83–84"
    );
    World::from_file(w).unwrap();
}

#[test]
fn unknown_names_and_signals_facing_the_wrong_way_are_hard_errors() {
    let mut w = converted("drain");
    let e = lines::apply(&mut w, &[line("X", Dir::Up, &["73", "nope", "T999"])]).unwrap_err();
    assert_eq!(e, LinesError::UnknownNames(names(&["T999", "nope"])));
    assert!(e.to_string().contains("`nope`"), "{e}");
    let e = lines::apply(&mut w, &[line("X", Dir::Up, &["73", "84", "83"])]).unwrap_err();
    assert_eq!(e, LinesError::WrongDirection(names(&["83", "84"])));
    assert_eq!(lines::apply(&mut w, &[line(" ", Dir::Up, &["73"])]), Err(LinesError::Empty));
    assert_eq!(lines::apply(&mut w, &[line("X", Dir::Up, &[])]), Err(LinesError::Empty));
    let e = lines::apply(&mut w, &[line("POINTS ONLY", Dir::Up, &["T1"])]).unwrap_err();
    assert_eq!(e, LinesError::Undrawn(names(&["POINTS ONLY"])), "T1 is a points section: only its legs, never drawn");
}

#[test]
fn a_failed_apply_leaves_the_world_untouched() {
    let mut w = converted("drain");
    let before = serde_json::to_string(&w).unwrap();
    assert!(lines::apply(&mut w, &[line("A", Dir::Up, &["73"]), line("B", Dir::Up, &["nope"])]).is_err());
    assert_eq!(serde_json::to_string(&w).unwrap(), before);
}

#[test]
fn a_world_without_a_drawing_cannot_be_labelled() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/fixtures/plain_line.json")).unwrap();
    let mut w: WorldFile = serde_json::from_str(&text).unwrap();
    assert_eq!(lines::apply(&mut w, &[line("X", Dir::Up, &["S1"])]), Err(LinesError::NoDrawing));
}

#[test]
fn parse_checks_the_fields() {
    assert!(matches!(lines::parse("{}"), Err(LinesError::Parse(_))));
    assert!(matches!(lines::parse(r#"[{"name": "X", "direction": "left", "through": ["1"]}]"#), Err(LinesError::Parse(_))));
    assert!(matches!(lines::parse(r#"[{"name": "X", "direction": "up", "through": [], "extra": 1}]"#), Err(LinesError::Parse(_))));
    assert_eq!(lines::parse("[]"), Ok(vec![]));
    assert_eq!(
        lines::parse(r#"[{"name": "UP MAIN", "direction": "down", "through": ["39,1V1"]}]"#),
        Ok(vec![line("UP MAIN", Dir::Down, &["39,1V1"])])
    );
}

#[test]
fn the_shipped_line_files_apply() {
    let texts = |w: &WorldFile| line_labels(w).iter().map(|l| l["text"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    let mut w = converted("liverpool-st");
    lines::apply(&mut w, &shipped("liverpool-st")).unwrap();
    assert_eq!(
        texts(&w),
        [
            "DOWN SUBURBAN", "UP SUBURBAN", "DOWN MAIN", "UP MAIN", "DOWN ELECTRIC", "UP ELECTRIC", "DOWN FAST", "UP FAST"
        ]
    );
    for l in line_labels(&w) {
        let down = l["text"].as_str().unwrap().starts_with("DOWN");
        assert_eq!(l["arrow"], json!([if down { 1.0 } else { -1.0 }, 0.0]), "Down is away from Liverpool Street: {l}");
    }
    World::from_file(w).unwrap();
    let mut w = converted("drain");
    lines::apply(&mut w, &shipped("drain")).unwrap();
    assert_eq!(texts(&w), ["WESTBOUND", "EASTBOUND"]);
    let mut w = converted("gretz-armainvilliers");
    let before = serde_json::to_string(&w).unwrap();
    lines::apply(&mut w, &shipped("gretz-armainvilliers")).unwrap();
    assert_eq!(serde_json::to_string(&w).unwrap(), before, "no line names known for Gretz yet");
}
