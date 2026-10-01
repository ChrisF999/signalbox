//! The ipc frame codec: round trips, the wire JSON, and every way a stream
//! can go wrong (an error, never a panic or a huge allocation).

use std::collections::BTreeMap;

use ipc::*;
use protocol::{ClientMsg, Notice, ServerMsg};
use serde_json::json;

fn s(x: &str) -> String {
    x.to_string()
}

fn status() -> StatusMsg {
    StatusMsg {
        sim_time: 25200.5,
        tick: 5,
        paused: false,
        speed: 8,
        holders: BTreeMap::from([(s("East"), None), (s("West"), Some(s("ann")))]),
        players: vec![PlayerStatus { name: s("ann"), connected: true }],
        counters: Counters { player_commands: 3, robot_commands: 9, save_busy_ms: 12, ..Counters::default() },
        preparing: None,
    }
}

fn frame(body: &[u8]) -> Vec<u8> {
    let mut v = (body.len() as u32).to_be_bytes().to_vec();
    v.extend_from_slice(body);
    v
}

#[tokio::test]
async fn frames_round_trip_in_order_and_end_cleanly() {
    let to_game = vec![
        ToGame::Connect { player: s("ann") },
        ToGame::Client { player: s("ann"), msg: ClientMsg::Claim { area: s("Hackney & Bow") } },
        ToGame::Disconnect { player: s("ann") },
        ToGame::Shutdown,
    ];
    let from_game = vec![
        FromGame::ToPlayer { player: s("ann"), msg: ServerMsg::Notice(Notice::Replaced) },
        FromGame::Status(status()),
        FromGame::Saved { tick: 600 },
        FromGame::Log { level: LogLevel::Error, message: s("sqlite: disk full") },
    ];
    let (mut a, mut b) = tokio::io::duplex(64);
    let writer = tokio::spawn({
        let (to_game, from_game) = (to_game.clone(), from_game.clone());
        async move {
            for m in &to_game {
                write_frame(&mut a, m).await.unwrap();
            }
            for m in &from_game {
                write_frame(&mut a, m).await.unwrap();
            }
        }
    });
    for want in &to_game {
        let got: ToGame = read_frame(&mut b).await.unwrap().unwrap();
        assert_eq!(&got, want);
    }
    for want in &from_game {
        let got: FromGame = read_frame(&mut b).await.unwrap().unwrap();
        assert_eq!(&got, want);
    }
    writer.await.unwrap();
    let end: Option<ToGame> = read_frame(&mut b).await.unwrap();
    assert_eq!(end, None, "a clean end between frames");
}

#[test]
fn wire_json() {
    let j = |m: &ToGame| serde_json::to_value(m).unwrap();
    assert_eq!(j(&ToGame::Connect { player: s("ann") }), json!({"type": "connect", "player": "ann"}));
    assert_eq!(j(&ToGame::Shutdown), json!({"type": "shutdown"}));
    assert_eq!(
        j(&ToGame::Client { player: s("ann"), msg: ClientMsg::Resync }),
        json!({"type": "client", "player": "ann", "msg": {"type": "resync"}})
    );
    assert_eq!(
        serde_json::to_value(FromGame::Status(status())).unwrap(),
        json!({"type": "status", "sim_time": 25200.5, "tick": 5, "paused": false, "speed": 8,
               "holders": {"East": null, "West": "ann"}, "players": [{"name": "ann", "connected": true}],
               "counters": {"spads": 0, "collisions": 0, "invariant_violations": 0, "player_commands": 3,
                            "robot_commands": 9, "save_busy_ms": 12}})
    );
    assert_eq!(serde_json::to_value(FromGame::Saved { tick: 7 }).unwrap(), json!({"type": "saved", "tick": 7}));
    assert_eq!(
        serde_json::to_value(FromGame::Log { level: LogLevel::Warn, message: s("x") }).unwrap(),
        json!({"type": "log", "level": "warn", "message": "x"})
    );
}

#[tokio::test]
async fn an_oversize_length_is_refused_before_reading_the_body() {
    let bytes = ((MAX_FRAME + 1) as u32).to_be_bytes();
    let r: Result<Option<ToGame>, _> = read_frame(&mut &bytes[..]).await;
    assert!(matches!(r, Err(IpcError::TooLarge(n)) if n == MAX_FRAME + 1), "{r:?}");
    let huge = u32::MAX.to_be_bytes();
    let r: Result<Option<ToGame>, _> = read_frame(&mut &huge[..]).await;
    assert!(matches!(r, Err(IpcError::TooLarge(_))), "{r:?}");
}

#[tokio::test]
async fn an_oversize_message_is_not_written() {
    let msg = ToGame::Connect { player: "x".repeat(MAX_FRAME) };
    let mut out: Vec<u8> = Vec::new();
    let r = write_frame(&mut out, &msg).await;
    assert!(matches!(r, Err(IpcError::TooLarge(_))), "{r:?}");
    assert!(out.is_empty(), "nothing half-written");
}

#[tokio::test]
async fn a_stream_that_ends_inside_a_frame_is_truncated() {
    let half_header = [0u8, 0];
    let r: Result<Option<ToGame>, _> = read_frame(&mut &half_header[..]).await;
    assert!(matches!(r, Err(IpcError::Truncated)), "{r:?}");
    let mut short = frame(br#"{"type":"shutdown"}"#);
    short.truncate(10);
    let r: Result<Option<ToGame>, _> = read_frame(&mut &short[..]).await;
    assert!(matches!(r, Err(IpcError::Truncated)), "{r:?}");
}

#[tokio::test]
async fn garbage_and_wrong_shapes_are_json_errors() {
    for body in [&b"not json at all"[..], br#"{"type":"teleport"}"#, br#"{"type":"connect"}"#, b""] {
        let bytes = frame(body);
        let r: Result<Option<ToGame>, _> = read_frame(&mut &bytes[..]).await;
        assert!(matches!(r, Err(IpcError::Json(_))), "{:?}: {r:?}", String::from_utf8_lossy(body));
    }
}

/// While a game is prepared its status says so; older statuses still read.
#[test]
fn a_preparing_status() {
    let st = StatusMsg { preparing: Some(protocol::Preparing { from: 20400.0, to: 27000.0 }), ..status() };
    let v = serde_json::to_value(FromGame::Status(st.clone())).unwrap();
    assert_eq!(v["preparing"], json!({"from": 20400.0, "to": 27000.0}));
    assert_eq!(serde_json::from_value::<FromGame>(v).unwrap(), FromGame::Status(st));
    let mut old = serde_json::to_value(FromGame::Status(status())).unwrap();
    assert!(old.get("preparing").is_none());
    old.as_object_mut().unwrap().remove("preparing");
    assert_eq!(serde_json::from_value::<FromGame>(old).unwrap(), FromGame::Status(status()));
}
