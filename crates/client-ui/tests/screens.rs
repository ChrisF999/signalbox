//! The screens, run headless: egui frames with synthetic input, the app
//! talking to an in-process twobox game through a `MemTransport`.

mod common;

use client_core::{App, MemHandle, MemTransport};
use client_ui::UiApp;
use common::*;
use egui::{Event, FullOutput, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape, pos2, vec2};
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
}

impl Rig {
    /// Open, in the lobby, the front's lists delivered.
    fn lobby(world: signalbox_core::world::World) -> Rig {
        let (tr, h) = MemTransport::new();
        let core = App::new(Box::new(tr), 0.0);
        h.open();
        let game = Game::new(world, GameMeta { layout: s("twobox"), seed: 1 });
        let mut r = Rig { ctx: egui::Context::default(), ui: UiApp::new(core), h, game, t: 0.0, events: vec![], lobby_sent: vec![] };
        r.frame();
        r.lobby_sent.clear();
        r.h.push(ServerFrame::Lobby(LobbyReply::Layouts { layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West"), s("East")] }] }));
        r.frame();
        r
    }

    /// In the game as "ann", holding `area`.
    fn in_game(world: signalbox_core::world::World, area: Option<&str>) -> Rig {
        let mut r = Rig::lobby(world);
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
    for want in ["g-test · West (ann)", "07:00:0", "1×", "TRAINS", "ALARMS", "West: ann", "East: robot", "1E01", "Leave"] {
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
    let ba = r.at(190.0, -15.0);
    r.click(ba, PointerButton::Secondary);
    let out = r.frame();
    assert!(has_text(&out, "Berth BA: empty"), "{:?}", texts(&out));
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
    assert!(has_text(&out, "g-test · West (ann)"), "the game stays on screen");
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
