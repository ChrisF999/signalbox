//! The front over HTTP and WebSockets with dev login (feature `dev-auth`):
//! the session gate, the lobby, limits, logout and shutdown.

mod common;

use std::time::Duration;

use bot::net::{Conn, NetError, dev_login, http_get};
use common::*;
use protocol::*;

fn refused(r: Result<Conn, NetError>) -> NetError {
    r.err().expect("the upgrade was accepted")
}

#[tokio::test]
async fn nothing_but_the_login_answers_without_a_session() {
    let f = front("gate").await;
    let e = refused(Conn::connect(&f.base, None).await);
    assert!(matches!(e, NetError::Status(401)), "{e}");
    let e = refused(Conn::connect(&f.base, Some("signalbox_session=made-up")).await);
    assert!(matches!(e, NetError::Status(401)), "{e}");
    let real = dev_login(&f.base, "ann").await.unwrap();
    let (name, value) = real.split_once('=').unwrap();
    let mut forged: Vec<char> = value.chars().collect();
    forged[0] = if forged[0] == 'A' { 'B' } else { 'A' };
    let forged = format!("{name}={}", forged.into_iter().collect::<String>());
    let e = refused(Conn::connect(&f.base, Some(&forged)).await);
    assert!(matches!(e, NetError::Status(401)), "a tampered signature: {e}");
    let r = http_get(&f.base, "/", None).await.unwrap();
    assert_eq!((r.status, r.header("location")), (303, Some("/auth/login")));
    let r = http_get(&f.base, "/", Some(&real)).await.unwrap();
    assert_eq!(r.status, 200);
    assert!(r.body.contains("Signed in as ann"), "{}", r.body);
    f.running.stop().await;
}

#[tokio::test]
async fn dev_login_takes_only_plain_names() {
    let f = front("dev-names").await;
    let long = format!("?user={}", "a".repeat(33));
    for q in ["", "?user=", "?user=a%20b", "?user=%3Cscript%3E", &long, "?user=robot", "?user=Robot", "?user=ROBOT"] {
        let r = http_get(&f.base, &format!("/auth/dev{q}"), None).await.unwrap();
        assert_eq!(r.status, 400, "{q}");
        assert!(r.header("set-cookie").is_none(), "{q}");
    }
    let r = http_get(&f.base, "/auth/dev?user=Ann_B.2-x", None).await.unwrap();
    assert_eq!((r.status, r.header("location")), (303, Some("/")));
    let cookie = r.header("set-cookie").unwrap();
    for attr in ["signalbox_session=", "HttpOnly", "SameSite=Lax", "Secure", "Path=/", "Max-Age=43200"] {
        assert!(cookie.contains(attr), "{attr} in {cookie}");
    }
    f.running.stop().await;
}

#[tokio::test]
async fn the_lobby_and_a_game_over_websockets() {
    let f = front("lobby").await;
    let mut ann = f.connect("ann").await;
    ann.send(&lobby(LobbyMsg::ListLayouts)).await.unwrap();
    assert_eq!(
        next(&mut ann).await.unwrap(),
        ServerFrame::Lobby(LobbyReply::Layouts { layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West"), s("East")] }] })
    );
    let id = create(&mut ann, "twobox").await;
    let mut bob = f.connect("bob").await;
    bob.send(&lobby(LobbyMsg::ListGames)).await.unwrap();
    let Some(ServerFrame::Lobby(LobbyReply::Games { games })) = next(&mut bob).await else { panic!() };
    assert_eq!((games.len(), games[0].id.as_str(), games[0].state), (1, id.as_str(), GameState::Running));
    bob.send(&lobby(LobbyMsg::Join { game: id.clone() })).await.unwrap();
    until(&mut bob, is_view).await;
    bob.send(&ClientFrame::Game(ClientMsg::Claim { area: s("East") })).await.unwrap();
    let got = until(&mut bob, |f| matches!(f, ServerFrame::Game(ServerMsg::Layout(l)) if l.area.as_deref() == Some("East"))).await;
    assert!(!got.is_empty());
    f.running.stop().await;
}

#[tokio::test]
async fn limits_malformed_binary_oversize_and_flood() {
    let f = front("limits").await;
    let mut c = f.connect("ann").await;
    let code = |fr: Option<ServerFrame>| match fr {
        Some(ServerFrame::Lobby(LobbyReply::Error { code, .. })) => code,
        other => panic!("{other:?}"),
    };
    c.send_text(s("{not json")).await.unwrap();
    assert_eq!(code(next(&mut c).await), codes::BAD_JSON);
    c.send_text(s(r#"{"type": "teleport"}"#)).await.unwrap();
    assert_eq!(code(next(&mut c).await), codes::BAD_MESSAGE);
    c.send_text(s(r#"{"type": "create_game", "layout": "../../etc/passwd"}"#)).await.unwrap();
    assert_eq!(code(next(&mut c).await), codes::UNKNOWN_LAYOUT);
    c.send_binary(vec![0, 1, 2]).await.unwrap();
    assert_eq!(code(next(&mut c).await), codes::BAD_MESSAGE);
    c.send(&lobby(LobbyMsg::ListGames)).await.unwrap();
    assert!(matches!(next(&mut c).await, Some(ServerFrame::Lobby(LobbyReply::Games { .. }))), "still open");

    // 70 KiB in one message: over the 64 KiB limit, the socket closes.
    let big = format!(r#"{{"type": "join", "game": "{}"}}"#, "x".repeat(70 * 1024));
    let _ = c.send_text(big).await;
    assert_eq!(next(&mut c).await, None, "closed");

    // 30 messages at once: 20 are answered, then the socket closes.
    let mut c = f.connect("ann").await;
    for _ in 0..30 {
        if c.send(&lobby(LobbyMsg::ListLayouts)).await.is_err() {
            break;
        }
    }
    let mut answered = 0;
    while let Some(fr) = next(&mut c).await {
        assert!(matches!(fr, ServerFrame::Lobby(LobbyReply::Layouts { .. })), "{fr:?}");
        answered += 1;
    }
    assert!(answered <= 20, "{answered}");
    assert!(f.saves().read_dir().unwrap().next().is_none(), "no file came of any of it");
    f.running.stop().await;
}

#[tokio::test]
async fn logout_ends_the_session() {
    let f = front("logout").await;
    let cookie = dev_login(&f.base, "ann").await.unwrap();
    assert!(Conn::connect(&f.base, Some(&cookie)).await.is_ok());
    let r = http_get(&f.base, "/auth/logout", Some(&cookie)).await.unwrap();
    assert_eq!(r.status, 200);
    assert!(r.header("set-cookie").unwrap().starts_with("signalbox_session=;"), "{:?}", r.header("set-cookie"));
    let e = refused(Conn::connect(&f.base, Some(&cookie)).await);
    assert!(matches!(e, NetError::Status(401)), "{e}");
    f.running.stop().await;
}

#[tokio::test]
async fn stopping_the_front_saves_and_stops_its_games() {
    let f = front("stop").await;
    let mut ann = f.connect("ann").await;
    let id = create(&mut ann, "twobox").await;
    let sup = f.running.sup.clone();
    assert_eq!(sup.live_count(), 1);
    let save = f.saves().join(format!("{id}.sqlite"));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    f.running.stop().await;
    assert_eq!(sup.live_count(), 0);
    let sum = game::save::read_summary(&save).unwrap();
    assert!(sum.sim_time > 25200.0, "saved on the way out: {}", sum.sim_time);
    while next(&mut ann).await.is_some() {}
}

#[tokio::test]
async fn a_socket_opened_while_the_front_stops_is_not_attached() {
    let f = front("stop-late").await;
    let cookie = dev_login(&f.base, "ann").await.unwrap();
    let mut ann = Conn::connect(&f.base, Some(&cookie)).await.unwrap();
    create(&mut ann, "twobox").await;
    let sup = f.running.sup.clone();
    let base = f.base.clone();
    let stopping = tokio::spawn(f.running.stop());
    for _ in 0..5 {
        tokio::task::yield_now().await;
    }
    assert_eq!(sup.live_count(), 1, "the game is still saving");
    let e = refused(Conn::connect(&base, Some(&cookie)).await);
    assert!(e.to_string().contains("Connection refused"), "no longer listening: {e}");
    assert!(matches!(http_get(&base, "/auth/dev?user=bob", None).await, Err(NetError::Io(_))), "no new logins either");
    assert_eq!(sup.game_of("ann"), None, "nobody was attached to the game");
    stopping.await.unwrap();
    assert_eq!(sup.live_count(), 0);
}
