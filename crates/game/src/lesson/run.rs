//! Running a lesson over a `Game` (tutorial spec §3). At each step's start
//! the runner keeps a snapshot, then runs the step's actions; it checks the
//! step's condition after every tick and every message from its player, and
//! moves on when it holds. Restart step puts the snapshot back and runs the
//! actions again. The clock only runs while the lesson's player is here.

use std::collections::{BTreeMap, BTreeSet};

use protocol::{ClientMsg, LessonView, Notice, ServerMsg, codes};
use signalbox_core::events::Event;
use signalbox_core::ids::{AreaId, TrainId};
use signalbox_core::interlocking::RouteState;
use signalbox_core::routes::Exit;

use crate::game::{Game, GameMeta, GameSnapshot, Out};
use crate::lesson::check::{Lesson, spawn_entry};
use crate::lesson::file::{Action, Condition, LessonFile};

/// Every tutorial runs with this seed: lessons play the same every time.
pub const LESSON_SEED: u64 = 1;

pub const SPAD_ALERT: &str = "A train passed a signal at danger: a SPAD (signal passed at danger). \
In a real box that is serious. Press Restart step to try this step again.";
pub const COLLISION_ALERT: &str = "Two trains collided. Press Restart step to try this step again.";

/// What the player did in the current step.
#[derive(Clone, Copy, Debug, Default)]
struct Progress {
    next: bool,
    rejected: bool,
}

/// What has happened so far in the lesson, so that a step whose event came
/// early (the player ran ahead) does not wait for it again. Kept with each
/// step's snapshot: Restart step puts it back too.
#[derive(Clone, Debug, Default)]
struct Seen {
    /// Entrances a route was cancelled from.
    cancelled: BTreeSet<String>,
    /// (headcode, signal) for every signal passed.
    passed: BTreeSet<(String, String)>,
    /// Headcodes seen in the lesson's area.
    in_area: BTreeSet<String>,
}

/// What the player's screen shows, as the client last said.
#[derive(Clone, Debug, Default, PartialEq)]
struct Screen {
    tab: Option<String>,
    selected: Option<String>,
}

pub struct Runner {
    id: String,
    lesson: LessonFile,
    area: AreaId,
    /// Who takes the lesson: the first to connect.
    player: Option<String>,
    /// The step showing; `lesson.steps.len()` once done.
    step: usize,
    /// The game and `seen` as each step so far started, before its actions.
    starts: Vec<(GameSnapshot, Seen)>,
    progress: Progress,
    seen: Seen,
    screen: Screen,
    alert: Option<String>,
    /// SPADs and collisions already known about.
    trouble: (usize, usize),
    /// Headcodes of the trains as last seen (a train may leave in the tick
    /// it passes a signal).
    names: BTreeMap<TrainId, String>,
}

/// A new game for `lesson`, and its runner at step 1 (its actions run).
pub fn start(lesson: Lesson) -> (Game, Runner) {
    let area = lesson.world.net.area(&lesson.file.area).expect("checked at load");
    let mut g = Game::new(lesson.world, GameMeta { layout: lesson.id.clone(), seed: LESSON_SEED });
    let mut r = Runner {
        id: lesson.id,
        lesson: lesson.file,
        area,
        player: None,
        step: 0,
        starts: Vec::new(),
        progress: Progress::default(),
        seen: Seen::default(),
        screen: Screen::default(),
        alert: None,
        trouble: (0, 0),
        names: BTreeMap::new(),
    };
    r.begin(&mut g, 0);
    (g, r)
}

impl Runner {
    /// The step showing (from 0); the number of steps once done.
    pub fn step(&self) -> usize {
        self.step
    }

    pub fn done(&self) -> bool {
        self.step >= self.lesson.steps.len()
    }

    pub fn player(&self) -> Option<&str> {
        self.player.as_deref()
    }

    pub fn lesson(&self) -> &LessonFile {
        &self.lesson
    }

    /// The lesson's area, by name.
    pub fn area(&self) -> &str {
        &self.lesson.area
    }

    pub fn view(&self) -> LessonView {
        let count = self.lesson.steps.len();
        let (say, highlight, needs_next) = match self.lesson.steps.get(self.step) {
            Some(s) => (s.say.clone(), s.highlight.clone(), s.wait_for.needs_next()),
            None => (String::new(), vec![], false),
        };
        LessonView {
            lesson: self.id.clone(),
            title: self.lesson.title.clone(),
            index: self.step as u32,
            count: count as u32,
            say,
            highlight,
            needs_next,
            done: self.done(),
            alert: self.alert.clone(),
        }
    }

    fn message(&self, g: &Game) -> Vec<Out> {
        match &self.player {
            Some(p) if g.connected(p) => vec![(p.clone(), ServerMsg::Lesson(self.view()))],
            _ => vec![],
        }
    }

    fn is_player(&self, who: &str) -> bool {
        self.player.as_deref() == Some(who)
    }

    /// The first to connect takes the lesson and holds its area; anyone
    /// else (the front lets nobody else in) is a spectator.
    pub fn connect(&mut self, g: &mut Game, player: &str) -> Vec<Out> {
        let out = g.connect(player);
        if self.player.is_none() && g.connected(player) {
            self.player = Some(player.to_string());
        }
        if !self.is_player(player) {
            return out;
        }
        // The claim sends the layout and a full view again, now of the
        // lesson's area: only those go out.
        let mut out = g.handle(player, ClientMsg::Claim { area: self.lesson.area.clone() });
        out.extend(self.message(g));
        out
    }

    pub fn handle(&mut self, g: &mut Game, player: &str, msg: ClientMsg) -> Vec<Out> {
        let mine = self.is_player(player) && g.connected(player);
        match msg {
            ClientMsg::LessonNext => {
                if !mine {
                    return vec![];
                }
                self.progress.next = true;
                self.settle(g)
            }
            ClientMsg::LessonRestartStep if mine => self.restart(g, self.step.min(self.lesson.steps.len().saturating_sub(1))),
            ClientMsg::LessonRestart if mine => self.restart(g, 0),
            ClientMsg::LessonUi { tab, selected } if mine => {
                self.screen = Screen { tab, selected };
                self.settle(g)
            }
            ClientMsg::LessonRestartStep | ClientMsg::LessonRestart | ClientMsg::LessonUi { .. } => vec![],
            ClientMsg::Claim { .. } | ClientMsg::Release => vec![(
                player.to_string(),
                ServerMsg::Notice(Notice::Error {
                    code: codes::IN_LESSON.to_string(),
                    message: "in a lesson you keep the lesson's area".to_string(),
                }),
            )],
            ClientMsg::Resync => {
                let mut out = g.handle(player, ClientMsg::Resync);
                if mine {
                    out.extend(self.message(g));
                }
                out
            }
            other => {
                let mut out = g.handle(player, other);
                if mine {
                    out.extend(self.settle(g));
                }
                out
            }
        }
    }

    /// Run the game for `real_dt`, checking the step after every tick. The
    /// clock stands still while the lesson's player is away.
    pub fn advance(&mut self, g: &mut Game, real_dt: f64) -> Vec<Out> {
        if !self.player.as_deref().is_some_and(|p| g.connected(p)) {
            return vec![];
        }
        g.advance_with(real_dt, |g, events, outs| self.after_tick(g, events, outs))
    }

    fn after_tick(&mut self, g: &mut Game, events: &[Event], outs: &[Out]) -> Vec<Out> {
        let w = g.sim().world();
        let net = &w.net;
        let name = |t: &TrainId, names: &BTreeMap<TrainId, String>| {
            g.sim().trains().iter().find(|x| x.id == *t).map(|x| x.headcode.clone()).or_else(|| names.get(t).cloned())
        };
        for e in events {
            match e {
                Event::RouteCancelled { route, .. } => {
                    let entrance = w.routes[route.idx()].entrance;
                    self.seen.cancelled.insert(net.signals[entrance.idx()].name.clone());
                }
                Event::SignalPassed { signal, train } => {
                    if let Some(h) = name(train, &self.names) {
                        self.seen.passed.insert((h, net.signals[signal.idx()].name.clone()));
                    }
                }
                _ => {}
            }
        }
        self.names = g.sim().trains().iter().map(|t| (t.id, t.headcode.clone())).collect();
        self.note_trains(g);
        self.note_refusals(outs);
        let mut out = Vec::new();
        let stats = g.stats();
        let trouble = (stats.spads, stats.collisions);
        if trouble != self.trouble {
            self.alert = Some(if trouble.1 > self.trouble.1 { COLLISION_ALERT } else { SPAD_ALERT }.to_string());
            self.trouble = trouble;
            out.extend(self.message(g));
        }
        out.extend(self.settle(g));
        out
    }

    /// Every train in the lesson's area now.
    fn note_trains(&mut self, g: &Game) {
        let net = &g.sim().world().net;
        for t in g.sim().trains() {
            if t.segments().any(|s| net.sections[net.segments[s.idx()].section.idx()].area == self.area) {
                self.seen.in_area.insert(t.headcode.clone());
            }
        }
    }

    /// A tick's messages: a `rejected` there is the interlocking refusing
    /// (spec §2). A command `Game` refuses at once (another area, an
    /// unknown name) never reaches the sim and does not count.
    fn note_refusals(&mut self, outs: &[Out]) {
        let refused =
            outs.iter().any(|(p, m)| self.is_player(p) && matches!(m, ServerMsg::Notice(Notice::Rejected { .. })));
        self.progress.rejected |= refused;
    }

    /// Start step `i` (or finish, past the last): snapshot, then actions.
    fn begin(&mut self, g: &mut Game, i: usize) {
        self.step = i;
        self.progress = Progress::default();
        self.alert = None;
        let stats = g.stats();
        self.trouble = (stats.spads, stats.collisions);
        self.names = g.sim().trains().iter().map(|t| (t.id, t.headcode.clone())).collect();
        self.note_trains(g);
        let Some(step) = self.lesson.steps.get(i) else { return };
        self.starts.truncate(i);
        self.starts.push((g.snapshot(), self.seen.clone()));
        for a in step.actions.clone() {
            self.act(g, &a);
        }
    }

    fn act(&mut self, g: &mut Game, a: &Action) {
        // Every action was checked against the world at load.
        match a {
            Action::Spawn { headcode, entry } => {
                if let Some(i) = spawn_entry(g.sim().world(), headcode, entry) {
                    let _ = g.offer_entry(i);
                }
            }
            Action::Pause {} => g.set_paused(true),
            Action::Run {} => g.set_paused(false),
            Action::Speed { x } => {
                g.set_speed(*x);
            }
            Action::SetRoute { entrance, exit } => {
                let _ = g.demonstrate(&protocol::PlayerCommand::SetRoute { entrance: entrance.clone(), exit: exit.clone() });
            }
            Action::CancelRoute { entrance } => {
                let _ = g.demonstrate(&protocol::PlayerCommand::CancelRoute { entrance: entrance.clone() });
            }
            Action::Interpose { berth, headcode } => {
                let _ = g.demonstrate(&protocol::PlayerCommand::Interpose { berth: berth.clone(), headcode: headcode.clone() });
            }
        }
    }

    /// Back to the start of step `i` and run it again. Restart lesson also
    /// forgets what the screen showed, until the client says again.
    fn restart(&mut self, g: &mut Game, i: usize) -> Vec<Out> {
        let Some((snap, seen)) = self.starts.get(i).cloned() else { return vec![] };
        if g.restore(&snap).is_err() {
            return vec![];
        }
        self.seen = seen;
        if i == 0 {
            self.screen = Screen::default();
        }
        self.begin(g, i);
        let mut out = self.message(g);
        out.extend(self.settle(g));
        out
    }

    /// Move on while the step's condition holds; the new step if it moved.
    /// While a SPAD or collision alert is up the step holds: only a restart
    /// (which clears the alert) moves the lesson.
    fn settle(&mut self, g: &mut Game) -> Vec<Out> {
        if self.alert.is_some() {
            return vec![];
        }
        let mut moved = false;
        while let Some(step) = self.lesson.steps.get(self.step) {
            if !self.met(g, &step.wait_for) {
                break;
            }
            self.begin(g, self.step + 1);
            moved = true;
        }
        if moved { self.message(g) } else { vec![] }
    }

    fn met(&self, g: &Game, c: &Condition) -> bool {
        let sim = g.sim();
        let w = sim.world();
        let net = &w.net;
        let il = sim.interlocking();
        let train = |h: &str| sim.trains().iter().find(|t| t.headcode == h);
        match c {
            Condition::Continue {} => self.progress.next,
            Condition::Selected { signal } => self.screen.selected.as_deref() == Some(signal),
            Condition::Tab(t) => self.screen.tab.as_deref() == Some(t),
            Condition::RouteSet { entrance, exit } => {
                let (Some(s), Some(to)) = (net.signal(entrance), exit_of(g, exit)) else { return false };
                w.find_route(s, to).is_some_and(|r| {
                    let st = &il.routes[r.idx()];
                    st.state == RouteState::Locked && st.cancel.is_none()
                })
            }
            Condition::RouteCancelled { entrance } => {
                net.signal(entrance).is_some_and(|s| il.active_route_from(w, s).is_none())
                    && self.seen.cancelled.contains(entrance)
            }
            Condition::Points { name, position } => net.node(name).is_some_and(|n| sim.points().detected(n) == Some(*position)),
            Condition::AutoWorking { signal, on } => net.signal(signal).is_some_and(|s| {
                let working = w.routes_from[s.idx()].iter().any(|r| {
                    let st = &il.routes[r.idx()];
                    st.state != RouteState::Idle && st.cancel.is_none() && st.auto_working
                });
                working == *on
            }),
            Condition::TrainAt { headcode, place, platform } => train(headcode).is_some_and(|t| {
                t.dwell.is_some_and(|d| {
                    let p = &net.platforms[d.platform.idx()];
                    p.place == *place && p.platform == *platform
                })
            }),
            Condition::TrainPassed { headcode, signal } => self.seen.passed.contains(&(headcode.clone(), signal.clone())),
            Condition::TrainLeftArea { headcode } => {
                self.seen.in_area.contains(headcode)
                    && !sim.trains().iter().any(|t| {
                        t.headcode == *headcode
                            && t.segments().any(|s| net.sections[net.segments[s.idx()].section.idx()].area == self.area)
                    })
            }
            Condition::Berth { name, headcode } => {
                net.berth(name).is_some_and(|b| sim.describer().get(b) == Some(headcode.as_str()))
            }
            Condition::Rejected {} => self.progress.rejected,
            Condition::Clock { paused } => g.clock().paused == *paused,
            Condition::All(v) => v.iter().all(|c| self.met(g, c)),
        }
    }
}

fn exit_of(g: &Game, exit: &protocol::ExitName) -> Option<Exit> {
    let net = &g.sim().world().net;
    Some(match exit {
        protocol::ExitName::Signal(s) => Exit::Signal(net.signal(s)?),
        protocol::ExitName::Node(n) => Exit::Node(net.node(n)?),
    })
}
