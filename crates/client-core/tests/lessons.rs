//! Tutorials in the client (tutorial spec §3–§4): the lobby's lessons,
//! the lesson state, the buttons, the screen reports and the ticks.

mod common;

use client_core::LessonTicks;
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
