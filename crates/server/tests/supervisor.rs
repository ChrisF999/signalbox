//! The front's supervisor driven directly (no HTTP): the lobby, real
//! `signalbox-game` children, relaying, duplicate logins, crashes, the
//! outbound queue and shutdown.

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use game::{Game, GameMeta};
use protocol::*;
use server::layouts::{Layouts, new_game_id, valid_game_id, valid_layout_name};
use server::lessons::Lessons;
use server::outbox::{OUTBOX_CAP, Outbox, Pushed};
use server::supervisor::{Attached, MAX_TUTORIALS, Supervisor, SupervisorConfig};
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
        admins: BTreeSet::from([s("root")]),
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
        trains: Default::default(),
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
    // The claim's View may still have been in flight when `attach` swapped
    // the sockets, so it can reach `second` before the reconnect's Layout:
    // wait for the Layout itself, not the first View.
    let got = until(&second, |f| matches!(f, ServerFrame::Game(ServerMsg::Layout(_)))).await;
    assert_eq!(got[0], ServerFrame::Lobby(LobbyReply::Joined { game: id.clone(), you: s("ann") }));
    let Some(ServerFrame::Game(ServerMsg::Layout(layout))) = got.last() else { unreachable!() };
    assert_eq!(layout.area.as_deref(), Some("West"), "still holding her area");

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
    assert!(why.ends_with(" (exit status 1)"), "and how it ended: {why}");
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
        admins: BTreeSet::new(),
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
    let why = crashed.error.unwrap();
    assert!(why.ends_with(" (killed by signal 9)"), "the crash reason says how it ended: {why}");
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
        admins: BTreeSet::new(),
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

#[tokio::test]
async fn a_socket_opened_while_stopping_is_not_attached() {
    let rig = rig("attach-closing", 600);
    let ann = rig.attach("ann");
    let id = create(&rig, &ann).await;
    let stopping = tokio::spawn({
        let sup = rig.sup.clone();
        async move { sup.shutdown_all(Duration::from_secs(10)).await }
    });
    // Let the shutdown start (current-thread runtime: it runs on a yield).
    for _ in 0..5 {
        tokio::task::yield_now().await;
    }
    assert_eq!(rig.sup.live_count(), 1, "the game is still saving");
    let late = rig.attach("ann");
    assert_eq!(next(&late).await, None, "no `joined`, closed at once");
    while let Some(f) = next(&ann).await {
        assert!(!matches!(f, ServerFrame::Game(ServerMsg::Notice(Notice::Replaced))), "the old socket was not replaced: {f:?}");
    }
    stopping.await.unwrap();
    assert_eq!(rig.info(&id).state, GameState::Saved);
}

#[tokio::test]
async fn the_robot_name_is_reserved_in_any_case() {
    let rig = rig("robot-case", 600);
    let ann = rig.attach("ann");
    let id = create(&rig, &ann).await;
    for name in ["Robot", "ROBOT", "rObOt"] {
        let r = rig.attach(name);
        rig.lobby(&r, LobbyMsg::CreateGame { layout: s("twobox"), seed: None, start: None });
        expect_error(&r, codes::RESERVED_NAME).await;
        rig.lobby(&r, LobbyMsg::Join { game: id.clone() });
        expect_error(&r, codes::RESERVED_NAME).await;
        assert_eq!(rig.sup.game_of(name), None, "{name}");
    }
    assert_eq!(rig.sup.live_count(), 1);
    rig.sup.shutdown_all(Duration::from_secs(10)).await;
}

/// The process group of `pid` (field 5 of `/proc/<pid>/stat`).
fn pgrp(pid: &str) -> u32 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let after = &stat[stat.rfind(')').unwrap() + 2..];
    after.split(' ').nth(2).unwrap().parse().unwrap()
}

#[tokio::test]
async fn game_children_run_in_their_own_process_group() {
    let rig = rig("pgrp", 600);
    let ann = rig.attach("ann");
    let id = create(&rig, &ann).await;
    let pid = rig.sup.pid(&id).expect("running games have a pid");
    assert_eq!(pgrp(&pid.to_string()), pid, "a terminal's Ctrl-C does not reach the game");
    assert_ne!(pgrp("self"), pid);
    rig.sup.shutdown_all(Duration::from_secs(10)).await;
}

/// Frames that arrive within `d`.
async fn drain_for(sock: &Sock, d: Duration) -> Vec<ServerFrame> {
    let mut got = Vec::new();
    let end = tokio::time::Instant::now() + d;
    while let Ok(Some(f)) = tokio::time::timeout_at(end, sock.me.outbox.pop()).await {
        got.push(f);
    }
    got
}

fn is_delta(f: &ServerFrame) -> bool {
    matches!(f, ServerFrame::Game(ServerMsg::Delta(_)))
}

#[tokio::test]
async fn a_lobby_reply_that_overflows_the_queue_asks_the_game_for_a_resync() {
    let rig = rig("lobby-overflow", 600);
    let ann = rig.attach("ann");
    create(&rig, &ann).await;
    // No await in between: the game's frames cannot interleave, so the
    // overflow happens on a lobby reply, not on a game frame.
    while ann.me.outbox.len() < OUTBOX_CAP {
        rig.lobby(&ann, LobbyMsg::ListGames);
    }
    rig.lobby(&ann, LobbyMsg::ListGames);
    assert!(ann.me.outbox.is_empty(), "the full queue was cleared");
    assert_eq!(ann.me.outbox.push(delta(1)), Pushed::Dropped, "waiting for a fresh view");
    let got = until(&ann, is_view).await;
    assert!(!got.iter().any(is_delta), "no delta before the new base: {got:?}");
    let more = drain_for(&ann, Duration::from_secs(1)).await;
    assert_eq!(more.iter().filter(|f| is_view(f)).count(), 0, "one view, not a stream of them: {more:?}");
    assert!(more.iter().any(is_delta), "deltas flow again");
    rig.sup.shutdown_all(Duration::from_secs(10)).await;
}

#[tokio::test]
async fn a_socket_that_takes_over_a_game_waits_for_its_base_view() {
    let rig = rig("takeover-base", 600);
    let first = rig.attach("ann");
    let id = create(&rig, &first).await;
    let second = rig.attach("ann");
    assert_eq!(second.me.outbox.push(delta(1)), Pushed::Dropped, "no delta before the base view");
    let got = until(&second, is_view).await;
    assert_eq!(got[0], ServerFrame::Lobby(LobbyReply::Joined { game: id, you: s("ann") }));
    assert!(!got.iter().any(is_delta), "{got:?}");
    rig.sup.shutdown_all(Duration::from_secs(10)).await;
}

#[tokio::test]
async fn a_fresh_socket_outside_a_game_takes_frames_at_once() {
    let rig = rig("fresh-outbox", 600);
    let ann = rig.attach("ann");
    assert_eq!(ann.me.outbox.push(delta(1)), Pushed::Queued);
}

#[tokio::test]
async fn joining_the_game_you_are_in_resyncs_without_pausing_it() {
    let rig = rig("rejoin", 600);
    let ann = rig.attach("ann");
    let id = create(&rig, &ann).await;
    rig.wait_for(&id, |g| g.sim_time > 25201.0).await;
    rig.lobby(&ann, LobbyMsg::Join { game: id.clone() });
    let got = until(&ann, is_view).await;
    assert!(got.contains(&ServerFrame::Lobby(LobbyReply::Joined { game: id.clone(), you: s("ann") })), "{got:?}");
    let Some(ServerFrame::Game(ServerMsg::View(v))) = got.last() else { unreachable!() };
    assert!(!v.paused, "a solo player's re-join did not pause the game");
    assert_eq!(rig.sup.game_of("ann").as_deref(), Some(id.as_str()));
    sleep(Duration::from_millis(1500)).await;
    let status = rig.sup.status(&id).expect("running");
    assert!(!status.paused, "still running a status later: {status:?}");
    rig.sup.shutdown_all(Duration::from_secs(10)).await;
}

// ---- deleting games (owner decision 13) ----

fn is_games(f: &ServerFrame) -> bool {
    matches!(f, ServerFrame::Lobby(LobbyReply::Games { .. }))
}

/// The next games list this socket gets.
async fn games_list(sock: &Sock) -> Vec<GameInfo> {
    let got = until(sock, is_games).await;
    let Some(ServerFrame::Lobby(LobbyReply::Games { games })) = got.last() else { unreachable!() };
    games.clone()
}

async fn listed(rig: &Rig, sock: &Sock) -> Vec<GameInfo> {
    rig.lobby(sock, LobbyMsg::ListGames);
    games_list(sock).await
}

fn save_files(rig: &Rig, id: &str) -> Vec<String> {
    std::fs::read_dir(rig.saves())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(id))
        .collect()
}

#[tokio::test]
async fn the_creator_deletes_their_saved_game_and_everyone_sees_it_go() {
    let rig = rig("delete", 1);
    let ann = rig.attach("ann");
    let id = create(&rig, &ann).await;
    let bob = rig.attach("bob");
    rig.lobby(&ann, LobbyMsg::DeleteGame { game: id.clone() });
    expect_error_eventually(&ann, codes::GAME_RUNNING).await;
    rig.lobby(&ann, LobbyMsg::Leave);
    games_list(&ann).await;
    let g = rig.wait_for(&id, |g| g.state == GameState::Saved).await;
    assert_eq!((g.creator.as_deref(), g.can_delete), (Some("ann"), false), "the plain list is nobody's");
    std::fs::write(rig.saves().join(format!("{id}.sqlite-wal")), "").unwrap();
    std::fs::write(rig.saves().join(format!("{id}.sqlite-shm")), "").unwrap();
    assert_eq!(listed(&rig, &ann).await[0].can_delete, true);
    assert_eq!(listed(&rig, &bob).await[0].can_delete, false);
    rig.lobby(&bob, LobbyMsg::DeleteGame { game: id.clone() });
    expect_error(&bob, codes::NOT_ALLOWED).await;
    assert_eq!(save_files(&rig, &id).len(), 3, "nothing was deleted");
    rig.lobby(&ann, LobbyMsg::DeleteGame { game: id.clone() });
    assert!(games_list(&ann).await.is_empty());
    assert!(games_list(&bob).await.is_empty(), "every client gets the new list");
    assert!(save_files(&rig, &id).is_empty(), "the save and its -wal and -shm are gone");
    rig.lobby(&ann, LobbyMsg::Join { game: id.clone() });
    expect_error(&ann, codes::UNKNOWN_GAME).await;
}

/// Errors can queue behind frames of the game the socket is in.
async fn expect_error_eventually(sock: &Sock, code: &str) {
    let got = until(sock, |f| error_code(f).is_some()).await;
    assert_eq!(error_code(got.last().unwrap()), Some(code), "{got:?}");
}

#[tokio::test]
async fn an_admin_deletes_anyones_game_and_saves_from_before_creators() {
    let rig = rig("delete-admin", 600);
    let legacy = "g-dddddddddddd";
    let json = std::fs::read_to_string(TWOBOX).unwrap();
    drop(Game::create(&rig.saves().join(format!("{legacy}.sqlite")), &json, GameMeta { layout: s("twobox"), seed: 2 }).unwrap());
    let ann = rig.attach("ann");
    let root = rig.attach("root");
    let g = &listed(&rig, &ann).await[0];
    assert_eq!((g.creator.clone(), g.can_delete), (None, false), "a save without a creator: admins only");
    assert!(listed(&rig, &root).await[0].can_delete);
    rig.lobby(&ann, LobbyMsg::DeleteGame { game: s(legacy) });
    expect_error(&ann, codes::NOT_ALLOWED).await;
    rig.lobby(&root, LobbyMsg::DeleteGame { game: s(legacy) });
    assert!(games_list(&root).await.is_empty());
    assert!(save_files(&rig, legacy).is_empty());
}

#[tokio::test]
async fn a_running_game_is_never_deleted_not_even_by_an_admin() {
    let rig = rig("delete-running", 600);
    let ann = rig.attach("ann");
    let id = create(&rig, &ann).await;
    let root = rig.attach("root");
    rig.lobby(&root, LobbyMsg::DeleteGame { game: id.clone() });
    expect_error(&root, codes::GAME_RUNNING).await;
    assert!(!listed(&rig, &root).await[0].can_delete);
    assert_eq!(rig.info(&id).state, GameState::Running);
    assert!(rig.saves().join(format!("{id}.sqlite")).exists());
    rig.sup.shutdown_all(Duration::from_secs(10)).await;
}

#[tokio::test]
async fn a_crashed_game_is_deleted_with_its_entry() {
    let rig = rig("delete-crashed", 600);
    let bad = "g-cccccccccccc";
    std::fs::write(rig.saves().join(format!("{bad}.sqlite")), "this is not a database").unwrap();
    let ann = rig.attach("ann");
    rig.lobby(&ann, LobbyMsg::Join { game: s(bad) });
    until(&ann, |f| *f == notice(Notice::GameCrashed)).await;
    assert_eq!(rig.info(bad).state, GameState::Crashed);
    let root = rig.attach("root");
    rig.lobby(&root, LobbyMsg::DeleteGame { game: s(bad) });
    assert!(games_list(&root).await.is_empty(), "neither the file nor the crash is listed");
    assert!(save_files(&rig, bad).is_empty());
}

#[tokio::test]
async fn deleting_checks_the_id_like_join() {
    let rig = rig("delete-ids", 600);
    let victim = rig.root.join("victim.sqlite");
    std::fs::write(&victim, "keep me").unwrap();
    std::fs::write(rig.saves().join("notes.sqlite"), "not a game id").unwrap();
    let root = rig.attach("root");
    for id in ["../victim", "g-../../victim", "notes", "", "g-aaaaaaaaaaaa", "g-AAAAAAAAAAAA"] {
        rig.lobby(&root, LobbyMsg::DeleteGame { game: s(id) });
        expect_error(&root, codes::UNKNOWN_GAME).await;
    }
    assert!(victim.exists() && rig.saves().join("notes.sqlite").exists());
}

// ---- tutorials (tutorial spec §3) ----

const LESSON_1: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../lessons/01-reading-the-panel");

/// A lessons directory: lesson 1, a broken lesson, a name that is no id,
/// and a stray file.
fn lessons_dir(root: &Path) -> PathBuf {
    let dir = root.join("lessons");
    let one = dir.join("01-reading-the-panel");
    std::fs::create_dir_all(&one).unwrap();
    for f in ["lesson.json", "world.json"] {
        std::fs::copy(Path::new(LESSON_1).join(f), one.join(f)).unwrap();
    }
    let broken = dir.join("02-broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::copy(Path::new(LESSON_1).join("world.json"), broken.join("world.json")).unwrap();
    std::fs::write(broken.join("lesson.json"), r#"{"schema": 1, "title": "B", "area": "Nowhere", "start": "08:00", "steps": []}"#).unwrap();
    std::fs::create_dir_all(dir.join("Not An Id")).unwrap();
    std::fs::write(dir.join("README"), "ignored").unwrap();
    dir
}

fn lesson_rig(name: &str) -> Rig {
    let root = temp_dir(name);
    let layouts = Layouts::load(&layouts_dir(&root)).unwrap();
    let lessons = Lessons::load(&lessons_dir(&root));
    let cfg = SupervisorConfig {
        game_bin: PathBuf::from(GAME_BIN),
        saves_dir: root.join("data/saves"),
        sockets_dir: root.join("data/sockets"),
        empty_exit_s: 600,
        admins: BTreeSet::from([s("root")]),
    };
    Rig { sup: Supervisor::with_lessons(cfg, layouts, lessons).unwrap(), root }
}

/// Start lesson 1 as `sock`'s user; its id once the first `lesson` came.
async fn start_lesson(rig: &Rig, sock: &Sock) -> String {
    rig.lobby(sock, LobbyMsg::StartLesson { lesson: s("01-reading-the-panel") });
    let got = until(sock, |f| matches!(f, ServerFrame::Game(ServerMsg::Lesson(_)))).await;
    let Some(ServerFrame::Lobby(LobbyReply::Joined { game, .. })) = got.first() else { panic!("{got:?}") };
    assert!(got.iter().any(|f| matches!(f, ServerFrame::Game(ServerMsg::Layout(l)) if l.area.as_deref() == Some("Saltmarsh"))));
    game.clone()
}

async fn until_gone(rig: &Rig, id: &str) {
    for _ in 0..200 {
        if rig.sup.status(id).is_none() && rig.sup.pid(id).is_none() && rig.sup.live_count() == 0 {
            return;
        }
        sleep(Duration::from_millis(50)).await;
    }
    panic!("tutorial {id} never ended");
}

#[test]
fn only_valid_lessons_are_offered_and_each_one_left_out_says_why() {
    let root = temp_dir("lessons-scan");
    let dir = lessons_dir(&root);
    let (lessons, left_out) = Lessons::scan(&dir);
    assert_eq!(lessons.infos(), [LessonInfo { id: s("01-reading-the-panel"), title: s("Reading the panel"), steps: 10 }]);
    assert_eq!(lessons.path("01-reading-the-panel"), Some(dir.join("01-reading-the-panel")));
    assert_eq!((lessons.path("02-broken"), lessons.path("../lessons/01-reading-the-panel")), (None, None));
    assert_eq!(left_out, [s("lesson 02-broken left out: lesson.json: a lesson has 1 to 60 steps")]);
    let (none, why) = Lessons::scan(&root.join("absent"));
    assert!(none.infos().is_empty());
    assert_eq!(why.len(), 1);
    assert!(why[0].starts_with("no lessons at "), "{why:?}");
}

#[tokio::test]
async fn the_lobby_lists_lessons_and_refuses_unknown_ones() {
    let rig = lesson_rig("lesson-lobby");
    let ann = rig.attach("ann");
    rig.lobby(&ann, LobbyMsg::ListLessons);
    let f = next(&ann).await.unwrap();
    let ServerFrame::Lobby(LobbyReply::Lessons { lessons }) = f else { panic!("{f:?}") };
    assert_eq!(lessons.len(), 1);
    rig.lobby(&ann, LobbyMsg::StartLesson { lesson: s("02-broken") });
    expect_error(&ann, codes::UNKNOWN_LESSON).await;
    let robot = rig.attach("Robot");
    rig.lobby(&robot, LobbyMsg::StartLesson { lesson: s("01-reading-the-panel") });
    expect_error(&robot, codes::RESERVED_NAME).await;
}

#[tokio::test]
async fn a_tutorial_is_private_unlisted_and_ends_when_its_player_leaves() {
    let rig = lesson_rig("lesson-private");
    let ann = rig.attach("ann");
    let id = start_lesson(&rig, &ann).await;
    assert!(valid_game_id(&id));
    assert!(rig.sup.list_games().is_empty(), "never listed");
    let bob = rig.attach("bob");
    rig.lobby(&bob, LobbyMsg::Join { game: id.clone() });
    expect_error(&bob, codes::UNKNOWN_GAME).await;
    rig.lobby(&bob, LobbyMsg::DeleteGame { game: id.clone() });
    expect_error(&bob, codes::UNKNOWN_GAME).await;
    let root = rig.attach("root");
    rig.lobby(&root, LobbyMsg::DeleteGame { game: id.clone() });
    expect_error(&root, codes::UNKNOWN_GAME).await;
    assert_eq!(rig.sup.game_of("bob"), None);
    rig.game_msg(&ann, ClientMsg::LessonNext);
    until(&ann, |f| matches!(f, ServerFrame::Game(ServerMsg::Lesson(v)) if v.index == 1)).await;
    rig.lobby(&ann, LobbyMsg::Leave);
    until_gone(&rig, &id).await;
    assert_eq!(std::fs::read_dir(rig.saves()).unwrap().count(), 0, "nothing saved");
}

#[tokio::test]
async fn a_dropped_socket_can_come_back_to_its_tutorial() {
    let rig = lesson_rig("lesson-back");
    let ann = rig.attach("ann");
    let id = start_lesson(&rig, &ann).await;
    rig.game_msg(&ann, ClientMsg::LessonNext);
    until(&ann, |f| matches!(f, ServerFrame::Game(ServerMsg::Lesson(v)) if v.index == 1)).await;
    rig.sup.detach("ann", ann.me.conn);
    let ann = rig.attach("ann");
    rig.lobby(&ann, LobbyMsg::Join { game: id.clone() });
    let got = until(&ann, |f| matches!(f, ServerFrame::Game(ServerMsg::Lesson(_)))).await;
    let Some(ServerFrame::Game(ServerMsg::Lesson(v))) = got.last() else { unreachable!() };
    assert_eq!(v.index, 1, "where ann left it");
}

#[tokio::test]
async fn starting_another_game_ends_your_tutorial() {
    let rig = lesson_rig("lesson-switch");
    let ann = rig.attach("ann");
    let id = start_lesson(&rig, &ann).await;
    let game = create(&rig, &ann).await;
    for _ in 0..200 {
        if rig.sup.pid(&id).is_none() {
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(rig.sup.pid(&id), None, "the tutorial ended");
    assert_eq!(rig.sup.game_of("ann").as_deref(), Some(game.as_str()));
}

#[tokio::test]
async fn tutorials_have_their_own_cap() {
    let rig = lesson_rig("lesson-cap");
    let mut socks = Vec::new();
    for i in 0..MAX_TUTORIALS {
        let sock = rig.attach(&format!("p{i}"));
        start_lesson(&rig, &sock).await;
        socks.push(sock);
    }
    let late = rig.attach("late");
    rig.lobby(&late, LobbyMsg::StartLesson { lesson: s("01-reading-the-panel") });
    let f = next(&late).await.unwrap();
    let ServerFrame::Lobby(LobbyReply::Error { code, message }) = f else { panic!("{f:?}") };
    assert_eq!((code.as_str(), message.as_str()), (codes::TOO_MANY_GAMES, "at most 8 tutorials run at once"));
    create(&rig, &late).await;
    assert_eq!(rig.sup.list_games().len(), 1, "a real game still starts, and only it is listed");
}

#[tokio::test]
async fn a_crashed_tutorial_is_simply_gone() {
    let rig = lesson_rig("lesson-crash");
    let ann = rig.attach("ann");
    let id = start_lesson(&rig, &ann).await;
    let pid = loop {
        if let Some(p) = rig.sup.pid(&id) {
            break p;
        }
        sleep(Duration::from_millis(20)).await;
    };
    kill(pid, "KILL");
    until(&ann, |f| matches!(f, ServerFrame::Game(ServerMsg::Notice(Notice::GameCrashed)))).await;
    until_gone(&rig, &id).await;
    assert!(rig.sup.list_games().is_empty(), "not listed as crashed");
    assert_eq!(rig.sup.game_of("ann"), None);
}
