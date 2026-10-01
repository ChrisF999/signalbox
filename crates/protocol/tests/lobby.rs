//! Lobby wire JSON (spec §3.1) and the frames that share one socket.

use protocol::lobby::{CLIENT_MSG_TYPES, LOBBY_MSG_TYPES, LOBBY_REPLY_TYPES, SERVER_MSG_TYPES};
use protocol::*;
use serde_json::{Value, json};

fn s(x: &str) -> String {
    x.to_string()
}

fn check_client(frame: ClientFrame, want: Value) {
    assert_eq!(serde_json::to_value(&frame).unwrap(), want, "{frame:?}");
    assert_eq!(ClientFrame::from_json(&want.to_string()).unwrap(), frame);
    let back: ClientFrame = serde_json::from_value(want).unwrap();
    assert_eq!(back, frame);
}

fn check_server(frame: ServerFrame, want: Value) {
    assert_eq!(serde_json::to_value(&frame).unwrap(), want, "{frame:?}");
    assert_eq!(ServerFrame::from_json(&frame.to_json()).unwrap(), frame);
    let back: ServerFrame = serde_json::from_value(want).unwrap();
    assert_eq!(back, frame);
}

#[test]
fn lobby_messages() {
    check_client(ClientFrame::Lobby(LobbyMsg::ListGames), json!({"type": "list_games"}));
    check_client(ClientFrame::Lobby(LobbyMsg::ListLayouts), json!({"type": "list_layouts"}));
    check_client(
        ClientFrame::Lobby(LobbyMsg::CreateGame { layout: s("liverpool-st"), seed: None, start: None }),
        json!({"type": "create_game", "layout": "liverpool-st"}),
    );
    check_client(
        ClientFrame::Lobby(LobbyMsg::CreateGame { layout: s("drain"), seed: Some(7), start: Some(s("06:30:00")) }),
        json!({"type": "create_game", "layout": "drain", "seed": 7, "start": "06:30:00"}),
    );
    check_client(ClientFrame::Lobby(LobbyMsg::Join { game: s("g-abcdefgh2345") }), json!({"type": "join", "game": "g-abcdefgh2345"}));
    check_client(ClientFrame::Lobby(LobbyMsg::Leave), json!({"type": "leave"}));
    check_client(
        ClientFrame::Lobby(LobbyMsg::DeleteGame { game: s("g-abcdefgh2345") }),
        json!({"type": "delete_game", "game": "g-abcdefgh2345"}),
    );
    check_client(ClientFrame::Lobby(LobbyMsg::ListLessons), json!({"type": "list_lessons"}));
    check_client(
        ClientFrame::Lobby(LobbyMsg::StartLesson { lesson: s("01-reading-the-panel") }),
        json!({"type": "start_lesson", "lesson": "01-reading-the-panel"}),
    );
}

#[test]
fn the_lessons_list() {
    check_server(
        ServerFrame::Lobby(LobbyReply::Lessons {
            lessons: vec![LessonInfo { id: s("01-reading-the-panel"), title: s("Reading the panel"), steps: 8 }],
        }),
        json!({"type": "lessons", "lessons": [{"id": "01-reading-the-panel", "title": "Reading the panel", "steps": 8}]}),
    );
    check_server(
        ServerFrame::error(codes::UNKNOWN_LESSON, "no lesson `x`"),
        json!({"type": "error", "code": "unknown_lesson", "message": "no lesson `x`"}),
    );
    assert_eq!(codes::IN_LESSON, "in_lesson");
    check_client(ClientFrame::Game(ClientMsg::LessonNext), json!({"type": "lesson_next"}));
}

#[test]
fn game_messages_travel_in_the_same_frames() {
    check_client(ClientFrame::Game(ClientMsg::Resync), json!({"type": "resync"}));
    check_client(
        ClientFrame::Game(ClientMsg::Claim { area: s("Hackney & Bow") }),
        json!({"type": "claim", "area": "Hackney & Bow"}),
    );
    check_server(
        ServerFrame::Game(ServerMsg::Notice(Notice::GameCrashed)),
        json!({"type": "notice", "kind": "game_crashed"}),
    );
}

#[test]
fn lobby_replies() {
    check_server(
        ServerFrame::Lobby(LobbyReply::Games {
            games: vec![
                GameInfo {
                    id: s("g-abcdefgh2345"),
                    layout: s("liverpool-st"),
                    state: GameState::Running,
                    sim_time: 25200.5,
                    areas: vec![
                        AreaHolder { name: s("Liverpool Street"), holder: Some(s("ann")) },
                        AreaHolder { name: s("Bethnal Green"), holder: None },
                    ],
                    players: vec![s("ann"), s("sam")],
                    error: None,
                    creator: None,
                    last_played: None,
                    can_delete: false,
                    preparing: None,
                },
                GameInfo {
                    id: s("g-zzzzzzzzzzzz"),
                    layout: s("drain"),
                    state: GameState::Crashed,
                    sim_time: 3600.0,
                    areas: vec![],
                    players: vec![],
                    error: Some(s("resume: bad snapshot")),
                    creator: Some(s("sam")),
                    last_played: None,
                    can_delete: true,
                    preparing: None,
                },
            ],
        }),
        json!({"type": "games", "games": [
            {"id": "g-abcdefgh2345", "layout": "liverpool-st", "state": "running", "sim_time": 25200.5,
             "areas": [{"name": "Liverpool Street", "holder": "ann"}, {"name": "Bethnal Green"}],
             "players": ["ann", "sam"]},
            {"id": "g-zzzzzzzzzzzz", "layout": "drain", "state": "crashed", "sim_time": 3600.0,
             "areas": [], "players": [], "error": "resume: bad snapshot", "creator": "sam", "can_delete": true}
        ]}),
    );
    check_server(
        ServerFrame::Lobby(LobbyReply::Layouts {
            layouts: vec![LayoutInfo { name: s("drain"), areas: vec![s("Drain"), s("Lambeth")], title: String::new(), description: String::new() }],
            you: None,
        }),
        json!({"type": "layouts", "layouts": [{"name": "drain", "areas": ["Drain", "Lambeth"]}]}),
    );
    // Polish spec M14: the signed-in name, a layout's title and description, when known.
    check_server(
        ServerFrame::Lobby(LobbyReply::Layouts {
            layouts: vec![LayoutInfo { name: s("drain"), areas: vec![s("Bank")], title: s("W&C"), description: s("A shuttle") }],
            you: Some(s("ann")),
        }),
        json!({"type": "layouts", "you": "ann", "layouts": [{"name": "drain", "areas": ["Bank"], "title": "W&C", "description": "A shuttle"}]}),
    );
    check_server(
        ServerFrame::Lobby(LobbyReply::Joined { game: s("g-abcdefgh2345"), you: s("ann") }),
        json!({"type": "joined", "game": "g-abcdefgh2345", "you": "ann"}),
    );
    check_server(
        ServerFrame::error(codes::UNKNOWN_GAME, "no game `g-x`"),
        json!({"type": "error", "code": "unknown_game", "message": "no game `g-x`"}),
    );
    check_server(ServerFrame::Lobby(LobbyReply::Games { games: vec![] }), json!({"type": "games", "games": []}));
    check_server(
        ServerFrame::error(codes::NOT_ALLOWED, "only its creator or an admin may delete a game"),
        json!({"type": "error", "code": "not_allowed", "message": "only its creator or an admin may delete a game"}),
    );
    assert_eq!(codes::GAME_RUNNING, "game_running");
}

#[test]
fn a_games_list_from_before_deletion_still_reads() {
    let old = json!({"type": "games", "games": [{"id": "g-abcdefgh2345", "layout": "drain", "state": "saved",
                                                "sim_time": 0.0, "areas": [], "players": []}]});
    let Ok(ServerFrame::Lobby(LobbyReply::Games { games })) = ServerFrame::from_json(&old.to_string()) else { panic!() };
    assert_eq!((games[0].creator.clone(), games[0].can_delete), (None, false));
}

#[test]
fn lobby_and_game_tags_never_collide() {
    for t in LOBBY_MSG_TYPES {
        assert!(!CLIENT_MSG_TYPES.contains(&t), "{t}");
    }
    for t in LOBBY_REPLY_TYPES {
        assert!(!SERVER_MSG_TYPES.contains(&t), "{t}");
    }
}

#[test]
fn bad_frames_are_classified() {
    let err = |text: &str| ClientFrame::from_json(text).unwrap_err();
    assert_eq!(err("{not json").code(), codes::BAD_JSON);
    assert_eq!(err("[1, 2]").code(), codes::BAD_MESSAGE);
    assert_eq!(err(r#"{"type": 3}"#).code(), codes::BAD_MESSAGE);
    assert_eq!(err(r#"{"type": "teleport"}"#), FrameError::BadMessage(s("unknown type `teleport`")));
    assert_eq!(err(r#"{"type": "join"}"#).code(), codes::BAD_MESSAGE, "missing field");
    assert_eq!(err(r#"{"type": "create_game", "layout": "x", "seed": -1}"#).code(), codes::BAD_MESSAGE);
    assert_eq!(err(r#"{"type": "games", "games": []}"#), FrameError::BadMessage(s("unknown type `games`")), "server-only type");
    assert!(ServerFrame::from_json(r#"{"type": "join", "game": "g"}"#).is_err(), "client-only type");
}

/// `from_json` reads the tag and then the message alone: every tag of
/// every kind reaches its own message (a missing field, not an unknown
/// type), and a frame reads the same as through a `Value`, extra keys,
/// key order and all.
#[test]
fn every_frame_type_is_read_by_its_tag() {
    let missing = |r: Result<(), FrameError>, t: &str| match r {
        Err(FrameError::BadMessage(m)) => assert!(!m.contains("unknown type"), "{t}: {m}"),
        other => panic!("{t}: {other:?}"),
    };
    for t in LOBBY_MSG_TYPES.iter().chain(CLIENT_MSG_TYPES.iter()) {
        let text = format!(r#"{{"type": "{t}", "game": 5}}"#);
        let r = ClientFrame::from_json(&text);
        let via_value = ClientFrame::from_value(serde_json::from_str(&text).unwrap());
        assert_eq!(r.is_ok(), via_value.is_ok(), "{t}");
        match r {
            Ok(f) => assert_eq!(Ok(f), via_value, "{t}"),
            Err(e) => missing(Err(e), t),
        }
    }
    for t in LOBBY_REPLY_TYPES.iter().chain(SERVER_MSG_TYPES.iter()) {
        let text = format!(r#"{{"type": "{t}"}}"#);
        missing(ServerFrame::from_json(&text).map(|_| ()), t);
    }
    // Fields before the tag, unknown fields, escapes in the tag.
    for (text, want) in [
        (r#"{"game": "g-1", "type": "join"}"#, ClientFrame::Lobby(LobbyMsg::Join { game: s("g-1") })),
        (r#"{"type": "join", "game": "g-1", "extra": [1, {"a": null}]}"#, ClientFrame::Lobby(LobbyMsg::Join { game: s("g-1") })),
        (r#"{"type": "\u0072esync"}"#, ClientFrame::Game(ClientMsg::Resync)),
        (r#" {"area": "W", "type": "claim"} "#, ClientFrame::Game(ClientMsg::Claim { area: s("W") })),
    ] {
        assert_eq!(ClientFrame::from_json(text), Ok(want.clone()), "{text}");
        assert_eq!(ClientFrame::from_value(serde_json::from_str(text).unwrap()), Ok(want), "{text}");
    }
    let joined = r#"{"you": "ann", "type": "joined", "game": "g-1"}"#;
    assert_eq!(ServerFrame::from_json(joined), Ok(ServerFrame::Lobby(LobbyReply::Joined { game: s("g-1"), you: s("ann") })));
    let delta = r#"{"seq": 3, "type": "delta", "sim_time": 1.5}"#;
    let Ok(ServerFrame::Game(ServerMsg::Delta(d))) = ServerFrame::from_json(delta) else { panic!() };
    assert_eq!((d.seq, d.sim_time), (3, Some(1.5)));
    assert_eq!(ServerFrame::from_json(delta), ServerFrame::from_value(serde_json::from_str(delta).unwrap()));
}

/// Only an object is a frame, whatever else would parse.
#[test]
fn frames_are_objects() {
    for text in [r#"["join", "g-1"]"#, r#"["resync"]"#, r#""resync""#, "3", "null"] {
        assert_eq!(ClientFrame::from_json(text).unwrap_err().code(), codes::BAD_MESSAGE, "{text}");
        assert_eq!(ServerFrame::from_json(text).unwrap_err().code(), codes::BAD_MESSAGE, "{text}");
    }
    for text in ["", "{", r#"{"type": "join", "game": "g"#, r#"{"type": "resync"} x"#, "[1, 2"] {
        assert_eq!(ClientFrame::from_json(text).unwrap_err().code(), codes::BAD_JSON, "{text}");
    }
    // A repeated key, the tag or any other, is a bad message that says so
    // (the old `Value` path kept the last one).
    for text in [r#"{"type": "resync", "type": "leave"}"#, r#"{"type": "join", "game": "a", "game": "b"}"#] {
        let e = ClientFrame::from_json(text).unwrap_err();
        assert_eq!(e.code(), codes::BAD_MESSAGE, "{text}");
        assert!(e.to_string().contains("duplicate field"), "{text}: {e}");
    }
    let e = ServerFrame::from_json(r#"{"type": "delta", "seq": 1, "seq": 2}"#).unwrap_err();
    assert!(matches!(&e, FrameError::BadMessage(m) if m.contains("duplicate field")), "{e}");
}

/// A game still being prepared (timetables spec §3.4): `preparing` is
/// additive, and a list without it still reads.
#[test]
fn a_preparing_game() {
    let info = GameInfo {
        id: s("g-abcdefgh2345"),
        layout: s("drain"),
        state: GameState::Running,
        sim_time: 21000.0,
        areas: vec![],
        players: vec![],
        error: None,
        creator: Some(s("ann")),
        last_played: None,
        can_delete: false,
        preparing: Some(Preparing { from: 20400.0, to: 27000.0 }),
    };
    check_server(
        ServerFrame::Lobby(LobbyReply::Games { games: vec![info.clone()] }),
        json!({"type": "games", "games": [
            {"id": "g-abcdefgh2345", "layout": "drain", "state": "running", "sim_time": 21000.0,
             "areas": [], "players": [], "creator": "ann", "preparing": {"from": 20400.0, "to": 27000.0}}
        ]}),
    );
    let old = json!({"id": "g-abcdefgh2345", "layout": "drain", "state": "running", "sim_time": 21000.0,
                     "areas": [], "players": []});
    let read: GameInfo = serde_json::from_value(old).unwrap();
    assert_eq!(read.preparing, None);
    assert_eq!(codes::SEED_TOO_SLOW, "seed_too_slow");
}
