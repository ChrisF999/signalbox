//! What seeding costs (timetables spec P8) on the shipped layouts, and the
//! real Waterloo & City WTT started late. Slow in debug builds:
//! `scripts/cargo test --release -p signalbox-game --test seed_timing -- --ignored --nocapture --test-threads 1`
//! The real WTT needs the git-ignored `external/wtt/wtt.bbox.html`
//! (external/wtt/README.md) and is skipped without it.

mod common;

use std::path::PathBuf;
use std::time::Instant;

use game::seed::{self, Progress, Seeding};
use game::{Game, GameMeta};
use signalbox_core::events::Event;
use signalbox_core::time::{fmt_hms, parse_hms};

const WTT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../external/wtt/wtt.bbox.html");

fn convert(name: &str, wtt: Option<&str>) -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    let ts2 = std::fs::read_to_string(format!("{dir}/../ts2-import/tests/data/{name}.json")).unwrap();
    let areas = std::fs::read_to_string(format!("{dir}/../../layouts/{name}.areas.json")).unwrap();
    let mut w = ts2_import::convert(&ts2).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&areas).unwrap()).unwrap();
    if let Some(text) = wtt {
        use ts2_import::wtt;
        let day = wtt::on_day(&wtt::parse(text).unwrap(), wtt::DAY).unwrap();
        wtt::check(&day, &wtt::Checks::waterloo_city()).unwrap();
        wtt::apply(&mut w, &day).unwrap();
    }
    serde_json::to_string(&w).unwrap()
}

fn temp_save(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("signalbox-seed-timing-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.sqlite"));
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }
    seed::remove_partial(&path);
    path
}

/// Seed `json` to `to`, printing how long it took; the game.
fn timed(name: &str, json: &str, to: &str) -> Game {
    let path = temp_save(name);
    let t0 = Instant::now();
    let mut s = Seeding::create(&path, json, GameMeta { layout: name.into(), seed: 11 }, parse_hms(to).unwrap()).unwrap();
    let created = t0.elapsed();
    let r = s.run(|_| true).unwrap();
    let ran = t0.elapsed();
    assert_eq!(r, Progress::Reached);
    let ticks = s.until_tick();
    let g = s.finish().unwrap();
    let all = t0.elapsed();
    let rows: i64 = rusqlite::Connection::open(&path).unwrap().query_row("SELECT COUNT(*) FROM commands", [], |r| r.get(0)).unwrap();
    let size = std::fs::metadata(&path).unwrap().len();
    println!(
        "{name}: {} → {to}: {ticks} ticks, {rows} seed commands, {:.2} s in all (create {:.2} s, run {:.2} s, finish {:.2} s), save {:.1} MB, {} trains",
        fmt_hms(seed::world_start(json).unwrap().into()),
        all.as_secs_f64(),
        created.as_secs_f64(),
        (ran - created).as_secs_f64(),
        (all - ran).as_secs_f64(),
        size as f64 / 1e6,
        g.sim().trains().len()
    );
    g
}

#[test]
#[ignore = "timing; run in release"]
fn seeding_cost_on_the_shipped_layouts() {
    let lst = convert("liverpool-st", None);
    timed("liverpool-st", &lst, "23:00");
    let gretz = convert("gretz-armainvilliers", None);
    timed("gretz-armainvilliers", &gretz, "23:00");
    let drain = convert("drain", None);
    timed("drain-ts2", &drain, "23:00");
    match std::fs::read_to_string(WTT) {
        Ok(text) => {
            let drain = convert("drain", Some(&text));
            timed("drain-wtt", &drain, "14:00");
        }
        Err(_) => println!("drain-wtt: skipped (no external/wtt/wtt.bbox.html)"),
    }
}

/// Lateness (s) of every arrival and departure in `mins` minutes of play.
fn lateness(g: &mut Game, mins: u64) -> Vec<i64> {
    g.set_paused(false);
    let mut late = Vec::new();
    for _ in 0..mins * 600 {
        g.advance_with(0.1, |_, events, _| {
            for e in events {
                if let Event::TrainArrived { late_s, .. } = e {
                    late.push(*late_s);
                }
            }
            vec![]
        });
    }
    late
}

/// The bug this fixes: Drain on the real WTT created for 07:30 released
/// every earlier train at once, 50 to 80 minutes late. Seeded, the first
/// quarter hour runs near time.
#[test]
#[ignore = "needs the owner's WTT in external/wtt/ (wtt.bbox.html); run in release"]
fn the_real_wtt_started_at_0730_opens_near_on_time() {
    let Ok(text) = std::fs::read_to_string(WTT) else {
        println!("skipped: no external/wtt/wtt.bbox.html");
        return;
    };
    let json = convert("drain", Some(&text));
    let mut g = timed("drain-0730", &json, "07:30");
    let late = lateness(&mut g, 15);
    let worst = late.iter().copied().max().unwrap();
    println!("seeded 07:30: {} arrivals in 15 min, worst {worst} s late, mean {:.0} s", late.len(), late.iter().sum::<i64>() as f64 / late.len() as f64);
    // The old way, for comparison: the world's start moved to 07:30.
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v["options"]["start_time"] = "07:30:00".into();
    let path = temp_save("drain-0730-old");
    let mut old = Game::create(&path, &v.to_string(), GameMeta { layout: "drain".into(), seed: 11 }).unwrap();
    let old_late = lateness(&mut old, 15);
    let old_worst = old_late.iter().copied().max().unwrap_or(0);
    println!("old 07:30 (start moved): {} arrivals in 15 min, worst {old_worst} s late", old_late.len());
    assert!(late.len() >= 10, "trains are running: {late:?}");
    assert!(worst < 5 * 60, "near on time: {late:?}");
}
