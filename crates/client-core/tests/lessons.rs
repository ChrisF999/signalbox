//! Tutorials in the client (tutorial spec §3–§4): the lobby's lessons,
//! the lesson state, the buttons, the screen reports and the ticks.

mod common;

use client_core::{LessonTicks, Link};
use client_core::lessons::LESSONS_KEY;
use common::*;
use protocol::*;

fn lesson(index: u32, done: bool) -> ServerFrame {
    ServerFrame::Game(ServerMsg::Lesson(LessonView {
        lesson: s("02-setting-routes"),
        title: s("Setting & cancelling routes"),
        index,
        count: 10,
        say: s("Click H3."),
        highlight: vec![Highlight::Signal(s("3"))],
        needs_next: false,
        done,
        alert: None,
    }))
}

fn game_msgs(sent: Vec<ClientFrame>) -> Vec<ClientMsg> {
    sent.into_iter()
        .filter_map(|f| match f {
            ClientFrame::Game(m) => Some(m),
            ClientFrame::Lobby(_) => None,
        })
        .collect()
}

#[test]
fn ticks_keep_lesson_ids_and_drop_anything_else() {
    assert_eq!(LESSONS_KEY, "signalbox.lessons");
    let mut t = LessonTicks::from_text("02-setting-routes\n\n../etc\nLesson\n=\n 01-reading-the-panel \nx y\n");
    assert!(t.done("01-reading-the-panel") && t.done("02-setting-routes"));
    assert_eq!(t.to_text(), "01-reading-the-panel\n02-setting-routes\n");
    assert!(t.insert("04-junctions-and-handovers"));
    assert!(!t.insert("04-junctions-and-handovers"), "already ticked");
    assert!(!t.insert("../x") && !t.insert(""));
    assert_eq!(LessonTicks::from_text(&t.to_text()), t);
    assert_eq!(LessonTicks::from_text("\u{0}\u{ffff}garbage=1"), LessonTicks::default());
}

#[test]
fn the_lobby_keeps_the_lessons_and_starts_one() {
    let (mut app, h) = open_app();
    let info = LessonInfo { id: s("01-reading-the-panel"), title: s("Reading the panel"), steps: 10 };
    h.push(ServerFrame::Lobby(LobbyReply::Lessons { lessons: vec![info.clone()] }));
    app.tick(0.1);
    assert_eq!(app.lessons(), [info]);
    app.start_lesson("01-reading-the-panel");
    assert_eq!(h.take_sent(), [ClientFrame::Lobby(LobbyMsg::StartLesson { lesson: s("01-reading-the-panel") })]);
    h.push(ServerFrame::Lobby(LobbyReply::Joined { game: s("g-tut"), you: s("ann") }));
    h.push(lesson(0, false));
    app.tick(0.2);
    let g = app.game().unwrap();
    assert_eq!((g.id.as_str(), g.lesson().map(|v| v.index)), ("g-tut", Some(0)));
}

/// An app in a tutorial at step `index`, nothing sent yet.
fn in_lesson(index: u32) -> (client_core::App, client_core::MemHandle) {
    let (mut app, h) = open_app();
    h.push(ServerFrame::Lobby(LobbyReply::Joined { game: s("g-tut"), you: s("ann") }));
    h.push(lesson(index, false));
    app.tick(0.1);
    h.take_sent();
    (app, h)
}

#[test]
fn the_lesson_buttons_send_their_messages() {
    let (mut app, h) = in_lesson(0);
    app.lesson_next();
    app.lesson_restart_step();
    app.lesson_restart();
    assert_eq!(game_msgs(h.take_sent()), [ClientMsg::LessonNext, ClientMsg::LessonRestartStep, ClientMsg::LessonRestart]);
}

#[test]
fn the_screen_is_reported_on_change_and_after_every_step() {
    let (mut app, h) = in_lesson(1);
    app.report_screen("trains");
    app.report_screen("trains");
    assert_eq!(game_msgs(h.take_sent()), [ClientMsg::LessonUi { tab: Some(s("trains")), selected: None }], "once");
    app.report_screen("simplifier");
    assert_eq!(game_msgs(h.take_sent()), [ClientMsg::LessonUi { tab: Some(s("simplifier")), selected: None }]);
    h.push(lesson(2, false));
    app.tick(0.2);
    app.report_screen("simplifier");
    assert_eq!(game_msgs(h.take_sent()).len(), 1, "a new step hears the screen again");
    h.push(lesson(10, true));
    app.tick(0.3);
    app.report_screen("trains");
    assert!(h.take_sent().is_empty(), "a finished lesson needs nothing");
}

#[test]
fn outside_a_tutorial_nothing_lesson_is_sent() {
    let mut t = Table::new("ann", Some("West"));
    t.h.take_sent();
    t.app.report_screen("simplifier");
    t.app.lesson_next();
    t.app.lesson_restart_step();
    t.app.lesson_restart();
    assert!(t.h.take_sent().is_empty());
    assert!(t.app.game().unwrap().lesson().is_none());
}

#[test]
fn restarting_the_lesson_drops_the_chosen_entrance_and_reports_afresh() {
    let mut t = Table::new("ann", Some("West"));
    t.h.push(lesson(3, false));
    t.app.tick(0.1);
    t.app.click(&client_core::Target::Signal(s("W1")));
    assert_eq!(t.app.game().unwrap().selected(), Some("W1"));
    t.h.take_sent();
    t.app.report_screen("trains");
    assert_eq!(game_msgs(t.h.take_sent()), [ClientMsg::LessonUi { tab: Some(s("trains")), selected: Some(s("W1")) }]);

    t.app.lesson_restart();
    assert_eq!(t.app.game().unwrap().selected(), None, "the old selection is gone");
    assert_eq!(game_msgs(t.h.take_sent()), [ClientMsg::LessonRestart]);
    t.h.push(lesson(0, false));
    t.app.tick(0.2);
    t.app.report_screen("trains");
    assert_eq!(
        game_msgs(t.h.take_sent()),
        [ClientMsg::LessonUi { tab: Some(s("trains")), selected: None }],
        "the lesson hears the cleared screen, not the old entrance"
    );
}

#[test]
fn restart_step_keeps_the_chosen_entrance() {
    let mut t = Table::new("ann", Some("West"));
    t.h.push(lesson(3, false));
    t.app.tick(0.1);
    t.app.click(&client_core::Target::Signal(s("W1")));
    t.app.report_screen("trains");
    t.h.take_sent();
    t.app.lesson_restart_step();
    assert_eq!(game_msgs(t.h.take_sent()), [ClientMsg::LessonRestartStep]);
    assert_eq!(t.app.game().unwrap().selected(), Some("W1"));
    t.app.report_screen("trains");
    assert!(t.h.take_sent().is_empty(), "the lesson already knows this screen");
}

#[test]
fn leaving_and_joining_a_normal_game_has_no_lesson() {
    let (mut app, h) = in_lesson(2);
    assert!(app.game().unwrap().lesson().is_some());
    app.leave();
    assert!(app.game().is_none());
    h.push(ServerFrame::Lobby(LobbyReply::Joined { game: s("g-normal"), you: s("ann") }));
    app.tick(0.2);
    assert!(app.game().unwrap().lesson().is_none());
    h.take_sent();
    app.report_screen("trains");
    assert!(h.take_sent().is_empty());
}

#[test]
fn no_reports_while_disconnected_or_rejoining_and_the_restart_waits_for_the_link() {
    let mut tb = Table::new("ann", Some("West"));
    tb.h.push(lesson(1, false));
    tb.app.tick(0.1);
    tb.app.click(&client_core::Target::Signal(s("W1")));
    tb.h.take_sent();
    let Table { mut app, h, .. } = tb;
    h.close();
    app.tick(1.0);
    app.report_screen("simplifier");
    app.lesson_restart();
    let g = app.game().unwrap();
    assert_eq!(g.log().len(), 1, "only the restart's own alarm");
    assert_eq!(g.selected(), Some("W1"), "a restart that was not sent keeps the selection");
    assert!(h.take_sent().is_empty());
    let Link::Waiting { retry_at } = app.link() else { panic!("{:?}", app.link()) };
    app.tick(retry_at);
    h.open();
    app.tick(retry_at + 0.1);
    assert_eq!(h.take_sent(), [ClientFrame::Lobby(LobbyMsg::Join { game: s("g-test") })]);
    // The rejoin is not confirmed yet: a screen change says nothing.
    app.report_screen("simplifier");
    assert!(h.take_sent().is_empty(), "no report before the rejoin confirms");
    h.push(ServerFrame::Lobby(LobbyReply::Joined { game: s("g-test"), you: s("ann") }));
    h.push(lesson(1, false));
    app.tick(retry_at + 0.2);
    app.report_screen("simplifier");
    assert_eq!(game_msgs(h.take_sent()), [ClientMsg::LessonUi { tab: Some(s("simplifier")), selected: Some(s("W1")) }]);
}
