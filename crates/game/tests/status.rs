//! What the process shell reads from a game (C2): status, the newest
//! snapshot, save errors, the empty-game pause, and the lobby's read-only
//! save summary.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::*;
use game::Game;
use game::save::{SaveError, read_summary};
use protocol::*;
use rusqlite::Connection;

fn s(x: &str) -> String {
    x.to_string()
}

fn temp_save(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("signalbox-status-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.sqlite"));
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }
    path
}

fn open(path: &Path) -> Connection {
    Connection::open(path).unwrap()
}

#[test]
fn status_reports_clock_holders_and_players() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    join(&mut g, "bob", None);
    g.advance(1.0);
    let st = g.status();
    assert_eq!((st.tick, st.paused, st.speed, st.connected), (10, false, 1, 2));
    assert_eq!(st.sim_time, 7.0 * 3600.0 + 1.0);
    assert_eq!(st.holders, BTreeMap::from([(s("East"), None), (s("West"), Some(s("alice")))]));
    assert_eq!(st.players, [(s("alice"), true), (s("bob"), true)]);
    assert_eq!(st.save_busy, Duration::ZERO, "no save, no time spent saving");
    g.disconnect("alice");
    g.disconnect("bob");
    let st = g.status();
    assert_eq!(st.players, [(s("alice"), false)], "a holder stays through grace; a spectator is forgotten");
    assert_eq!(st.connected, 0);
}

#[test]
fn status_counts_what_reached_the_sim() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    command(&mut g, "alice", set_route("W1", ExitName::Signal(s("A"))));
    run_to_tick(&mut g, 20);
    let st = g.status();
    assert_eq!(st.stats.player_commands, 1);
    assert_eq!((st.stats.spads, st.stats.collisions, st.stats.invariant_violations), (0, 0, 0));
}

#[test]
fn pause_for_empty_stops_the_clock_and_drops_the_vote() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    join(&mut g, "bob", Some("East"));
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Speed { x: 4 } });
    assert!(g.clock().vote.is_some(), "bob has not agreed yet");
    g.pause_for_empty();
    assert!(g.clock().paused);
    assert!(g.clock().vote.is_none());
    let tick = g.sim().tick();
    g.advance(5.0);
    assert_eq!(g.sim().tick(), tick, "paused");
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Resume });
    send(&mut g, "bob", ClientMsg::Vote { proposal: Proposal::Resume });
    assert!(!g.clock().paused, "players resume it by vote");
    assert_eq!(g.clock().speed, 1);
}

#[test]
fn last_snapshot_tick_follows_saves_and_resumes() {
    assert_eq!(game().last_snapshot_tick(), None);
    let path = temp_save("last");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    assert_eq!(g.last_snapshot_tick(), Some(0));
    run_to_tick(&mut g, 25);
    assert!(g.save_now().is_empty());
    assert_eq!(g.last_snapshot_tick(), Some(25));
    run_to_tick(&mut g, 40);
    assert!(g.status().save_busy > Duration::ZERO);
    drop(g);
    let g = Game::resume(&path).unwrap();
    assert_eq!(g.last_snapshot_tick(), Some(25));
}

#[test]
fn save_errors_are_kept_for_the_shell_with_nobody_connected() {
    let path = temp_save("errors");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    open(&path).execute("DROP TABLE snapshots", []).unwrap();
    assert!(g.save_now().is_empty(), "nobody connected, nobody told");
    assert_eq!(g.last_snapshot_tick(), Some(0), "a failed snapshot does not count");
    let errors = g.take_save_errors();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("snapshots"), "{errors:?}");
    assert!(g.take_save_errors().is_empty(), "taken once");
    join(&mut g, "alice", Some("West"));
    let out = g.save_now();
    assert_eq!(error_codes(&out, "alice"), [codes::SAVE_FAILED]);
    assert_eq!(g.take_save_errors().len(), 1);
}

#[test]
fn read_summary_reads_meta_and_the_newest_snapshot() {
    let path = temp_save("summary");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    run_to_tick(&mut g, 30);
    assert!(g.save_now().is_empty());
    let while_running = read_summary(&path).unwrap();
    assert_eq!(while_running.tick, 30);
    drop(g);
    let sum = read_summary(&path).unwrap();
    assert_eq!((sum.layout.as_str(), sum.seed, sum.tick), ("twobox", 1, 30));
    assert_eq!(sum.areas, [s("West"), s("East")]);
    assert_eq!(sum.sim_time, 7.0 * 3600.0 + 3.0);
    assert!(sum.last_played > 1_700_000_000, "{}", sum.last_played);
    assert_eq!(sum.creator, None, "nobody recorded");
    assert!(Game::resume(&path).is_ok(), "reading left the save usable");
}

/// Owner decision 13: the save names its creator; a save without the row
/// (every save from before) reads as `None`.
#[test]
fn the_creator_is_kept_in_the_save() {
    let path = temp_save("creator");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    g.set_creator("Hackney & Bow's ann").unwrap();
    drop(g);
    assert_eq!(read_summary(&path).unwrap().creator.as_deref(), Some("Hackney & Bow's ann"));
    let mut g = Game::resume(&path).unwrap();
    assert!(g.save_now().is_empty());
    drop(g);
    assert_eq!(read_summary(&path).unwrap().creator.as_deref(), Some("Hackney & Bow's ann"), "resuming keeps it");
    let mut unsaved = Game::new(twobox(), meta());
    unsaved.set_creator("ann").unwrap();
}

#[test]
fn read_summary_rejects_missing_and_foreign_files() {
    assert!(matches!(read_summary(&temp_save("absent")), Err(SaveError::Bad(_))));
    let path = temp_save("foreign");
    let c = open(&path);
    c.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL); INSERT INTO meta VALUES ('schema', '1');")
        .unwrap();
    drop(c);
    let Err(SaveError::Bad(why)) = read_summary(&path) else { panic!("read a schema 1 save") };
    assert_eq!(why, "unsupported save schema 1");
}
