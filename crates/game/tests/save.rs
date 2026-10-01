//! SQLite saves (spec §7): a saved, dropped and resumed game continues
//! exactly as an uninterrupted one.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use game::names::resolve;
use game::save::{KEEP_SNAPSHOTS, SaveDb, SaveError};
use game::{Game, GameError, ROBOT};
use protocol::*;
use rusqlite::Connection;
use signalbox_core::events::Command;

fn s(x: &str) -> String {
    x.to_string()
}

fn temp_save(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("signalbox-game-{}", std::process::id()));
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

fn snapshot_ticks(path: &Path) -> Vec<i64> {
    let c = open(path);
    let mut st = c.prepare("SELECT tick FROM snapshots ORDER BY tick").unwrap();
    let ticks: Vec<i64> = st.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    ticks
}

fn resume_err(path: &Path) -> GameError {
    match Game::resume(path) {
        Ok(_) => panic!("resumed a broken save"),
        Err(e) => e,
    }
}

/// Alice claims West again and resumes the clock.
fn rejoin(g: &mut Game) {
    join(g, "alice", Some("West"));
    send(g, "alice", ClientMsg::Vote { proposal: Proposal::Resume });
    assert!(!g.clock().paused);
}

/// Resume the clock of a game nobody should hold: claim, vote, release.
fn unpause(g: &mut Game) {
    join(g, "ops", Some("West"));
    send(g, "ops", ClientMsg::Vote { proposal: Proposal::Resume });
    send(g, "ops", ClientMsg::Release);
    assert!(!g.clock().paused);
}

#[test]
fn create_writes_meta_world_and_a_first_snapshot() {
    let path = temp_save("create");
    let g = Game::create(&path, &twobox_json(), meta()).unwrap();
    assert_eq!(g.meta(), &meta());
    drop(g);
    let c = open(&path);
    let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "wal");
    let mut st = c.prepare("SELECT key, value FROM meta ORDER BY key").unwrap();
    let rows: Vec<(String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect();
    let keys: Vec<&str> = rows.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["areas", "created", "last_played", "layout", "schema", "seed", "start"]);
    let get = |k: &str| rows.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()).unwrap();
    assert_eq!(
        (get("schema"), get("layout"), get("seed"), get("areas"), get("start")),
        (s("2"), s("twobox"), s("1"), s(r#"["West","East"]"#), s("07:00"))
    );
    assert!(get("created").parse::<u64>().is_ok(), "{}", get("created"));
    let world: String = c.query_row("SELECT json FROM world WHERE id = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(world, twobox_json());
    drop(st);
    drop(c);
    assert_eq!(snapshot_ticks(&path), [0]);
}

#[test]
fn create_refuses_an_existing_file() {
    let path = temp_save("exists");
    std::fs::write(&path, "").unwrap();
    assert!(matches!(Game::create(&path, &twobox_json(), meta()), Err(GameError::Save(_))));
}

#[test]
fn commands_are_logged_with_player_area_and_tick() {
    let path = temp_save("log");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    join(&mut g, "alice", Some("East"));
    command(&mut g, "alice", PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse });
    run_to_tick(&mut g, 11);
    drop(g);
    let c = open(&path);
    let mut st = c.prepare("SELECT tick, player, area, command FROM commands ORDER BY seq").unwrap();
    let rows: Vec<(i64, String, String, String)> =
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!((rows[0].0, rows[0].1.as_str(), rows[0].2.as_str()), (0, "alice", "East"));
    let first: Command = serde_json::from_str(&rows[0].3).unwrap();
    assert!(matches!(first, Command::SwingPoints { .. }), "{first:?}");
    let robot: Vec<&(i64, String, String, String)> = rows.iter().filter(|r| r.1 == ROBOT).collect();
    assert!(!robot.is_empty(), "the robot routes 1E01 at tick 10: {rows:?}");
    assert!(robot.iter().all(|r| r.0 == 10 && r.2 == "West"), "{robot:?}");
}

#[test]
fn keeps_the_newest_three_snapshots() {
    let path = temp_save("three");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    for t in [5, 10, 15, 20] {
        run_to_tick(&mut g, t);
        assert!(g.save_now().is_empty());
    }
    drop(g);
    assert_eq!(KEEP_SNAPSHOTS, 3);
    assert_eq!(snapshot_ticks(&path), [10, 15, 20]);
}

#[test]
fn autosaves_every_minute_of_running_time() {
    let path = temp_save("auto");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    run(&mut g, 61.0, 0.1);
    assert_eq!(snapshot_ticks(&path).len(), 2);
    join(&mut g, "alice", Some("West"));
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Pause });
    run(&mut g, 120.0, 1.0);
    assert_eq!(snapshot_ticks(&path).len(), 2, "no snapshots while paused");
}

#[test]
fn resumed_games_start_paused_at_1x_with_no_claims() {
    let path = temp_save("fresh");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    join(&mut g, "alice", Some("West"));
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Speed { x: 4 } });
    run(&mut g, 2.0, 0.1);
    drop(g);
    let r = Game::resume(&path).unwrap();
    assert!(r.clock().paused);
    assert_eq!(r.clock().speed, 1);
    assert_eq!(r.holder("West"), None);
    assert_eq!(r.meta(), &meta());
}

fn opening(g: &mut Game) {
    join(g, "alice", Some("West"));
    run_to_tick(g, 20);
    command(g, "alice", set_route("W1", ExitName::Signal(s("A"))));
    command(g, "alice", set_route("A", ExitName::Node(s("E"))));
    run_to_tick(g, 300);
}

fn middle(g: &mut Game) {
    command(g, "alice", PlayerCommand::Interpose { berth: s("BW2"), headcode: s("9Z99") });
    run_to_tick(g, 450);
    command(g, "alice", PlayerCommand::Interpose { berth: s("BW1"), headcode: s("8Z88") });
    run_to_tick(g, 555);
}

/// Spec §1 success criterion 2 and §12, in process.
#[test]
fn save_resume_continue_matches_an_uninterrupted_run() {
    let path = temp_save("resume");
    let mut reference = game();
    opening(&mut reference);
    middle(&mut reference);
    run_to_tick(&mut reference, 9000);

    let mut saved = Game::create(&path, &twobox_json(), meta()).unwrap();
    opening(&mut saved);
    assert!(saved.save_now().is_empty());
    middle(&mut saved);
    drop(saved);

    let mut resumed = Game::resume(&path).unwrap();
    assert_eq!(resumed.sim().tick(), 450, "positioned at the last logged tick");
    rejoin(&mut resumed);
    run_to_tick(&mut resumed, 9000);
    assert_eq!(resumed.sim().state_hash(), reference.sim().state_hash());
}

#[test]
fn save_now_right_after_a_command_resumes_exactly() {
    let path = temp_save("exact");
    let mut saved = Game::create(&path, &twobox_json(), meta()).unwrap();
    join(&mut saved, "alice", Some("West"));
    run_to_tick(&mut saved, 40);
    command(&mut saved, "alice", set_route("W1", ExitName::Signal(s("A"))));
    assert!(saved.save_now().is_empty());
    let want = saved.sim().snapshot();
    assert_eq!(want.queue.len(), 1);
    drop(saved);
    let resumed = Game::resume(&path).unwrap();
    assert_eq!(resumed.sim().snapshot(), want);
}

#[test]
fn resume_skips_commands_already_queued_in_the_snapshot() {
    let path = temp_save("queued");
    let w1a = set_route("W1", ExitName::Signal(s("A")));
    let ae = set_route("A", ExitName::Node(s("E")));
    let mut reference = game();
    join(&mut reference, "alice", Some("West"));
    run_to_tick(&mut reference, 5);
    command(&mut reference, "alice", w1a.clone());
    command(&mut reference, "alice", ae.clone());
    run_to_tick(&mut reference, 2000);

    let mut saved = Game::create(&path, &twobox_json(), meta()).unwrap();
    join(&mut saved, "alice", Some("West"));
    run_to_tick(&mut saved, 5);
    command(&mut saved, "alice", w1a.clone());
    assert!(saved.save_now().is_empty());
    command(&mut saved, "alice", ae.clone());
    drop(saved);

    let mut resumed = Game::resume(&path).unwrap();
    let w = resumed.sim().world().clone();
    assert_eq!(resumed.sim().tick(), 5);
    assert_eq!(resumed.sim().snapshot().queue, vec![resolve(&w, &w1a).unwrap(), resolve(&w, &ae).unwrap()]);
    rejoin(&mut resumed);
    run_to_tick(&mut resumed, 2000);
    assert_eq!(resumed.sim().state_hash(), reference.sim().state_hash());
}

#[test]
fn resuming_after_robot_commands_does_not_run_the_robot_twice() {
    let path = temp_save("robot");
    let mut reference = game();
    run_to_tick(&mut reference, 3000);

    let mut saved = Game::create(&path, &twobox_json(), meta()).unwrap();
    run_to_tick(&mut saved, 11);
    let robot = saved.stats().robot_commands;
    assert!(robot > 0, "the robot routes 1E01 at tick 10");
    drop(saved);

    let mut resumed = Game::resume(&path).unwrap();
    assert_eq!(resumed.sim().tick(), 10, "the robot's tick, with its commands queued");
    assert_eq!(resumed.sim().snapshot().queue.len(), robot);
    unpause(&mut resumed);
    run_to_tick(&mut resumed, 3000);
    assert_eq!(resumed.stats().robot_commands, reference.stats().robot_commands - robot);
    assert_eq!(resumed.sim().state_hash(), reference.sim().state_hash());
}

#[test]
fn broken_saves_fail_to_resume() {
    assert!(matches!(resume_err(&temp_save("missing")), GameError::Save(_)));

    let path = temp_save("corrupt");
    drop(Game::create(&path, &twobox_json(), meta()).unwrap());
    open(&path).execute("UPDATE snapshots SET state = '{}'", []).unwrap();
    assert!(matches!(resume_err(&path), GameError::Save(_)));

    let path = temp_save("mismatch");
    drop(Game::create(&path, &twobox_json(), meta()).unwrap());
    let other = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/fixtures/plain_line.json")).unwrap();
    open(&path).execute("UPDATE world SET json = ?1", [other]).unwrap();
    assert!(matches!(resume_err(&path), GameError::Resume(_)));
}

#[test]
fn save_failures_become_error_notices_and_the_game_goes_on() {
    let path = temp_save("fail");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    join(&mut g, "alice", Some("West"));
    join(&mut g, "sam", None);
    open(&path).execute("DROP TABLE commands", []).unwrap();
    let out = command(&mut g, "alice", set_route("W1", ExitName::Signal(s("A"))));
    assert_eq!(error_codes(&out, "alice"), [codes::SAVE_FAILED]);
    assert_eq!(error_codes(&out, "sam"), [codes::SAVE_FAILED]);
    g.advance(0.1);
    assert_eq!(g.sim().log().len(), 1, "the command still ran");
}

/// A game resumed on the robot's tick and saved again at once holds the
/// robot's commands in its snapshot queue; the next resume must still know
/// the robot already ran there.
#[test]
fn saving_a_resumed_game_before_it_runs_keeps_the_robot_from_running_twice() {
    let path = temp_save("robot-twice");
    let mut reference = game();
    run_to_tick(&mut reference, 3000);

    let mut saved = Game::create(&path, &twobox_json(), meta()).unwrap();
    run_to_tick(&mut saved, 11);
    let robot = saved.stats().robot_commands;
    drop(saved);
    let mut again = Game::resume(&path).unwrap();
    assert!(again.save_now().is_empty());
    drop(again);

    let mut resumed = Game::resume(&path).unwrap();
    assert_eq!(resumed.sim().tick(), 10);
    assert_eq!(resumed.sim().snapshot().queue.len(), robot);
    unpause(&mut resumed);
    run_to_tick(&mut resumed, 3000);
    assert_eq!(resumed.stats().robot_commands, reference.stats().robot_commands - robot);
    assert_eq!(resumed.sim().state_hash(), reference.sim().state_hash());
}

/// Commands a resumed sim holds in its queue still route a sim rejection
/// back to the player who sent them.
#[test]
fn rejections_of_commands_queued_before_a_resume_reach_their_sender() {
    let path = temp_save("reject");
    let mut saved = Game::create(&path, &twobox_json(), meta()).unwrap();
    join(&mut saved, "alice", Some("West"));
    run_to_tick(&mut saved, 5);
    let cancel = PlayerCommand::CancelRoute { entrance: s("W1") };
    assert!(command(&mut saved, "alice", cancel.clone()).is_empty());
    assert!(saved.save_now().is_empty());
    drop(saved);

    let mut resumed = Game::resume(&path).unwrap();
    rejoin(&mut resumed);
    let out = resumed.advance(0.1);
    assert_eq!(notices(&out, "alice"), vec![Notice::Rejected { cmd: cancel, reason: Rejection::RouteNotSet }]);
    assert_eq!(resumed.stats().sim_rejections, 1);
}

/// A command whose append failed is in the snapshot's queue but not in the
/// log; resume must still replay the command logged after the snapshot.
#[test]
fn a_failed_append_does_not_make_resume_skip_a_later_command() {
    let path = temp_save("failed-append");
    let w1a = set_route("W1", ExitName::Signal(s("A")));
    let ae = set_route("A", ExitName::Node(s("E")));
    let mut reference = game();
    join(&mut reference, "alice", Some("West"));
    run_to_tick(&mut reference, 5);
    command(&mut reference, "alice", w1a.clone());
    command(&mut reference, "alice", ae.clone());
    run_to_tick(&mut reference, 2000);

    let mut saved = Game::create(&path, &twobox_json(), meta()).unwrap();
    join(&mut saved, "alice", Some("West"));
    run_to_tick(&mut saved, 5);
    let fail = "CREATE TRIGGER fail BEFORE INSERT ON commands BEGIN SELECT RAISE(ABORT, 'injected'); END";
    open(&path).execute(fail, []).unwrap();
    let out = command(&mut saved, "alice", w1a.clone());
    assert_eq!(error_codes(&out, "alice"), [codes::SAVE_FAILED]);
    open(&path).execute("DROP TRIGGER fail", []).unwrap();
    assert!(saved.save_now().is_empty());
    assert!(command(&mut saved, "alice", ae.clone()).is_empty());
    drop(saved);

    let mut resumed = Game::resume(&path).unwrap();
    let w = resumed.sim().world().clone();
    assert_eq!(resumed.sim().tick(), 5);
    assert_eq!(resumed.sim().snapshot().queue, vec![resolve(&w, &w1a).unwrap(), resolve(&w, &ae).unwrap()]);
    rejoin(&mut resumed);
    run_to_tick(&mut resumed, 2000);
    assert_eq!(resumed.sim().state_hash(), reference.sim().state_hash());
}

fn count_commands(path: &Path, filter: &str) -> i64 {
    open(path).query_row(&format!("SELECT COUNT(*) FROM commands WHERE {filter}"), [], |r| r.get(0)).unwrap()
}

/// A batch (one robot run) is one transaction: nothing of it is visible,
/// or survives a crash, until it is committed; appends outside a batch are
/// committed one by one as before.
#[test]
fn a_batch_of_commands_is_committed_together() {
    let path = temp_save("batch");
    let db = SaveDb::create(&path, &meta(), &twobox_json()).unwrap();
    let cmd = resolve(&twobox(), &set_route("W1", ExitName::Signal(s("A")))).unwrap();
    db.begin_batch().unwrap();
    db.append_command(10, ROBOT, "West", &cmd).unwrap();
    db.append_command(10, ROBOT, "West", &cmd).unwrap();
    assert_eq!(count_commands(&path, "1"), 0, "not visible before the commit");
    db.commit_batch().unwrap();
    assert_eq!(count_commands(&path, "1"), 2);
    db.append_command(11, "alice", "West", &cmd).unwrap();
    assert_eq!(count_commands(&path, "1"), 3, "a single append commits at once");
    db.begin_batch().unwrap();
    db.append_command(20, ROBOT, "West", &cmd).unwrap();
    drop(db);
    assert_eq!(count_commands(&path, "1"), 3, "a batch cut off by a crash leaves nothing");
    assert_eq!(count_commands(&path, "tick = 20"), 0);
}

/// The robot's commands are committed before the sim steps with them, so
/// a crash in the middle of a later batch resumes exactly at what was
/// committed and carries on as an uninterrupted game.
#[test]
fn a_crash_in_the_middle_of_a_batch_resumes_at_the_last_commit() {
    let path = temp_save("batch-crash");
    let mut reference = game();
    run_to_tick(&mut reference, 3000);

    let mut saved = Game::create(&path, &twobox_json(), meta()).unwrap();
    run_to_tick(&mut saved, 11);
    let robot = saved.stats().robot_commands;
    assert!(robot > 0, "the robot routes 1E01 at tick 10");
    assert_eq!(count_commands(&path, "player = 'robot' AND tick = 10"), robot as i64, "committed before tick 10 ran");
    drop(saved);
    // The next run's batch, cut off by a crash before its commit.
    let db = SaveDb::open(&path).unwrap();
    let cmd = resolve(&twobox(), &set_route("W1", ExitName::Signal(s("A")))).unwrap();
    db.begin_batch().unwrap();
    db.append_command(20, ROBOT, "West", &cmd).unwrap();
    drop(db);

    let mut resumed = Game::resume(&path).unwrap();
    assert_eq!(resumed.sim().tick(), 10, "the last committed tick, with its commands queued");
    assert_eq!(resumed.sim().snapshot().queue.len(), robot);
    unpause(&mut resumed);
    run_to_tick(&mut resumed, 3000);
    assert_eq!(resumed.sim().state_hash(), reference.sim().state_hash());
}

/// Appends that fail in the middle of a robot run are save failures for
/// those commands only: what was appended before them is committed, and a
/// resume replays exactly the committed rows.
#[test]
fn a_failed_append_in_a_batch_keeps_the_rest_of_it() {
    let path = temp_save("batch-fail");
    let mut saved = Game::create(&path, &twobox_json(), meta()).unwrap();
    run_to_tick(&mut saved, 9);
    let fail = "CREATE TRIGGER fail BEFORE INSERT ON commands \
                WHEN NEW.tick = 10 AND EXISTS (SELECT 1 FROM commands WHERE tick = 10) \
                BEGIN SELECT RAISE(ABORT, 'injected'); END";
    open(&path).execute(fail, []).unwrap();
    run_to_tick(&mut saved, 11);
    let robot = saved.stats().robot_commands;
    assert!(robot >= 2, "the robot sends several commands at tick 10: {robot}");
    let errors = saved.take_save_errors();
    assert_eq!(errors.len(), robot - 1, "all but the first: {errors:?}");
    assert!(errors.iter().all(|e| e.contains("injected")), "{errors:?}");
    assert_eq!(count_commands(&path, "tick = 10"), 1);
    assert_eq!(saved.sim().log().len(), robot, "every command still ran");
    open(&path).execute("DROP TRIGGER fail", []).unwrap();
    let committed: Vec<Command> = {
        let c = open(&path);
        let mut st = c.prepare("SELECT command FROM commands WHERE tick = 10 ORDER BY seq").unwrap();
        st.query_map([], |r| r.get::<_, String>(0)).unwrap().map(|j| serde_json::from_str(&j.unwrap()).unwrap()).collect()
    };
    drop(saved);

    let resumed = Game::resume(&path).unwrap();
    assert_eq!(resumed.sim().tick(), 10);
    assert_eq!(resumed.sim().snapshot().queue, committed);
}

#[test]
fn old_schema_saves_are_rejected() {
    let path = temp_save("old-schema");
    drop(Game::create(&path, &twobox_json(), meta()).unwrap());
    let c = open(&path);
    c.execute("UPDATE meta SET value = '1' WHERE key = 'schema'", []).unwrap();
    c.execute("ALTER TABLE snapshots DROP COLUMN last_seq", []).unwrap();
    drop(c);
    match resume_err(&path) {
        GameError::Save(SaveError::Bad(m)) => assert_eq!(m, "unsupported save schema 1"),
        e => panic!("{e:?}"),
    }
}
