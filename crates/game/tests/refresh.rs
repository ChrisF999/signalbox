//! Old saves get today's display data (polish spec §5): the layout file's
//! drawing and names when its network matches the save's, and the same
//! simulation either way.

mod common;

use std::path::PathBuf;

use common::*;
use game::save::refresh_display;
use game::{Game, GameMeta, Refresh};
use protocol::{ClientMsg, Proposal};
use serde_json::Value;

/// Drain as the image converts it now: box `W`, line names.
fn drain_now() -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    let read = |p: String| std::fs::read_to_string(p).unwrap();
    let mut w = ts2_import::convert(&read(format!("{dir}/../ts2-import/tests/data/drain.json"))).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&read(format!("{dir}/../../layouts/drain.areas.json"))).unwrap()).unwrap();
    ts2_import::lines::apply(&mut w, &ts2_import::lines::parse(&read(format!("{dir}/../../layouts/drain.lines.json"))).unwrap()).unwrap();
    serde_json::to_string(&w).unwrap()
}

/// Drain as saves from before the realism pass hold it: no box prefix,
/// workstation letters or line names, so the prefix falls back to `L`.
fn drain_before() -> String {
    let mut v: Value = serde_json::from_str(&drain_now()).unwrap();
    let l = v["layout"].as_object_mut().unwrap();
    l.remove("box_prefix");
    l.remove("workstations");
    l.get_mut("labels").unwrap().as_array_mut().unwrap().retain(|x| x.get("arrow").is_none());
    v.to_string()
}

fn temp_save(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("signalbox-refresh-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.sqlite"));
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }
    path
}

/// A saved Drain game from before the realism pass, a minute in.
fn old_save(name: &str) -> PathBuf {
    let path = temp_save(name);
    let mut g = Game::create(&path, &drain_before(), GameMeta { layout: "drain".into(), seed: 3 }).unwrap();
    run(&mut g, 60.0, 0.1);
    assert!(g.save_now().is_empty());
    path
}

fn prefix(g: &mut Game) -> String {
    g.connect("ann");
    g.layout_of("ann").unwrap().box_prefix
}

#[test]
fn the_drawing_and_names_come_from_the_layout_when_the_network_matches() {
    let mut before: Value = serde_json::from_str(&drain_before()).unwrap();
    before["options"]["start_time"] = "07:30:00".into();
    before["services"].as_array_mut().unwrap().truncate(1);
    let now: Value = serde_json::from_str(&drain_now()).unwrap();
    let out: Value = serde_json::from_str(&refresh_display(&before.to_string(), &now.to_string()).unwrap()).unwrap();
    assert_eq!(out["layout"], now["layout"], "today's drawing and names");
    assert_eq!(out["options"]["start_time"], "07:30:00", "the save's own start");
    assert_eq!(out["services"].as_array().unwrap().len(), 1, "the save's own timetable");
}

#[test]
fn a_different_network_or_a_bad_file_keeps_the_saves_own() {
    let before = drain_before();
    let mut now: Value = serde_json::from_str(&drain_now()).unwrap();
    now["routes"].as_array_mut().unwrap().pop();
    assert!(refresh_display(&before, &now.to_string()).unwrap_err().contains("`routes`"));
    let mut now: Value = serde_json::from_str(&drain_now()).unwrap();
    now["areas"][0]["name"] = "City".into();
    assert!(refresh_display(&before, &now.to_string()).unwrap_err().contains("`areas`"));
    assert!(refresh_display(&before, "{not json").unwrap_err().contains("unreadable"));
    let mut now: Value = serde_json::from_str(&drain_now()).unwrap();
    now.as_object_mut().unwrap().remove("layout");
    assert!(refresh_display(&before, &now.to_string()).unwrap_err().contains("no drawing"));
}

#[test]
fn an_old_drain_save_resumes_with_todays_names_and_the_same_sim() {
    let path = old_save("drain");
    let (mut old, r) = Game::resume_with_layout(&path, None).unwrap();
    assert_eq!((r, prefix(&mut old)), (Refresh::NotAsked, "L".to_string()), "the bug: the title's first letter");
    let (mut new, r) = Game::resume_with_layout(&path, Some(&drain_now())).unwrap();
    assert_eq!((r, prefix(&mut new)), (Refresh::Refreshed, "W".to_string()));
    // Display data never reaches the sim: both run on identically.
    for g in [&mut old, &mut new] {
        send(g, "ann", ClientMsg::Vote { proposal: Proposal::Resume });
        assert!(!g.clock().paused);
        run(g, 120.0, 0.1);
    }
    assert_eq!(old.sim().tick(), new.sim().tick());
    assert_eq!(old.sim().state_hash(), new.sim().state_hash());
    // Nothing was written back: a plain resume still shows the old prefix.
    let mut again = Game::resume(&path).unwrap();
    assert_eq!(prefix(&mut again), "L");
}

#[test]
fn a_layout_that_does_not_match_is_reported_and_ignored() {
    let path = old_save("mismatch");
    let (mut g, r) = Game::resume_with_layout(&path, Some(&liverpool_json())).unwrap();
    let Refresh::Kept(why) = r else { panic!("{r:?}") };
    assert!(why.contains("differ"), "{why}");
    assert_eq!(prefix(&mut g), "L");
}
