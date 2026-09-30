//! The screens (spec D1 §3): the lobby, and in a game the top bar (game,
//! area, clock, votes, players), the diagram, and the train list and alarms
//! on the right. `UiApp::ui` is the whole frame; the shell calls it.

use std::time::Duration;

use client_core::text::{fmt_hms, proposal_text, vote_text};
use client_core::trains::train_list;
use client_core::{App, Link, Target};
use egui::{Align2, Color32, CornerRadius, FontId, Frame, Key, PointerButton, Rect, RichText, Sense, Ui};
use protocol::{GameState, Proposal, TrainState};

use crate::camera::Camera;
use crate::hit::hit_test;
use crate::paint::{self, BG, PaintState};
use crate::scene::Scene;

/// Alarms and the connection banner.
pub const ALARM: Color32 = Color32::from_rgb(0xFF, 0x5A, 0x5A);
/// How far one wheel "line" (egui points of scroll) zooms.
const ZOOM_PER_POINT: f32 = 1.0 / 200.0;

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
}

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
        }
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
        // Clocks, flashing and reconnect timers move without input.
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }

    fn lobby(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("signalbox");
            if let Some(n) = self.core.lobby_note() {
                ui.label(RichText::new(n).color(ALARM));
            }
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
            egui::Grid::new("games").striped(true).show(ui, |ui| {
                for h in ["Game", "Layout", "State", "Time", "Areas", "Players", ""] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for g in &games {
                    ui.label(&g.id);
                    ui.label(&g.layout);
                    let state = match g.state {
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
                    if ui.button(if g.state == GameState::Running { "Join" } else { "Resume" }).clicked() {
                        join = Some(g.id.clone());
                    }
                    ui.end_row();
                }
            });
            if let Some(id) = join {
                self.core.join(&id);
            }
        });
    }

    fn game(&mut self, ui: &mut Ui, now: f64) {
        self.top_bar(ui);
        egui::Panel::right("side").default_size(330.0).show(ui, |ui| self.side(ui));
        egui::CentralPanel::default().frame(Frame::NONE.fill(BG)).show(ui, |ui| self.diagram_ui(ui, now));
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        let Some(g) = self.core.game() else { return };
        let title = format!("{} · {} ({})", g.id, g.area().unwrap_or("spectating"), g.you);
        let view = g.view().cloned();
        let areas: Vec<String> = g.layout().map(|l| l.areas.clone()).unwrap_or_default();
        let holding = g.area().is_some();
        let can_vote = g.can_vote();
        let mut act: Vec<Box<dyn FnOnce(&mut App)>> = Vec::new();
        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(title).strong());
                if let Some(v) = &view {
                    ui.label(RichText::new(fmt_hms(v.sim_time)).monospace().size(16.0));
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
                    if let Some(score) = v.score {
                        ui.label(format!("Penalty {score}"));
                    }
                }
                if ui.button("Fit").clicked() {
                    self.fitted = None;
                }
                if holding {
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
                    if !holding && holder == "robot" && ui.small_button("Claim").clicked() {
                        let area = area.clone();
                        act.push(Box::new(move |a| a.claim(&area)));
                    }
                }
            });
        });
        for f in act {
            f(&mut self.core);
        }
    }

    fn side(&mut self, ui: &mut Ui) {
        let Some(g) = self.core.game() else { return };
        ui.label(RichText::new("TRAINS").strong());
        egui::ScrollArea::vertical().id_salt("trains").max_height(ui.available_height() * 0.5).show(ui, |ui| {
            let Some(v) = g.view() else { return };
            egui::Grid::new("train_list").striped(true).show(ui, |ui| {
                for (h, r) in train_list(v) {
                    ui.label(RichText::new(h).monospace().color(paint::HEADCODE));
                    ui.label(match r.state {
                        TrainState::AtPlatform => "at platform",
                        TrainState::InArea => "in area",
                        TrainState::Approaching => "approaching",
                        TrainState::Due => "due",
                    });
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
            let msg = if has_layout { "No diagram for this layout" } else { "Waiting for the layout…" };
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
        let hit_at = |p: Option<egui::Pos2>| p.and_then(|p| hit_test(scene, &cam, rect, p));
        let hover = hit_at(resp.hover_pos());
        // Every click goes on, even one on nothing or on what is not yours:
        // a dead click clears the entrance (`App::click` decides what the
        // rest mean, from the same operability `Hit::clickable` shows).
        let click = resp.clicked().then(|| hit_at(resp.interact_pointer_pos()).map(|h| h.target));
        if resp.secondary_clicked() {
            self.menu_target = hit_at(resp.interact_pointer_pos()).map(|h| h.target);
        }
        let exits = self.core.valid_exits();
        let Some(g) = self.core.game() else { return };
        let st = PaintState { view: g.view(), selected: g.selected(), exits: &exits, flashing: g.flashing(), time: now };
        paint::paint(&painter, paint::draw(scene, &cam, rect, &st));
        match click {
            Some(Some(t)) => self.core.click(&t),
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
