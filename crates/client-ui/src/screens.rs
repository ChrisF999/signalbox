//! The screens (spec D1 §3, realism spec §3–§4, tutorial spec §3–§4): the
//! lobby with its Tutorial list, and in a game the top bar (game,
//! workstation, clock, votes, settings, players), the diagram, the lesson
//! box in a tutorial, the train list or the simplifier and the alarms on
//! the right, and the headcode enquiry window. `UiApp::ui` is the whole
//! frame; the shell calls it.

use std::time::Duration;

use client_core::names::shown_headcode;
use client_core::simplifier::{self, Line};
use client_core::text::{fmt_hms, proposal_text, train_state_text, vote_text};
use client_core::trains::train_list;
use client_core::{App, AspectMode, LessonTicks, Link, Settings, SettingsStore, Target};
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Frame, Key, Layout, PointerButton, Rect, Response, RichText, Sense, Stroke,
    StrokeKind, Ui, vec2,
};
use protocol::{GameState, Highlight, Proposal, TrainState};

use crate::camera::Camera;
use crate::hit::hit_test;
use crate::labels::{self, Plan};
use crate::paint::{self, BG, PaintState};
use crate::scene::Scene;

/// Alarms and the connection banner.
pub const ALARM: Color32 = Color32::from_rgb(0xFF, 0x5A, 0x5A);
/// How far one wheel "line" (egui points of scroll) zooms: about ×1.2 a
/// notch (polish spec M11; it was ×1.8).
pub const ZOOM_PER_POINT: f32 = 1.0 / 600.0;
/// One press of the zoom buttons or keys.
pub const ZOOM_STEP: f32 = 1.25;
/// The zoom buttons: this big, this far in from the diagram's corner.
const ZOOM_BUTTON: f32 = 26.0;
const ZOOM_INSET: f32 = 8.0;
/// Fit keeps this band clear at the top (the zoom buttons) and, to stay
/// centred, at the bottom (the hint), so no signal or label starts under them.
pub const VIEW_BAND: f32 = 2.0 * ZOOM_INSET + ZOOM_BUTTON;
/// Until the player first moves the view, the diagram says how.
pub const VIEW_HINT: &str = "Drag to move · wheel, + or - to zoom · Fit shows it all";
/// Simplifier columns, in points: headcode, lateness, from, to, at,
/// platform, arrival, departure (wide enough for `BTHNLGR`, `ML_UP` and
/// `05:03½`).
const SIMPLIFIER_COLUMNS: [f32; 8] = [38.0, 26.0, 50.0, 50.0, 56.0, 48.0, 46.0, 46.0];
/// Between two simplifier cells.
const CELL_GAP: f32 = 2.0;
/// The side panel can be dragged this narrow (polish spec H7); the
/// simplifier then scrolls sideways, its header with it.
const SIDE_MIN_W: f32 = 240.0;
/// The side panel's margins and a scroll bar, beside the simplifier.
const SIDE_PAD: f32 = 24.0;

/// The simplifier's columns with the Train column at least `train_w` wide
/// (polish spec H3: Gretz's 8-character headcodes fit, not `W118...`).
pub fn simplifier_columns(train_w: f32) -> [f32; 8] {
    let mut c = SIMPLIFIER_COLUMNS;
    c[0] = c[0].max(train_w.ceil());
    c
}

/// The columns and the gaps between them.
pub fn table_width(cols: &[f32; 8]) -> f32 {
    cols.iter().sum::<f32>() + CELL_GAP * (cols.len() - 1) as f32
}
/// The enquiry window opens this far right of and below where it was asked for.
const ENQUIRY_OFFSET_PX: f32 = 16.0;
/// The top bar's fixed widths (polish spec M5): the clock state (`paused`,
/// `8×`) and the pause/resume button.
const CLOCK_STATE_W: f32 = 52.0;
const PAUSE_W: f32 = 64.0;
/// Hide panel / Show panel, one width so the buttons beside it stay put.
const HIDE_PANEL_W: f32 = 82.0;
/// "Release area" and its Cancel share this width, so a double-click on the
/// one never lands on "Yes, release" (polish spec M10); the confirm's own
/// slot keeps Settings and Fit from shifting while it shows.
const RELEASE_W: f32 = 88.0;
const CONFIRM_W: f32 = 96.0;
/// Below this width the buttons wrap onto a row of their own rather than
/// draw over the clock (1000 before Hide panel joined them, polish spec H7).
const BAR_WRAP_W: f32 = 1100.0;
/// The lobby's layout list, wide enough for every name, so Create never moves.
const LAYOUT_COMBO_W: f32 = 180.0;
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
    /// 0: watch; `i + 1`: claim the layout's area `i` (polish spec H2).
    area: usize,
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
    /// Release area was pressed and awaits "Yes, release" (polish spec M10).
    confirm_release: bool,
    settings: Settings,
    /// Where the settings are kept between visits (none in most tests).
    store: Option<Box<dyn SettingsStore>>,
    side_tab: SideTab,
    /// The simplifier's headcode search.
    search: String,
    /// The headcode whose enquiry window is open.
    enquiry: Option<String>,
    /// Where the pointer was when it opened: the window opens beside it
    /// (polish spec M7).
    enquiry_at: Option<egui::Pos2>,
    /// The game drawn last frame; another (or the lobby) forgets the
    /// enquiry, the search and the simplifier lines.
    shown_game: Option<String>,
    /// The simplifier's columns for the layout shown (polish spec H3).
    simplifier_cols: [f32; 8],
    /// The Train column's width for (game, layout generation): it changes only with the layout.
    train_col: Option<((String, u64), f32)>,
    /// The diagram's size last frame, and whether the player has panned or
    /// zoomed since the last fit: an untouched fit follows a resize (polish spec H7).
    fit_size: Option<egui::Vec2>,
    cam_moved: bool,
    /// The side panel is shown (polish spec H7: it can be hidden).
    side_open: bool,
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
            confirm_release: false,
            settings: Settings::default(),
            store: None,
            side_tab: SideTab::default(),
            search: String::new(),
            enquiry: None,
            enquiry_at: None,
            shown_game: None,
            simplifier_cols: SIMPLIFIER_COLUMNS,
            train_col: None,
            fit_size: None,
            cam_moved: false,
            side_open: true,
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
            self.confirm_release = false;
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
            let areas: Vec<Vec<String>> = self.core.layouts().iter().map(|l| l.areas.clone()).collect();
            if layouts.is_empty() {
                ui.label("No layouts yet.");
            } else {
                self.new_game.layout = self.new_game.layout.min(layouts.len() - 1);
                ui.horizontal(|ui| {
                    egui::ComboBox::from_label("Layout").width(LAYOUT_COMBO_W).selected_text(layouts[self.new_game.layout].as_str()).show_ui(ui, |ui| {
                        for (i, name) in layouts.iter().enumerate() {
                            ui.selectable_value(&mut self.new_game.layout, i, name.as_str());
                        }
                    });
                    // Where the creator starts (polish spec H2): an area to signal, or watching.
                    let mine = &areas[self.new_game.layout];
                    self.new_game.area = self.new_game.area.min(mine.len());
                    let shown = |i: usize| if i == 0 { "watch".to_string() } else { mine[i - 1].clone() };
                    ui.label("Signal");
                    egui::ComboBox::from_id_salt("new_game_area").selected_text(shown(self.new_game.area)).show_ui(ui, |ui| {
                        for i in 0..=mine.len() {
                            ui.selectable_value(&mut self.new_game.area, i, shown(i));
                        }
                    });
                    ui.label("Seed");
                    ui.add(egui::TextEdit::singleline(&mut self.new_game.seed).desired_width(90.0).hint_text("random"));
                    ui.label("Start");
                    ui.add(egui::TextEdit::singleline(&mut self.new_game.start).desired_width(70.0).hint_text("HH:MM"));
                    if ui.button("Create").clicked() {
                        let seed = self.new_game.seed.trim().parse().ok();
                        let start = Some(self.new_game.start.trim().to_string()).filter(|s| !s.is_empty());
                        let area = self.new_game.area.checked_sub(1).map(|i| mine[i].clone());
                        self.core.create_game_in(&layouts[self.new_game.layout], seed, start, area.as_deref());
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
        // The panel starts as wide as the simplifier for the longest headcode it
        // shows (polish spec H3, U3); keyed by that width, so it resets when the
        // longest headcode changes (ruling D8) and is otherwise draggable.
        let key = self.core.game().map(|g| (g.id.clone(), g.layout_gen()));
        let train_w = match (&self.train_col, key) {
            (Some((k, w)), Some(key)) if *k == key => *w,
            (_, Some(key)) => {
                let w = self.train_column_w(ui);
                self.train_col = Some((key, w));
                w
            }
            (_, None) => 0.0,
        };
        self.simplifier_cols = simplifier_columns(train_w);
        let side_w = table_width(&self.simplifier_cols) + SIDE_PAD;
        // A tutorial's panel carries the lesson box: it is never hidden there.
        let lesson = self.core.game().is_some_and(|g| g.lesson().is_some());
        if self.side_open || lesson {
            egui::Panel::right(egui::Id::new(("side", side_w.round() as i32)))
                .default_size(side_w)
                .min_size(SIDE_MIN_W)
                .show(ui, |ui| self.side(ui, now));
        }
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
        let me = g.you.clone();
        let mut act: Vec<Box<dyn FnOnce(&mut App)>> = Vec::new();
        let mut settings = self.settings;
        let mut confirm_release = self.confirm_release && holding;
        let mut refit = false;
        let mut side_open = self.side_open;
        let buttons = |ui: &mut Ui, act: &mut Vec<Box<dyn FnOnce(&mut App)>>, settings: &mut Settings, confirm_release: &mut bool, refit: &mut bool, side_open: &mut bool| {
                if ui.button("Leave").clicked() {
                    act.push(Box::new(|a| a.leave()));
                }
                // A tutorial's player keeps the lesson's area. Releasing
                // asks first (polish spec M10).
                if holding && !lesson {
                    if *confirm_release {
                        if ui.add_sized([RELEASE_W, 18.0], egui::Button::new("Cancel")).clicked() {
                            *confirm_release = false;
                        }
                        let yes = egui::Button::new(RichText::new("Yes, release").color(ALARM));
                        if ui.add_sized([CONFIRM_W, 18.0], yes).clicked() {
                            *confirm_release = false;
                            act.push(Box::new(|a| a.release()));
                        }
                    } else {
                        if ui.add_sized([RELEASE_W, 18.0], egui::Button::new("Release area")).clicked() {
                            *confirm_release = true;
                        }
                        ui.add_space(CONFIRM_W + ui.spacing().item_spacing.x);
                    }
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
                // Polish spec H7: the panel can make way for the diagram.
                // Fixed width, so Fit and Settings do not shift with the label.
                let label = if *side_open { "Hide panel" } else { "Show panel" };
                if !lesson && ui.add_sized([HIDE_PANEL_W, 18.0], egui::Button::new(label)).clicked() {
                    *side_open = !*side_open;
                }
                if ui.button("Fit").clicked() {
                    *refit = true;
                }
        };
        let narrow = ui.available_width() < BAR_WRAP_W;
        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).strong());
                if let Some(v) = &view {
                    let clock = ui.label(RichText::new(fmt_hms(v.sim_time)).monospace().size(16.0));
                    mark(ui, &clock, marked("clock"), now);
                    let text = if v.paused { "paused".to_string() } else { format!("{}×", v.speed) };
                    let state = ui.add_sized([CLOCK_STATE_W, 18.0], egui::Label::new(text));
                    if v.paused && v.vote.is_none() {
                        state.on_hover_text(if can_vote {
                            "Paused. Any voter can propose resume."
                        } else {
                            "Paused. The holders can resume it."
                        });
                    }
                    // Only voters get the buttons (owner decision 12).
                    if can_vote {
                        let pause = if v.paused { Proposal::Resume } else { Proposal::Pause };
                        if ui.add_sized([PAUSE_W, 18.0], egui::Button::new(proposal_text(pause))).clicked() {
                            act.push(Box::new(move |a| a.vote(pause)));
                        }
                        for x in [1u8, 2, 4, 8] {
                            if ui.selectable_label(!v.paused && v.speed == x, format!("{x}×")).clicked() {
                                act.push(Box::new(move |a| a.vote(Proposal::Speed { x })));
                            }
                        }
                    }
                    // A tutorial keeps no score (tutorial spec §3).
                    if let Some(score) = v.score.filter(|_| !lesson) {
                        ui.label(format!("Penalty {score}"));
                    }
                }
                if !narrow {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| buttons(ui, &mut act, &mut settings, &mut confirm_release, &mut refit, &mut side_open));
                }
            });
            if narrow {
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        buttons(ui, &mut act, &mut settings, &mut confirm_release, &mut refit, &mut side_open)
                    });
                });
            }
            ui.horizontal_wrapped(|ui| {
                match view.as_ref().and_then(|v| v.vote.as_ref()) {
                    Some(vote) => {
                        ui.label(RichText::new(vote_text(vote)).color(paint::YELLOW));
                        // Polish spec M8: say yes or no explicitly.
                        if can_vote {
                            let p = vote.proposal;
                            if !vote.agreed.contains(&me) && ui.button("Agree").clicked() {
                                act.push(Box::new(move |a| a.agree_vote(p)));
                            }
                            let no = if vote.agreed.contains(&me) { "Withdraw" } else { "Decline" };
                            if ui.button(no).clicked() {
                                act.push(Box::new(|a| a.decline_vote()));
                            }
                        }
                    }
                    // Polish spec H2: a spectator's clicks do nothing; say so where they look.
                    None if !holding && !lesson => {
                        let free = areas.iter().any(|a| view.as_ref().and_then(|v| v.holders.get(a)).is_none_or(|h| h == "robot"));
                        let hint = if free { "You are watching. Claim an area to signal:" } else { "All areas are held; you are watching" };
                        ui.label(RichText::new(hint).color(paint::YELLOW));
                    }
                    None => {}
                }
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
        self.confirm_release = confirm_release;
        self.side_open = side_open;
        if refit {
            self.fitted = None;
        }
        self.set_settings(settings);
        for f in act {
            f(&mut self.core);
        }
    }

    /// How wide the simplifier's Train column must be for the longest
    /// headcode the layout shows, as displayed (the same set as the berth
    /// boxes' width, `Scene::build`), in the monospace font.
    fn train_column_w(&self, ui: &Ui) -> f32 {
        let Some(l) = self.core.game().and_then(|g| g.layout()) else { return 0.0 };
        let rows = l.simplifier.iter().map(|r| shown_headcode(&l.display_headcodes, &r.headcode));
        let Some(longest) = rows.chain(l.display_headcodes.values().map(String::as_str)).max_by_key(|h| h.chars().count()) else {
            return 0.0;
        };
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        ui.fonts_mut(|f| f.layout_no_wrap(longest.to_string(), font, Color32::WHITE).size().x) + 2.0
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

    /// The train list (polish spec M6): headed columns, the next call's
    /// booked arrival and departure, lateness as the simplifier shows it
    /// (`OT`, `3L`; blank while due), and a line when it is empty.
    fn trains_ui(&mut self, ui: &mut Ui, height: f32) {
        let Some(g) = self.core.game() else { return };
        let enquiry = self.settings.enquiry;
        let mut open = None;
        egui::ScrollArea::vertical().id_salt("trains").max_height(height).show(ui, |ui| {
            let Some(v) = g.view() else { return };
            let rows = train_list(v);
            if rows.is_empty() {
                ui.label(RichText::new("No trains here or due in the next 30 minutes").color(paint::LABEL));
                return;
            }
            egui::Grid::new("train_list").striped(true).show(ui, |ui| {
                for h in ["Train", "State", "Next", "Arr", "Dep", "Late"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for (h, r) in rows {
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
                    // Codes in the table; the place's name on hover (polish spec M2).
                    let place = r.next_place.as_deref().map(|p| g.names().place(p).to_string());
                    let cell = ui.label(next);
                    if let Some(name) = place {
                        cell.on_hover_text(name);
                    }
                    ui.label(r.arr.map(simplifier::fmt_wtt).unwrap_or_default());
                    ui.label(r.dep.map(simplifier::fmt_wtt).unwrap_or_default());
                    let late = if r.state == TrainState::Due { String::new() } else { simplifier::late_text(r.late_s) };
                    ui.label(RichText::new(&late).color(if late == "OT" { paint::LABEL } else { ALARM }));
                    ui.end_row();
                }
            });
        });
        if open.is_some() {
            self.enquiry = open;
            self.enquiry_at = ui.ctx().pointer_interact_pos();
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
        let cols = self.simplifier_cols;
        let names = g.names();
        // Narrower than its columns, the table scrolls sideways, header and all.
        egui::ScrollArea::horizontal().id_salt("simplifier_wide").show(ui, |ui| {
            simplifier_row(ui, row_h, header.map(|h| RichText::new(h).strong()), &cols, Default::default());
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
                    // Every cell's whole text on hover, places by name (polish spec H3, M2).
                    let place = |p: &str| if p.is_empty() { String::new() } else { names.place(p).to_string() };
                    let hovers = [
                        names.headcode(&line.headcode).to_string(),
                        String::new(),
                        place(&line.from),
                        place(&line.to),
                        place(&line.place),
                        line.platform.clone(),
                        String::new(),
                        String::new(),
                    ];
                    simplifier_row(ui, row_h, cells, &cols, hovers);
                }
            });
        });
    }

    /// The headcode enquiry (realism spec §3; polish spec M7): opened beside
    /// where it was asked for, its facts in a labelled grid, then the
    /// timetable rows with headed columns.
    fn enquiry_window(&mut self, ui: &mut Ui) {
        let Some(h) = self.enquiry.clone() else { return };
        let mut open = true;
        let Some(g) = self.core.game() else { return };
        let (Some(l), v) = (g.layout(), g.view()) else { return };
        let e = simplifier::enquiry(l, v, &h);
        let names = g.names();
        let place = |p: Option<&str>| p.map_or("?", |p| names.place(p)).to_string();
        let mut w = egui::Window::new(format!("Train {}", names.headcode(&h))).id(egui::Id::new("enquiry")).open(&mut open).resizable(false).drag_area(egui::WindowDrag::Anywhere);
        // One-shot per click: positions the window for this frame, after which
        // it can be dragged (egui keeps a closed window's place otherwise).
        // `current_pos` is lost under egui's title-bar-only drag mode (it
        // restores the pre-frame position), hence drag from anywhere.
        if let Some(at) = self.enquiry_at.take() {
            w = w.current_pos(at + vec2(ENQUIRY_OFFSET_PX, ENQUIRY_OFFSET_PX));
        }
        w.show(ui.ctx(), |ui| {
            egui::Grid::new("enquiry_facts").num_columns(2).show(ui, |ui| {
                ui.label(RichText::new("State").strong());
                ui.label(e.live_text());
                ui.end_row();
                if let Some(next) = e.next_text(names) {
                    ui.label(RichText::new("Next").strong());
                    ui.label(next);
                    ui.end_row();
                }
                for r in &e.rows {
                    ui.label(RichText::new("Runs").strong());
                    ui.label(format!("{} to {}", place(r.origin.as_deref()), place(r.destination.as_deref())));
                    ui.end_row();
                }
            });
            if e.rows.is_empty() {
                ui.label("Not in the simplifier for this area");
            }
            for (i, r) in e.rows.iter().enumerate() {
                ui.separator();
                egui::Grid::new(("enquiry_calls", i)).striped(true).show(ui, |ui| {
                    for head in ["Place", "Plat", "Arr", "Dep"] {
                        ui.label(RichText::new(head).strong());
                    }
                    ui.end_row();
                    for line in simplifier::lines(r) {
                        ui.label(names.place(&line.place));
                        ui.label(&line.platform);
                        ui.label(&line.arr);
                        ui.label(&line.dep);
                        ui.end_row();
                    }
                });
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
        // Fit again for a new game or area, after Fit, and when the diagram
        // changes size while the player has not moved the view (polish spec H7).
        let resized = self.fit_size.is_some_and(|s| (s - rect.size()).length() > 0.5);
        if self.fitted.as_ref() != Some(&fit_key) || self.cam.is_none() || (resized && !self.cam_moved) {
            self.cam = Some(scene.fit_bounds().map_or(Camera { centre: rect.center(), scale: 1.0 }, |b| Camera::fit(b, rect.shrink2(vec2(0.0, VIEW_BAND)))));
            self.fitted = Some(fit_key);
            self.cam_moved = false;
        }
        self.fit_size = Some(rect.size());
        let Some(cam) = self.cam.as_mut() else { return };
        // A press held still (a slow click) is a drag to egui but moves nothing.
        let drag = resp.drag_delta();
        if resp.dragged_by(PointerButton::Primary) && drag != vec2(0.0, 0.0) {
            cam.pan(drag);
            self.cam_moved = true;
        }
        // Buttons and keys zoom about the middle (polish spec M11); not while
        // typing in the simplifier's search.
        let keys = if ui.ctx().egui_wants_keyboard_input() {
            0
        } else {
            // Ctrl/Cmd + and - are the browser's page zoom, not ours.
            ui.input(|i| {
                let plain = !i.modifiers.command && !i.modifiers.ctrl && !i.modifiers.alt;
                i32::from(plain && (i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals)))
                    - i32::from(plain && i.key_pressed(Key::Minus))
            })
        };
        if keys != 0 {
            cam.zoom_at(rect, rect.center(), ZOOM_STEP.powi(keys));
            self.cam_moved = true;
        }
        // The zoom buttons' places; under them the diagram is not hovered.
        let corner = |k: f32| rect.right_top() + vec2(-(ZOOM_INSET + ZOOM_BUTTON) * k, ZOOM_INSET);
        let plus_rect = Rect::from_min_size(corner(2.0) - vec2(4.0, 0.0), vec2(ZOOM_BUTTON, ZOOM_BUTTON));
        let minus_rect = Rect::from_min_size(corner(1.0), vec2(ZOOM_BUTTON, ZOOM_BUTTON));
        let hover_pos = resp.hover_pos().filter(|p| !plus_rect.contains(*p) && !minus_rect.contains(*p));
        if let Some(p) = hover_pos {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            if scroll != 0.0 {
                cam.zoom_at(rect, p, (scroll * ZOOM_PER_POINT).exp());
                self.cam_moved = true;
            }
            if zoom != 1.0 {
                cam.zoom_at(rect, p, zoom);
                self.cam_moved = true;
            }
        }
        let cam = *cam;
        let view = self.core.game().and_then(|g| g.view());
        let hit_at = |p: Option<egui::Pos2>| p.and_then(|p| hit_test(scene, view, &cam, rect, p));
        let hover = hit_at(hover_pos);
        // Every click goes on, even one on nothing or on what is not yours:
        // a dead click clears the entrance (`App::click` decides what the
        // rest mean, from the same operability `Hit::clickable` shows). The
        // exception is a left click on points you work: it opens their menu
        // and leaves the chosen entrance alone, as a right click does.
        let click = resp.clicked().then(|| hit_at(resp.interact_pointer_pos()));
        // Points you can work open their menu on a left click too, and never
        // swing on it (polish spec M3, decision U9).
        let points_menu = matches!(&click, Some(Some(h)) if h.clickable && matches!(h.target, Target::Points(_)));
        let click = if points_menu { None } else { click.map(|h| h.map(|h| h.target)) };
        if resp.secondary_clicked() || points_menu {
            self.menu_target = hit_at(resp.interact_pointer_pos()).map(|h| h.target);
        }
        // What can be clicked shows a pointing hand (polish spec M3).
        if hover.as_ref().is_some_and(|h| h.clickable) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let exits = self.core.valid_exits();
        let highlight = self.highlights();
        let Some(g) = self.core.game() else { return };
        let st = PaintState {
            view: g.view(),
            selected: g.selected(),
            exits: &exits,
            refused: g.refused(),
            blocking: g.blocking(),
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
        if !self.cam_moved {
            painter.text(rect.left_bottom() + vec2(ZOOM_INSET, -ZOOM_INSET), Align2::LEFT_BOTTOM, VIEW_HINT, FontId::proportional(12.0), paint::LABEL);
        }
        // The zoom buttons, on top of the diagram (polish spec M11).
        let plus = ui.put(plus_rect, egui::Button::new("+"));
        let minus = ui.put(minus_rect, egui::Button::new("-"));
        let steps = i32::from(plus.clicked()) - i32::from(minus.clicked());
        if let (Some(c), true) = (self.cam.as_mut(), steps != 0) {
            c.zoom_at(rect, rect.center(), ZOOM_STEP.powi(steps));
            self.cam_moved = true;
        }
        match click {
            // With the enquiry on, a headcode opens its window and nothing else.
            Some(Some(t)) => match self.core.headcode_at(&t).filter(|_| self.settings.enquiry) {
                Some(h) => {
                    self.enquiry = Some(h);
                    self.enquiry_at = ui.ctx().pointer_interact_pos();
                }
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
            Some(h) => {
                let text = match self.core.hint(&h.target) {
                    Some(hint) => format!("{}\n{hint}", self.core.describe(&h.target)),
                    None => self.core.describe(&h.target),
                };
                resp.on_hover_text_at_pointer(text)
            }
            None => resp,
        };
        let open = if resp.secondary_clicked() || points_menu {
            Some(egui::SetOpenCommand::Bool(true))
        } else if resp.clicked() {
            Some(egui::SetOpenCommand::Bool(false))
        } else {
            None
        };
        egui::Popup::menu(&resp).open_memory(open).at_pointer_fixed().show(|ui| self.menu_ui(ui));
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

/// One simplifier line in the cells `cols` give, each with its hover text
/// (none where empty).
fn simplifier_row(ui: &mut Ui, row_h: f32, cells: [RichText; 8], cols: &[f32; 8], hovers: [String; 8]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = CELL_GAP;
        for ((text, w), hover) in cells.into_iter().zip(*cols).zip(hovers) {
            ui.allocate_ui_with_layout(vec2(w, row_h), Layout::left_to_right(Align::Center), |ui| {
                ui.set_min_width(w);
                let r = ui.add(egui::Label::new(text).truncate());
                if !hover.is_empty() {
                    r.on_hover_text(hover);
                }
            });
        }
    });
}
