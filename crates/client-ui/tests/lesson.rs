//! Tutorials on screen (tutorial spec §3–§4), headless: the lobby's
//! Tutorial list with its ticks, and a real lesson (a `game::lesson`
//! runner in process) driven through the lesson box and the diagram.

mod common;

use client_core::{App, MemHandle, MemStore, MemTransport};
use client_ui::UiApp;
use common::*;
use egui::{Event, FullOutput, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape, pos2, vec2};
use game::Game;
use game::lesson::{self, Runner};
use protocol::*;

const LESSONS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../lessons");

struct Rig {
    ctx: egui::Context,
    ui: UiApp,
    h: MemHandle,
    /// The tutorial, once one is started.
    game: Option<(Game, Runner)>,
    t: f64,
    events: Vec<Event>,
    lobby_sent: Vec<LobbyMsg>,
    ticks: MemStore,
}

fn texts(out: &FullOutput) -> Vec<(String, Rect)> {
    out.shapes
        .iter()
        .filter_map(|c| match &c.shape {
            Shape::Text(t) => Some((t.galley.text().to_string(), t.galley.rect.translate(t.pos.to_vec2()))),
            _ => None,
        })
        .collect()
}

fn has_text(out: &FullOutput, want: &str) -> bool {
    texts(out).iter().any(|(t, _)| t.contains(want))
}

/// The highlight colour at some point of its pulse (alpha premultiplied).
fn is_highlight(c: egui::Color32) -> bool {
    let h = client_ui::paint::HIGHLIGHT;
    c.a() > 0 && c.r() > 0 && (f32::from(c.g()) / f32::from(c.r()) - f32::from(h.g()) / f32::from(h.r())).abs() < 0.02
}

fn find(out: &FullOutput, want: &str) -> Pos2 {
    texts(out).into_iter().find(|(t, _)| t == want).unwrap_or_else(|| panic!("no `{want}` in {:?}", texts(out))).1.center()
}

impl Rig {
    /// In the lobby with the shipped lessons listed; `ticked` already done.
    fn lobby(ticked: &str) -> Rig {
        let (tr, h) = MemTransport::new();
        let core = App::new(Box::new(tr), 0.0);
        h.open();
        let ticks = MemStore::new();
        let mut store = ticks.clone();
        client_core::SettingsStore::save(&mut store, ticked);
        let ui = UiApp::with_stores(core, Box::new(MemStore::new()), Box::new(ticks.clone()));
        let mut r = Rig { ctx: egui::Context::default(), ui, h, game: None, t: 0.0, events: vec![], lobby_sent: vec![], ticks };
        r.frame();
        r.lobby_sent.clear();
        let mut dirs: Vec<_> = std::fs::read_dir(LESSONS).unwrap().map(|e| e.unwrap().path()).collect();
        dirs.sort();
        let lessons = dirs
            .iter()
            .map(|d| {
                let l = lesson::load_lesson(d).unwrap();
                LessonInfo { id: l.id, title: l.file.title, steps: l.file.steps.len() as u32 }
            })
            .collect();
        r.h.push(ServerFrame::Lobby(LobbyReply::Lessons { lessons }));
        r.frame();
        r
    }

    /// Answer `start_lesson` as the front and the game process would.
    fn serve_start(&mut self) {
        let Some(LobbyMsg::StartLesson { lesson: id }) = self.lobby_sent.pop() else { panic!("{:?}", self.lobby_sent) };
        let l = lesson::load_lesson(&std::path::Path::new(LESSONS).join(&id)).unwrap();
        let (mut g, mut r) = lesson::start(l);
        self.h.push(ServerFrame::Lobby(LobbyReply::Joined { game: s("g-tut"), you: s("ann") }));
        for (_, m) in r.connect(&mut g, "ann") {
            self.h.push(ServerFrame::Game(m));
        }
        self.game = Some((g, r));
        self.frame();
        self.frame();
    }

    /// A tutorial of lesson `id`, started from the lobby.
    fn in_lesson(id: &str) -> Rig {
        let mut r = Rig::lobby("");
        r.lobby_sent.push(LobbyMsg::StartLesson { lesson: s(id) });
        r.serve_start();
        r
    }

    fn frame(&mut self) -> FullOutput {
        self.t += 0.1;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
            time: Some(self.t),
            events: std::mem::take(&mut self.events),
            ..RawInput::default()
        };
        let mut out = self.ctx.run_ui(input, |ui| self.ui.ui(ui));
        out.textures_delta.clear();
        for f in self.h.take_sent() {
            match f {
                ClientFrame::Game(m) => {
                    if let Some((g, r)) = self.game.as_mut() {
                        for (p, reply) in r.handle(g, "ann", m) {
                            if p == "ann" {
                                self.h.push(ServerFrame::Game(reply));
                            }
                        }
                    }
                }
                ClientFrame::Lobby(m) => self.lobby_sent.push(m),
            }
        }
        if let Some((g, r)) = self.game.as_mut() {
            let mut msgs = r.advance(g, 0.1);
            msgs.extend(g.flush());
            for (p, m) in msgs {
                if p == "ann" {
                    self.h.push(ServerFrame::Game(m));
                }
            }
        }
        out
    }

    fn click(&mut self, at: Pos2) {
        self.events.push(Event::PointerMoved(at));
        self.frame();
        self.events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::default() });
        self.frame();
        self.events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::default() });
        self.frame();
    }

    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.ui.camera().unwrap().to_screen(self.ui.diagram_rect().unwrap(), pos2(x, y))
    }

    fn step(&self) -> usize {
        self.game.as_ref().unwrap().1.step()
    }

    /// Frames until the lesson box shows `want` (5 s).
    fn until(&mut self, want: &str) -> FullOutput {
        for _ in 0..50 {
            let out = self.frame();
            if has_text(&out, want) {
                return out;
            }
        }
        panic!("`{want}` never shown");
    }
}

#[test]
fn the_lobby_lists_the_lessons_ticks_the_done_ones_and_starts_one() {
    let mut r = Rig::lobby("01-reading-the-panel\n");
    let out = r.frame();
    for want in ["Tutorial", "Reading the panel", "10 steps", "Setting & cancelling routes", "Junctions, auto-working & handovers"] {
        assert!(has_text(&out, want), "{want} in {:?}", texts(&out));
    }
    assert_eq!(texts(&out).iter().filter(|(t, _)| t == "done").count(), 1, "only lesson 1 is done here");
    let starts: Vec<Pos2> = texts(&out).into_iter().filter(|(t, _)| t == "Start").map(|(_, at)| at.center()).collect();
    assert_eq!(starts.len(), 4);
    r.click(starts[2]);
    assert_eq!(r.lobby_sent, [LobbyMsg::StartLesson { lesson: s("03-running-trains") }]);
}

#[test]
fn a_lesson_is_followed_through_the_lesson_box_and_the_diagram() {
    let mut r = Rig::in_lesson("02-setting-routes");
    let out = r.frame();
    for want in ["Setting & cancelling routes", "Step 1 of 10", "Hollins Cross station has two platforms", "Next", "Restart step", "Restart lesson"] {
        assert!(has_text(&out, want), "{want} in {:?}", texts(&out));
    }
    assert!(has_text(&out, "Tutorial · Hollins Cross (ann)"), "{:?}", texts(&out));
    for gone in ["Penalty", "Release area", "Claim"] {
        assert!(!has_text(&out, gone), "{gone} is not offered in a lesson");
    }
    r.click(find(&out, "Next"));
    let out = r.until("Step 2 of 10");
    assert!(!has_text(&out, "Next"), "this step waits for a click on the diagram");
    // The step's highlight pulses round H3.
    let rings = out.shapes.iter().filter(|c| matches!(&c.shape, Shape::Circle(cs) if is_highlight(cs.stroke.color))).count();
    assert_eq!(rings, 1, "a highlight ring round H3");
    let h3 = r.at(250.0, 0.0);
    r.click(h3);
    r.until("Step 3 of 10");
    assert_eq!(r.step(), 2, "the selection reached the lesson");
    let out = r.frame();
    r.click(find(&out, "Next"));
    r.until("Step 4 of 10");
    assert_eq!(r.ui.core.game().unwrap().selected(), Some("3"), "H3 is still the entrance");
    r.click(r.at(405.0, 0.0));
    r.until("Step 5 of 10");
    let out = r.frame();
    r.click(find(&out, "Restart step"));
    let out = r.until("Step 5 of 10");
    assert!(has_text(&out, "Now cancel the route"));
}

#[test]
fn finishing_ticks_the_lesson_and_leads_back_to_the_lobby() {
    let mut r = Rig::in_lesson("01-reading-the-panel");
    for i in 0..10 {
        let out = r.until(&format!("Step {} of 10", i + 1));
        if has_text(&out, "Next") {
            r.click(find(&out, "Next"));
        }
        // Steps that wait on the train: let it run.
        for _ in 0..1200 {
            if r.step() > i {
                break;
            }
            r.frame();
        }
        assert!(r.step() > i, "step {} never ended", i + 1);
        if i == 9 {
            break;
        }
    }
    let out = r.until("Lesson complete");
    assert_eq!(r.ticks.text().as_deref(), Some("01-reading-the-panel\n"), "kept for the next visit");
    assert!(r.ui.ticks().done("01-reading-the-panel"));
    r.click(find(&out, "Back to tutorials"));
    assert_eq!(r.lobby_sent, [LobbyMsg::Leave]);
    let out = r.frame();
    assert!(has_text(&out, "Tutorial") && has_text(&out, "done"));
}

#[test]
fn ui_highlights_outline_the_named_controls() {
    let mut r = Rig::in_lesson("03-running-trains");
    let out = r.frame();
    r.click(find(&out, "Next"));
    let out = r.until("Step 2 of 10");
    let trains = find(&out, "TRAINS");
    let outlined = |out: &FullOutput, at: Pos2| {
        out.shapes.iter().any(|c| matches!(&c.shape, Shape::Rect(rs) if rs.stroke.width == client_ui::paint::HIGHLIGHT_W && rs.rect.expand(4.0).contains(at)))
    };
    assert!(outlined(&out, trains), "the TRAINS tab is outlined");
    r.click(find(&out, "Next"));
    let out = r.until("Step 3 of 10");
    let simplifier = find(&out, "SIMPLIFIER");
    assert!(outlined(&out, simplifier));
    r.click(simplifier);
    r.until("Step 4 of 10");
}

/// Is `at` inside a highlight outline, drawn whole (not cut by its clip)?
fn outlined_whole(out: &FullOutput, at: Pos2) -> bool {
    out.shapes.iter().any(|c| match &c.shape {
        Shape::Rect(rs) => {
            rs.stroke.width == client_ui::paint::HIGHLIGHT_W
                && rs.rect.contains(at)
                && c.clip_rect.contains_rect(rs.rect.expand(rs.stroke.width))
        }
        _ => false,
    })
}

#[test]
fn the_clock_and_settings_highlights_outline_them_in_the_top_bar() {
    let mut r = Rig::in_lesson("02-setting-routes");
    let out = r.frame();
    // The top bar wraps: a label's galley starts at the row's left, so
    // take a point at the clock text's right end.
    let clock = |out: &FullOutput| {
        let r = texts(out).into_iter().find(|(t, _)| t.len() == 8 && t.as_bytes()[2] == b':' && t.as_bytes()[5] == b':').unwrap().1;
        pos2(r.max.x - 4.0, r.center().y)
    };
    assert!(!outlined_whole(&out, clock(&out)) && !outlined_whole(&out, find(&out, "Settings")));
    let mut v = r.ui.core.game().unwrap().lesson().unwrap().clone();
    v.highlight = vec![Highlight::Ui(s("clock")), Highlight::Ui(s("settings"))];
    r.h.push(ServerFrame::Game(ServerMsg::Lesson(v)));
    r.frame();
    let out = r.frame();
    assert!(outlined_whole(&out, clock(&out)), "the clock is outlined");
    assert!(outlined_whole(&out, find(&out, "Settings")), "Settings is outlined");
}

#[test]
fn a_showing_highlight_repaints_often_enough_to_pulse() {
    let delay = |out: &FullOutput| out.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
    let mut r = Rig::in_lesson("03-running-trains");
    for _ in 0..5 {
        r.frame();
    }
    assert!(r.ui.core.game().unwrap().lesson().unwrap().highlight.is_empty());
    let out = r.frame();
    assert!(delay(&out) > std::time::Duration::from_millis(200), "no highlight: the usual pace, not {:?}", delay(&out));
    let mut r = Rig::in_lesson("02-setting-routes");
    for _ in 0..5 {
        r.frame();
    }
    let out = r.frame();
    assert!(delay(&out) <= std::time::Duration::from_millis(50), "{:?}", delay(&out));
}

#[test]
fn leave_in_the_lesson_box_goes_back_to_the_lobby() {
    let mut r = Rig::in_lesson("02-setting-routes");
    let out = r.frame();
    let right = r.ui.diagram_rect().unwrap().max.x;
    let leave = texts(&out).into_iter().find(|(t, at)| t == "Leave" && at.min.x >= right).expect("Leave in the lesson box").1.center();
    r.click(leave);
    assert_eq!(r.lobby_sent, [LobbyMsg::Leave]);
    assert!(r.ui.core.game().is_none());
}

/// egui's default fonts draw a box for a missing glyph: the Tutorial list,
/// the lesson box and every lesson's text must all have theirs.
#[test]
fn every_tutorial_character_has_a_glyph() {
    let mut r = Rig::in_lesson("02-setting-routes");
    let mut shown: String = texts(&r.frame()).into_iter().map(|(t, _)| t).collect();
    shown.extend(texts(&Rig::lobby("01-reading-the-panel\n").frame()).into_iter().map(|(t, _)| t));
    for t in ["Lesson complete. Well done!", "Back to tutorials", lesson::SPAD_ALERT, lesson::COLLISION_ALERT] {
        shown.push_str(t);
    }
    let mut dirs: Vec<_> = std::fs::read_dir(LESSONS).unwrap().map(|e| e.unwrap().path()).collect();
    dirs.sort();
    for d in dirs {
        let l = lesson::load_lesson(&d).unwrap();
        shown.push_str(&l.file.title);
        for step in &l.file.steps {
            shown.push_str(&step.say);
        }
    }
    let missing: Vec<char> = r.ctx.fonts_mut(|f| {
        shown
            .chars()
            .filter(|c| !c.is_whitespace())
            .filter(|&c| !f.has_glyph(&egui::FontId::proportional(14.0), c) && !f.has_glyph(&egui::FontId::monospace(14.0), c))
            .collect()
    });
    assert!(missing.is_empty(), "no glyph for {missing:?}");
}
