//! End to end (spec §1 criteria 1, 4, 5; §12): the real front with dev
//! login, real `signalbox-game` processes, two bots over WebSockets and the
//! robot on Liverpool Street at 8x.
//!
//! Honest label (C2 decision 12): the bots play `Greedy` from their own
//! views; they exercise the transport and the area rules, not good
//! signalling. Safety comes from the game's own counters, which cover the
//! robot's area too.

mod common;

use std::time::{Duration, Instant};

use bot::net::{Conn, NetError};
use bot::play::NetPlayer;
use common::*;
use protocol::*;

const SPEED: u8 = 8;
const WAIT: Duration = Duration::from_secs(20);
/// Bots decide twice a real second (4 sim seconds at 8x).
const DECIDE_EVERY: Duration = Duration::from_millis(500);

fn liverpool_json() -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    let ts2 = std::fs::read_to_string(format!("{dir}/../ts2-import/tests/data/liverpool-st.json")).unwrap();
    let text = std::fs::read_to_string(format!("{dir}/../../layouts/liverpool-st.areas.json")).unwrap();
    let mut w = ts2_import::convert(&ts2).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&text).unwrap()).unwrap();
    serde_json::to_string(&w).unwrap()
}

async fn liverpool_front(name: &str) -> Front {
    front_with(name, &[("liverpool-st", liverpool_json())]).await
}

fn is_layout_for(area: &'static str) -> impl Fn(&ServerFrame) -> bool {
    move |f| matches!(f, ServerFrame::Game(ServerMsg::Layout(l)) if l.area.as_deref() == Some(area))
}

/// A new Liverpool Street game created by `name`; returns the player.
async fn create_liverpool(f: &Front, name: &str) -> NetPlayer {
    let mut p = NetPlayer::login(&f.base, name).await.unwrap();
    p.lobby_msg(LobbyMsg::CreateGame { layout: s("liverpool-st"), seed: Some(7), start: None }).await.unwrap();
    p.until(WAIT, is_view).await.unwrap();
    assert!(p.game.is_some());
    p
}

/// ann holds Liverpool Street, hal Hackney & Bow, the robot Bethnal Green;
/// both vote 8x.
async fn seat(f: &Front) -> (String, NetPlayer, NetPlayer) {
    let mut ann = create_liverpool(f, "ann").await;
    let id = ann.game.clone().unwrap();
    let mut hal = NetPlayer::login(&f.base, "hal").await.unwrap();
    hal.lobby_msg(LobbyMsg::Join { game: id.clone() }).await.unwrap();
    hal.until(WAIT, is_view).await.unwrap();
    ann.game_msg(ClientMsg::Claim { area: s("Liverpool Street") }).await.unwrap();
    ann.until(WAIT, is_layout_for("Liverpool Street")).await.unwrap();
    hal.game_msg(ClientMsg::Claim { area: s("Hackney & Bow") }).await.unwrap();
    hal.until(WAIT, is_layout_for("Hackney & Bow")).await.unwrap();
    for p in [&mut ann, &mut hal] {
        p.game_msg(ClientMsg::Vote { proposal: Proposal::Speed { x: SPEED } }).await.unwrap();
    }
    for p in [&mut ann, &mut hal] {
        p.until_view(WAIT, |v| v.speed == SPEED).await.unwrap();
    }
    (id, ann, hal)
}

/// Play `minutes` of sim time, stop the clock, and check everything.
async fn soak(name: &str, minutes: u64) {
    let f = liverpool_front(name).await;
    let (id, mut ann, mut hal) = seat(&f).await;
    let until = ann.bot.view().unwrap().sim_time + minutes as f64 * 60.0;
    // 8x means minutes * 7.5 s of real time; allow three times that.
    let limit = Duration::from_secs(minutes * 60 * 3 / 8 + 30);
    let wall = Instant::now();
    let (a, h) = tokio::join!(ann.play_until(until, DECIDE_EVERY, limit), hal.play_until(until, DECIDE_EVERY, limit));
    a.unwrap();
    h.unwrap();
    let played = wall.elapsed();

    // Pause so the views hold still, then each bot's delta-built view must
    // equal a fresh full view (spec §1 criterion 1).
    for p in [&mut ann, &mut hal] {
        p.game_msg(ClientMsg::Vote { proposal: Proposal::Pause }).await.unwrap();
    }
    for p in [&mut ann, &mut hal] {
        p.until_view(WAIT, |v| v.paused).await.unwrap();
    }
    for p in [&mut ann, &mut hal] {
        let (built, fresh) = p.resync_and_compare(WAIT).await.unwrap();
        let (mut built, mut fresh) = (built.unwrap(), fresh.unwrap());
        assert!(fresh.seq > built.seq);
        (built.seq, fresh.seq) = (0, 0);
        assert_eq!(built, fresh, "{}'s view built from deltas", p.name);
        let nya = p.bot.take_notices().into_iter().filter(|n| matches!(n, Notice::NotYourArea { .. })).count();
        assert_eq!(nya, 0, "{} only works its own area", p.name);
    }

    // Safety and traffic from the game's own counters (status comes each second).
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let st = f.running.sup.status(&id).expect("a status");
    let c = st.counters;
    assert_eq!((c.spads, c.collisions, c.invariant_violations), (0, 0, 0), "{c:?}");
    assert!(c.player_commands > 0, "the bots' commands reached the sim: {c:?}");
    if minutes >= 20 {
        // Bethnal Green sees its first train needing a route well after the
        // fast run's four minutes (prototype: none by then, 7 by 20).
        assert!(c.robot_commands > 0, "the robot worked its area: {c:?}");
    }
    assert!(ann.commands_sent + hal.commands_sent > 0);
    assert_eq!((st.speed, st.paused), (SPEED, true));
    assert_eq!(st.holders["Bethnal Green"], None, "the robot kept its area");
    eprintln!(
        "e2e {minutes} sim min at {SPEED}x: {played:?} real; commands player {} robot {}; resyncs ann {} hal {}; \
         SQLite writes {} ms in total ({:.2} ms per real second)",
        c.player_commands,
        c.robot_commands,
        ann.bot.resyncs(),
        hal.bot.resyncs(),
        c.save_busy_ms,
        c.save_busy_ms as f64 / played.as_secs_f64(),
    );
    f.running.stop().await;
}

/// Four sim minutes (about 30 s): part of the normal dev-auth test run.
#[tokio::test]
async fn liverpool_street_two_bots_and_the_robot_fast() {
    soak("fast", 4).await;
}

/// One sim hour at 8x (about 7.5 min). Reports SQLite write cost:
/// `scripts/cargo test --release -p signalbox-server --features dev-auth --test e2e -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn liverpool_street_two_bots_and_the_robot_long() {
    soak("long", 60).await;
}

fn kill(pid: u32) {
    let ok = std::process::Command::new("sh").args(["-c", &format!("kill -KILL {pid}")]).status().unwrap();
    assert!(ok.success());
}

fn games_of(f: &ServerFrame) -> Vec<GameInfo> {
    match f {
        ServerFrame::Lobby(LobbyReply::Games { games }) => games.clone(),
        other => panic!("{other:?}"),
    }
}

fn is_games(f: &ServerFrame) -> bool {
    matches!(f, ServerFrame::Lobby(LobbyReply::Games { .. }))
}

/// Spec §1 criteria 4 and 5: a killed game process leaves the front and
/// the other game running, shows as crashed, and resumes on join; nothing
/// answers a socket without a session.
#[tokio::test]
async fn a_crashed_game_leaves_the_front_and_the_other_game_running() {
    let f = liverpool_front("crash").await;
    let e = Conn::connect(&f.base, None).await.err().expect("refused");
    assert!(matches!(e, NetError::Status(401)), "{e}");

    let mut ann = create_liverpool(&f, "ann").await;
    let doomed = ann.game.clone().unwrap();
    let mut bob = create_liverpool(&f, "bob").await;
    let other = bob.game.clone().unwrap();
    kill(f.running.sup.pid(&doomed).expect("a running game has a pid"));
    ann.until(WAIT, |fr| *fr == ServerFrame::Game(ServerMsg::Notice(Notice::GameCrashed))).await.unwrap();

    let t0 = bob.bot.view().unwrap().sim_time;
    bob.until_view(WAIT, |v| v.sim_time > t0 + 2.0).await.unwrap();

    ann.lobby_msg(LobbyMsg::ListGames).await.unwrap();
    let games = games_of(&ann.until(WAIT, is_games).await.unwrap());
    let state = |id: &str| games.iter().find(|g| g.id == id).map(|g| g.state);
    assert_eq!((state(&doomed), state(&other)), (Some(GameState::Crashed), Some(GameState::Running)), "{games:?}");

    ann.lobby_msg(LobbyMsg::Join { game: doomed.clone() }).await.unwrap();
    ann.until(WAIT, is_view).await.unwrap();
    assert_eq!(ann.game.as_deref(), Some(doomed.as_str()));
    assert!(ann.bot.view().unwrap().paused, "resumed from its save, paused");
    ann.lobby_msg(LobbyMsg::ListGames).await.unwrap();
    let games = games_of(&ann.until(WAIT, is_games).await.unwrap());
    assert!(games.iter().all(|g| g.state == GameState::Running), "{games:?}");
    f.running.stop().await;
}
