//! The front's supervisor driven directly (no HTTP): the lobby, real
//! `signalbox-game` children, relaying, duplicate logins, crashes, the
//! outbound queue and shutdown.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use game::{Game, GameMeta};
use protocol::*;
use server::layouts::{Layouts, new_game_id, valid_game_id, valid_layout_name};
use server::outbox::{OUTBOX_CAP, Outbox, Pushed};
use server::supervisor::{Attached, Supervisor, SupervisorConfig};
use tokio::time::{sleep, timeout};

const TWOBOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../game/tests/fixtures/twobox.json");
const GAME_BIN: &str = env!("CARGO_BIN_EXE_signalbox-game");

fn s(x: &str) -> String {
    x.to_string()
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sbx-sup-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A layouts directory holding `twobox.json` and files the front must skip.
fn layouts_dir(root: &Path) -> PathBuf {
    let dir = root.join("layouts");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(TWOBOX, dir.join("twobox.json")).unwrap();
    std::fs::write(dir.join("Bad Name.json"), "not even json").unwrap();
    std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
    dir
}

struct Rig {
    root: PathBuf,
    sup: Arc<Supervisor>,
}

fn rig(name: &str, empty_exit_s: u64) -> Rig {
    let root = temp_dir(name);
    let layouts = Layouts::load(&layouts_dir(&root)).unwrap();
    let cfg = SupervisorConfig {
        game_bin: PathBuf::from(GAME_BIN),
        saves_dir: root.join("data/saves"),
        sockets_dir: root.join("data/sockets"),
        empty_exit_s,
    };
    Rig { sup: Supervisor::new(cfg, layouts).unwrap(), root }
}

/// One client socket, as the WebSocket loop would hold it.
struct Sock {
    user: String,
    me: Attached,
}

impl Rig {
    fn saves(&self) -> PathBuf {
        self.root.join("data/saves")
    }

    fn attach(&self, user: &str) -> Sock {
        Sock { user: s(user), me: self.sup.attach(user) }
    }

    fn send(&self, sock: &Sock, frame: ClientFrame) {
        self.sup.handle_frame(&sock.user, sock.me.conn, frame);
    }

    fn lobby(&self, sock: &Sock, msg: LobbyMsg) {
        self.send(sock, ClientFrame::Lobby(msg));
    }

    fn game_msg(&self, sock: &Sock, msg: ClientMsg) {
        self.send(sock, ClientFrame::Game(msg));
    }

    fn info(&self, id: &str) -> GameInfo {
        self.sup.list_games().into_iter().find(|g| g.id == id).unwrap_or_else(|| panic!("{id} is not listed"))
    }

    /// Poll the lobby until `ok` holds for game `id` (10 s).
    async fn wait_for(&self, id: &str, ok: impl Fn(&GameInfo) -> bool) -> GameInfo {
        for _ in 0..200 {
            if let Some(g) = self.sup.list_games().into_iter().find(|g| g.id == id) {
                if ok(&g) {
                    return g;
                }
            }
            sleep(Duration::from_millis(50)).await;
        }
        panic!("game {id} never got there: {:?}", self.sup.list_games());
    }
}

/// The next frame for this socket (10 s), `None` once it is closed.
async fn next(sock: &Sock) -> Option<ServerFrame> {
    timeout(Duration::from_secs(10), sock.me.outbox.pop()).await.expect("no frame within 10 s")
}

/// Frames until one matches `stop` (included).
async fn until(sock: &Sock, stop: impl Fn(&ServerFrame) -> bool) -> Vec<ServerFrame> {
    let mut got = Vec::new();
    loop {
        let f = next(sock).await.unwrap_or_else(|| panic!("closed; got {got:?}"));
        let done = stop(&f);
        got.push(f);
        if done {
            return got;
        }
    }
}

fn is_view(f: &ServerFrame) -> bool {
    matches!(f, ServerFrame::Game(ServerMsg::View(_)))
}

fn error_code(f: &ServerFrame) -> Option<&str> {
    match f {
        ServerFrame::Lobby(LobbyReply::Error { code, .. }) => Some(code),
        _ => None,
    }
}

async fn expect_error(sock: &Sock, code: &str) {
    let f = next(sock).await.expect("open");
    assert_eq!(error_code(&f), Some(code), "{f:?}");
}

/// Create a twobox game as `sock`'s user; returns its id once the view came.
async fn create(rig: &Rig, sock: &Sock) -> String {
    rig.lobby(sock, LobbyMsg::CreateGame { layout: s("twobox"), seed: Some(5), start: None });
    let got = until(sock, is_view).await;
    let Some(ServerFrame::Lobby(LobbyReply::Joined { game, you })) = got.first() else { panic!("{got:?}") };
    assert_eq!(you, &sock.user);
    game.clone()
}

fn kill(pid: u32, signal: &str) {
    let ok = std::process::Command::new("sh").args(["-c", &format!("kill -{signal} {pid}")]).status().unwrap();
    assert!(ok.success());
}

// ---- pure parts ----

#[test]
fn names_that_become_file_names_are_checked() {
    assert!(valid_layout_name("liverpool-st") && valid_layout_name("drain") && valid_layout_name("a"));
    for bad in ["", "Drain", "../x", "a/b", "a.json", "x y", &"a".repeat(41)] {
        assert!(!valid_layout_name(bad), "{bad}");
    }
    assert!(valid_game_id("g-abcdefgh2345"));
    for bad in ["g-abcdefgh234", "g-abcdefgh23456", "g-ABCDEFGH2345", "g-abcdefgh2341", "x-abcdefgh2345", "g-../x", "g-abcdefgh234/"] {
        assert!(!valid_game_id(bad), "{bad}");
    }
    for _ in 0..100 {
        let id = new_game_id();
        assert!(valid_game_id(&id), "{id}");
    }
}

#[test]
fn layouts_are_read_once_and_only_valid_names_count() {
    let root = temp_dir("layouts");
    let dir = layouts_dir(&root);
    let l = Layouts::load(&dir).unwrap();
    assert_eq!(l.infos(), [LayoutInfo { name: s("twobox"), areas: vec![s("West"), s("East")] }]);
    assert_eq!(l.path("twobox"), Some(dir.join("twobox.json")));
    assert_eq!(l.path("../layouts/twobox"), None);
    assert_eq!(l.path("Bad Name"), None);
    std::fs::write(dir.join("broken.json"), r#"{"areas": [{"title": "no name"}]}"#).unwrap();
    let err = Layouts::load(&dir).unwrap_err();
    assert!(err.contains("broken.json") && err.contains("no named areas"), "{err}");
    assert!(Layouts::load(&root.join("absent")).is_err());
}

fn view(seq: u64) -> ServerFrame {
    ServerFrame::Game(ServerMsg::View(View {
        seq,
        sim_time: 25200.0,
        speed: 1,
        paused: false,
        vote: None,
        holders: Default::default(),
        score: None,
        signals: Default::default(),
        routes: Default::default(),
        points: Default::default(),
        sections: Default::default(),
        berths: Default::default(),
    }))
}

fn delta(seq: u64) -> ServerFrame {
    ServerFrame::Game(ServerMsg::Delta(Delta { seq, sim_time: Some(25200.0 + seq as f64), ..Delta::default() }))
}

fn notice(n: Notice) -> ServerFrame {
    ServerFrame::Game(ServerMsg::Notice(n))
}

#[tokio::test]
async fn outbox_overflow_clears_asks_for_one_resync_and_drops_deltas_until_a_view() {
    let o = Outbox::new();
    assert_eq!(o.push(view(1)), Pushed::Queued);
    for seq in 2..=OUTBOX_CAP as u64 {
        assert_eq!(o.push(delta(seq)), Pushed::Queued);
    }
    assert_eq!(o.len(), OUTBOX_CAP);
    assert_eq!(o.push(delta(65)), Pushed::Overflowed, "full: clear and ask for a resync");
    assert!(o.is_empty(), "the stale queue is gone");
    assert_eq!(o.push(delta(66)), Pushed::Dropped, "a delta against a base the client never got");
    assert_eq!(o.push(notice(Notice::Replaced)), Pushed::Queued, "notices still go through");
    assert_eq!(o.push(view(67)), Pushed::Queued, "the full view is the new base");
    assert_eq!(o.push(delta(68)), Pushed::Queued, "deltas flow again");
    let mut got = Vec::new();
    while !o.is_empty() {
        got.push(o.pop().await.unwrap());
    }
    assert_eq!(got, [notice(Notice::Replaced), view(67), delta(68)]);
    o.close();
    assert_eq!(o.push(view(69)), Pushed::Closed);
    assert_eq!(o.pop().await, None, "closed and drained");
}

#[tokio::test]
async fn a_closed_outbox_still_hands_out_what_it_holds() {
    let o = Outbox::new();
    o.push(notice(Notice::Replaced));
    o.close();
    assert_eq!(o.pop().await, Some(notice(Notice::Replaced)));
    assert_eq!(o.pop().await, None);
}

// ---- the supervisor with real game processes ----

#[tokio::test]
async fn the_data_directories_are_private() {
    let rig = rig("private", 600);
    for d in ["data/saves", "data/sockets"] {
        let mode = std::fs::metadata(rig.root.join(d)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "{d}");
    }
}

#[tokio::test]
async fn the_lobby_lists_layouts_and_saved_games() {
    let rig = rig("list", 600);
    let saved = "g-aaaaaaaaaaaa";
    let json = std::fs::read_to_string(TWOBOX).unwrap();
    drop(Game::create(&rig.saves().join(format!("{saved}.sqlite")), &json, GameMeta { layout: s("twobox"), seed: 2 }).unwrap());
    std::fs::write(rig.saves().join("g-bbbbbbbbbbbb.sqlite"), "this is not a database").unwrap();
    std::fs::write(rig.saves().join("notes.sqlite"), "not a game id").unwrap();
    let ann = rig.attach("ann");
    rig.lobby(&ann, LobbyMsg::ListLayouts);
    assert_eq!(
        next(&ann).await.unwrap(),
        ServerFrame::Lobby(LobbyReply::Layouts { layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West"), s("East")] }] })
    );
    rig.lobby(&ann, LobbyMsg::ListGames);
    let Some(ServerFrame::Lobby(LobbyReply::Games { games })) = next(&ann).await else { panic!() };
    assert_eq!(games.len(), 2, "{games:?}");
    assert_eq!(games[0].id, saved);
    assert_eq!((games[0].layout.as_str(), games[0].state, games[0].sim_time), ("twobox", GameState::Saved, 25200.0));
    assert_eq!(
        games[0].areas,
        [AreaHolder { name: s("West"), holder: None }, AreaHolder { name: s("East"), holder: None }]
    );
    assert_eq!((games[1].id.as_str(), games[1].state), ("g-bbbbbbbbbbbb", GameState::Crashed));
    assert!(games[1].error.as_deref().unwrap().contains("not a database"), "{:?}", games[1].error);
}

#[tokio::test]
async fn lobby_rejects_bad_layouts_starts_and_ids() {
    let rig = rig("reject", 600);
    let ann = rig.attach("ann");
    for layout in ["../../etc/passwd", "nope", "Bad Name", ""] {
        rig.lobby(&ann, LobbyMsg::CreateGame { layout: s(layout), seed: None, start: None });
        expect_error(&ann, codes::UNKNOWN_LAYOUT).await;
    }
    for start in ["25:00", "24:00", "7", "noon", "07:60"] {
        rig.lobby(&ann, LobbyMsg::CreateGame { layout: s("twobox"), seed: None, start: Some(s(start)) });
        expect_error(&ann, codes::BAD_START).await;
    }
    for id in ["g-../x", "../saves/x", "g-aaaaaaaaaaaa", ""] {
        rig.lobby(&ann, LobbyMsg::Join { game: s(id) });
        expect_error(&ann, codes::UNKNOWN_GAME).await;
    }
    rig.game_msg(&ann, ClientMsg::Claim { area: s("West") });
    expect_error(&ann, codes::NOT_IN_GAME).await;
    rig.sup.handle_text("ann", ann.me.conn, "{not json");
    expect_error(&ann, codes::BAD_JSON).await;
    rig.sup.handle_text("ann", ann.me.conn, r#"{"type": "teleport"}"#);
    expect_error(&ann, codes::BAD_MESSAGE).await;
    assert_eq!(rig.sup.live_count(), 0, "nothing started");
    assert_eq!(std::fs::read_dir(rig.saves()).unwrap().count(), 0, "no file was written");
}

#[tokio::test]
async fn create_join_claim_and_leave() {
    let rig = rig("play", 600);
    let ann = rig.attach("ann");
    rig.lobby(&ann, LobbyMsg::CreateGame { layout: s("twobox"), seed: Some(5), start: Some(s("8:00")) });
    let got = until(&ann, is_view).await;
    let Some(ServerFrame::Lobby(LobbyReply::Joined { game: id, .. })) = got.first() else { panic!("{got:?}") };
    let id = id.clone();
    let Some(ServerFrame::Game(ServerMsg::View(v))) = got.last() else { unreachable!() };
    assert!((28800.0..28801.0).contains(&v.sim_time), "the start time reached the game: {}", v.sim_time);
    assert!(valid_game_id(&id) && rig.saves().join(format!("{id}.sqlite")).exists());
    assert_eq!(rig.sup.game_of("ann").as_deref(), Some(id.as_str()));

    let bob = rig.attach("bob");
    rig.lobby(&bob, LobbyMsg::Join { game: id.clone() });
    let got = until(&bob, is_view).await;
    assert_eq!(got[0], ServerFrame::Lobby(LobbyReply::Joined { game: id.clone(), you: s("bob") }));

    rig.game_msg(&ann, ClientMsg::Claim { area: s("West") });
    let got = until(&ann, |f| matches!(f, ServerFrame::Game(ServerMsg::Layout(_)))).await;
    let Some(ServerFrame::Game(ServerMsg::Layout(l))) = got.last() else { unreachable!() };
    assert_eq!(l.area.as_deref(), Some("West"));
    let g = rig.wait_for(&id, |g| g.areas.iter().any(|a| a.holder.as_deref() == Some("ann"))).await;
    assert_eq!((g.state, g.players.clone()), (GameState::Running, vec![s("ann"), s("bob")]));

    rig.lobby(&bob, LobbyMsg::Leave);
    let got = until(&bob, |f| matches!(f, ServerFrame::Lobby(LobbyReply::Games { .. }))).await;
    assert!(got.iter().all(|f| !matches!(f, ServerFrame::Lobby(LobbyReply::Error { .. }))), "{got:?}");
    assert_eq!(rig.sup.game_of("bob"), None);
    rig.wait_for(&id, |g| g.players == [s("ann")]).await;
    rig.game_msg(&bob, ClientMsg::Resync);
    let f = until(&bob, |f| error_code(f).is_some()).await;
    assert_eq!(error_code(f.last().unwrap()), Some(codes::NOT_IN_GAME));
    rig.sup.shutdown_all(Duration::from_secs(10)).await;
}

#[tokio::test]
async fn a_second_login_replaces_the_first_and_keeps_the_game() {
    let rig = rig("replace", 600);
    let first = rig.attach("ann");
    let id = create(&rig, &first).await;
    rig.game_msg(&first, ClientMsg::Claim { area: s("West") });
    until(&first, |f| matches!(f, ServerFrame::Game(ServerMsg::Layout(l)) if l.area.as_deref() == Some("West"))).await;

    let second = rig.attach("ann");
    let old = until(&first, |f| *f == notice(Notice::Replaced)).await;
    assert_eq!(old.last(), Some(&notice(Notice::Replaced)));
    assert_eq!(next(&first).await, None, "the old socket is closed");
    let got = until(&second, is_view).await;
    assert_eq!(got[0], ServerFrame::Lobby(LobbyReply::Joined { game: id.clone(), you: s("ann") }));
    let layout = got.iter().find_map(|f| match f {
        ServerFrame::Game(ServerMsg::Layout(l)) => Some(l.clone()),
        _ => None,
    });
    assert_eq!(layout.unwrap().area.as_deref(), Some("West"), "still holding her area");

    rig.sup.detach("ann", first.me.conn);
    rig.game_msg(&first, ClientMsg::Release);
    sleep(Duration::from_millis(1500)).await;
    let g = rig.info(&id);
    assert_eq!(g.players, [s("ann")], "the old socket closing did not disconnect her");
    assert_eq!(g.areas[0], AreaHolder { name: s("West"), holder: Some(s("ann")) }, "and its messages were ignored");
    assert_eq!(rig.sup.game_of("ann").as_deref(), Some(id.as_str()));
    rig.sup.shutdown_all(Duration::from_secs(10)).await;
}

#[tokio::test]
async fn a_game_that_fails_to_resume_shows_as_crashed_with_its_error() {
    let rig = rig("bad-resume", 600);
    let bob = rig.attach("bob");
    let other = create(&rig, &bob).await;
    let bad = "g-cccccccccccc";
    std::fs::write(rig.saves().join(format!("{bad}.sqlite")), "this is not a database").unwrap();
    let ann = rig.attach("ann");
    rig.lobby(&ann, LobbyMsg::Join { game: s(bad) });
    let got = until(&ann, |f| *f == notice(Notice::GameCrashed)).await;
    assert_eq!(got[0], ServerFrame::Lobby(LobbyReply::Joined { game: s(bad), you: s("ann") }));
    assert_eq!(rig.sup.game_of("ann"), None, "back in the lobby");
    let g = rig.info(bad);
    assert_eq!(g.state, GameState::Crashed);
    let why = g.error.unwrap();
    assert!(why.starts_with("signalbox-game: ") && why.contains("not a database"), "the child's own words: {why}");
    assert_eq!(rig.info(&other).state, GameState::Running, "the other game carries on");
    rig.game_msg(&bob, ClientMsg::Resync);
    until(&bob, is_view).await;
    rig.sup.shutdown_all(Duration::from_secs(10)).await;
}

#[tokio::test]
async fn a_game_whose_binary_is_missing_is_crashed_not_fatal() {
    let root = temp_dir("no-bin");
    let cfg = SupervisorConfig {
        game_bin: root.join("absent-game-binary"),
        saves_dir: root.join("saves"),
        sockets_dir: root.join("sockets"),
        empty_exit_s: 600,
    };
    let sup = Supervisor::new(cfg, Layouts::load(&layouts_dir(&root)).unwrap()).unwrap();
    let me = sup.attach("ann");
    sup.handle_frame("ann", me.conn, ClientFrame::Lobby(LobbyMsg::CreateGame { layout: s("twobox"), seed: None, start: None }));
    let sock = Sock { user: s("ann"), me };
    until(&sock, |f| *f == notice(Notice::GameCrashed)).await;
    let games = sup.list_games();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].state, GameState::Crashed);
    assert!(games[0].error.as_deref().unwrap().starts_with("cannot start"), "{games:?}");
}

#[tokio::test]
async fn a_killed_game_is_crashed_and_join_resumes_it() {
    let rig = rig("kill", 600);
    let ann = rig.attach("ann");
    let id = create(&rig, &ann).await;
    let bob = rig.attach("bob");
    let other = create(&rig, &bob).await;
    let g = rig.wait_for(&id, |g| g.sim_time > 25201.0).await;
    let pid = rig.sup.pid(&id).expect("running games have a pid");
    kill(pid, "KILL");
    until(&ann, |f| *f == notice(Notice::GameCrashed)).await;
    let crashed = rig.wait_for(&id, |g| g.state == GameState::Crashed).await;
    assert!(crashed.error.is_some());
    assert_eq!(rig.info(&other).state, GameState::Running);
    assert_eq!(rig.sup.live_count(), 1);

    rig.lobby(&ann, LobbyMsg::Join { game: id.clone() });
    let got = until(&ann, is_view).await;
    let Some(ServerFrame::Game(ServerMsg::View(v))) = got.last() else { unreachable!() };
    assert!(v.paused, "a resumed game starts paused");
    assert!(v.sim_time >= 25200.0 && v.sim_time <= g.sim_time + 1.0, "back to its last save");
    assert_eq!(rig.info(&id).state, GameState::Running);
    rig.sup.shutdown_all(Duration::from_secs(10)).await;
}

#[tokio::test]
async fn an_empty_game_saves_exits_and_is_listed_as_saved() {
    let rig = rig("empty", 1);
    let ann = rig.attach("ann");
    let id = create(&rig, &ann).await;
    rig.lobby(&ann, LobbyMsg::Leave);
    let g = rig.wait_for(&id, |g| g.state == GameState::Saved).await;
    assert_eq!(rig.sup.live_count(), 0);
    assert!(g.error.is_none());
}

#[tokio::test]
async fn a_stalled_client_gets_one_fresh_view_after_its_queue_overflows() {
    let rig = rig("stall", 600);
    let ann = rig.attach("ann");
    create(&rig, &ann).await;
    // Every resync answers with a layout and a view: 80 frames nobody reads.
    for _ in 0..40 {
        rig.game_msg(&ann, ClientMsg::Resync);
    }
    sleep(Duration::from_secs(2)).await;
    assert!(ann.me.outbox.len() <= OUTBOX_CAP);
    // Deltas keep coming five times a second, so drain for a fixed time.
    let mut got = Vec::new();
    let end = tokio::time::Instant::now() + Duration::from_secs(1);
    while let Ok(Some(f)) = tokio::time::timeout_at(end, ann.me.outbox.pop()).await {
        got.push(f);
    }
    assert!(got.len() < 80, "the overflow dropped the stale frames ({})", got.len());
    let first_view = got.iter().position(is_view).expect("a full view came");
    assert!(
        !got[..first_view].iter().any(|f| matches!(f, ServerFrame::Game(ServerMsg::Delta(_)))),
        "no delta before the new base"
    );
    rig.sup.shutdown_all(Duration::from_secs(10)).await;
}

#[tokio::test]
async fn shutdown_saves_every_game_and_closes_every_client() {
    let rig = rig("shutdown", 600);
    let ann = rig.attach("ann");
    let a = create(&rig, &ann).await;
    let bob = rig.attach("bob");
    let b = create(&rig, &bob).await;
    rig.wait_for(&b, |g| g.sim_time > 25201.0).await;
    rig.sup.shutdown_all(Duration::from_secs(10)).await;
    assert_eq!(rig.sup.live_count(), 0);
    for id in [&a, &b] {
        assert_eq!(rig.info(id).state, GameState::Saved, "{id}");
    }
    assert!(game::save::read_summary(&rig.saves().join(format!("{b}.sqlite"))).unwrap().sim_time > 25201.0, "saved on the way out");
    while let Some(f) = next(&ann).await {
        assert!(!matches!(f, ServerFrame::Game(ServerMsg::Notice(Notice::GameCrashed))), "{f:?}");
    }
    let late = rig.attach("cat");
    rig.lobby(&late, LobbyMsg::CreateGame { layout: s("twobox"), seed: None, start: None });
    assert_eq!(next(&late).await, None, "a stopping front takes no new sockets");
}

#[tokio::test]
async fn the_robot_name_cannot_enter_a_game() {
    let rig = rig("robot", 600);
    let ann = rig.attach("ann");
    let id = create(&rig, &ann).await;
    let robot = rig.attach(game::ROBOT);
    rig.lobby(&robot, LobbyMsg::CreateGame { layout: s("twobox"), seed: None, start: None });
    expect_error(&robot, codes::RESERVED_NAME).await;
    rig.lobby(&robot, LobbyMsg::Join { game: id.clone() });
    expect_error(&robot, codes::RESERVED_NAME).await;
    assert_eq!(rig.sup.game_of(game::ROBOT), None);
    assert_eq!(rig.sup.live_count(), 1, "no game was started for it");
    rig.game_msg(&robot, ClientMsg::Resync);
    expect_error(&robot, codes::NOT_IN_GAME).await;
    rig.sup.shutdown_all(Duration::from_secs(10)).await;
}

#[tokio::test]
async fn a_game_still_starting_at_shutdown_is_stopped_not_crashed() {
    let root = temp_dir("slow-start");
    // A "game" that never listens: it is still starting when the front stops.
    let bin = root.join("slow-game");
    std::fs::write(&bin, "#!/bin/sh\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let cfg = SupervisorConfig {
        game_bin: bin,
        saves_dir: root.join("saves"),
        sockets_dir: root.join("sockets"),
        empty_exit_s: 600,
    };
    let sup = Supervisor::new(cfg, Layouts::load(&layouts_dir(&root)).unwrap()).unwrap();
    let sock = Sock { user: s("ann"), me: sup.attach("ann") };
    sup.handle_frame("ann", sock.me.conn, ClientFrame::Lobby(LobbyMsg::CreateGame { layout: s("twobox"), seed: None, start: None }));
    assert!(matches!(next(&sock).await, Some(ServerFrame::Lobby(LobbyReply::Joined { .. }))));
    assert_eq!(sup.live_count(), 1);
    let t0 = tokio::time::Instant::now();
    sup.shutdown_all(Duration::from_millis(300)).await;
    assert!(t0.elapsed() < Duration::from_secs(5), "{:?}", t0.elapsed());
    assert_eq!(sup.live_count(), 0);
    let games = sup.list_games();
    assert!(games.iter().all(|g| g.state != GameState::Crashed), "stopping it was not a crash: {games:?}");
    while let Some(f) = next(&sock).await {
        assert_ne!(f, notice(Notice::GameCrashed));
    }
}
