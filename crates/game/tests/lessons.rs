//! Every shipped lesson (`lessons/` at the repo root) loads, and its world
//! draws like a converted layout (tutorial spec §2). The play-through is
//! added in Task 5.

use std::path::PathBuf;

use game::areas::{AreaMap, Visibility};
use game::display::prefixes;
use game::geometry::WorldGeometry;
use game::lesson::load_lesson;

fn lessons_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../lessons"))
}

/// The shipped lesson directories, in name order.
fn shipped() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        std::fs::read_dir(lessons_dir()).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs
}

/// Lesson worlds are hand-made but draw like converted layouts: every
/// track segment longer than a points leg has a line, every signal is drawn
/// facing its way, and the box prefix and workstation letters are the
/// world's own (not the defaults).
#[test]
fn every_lesson_world_draws_like_a_converted_layout() {
    let want = [("S", vec!["A"]), ("H", vec!["A"]), ("H", vec!["A"]), ("K", vec!["A", "B"])];
    for (d, (prefix, letters)) in shipped().iter().zip(want) {
        let l = load_lesson(d).unwrap();
        let w = &l.world;
        let geo = WorldGeometry::from_world(w).unwrap_or_else(|| panic!("{}: no drawing", d.display()));
        let all = geo.visible(w, &Visibility::spectator(w, &AreaMap::new(w)));
        for s in w.net.segments.iter().filter(|s| s.length_m > 30.0) {
            assert!(all.lines.iter().any(|g| g.segment == s.name), "{}: segment {} is not drawn", d.display(), s.name);
        }
        assert_eq!(all.signals.len(), w.net.signals.len(), "{}", d.display());
        assert!(all.signals.iter().all(|s| s.facing.is_some_and(|f| f[0] > 0.0)), "{}: every lesson runs left to right", d.display());
        assert_eq!(all.points.len(), w.net.nodes.iter().filter(|n| n.name.starts_with('P')).count(), "{}", d.display());
        assert!(all.points.iter().all(|p| p.toe.is_some() && p.normal.is_some() && p.reverse.is_some()), "{}: a points leg is not drawn", d.display());
        let (box_prefix, ws) = prefixes(w);
        assert_eq!(box_prefix, prefix, "{}", d.display());
        assert_eq!(ws.values().map(String::as_str).collect::<Vec<_>>(), letters, "{}", d.display());
        assert!(w.entries.iter().all(|e| e.on_demand), "{}: lesson trains come by `spawn`", d.display());
    }
}

#[test]
fn four_lessons_ship_and_every_one_loads() {
    let dirs = shipped();
    let ids: Vec<String> = dirs.iter().map(|d| d.file_name().unwrap().to_str().unwrap().to_string()).collect();
    assert_eq!(ids, ["01-reading-the-panel", "02-setting-routes", "03-running-trains", "04-junctions-and-handovers"]);
    let want = [
        ("Reading the panel", 10),
        ("Setting & cancelling routes", 10),
        ("Running trains", 10),
        ("Junctions, auto-working & handovers", 14),
    ];
    for (d, (title, steps)) in dirs.iter().zip(want) {
        let l = load_lesson(d).unwrap_or_else(|e| panic!("{}: {e}", d.display()));
        assert_eq!((l.file.title.as_str(), l.file.steps.len()), (title, steps));
    }
}
