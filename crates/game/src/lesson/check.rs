//! Loading a lesson directory and checking every name and step against its
//! world (tutorial spec §2): what the front does at startup before listing a
//! lesson, and the game process again before running one.

use std::collections::BTreeSet;
use std::path::Path;

use protocol::{ExitName, Highlight};
use signalbox_core::ids::{AreaId, SignalId};
use signalbox_core::network::NodeKind;
use signalbox_core::routes::Exit;
use signalbox_core::time::{fmt_hms, parse_hms};
use signalbox_core::timetable::EntryStart;
use signalbox_core::world::World;
use signalbox_core::world::file::WorldFile;

use crate::areas::AreaMap;
use crate::clock::SPEEDS;
use crate::lesson::file::{Action, Condition, LESSON_SCHEMA, LessonFile, Step};
use crate::names::{resolve, valid_headcode};

/// Steps in one lesson, at most.
pub const MAX_STEPS: usize = 60;
/// Characters a step may say, at most.
pub const MAX_SAY: usize = 1200;
/// Screen controls a step may highlight (and `auto:<signal>`).
pub const UI_CONTROLS: [&str; 4] = ["settings", "simplifier", "trains", "clock"];
/// Side panel tabs a step may wait for.
pub const TABS: [&str; 2] = ["trains", "simplifier"];

/// A lesson read and checked, ready to run.
#[derive(Clone, Debug)]
pub struct Lesson {
    /// The directory's name.
    pub id: String,
    pub file: LessonFile,
    /// The world, its start time set to the lesson's.
    pub world: World,
}

/// A lesson id: 1–40 of `a-z`, `0-9`, `-` (it names a directory).
pub fn valid_lesson_id(s: &str) -> bool {
    (1..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Read `dir/lesson.json` and `dir/world.json` and check them. The error
/// is one line, saying what is wrong.
pub fn load_lesson(dir: &Path) -> Result<Lesson, String> {
    let id = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if !valid_lesson_id(id) {
        return Err(format!("`{}` is not a lesson id (a-z, 0-9 and -)", dir.display()));
    }
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).map_err(|e| format!("{name}: {e}"));
    parse_lesson(id, &read("lesson.json")?, &read("world.json")?)
}

/// `load_lesson` from the two files' text.
pub fn parse_lesson(id: &str, lesson_json: &str, world_json: &str) -> Result<Lesson, String> {
    let file: LessonFile = serde_json::from_str(lesson_json).map_err(|e| format!("lesson.json: {e}"))?;
    if file.schema != LESSON_SCHEMA {
        return Err(format!("lesson.json: schema {} (expected {LESSON_SCHEMA})", file.schema));
    }
    let start = parse_hms(&file.start).filter(|&t| t < 24 * 3600).ok_or_else(|| format!("lesson.json: bad start `{}`", file.start))?;
    let mut wf: WorldFile = serde_json::from_str(world_json).map_err(|e| format!("world.json: {e}"))?;
    wf.options.start_time = fmt_hms(f64::from(start));
    let world = World::from_file(wf).map_err(|e| format!("world.json: {e}"))?;
    check(&file, &world)?;
    Ok(Lesson { id: id.to_string(), file, world })
}

/// The index (into the loaded world's time-sorted `entries`, as
/// `Sim::offer_entry` takes it) of the on-demand entry that `spawn {headcode, entry}` offers.
pub fn spawn_entry(w: &World, headcode: &str, entry: &str) -> Option<usize> {
    w.entries.iter().position(|e| {
        e.on_demand
            && w.services[e.service.idx()].headcode == headcode
            && matches!(e.start, EntryStart::Boundary(n) if w.net.nodes[n.idx()].name == entry)
    })
}

struct Checker<'a> {
    w: &'a World,
    map: AreaMap,
    area: AreaId,
    /// Headcodes a train can carry by now: timed entries, and spawns so far.
    trains: BTreeSet<String>,
    /// Entries (indices into `w.entries`) already spawned: offering one twice
    /// would put a second train on the line.
    spawned: BTreeSet<usize>,
}

impl Checker<'_> {
    fn signal(&self, name: &str) -> Result<SignalId, String> {
        self.w.net.signal(name).ok_or_else(|| format!("no signal `{name}`"))
    }

    /// A signal the player works: in the lesson's area, starting a route.
    fn players_signal(&self, name: &str) -> Result<SignalId, String> {
        let s = self.signal(name)?;
        if self.map.signal[s.idx()] != self.area || self.w.routes_from[s.idx()].is_empty() {
            return Err(format!("`{name}` is not a signal the player works"));
        }
        Ok(s)
    }

    fn route(&self, entrance: &str, exit: &ExitName) -> Result<(), String> {
        let s = self.signal(entrance)?;
        let to = match exit {
            ExitName::Signal(n) => Exit::Signal(self.signal(n)?),
            ExitName::Node(n) => Exit::Node(self.w.net.node(n).ok_or_else(|| format!("no node `{n}`"))?),
        };
        self.w.find_route(s, to).map(|_| ()).ok_or_else(|| format!("no route from `{entrance}` to {exit:?}"))
    }

    fn service(&self, headcode: &str) -> Result<(), String> {
        self.w.service(headcode).map(|_| ()).ok_or_else(|| format!("no service `{headcode}`"))
    }

    fn train(&self, headcode: &str) -> Result<(), String> {
        self.service(headcode)?;
        if !self.trains.contains(headcode) {
            return Err(format!("no train `{headcode}` runs by now (spawn it in this or an earlier step)"));
        }
        Ok(())
    }

    fn highlight(&self, h: &Highlight) -> Result<(), String> {
        let net = &self.w.net;
        match h {
            Highlight::Signal(s) => self.signal(s).map(|_| ()),
            Highlight::Exit(ExitName::Signal(s)) => self.signal(s).map(|_| ()),
            Highlight::Exit(ExitName::Node(n)) => {
                let id = net.node(n).ok_or_else(|| format!("no node `{n}`"))?;
                if !self.w.routes.iter().any(|r| r.exit == Exit::Node(id)) {
                    return Err(format!("no route ends at `{n}`"));
                }
                Ok(())
            }
            Highlight::Points(p) => match net.node(p).map(|n| &net.nodes[n.idx()].kind) {
                Some(NodeKind::Points { .. }) => Ok(()),
                _ => Err(format!("no points `{p}`")),
            },
            Highlight::Berth(b) => net.berth(b).map(|_| ()).ok_or_else(|| format!("no berth `{b}`")),
            Highlight::Section(s) => net.section(s).map(|_| ()).ok_or_else(|| format!("no section `{s}`")),
            Highlight::Platform { place, platform } => {
                if net.platforms.iter().any(|p| p.place == *place && p.platform == *platform) {
                    Ok(())
                } else {
                    Err(format!("no platform {platform} at `{place}`"))
                }
            }
            Highlight::Ui(u) => match u.strip_prefix("auto:") {
                Some(s) => {
                    let id = self.players_signal(s)?;
                    if self.w.routes_from[id.idx()].iter().all(|r| self.w.routes[r.idx()].automatic) {
                        return Err(format!("`{s}` has no ○A button"));
                    }
                    Ok(())
                }
                None if UI_CONTROLS.contains(&u.as_str()) => Ok(()),
                None => Err(format!("no screen control `{u}`")),
            },
        }
    }

    fn condition(&self, c: &Condition, step: &Step) -> Result<(), String> {
        let net = &self.w.net;
        match c {
            Condition::Continue {} | Condition::Clock { .. } => Ok(()),
            Condition::Rejected {} => {
                if step.solution.is_empty() {
                    return Err("`rejected` needs a `solution` for the play-through".into());
                }
                Ok(())
            }
            Condition::Selected { signal } => self.players_signal(signal).map(|_| ()),
            Condition::RouteSet { entrance, exit } => {
                self.route(entrance, exit)?;
                let demonstrated = step.actions.iter().any(|a| matches!(a, Action::SetRoute { entrance: e, exit: x } if e == entrance && x == exit));
                if demonstrated { Ok(()) } else { self.players_signal(entrance).map(|_| ()) }
            }
            Condition::RouteCancelled { entrance } => {
                let demonstrated = step.actions.iter().any(|a| matches!(a, Action::CancelRoute { entrance: e } if e == entrance));
                if demonstrated { self.signal(entrance).map(|_| ()) } else { self.players_signal(entrance).map(|_| ()) }
            }
            Condition::Points { name, .. } => {
                let id = net.node(name).ok_or_else(|| format!("no points `{name}`"))?;
                if self.map.points[id.idx()] != Some(self.area) {
                    return Err(format!("`{name}` are not points the player works"));
                }
                Ok(())
            }
            Condition::AutoWorking { signal, .. } => {
                let s = self.players_signal(signal)?;
                if self.w.routes_from[s.idx()].iter().all(|r| self.w.routes[r.idx()].automatic) {
                    return Err(format!("`{signal}` has only automatic routes"));
                }
                Ok(())
            }
            Condition::TrainAt { headcode, place, platform } => {
                self.train(headcode)?;
                let svc = &self.w.services[self.w.service(headcode).expect("checked").idx()];
                let stops = svc.calls.iter().any(|c| c.stop && c.place == *place && c.platform.as_ref().is_none_or(|p| p == platform));
                if !stops || !net.platforms.iter().any(|p| p.place == *place && p.platform == *platform) {
                    return Err(format!("`{headcode}` never stops at {place} platform {platform}"));
                }
                Ok(())
            }
            Condition::TrainPassed { headcode, signal } => {
                self.train(headcode)?;
                self.signal(signal).map(|_| ())
            }
            Condition::TrainLeftArea { headcode } => self.train(headcode),
            Condition::Berth { name, headcode } => {
                net.berth(name).ok_or_else(|| format!("no berth `{name}`"))?;
                if !valid_headcode(headcode) {
                    return Err(format!("`{headcode}` is not a headcode"));
                }
                Ok(())
            }
            Condition::Tab(t) => {
                if TABS.contains(&t.as_str()) { Ok(()) } else { Err(format!("no tab `{t}`")) }
            }
            Condition::All(v) => {
                if v.is_empty() {
                    return Err("`all` of nothing".into());
                }
                v.iter().try_for_each(|c| self.condition(c, step))
            }
        }
    }

    fn action(&mut self, a: &Action) -> Result<(), String> {
        let net = &self.w.net;
        match a {
            Action::Spawn { headcode, entry } => {
                self.service(headcode)?;
                let i = spawn_entry(self.w, headcode, entry).ok_or_else(|| format!("no on-demand entry of `{headcode}` at `{entry}`"))?;
                if !self.spawned.insert(i) {
                    return Err(format!("`{headcode}` is spawned at `{entry}` more than once"));
                }
                self.trains.insert(headcode.clone());
                Ok(())
            }
            Action::Pause {} | Action::Run {} => Ok(()),
            Action::Speed { x } => {
                if SPEEDS.contains(x) { Ok(()) } else { Err(format!("speed {x} is not 1, 2, 4 or 8")) }
            }
            Action::SetRoute { entrance, exit } => self.route(entrance, exit),
            Action::CancelRoute { entrance } => self.signal(entrance).map(|_| ()),
            Action::Interpose { berth, headcode } => {
                net.berth(berth).ok_or_else(|| format!("no berth `{berth}`"))?;
                if !valid_headcode(headcode) {
                    return Err(format!("`{headcode}` is not a headcode"));
                }
                Ok(())
            }
        }
    }

    fn step(&mut self, step: &Step) -> Result<(), String> {
        if step.say.trim().is_empty() || step.say.chars().count() > MAX_SAY {
            return Err(format!("`say` must be 1 to {MAX_SAY} characters"));
        }
        step.highlight.iter().try_for_each(|h| self.highlight(h))?;
        step.actions.iter().try_for_each(|a| self.action(a))?;
        self.condition(&step.wait_for, step)?;
        for m in &step.solution {
            let cmd = resolve(self.w, &m.command()).ok_or_else(|| format!("solution {m:?} names something unknown"))?;
            if self.map.subject(&cmd) != Some(self.area) {
                return Err(format!("solution {m:?} is not the player's to do"));
            }
        }
        Ok(())
    }
}

/// Every name in `l` resolves in `w`, every step is well formed and can be
/// completed by the player (or by the lesson itself).
pub fn check(l: &LessonFile, w: &World) -> Result<(), String> {
    if l.title.trim().is_empty() {
        return Err("lesson.json: the title is empty".into());
    }
    if l.steps.is_empty() || l.steps.len() > MAX_STEPS {
        return Err(format!("lesson.json: a lesson has 1 to {MAX_STEPS} steps"));
    }
    let area = w.net.area(&l.area).ok_or_else(|| format!("lesson.json: no area `{}`", l.area))?;
    let trains = w.entries.iter().filter(|e| !e.on_demand).map(|e| w.services[e.service.idx()].headcode.clone()).collect();
    let mut c = Checker { w, map: AreaMap::new(w), area, trains, spawned: BTreeSet::new() };
    for (i, step) in l.steps.iter().enumerate() {
        c.step(step).map_err(|e| format!("lesson.json: step {}: {e}", i + 1))?;
    }
    Ok(())
}
