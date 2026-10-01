//! The screens, run headless: egui frames with synthetic input, the app
//! talking to an in-process twobox game through a `MemTransport`.

mod common;

use client_core::{App, AspectMode, MemHandle, MemStore, MemTransport};
use client_ui::UiApp;
use common::*;
use egui::{Event, FullOutput, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect, Shape, TouchPhase, pos2, vec2};
use game::{Game, GameMeta};
use protocol::*;

struct Rig {
    ctx: egui::Context,
    ui: UiApp,
    h: MemHandle,
    game: Game,
    t: f64,
    events: Vec<Event>,
    /// Lobby frames the app sent (game frames go to the game).
    lobby_sent: Vec<LobbyMsg>,
    /// Every command the app sent to the game.
    commands: Vec<PlayerCommand>,
}

impl Rig {
    /// Open, in the lobby, the front's lists delivered.
    fn lobby(world: signalbox_core::world::World) -> Rig {
        Rig::lobby_with(world, None)
    }

    /// `lobby`, the settings kept in `store`.
    fn lobby_with(world: signalbox_core::world::World, store: Option<MemStore>) -> Rig {
        let (tr, h) = MemTransport::new();
        let core = App::new(Box::new(tr), 0.0);
        h.open();
        let game = Game::new(world, GameMeta { layout: s("twobox"), seed: 1 });
        let ui = match store {
            Some(st) => UiApp::with_store(core, Box::new(st)),
            None => UiApp::new(core),
        };
        let mut r = Rig { ctx: egui::Context::default(), ui, h, game, t: 0.0, events: vec![], lobby_sent: vec![], commands: vec![] };
        r.frame();
        r.lobby_sent.clear();
        r.h.push(ServerFrame::Lobby(LobbyReply::Layouts { layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West"), s("East")] }] }));
        r.frame();
        r
    }

    /// In the game as "ann", holding `area`.
    fn in_game(world: signalbox_core::world::World, area: Option<&str>) -> Rig {
        Rig::in_game_with(world, area, None)
    }

    fn in_game_with(world: signalbox_core::world::World, area: Option<&str>, store: Option<MemStore>) -> Rig {
        let mut r = Rig::lobby_with(world, store);
        r.h.push(ServerFrame::Lobby(LobbyReply::Joined { game: s("g-test"), you: s("ann") }));
        for (_, m) in r.game.connect("ann") {
            r.h.push(ServerFrame::Game(m));
        }
        if let Some(a) = area {
            for (_, m) in r.game.handle("ann", ClientMsg::Claim { area: s(a) }) {
                r.h.push(ServerFrame::Game(m));
            }
        }
        r.frame();
        r.frame();
        r
    }

    /// One frame of 0.1 s; the game runs alongside and answers.
    fn frame(&mut self) -> FullOutput {
        self.t += 0.1;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
            time: Some(self.t),
            events: std::mem::take(&mut self.events),
            ..RawInput::default()
        };
        let mut out = self.ctx.run_ui(input, |ui| self.ui.ui(ui));
        out.textures_delta.clear(); // no GPU to upload the font atlas to
        for f in self.h.take_sent() {
            match f {
                ClientFrame::Game(m) => {
                    if let ClientMsg::Command { cmd } = &m {
                        self.commands.push(cmd.clone());
                    }
                    for (p, reply) in self.game.handle("ann", m) {
                        if p == "ann" {
                            self.h.push(ServerFrame::Game(reply));
                        }
                    }
                }
                ClientFrame::Lobby(m) => self.lobby_sent.push(m),
            }
        }
        let mut out_msgs = self.game.advance(0.1);
        out_msgs.extend(self.game.flush());
        for (p, m) in out_msgs {
            if p == "ann" {
                self.h.push(ServerFrame::Game(m));
            }
        }
        out
    }

    fn click(&mut self, at: Pos2, button: PointerButton) {
        self.events.push(Event::PointerMoved(at));
        self.frame();
        self.events.push(Event::PointerButton { pos: at, button, pressed: true, modifiers: Modifiers::default() });
        self.frame();
        self.events.push(Event::PointerButton { pos: at, button, pressed: false, modifiers: Modifiers::default() });
        self.frame();
    }

    /// Where a layout point is on screen now.
    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.ui.camera().unwrap().to_screen(self.ui.diagram_rect().unwrap(), pos2(x, y))
    }

    fn view(&self) -> View {
        self.ui.core.game().unwrap().view().unwrap().clone()
    }
}

/// Every piece of text drawn, with where it was drawn.
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

#[test]
fn the_lobby_lists_games_and_creates_one() {
    let mut r = Rig::lobby(drawn_twobox());
    let info = GameInfo {
        id: s("g-abc"),
        layout: s("twobox"),
        state: GameState::Crashed,
        sim_time: 25_300.0,
        areas: vec![AreaHolder { name: s("West"), holder: Some(s("bob")) }, AreaHolder { name: s("East"), holder: None }],
        players: vec![s("bob")],
        error: Some(s("disk full")),
        creator: Some(s("bob")),
        can_delete: false,
        preparing: None,
    };
    r.h.push(ServerFrame::Lobby(LobbyReply::Games { games: vec![info] }));
    r.frame();
    let out = r.frame();
    for want in ["signalbox", "New game", "twobox", "g-abc", "crashed: disk full", "07:01:40", "West (bob), East (robot)", "Resume"] {
        assert!(has_text(&out, want), "{want} in {:?}", texts(&out));
    }
    let create = texts(&out).into_iter().find(|(t, _)| t == "Create").unwrap().1.center();
    r.click(create, PointerButton::Primary);
    assert_eq!(r.lobby_sent, [LobbyMsg::CreateGame { layout: s("twobox"), seed: None, start: None }]);
}

/// Owner decision 13: Delete only where the front says you may, and only
/// after an in-page confirmation.
#[test]
fn deleting_a_game_asks_first() {
    let mut r = Rig::lobby(drawn_twobox());
    let game = |id: &str, can_delete: bool| GameInfo {
        id: s(id),
        layout: s("twobox"),
        state: GameState::Saved,
        sim_time: 25_200.0,
        areas: vec![],
        players: vec![],
        error: None,
        creator: Some(s("ann")),
        can_delete,
        preparing: None,
    };
    r.h.push(ServerFrame::Lobby(LobbyReply::Games { games: vec![game("g-mine", true), game("g-theirs", false)] }));
    r.frame();
    let out = r.frame();
    let find = |out: &FullOutput, want: &str| texts(out).into_iter().filter(|(t, _)| t == want).map(|(_, at)| at.center()).collect::<Vec<_>>();
    let deletes = find(&out, "Delete");
    assert_eq!(deletes.len(), 1, "only g-mine: {:?}", texts(&out));
    r.click(deletes[0], PointerButton::Primary);
    let out = r.frame();
    assert!(has_text(&out, "Delete for good?"));
    r.click(find(&out, "Cancel")[0], PointerButton::Primary);
    let out = r.frame();
    assert!(!has_text(&out, "Delete for good?") && r.lobby_sent.is_empty(), "cancelled: nothing sent");
    r.click(find(&out, "Delete")[0], PointerButton::Primary);
    let out = r.frame();
    r.click(find(&out, "Yes, delete")[0], PointerButton::Primary);
    assert_eq!(r.lobby_sent, [LobbyMsg::DeleteGame { game: s("g-mine") }]);
}

#[test]
fn the_game_screen_shows_bar_trains_alarms_and_the_fitted_diagram() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let out = r.frame();
    for want in ["g-test · Workstation A · West (ann)", "07:00:0", "1×", "TRAINS", "ALARMS", "West: ann", "East: robot", "1E01", "Leave"] {
        assert!(has_text(&out, want), "{want} in {:?}", texts(&out));
    }
    let rect = r.ui.diagram_rect().unwrap();
    let own = Rect::from_min_max(pos2(0.0, -5.0), pos2(200.0, 5.0));
    assert_eq!(r.ui.camera(), Some(client_ui::camera::Camera::fit(own, rect)), "fitted to West on join");
    let lines = out.shapes.iter().filter(|c| matches!(c.shape, Shape::LineSegment { .. })).count();
    assert!(lines >= 5, "track and points drawn: {lines}");
}

#[test]
fn clicking_entrance_then_exit_sets_a_route() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let w1 = r.at(100.0, -5.0);
    r.click(w1, PointerButton::Primary);
    assert_eq!(r.ui.core.game().unwrap().selected(), Some("W1"));
    let a = r.at(200.0, -5.0);
    r.click(a, PointerButton::Primary);
    for _ in 0..20 {
        r.frame();
    }
    assert!(r.view().routes.contains_key("W1-A"), "{:?}", r.view().routes);
    assert_eq!(r.ui.core.game().unwrap().selected(), None);
}

#[test]
fn esc_clears_the_entrance_and_dragging_pans() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let w1 = r.at(100.0, -5.0);
    r.click(w1, PointerButton::Primary);
    r.events.push(Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::default() });
    r.frame();
    assert_eq!(r.ui.core.game().unwrap().selected(), None);
    let before = r.ui.camera().unwrap().centre;
    let start = r.at(50.0, 40.0);
    r.events.push(Event::PointerMoved(start));
    r.frame();
    r.events.push(Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::default() });
    r.frame();
    for i in 1..=5 {
        r.events.push(Event::PointerMoved(start + vec2(20.0 * i as f32, 0.0)));
        r.frame();
    }
    r.events.push(Event::PointerButton { pos: start + vec2(100.0, 0.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::default() });
    r.frame();
    let after = r.ui.camera().unwrap().centre;
    assert!(after.x < before.x, "dragging right moves the drawing right: {before:?} → {after:?}");
    assert_eq!(r.ui.core.game().unwrap().selected(), None, "a drag is not a click");
}

/// Plan decision 10: a dead click clears the entrance, whether it hits
/// nothing or something that is not yours to work.
#[test]
fn a_click_on_nothing_or_on_what_you_cannot_work_clears_the_entrance() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    // Fitted to West, C is just off the right edge: drag the drawing left.
    let start = r.at(50.0, 40.0);
    r.events.push(Event::PointerMoved(start));
    r.frame();
    r.events.push(Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::default() });
    r.frame();
    for i in 1..=5 {
        r.events.push(Event::PointerMoved(start - vec2(40.0 * i as f32, 0.0)));
        r.frame();
    }
    r.events.push(Event::PointerButton { pos: start - vec2(200.0, 0.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::default() });
    r.frame();
    let w1 = r.at(100.0, -5.0);
    let rect = r.ui.diagram_rect().unwrap();
    // Track TW1 (hover only), empty space, and East's signal C on the fringe.
    for (what, x, y) in [("track", 50.0, 0.0), ("nothing", 50.0, 40.0), ("fringe signal C", 215.0, 5.0)] {
        let at = r.at(x, y);
        assert!(rect.contains(at), "{what} is on screen at {at:?} in {rect:?}");
        r.click(w1, PointerButton::Primary);
        assert_eq!(r.ui.core.game().unwrap().selected(), Some("W1"));
        r.click(at, PointerButton::Primary);
        assert_eq!(r.ui.core.game().unwrap().selected(), None, "a click on {what} clears");
    }
    for _ in 0..10 {
        r.frame();
    }
    assert!(r.view().routes.is_empty(), "nothing was set: {:?}", r.view().routes);
}

#[test]
fn right_click_opens_the_menu_for_what_is_under_the_pointer() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    // BA is in the track 24 px behind A (its signal at x 200, facing right).
    let ba = r.at(200.0, 0.0) - vec2(client_ui::scene::BERTH_BACK_PX, 0.0);
    r.click(ba, PointerButton::Secondary);
    let out = r.frame();
    assert!(has_text(&out, "Berth TAA: empty"), "{:?}", texts(&out));
    assert!(has_text(&out, "Interpose"));
}

/// Owner decision 12: a spectator gets the clock buttons exactly while
/// nobody holds an area.
#[test]
fn a_spectator_votes_only_while_nobody_holds_an_area() {
    let mut r = Rig::in_game(drawn_twobox(), None);
    let out = r.frame();
    let pause = texts(&out).into_iter().find(|(t, _)| t == "pause").expect("every area is the robot's").1.center();
    r.click(pause, PointerButton::Primary);
    for _ in 0..3 {
        r.frame();
    }
    assert!(r.view().paused, "a lone spectator's pause applies at once");
    r.game.connect("bob");
    r.game.handle("bob", ClientMsg::Claim { area: s("East") });
    for _ in 0..3 {
        r.frame();
    }
    let out = r.frame();
    assert!(has_text(&out, "East: bob"), "{:?}", texts(&out));
    assert!(!texts(&out).iter().any(|(t, _)| t == "resume" || t == "2×"), "no clock buttons now: {:?}", texts(&out));
}

#[test]
fn a_layout_without_geometry_says_so() {
    let world = signalbox_core::world::World::from_json(
        &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../game/tests/fixtures/twobox.json")).unwrap(),
    )
    .unwrap();
    let mut r = Rig::in_game(world, Some("West"));
    let out = r.frame();
    assert!(has_text(&out, "No diagram for this layout"), "{:?}", texts(&out));
    assert!(has_text(&out, "TRAINS") && has_text(&out, "1E01"));
}

#[test]
fn a_lost_connection_shows_the_banner() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    r.h.close();
    r.frame();
    let out = r.frame();
    assert!(has_text(&out, "Connection lost. Reconnecting in"), "{:?}", texts(&out));
    assert!(has_text(&out, "g-test · Workstation A · West (ann)"), "the game stays on screen");
}

/// egui's default fonts lack some symbols (an arrow, for one) and draw a box
/// instead: everything the screens show must have a glyph.
#[test]
fn every_character_on_screen_has_a_glyph() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let (w1, a) = (r.at(100.0, -5.0), r.at(200.0, -5.0));
    r.click(w1, PointerButton::Primary);
    r.click(a, PointerButton::Primary);
    r.click(w1, PointerButton::Primary);
    r.click(a, PointerButton::Primary);
    for _ in 0..10 {
        r.frame();
    }
    r.click(w1, PointerButton::Secondary);
    let with_menu = r.frame();
    r.h.close();
    r.frame();
    let with_banner = r.frame();
    let mut shown: String = texts(&with_menu).into_iter().chain(texts(&with_banner)).map(|(t, _)| t).collect();
    // The realism pass: the settings menu, the simplifier (with a half
    // minute) and the enquiry window.
    let half = drawn_twobox_with(|w| w["services"][0]["calls"][0]["arr"] = serde_json::json!("07:04:30"));
    let store = MemStore::new();
    let mut st = store.clone();
    client_core::SettingsStore::save(&mut st, "enquiry=on");
    let mut r2 = Rig::in_game_with(half, Some("East"), Some(store));
    let out = r2.frame();
    click_text(&mut r2, &out, "Settings");
    shown.extend(texts(&r2.frame()).into_iter().map(|(t, _)| t));
    r2.events.push(Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::default() });
    r2.frame();
    let out = r2.frame();
    let right = r2.ui.diagram_rect().unwrap().max.x;
    let at = texts(&out).into_iter().find(|(t, at)| t == "1E01" && at.min.x >= right).unwrap().1.center();
    r2.click(at, PointerButton::Primary);
    let out = r2.frame();
    click_text(&mut r2, &out, "SIMPLIFIER");
    let out = r2.frame();
    shown.extend(texts(&out).into_iter().map(|(t, _)| t));
    assert!(shown.contains("07:04½") && shown.contains("Train 1E01") && shown.contains("Real aspects"), "{shown}");
    assert!(!shown.contains(['→', '←', '○', '●']), "arrows and the auto button are shapes: {shown}");
    assert!(shown.contains("Refused") && shown.contains("Cancel route TAW1 to TAA"), "{shown}");
    // Not reached above: a train with no next call and an open vote show a dash.
    shown.push('—');
    let missing: Vec<char> = r.ctx.fonts_mut(|f| {
        shown
            .chars()
            .filter(|c| !c.is_whitespace())
            .filter(|&c| !f.has_glyph(&egui::FontId::proportional(14.0), c) && !f.has_glyph(&egui::FontId::monospace(14.0), c))
            .collect()
    });
    assert!(missing.is_empty(), "no glyph for {missing:?}");
}

// ---- the realism pass: workstation, settings, simplifier, enquiry ----

/// Text drawn right of the diagram: the side panel.
fn side_texts(r: &Rig, out: &FullOutput) -> Vec<String> {
    let right = r.ui.diagram_rect().unwrap().max.x;
    texts(out).into_iter().filter(|(_, at)| at.min.x >= right).map(|(t, _)| t).collect()
}

fn click_text(r: &mut Rig, out: &FullOutput, want: &str) {
    let at = texts(out).into_iter().find(|(t, _)| t == want).unwrap_or_else(|| panic!("no {want:?} in {:?}", texts(out))).1.center();
    r.click(at, PointerButton::Primary);
}

/// Where berth `name` is drawn now.
fn berth_at(r: &Rig, name: &str) -> Pos2 {
    let g = r.ui.core.game().unwrap();
    let sc = client_ui::scene::Scene::build(g.layout().unwrap()).unwrap();
    let b = sc.berths.iter().find(|b| b.name == name).unwrap();
    client_ui::hit::berth_rect(&r.ui.camera().unwrap(), r.ui.diagram_rect().unwrap(), b.at, b.offset_px).center()
}

/// Frames until 1E01 (entering at W at 07:00) is described in a berth.
fn until_1e01_is_shown(r: &mut Rig) -> String {
    for _ in 0..100 {
        r.frame();
        if let Some((b, _)) = r.view().berths.iter().find(|(_, h)| *h == "1E01") {
            return b.clone();
        }
    }
    panic!("1E01 never described: {:?}", r.view().berths);
}

#[test]
fn the_top_bar_names_the_workstation_or_says_spectating() {
    let mut r = Rig::in_game(drawn_twobox(), Some("East"));
    assert!(has_text(&r.frame(), "g-test · Workstation B · East (ann)"));
    let mut r = Rig::in_game(drawn_twobox(), None);
    assert!(has_text(&r.frame(), "g-test · spectating (ann)"));
}

#[test]
fn settings_change_from_the_menu_and_are_kept() {
    let store = MemStore::new();
    let mut r = Rig::in_game_with(drawn_twobox(), Some("West"), Some(store.clone()));
    assert_eq!(r.ui.settings(), client_core::Settings::default());
    for item in ["Real aspects", "Signal numbers"] {
        let out = r.frame();
        click_text(&mut r, &out, "Settings");
        let out = r.frame();
        click_text(&mut r, &out, item);
    }
    assert_eq!((r.ui.settings().aspects, r.ui.settings().numbers), (AspectMode::Real, false));
    assert_eq!(store.text().as_deref(), Some("aspects=real\nenquiry=off\nnumbers=off\n"));
    let again = Rig::in_game_with(drawn_twobox(), Some("West"), Some(store));
    assert_eq!(again.ui.settings().aspects, AspectMode::Real, "a new page reads them back");
}

#[test]
fn the_simplifier_tab_lists_searches_and_shows_lateness() {
    let mut r = Rig::in_game(drawn_twobox(), None);
    let out = r.frame();
    click_text(&mut r, &out, "SIMPLIFIER");
    let out = r.frame();
    let side = side_texts(&r, &out);
    for want in ["Train", "Late", "Plat", "1E01", "1N02", "2W03", "EST", "07:04", "07:05"] {
        assert!(side.iter().any(|t| t == want), "{want} in {side:?}");
    }
    assert!(has_text(&out, "ALARMS"), "the alarms stay in view");
    until_1e01_is_shown(&mut r);
    let out = r.frame();
    let side = side_texts(&r, &out);
    assert!(side.iter().any(|t| t == "OT"), "1E01 is running, on time: {side:?}");
    let out = r.frame();
    click_text(&mut r, &out, "headcode");
    r.events.push(Event::Text(s("1n")));
    r.frame();
    let out = r.frame();
    let side = side_texts(&r, &out);
    assert!(side.iter().any(|t| t == "1N02") && !side.iter().any(|t| t == "1E01"), "{side:?}");
    r.events.push(Event::Text(s("zz")));
    r.frame();
    assert!(has_text(&r.frame(), "No headcode matches"));
}

#[test]
fn a_headcode_click_opens_the_enquiry_only_when_it_is_on() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let berth = until_1e01_is_shown(&mut r);
    let w1 = r.at(100.0, -5.0);
    r.click(w1, PointerButton::Primary);
    r.click(berth_at(&r, &berth), PointerButton::Primary);
    assert!(!has_text(&r.frame(), "Train 1E01"), "enquiry off: no window");
    assert_eq!(r.ui.core.game().unwrap().selected(), None, "off, it is a dead click as in D1");

    let store = MemStore::new();
    let mut w = store.clone();
    client_core::SettingsStore::save(&mut w, "enquiry=on");
    let mut r = Rig::in_game_with(drawn_twobox(), Some("West"), Some(store));
    let berth = until_1e01_is_shown(&mut r);
    let w1 = r.at(100.0, -5.0);
    r.click(w1, PointerButton::Primary);
    r.click(berth_at(&r, &berth), PointerButton::Primary);
    let out = r.frame();
    assert!(has_text(&out, "Train 1E01") && has_text(&out, "in area, OT"), "{:?}", texts(&out));
    assert!(has_text(&out, "Not in the simplifier for this area"), "West has no platforms");
    assert_eq!(r.ui.enquiry(), Some("1E01"));
    assert_eq!(r.ui.core.game().unwrap().selected(), Some("W1"), "looking a train up never touches the entrance");
}

#[test]
fn a_headcode_in_the_train_list_opens_the_enquiry() {
    let store = MemStore::new();
    let mut w = store.clone();
    client_core::SettingsStore::save(&mut w, "enquiry=on");
    let mut r = Rig::in_game_with(drawn_twobox(), Some("East"), Some(store));
    let out = r.frame();
    let right = r.ui.diagram_rect().unwrap().max.x;
    let at = texts(&out).into_iter().find(|(t, at)| t == "1E01" && at.min.x >= right).expect("1E01 in the train list").1.center();
    r.click(at, PointerButton::Primary);
    let out = r.frame();
    assert!(has_text(&out, "Train 1E01"), "{:?}", texts(&out));
    assert!(has_text(&out, "EST to EST") && has_text(&out, "EST 1 07:04 07:05"), "East's simplifier row");
}

// ---- fix round 1: per-game state, the simplifier's cache and scroll, the enquiry's ways out ----

/// A store with the enquiry already on.
fn enquiry_on() -> MemStore {
    let store = MemStore::new();
    let mut w = store.clone();
    client_core::SettingsStore::save(&mut w, "enquiry=on");
    store
}

/// Click 1E01 in the train list (the TRAINS tab showing).
fn open_1e01_from_the_train_list(r: &mut Rig) {
    let out = r.frame();
    let right = r.ui.diagram_rect().unwrap().max.x;
    let at = texts(&out).into_iter().find(|(t, at)| t == "1E01" && at.min.x >= right).expect("1E01 in the train list").1.center();
    r.click(at, PointerButton::Primary);
}

/// Open the Settings menu and click `item` in it.
fn settings_item(r: &mut Rig, item: &str) {
    let out = r.frame();
    click_text(r, &out, "Settings");
    let out = r.frame();
    click_text(r, &out, item);
}

/// Where the open enquiry window's close button (an X of two diagonal
/// lines in its title bar) is drawn.
fn enquiry_close_button(r: &Rig, out: &FullOutput) -> Pos2 {
    let win = r.ctx.memory(|m| m.area_rect(egui::Id::new("enquiry"))).expect("the enquiry window is open");
    let title = Rect::from_min_max(win.min, pos2(win.max.x, win.min.y + 30.0));
    let ends: Vec<Pos2> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            Shape::LineSegment { points: [a, b], .. } if a.x != b.x && a.y != b.y && title.contains(*a) && title.contains(*b) => {
                Some([*a, *b])
            }
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(ends.len(), 4, "an X: {ends:?}");
    pos2(ends.iter().map(|p| p.x).sum::<f32>() / 4.0, ends.iter().map(|p| p.y).sum::<f32>() / 4.0)
}

#[test]
fn leaving_a_game_forgets_its_enquiry_and_search() {
    let mut r = Rig::in_game_with(drawn_twobox(), Some("East"), Some(enquiry_on()));
    open_1e01_from_the_train_list(&mut r);
    let out = r.frame();
    click_text(&mut r, &out, "SIMPLIFIER");
    let out = r.frame();
    click_text(&mut r, &out, "headcode");
    r.events.push(Event::Text(s("1e")));
    r.frame();
    let out = r.frame();
    assert!(has_text(&out, "Train 1E01") && side_texts(&r, &out).iter().any(|t| t == "1e"), "{:?}", texts(&out));
    click_text(&mut r, &out, "Leave");
    let out = r.frame();
    assert!(has_text(&out, "New game"), "back in the lobby: {:?}", texts(&out));
    r.h.push(ServerFrame::Lobby(LobbyReply::Joined { game: s("g-test"), you: s("ann") }));
    for (_, m) in r.game.connect("ann") {
        r.h.push(ServerFrame::Game(m));
    }
    r.frame();
    r.frame();
    let out = r.frame();
    assert!(r.ui.core.game().is_some(), "joined again");
    assert!(!has_text(&out, "Train 1E01") && r.ui.enquiry().is_none(), "no window without a click: {:?}", texts(&out));
    let side = side_texts(&r, &out);
    assert!(side.iter().any(|t| t == "headcode") && !side.iter().any(|t| t == "1e"), "the search starts empty: {side:?}");
    assert!(side.iter().any(|t| t == "Train"), "the simplifier tab is kept: {side:?}");
}

#[test]
fn the_simplifier_follows_a_new_layout_and_a_changed_search() {
    let mut r = Rig::in_game(drawn_twobox(), None);
    let out = r.frame();
    click_text(&mut r, &out, "SIMPLIFIER");
    let out = r.frame();
    assert!(side_texts(&r, &out).iter().any(|t| t == "1E01"), "a spectator sees every row");
    let before = r.ui.core.game().unwrap().layout_gen();
    for (_, m) in r.game.handle("ann", ClientMsg::Claim { area: s("West") }) {
        r.h.push(ServerFrame::Game(m));
    }
    r.frame();
    let out = r.frame();
    assert!(r.ui.core.game().unwrap().layout_gen() > before, "a new layout came");
    assert!(has_text(&out, "No booked trains here"), "West's simplifier is empty: {:?}", side_texts(&r, &out));
    let mut r = Rig::in_game(drawn_twobox(), None);
    let out = r.frame();
    click_text(&mut r, &out, "SIMPLIFIER");
    let out = r.frame();
    click_text(&mut r, &out, "headcode");
    for (typed, has_1e01, has_1n02) in [("1", true, true), ("n", false, true)] {
        r.events.push(Event::Text(s(typed)));
        r.frame();
        let out = r.frame();
        let side = side_texts(&r, &out);
        assert_eq!((side.iter().any(|t| t == "1E01"), side.iter().any(|t| t == "1N02")), (has_1e01, has_1n02), "{side:?}");
    }
}

/// The side panel is never narrower than the simplifier's columns, so the
/// table never scrolls sideways (where a fixed header would slip off its
/// columns) and switching tabs does not move the diagram.
#[test]
fn the_side_panel_fits_the_simplifier_columns() {
    let mut r = Rig::in_game(drawn_twobox(), None);
    let out = r.frame();
    let diagram = r.ui.diagram_rect().unwrap();
    // 360 points of columns and 7 gaps of 2 points.
    assert!(1280.0 - diagram.max.x >= 374.0, "wide enough from the start: {diagram:?}");
    click_text(&mut r, &out, "SIMPLIFIER");
    let out = r.frame();
    assert_eq!(r.ui.diagram_rect(), Some(diagram), "the diagram stays put");
    let right = diagram.max.x;
    let x_of = |out: &FullOutput, want: &str| {
        texts(out).into_iter().find(|(t, at)| t == want && at.min.x >= right).unwrap_or_else(|| panic!("no {want}")).1
    };
    let (head, row) = (x_of(&out, "Train"), x_of(&out, "1E01"));
    assert_eq!(head.min.x, row.min.x, "the rows sit under their headings");
    assert!(x_of(&out, "Dep").max.x <= 1280.0, "the last column is in view");
    r.events.push(Event::PointerMoved(row.center()));
    r.frame();
    r.events.push(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(-60.0, 0.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::default(),
    });
    for _ in 0..20 {
        r.frame();
    }
    let out = r.frame();
    assert_eq!(x_of(&out, "1E01").min.x, row.min.x, "nothing to scroll sideways");
}

#[test]
fn the_enquiry_toggles_from_the_menu_and_turning_it_off_closes_the_window() {
    let store = MemStore::new();
    let mut r = Rig::in_game_with(drawn_twobox(), Some("East"), Some(store.clone()));
    settings_item(&mut r, "Headcode enquiry");
    assert!(r.ui.settings().enquiry);
    assert_eq!(store.text().as_deref(), Some("aspects=red_green\nenquiry=on\nnumbers=on\n"));
    open_1e01_from_the_train_list(&mut r);
    assert!(has_text(&r.frame(), "Train 1E01"));
    settings_item(&mut r, "Headcode enquiry");
    let out = r.frame();
    assert!(!has_text(&out, "Train 1E01") && r.ui.enquiry().is_none(), "{:?}", texts(&out));
    assert_eq!(store.text().as_deref(), Some("aspects=red_green\nenquiry=off\nnumbers=on\n"));
}

#[test]
fn the_enquiry_window_closes_with_its_cross() {
    let mut r = Rig::in_game_with(drawn_twobox(), Some("East"), Some(enquiry_on()));
    open_1e01_from_the_train_list(&mut r);
    let out = r.frame();
    assert!(has_text(&out, "Train 1E01"));
    let x = enquiry_close_button(&r, &out);
    r.click(x, PointerButton::Primary);
    let out = r.frame();
    assert!(!has_text(&out, "Train 1E01") && r.ui.enquiry().is_none(), "{:?}", texts(&out));
    assert!(r.ui.settings().enquiry, "closing the window leaves the setting on");
}

/// A game being prepared (timetables spec §3.4) says so in the lobby, and
/// its creator, joined but without a layout yet, sees the same words.
#[test]
fn a_game_being_prepared_says_so_in_the_lobby_and_while_waiting() {
    let mut r = Rig::lobby(drawn_twobox());
    let info = GameInfo {
        id: s("g-prep"),
        layout: s("twobox"),
        state: GameState::Running,
        sim_time: 22_000.0,
        areas: vec![],
        players: vec![],
        error: None,
        creator: Some(s("ann")),
        can_delete: false,
        preparing: Some(Preparing { from: 20_400.0, to: 27_000.0 }),
    };
    r.h.push(ServerFrame::Lobby(LobbyReply::Games { games: vec![info] }));
    r.frame();
    let out = r.frame();
    assert!(has_text(&out, "Preparing 05:40 to 07:30…"), "{:?}", texts(&out));
    assert!(!has_text(&out, "running"));
    r.h.push(ServerFrame::Lobby(LobbyReply::Joined { game: s("g-prep"), you: s("ann") }));
    r.frame();
    let out = r.frame();
    assert!(has_text(&out, "Preparing 05:40 to 07:30…"), "{:?}", texts(&out));
    assert!(!has_text(&out, "Waiting for the layout"));
    let missing: Vec<char> = r.ctx.fonts_mut(|f| {
        "Preparing 05:40 to 07:30…".chars().filter(|&c| c != ' ' && !f.has_glyph(&egui::FontId::proportional(16.0), c)).collect()
    });
    assert!(missing.is_empty(), "no glyph for {missing:?}");
}

/// Polish spec §3: the frame the player sees has no text drawn over
/// another, here Liverpool Street box A at Fit (4 overlaps before).
#[test]
fn the_diagram_never_draws_text_over_text() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let read = |p: String| std::fs::read_to_string(p).unwrap();
    let mut w = ts2_import::convert(&read(format!("{dir}/../ts2-import/tests/data/liverpool-st.json"))).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&read(format!("{dir}/../../layouts/liverpool-st.areas.json"))).unwrap()).unwrap();
    ts2_import::lines::apply(&mut w, &ts2_import::lines::parse(&read(format!("{dir}/../../layouts/liverpool-st.lines.json"))).unwrap()).unwrap();
    let mut r = Rig::in_game(signalbox_core::world::World::from_file(w).unwrap(), Some("Liverpool Street"));
    r.frame();
    let out = r.frame();
    let diagram = r.ui.diagram_rect().unwrap();
    let drawn: Vec<(String, Rect)> = texts(&out).into_iter().filter(|(_, at)| diagram.contains(at.center())).collect();
    assert!(drawn.len() > 50, "the box is drawn: {}", drawn.len());
    let mut overlaps = Vec::new();
    for (i, a) in drawn.iter().enumerate() {
        for b in &drawn[i + 1..] {
            let x = a.1.intersect(b.1);
            if x.width() > 0.5 && x.height() > 0.5 {
                overlaps.push((a.0.clone(), b.0.clone()));
            }
        }
    }
    assert!(overlaps.is_empty(), "{overlaps:?}");
}

/// Polish spec §7: half an hour into Drain's TS2 timetable the simplifier
/// opens at the trains still running, not at 06:00's.
#[test]
fn the_simplifier_opens_at_now() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let w = ts2_import::convert(&std::fs::read_to_string(format!("{dir}/../ts2-import/tests/data/drain.json")).unwrap()).unwrap().world;
    let mut r = Rig::in_game(signalbox_core::world::World::from_file(w).unwrap(), None);
    r.game.handle("ann", ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
    for _ in 0..225 {
        r.game.advance(1.0); // 06:00 to 06:30 at 8x
    }
    for (p, m) in r.game.resync("ann") {
        if p == "ann" {
            r.h.push(ServerFrame::Game(m));
        }
    }
    r.frame();
    let out = r.frame();
    click_text(&mut r, &out, "SIMPLIFIER");
    r.frame();
    let out = r.frame();
    let side = side_texts(&r, &out);
    assert!(!side.iter().any(|t| t == "BW01"), "BW01 ran at 06:00: {side:?}");
    assert!(side.iter().any(|t| t == "BW06") && side.iter().any(|t| t == "BW07"), "06:30's trains: {side:?}");
}

/// The layout can arrive before the first view with SIMPLIFIER open: the
/// list still opens at now once the view comes.
#[test]
fn the_simplifier_opens_at_now_when_the_view_follows_the_layout() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let w = ts2_import::convert(&std::fs::read_to_string(format!("{dir}/../ts2-import/tests/data/drain.json")).unwrap()).unwrap().world;
    let mut r = Rig::in_game(signalbox_core::world::World::from_file(w).unwrap(), None);
    let out = r.frame();
    click_text(&mut r, &out, "SIMPLIFIER"); // the tab is kept across a rejoin
    let out = r.frame();
    click_text(&mut r, &out, "Leave");
    let out = r.frame();
    assert!(has_text(&out, "New game"), "back in the lobby: {:?}", texts(&out));
    r.game.handle("ann", ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
    for _ in 0..225 {
        r.game.advance(1.0); // 06:00 to 06:30 at 8x
    }
    r.h.push(ServerFrame::Lobby(LobbyReply::Joined { game: s("g-test"), you: s("ann") }));
    let (layout, rest): (Vec<_>, Vec<_>) = r.game.connect("ann").into_iter().partition(|(_, m)| matches!(m, ServerMsg::Layout(_)));
    assert!(!layout.is_empty(), "connect sends the layout");
    for (_, m) in layout {
        r.h.push(ServerFrame::Game(m));
    }
    r.frame();
    r.frame();
    assert!(r.ui.core.game().is_some_and(|g| g.view().is_none() && g.layout().is_some()), "layout before view");
    for (_, m) in rest {
        r.h.push(ServerFrame::Game(m));
    }
    r.frame();
    let out = r.frame();
    let side = side_texts(&r, &out);
    assert!(!side.iter().any(|t| t == "BW01"), "BW01 ran at 06:00: {side:?}");
    assert!(side.iter().any(|t| t == "BW06"), "06:30's trains: {side:?}");
}

/// Polish spec H2: the lobby offers an area to signal, a spectator is told
/// how to start signalling, and a click on a signal while watching says why
/// nothing happened.
#[test]
fn a_spectator_is_told_to_claim_an_area() {
    let r = Rig::lobby(drawn_twobox());
    let mut r = r;
    let out = r.frame();
    assert!(has_text(&out, "Signal") && has_text(&out, "watch"), "{:?}", texts(&out));
    let mut r = Rig::in_game(drawn_twobox(), None);
    let out = r.frame();
    assert!(has_text(&out, "You are watching. Claim an area to signal:"));
    let w1 = r.at(100.0, 0.0);
    r.click(w1, PointerButton::Primary);
    let out = r.frame();
    assert!(has_text(&out, "You are watching: claim an area to signal"), "{:?}", side_texts(&r, &out));
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    assert!(!has_text(&r.frame(), "You are watching"));
}

/// Fix round 1: with every area held, the spectator is not told to claim.
#[test]
fn a_spectator_is_not_told_to_claim_when_every_area_is_held() {
    let mut r = Rig::in_game(drawn_twobox(), None);
    for (who, area) in [("bob", "West"), ("cat", "East")] {
        r.game.connect(who);
        for (to, m) in r.game.handle(who, ClientMsg::Claim { area: area.to_string() }) {
            if to == "ann" {
                r.h.push(ServerFrame::Game(m));
            }
        }
    }
    r.frame();
    let out = r.frame();
    assert!(has_text(&out, "All areas are held; you are watching"), "{:?}", texts(&out));
    assert!(!has_text(&out, "Claim an area to signal"));
}

/// Polish spec M3 (U9): what you can click shows a pointing hand and says
/// what a click does; points you work open their menu on a left click and
/// are never swung by it.
#[test]
fn clickable_things_say_so_and_points_open_on_a_left_click() {
    let mut r = Rig::in_game(drawn_twobox(), Some("East"));
    let p = r.at(207.5, 0.0);
    r.events.push(Event::PointerMoved(p));
    r.frame();
    let out = r.frame();
    assert_eq!(out.platform_output.cursor_icon, egui::CursorIcon::PointingHand);
    r.click(p, PointerButton::Primary);
    let out = r.frame();
    assert!(has_text(&out, "Swing TBP reverse"), "{:?}", texts(&out));
    // The click only opened the menu: no command was sent. Esc closes it.
    assert!(r.commands.is_empty(), "a left click never swings: {:?}", r.commands);
    r.events.push(Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::default() });
    r.frame();
    assert!(!has_text(&r.frame(), "Swing TBP reverse"), "Esc closes the menu");
    assert!(r.commands.is_empty());
    // A click elsewhere closes it too.
    let mut r = Rig::in_game(drawn_twobox(), Some("East"));
    r.click(p, PointerButton::Primary);
    assert!(has_text(&r.frame(), "Swing TBP reverse"));
    r.click(r.at(150.0, 0.0), PointerButton::Primary);
    assert!(!has_text(&r.frame(), "Swing TBP reverse"), "an outside click closes the menu");
    assert!(r.commands.is_empty());
    // The menu item swings, once.
    let mut r = Rig::in_game(drawn_twobox(), Some("East"));
    r.click(p, PointerButton::Primary);
    let out = r.frame();
    click_text(&mut r, &out, "Swing TBP reverse");
    r.frame();
    assert_eq!(r.commands, [PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse }]);
    // Track is hover only: no hand.
    let mut r = Rig::in_game(drawn_twobox(), Some("East"));
    r.events.push(Event::PointerMoved(r.at(150.0, 0.0)));
    r.frame();
    assert_eq!(r.frame().platform_output.cursor_icon, egui::CursorIcon::Default);
}
