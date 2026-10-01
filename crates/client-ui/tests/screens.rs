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
    /// The window, 1280 × 800 unless a test resizes it.
    size: egui::Vec2,
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
        let mut r = Rig { ctx: egui::Context::default(), ui, h, game, t: 0.0, events: vec![], lobby_sent: vec![], size: vec2(1280.0, 800.0), commands: vec![] };
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
        self.frame_at(self.size.x)
    }

    /// A frame in a window `width` points wide.
    fn frame_at(&mut self, width: f32) -> FullOutput {
        self.t += 0.1;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, self.size.y))),
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
    // Polish spec M7: labelled, with what the train does next, and opened
    // beside the click, clear of the top bar's Players row.
    for want in ["State", "Next", "arrive EST 1 at 07:04", "Runs", "EST to EST", "Place", "Plat", "07:04", "07:05"] {
        assert!(has_text(&out, want), "{want} in {:?}", texts(&out));
    }
    let win = r.ctx.memory(|m| m.area_rect(egui::Id::new("enquiry"))).unwrap();
    let players = texts(&out).into_iter().find(|(t, _)| t == "Players:").unwrap().1;
    assert!(win.min.y > players.max.y, "{win:?} below {players:?}");
    assert!((win.min.y - at.y).abs() < 40.0, "{win:?} level with {at:?} (kept on screen sideways)");
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

/// Polish spec M8: a vote shows who it waits for, with Agree and Decline
/// for those who have not agreed, and its end is logged.
#[test]
fn a_vote_waits_for_named_players_who_agree_or_decline() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    r.game.connect("bob");
    r.game.handle("bob", ClientMsg::Claim { area: s("East") });
    r.game.handle("bob", ClientMsg::Vote { proposal: Proposal::Speed { x: 2 } });
    for _ in 0..3 {
        r.frame();
    }
    let out = r.frame();
    assert!(has_text(&out, "waiting for ann"), "{:?}", texts(&out));
    click_text(&mut r, &out, "Decline");
    for _ in 0..3 {
        r.frame();
    }
    let out = r.frame();
    assert!(has_text(&out, "Vote declined by ann: 2×"), "{:?}", texts(&out));
    assert!(!has_text(&out, "Agree"));
    // Agree agrees to the open proposal (review M2).
    r.game.handle("bob", ClientMsg::Vote { proposal: Proposal::Speed { x: 4 } });
    for _ in 0..3 {
        r.frame();
    }
    let out = r.frame();
    click_text(&mut r, &out, "Agree");
    for _ in 0..3 {
        r.frame();
    }
    let out = r.frame();
    assert!(has_text(&out, "Vote passed: 4×"), "{:?}", texts(&out));
    assert_eq!(r.game.clock().speed, 4);
}

fn text_at(out: &FullOutput, want: &str) -> Rect {
    texts(out).into_iter().find(|(t, _)| t == want).unwrap_or_else(|| panic!("no {want:?} in {:?}", texts(out))).1
}

/// Polish spec M5: the bar's buttons stay put while a vote opens and the clock pauses; the pause button keeps its place.
#[test]
fn the_top_bar_does_not_move_under_the_pointer() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    r.game.connect("bob");
    r.game.handle("bob", ClientMsg::Claim { area: s("East") });
    for _ in 0..3 {
        r.frame();
    }
    let out = r.frame();
    let (leave, fit, pause) = (text_at(&out, "Leave"), text_at(&out, "Fit"), text_at(&out, "pause"));
    click_text(&mut r, &out, "pause");
    for _ in 0..3 {
        r.frame();
    }
    let out = r.frame();
    assert!(has_text(&out, "waiting for bob"), "{:?}", texts(&out));
    assert_eq!((text_at(&out, "Leave"), text_at(&out, "Fit")), (leave, fit), "a vote opened");
    assert!(text_at(&out, "Vote: pause — waiting for bob, 30 s left").min.y > leave.max.y, "on the second row");
    r.game.handle("bob", ClientMsg::Vote { proposal: Proposal::Pause });
    for _ in 0..3 {
        r.frame();
    }
    let out = r.frame();
    assert_eq!((text_at(&out, "Leave"), text_at(&out, "Fit")), (leave, fit), "paused");
    assert!((text_at(&out, "resume").center().x - pause.center().x).abs() < 1.0, "the same button, the same place");
}

/// Polish spec M10: Release area asks first.
#[test]
fn releasing_an_area_asks_first() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let out = r.frame();
    click_text(&mut r, &out, "Release area");
    let out = r.frame();
    assert!(r.ui.core.game().unwrap().area().is_some(), "not yet");
    click_text(&mut r, &out, "Cancel");
    let out = r.frame();
    click_text(&mut r, &out, "Release area");
    let out = r.frame();
    click_text(&mut r, &out, "Yes, release");
    for _ in 0..3 {
        r.frame();
    }
    assert_eq!(r.ui.core.game().unwrap().area(), None);
}

/// Review of task 13: a double-click on "Release area" must not reach
/// "Yes, release", and the buttons beside it stay put while it asks.
#[test]
fn the_confirm_never_lies_under_the_release_button() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let out = r.frame();
    let (release, fit, settings) = (text_at(&out, "Release area"), text_at(&out, "Fit"), text_at(&out, "Settings"));
    click_text(&mut r, &out, "Release area");
    let out = r.frame();
    let (yes, cancel) = (text_at(&out, "Yes, release"), text_at(&out, "Cancel"));
    assert!(!yes.expand(8.0).intersects(release.expand(8.0)), "{yes:?} over {release:?}");
    assert!((cancel.center().x - release.center().x).abs() < 1.0, "Cancel takes the slot");
    assert_eq!((text_at(&out, "Fit"), text_at(&out, "Settings")), (fit, settings), "nothing shifts");
}

/// Leaving with the confirm showing does not leave it for the next game.
#[test]
fn a_pending_release_confirm_does_not_follow_to_the_next_game() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let out = r.frame();
    click_text(&mut r, &out, "Release area");
    let out = r.frame();
    click_text(&mut r, &out, "Leave");
    for _ in 0..3 {
        r.frame();
    }
    r.h.push(ServerFrame::Lobby(LobbyReply::Joined { game: s("g-two"), you: s("ann") }));
    for (_, m) in r.game.connect("ann") {
        r.h.push(ServerFrame::Game(m));
    }
    for _ in 0..3 {
        r.frame();
    }
    r.game.handle("ann", ClientMsg::Claim { area: s("West") });
    for _ in 0..3 {
        r.frame();
    }
    let out = r.frame();
    assert!(has_text(&out, "Release area"), "{:?}", texts(&out));
    assert!(!has_text(&out, "Yes, release"));
}

/// Narrow windows (1024 and 800 pt): the buttons wrap below instead of drawing over the clock.
#[test]
fn a_narrow_top_bar_wraps_its_buttons_clear_of_the_clock() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    for width in [1024.0, 800.0] {
        r.frame_at(width);
        let out = r.frame_at(width);
        let clock = texts(&out).into_iter().find(|(t, _)| t.contains(':') && t.len() == 8).expect("clock").1;
        for b in ["Fit", "Hide panel", "Settings", "Release area", "Leave"] {
            let at = text_at(&out, b);
            for left in ["Penalty 0", "pause", "8×"] {
                assert!(!at.intersects(text_at(&out, left)), "{width}: {b} {at:?} over {left}");
            }
            assert!(!at.intersects(clock) && at.min.x >= 0.0 && at.max.x <= width, "{width}: {b} {at:?} vs clock {clock:?}");
        }
    }
}

/// Polish spec M6: the train list has headings, the next call's Arr and
/// Dep, the simplifier's lateness style, and says when it is empty.
#[test]
fn the_train_list_is_headed_and_late_as_the_simplifier_says() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let out = r.frame();
    let side = side_texts(&r, &out);
    for want in ["Train", "State", "Next", "Arr", "Dep", "Late", "07:04", "07:05"] {
        assert!(side.iter().any(|t| t == want), "{want} in {side:?}");
    }
    assert!(side.iter().any(|t| t == "OT"), "1E01 is running, on time: {side:?}");
    let empty = drawn_twobox_with(|w| {
        w["entries"] = serde_json::json!([]);
    });
    let mut r = Rig::in_game(empty, Some("West"));
    assert!(has_text(&r.frame(), "No trains here or due in the next 30 minutes"));
}

/// Polish spec M7, review I1: every open is placed beside its own click, not
/// only the first of the session: after closing, and while another headcode's
/// enquiry is already open.
#[test]
fn each_enquiry_click_places_the_window_beside_it() {
    let mut r = Rig::in_game_with(drawn_twobox(), Some("West"), Some(enquiry_on()));
    let berth = until_1e01_is_shown(&mut r);
    let win = |r: &Rig| r.ctx.memory(|m| m.area_rect(egui::Id::new("enquiry"))).unwrap().min;
    open_1e01_from_the_train_list(&mut r);
    let out = r.frame();
    let x = enquiry_close_button(&r, &out);
    r.click(x, PointerButton::Primary);
    assert!(r.ui.enquiry().is_none());
    r.frame();
    let at = berth_at(&r, &berth);
    r.click(at, PointerButton::Primary);
    r.frame();
    assert_eq!(r.ui.enquiry(), Some("1E01"));
    let p = win(&r);
    assert!((p.y - at.y).abs() < 40.0 && (p.x - at.x).abs() < 40.0, "reopened {p:?} beside {at:?}");
    // Still open: the train list's headcode asks again, elsewhere.
    let before = p;
    open_1e01_from_the_train_list(&mut r);
    r.frame();
    r.frame();
    assert_ne!(win(&r), before, "a click while open moves the window");
}

fn converted(name: &str) -> signalbox_core::world::World {
    let dir = env!("CARGO_MANIFEST_DIR");
    let read = |p: String| std::fs::read_to_string(p).unwrap();
    let mut w = ts2_import::convert(&read(format!("{dir}/../ts2-import/tests/data/{name}.json"))).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&read(format!("{dir}/../../layouts/{name}.areas.json"))).unwrap()).unwrap();
    signalbox_core::world::World::from_file(w).unwrap()
}

/// Polish spec H3: Gretz's headcodes (up to 8 characters) fit the simplifier's
/// Train column (the panel starts wider for them); none is cut short.
#[test]
fn the_simplifier_fits_the_layouts_longest_headcode() {
    let mut r = Rig::in_game(converted("gretz-armainvilliers"), Some("Gretz"));
    let out = r.frame();
    click_text(&mut r, &out, "SIMPLIFIER");
    r.frame();
    let out = r.frame();
    let side = side_texts(&r, &out);
    // `galley.text()` is the unshortened text: what was cut short is `galley.elided`.
    let right = r.ui.diagram_rect().unwrap().max.x;
    let cut: Vec<String> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            Shape::Text(t) if t.galley.elided && t.pos.x >= right => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect();
    let l = r.ui.core.game().unwrap().layout().unwrap().clone();
    let most = l.simplifier.iter().map(|x| x.headcode.chars().count()).max().unwrap();
    assert_eq!(most, 8, "Gretz's longest, `W118412a`");
    assert!(side.iter().any(|t| t.chars().count() == most && l.simplifier.iter().any(|x| x.headcode == *t)), "{side:?}");
    assert!(cut.iter().all(|t| !l.simplifier.iter().any(|x| x.headcode == *t)), "no headcode cut short: {cut:?}");
    assert!(1280.0 - r.ui.diagram_rect().unwrap().max.x > 398.0, "the panel grew");
}

/// Polish spec H3, M2: a cell's whole text shows on hover, places by name.
#[test]
fn a_simplifier_cell_shows_its_whole_text_on_hover() {
    let mut r = Rig::in_game(converted("gretz-armainvilliers"), Some("Gretz"));
    let out = r.frame();
    click_text(&mut r, &out, "SIMPLIFIER");
    r.frame();
    let out = r.frame();
    let right = r.ui.diagram_rect().unwrap().max.x;
    let l = r.ui.core.game().unwrap().layout().unwrap().clone();
    let names = r.ui.core.game().unwrap().names();
    // A drawn cell whose text is a place code with a name different from the code.
    let (code, at) = texts(&out)
        .into_iter()
        .filter(|(t, at)| at.min.x >= right && l.places.contains_key(t) && names.place(t) != t)
        .map(|(t, at)| (t, at.center()))
        .next()
        .expect("a place cell");
    let name = names.place(&code).to_string();
    r.events.push(Event::PointerMoved(at));
    let mut seen = false;
    for _ in 0..10 {
        seen |= has_text(&r.frame(), &name);
    }
    assert!(seen, "hover over {code} shows {name}");
}

/// Polish spec H7: the side panel can be hidden and dragged narrower, and
/// an untouched Fit follows the window's size; a view the player has moved
/// is left alone.
#[test]
fn the_panel_hides_and_the_fit_follows_the_window() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    r.size = vec2(1024.0, 700.0);
    r.frame();
    r.frame();
    let narrow = (r.ui.diagram_rect().unwrap(), r.ui.camera().unwrap());
    let out = r.frame();
    click_text(&mut r, &out, "Hide panel");
    r.frame();
    let out = r.frame();
    let wide = r.ui.diagram_rect().unwrap();
    assert!(wide.width() > narrow.0.width() + 200.0, "{wide:?} vs {:?}", narrow.0);
    assert!(r.ui.camera().unwrap().scale > narrow.1.scale, "fitted again, larger");
    assert!(!has_text(&out, "TRAINS"), "the panel is gone");
    click_text(&mut r, &out, "Show panel");
    // A moved view stays where the player put it.
    let start = r.at(150.0, 0.0);
    r.events.push(Event::PointerMoved(start));
    r.events.push(Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::default() });
    r.frame();
    r.events.push(Event::PointerMoved(start + vec2(40.0, 0.0)));
    r.frame();
    r.events.push(Event::PointerButton { pos: start + vec2(40.0, 0.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::default() });
    r.frame();
    let moved = r.ui.camera().unwrap();
    r.size = vec2(1280.0, 800.0);
    r.frame();
    r.frame();
    assert_eq!(r.ui.camera().unwrap(), moved, "not refitted after a pan");
}

/// Polish spec H7, U3: the panel drags down to 240 pt, no further, and the
/// simplifier (header with it) then scrolls sideways instead of being cut.
#[test]
fn a_narrow_panel_scrolls_the_simplifier_sideways() {
    let mut r = Rig::in_game(converted("gretz-armainvilliers"), Some("Gretz"));
    r.frame();
    let out = r.frame();
    click_text(&mut r, &out, "SIMPLIFIER");
    r.frame();
    let edge = r.ui.diagram_rect().unwrap().max.x;
    let from = pos2(edge + 1.0, 400.0);
    r.events.push(Event::PointerMoved(from));
    r.frame();
    r.events.push(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::default() });
    r.frame();
    for dx in [200.0, 600.0] {
        r.events.push(Event::PointerMoved(from + vec2(dx, 0.0)));
        r.frame();
    }
    r.events.push(Event::PointerButton { pos: from + vec2(600.0, 0.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::default() });
    r.frame();
    let out = r.frame();
    let panel = 1280.0 - r.ui.diagram_rect().unwrap().max.x;
    assert!((239.0..=245.0).contains(&panel), "dragged to the minimum, no further: {panel}");
    // Header and rows share one sideways scroll: Dep starts out of the panel's reach.
    let left = r.ui.diagram_rect().unwrap().max.x;
    assert!(texts(&out).iter().all(|(t, _)| t != "Dep"), "Dep is out of view: {:?}", side_texts(&r, &out));
    let at = pos2(left + panel / 2.0, 400.0);
    r.events.push(Event::PointerMoved(at));
    r.events.push(Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(-400.0, 0.0), modifiers: Modifiers::default(), phase: egui::TouchPhase::Move });
    r.frame();
    let out = r.frame();
    let dep = text_at(&out, "Dep");
    assert!(dep.min.x >= left && dep.max.x <= 1280.0, "Dep scrolled into the panel: {dep:?}");
}

/// Fit clears a moved view: pan, Fit, resize, and the fit follows again.
#[test]
fn fit_after_a_pan_follows_the_window_again() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    r.frame();
    r.frame();
    let start = r.at(150.0, 0.0);
    r.events.push(Event::PointerMoved(start));
    r.events.push(Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::default() });
    r.frame();
    r.events.push(Event::PointerMoved(start + vec2(40.0, 0.0)));
    r.frame();
    r.events.push(Event::PointerButton { pos: start + vec2(40.0, 0.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::default() });
    r.frame();
    let out = r.frame();
    click_text(&mut r, &out, "Fit");
    r.frame();
    let fitted = r.ui.camera().unwrap();
    r.size = vec2(1024.0, 700.0);
    r.frame();
    r.frame();
    assert_ne!(r.ui.camera().unwrap(), fitted, "refitted to the smaller window");
}

/// A press held still past egui's click time is a drag that moves nothing:
/// it does not count as a pan, so the fit still follows the window.
#[test]
fn a_press_held_still_does_not_stop_the_fit_following() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    r.frame();
    r.frame();
    let start = r.at(150.0, 0.0);
    r.events.push(Event::PointerMoved(start));
    r.events.push(Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::default() });
    for _ in 0..12 {
        r.frame();
    }
    r.events.push(Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::default() });
    r.frame();
    let before = r.ui.camera().unwrap();
    r.size = vec2(1024.0, 700.0);
    r.frame();
    r.frame();
    assert_ne!(r.ui.camera().unwrap(), before, "still follows the window");
}

/// Polish spec M11: + and - buttons and keys zoom in steps about the
/// middle, and the diagram says how to move it until the player has.
#[test]
fn the_diagram_zooms_with_buttons_and_keys_and_says_how() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let out = r.frame();
    assert!(has_text(&out, client_ui::screens::VIEW_HINT));
    let fit = r.ui.camera().unwrap().scale;
    click_text(&mut r, &out, "+");
    let zoomed = r.ui.camera().unwrap().scale;
    assert!((zoomed / fit - client_ui::screens::ZOOM_STEP).abs() < 1e-4, "{fit} → {zoomed}");
    r.events.push(Event::Key { key: Key::Minus, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::default() });
    r.frame();
    assert!((r.ui.camera().unwrap().scale - fit).abs() < 1e-3, "back out");
    assert!(!has_text(&r.frame(), client_ui::screens::VIEW_HINT), "moved: the hint goes");
    assert!((client_ui::screens::ZOOM_PER_POINT * 100.0).exp() < 1.2, "a wheel notch is a small step");
}

/// Polish spec M11 with H7: the zoom buttons sit inside the diagram, clear of
/// each other and the top bar, at 1024 pt; zooming counts as moving the view
/// (a resize no longer refits it) and Fit clears that.
#[test]
fn the_zoom_buttons_fit_at_1024_and_count_as_moving_the_view() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    r.size = vec2(1024.0, 700.0);
    r.frame();
    let out = r.frame();
    let diagram = r.ui.diagram_rect().unwrap();
    let (plus, minus) = (text_at(&out, "+"), text_at(&out, "-"));
    assert!(!plus.intersects(minus), "{plus:?} over {minus:?}");
    for b in [plus, minus] {
        assert!(diagram.contains_rect(b), "{b:?} outside {diagram:?}");
    }
    for bar in ["Fit", "Hide panel", "Settings", "Leave", "Release area"] {
        let at = text_at(&out, bar);
        assert!(!at.intersects(plus) && !at.intersects(minus), "{bar} {at:?}");
    }
    let fit = r.ui.camera().unwrap();
    click_text(&mut r, &out, "+");
    let zoomed = r.ui.camera().unwrap();
    assert!(zoomed.scale > fit.scale * 1.2, "the + was pressed: {fit:?} → {zoomed:?}");
    r.size = vec2(900.0, 700.0);
    r.frame();
    let out = r.frame();
    assert_eq!(r.ui.camera().unwrap(), zoomed, "a zoomed view is not refitted on a resize");
    click_text(&mut r, &out, "Fit");
    r.frame();
    let refit = r.ui.camera().unwrap();
    assert!(refit.scale < zoomed.scale, "Fit refits: {zoomed:?} → {refit:?}");
}

fn key(r: &mut Rig, key: Key, modifiers: Modifiers) {
    r.events.push(Event::ModifiersChanged(modifiers));
    r.events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
    r.frame();
    r.events.push(Event::ModifiersChanged(Modifiers::default()));
    r.frame();
}

/// Polish spec M11, review M4: the buttons take their own click (the entrance
/// stays chosen); the keys are not ours while a text field has focus or with
/// Ctrl/Cmd (the browser's page zoom).
#[test]
fn zoom_buttons_and_keys_leave_the_entrance_the_search_and_page_zoom_alone() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let w1 = r.at(100.0, -5.0);
    r.click(w1, PointerButton::Primary);
    assert_eq!(r.ui.core.game().unwrap().selected(), Some("W1"));
    let out = r.frame();
    click_text(&mut r, &out, "+");
    assert_eq!(r.ui.core.game().unwrap().selected(), Some("W1"), "a click on + is not a dead click");
    let before = r.ui.camera().unwrap();
    r.ctx.options_mut(|o| o.zoom_with_keyboard = false); // as eframe's web backend sets it
    let ctrl = Modifiers { ctrl: true, command: true, ..Modifiers::default() };
    key(&mut r, Key::Equals, ctrl);
    assert_eq!(r.ui.camera().unwrap(), before, "Ctrl+= is the browser's");
    key(&mut r, Key::Minus, ctrl);
    assert_eq!(r.ui.camera().unwrap(), before, "and so is Ctrl+-");
    key(&mut r, Key::Equals, Modifiers::SHIFT);
    assert!(r.ui.camera().unwrap().scale > before.scale, "+ is Shift+=");
    let mut r = Rig::in_game(drawn_twobox(), None);
    let out = r.frame();
    click_text(&mut r, &out, "SIMPLIFIER");
    let out = r.frame();
    click_text(&mut r, &out, "headcode");
    r.frame();
    let before = r.ui.camera().unwrap();
    key(&mut r, Key::Equals, Modifiers::default());
    assert_eq!(r.ui.camera().unwrap(), before, "typing in the search does not zoom");
    key(&mut r, Key::Minus, Modifiers::default());
    assert_eq!(r.ui.camera().unwrap(), before, "nor does -");
}

/// Polish spec M11, review M3: over a zoom button the diagram is not hovered,
/// so the wheel there does not zoom about it.
#[test]
fn the_wheel_over_a_zoom_button_does_nothing() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let out = r.frame();
    let at = text_at(&out, "+").center();
    let before = r.ui.camera().unwrap();
    r.events.push(Event::PointerMoved(at));
    r.frame();
    r.events.push(Event::MouseWheel { unit: MouseWheelUnit::Point, delta: vec2(0.0, 60.0), modifiers: Modifiers::default(), phase: TouchPhase::Move });
    for _ in 0..10 {
        r.frame();
    }
    assert_eq!(r.ui.camera().unwrap(), before);
}

/// Polish spec M11, review M2: Fit keeps the button strip and the hint's
/// strip clear, so on a tall layout nothing starts under them.
#[test]
fn fit_keeps_the_zoom_buttons_and_hint_clear_of_the_drawing() {
    let tall = drawn_twobox_with(|w| {
        w["layout"]["lines"][3]["y2"] = serde_json::json!(900.0);
    });
    for world in [tall, converted("gretz-armainvilliers")] {
        let mut r = Rig::in_game(world, None);
        r.frame();
        let rect = r.ui.diagram_rect().unwrap();
        let cam = r.ui.camera().unwrap();
        let g = r.ui.core.game().unwrap();
        let b = client_ui::scene::Scene::build(g.layout().unwrap()).unwrap().fit_bounds().unwrap();
        let (top, bottom) = (cam.to_screen(rect, b.min).y, cam.to_screen(rect, b.max).y);
        let band = client_ui::screens::VIEW_BAND;
        assert!(top >= rect.min.y + band - 0.5 && bottom <= rect.max.y - band + 0.5, "{top} {bottom} in {rect:?}");
    }
}

/// Polish spec M13: Gretz's long areas are not a thin strip at Fit: Fit
/// shows them at a readable scale round their busiest station; short areas
/// and spectators still see everything.
#[test]
fn fit_keeps_a_long_area_readable() {
    use client_ui::scene::FIT_MIN_SCALE;
    let mut r = Rig::in_game(converted("gretz-armainvilliers"), Some("Gretz"));
    r.frame();
    let cam = r.ui.camera().unwrap();
    let sc = client_ui::scene::Scene::build(r.ui.core.game().unwrap().layout().unwrap()).unwrap();
    assert_eq!((cam.scale, Some(cam.centre)), (FIT_MIN_SCALE, sc.focus), "{cam:?}");
    let mut r = Rig::in_game(converted("gretz-armainvilliers"), None);
    r.frame();
    assert!(r.ui.camera().unwrap().scale < FIT_MIN_SCALE, "a spectator sees the whole layout");
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    r.frame();
    assert!(r.ui.camera().unwrap().scale > FIT_MIN_SCALE);
}
