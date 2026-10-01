//! Every shipped lesson (`lessons/` at the repo root) loads, its world draws
//! like a converted layout, and a scripted player completes it within a
//! sim-time limit (tutorial spec §2, §6).

use std::path::PathBuf;

use game::areas::{AreaMap, Visibility};
use game::display::prefixes;
use game::geometry::WorldGeometry;
use game::lesson::{self, Action, Condition, Runner, Step, load_lesson};
use game::names::to_player_command;
use game::{Game, Out};
use protocol::{ClientMsg, PlayerCommand, Proposal};
use signalbox_core::robot;

const PLAYER: &str = "pat";
/// Sim seconds one step may take in the play-through.
const STEP_LIMIT_S: f64 = 1800.0;
/// Sim seconds a whole lesson may take.
const LESSON_LIMIT_S: f64 = 7200.0;

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

/// The CI player: what the screen shows, as it would tell the game.
struct Player {
    g: Game,
    r: Runner,
    tab: Option<String>,
    selected: Option<String>,
}

impl Player {
    fn send(&mut self, msg: ClientMsg) -> Vec<Out> {
        self.r.handle(&mut self.g, PLAYER, msg)
    }

    fn command(&mut self, cmd: PlayerCommand) {
        self.send(ClientMsg::Command { cmd });
    }

    fn screen(&mut self) {
        let (tab, selected) = (self.tab.clone(), self.selected.clone());
        self.send(ClientMsg::LessonUi { tab, selected });
    }

    /// What a player does at the start of `step`: its `solution`, or what
    /// each condition asks for (not what the step's own actions do, nor
    /// what trains do by themselves).
    fn act(&mut self, step: &Step) {
        if !step.solution.is_empty() {
            for m in &step.solution {
                self.command(m.command());
            }
            return;
        }
        let shown = |a: &Action| step.actions.contains(a);
        for c in step.wait_for.leaves() {
            match c.clone() {
                Condition::Continue {} => {
                    self.send(ClientMsg::LessonNext);
                }
                Condition::Selected { signal } => {
                    self.selected = Some(signal);
                    self.screen();
                }
                Condition::Tab(t) => {
                    self.tab = Some(t);
                    self.screen();
                }
                Condition::RouteSet { entrance, exit } => {
                    if !shown(&Action::SetRoute { entrance: entrance.clone(), exit: exit.clone() }) {
                        self.selected = None;
                        self.command(PlayerCommand::SetRoute { entrance, exit });
                    }
                }
                Condition::RouteCancelled { entrance } => {
                    if !shown(&Action::CancelRoute { entrance: entrance.clone() }) {
                        self.command(PlayerCommand::CancelRoute { entrance });
                    }
                }
                Condition::Points { name, position } => self.command(PlayerCommand::SwingPoints { points: name, to: position }),
                Condition::AutoWorking { signal, on } => self.command(PlayerCommand::SetAutoWorking { entrance: signal, on }),
                Condition::Clock { paused } => {
                    let proposal = if paused { Proposal::Pause } else { Proposal::Resume };
                    self.send(ClientMsg::Vote { proposal });
                }
                // Trains and berths move by themselves; `rejected` and `all`
                // never get here (a solution, the leaves).
                _ => {}
            }
        }
    }

    /// Route the trains in the lesson's area as the robot would.
    fn drive(&mut self) {
        let w = self.g.sim().world();
        let area = w.net.area(self.r.area()).unwrap();
        let map = AreaMap::new(w);
        let cmds: Vec<PlayerCommand> =
            robot::commands(self.g.sim()).iter().filter(|c| map.subject(c) == Some(area)).map(|c| to_player_command(w, c)).collect();
        for cmd in cmds {
            self.command(cmd);
        }
    }
}

/// Play the lesson in `dir` to the end; the sim seconds it took.
fn play(dir: &PathBuf) -> f64 {
    let (g, r) = lesson::start(load_lesson(dir).unwrap());
    let mut p = Player { g, r, tab: Some("trains".into()), selected: None };
    p.r.connect(&mut p.g, PLAYER);
    let t0 = p.g.sim().now_s();
    while !p.r.done() {
        let i = p.r.step();
        let step = p.r.lesson().steps[i].clone();
        p.act(&step);
        let trains = step.wait_for.leaves().iter().any(|c| {
            matches!(c, Condition::TrainAt { .. } | Condition::TrainPassed { .. } | Condition::TrainLeftArea { .. })
        });
        let since = p.g.sim().now_s();
        let mut ticks = 0u32;
        while p.r.step() == i {
            // The task is done: the player looks, then presses Next (polish spec H5).
            if p.r.view().completed {
                p.send(ClientMsg::LessonNext);
                continue;
            }
            if trains {
                p.drive();
            }
            // As a player would: try again a little later (a lesson's own
            // command may still hold the points, say).
            ticks += 1;
            if ticks % 15 == 0 {
                p.act(&step);
            }
            p.r.advance(&mut p.g, 1.0);
            p.g.flush();
            let waited = p.g.sim().now_s() - since;
            assert!(waited <= STEP_LIMIT_S, "{}: step {} never completed: {}", dir.display(), i + 1, step.say);
            assert!(!p.g.clock().paused || p.r.step() != i, "{}: step {} waits on a paused clock", dir.display(), i + 1);
        }
        assert!(p.g.stats().spads == 0 && p.g.stats().collisions == 0, "{}: trouble at step {}", dir.display(), i + 1);
    }
    let v = p.r.view();
    assert!(v.done && v.index == v.count, "{}: the client is told the lesson is done", dir.display());
    let took = p.g.sim().now_s() - t0;
    assert!(took <= LESSON_LIMIT_S, "{}: took {took} s", dir.display());
    took
}

#[test]
fn every_lesson_is_played_through_to_the_end() {
    for d in shipped() {
        let took = play(&d);
        eprintln!("{}: done in {took:.0} sim s", d.display());
    }
}
