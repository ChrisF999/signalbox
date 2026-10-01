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
                    can_delete: false,
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
                    can_delete: true,
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
            layouts: vec![LayoutInfo { name: s("drain"), areas: vec![s("Drain"), s("Lambeth")] }],
        }),
        json!({"type": "layouts", "layouts": [{"name": "drain", "areas": ["Drain", "Lambeth"]}]}),
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
