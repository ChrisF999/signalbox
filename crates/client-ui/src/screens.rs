//! The screens (spec D1 §3, realism spec §3–§4, tutorial spec §3–§4): the
//! lobby with its Tutorial list, and in a game the top bar (game,
//! workstation, clock, votes, settings, players), the diagram, the lesson
//! box in a tutorial, the train list or the simplifier and the alarms on
//! the right, and the headcode enquiry window. `UiApp::ui` is the whole
//! frame; the shell calls it.

use std::time::Duration;

use client_core::simplifier::{self, Line};
use client_core::text::{fmt_hms, proposal_text, train_state_text, vote_text};
use client_core::trains::train_list;
use client_core::{App, AspectMode, LessonTicks, Link, Settings, SettingsStore, Target};
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Frame, Key, Layout, PointerButton, Rect, Response, RichText, Sense, Stroke,
    StrokeKind, Ui, vec2,
};
use protocol::{GameState, Highlight, Proposal};

use crate::camera::Camera;
use crate::hit::hit_test;
use crate::labels::{self, Plan};
use crate::paint::{self, BG, PaintState};
use crate::scene::Scene;

/// Alarms and the connection banner.
pub const ALARM: Color32 = Color32::from_rgb(0xFF, 0x5A, 0x5A);
/// How far one wheel "line" (egui points of scroll) zooms.
const ZOOM_PER_POINT: f32 = 1.0 / 200.0;
/// Simplifier columns, in points: headcode, lateness, from, to, at,
/// platform, arrival, departure (wide enough for `BTHNLGR`, `ML_UP` and
/// `05:03½`).
const SIMPLIFIER_COLUMNS: [f32; 8] = [38.0, 26.0, 50.0, 50.0, 56.0, 48.0, 46.0, 46.0];
/// Between two simplifier cells.
const CELL_GAP: f32 = 2.0;
/// The simplifier's columns and the gaps between them.
const SIMPLIFIER_WIDTH: f32 = {
    let mut w = CELL_GAP * (SIMPLIFIER_COLUMNS.len() - 1) as f32;
    let mut i = 0;
    while i < SIMPLIFIER_COLUMNS.len() {
        w += SIMPLIFIER_COLUMNS[i];
        i += 1;
    }
    w
};
/// The side panel's least width: the simplifier's columns plus the panel's
/// margins and a scroll bar, so the table never scrolls sideways (its
/// header would slip off its columns) and the tabs never resize the panel.
const SIDE_W: f32 = SIMPLIFIER_WIDTH + 24.0;
/// Repaint at least this often (ms): clocks, flashing, reconnect timers.
const REPAINT_MS: u64 = 250;
/// While a tutorial highlight shows: often enough for a smooth 1 Hz pulse.
const PULSE_REPAINT_MS: u64 = 50;

/// The upper half of the side panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SideTab {
    #[default]
    Trains,
    Simplifier,
}

#[derive(Default)]
struct NewGame {
    layout: usize,
    seed: String,
    start: String,
}

pub struct UiApp {
    pub core: App,
    scene: Option<Scene>,
    /// (game, layout generation) the scene was built for.
    scene_key: Option<(String, u64)>,
    cam: Option<Camera>,
    /// (game, area) the camera was fitted for; a change fits again.
    fitted: Option<(String, Option<String>)>,
    diagram: Option<Rect>,
    /// What the open right-click menu is about.
    menu_target: Option<Target>,
    headcode: String,
    new_game: NewGame,
    /// The game whose Delete was pressed and awaits "Yes, delete".
    confirm_delete: Option<String>,
    settings: Settings,
    /// Where the settings are kept between visits (none in most tests).
    store: Option<Box<dyn SettingsStore>>,
    side_tab: SideTab,
    /// The simplifier's headcode search.
    search: String,
    /// The headcode whose enquiry window is open.
    enquiry: Option<String>,
    /// The game drawn last frame; another (or the lobby) forgets the
    /// enquiry, the search and the simplifier lines.
    shown_game: Option<String>,
    /// The simplifier's lines (each marked if it is its row's first) for
    /// (layout generation, search).
    simplifier_lines: Option<((u64, String), Vec<(Line, bool)>)>,
    /// Where the diagram's texts go, for (game, layout generation, scale
    /// bits, numbers on): made again only on a zoom or a settings change.
    placement: Option<(PlacementKey, Plan)>,
    /// The simplifier line to scroll to once, set when its lines are built.
    simplifier_scroll: Option<usize>,
    /// The lessons completed in this browser.
    ticks: LessonTicks,
    /// Where the ticks are kept between visits (none in most tests).
    ticks_store: Option<Box<dyn SettingsStore>>,
}

type PlacementKey = (String, u64, u32, bool);

impl UiApp {
    pub fn new(core: App) -> UiApp {
        UiApp {
            core,
            scene: None,
            scene_key: None,
            cam: None,
            fitted: None,
            diagram: None,
            menu_target: None,
            headcode: String::new(),
            new_game: NewGame::default(),
            confirm_delete: None,
            settings: Settings::default(),
            store: None,
            side_tab: SideTab::default(),
            search: String::new(),
            enquiry: None,
            shown_game: None,
            simplifier_lines: None,
            placement: None,
            simplifier_scroll: None,
            ticks: LessonTicks::default(),
            ticks_store: None,
        }
    }

    /// With the settings `store` holds, saving every change back to it.
    pub fn with_store(core: App, store: Box<dyn SettingsStore>) -> UiApp {
        let mut ui = UiApp::new(core);
        ui.settings = store.load().map_or_else(Settings::default, |t| Settings::from_text(&t));
        ui.store = Some(store);
        ui
    }

    /// `with_store`, and the lesson ticks `lessons` holds, saving each new
    /// one back to it.
    pub fn with_stores(core: App, settings: Box<dyn SettingsStore>, lessons: Box<dyn SettingsStore>) -> UiApp {
        let mut ui = UiApp::with_store(core, settings);
        ui.ticks = lessons.load().map_or_else(LessonTicks::default, |t| LessonTicks::from_text(&t));
        ui.ticks_store = Some(lessons);
        ui
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    /// The lessons completed in this browser.
    pub fn ticks(&self) -> &LessonTicks {
        &self.ticks
    }

    /// A finished lesson is ticked (and kept) as soon as it is seen.
    fn tick_finished_lesson(&mut self) {
        let Some(id) = self.core.game().and_then(|g| g.lesson()).filter(|v| v.done).map(|v| v.lesson.clone()) else { return };
        if self.ticks.insert(&id) {
            if let Some(store) = self.ticks_store.as_mut() {
                store.save(&self.ticks.to_text());
            }
        }
    }

    /// The highlights of the lesson step showing (none outside a tutorial).
    fn highlights(&self) -> Vec<Highlight> {
        self.core.game().and_then(|g| g.lesson()).map_or_else(Vec::new, |v| if v.done { vec![] } else { v.highlight.clone() })
    }

    fn set_settings(&mut self, s: Settings) {
        if s == self.settings {
            return;
        }
        self.settings = s;
        if let Some(store) = self.store.as_mut() {
            store.save(&s.to_text());
        }
        if !s.enquiry {
            self.enquiry = None;
        }
    }

    /// The headcode whose enquiry window is open.
    pub fn enquiry(&self) -> Option<&str> {
        self.enquiry.as_deref()
    }

    /// The diagram's camera (tests and the "Fit" button).
    pub fn camera(&self) -> Option<Camera> {
        self.cam
    }

    /// Where the diagram was drawn last frame.
    pub fn diagram_rect(&self) -> Option<Rect> {
        self.diagram
    }

    /// One frame: move the app on with egui's clock, then draw.
    pub fn ui(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        self.core.tick(now);
        self.tick_finished_lesson();
        let game = self.core.game().map(|g| g.id.clone());
        if game != self.shown_game {
            self.enquiry = None;
            self.search.clear();
            self.simplifier_lines = None;
            self.shown_game = game;
        }
        if let Some(b) = self.core.banner() {
            egui::Panel::top("banner").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(b).color(ALARM).strong());
                    if self.core.link() == Link::Replaced && ui.button("Use it here").clicked() {
                        self.core.reconnect_now();
                    }
                });
            });
        }
        if self.core.game().is_some() {
            self.game(ui, now);
        } else {
            self.lobby(ui);
        }
        // Clocks, flashing and reconnect timers move without input; a
        // tutorial highlight's pulse needs more frames to look smooth.
        let every = if self.highlights().is_empty() { REPAINT_MS } else { PULSE_REPAINT_MS };
        ui.ctx().request_repaint_after(Duration::from_millis(every));
    }

    fn lobby(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("signalbox");
            if let Some(n) = self.core.lobby_note() {
                ui.label(RichText::new(n).color(ALARM));
            }
            ui.separator();
            self.tutorials(ui);
            ui.separator();
            ui.label(RichText::new("New game").strong());
            let layouts: Vec<String> = self.core.layouts().iter().map(|l| l.name.clone()).collect();
            if layouts.is_empty() {
                ui.label("No layouts yet.");
            } else {
                self.new_game.layout = self.new_game.layout.min(layouts.len() - 1);
                ui.horizontal(|ui| {
                    egui::ComboBox::from_label("Layout").selected_text(layouts[self.new_game.layout].as_str()).show_ui(ui, |ui| {
                        for (i, name) in layouts.iter().enumerate() {
                            ui.selectable_value(&mut self.new_game.layout, i, name.as_str());
                        }
                    });
                    ui.label("Seed");
                    ui.add(egui::TextEdit::singleline(&mut self.new_game.seed).desired_width(90.0).hint_text("random"));
                    ui.label("Start");
                    ui.add(egui::TextEdit::singleline(&mut self.new_game.start).desired_width(70.0).hint_text("HH:MM"));
                    if ui.button("Create").clicked() {
                        let seed = self.new_game.seed.trim().parse().ok();
                        let start = Some(self.new_game.start.trim().to_string()).filter(|s| !s.is_empty());
                        self.core.create_game(&layouts[self.new_game.layout], seed, start);
                    }
                });
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Games").strong());
                if ui.button("Refresh").clicked() {
                    self.core.refresh();
                }
            });
            let games = self.core.games().to_vec();
            if games.is_empty() {
                ui.label("No games yet.");
                return;
            }
            let mut join = None;
            let mut delete = None;
            egui::Grid::new("games").striped(true).show(ui, |ui| {
                for h in ["Game", "Layout", "State", "Time", "Areas", "Players", ""] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for g in &games {
                    ui.label(&g.id);
                    ui.label(&g.layout);
                    let state = match g.state {
                        GameState::Running if g.preparing.is_some() => {
                            g.preparing.as_ref().map(client_core::text::preparing_text).unwrap_or_default()
                        }
                        GameState::Running => "running".to_string(),
                        GameState::Saved => "saved".to_string(),
                        GameState::Crashed => format!("crashed: {}", g.error.as_deref().unwrap_or("?")),
                    };
                    ui.label(state);
                    ui.label(fmt_hms(g.sim_time));
                    let areas: Vec<String> =
                        g.areas.iter().map(|a| format!("{} ({})", a.name, a.holder.as_deref().unwrap_or("robot"))).collect();
                    ui.label(areas.join(", "));
                    ui.label(g.players.join(", "));
                    ui.horizontal(|ui| {
                        if ui.button(if g.state == GameState::Running { "Join" } else { "Resume" }).clicked() {
                            join = Some(g.id.clone());
                        }
                        // Owner decision 13: the front re-checks all of it.
                        if g.can_delete {
                            if self.confirm_delete.as_deref() == Some(g.id.as_str()) {
                                ui.label(RichText::new("Delete for good?").color(ALARM));
                                if ui.button("Yes, delete").clicked() {
                                    delete = Some(g.id.clone());
                                }
                                if ui.button("Cancel").clicked() {
                                    self.confirm_delete = None;
                                }
                            } else if ui.button("Delete").clicked() {
                                self.confirm_delete = Some(g.id.clone());
                            }
                        }
                    });
                    ui.end_row();
                }
            });
            if let Some(id) = join {
                self.core.join(&id);
            }
            if let Some(id) = delete {
                self.confirm_delete = None;
                self.core.delete_game(&id);
            }
        });
    }

    /// The lobby's Tutorial list: each lesson, ticked once done here.
    fn tutorials(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Tutorial").strong());
        let lessons = self.core.lessons().to_vec();
        if lessons.is_empty() {
            ui.label("No lessons on this server.");
            return;
        }
        let mut start = None;
        egui::Grid::new("lessons").striped(true).show(ui, |ui| {
            for l in &lessons {
                ui.label(&l.title);
                ui.label(format!("{} steps", l.steps));
                ui.label(if self.ticks.done(&l.id) { RichText::new("done").color(paint::GREEN) } else { RichText::new("") });
                if ui.button("Start").clicked() {
                    start = Some(l.id.clone());
                }
                ui.end_row();
            }
        });
        if let Some(id) = start {
            self.core.start_lesson(&id);
        }
    }

    fn game(&mut self, ui: &mut Ui, now: f64) {
        self.top_bar(ui, now);
        egui::Panel::right("side").default_size(SIDE_W).min_size(SIDE_W).show(ui, |ui| self.side(ui, now));
        // After the side panel, so a tab clicked this frame is told at once.
        let tab = match self.side_tab {
            SideTab::Trains => "trains",
            SideTab::Simplifier => "simplifier",
        };
        self.core.report_screen(tab);
        egui::CentralPanel::default().frame(Frame::NONE.fill(BG)).show(ui, |ui| self.diagram_ui(ui, now));
        self.enquiry_window(ui);
    }

    fn top_bar(&mut self, ui: &mut Ui, now: f64) {
        let Some(g) = self.core.game() else { return };
        let lesson = g.lesson().is_some();
        let id = if lesson { "Tutorial" } else { g.id.as_str() };
        let title = match g.area() {
            Some(a) => match g.names().workstation(a) {
                Some(ws) => format!("{id} · Workstation {ws} · {a} ({})", g.you),
                None => format!("{id} · {a} ({})", g.you),
            },
            None => format!("{id} · spectating ({})", g.you),
        };
        let lit = self.highlights();
        let marked = |name: &str| lit.contains(&Highlight::Ui(name.to_string()));
        let view = g.view().cloned();
        let areas: Vec<String> = g.layout().map(|l| l.areas.clone()).unwrap_or_default();
        let holding = g.area().is_some();
        let can_vote = g.can_vote();
        let mut act: Vec<Box<dyn FnOnce(&mut App)>> = Vec::new();
        let mut settings = self.settings;
        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(title).strong());
                if let Some(v) = &view {
                    let clock = ui.label(RichText::new(fmt_hms(v.sim_time)).monospace().size(16.0));
                    mark(ui, &clock, marked("clock"), now);
                    ui.label(if v.paused { "paused".to_string() } else { format!("{}×", v.speed) });
                    // Only voters get the buttons (owner decision 12).
                    if can_vote {
                        let pause = if v.paused { Proposal::Resume } else { Proposal::Pause };
                        if ui.button(proposal_text(pause)).clicked() {
                            act.push(Box::new(move |a| a.vote(pause)));
                        }
                        for x in [1u8, 2, 4, 8] {
                            if ui.selectable_label(!v.paused && v.speed == x, format!("{x}×")).clicked() {
                                act.push(Box::new(move |a| a.vote(Proposal::Speed { x })));
                            }
                        }
                    }
                    if let Some(vote) = &v.vote {
                        ui.label(RichText::new(vote_text(vote)).color(paint::YELLOW));
                    }
                    // A tutorial keeps no score (tutorial spec §3).
                    if let Some(score) = v.score.filter(|_| !lesson) {
                        ui.label(format!("Penalty {score}"));
                    }
                }
                if ui.button("Fit").clicked() {
                    self.fitted = None;
                }
                let menu = ui.menu_button("Settings", |ui| {
                    ui.label(RichText::new("Signal aspects").strong());
                    ui.radio_value(&mut settings.aspects, AspectMode::RedGreen, "Red/green (panel)");
                    ui.radio_value(&mut settings.aspects, AspectMode::Real, "Real aspects");
                    ui.separator();
                    ui.checkbox(&mut settings.enquiry, "Headcode enquiry");
                    ui.checkbox(&mut settings.numbers, "Signal numbers");
                });
                mark(ui, &menu.response, marked("settings"), now);
                // A tutorial's player keeps the lesson's area.
                if holding && !lesson {
                    if ui.button("Release area").clicked() {
                        act.push(Box::new(|a| a.release()));
                    }
                }
                if ui.button("Leave").clicked() {
                    act.push(Box::new(|a| a.leave()));
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Players:");
                for area in &areas {
                    let holder = view.as_ref().and_then(|v| v.holders.get(area)).map_or("robot", String::as_str);
                    ui.label(format!("{area}: {holder}"));
                    if !holding && !lesson && holder == "robot" && ui.small_button("Claim").clicked() {
                        let area = area.clone();
                        act.push(Box::new(move |a| a.claim(&area)));
                    }
                }
            });
        });
        self.set_settings(settings);
        for f in act {
            f(&mut self.core);
        }
    }

    /// In a tutorial the lesson box on top; then the train list or the
    /// simplifier; below them the alarms, always in view.
    fn side(&mut self, ui: &mut Ui, now: f64) {
        self.lesson_box(ui);
        let lit = self.highlights();
        ui.horizontal(|ui| {
            let trains = ui.selectable_value(&mut self.side_tab, SideTab::Trains, RichText::new("TRAINS").strong());
            mark(ui, &trains, lit.contains(&Highlight::Ui("trains".into())), now);
            let was_simplifier = self.side_tab == SideTab::Simplifier;
            let simplifier = ui.selectable_value(&mut self.side_tab, SideTab::Simplifier, RichText::new("SIMPLIFIER").strong());
            mark(ui, &simplifier, lit.contains(&Highlight::Ui("simplifier".into())), now);
            if simplifier.clicked() && !was_simplifier {
                // Opened again: build the lines afresh and scroll to now.
                self.simplifier_lines = None;
            }
        });
        let half = ui.available_height() * 0.5;
        match self.side_tab {
            SideTab::Trains => self.trains_ui(ui, half),
            SideTab::Simplifier => self.simplifier_ui(ui, half),
        }
        let Some(g) = self.core.game() else { return };
        ui.separator();
        ui.label(RichText::new("ALARMS").strong());
        egui::ScrollArea::vertical().id_salt("alarms").show(ui, |ui| {
            for e in g.log().entries().rev() {
                let when = e.sim_time.map(fmt_hms).unwrap_or_default();
                let text = RichText::new(format!("{when} {}", e.text));
                ui.label(if e.alarm { text.color(ALARM) } else { text });
            }
        });
    }

    /// The lesson (tutorial spec §4): title, step, what to do, the alert,
    /// and its buttons; on `done`, the way back to the lobby.
    fn lesson_box(&mut self, ui: &mut Ui) {
        let Some(v) = self.core.game().and_then(|g| g.lesson()).cloned() else { return };
        let mut act: Option<fn(&mut App)> = None;
        ui.label(RichText::new(&v.title).strong().size(15.0));
        if v.done {
            ui.label(RichText::new("Lesson complete. Well done!").color(paint::GREEN));
            ui.horizontal(|ui| {
                if ui.button("Back to tutorials").clicked() {
                    act = Some(App::leave);
                }
                if ui.button("Restart lesson").clicked() {
                    act = Some(App::lesson_restart);
                }
            });
        } else {
            ui.label(format!("Step {} of {}", v.index + 1, v.count));
            ui.label(RichText::new(&v.say).size(14.0));
            if let Some(a) = &v.alert {
                ui.label(RichText::new(a).color(ALARM));
            }
            ui.horizontal(|ui| {
                if v.needs_next && ui.button("Next").clicked() {
                    act = Some(App::lesson_next);
                }
                if ui.button("Restart step").clicked() {
                    act = Some(App::lesson_restart_step);
                }
                if ui.button("Restart lesson").clicked() {
                    act = Some(App::lesson_restart);
                }
                if ui.button("Leave").clicked() {
                    act = Some(App::leave);
                }
            });
        }
        ui.separator();
        if let Some(f) = act {
            f(&mut self.core);
        }
    }

    fn trains_ui(&mut self, ui: &mut Ui, height: f32) {
        let Some(g) = self.core.game() else { return };
        let enquiry = self.settings.enquiry;
        let mut open = None;
        egui::ScrollArea::vertical().id_salt("trains").max_height(height).show(ui, |ui| {
            let Some(v) = g.view() else { return };
            egui::Grid::new("train_list").striped(true).show(ui, |ui| {
                for (h, r) in train_list(v) {
                    let code = RichText::new(g.names().headcode(h)).monospace().color(paint::HEADCODE);
                    // With the enquiry on, a headcode opens its window.
                    if enquiry {
                        if ui.add(egui::Label::new(code).sense(Sense::click())).clicked() {
                            open = Some(h.to_string());
                        }
                    } else {
                        ui.label(code);
                    }
                    ui.label(train_state_text(r.state));
                    let next = match (&r.next_place, &r.next_platform) {
                        (Some(p), Some(pf)) => format!("{p} {pf}"),
                        (Some(p), None) => p.clone(),
                        (None, _) => "—".to_string(),
                    };
                    ui.label(next);
                    ui.label(r.booked.map_or(String::new(), |b| fmt_hms(b)[..5].to_string()));
                    ui.label(if r.late_s > 0 { format!("+{}", r.late_s / 60) } else { String::new() });
                    ui.end_row();
                }
            });
        });
        if open.is_some() {
            self.enquiry = open;
        }
    }

    /// The simplifier (realism spec §3): the layout's rows in running
    /// order, searched by headcode, drawn only where the scroll shows them.
    /// The lines are built once per layout and search; lateness only for
    /// the lines in view.
    fn simplifier_ui(&mut self, ui: &mut Ui, height: f32) {
        ui.add(egui::TextEdit::singleline(&mut self.search).id_salt("simplifier_search").desired_width(120.0).hint_text("headcode"));
        let Some(g) = self.core.game() else { return };
        let Some(l) = g.layout() else { return };
        // The scroll target needs the clock: wait for the first view.
        let Some(v) = g.view() else { return };
        let key = (g.layout_gen(), self.search.clone());
        if self.simplifier_lines.as_ref().map(|(k, _)| k) != Some(&key) {
            let rows = simplifier::rows(l, &self.search);
            self.simplifier_scroll = Some(simplifier::now_line(&rows, v.sim_time));
            let lines = rows
                .into_iter()
                .flat_map(|r| simplifier::lines(r).into_iter().enumerate().map(|(i, line)| (line, i == 0)))
                .collect();
            self.simplifier_lines = Some((key, lines));
        }
        let Some((_, lines)) = &self.simplifier_lines else { return };
        if lines.is_empty() {
            ui.label(if l.simplifier.is_empty() { "No booked trains here" } else { "No headcode matches" });
            return;
        }
        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
        let header = ["Train", "Late", "From", "To", "At", "Plat", "Arr", "Dep"];
        simplifier_row(ui, row_h, header.map(|h| RichText::new(h).strong()));
        let mut area = egui::ScrollArea::vertical().id_salt("simplifier").max_height(height);
        if let Some(line) = self.simplifier_scroll.take() {
            area = area.vertical_scroll_offset(line as f32 * (row_h + ui.spacing().item_spacing.y));
        }
        area.show_rows(ui, row_h, lines.len(), |ui, range| {
            for (line, first) in &lines[range] {
                let late = if *first { simplifier::lateness(Some(v), &line.headcode) } else { None };
                let late = late.as_deref().unwrap_or("");
                let cells = [
                    RichText::new(g.names().headcode(&line.headcode)).monospace().color(paint::HEADCODE),
                    RichText::new(late).color(if late == "OT" { paint::LABEL } else { ALARM }),
                    RichText::new(&line.from),
                    RichText::new(&line.to),
                    RichText::new(&line.place),
                    RichText::new(&line.platform),
                    RichText::new(&line.arr),
                    RichText::new(&line.dep),
                ];
                simplifier_row(ui, row_h, cells);
            }
        });
    }

    fn enquiry_window(&mut self, ui: &mut Ui) {
        let Some(h) = self.enquiry.clone() else { return };
        let mut open = true;
        let Some(g) = self.core.game() else { return };
        let (Some(l), v) = (g.layout(), g.view()) else { return };
        let e = simplifier::enquiry(l, v, &h);
        egui::Window::new(format!("Train {}", g.names().headcode(&h))).id(egui::Id::new("enquiry")).open(&mut open).resizable(false).show(ui.ctx(), |ui| {
            ui.label(e.live_text());
            if e.rows.is_empty() {
                ui.label("Not in the simplifier for this area");
            }
            for r in &e.rows {
                ui.label(format!("{} to {}", r.origin.as_deref().unwrap_or("?"), r.destination.as_deref().unwrap_or("?")));
                for line in simplifier::lines(r) {
                    ui.label(format!("{} {} {} {}", line.place, line.platform, line.arr, line.dep));
                }
            }
        });
        if !open {
            self.enquiry = None;
        }
    }

    fn diagram_ui(&mut self, ui: &mut Ui, now: f64) {
        let Some(g) = self.core.game() else { return };
        let key = (g.id.clone(), g.layout_gen());
        if self.scene_key.as_ref() != Some(&key) {
            self.scene = g.layout().and_then(Scene::build);
            self.scene_key = Some(key);
        }
        let fit_key = (g.id.clone(), g.area().map(str::to_string));
        let has_layout = g.layout().is_some();
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        self.diagram = Some(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, BG);
        let Some(scene) = &self.scene else {
            let preparing = self.core.preparing().map(|p| client_core::text::preparing_text(&p));
            let msg = match preparing {
                _ if has_layout => "No diagram for this layout".to_string(),
                Some(text) => text,
                None => "Waiting for the layout…".to_string(),
            };
            painter.text(rect.center(), Align2::CENTER_CENTER, msg, FontId::proportional(16.0), paint::LABEL);
            return;
        };
        if self.fitted.as_ref() != Some(&fit_key) || self.cam.is_none() {
            self.cam = Some(scene.fit_bounds().map_or(Camera { centre: rect.center(), scale: 1.0 }, |b| Camera::fit(b, rect)));
            self.fitted = Some(fit_key);
        }
        let Some(cam) = self.cam.as_mut() else { return };
        if resp.dragged_by(PointerButton::Primary) {
            cam.pan(resp.drag_delta());
        }
        if let Some(p) = resp.hover_pos() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            if scroll != 0.0 {
                cam.zoom_at(rect, p, (scroll * ZOOM_PER_POINT).exp());
            }
            if zoom != 1.0 {
                cam.zoom_at(rect, p, zoom);
            }
        }
        let cam = *cam;
        let view = self.core.game().and_then(|g| g.view());
        let hit_at = |p: Option<egui::Pos2>| p.and_then(|p| hit_test(scene, view, &cam, rect, p));
        let hover = hit_at(resp.hover_pos());
        // Every click goes on, even one on nothing or on what is not yours:
        // a dead click clears the entrance (`App::click` decides what the
        // rest mean, from the same operability `Hit::clickable` shows).
        let click = resp.clicked().then(|| hit_at(resp.interact_pointer_pos()).map(|h| h.target));
        if resp.secondary_clicked() {
            self.menu_target = hit_at(resp.interact_pointer_pos()).map(|h| h.target);
        }
        let exits = self.core.valid_exits();
        let highlight = self.highlights();
        let Some(g) = self.core.game() else { return };
        let st = PaintState {
            view: g.view(),
            selected: g.selected(),
            exits: &exits,
            refused: g.refused(),
            time: now,
            aspects: self.settings.aspects,
            numbers: self.settings.numbers,
            names: g.names(),
            highlight: &highlight,
        };
        let d = paint::draw(scene, &cam, rect, &st);
        let key = (g.id.clone(), g.layout_gen(), cam.scale.to_bits(), self.settings.numbers);
        if self.placement.as_ref().is_none_or(|(k, p)| *k != key || p.spots.len() != d.movable.len()) {
            let plan = ui.ctx().fonts_mut(|f| {
                labels::plan(&d, &mut |t| f.layout_no_wrap(t.text.clone(), paint::font(t), t.colour).size())
            });
            self.placement = Some((key, plan));
        }
        let d = match &self.placement {
            Some((_, plan)) => labels::apply(d, plan),
            None => d,
        };
        paint::paint(&painter, d);
        match click {
            // With the enquiry on, a headcode opens its window and nothing else.
            Some(Some(t)) => match self.core.headcode_at(&t).filter(|_| self.settings.enquiry) {
                Some(h) => self.enquiry = Some(h),
                None => self.core.click(&t),
            },
            Some(None) => self.core.escape(),
            None => {}
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.core.escape();
            self.menu_target = None;
        }
        let resp = match &hover {
            Some(h) => resp.on_hover_text_at_pointer(self.core.describe(&h.target)),
            None => resp,
        };
        resp.context_menu(|ui| self.menu_ui(ui));
    }

    fn menu_ui(&mut self, ui: &mut Ui) {
        let Some(t) = self.menu_target.clone() else {
            ui.close();
            return;
        };
        ui.label(self.core.describe(&t));
        let items = self.core.menu(&t);
        let interpose = match &t {
            Target::Berth(b) if self.core.can_interpose(b) => Some(b.clone()),
            _ => None,
        };
        if items.is_empty() && interpose.is_none() {
            return;
        }
        ui.separator();
        for item in items {
            if ui.button(&item.label).clicked() {
                self.core.command(item.cmd);
                ui.close();
            }
        }
        if let Some(b) = interpose {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.headcode).desired_width(60.0).hint_text("1A01"));
                if ui.button("Interpose").clicked() {
                    self.core.interpose(&b, &self.headcode);
                    self.headcode.clear();
                    ui.close();
                }
            });
        }
    }
}

/// The tutorial's pulsing outline round a control the step points at,
/// kept inside the clip so a control at a panel's edge is outlined whole.
fn mark(ui: &Ui, r: &Response, on: bool, now: f64) {
    if on {
        let stroke = Stroke::new(paint::HIGHLIGHT_W, paint::highlight_colour(now));
        let rect = r.rect.expand(2.0).intersect(ui.clip_rect().shrink(paint::HIGHLIGHT_W));
        ui.painter().rect_stroke(rect, CornerRadius::same(3), stroke, StrokeKind::Outside);
    }
}

/// One simplifier line in fixed-width cells.
fn simplifier_row(ui: &mut Ui, row_h: f32, cells: [RichText; 8]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = CELL_GAP;
        for (text, w) in cells.into_iter().zip(SIMPLIFIER_COLUMNS) {
            ui.allocate_ui_with_layout(vec2(w, row_h), Layout::left_to_right(Align::Center), |ui| {
                ui.set_min_width(w);
                ui.add(egui::Label::new(text).truncate());
            });
        }
    });
}
