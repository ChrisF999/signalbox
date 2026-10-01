//! Seeding a game created later than its world's start (timetables spec
//! §3.4, P7/P8): the robot runs every area from the world's start to the
//! chosen time, logged as `seed`, and the game opens there.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use game::seed::{self, Progress, SEED, Seeding};
use game::{Game, GameMeta, ROBOT};
use rusqlite::Connection;
use signalbox_core::events::Command;
use signalbox_core::robot::{self, ROBOT_EVERY_TICKS};
use signalbox_core::sim::Sim;
use signalbox_core::time::parse_hms;
use signalbox_core::world::World;

fn temp_save(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("signalbox-seed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.sqlite"));
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }
    seed::remove_partial(&path);
    path
}

fn drain_meta() -> GameMeta {
    GameMeta { layout: "drain".into(), seed: 7 }
}

fn hms(s: &str) -> u32 {
    parse_hms(s).unwrap()
}

/// An uninterrupted robot-run game of `json` for `ticks` ticks, as
/// `robot::soak` runs one.
fn robot_run(json: &str, seed: u64, ticks: u64) -> Sim {
    let mut sim = Sim::new(World::from_json(json).unwrap(), seed);
    while sim.tick() < ticks {
        if sim.tick() % ROBOT_EVERY_TICKS == 0 {
            for c in robot::commands(&sim) {
                sim.submit(c);
            }
        }
        sim.step();
    }
    sim
}

/// The command log: (tick, player, command), in order.
fn logged(path: &Path) -> Vec<(u64, String, Command)> {
    let c = Connection::open(path).unwrap();
    let mut st = c.prepare("SELECT tick, player, command FROM commands ORDER BY seq").unwrap();
    st.query_map([], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
        .unwrap()
        .map(|r| {
            let (t, p, c) = r.unwrap();
            (t, p, serde_json::from_str(&c).unwrap())
        })
        .collect()
}

fn meta_row(path: &Path, key: &str) -> Option<String> {
    let c = Connection::open(path).unwrap();
    c.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).ok()
}

fn snapshot_ticks(path: &Path) -> Vec<u64> {
    let c = Connection::open(path).unwrap();
    let mut st = c.prepare("SELECT tick FROM snapshots ORDER BY tick").unwrap();
    st.query_map([], |r| r.get::<_, i64>(0)).unwrap().map(|t| t.unwrap() as u64).collect()
}

fn seeded(name: &str, json: &str, start: &str) -> (PathBuf, Game) {
    let path = temp_save(name);
    let mut s = Seeding::create(&path, json, drain_meta(), hms(start)).unwrap();
    assert!(!path.exists(), "nothing at the real path while preparing");
    assert!(seed::temp_path(&path).exists());
    assert_eq!(s.run(|_| true).unwrap(), Progress::Reached);
    let g = s.finish().unwrap();
    (path, g)
}

#[test]
fn only_starts_later_than_the_worlds_need_seeding() {
    let json = twobox_json();
    assert_eq!(seed::world_start(&json), Ok(hms("07:00")));
    assert!(!seed::needs_seeding(&json, hms("06:00")).unwrap());
    assert!(!seed::needs_seeding(&json, hms("07:00")).unwrap());
    assert!(seed::needs_seeding(&json, hms("07:00:01")).unwrap());
    let path = temp_save("not-later");
    assert!(Seeding::create(&path, &json, meta(), hms("07:00")).is_err());
    assert!(!path.exists() && !seed::temp_path(&path).exists());
}

/// The bug this fixes: a 06:20 start released every earlier train at once.
/// Seeded, the game at 06:20 is exactly an uninterrupted robot run's.
#[test]
fn a_late_start_has_the_trains_where_an_uninterrupted_run_has_them() {
    let json = drain_wtt_json();
    let from = seed::world_start(&json).unwrap();
    assert!(from < hms("06:00"), "the synthetic WTT starts early: {from}");
    let (path, g) = seeded("drain-late", &json, "06:20");
    let until = u64::from(hms("06:20") - from) * 10;
    assert_eq!(g.sim().tick(), until);
    assert_eq!(g.sim().now_s(), f64::from(hms("06:20")));
    assert_eq!(g.sim().world().options.start_s, f64::from(from), "the world keeps its own start");
    let reference = robot_run(&json, 7, until);
    assert_eq!(g.sim().state_hash(), reference.state_hash());
    assert!(!g.sim().trains().is_empty(), "trains are running at 06:20");
    assert!(!g.clock().paused, "a new game runs");

    // The save: the world's start, `seed_to`, a snapshot at the start time,
    // and only `seed` commands, all before it.
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(meta_row(&path, "start").as_deref(), v["options"]["start_time"].as_str());
    assert_eq!(meta_row(&path, "seed_to").as_deref(), Some("06:20:00"));
    assert_eq!(snapshot_ticks(&path), [0, until]);
    let log = logged(&path);
    assert!(!log.is_empty());
    assert!(log.iter().all(|(t, p, _)| p == SEED && *t < until && t % ROBOT_EVERY_TICKS == 0), "{:?}", &log[..3]);
    assert!(!seed::temp_path(&path).exists(), "the half-built file is gone");
    assert_eq!(g.last_snapshot_tick(), Some(until));
}

#[test]
fn a_seeded_save_replays_bit_exactly_from_tick_0() {
    let json = drain_wtt_json();
    let (path, g) = seeded("drain-replay", &json, "06:10");
    let until = g.sim().tick();
    let log: Vec<(u64, Command)> = logged(&path).into_iter().map(|(t, _, c)| (t, c)).collect();
    let replayed = Sim::replay(World::from_json(&json).unwrap(), 7, &log, until);
    assert_eq!(replayed.tick(), until);
    assert_eq!(replayed.state_hash(), g.sim().state_hash());
}

#[test]
fn a_seeded_game_resumes_at_its_start_without_seeding_again_and_plays_on_exactly() {
    let json = drain_wtt_json();
    let (path, mut g) = seeded("drain-resume", &json, "06:15");
    let until = g.sim().tick();
    let rows = logged(&path).len();
    let hash = g.sim().state_hash();
    // The save as it was when the game opened (all in the main file).
    let copy = temp_save("drain-resume-copy");
    std::fs::copy(&path, &copy).unwrap();
    // Played on uninterrupted, it is the robot run (the robot runs at the
    // start tick too: nobody holds an area).
    let reference = robot_run(&json, 7, until + 3000);
    run_to_tick(&mut g, until + 3000);
    assert_eq!(g.sim().state_hash(), reference.state_hash());
    drop(g);
    let path = copy;
    assert_eq!(logged(&path).len(), rows);
    let mut r = Game::resume(&path).unwrap();
    assert_eq!(r.sim().tick(), until, "resumed at the start, not run again");
    assert_eq!(r.sim().state_hash(), hash);
    assert_eq!(logged(&path).len(), rows, "resuming logs nothing");
    r.set_paused(false);
    run_to_tick(&mut r, until + 3000);
    assert_eq!(r.sim().state_hash(), reference.state_hash());
    let after: Vec<(u64, String, Command)> = logged(&path).into_iter().filter(|(t, _, _)| *t >= until).collect();
    assert!(after.iter().all(|(_, p, _)| p == ROBOT), "play after the start is the robot's, not the seed's");
    assert!(!after.is_empty(), "the robot played on");
}

#[test]
fn a_seeding_told_to_stop_leaves_nothing_behind() {
    let json = drain_wtt_json();
    let path = temp_save("drain-stop");
    let mut s = Seeding::create(&path, &json, drain_meta(), hms("08:00")).unwrap();
    let mut asked = 0;
    let r = s.run(|g| {
        asked += 1;
        g.sim().tick() < 500
    });
    assert_eq!(r.unwrap(), Progress::Stopped);
    assert_eq!(s.game().sim().tick(), 500);
    assert_eq!(asked, 501);
    s.abandon();
    assert!(!path.exists() && !seed::temp_path(&path).exists());
}

#[test]
fn an_unfinished_seeding_cannot_finish_and_cleans_up() {
    let path = temp_save("twobox-unfinished");
    let mut s = Seeding::create(&path, &twobox_json(), meta(), hms("07:30")).unwrap();
    s.run(|g| g.sim().tick() < 20).unwrap();
    assert!(s.finish().is_err());
    assert!(!path.exists() && !seed::temp_path(&path).exists());
}

/// A crash while preparing (the process dies) leaves only the temporary
/// file, which `remove_partial` clears; the real path never existed.
#[test]
fn a_crash_while_preparing_leaves_only_the_temporary_file() {
    let path = temp_save("twobox-crash");
    let mut s = Seeding::create(&path, &twobox_json(), meta(), hms("07:30")).unwrap();
    s.run(|g| g.sim().tick() < 100).unwrap();
    std::mem::forget(s);
    assert!(!path.exists());
    assert!(seed::temp_path(&path).exists());
    seed::remove_partial(&path);
    assert!(!seed::temp_path(&path).exists());
    // And a new attempt at the same path starts afresh.
    let (_, g) = (path.clone(), {
        let mut s = Seeding::create(&path, &twobox_json(), meta(), hms("07:01")).unwrap();
        s.run(|_| true).unwrap();
        s.finish().unwrap()
    });
    assert_eq!(g.sim().tick(), 600);
}

#[test]
fn creating_over_an_existing_save_is_refused() {
    let path = temp_save("twobox-exists");
    drop(Game::create(&path, &twobox_json(), meta()).unwrap());
    assert!(Seeding::create(&path, &twobox_json(), meta(), hms("07:30")).is_err());
}

/// Saves from before seeding moved the world's start instead (as an
/// earlier start still does): they have no `seed` rows and no `seed_to`,
/// and resume and play on exactly as before.
#[test]
fn a_save_with_a_moved_start_resumes_exactly_as_before() {
    let mut v: serde_json::Value = serde_json::from_str(&twobox_json()).unwrap();
    v["options"]["start_time"] = "07:05:00".into();
    let json = v.to_string();
    let path = temp_save("old-style");
    let mut g = Game::create(&path, &json, meta()).unwrap();
    run_to_tick(&mut g, 600);
    g.save_now();
    let copy = temp_save("old-style-copy");
    std::fs::copy(&path, &copy).unwrap();
    std::fs::copy(format!("{}-wal", path.display()), format!("{}-wal", copy.display())).unwrap();
    run_to_tick(&mut g, 1800);
    let mut r = Game::resume(&copy).unwrap();
    assert_eq!(r.sim().tick(), 600);
    r.set_paused(false);
    run_to_tick(&mut r, 1800);
    assert_eq!(r.sim().state_hash(), g.sim().state_hash());
    assert_eq!(meta_row(&copy, "seed_to"), None);
    assert_eq!(meta_row(&copy, "start").as_deref(), Some("07:05:00"));
    let log = logged(&copy);
    assert!(!log.is_empty() && log.iter().all(|(_, p, _)| p == ROBOT));
}
