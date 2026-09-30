//! The converter CLI's `--areas` and `--lines` flags.

use std::path::PathBuf;
use std::process::{Command, Output};

use signalbox_core::world::file::WorldFile;

const DRAIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/drain.json");
const DRAIN_AREAS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../layouts/drain.areas.json");
const DRAIN_LINES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../layouts/drain.lines.json");

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ts2-import")).args(args).output().expect("ts2-import runs")
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("signalbox-ts2-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn areas_flag_splits_the_world() {
    let dir = temp_dir("ok");
    let out = dir.join("drain.json");
    let o = cli(&[DRAIN, "-o", out.to_str().unwrap(), "--areas", DRAIN_AREAS]);
    assert!(o.status.success(), "{}", stderr(&o));
    let w: WorldFile = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(w.areas.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Bank", "Waterloo"]);
    assert!(stderr(&o).contains("area Bank: 11 sections, 4 signals"), "{}", stderr(&o));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bad_areas_file_fails_with_the_names_and_writes_nothing() {
    let dir = temp_dir("bad");
    let bad = dir.join("bad.areas.json");
    std::fs::write(&bad, r#"{"schema": 1, "boundaries": ["nope"], "areas": [{"name": "A", "seeds": ["72"]}]}"#).unwrap();
    let out = dir.join("drain.json");
    let o = cli(&[DRAIN, "-o", out.to_str().unwrap(), "--areas", bad.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(stderr(&o).contains("`nope`"), "{}", stderr(&o));
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn areas_flag_needs_a_value() {
    let o = cli(&[DRAIN, "-o", "/tmp/unused.json", "--areas"]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn lines_flag_labels_the_lines() {
    let dir = temp_dir("lines");
    let out = dir.join("drain.json");
    let o = cli(&[DRAIN, "-o", out.to_str().unwrap(), "--areas", DRAIN_AREAS, "--lines", DRAIN_LINES]);
    assert!(o.status.success(), "{}", stderr(&o));
    let w: WorldFile = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    let texts: Vec<&str> = w.layout["labels"].as_array().unwrap().iter().filter_map(|l| l["text"].as_str()).collect();
    assert!(texts.contains(&"WESTBOUND") && texts.contains(&"EASTBOUND"), "{texts:?}");
    assert_eq!(w.layout["box_prefix"], "W");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bad_lines_file_fails_with_the_names_and_writes_nothing() {
    let dir = temp_dir("badlines");
    let bad = dir.join("bad.lines.json");
    std::fs::write(&bad, r#"[{"name": "UP", "direction": "up", "through": ["nope"]}]"#).unwrap();
    let out = dir.join("drain.json");
    let o = cli(&[DRAIN, "-o", out.to_str().unwrap(), "--lines", bad.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(stderr(&o).contains("`nope`"), "{}", stderr(&o));
    assert!(!out.exists());
    let o = cli(&[DRAIN, "-o", out.to_str().unwrap(), "--lines"]);
    assert_eq!(o.status.code(), Some(2), "--lines needs a value");
    let _ = std::fs::remove_dir_all(&dir);
}
