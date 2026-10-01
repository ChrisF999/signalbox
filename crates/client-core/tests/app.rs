//! The connection, the lobby and the game state (spec D1 §2, §5) against
//! an in-memory transport.

mod common;

use std::collections::BTreeMap;

use client_core::app::{FIRST_BACKOFF_S, MAX_BACKOFF_S, backoff_s};
use client_core::log::LOG_CAP;
use client_core::{App, Link, MemTransport};
use common::*;
use protocol::*;

fn lobby(m: LobbyMsg) -> ClientFrame {
    ClientFrame::Lobby(m)
}

fn view(seq: u64) -> ServerFrame {
    ServerFrame::Game(ServerMsg::View(View {
        seq,
        sim_time: 25_200.0,
        speed: 1,
        paused: false,
        vote: None,
        holders: BTreeMap::new(),
        score: None,
        signals: BTreeMap::new(),
        routes: BTreeMap::new(),
        points: BTreeMap::new(),
        sections: BTreeMap::new(),
        berths: BTreeMap::new(),
        trains: BTreeMap::new(),
    }))
}

fn delta(seq: u64) -> ServerFrame {
    ServerFrame::Game(ServerMsg::Delta(Delta { seq, sim_time: Some(25_200.0 + seq as f64), ..Delta::default() }))
}

fn joined(game: &str) -> ServerFrame {
    ServerFrame::Lobby(LobbyReply::Joined { game: s(game), you: s("ann") })
}

fn notice(n: Notice) -> ServerFrame {
    ServerFrame::Game(ServerMsg::Notice(n))
}

/// An open app inside game `g-one` with a first view.
fn in_game() -> (App, client_core::MemHandle) {
    let (mut app, h) = open_app();
    h.push(joined("g-one"));
    h.push(view(1));
    app.tick(1.0);
    (app, h)
}

#[test]
fn it_connects_at_once_and_asks_for_the_lobby_when_open() {
    let (t, h) = MemTransport::new();
    let mut app = App::new(Box::new(t), 0.0);
    assert_eq!((h.connects(), app.link()), (1, Link::Connecting));
    assert_eq!(app.banner().as_deref(), Some("Connecting…"));
    app.tick(0.1);
    assert!(h.take_sent().is_empty(), "nothing is sent before the socket opens");
    h.open();
    app.tick(0.2);
    assert_eq!(app.link(), Link::Open);
    assert_eq!(app.banner(), None);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::ListLayouts), lobby(LobbyMsg::ListLessons), lobby(LobbyMsg::ListGames)]);
}

#[test]
fn the_lobby_lists_games_and_layouts_and_sends_what_you_ask() {
    let (mut app, h) = open_app();
    let info = GameInfo {
        id: s("g-one"),
        layout: s("twobox"),
        state: GameState::Running,
        sim_time: 25_200.0,
        areas: vec![AreaHolder { name: s("West"), holder: None }],
        players: vec![s("bob")],
        error: None,
        creator: Some(s("bob")),
        can_delete: false,
        preparing: None,
    };
    h.push(ServerFrame::Lobby(LobbyReply::Games { games: vec![info.clone()] }));
    h.push(ServerFrame::Lobby(LobbyReply::Layouts { layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West")] }] }));
    app.tick(1.0);
    assert_eq!(app.games(), [info]);
    assert_eq!(app.layouts()[0].name, "twobox");
    app.refresh();
    app.create_game("twobox", Some(5), Some(s("07:30")));
    app.join("g-one");
    app.delete_game("g-old");
    assert_eq!(
        h.take_sent(),
        [
            lobby(LobbyMsg::ListGames),
            lobby(LobbyMsg::CreateGame { layout: s("twobox"), seed: Some(5), start: Some(s("07:30")) }),
            lobby(LobbyMsg::Join { game: s("g-one") }),
            lobby(LobbyMsg::DeleteGame { game: s("g-old") }),
        ]
    );
    h.push(ServerFrame::error(codes::UNKNOWN_LAYOUT, "no layout `x`"));
    app.tick(2.0);
    assert_eq!(app.lobby_note(), Some("no layout `x`"));
}

#[test]
fn joined_puts_you_in_the_game_and_leave_takes_you_out() {
    let (mut app, h) = in_game();
    let g = app.game().unwrap();
    assert_eq!((g.id.as_str(), g.you.as_str(), g.view().unwrap().seq), ("g-one", "ann", 1));
    h.push(delta(2));
    app.tick(2.0);
    assert_eq!(app.game().unwrap().view().unwrap().seq, 2);
    app.claim("West");
    app.vote(Proposal::Speed { x: 4 });
    app.release();
    app.leave();
    assert!(app.game().is_none());
    assert_eq!(
        h.take_sent(),
        [
            ClientFrame::Game(ClientMsg::Claim { area: s("West") }),
            ClientFrame::Game(ClientMsg::Vote { proposal: Proposal::Speed { x: 4 } }),
            ClientFrame::Game(ClientMsg::Release),
            lobby(LobbyMsg::Leave),
        ]
    );
    h.push(view(3));
    app.tick(3.0);
    assert!(app.game().is_none(), "game frames after leaving are dropped");
}

#[test]
fn backoff_doubles_from_half_a_second_to_ten() {
    let waits: Vec<f64> = (0..8).map(backoff_s).collect();
    assert_eq!(waits, [0.5, 1.0, 2.0, 4.0, 8.0, 10.0, 10.0, 10.0]);
    assert_eq!((FIRST_BACKOFF_S, MAX_BACKOFF_S), (0.5, 10.0));
    assert_eq!(backoff_s(u32::MAX), 10.0);
}

/// Spec D1 §5: a lost connection shows a banner, retries with backoff,
/// and on reconnect rejoins with exactly one `join` — its layout and view
/// are the one resync. The game state and log survive meanwhile.
#[test]
fn a_lost_connection_backs_off_and_rejoins_once() {
    let (mut app, h) = in_game();
    h.push(notice(Notice::AreaTaken { area: s("East"), holder: s("bob") }));
    app.tick(1.5);
    h.close();
    app.tick(2.0);
    assert_eq!(app.link(), Link::Waiting { retry_at: 2.5 });
    assert_eq!(app.banner().as_deref(), Some("Connection lost. Reconnecting in 1 s…"));
    let mut t = 2.0;
    for want in [0.5, 1.0, 2.0, 4.0, 8.0, 10.0, 10.0] {
        let Link::Waiting { retry_at } = app.link() else { panic!("{:?}", app.link()) };
        assert_eq!(retry_at - t, want);
        let before = h.connects();
        app.tick(retry_at - 0.01);
        assert_eq!(h.connects(), before, "not before the wait is over");
        t = retry_at;
        app.tick(t);
        assert_eq!((h.connects(), app.link()), (before + 1, Link::Connecting));
        assert_eq!(app.banner().as_deref(), Some("Reconnecting…"));
        h.close();
        app.tick(t);
    }
    assert!(app.game().is_some(), "the game is kept, drawn stale under the banner");
    let Link::Waiting { retry_at } = app.link() else { panic!() };
    t = retry_at;
    app.tick(t);
    h.open();
    app.tick(t + 0.1);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::Join { game: s("g-one") })], "one join, no resync, no lobby lists");
    h.push(joined("g-one"));
    h.push(view(40));
    app.tick(t + 0.2);
    let g = app.game().unwrap();
    assert_eq!((g.view().unwrap().seq, g.resyncs(), g.log().len()), (40, 0, 1));
    h.close();
    app.tick(t + 0.3);
    assert_eq!(app.link(), Link::Waiting { retry_at: t + 0.3 + 0.5 }, "a connection that carried frames resets the backoff");
}

#[test]
fn a_connection_that_opens_and_closes_without_a_frame_keeps_backing_off() {
    let (mut app, h) = open_app();
    h.close();
    app.tick(1.0);
    app.tick(1.5);
    h.open();
    app.tick(1.6);
    h.close();
    app.tick(1.7);
    assert_eq!(app.link(), Link::Waiting { retry_at: 2.7 }, "second wait is 1 s");
}

#[test]
fn a_failed_rejoin_goes_back_to_the_lobby_with_the_reason() {
    let (mut app, h) = in_game();
    h.close();
    app.tick(2.0);
    app.tick(2.5);
    h.open();
    app.tick(2.6);
    h.take_sent();
    h.push(ServerFrame::error(codes::UNKNOWN_GAME, "no game `g-one`"));
    app.tick(2.7);
    assert!(app.game().is_none());
    assert_eq!(app.lobby_note(), Some("Could not rejoin the game: no game `g-one`"));
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::ListGames)]);
    h.close();
    app.tick(3.0);
    app.tick(3.5);
    h.open();
    app.tick(3.6);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::ListLayouts), lobby(LobbyMsg::ListLessons), lobby(LobbyMsg::ListGames)], "no rejoin any more");
}

#[test]
fn an_expired_session_asks_for_a_login_and_stops_retrying() {
    let (t, h) = MemTransport::new();
    let mut app = App::new(Box::new(t), 0.0);
    h.unauthorized();
    app.tick(0.5);
    assert!(app.wants_login());
    assert_eq!(app.banner().as_deref(), Some("Your session has expired. Signing in again…"));
    app.tick(100.0);
    assert_eq!(h.connects(), 1);
}

#[test]
fn replaced_by_another_tab_stays_down_until_asked() {
    let (mut app, h) = in_game();
    h.push(notice(Notice::Replaced));
    app.tick(2.0);
    h.close();
    app.tick(3.0);
    app.tick(60.0);
    assert_eq!((app.link(), h.connects()), (Link::Replaced, 1));
    assert_eq!(app.banner().as_deref(), Some("This login is now open in another tab or window."));
    app.reconnect_now();
    assert_eq!((app.link(), h.connects()), (Link::Connecting, 2));
    h.open();
    app.tick(61.0);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::Join { game: s("g-one") })]);
}

#[test]
fn a_crashed_game_returns_to_the_lobby_with_a_banner() {
    let (mut app, h) = in_game();
    h.push(notice(Notice::GameCrashed));
    h.push(view(2));
    app.tick(2.0);
    assert!(app.game().is_none());
    assert_eq!(app.lobby_note(), Some("The game stopped unexpectedly. Join it again to resume it."));
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::ListGames)]);
}

#[test]
fn an_unreadable_frame_is_logged_and_asks_for_one_resync() {
    let (mut app, h) = in_game();
    h.push_text("{\"type\": \"view\", \"seq\": \"soon\"}");
    h.push_text("not json at all");
    app.tick(2.0);
    assert_eq!(h.take_sent(), [ClientFrame::Game(ClientMsg::Resync)], "one resync for both");
    let g = app.game().unwrap();
    assert_eq!(g.log().len(), 2);
    assert!(g.log().entries().all(|e| e.text.starts_with("Unreadable message from the server")));
    h.push(view(9));
    h.push(delta(11));
    app.tick(3.0);
    assert_eq!(h.take_sent(), [ClientFrame::Game(ClientMsg::Resync)], "a gap after the new view asks again");
}

/// Spec D1 §1 criterion 3: nothing the server sends can panic the client.
#[test]
fn hostile_frames_never_panic() {
    let junk = [
        "",
        "null",
        "[]",
        "{}",
        "{\"type\": 7}",
        "{\"type\": \"layout\"}",
        "{\"type\": \"view\", \"seq\": -1}",
        "{\"type\": \"delta\", \"seq\": 18446744073709551615}",
        "{\"type\": \"delta\", \"seq\": 2, \"signals\": {\"nowhere\": \"red\"}, \"trains\": {\"x\": null}}",
        "{\"type\": \"notice\", \"kind\": \"rejected\"}",
        "{\"type\": \"notice\", \"kind\": \"late\", \"train\": \"1A01\", \"place\": \"X\", \"platform\": \"1\", \"late_s\": -9223372036854775808}",
        "{\"type\": \"joined\", \"game\": \"\", \"you\": \"\"}",
        "{\"type\": \"games\", \"games\": [{\"id\": 1}]}",
        "{\"type\": \"error\"}",
        "\u{0}\u{feff}{",
    ];
    let (mut app, h) = open_app();
    for j in junk {
        h.push_text(j);
    }
    app.tick(1.0);
    let (mut app2, h2) = in_game();
    for j in junk {
        h2.push_text(j);
    }
    app2.tick(2.0);
    h.push(joined("g-two"));
    h.push(view(1));
    app.tick(3.0);
    assert_eq!(app.game().unwrap().view().unwrap().seq, 1, "still works afterwards");
}

#[test]
fn notices_are_logged_with_alarms_and_the_log_is_bounded() {
    let (mut app, h) = in_game();
    h.push(notice(Notice::Spad { signal: s("A"), train: s("1A01") }));
    h.push(notice(Notice::Handover { headcode: s("2W03"), from_area: s("East") }));
    app.tick(2.0);
    let lines: Vec<(String, bool, Option<f64>)> =
        app.game().unwrap().log().entries().map(|e| (e.text.clone(), e.alarm, e.sim_time)).collect();
    assert_eq!(
        lines,
        [
            (s("SPAD: 1A01 passed A at danger"), true, Some(25_200.0)),
            (s("2W03 offered from East"), false, Some(25_200.0)),
        ]
    );
    for i in 0..(LOG_CAP + 50) {
        h.push(notice(Notice::Error { code: s("x"), message: format!("e{i}") }));
    }
    app.tick(3.0);
    let log = app.game().unwrap().log();
    assert_eq!(log.len(), LOG_CAP);
    assert_eq!(log.entries().last().unwrap().text, format!("Error: e{}", LOG_CAP + 49));
}

#[test]
fn commands_without_a_connection_are_not_sent_and_say_so() {
    let (mut app, h) = in_game();
    h.close();
    app.tick(2.0);
    app.command(PlayerCommand::CancelRoute { entrance: s("A") });
    assert!(h.take_sent().is_empty());
    let last = app.game().unwrap().log().entries().last().unwrap().clone();
    assert_eq!((last.text.as_str(), last.alarm), ("Not connected: nothing was sent", true));
}

/// A reload or second tab: the front hands the new socket the old one's
/// game and says `joined` before the client asks for anything.
#[test]
fn an_unasked_joined_enters_the_game() {
    let (mut app, h) = open_app();
    h.push(joined("g-one"));
    h.push(view(7));
    app.tick(1.0);
    assert_eq!(app.game().unwrap().view().unwrap().seq, 7);
    assert!(h.take_sent().is_empty(), "nothing to ask for");
    h.close();
    app.tick(2.0);
    app.tick(2.5);
    h.open();
    app.tick(2.6);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::Join { game: s("g-one") })], "and it is the game to rejoin");
}

// ---- fix round 1: a lost `joined`, lobby errors in a game, the watchdog ----

fn layout(you: &str) -> ServerFrame {
    ServerFrame::Game(ServerMsg::Layout(Layout {
        title: s("Two boxes"),
        you: s(you),
        area: None,
        areas: vec![s("West")],
        sections: vec![],
        segments: vec![],
        signals: vec![],
        points: vec![],
        berths: vec![],
        platforms: vec![],
        routes: vec![],
        geometry: None,
        box_prefix: String::new(),
        workstations: BTreeMap::new(),
        simplifier: vec![],
        display_headcodes: Default::default(),
        places: Default::default(),
    }))
}

/// C2: an overflowing outbox can lose the `joined`; the layout and view
/// that follow it still say which game you are in.
#[test]
fn a_lost_joined_is_made_good_by_the_layout_and_view() {
    let (mut app, h) = open_app();
    app.join("g-one");
    h.push(layout("ann"));
    h.push(view(4));
    app.tick(1.0);
    let g = app.game().expect("in the game without a `joined`");
    assert_eq!((g.id.as_str(), g.you.as_str(), g.view().unwrap().seq), ("g-one", "ann", 4));
    assert_eq!(g.layout_gen(), 1);
    h.push(ServerFrame::error(codes::NOT_HOLDING, "you hold no area"));
    app.tick(2.0);
    assert!(app.game().is_some(), "the join is over: a later error is no failed join");
}

#[test]
fn game_frames_in_the_lobby_without_a_join_are_still_dropped() {
    let (mut app, h) = open_app();
    h.push(layout("ann"));
    h.push(view(4));
    app.tick(1.0);
    assert!(app.game().is_none());
}

#[test]
fn a_lost_joined_after_a_reconnect_ends_the_rejoin_at_the_view() {
    let (mut app, h) = in_game();
    h.close();
    app.tick(2.0);
    app.tick(2.5);
    h.open();
    app.tick(2.6);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::Join { game: s("g-one") })]);
    h.push(layout("ann"));
    h.push(view(30));
    app.tick(2.7);
    h.push(ServerFrame::error(codes::BAD_SPEED, "no such speed"));
    app.tick(2.8);
    let g = app.game().expect("the rejoin succeeded; a later error does not undo it");
    assert_eq!(g.view().unwrap().seq, 30);
    assert_eq!(g.log().entries().last().unwrap().text, "Error: no such speed");
}

#[test]
fn a_lobby_error_in_a_game_is_an_alarm_not_an_exit() {
    let (mut app, h) = in_game();
    h.push(ServerFrame::error(codes::NOT_IN_GAME, "join a game first"));
    app.tick(2.0);
    let g = app.game().expect("still in the game");
    let last = g.log().entries().last().unwrap();
    assert_eq!((last.text.as_str(), last.alarm), ("Error: join a game first", true));
    assert_eq!(app.lobby_note(), None);
    assert!(h.take_sent().is_empty());
}

#[test]
fn a_stopped_game_returns_to_the_lobby_with_a_banner() {
    let (mut app, h) = in_game();
    h.push(ServerFrame::error(codes::GAME_STOPPED, "the game stopped; join it again to resume it"));
    h.push(view(2));
    app.tick(2.0);
    assert!(app.game().is_none());
    assert_eq!(app.lobby_note(), Some("The game stopped. Join it again to resume it."));
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::ListGames)]);
    h.close();
    app.tick(3.0);
    app.tick(3.5);
    h.open();
    app.tick(3.6);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::ListLayouts), lobby(LobbyMsg::ListLessons), lobby(LobbyMsg::ListGames)], "no rejoin");
}

/// Twenty silent seconds in a game (a lost `game_crashed`, say): join the
/// game again once; the front answers with a fresh view or an eviction.
#[test]
fn twenty_silent_seconds_in_a_game_join_it_again_once() {
    let (mut app, h) = in_game(); // last frame at 1.0
    app.tick(20.9);
    assert!(h.take_sent().is_empty(), "19.9 s is not yet silent");
    app.tick(21.0);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::Join { game: s("g-one") })]);
    app.tick(21.5);
    app.tick(40.9);
    assert!(h.take_sent().is_empty(), "once");
    assert_eq!((app.link(), h.connects()), (Link::Open, 1));
}

/// Twenty more silent seconds after the watchdog's join: even a paused game
/// answers a join, so the connection is dead though it never closed. It is
/// treated as closed: back off, then connect again and rejoin.
#[test]
fn a_watchdog_join_unanswered_for_twenty_seconds_reconnects() {
    let (mut app, h) = in_game(); // last frame at 1.0
    app.tick(21.0);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::Join { game: s("g-one") })]);
    app.tick(40.9);
    assert_eq!(app.link(), Link::Open, "19.9 s after the join is not yet dead");
    app.tick(41.0);
    assert_eq!(app.link(), Link::Waiting { retry_at: 41.0 + FIRST_BACKOFF_S });
    assert!(h.take_sent().is_empty(), "no third join down a dead socket");
    app.tick(41.0 + FIRST_BACKOFF_S);
    assert_eq!((app.link(), h.connects()), (Link::Connecting, 2), "a new connection");
    h.open();
    app.tick(41.6);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::Join { game: s("g-one") })], "the rejoin");
    assert_eq!(app.game().unwrap().id, "g-one", "still in the game");
    h.push(joined("g-one"));
    h.push(view(9));
    app.tick(41.7);
    app.tick(61.6);
    assert!(h.take_sent().is_empty(), "the new connection's watchdog starts afresh");
    app.tick(61.7);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::Join { game: s("g-one") })], "a join first, not a reconnect");
    assert_eq!(h.connects(), 2);
}

/// Any frame after the watchdog's join (a paused game's answer, a delta)
/// shows the connection alive: no reconnect, and the watchdog starts over.
#[test]
fn a_frame_after_the_watchdog_join_keeps_the_connection() {
    let (mut app, h) = in_game(); // last frame at 1.0
    app.tick(21.0);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::Join { game: s("g-one") })]);
    h.push(delta(2));
    app.tick(30.0);
    app.tick(49.9);
    assert!(h.take_sent().is_empty());
    assert_eq!((app.link(), h.connects()), (Link::Open, 1), "no reconnect");
    app.tick(50.0);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::Join { game: s("g-one") })], "silence again: a join, not a reconnect");
    assert_eq!((app.link(), h.connects()), (Link::Open, 1));
    assert_eq!(app.game().unwrap().view().unwrap().seq, 2);
}

#[test]
fn the_watchdog_sleeps_in_the_lobby_and_without_a_connection() {
    let (mut app, h) = open_app();
    app.tick(100.0);
    assert!(h.take_sent().is_empty(), "the lobby is quiet by nature");
    let (mut app, h) = in_game();
    h.close();
    app.tick(2.0);
    app.reconnect_now();
    app.tick(30.0);
    h.open();
    app.tick(30.1);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::Join { game: s("g-one") })], "the rejoin only");
    app.tick(50.0);
    assert!(h.take_sent().is_empty(), "the timer starts when the connection opens");
    app.tick(50.1);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::Join { game: s("g-one") })]);
}

// ---- fix round 2 ----

/// The watchdog's join is a rejoin: if the game is gone (or there is no
/// room to resume it) the answer ends the game instead of an alarm every 20 s.
#[test]
fn a_failed_watchdog_join_goes_back_to_the_lobby() {
    let (mut app, h) = in_game();
    app.tick(21.0);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::Join { game: s("g-one") })]);
    h.push(ServerFrame::error(codes::UNKNOWN_GAME, "no game `g-one`"));
    app.tick(21.5);
    assert!(app.game().is_none());
    assert_eq!(app.lobby_note(), Some("Could not rejoin the game: no game `g-one`"));
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::ListGames)]);
}

#[test]
fn a_view_at_the_largest_seq_then_a_delta_never_panics() {
    let (mut app, h) = in_game();
    h.push(view(u64::MAX));
    h.push(delta(0));
    h.push_text("{\"type\": \"delta\", \"seq\": 18446744073709551615}");
    app.tick(2.0);
    assert_eq!(app.game().unwrap().view().unwrap().seq, u64::MAX);
    assert_eq!(h.take_sent(), [ClientFrame::Game(ClientMsg::Resync)], "a gap: one resync");
}

#[test]
fn an_unreadable_frame_in_the_lobby_refreshes_the_lobby() {
    let (mut app, h) = open_app();
    h.push_text("not json at all");
    app.tick(1.0);
    assert!(app.lobby_note().unwrap().starts_with("Unreadable message from the server"));
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::ListGames)]);
}

/// Timetables spec §3.4: the game we wait in is listed as being prepared;
/// if it could not be prepared in time we are back in the lobby, told why.
#[test]
fn a_game_being_prepared_and_one_that_was_too_slow() {
    let (mut app, h) = open_app();
    h.push(joined("g-one"));
    let prep = Preparing { from: 20_400.0, to: 27_000.0 };
    let info = GameInfo {
        id: s("g-one"),
        layout: s("drain"),
        state: GameState::Running,
        sim_time: 21_000.0,
        areas: vec![],
        players: vec![],
        error: None,
        creator: Some(s("ann")),
        can_delete: false,
        preparing: Some(prep),
    };
    h.push(ServerFrame::Lobby(LobbyReply::Games { games: vec![info] }));
    app.tick(1.0);
    assert_eq!(app.preparing(), Some(prep));
    assert_eq!(client_core::text::preparing_text(&prep), "Preparing 05:40 to 07:30…");
    h.push(ServerFrame::error(codes::SEED_TOO_SLOW, "The game could not be prepared in time."));
    app.tick(2.0);
    assert!(app.game().is_none());
    assert_eq!(app.preparing(), None);
    assert_eq!(app.lobby_note(), Some("The game could not be prepared in time."));
}

/// A game that could not be prepared, or was stopped first, was never
/// created: back to the lobby with the front's words.
#[test]
fn a_game_that_was_not_created_returns_to_the_lobby() {
    let (mut app, h) = open_app();
    h.push(joined("g-one"));
    app.tick(1.0);
    h.push(ServerFrame::error(codes::NOT_CREATED, "The game was stopped before it was ready; create it again."));
    app.tick(2.0);
    assert!(app.game().is_none());
    assert_eq!(app.lobby_note(), Some("The game was stopped before it was ready; create it again."));
}

/// Polish spec H2: a game created "to signal" an area claims it as soon as
/// its first layout comes, once; a plain create stays watching.
#[test]
fn a_new_game_claims_the_creators_area_once_its_layout_comes() {
    let (mut app, h) = open_app();
    h.take_sent();
    app.create_game_in("twobox", None, None, Some("West"));
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::CreateGame { layout: s("twobox"), seed: None, start: None })]);
    h.push(joined("g-new"));
    h.push(layout("ann"));
    app.tick(1.0);
    assert_eq!(h.take_sent(), [ClientFrame::Game(ClientMsg::Claim { area: s("West") })]);
    h.push(layout("ann"));
    app.tick(2.0);
    assert!(h.take_sent().is_empty(), "only once");
    let (mut app, h) = open_app();
    h.take_sent();
    app.create_game_in("twobox", None, None, None);
    h.push(joined("g-new"));
    h.push(layout("ann"));
    app.tick(1.0);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::CreateGame { layout: s("twobox"), seed: None, start: None })], "no claim");
}

/// Late start: the area is claimed when the game is ready and its first
/// layout arrives, never while it is still being prepared; a game that is
/// never created leaves no claim behind for the next one.
#[test]
fn a_late_start_claims_after_preparing_not_during() {
    let (mut app, h) = open_app();
    h.take_sent();
    app.create_game_in("drain", None, Some(s("05:40")), Some("West"));
    h.take_sent();
    h.push(joined("g-late"));
    let prep = Preparing { from: 20_400.0, to: 27_000.0 };
    let info = GameInfo {
        id: s("g-late"),
        layout: s("drain"),
        state: GameState::Running,
        sim_time: 21_000.0,
        areas: vec![],
        players: vec![],
        error: None,
        creator: Some(s("ann")),
        can_delete: false,
        preparing: Some(prep),
    };
    h.push(ServerFrame::Lobby(LobbyReply::Games { games: vec![info] }));
    app.tick(1.0);
    assert!(app.preparing().is_some());
    assert!(h.take_sent().is_empty(), "nothing is claimed while preparing");
    app.tick(2.0);
    assert!(h.take_sent().is_empty(), "still nothing");
    h.push(layout("ann"));
    app.tick(3.0);
    assert_eq!(h.take_sent(), [ClientFrame::Game(ClientMsg::Claim { area: s("West") })]);

    // Not created: back in the lobby; the next game, joined plainly, is not claimed.
    let (mut app, h) = open_app();
    h.take_sent();
    app.create_game_in("drain", None, Some(s("05:40")), Some("West"));
    h.push(joined("g-late"));
    h.push(ServerFrame::error(codes::SEED_TOO_SLOW, "The game could not be prepared in time."));
    app.tick(1.0);
    assert!(app.game().is_none());
    h.take_sent();
    app.join("g-other");
    h.take_sent();
    h.push(joined("g-other"));
    h.push(layout("ann"));
    app.tick(2.0);
    assert!(h.take_sent().is_empty(), "no stale claim");
}

/// Fix round 1: a create the front refuses takes its claim with it, and a
/// creator whose area was taken first is told the claim failed.
#[test]
fn a_refused_create_drops_the_claim_and_a_taken_area_says_who() {
    let (mut app, h) = open_app();
    h.take_sent();
    app.create_game_in("twobox", None, Some(s("99:99")), Some("West"));
    h.take_sent();
    h.push(ServerFrame::error("bad_request", "Not a time."));
    app.tick(1.0);
    assert_eq!(app.lobby_note(), Some("Not a time."));
    app.join("g-other");
    h.take_sent();
    h.push(joined("g-other"));
    h.push(layout("ann"));
    app.tick(2.0);
    assert!(h.take_sent().is_empty(), "the refused create left no claim");

    let (mut app, h) = open_app();
    h.take_sent();
    app.create_game_in("twobox", None, None, Some("West"));
    h.push(joined("g-new"));
    h.push(layout("ann"));
    app.tick(1.0);
    assert_eq!(h.take_sent().len(), 2);
    h.push(ServerFrame::Game(ServerMsg::Notice(Notice::AreaTaken { area: s("West"), holder: s("bob") })));
    app.tick(2.0);
    let lines: Vec<String> = app.game().unwrap().log().entries().map(|e| e.text.clone()).collect();
    assert_eq!(lines, [s("Could not claim West: bob took it first")]);
    h.push(ServerFrame::Game(ServerMsg::Notice(Notice::AreaTaken { area: s("West"), holder: s("bob") })));
    app.tick(3.0);
    assert_eq!(app.game().unwrap().log().entries().last().unwrap().text, "West is now bob's", "only the creator's own claim");
}
