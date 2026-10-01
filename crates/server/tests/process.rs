//! The game process: `Shell` decisions in process, then the real
//! `signalbox-game` binary over a real Unix socket.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use game::{Game, GameMeta};
use ipc::{FromGame, LogLevel, ToGame, read_frame, write_frame};
use protocol::{ClientMsg, ServerMsg};
use server::process::*;
use signalbox_core::world::World;
use tokio::net::UnixStream;
use tokio::process::{Child, Command};
use tokio::time::timeout;

const TWOBOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../game/tests/fixtures/twobox.json");
const GAME_BIN: &str = env!("CARGO_BIN_EXE_signalbox-game");

fn s(x: &str) -> String {
    x.to_string()
}

fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sbx-proc-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn twobox_json() -> String {
    std::fs::read_to_string(TWOBOX).unwrap()
}

fn meta() -> GameMeta {
    GameMeta { layout: s("twobox"), seed: 1 }
}

fn saved_ticks(out: &[FromGame]) -> Vec<u64> {
    out.iter()
        .filter_map(|m| match m {
            FromGame::Saved { tick } => Some(*tick),
            _ => None,
        })
        .collect()
}

#[test]
fn arguments_parse_and_bad_ones_are_explained() {
    let a = Args::parse(&args(&["--save", "/d/g.sqlite", "--socket", "/d/g.sock"])).unwrap();
    assert_eq!((a.save, a.socket, a.empty_exit, a.create), (PathBuf::from("/d/g.sqlite"), PathBuf::from("/d/g.sock"), Duration::from_secs(600), None));
    let a = Args::parse(&args(&[
        "--save", "g.sqlite", "--socket", "g.sock", "--create", "--layout", "w.json", "--layout-name", "drain", "--seed", "9",
        "--start", "6:5", "--empty-exit-s", "2",
    ]))
    .unwrap();
    assert_eq!(a.empty_exit, Duration::from_secs(2));
    assert_eq!(
        a.create,
        Some(CreateArgs {
            world: PathBuf::from("w.json"),
            layout_name: s("drain"),
            seed: 9,
            start: Some(s("06:05:00")),
            creator: None,
            seed_budget: SEED_BUDGET,
        })
    );
    let err = |v: &[&str]| Args::parse(&args(v)).unwrap_err();
    assert_eq!(err(&["--socket", "x"]), "--save is required");
    assert_eq!(err(&["--save", "x"]), "--socket is required");
    assert_eq!(err(&["--save"]), "--save needs a value");
    assert_eq!(err(&["--save", "x", "--socket", "y", "--create"]), "--create needs --layout");
    assert_eq!(
        err(&["--save", "x", "--socket", "y", "--seed", "1"]),
        "--layout, --layout-name, --seed, --start and --creator need --create"
    );
    assert_eq!(
        err(&["--save", "x", "--socket", "y", "--creator", "ann"]),
        "--layout, --layout-name, --seed, --start and --creator need --create"
    );
    let a = Args::parse(&args(&[
        "--save", "g.sqlite", "--socket", "g.sock", "--create", "--layout", "w.json", "--layout-name", "drain", "--seed", "9",
        "--creator", "Hackney & Bow's ann",
    ]))
    .unwrap();
    assert_eq!(a.create.unwrap().creator.as_deref(), Some("Hackney & Bow's ann"));
    assert_eq!(err(&["--save", "x", "--socket", "y", "--frobnicate"]), "unknown argument `--frobnicate`");
    assert_eq!(err(&["--save", "x", "--socket", "y", "--create", "--start", "24:00"]), "bad --start `24:00`");
    assert_eq!(err(&["--save", "x", "--socket", "y", "--seed-budget-ms", "5"]), "--seed-budget-ms needs --create");
    assert_eq!(err(&["--socket", "y", "--lesson", "l", "--seed-budget-ms", "5"]), "--lesson takes only --socket and --empty-exit-s");
    let a = Args::parse(&args(&[
        "--save", "g.sqlite", "--socket", "g.sock", "--create", "--layout", "w.json", "--layout-name", "drain", "--seed", "9",
        "--seed-budget-ms", "250",
    ]))
    .unwrap();
    assert_eq!(a.create.unwrap().seed_budget, Duration::from_millis(250));
    assert_eq!(SEED_BUDGET, Duration::from_secs(60), "timetables spec P8");
    assert_eq!(exit_status("seed_too_slow: preparing took too long"), EXIT_SEED_TOO_SLOW);
    assert_eq!(exit_status("create: bad world"), 1);
}

#[test]
fn start_times_are_normalised_and_bounded() {
    assert_eq!(normalise_start("06:30").as_deref(), Some("06:30:00"));
    assert_eq!(normalise_start("6:5:3").as_deref(), Some("06:05:03"));
    assert_eq!(normalise_start("23:59:59").as_deref(), Some("23:59:59"));
    for bad in ["24:00", "25:00", "", "aa:bb", "06", "06:00:00:00", "-1:00", "06:60"] {
        assert_eq!(normalise_start(bad), None, "{bad}");
    }
}

#[test]
fn set_start_time_rewrites_only_the_start() {
    let json = set_start_time(&twobox_json(), "08:15:00").unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["options"]["start_time"], "08:15:00");
    let mut before: serde_json::Value = serde_json::from_str(&twobox_json()).unwrap();
    before["options"]["start_time"] = "08:15:00".into();
    assert_eq!(v, before);
    assert_eq!(set_start_time(r#"{"areas": []}"#, "07:00:00").unwrap(), r#"{"areas":[],"options":{"start_time":"07:00:00"}}"#);
    assert!(set_start_time("[1]", "07:00:00").is_err());
    assert!(set_start_time(r#"{"options": 3}"#, "07:00:00").is_err());
}

fn shell(empty_exit_s: u64) -> Shell {
    Shell::new(Game::new(World::from_json(&twobox_json()).unwrap(), meta()), Duration::from_secs(empty_exit_s))
}

fn saved_shell(dir: &Path, empty_exit_s: u64) -> Shell {
    let game = Game::create(&dir.join("g.sqlite"), &twobox_json(), meta()).unwrap();
    Shell::new(game, Duration::from_secs(empty_exit_s))
}

fn kinds(out: &[FromGame]) -> Vec<String> {
    out.iter()
        .map(|m| match m {
            FromGame::ToPlayer { player, msg } => {
                let t = serde_json::to_value(msg).unwrap()["type"].as_str().unwrap().to_string();
                format!("{player}:{t}")
            }
            FromGame::Status(_) => s("status"),
            FromGame::Saved { tick } => format!("saved:{tick}"),
            FromGame::Log { level, .. } => format!("log:{level:?}"),
        })
        .collect()
}

#[test]
fn the_shell_relays_players_and_their_messages() {
    let mut sh = shell(600);
    let (out, next) = sh.on_frame(ToGame::Connect { player: s("ann") });
    assert_eq!((kinds(&out), next), (vec![s("ann:layout"), s("ann:view")], Next::Continue));
    let (out, _) = sh.on_frame(ToGame::Client { player: s("ann"), msg: ClientMsg::Claim { area: s("West") } });
    assert_eq!(kinds(&out), [s("ann:layout"), s("ann:view")]);
    assert_eq!(sh.game().holder("West"), Some("ann"));
    let (out, next) = sh.on_advance(1.0);
    assert_eq!((out.len(), next), (0, Next::Continue));
    assert_eq!(sh.game().sim().tick(), 10);
    assert_eq!(kinds(&sh.on_flush()), [s("ann:delta")]);
    let FromGame::Status(st) = sh.status() else { panic!("not a status") };
    assert_eq!((st.tick, st.players.len(), st.holders["West"].as_deref()), (10, 1, Some("ann")));
}

#[test]
fn the_last_player_leaving_pauses_and_saves() {
    let dir = temp_dir("leave");
    let mut sh = saved_shell(&dir, 600);
    sh.on_frame(ToGame::Connect { player: s("ann") });
    sh.on_frame(ToGame::Connect { player: s("bob") });
    sh.on_advance(1.0);
    let (out, _) = sh.on_frame(ToGame::Disconnect { player: s("bob") });
    assert!(out.is_empty(), "ann is still here");
    assert!(!sh.game().clock().paused);
    let (out, next) = sh.on_frame(ToGame::Disconnect { player: s("ann") });
    assert_eq!((saved_ticks(&out), next), (vec![10], Next::Continue));
    assert!(sh.game().clock().paused);
    let (out, _) = sh.on_frame(ToGame::Disconnect { player: s("ann") });
    assert!(out.is_empty(), "a repeated disconnect saves nothing more");
}

#[test]
fn an_empty_game_saves_and_exits_after_the_empty_time() {
    let dir = temp_dir("empty");
    let mut sh = saved_shell(&dir, 5);
    assert_eq!(sh.on_advance(2.0).1, Next::Continue);
    sh.on_frame(ToGame::Connect { player: s("ann") });
    assert_eq!(sh.on_advance(10.0).1, Next::Continue, "somebody is here");
    let (out, _) = sh.on_frame(ToGame::Disconnect { player: s("ann") });
    let tick = sh.game().sim().tick();
    assert_eq!(saved_ticks(&out), [tick], "saved as the last player left");
    assert_eq!(sh.on_advance(4.0).1, Next::Continue, "the empty clock restarted");
    let (out, next) = sh.on_advance(1.0);
    assert_eq!(next, Next::Exit);
    assert!(out.is_empty(), "paused since, so nothing new to report: {out:?}");
    assert_eq!(game::save::read_summary(&dir.join("g.sqlite")).unwrap().tick, tick);
}

#[test]
fn shutdown_saves_and_exits_and_save_failures_are_logged() {
    let dir = temp_dir("shutdown");
    let mut sh = saved_shell(&dir, 600);
    sh.on_advance(0.5);
    let (out, next) = sh.on_frame(ToGame::Shutdown);
    assert_eq!((saved_ticks(&out), next), (vec![5], Next::Exit));

    let dir = temp_dir("shutdown-fail");
    let mut sh = saved_shell(&dir, 600);
    rusqlite::Connection::open(dir.join("g.sqlite")).unwrap().execute("DROP TABLE snapshots", []).unwrap();
    let (out, next) = sh.on_frame(ToGame::Shutdown);
    assert_eq!(next, Next::Exit);
    assert!(saved_ticks(&out).is_empty());
    let FromGame::Log { level, message } = &out[0] else { panic!("{out:?}") };
    assert_eq!(*level, LogLevel::Error);
    assert!(message.starts_with("save failed: "), "{message}");
}

// ---- the real binary ----

fn spawn(argv: &[String]) -> Child {
    Command::new(GAME_BIN)
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}

async fn connect(sock: &Path) -> UnixStream {
    for _ in 0..500 {
        if let Ok(s) = UnixStream::connect(sock).await {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the game never listened on {}", sock.display());
}

/// Frames until `stop` matches one (included), within 10 s.
async fn read_until(rd: &mut UnixStream, stop: impl Fn(&FromGame) -> bool) -> Vec<FromGame> {
    let mut got = Vec::new();
    loop {
        let m: FromGame = timeout(Duration::from_secs(10), read_frame(rd)).await.expect("timed out").unwrap().expect("eof");
        let done = stop(&m);
        got.push(m);
        if done {
            return got;
        }
    }
}

async fn exit_code(child: &mut Child) -> Option<i32> {
    timeout(Duration::from_secs(10), child.wait()).await.expect("the game did not exit").unwrap().code()
}

async fn stderr_of(child: &mut Child) -> String {
    use tokio::io::AsyncReadExt;
    let mut text = String::new();
    child.stderr.take().unwrap().read_to_string(&mut text).await.unwrap();
    text
}

fn create_args(dir: &Path, extra: &[&str]) -> Vec<String> {
    let mut v = args(&[
        "--save", dir.join("g.sqlite").to_str().unwrap(), "--socket", dir.join("g.sock").to_str().unwrap(),
        "--create", "--layout", TWOBOX, "--layout-name", "twobox", "--seed", "3",
    ]);
    v.extend(args(extra));
    v
}

#[tokio::test]
async fn create_play_status_and_shutdown() {
    let dir = temp_dir("bin-create");
    let mut child = spawn(&create_args(&dir, &["--start", "08:00"]));
    let mut sock = connect(&dir.join("g.sock")).await;
    write_frame(&mut sock, &ToGame::Connect { player: s("ann") }).await.unwrap();
    let got = read_until(&mut sock, |m| matches!(m, FromGame::ToPlayer { msg: ServerMsg::View(_), .. })).await;
    assert!(!dir.join("g.sock").exists(), "the socket file goes once the front is connected");
    let Some(FromGame::ToPlayer { msg: ServerMsg::View(v), .. }) = got.last() else { unreachable!() };
    assert!((28800.0..28802.0).contains(&v.sim_time), "the start time was applied: {}", v.sim_time);
    read_until(&mut sock, |m| matches!(m, FromGame::Status(_))).await;
    write_frame(&mut sock, &ToGame::Shutdown).await.unwrap();
    let got = read_until(&mut sock, |m| matches!(m, FromGame::Saved { .. })).await;
    assert!(!saved_ticks(&got).is_empty());
    let end: Option<FromGame> = read_frame(&mut sock).await.unwrap();
    assert!(end.is_none(), "{end:?}");
    assert_eq!(exit_code(&mut child).await, Some(0));
    let sum = game::save::read_summary(&dir.join("g.sqlite")).unwrap();
    assert_eq!((sum.layout.as_str(), sum.seed), ("twobox", 3));
    assert!(sum.sim_time >= 28800.0);
}

#[tokio::test]
async fn a_resumed_game_left_empty_saves_and_exits() {
    let dir = temp_dir("bin-empty");
    drop(Game::create(&dir.join("g.sqlite"), &twobox_json(), meta()).unwrap());
    let mut child = spawn(&args(&[
        "--save", dir.join("g.sqlite").to_str().unwrap(), "--socket", dir.join("g.sock").to_str().unwrap(),
        "--empty-exit-s", "1",
    ]));
    let mut sock = connect(&dir.join("g.sock")).await;
    write_frame(&mut sock, &ToGame::Connect { player: s("ann") }).await.unwrap();
    read_until(&mut sock, |m| matches!(m, FromGame::ToPlayer { msg: ServerMsg::View(_), .. })).await;
    write_frame(&mut sock, &ToGame::Disconnect { player: s("ann") }).await.unwrap();
    assert_eq!(exit_code(&mut child).await, Some(0), "exits about a second after the last player left");
}

#[tokio::test]
async fn sigterm_saves_and_exits_cleanly() {
    let dir = temp_dir("bin-term");
    let mut child = spawn(&create_args(&dir, &[]));
    let mut sock = connect(&dir.join("g.sock")).await;
    read_until(&mut sock, |m| matches!(m, FromGame::Status(_))).await;
    let pid = child.id().unwrap();
    // `kill` is a shell builtin; the slim Rust image has no /bin/kill.
    let sent = std::process::Command::new("sh").args(["-c", &format!("kill -TERM {pid}")]).status().unwrap();
    assert!(sent.success());
    let got = read_until(&mut sock, |m| matches!(m, FromGame::Saved { .. })).await;
    assert!(!saved_ticks(&got).is_empty());
    assert_eq!(exit_code(&mut child).await, Some(0));
}

#[tokio::test]
async fn the_front_going_away_saves_and_exits() {
    let dir = temp_dir("bin-gone");
    let mut child = spawn(&create_args(&dir, &[]));
    let sock = connect(&dir.join("g.sock")).await;
    drop(sock);
    assert_eq!(exit_code(&mut child).await, Some(0));
}

#[tokio::test]
async fn a_save_that_does_not_resume_exits_non_zero_with_a_message() {
    let dir = temp_dir("bin-bad");
    std::fs::write(dir.join("g.sqlite"), "this is not a database").unwrap();
    let mut child = spawn(&args(&["--save", dir.join("g.sqlite").to_str().unwrap(), "--socket", dir.join("g.sock").to_str().unwrap()]));
    let err = stderr_of(&mut child).await;
    assert_eq!(exit_code(&mut child).await, Some(1));
    assert!(err.starts_with("signalbox-game: "), "{err}");
    assert!(!dir.join("g.sock").exists(), "never listened");
}

#[tokio::test]
async fn bad_arguments_exit_2_with_usage() {
    let mut child = spawn(&args(&["--save"]));
    let err = stderr_of(&mut child).await;
    assert_eq!(exit_code(&mut child).await, Some(2));
    assert!(err.contains("usage: signalbox-game"), "{err}");
}

#[tokio::test]
async fn a_client_command_flows_through_the_process() {
    let dir = temp_dir("bin-flow");
    let mut child = spawn(&create_args(&dir, &[]));
    let mut sock = connect(&dir.join("g.sock")).await;
    write_frame(&mut sock, &ToGame::Connect { player: s("ann") }).await.unwrap();
    write_frame(&mut sock, &ToGame::Client { player: s("ann"), msg: ClientMsg::Claim { area: s("West") } }).await.unwrap();
    let got = read_until(&mut sock, |m| matches!(m, FromGame::Status(st) if st.holders["West"].as_deref() == Some("ann"))).await;
    assert!(got.iter().any(|m| matches!(m, FromGame::ToPlayer { msg: ServerMsg::Delta(_), .. })), "deltas flow");
    write_frame(&mut sock, &ToGame::Shutdown).await.unwrap();
    assert_eq!(exit_code(&mut child).await, Some(0));
}

#[test]
fn status_carries_the_counters_and_save_time_in_milliseconds() {
    let st = game::GameStatus {
        sim_time: 3600.0,
        tick: 7,
        paused: true,
        speed: 2,
        holders: [(s("West"), Some(s("ann"))), (s("East"), None)].into_iter().collect(),
        players: vec![(s("ann"), true), (s("bob"), false)],
        connected: 1,
        stats: game::GameStats { spads: 1, collisions: 2, invariant_violations: 3, player_commands: 4, robot_commands: 5, sim_rejections: 6 },
        save_busy: Duration::from_micros(1_234_999),
    };
    let m = status_msg(&st);
    assert_eq!((m.sim_time, m.tick, m.paused, m.speed), (3600.0, 7, true, 2));
    assert_eq!(m.holders, st.holders);
    assert_eq!(m.players, [ipc::PlayerStatus { name: s("ann"), connected: true }, ipc::PlayerStatus { name: s("bob"), connected: false }]);
    assert_eq!(
        m.counters,
        ipc::Counters { spads: 1, collisions: 2, invariant_violations: 3, player_commands: 4, robot_commands: 5, save_busy_ms: 1234 }
    );
}

#[test]
fn a_player_who_joins_and_leaves_between_advances_restarts_the_empty_clock() {
    let dir = temp_dir("blip");
    let mut sh = saved_shell(&dir, 5);
    assert_eq!(sh.on_advance(4.9).1, Next::Continue);
    sh.on_frame(ToGame::Connect { player: s("ann") });
    sh.on_frame(ToGame::Disconnect { player: s("ann") });
    assert_eq!(sh.on_advance(0.2).1, Next::Continue, "somebody was here since the last advance");
}

// ---- tutorials (tutorial spec §3) ----

const LESSON_1: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../lessons/01-reading-the-panel");

#[test]
fn lesson_arguments_take_no_save() {
    let a = Args::parse(&args(&["--lesson", "/l/01-x", "--socket", "/d/t.sock", "--empty-exit-s", "60"])).unwrap();
    assert_eq!(a.lesson, Some(PathBuf::from("/l/01-x")));
    assert_eq!((a.save, a.create, a.empty_exit), (PathBuf::new(), None, Duration::from_secs(60)));
    let err = |v: &[&str]| Args::parse(&args(v)).unwrap_err();
    assert_eq!(err(&["--lesson", "x", "--socket", "y", "--save", "z"]), "--lesson takes only --socket and --empty-exit-s");
    assert_eq!(err(&["--lesson", "x", "--socket", "y", "--create"]), "--lesson takes only --socket and --empty-exit-s");
    assert_eq!(err(&["--lesson", "x"]), "--socket is required");
    assert_eq!(Args::parse(&args(&["--save", "g", "--socket", "s"])).unwrap().lesson, None);
}

fn lesson_shell(empty_exit_s: u64) -> Shell {
    let (game, runner) = open_lesson(Path::new(LESSON_1)).unwrap();
    Shell::lesson(game, runner, Duration::from_secs(empty_exit_s))
}

#[test]
fn a_lesson_shell_puts_its_player_in_the_lesson() {
    let mut sh = lesson_shell(60);
    let (out, next) = sh.on_frame(ToGame::Connect { player: s("ann") });
    assert_eq!(next, Next::Continue);
    assert!(out.iter().any(|m| matches!(m, FromGame::ToPlayer { msg: ServerMsg::Lesson(v), .. } if v.index == 0)));
    assert_eq!(sh.game().area_of("ann"), Some("Saltmarsh"));
    let (out, _) = sh.on_frame(ToGame::Client { player: s("ann"), msg: ClientMsg::LessonNext });
    assert!(out.iter().any(|m| matches!(m, FromGame::ToPlayer { msg: ServerMsg::Lesson(v), .. } if v.index == 1)));
    assert_eq!(sh.runner().unwrap().step(), 1);
}

#[test]
fn a_lesson_left_alone_neither_pauses_nor_saves_and_exits_after_the_empty_time() {
    let mut sh = lesson_shell(2);
    sh.on_frame(ToGame::Connect { player: s("ann") });
    sh.on_advance(0.5);
    let (out, _) = sh.on_frame(ToGame::Disconnect { player: s("ann") });
    assert!(out.is_empty(), "{out:?}");
    assert!(!sh.game().clock().paused, "a lesson's clock stops by itself, without a pause");
    let t = sh.game().sim().now_s();
    let (_, next) = sh.on_advance(1.0);
    assert_eq!((next, sh.game().sim().now_s()), (Next::Continue, t));
    let (out, next) = sh.on_advance(1.0);
    assert_eq!(next, Next::Exit);
    assert!(saved_ticks(&out).is_empty(), "nothing is saved");
}

#[tokio::test]
async fn the_binary_runs_a_lesson_and_writes_nothing() {
    let dir = temp_dir("bin-lesson");
    let sock_path = dir.join("t.sock");
    let mut child = spawn(&args(&["--lesson", LESSON_1, "--socket", sock_path.to_str().unwrap()]));
    let mut sock = connect(&sock_path).await;
    write_frame(&mut sock, &ToGame::Connect { player: s("ann") }).await.unwrap();
    let got = read_until(&mut sock, |m| matches!(m, FromGame::ToPlayer { msg: ServerMsg::Lesson(_), .. })).await;
    assert!(got.iter().any(|m| matches!(m, FromGame::ToPlayer { msg: ServerMsg::Layout(l), .. } if l.area.as_deref() == Some("Saltmarsh"))));
    write_frame(&mut sock, &ToGame::Shutdown).await.unwrap();
    assert_eq!(exit_code(&mut child).await, Some(0));
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "no save, and the socket is gone");
}

#[tokio::test]
async fn a_broken_lesson_exits_non_zero_with_its_reason() {
    let dir = temp_dir("bin-bad-lesson");
    let lesson = dir.join("01-broken");
    std::fs::create_dir_all(&lesson).unwrap();
    std::fs::write(lesson.join("lesson.json"), "{}").unwrap();
    let mut child = spawn(&args(&["--lesson", lesson.to_str().unwrap(), "--socket", dir.join("t.sock").to_str().unwrap()]));
    let err = stderr_of(&mut child).await;
    assert_eq!(exit_code(&mut child).await, Some(1));
    assert!(err.starts_with("signalbox-game: lesson "), "{err}");
    assert!(err.contains("01-broken"), "{err}");
}

// ---- preparing a later start (timetables spec §3.4) ----

fn preparing_of(m: &FromGame) -> Option<Option<protocol::Preparing>> {
    match m {
        FromGame::Status(st) => Some(st.preparing),
        _ => None,
    }
}

/// twobox starts at 07:00: a game created for 07:30 is prepared first,
/// reported as `preparing`; a player who connects meanwhile gets the game
/// at 07:30 once it is ready.
#[tokio::test]
async fn a_later_start_is_prepared_before_anyone_plays() {
    let dir = temp_dir("bin-seed");
    let mut child = spawn(&create_args(&dir, &["--start", "07:30", "--creator", "ann"]));
    let mut sock = connect(&dir.join("g.sock")).await;
    write_frame(&mut sock, &ToGame::Connect { player: s("ann") }).await.unwrap();
    let got = read_until(&mut sock, |m| matches!(m, FromGame::ToPlayer { msg: ServerMsg::View(_), .. })).await;
    let first = got.iter().find_map(preparing_of).expect("a status first");
    assert_eq!(first, Some(protocol::Preparing { from: 25200.0, to: 27000.0 }), "{got:?}");
    // Ready: a status without `preparing` comes before the player's frames.
    let ready = got.iter().position(|m| preparing_of(m) == Some(None)).expect("a ready status");
    let layout = got.iter().position(|m| matches!(m, FromGame::ToPlayer { msg: ServerMsg::Layout(_), .. })).unwrap();
    assert!(ready < layout, "{got:?}");
    let Some(FromGame::ToPlayer { msg: ServerMsg::View(v), .. }) = got.last() else { unreachable!() };
    assert!((27000.0..27001.0).contains(&v.sim_time), "the game opens at 07:30: {}", v.sim_time);
    write_frame(&mut sock, &ToGame::Shutdown).await.unwrap();
    assert_eq!(exit_code(&mut child).await, Some(0));
    assert!(stderr_of(&mut child).await.contains("prepared 07:00:00 to 07:30:00 in "));
    let sum = game::save::read_summary(&dir.join("g.sqlite")).unwrap();
    assert_eq!(sum.creator.as_deref(), Some("ann"));
    assert!(sum.sim_time >= 27000.0);
    assert!(!game::seed::temp_path(&dir.join("g.sqlite")).exists());
    // Resuming it does not prepare it again.
    let mut child = spawn(&args(&[
        "--save", dir.join("g.sqlite").to_str().unwrap(), "--socket", dir.join("g.sock").to_str().unwrap(),
    ]));
    let mut sock = connect(&dir.join("g.sock")).await;
    write_frame(&mut sock, &ToGame::Connect { player: s("ann") }).await.unwrap();
    let got = read_until(&mut sock, |m| matches!(m, FromGame::ToPlayer { msg: ServerMsg::View(_), .. })).await;
    assert!(got.iter().all(|m| preparing_of(m) != Some(Some(protocol::Preparing { from: 25200.0, to: 27000.0 }))));
    let Some(FromGame::ToPlayer { msg: ServerMsg::View(v), .. }) = got.last() else { unreachable!() };
    assert!(v.sim_time >= 27000.0, "{}", v.sim_time);
    write_frame(&mut sock, &ToGame::Shutdown).await.unwrap();
    assert_eq!(exit_code(&mut child).await, Some(0));
}

#[tokio::test]
async fn preparing_past_its_budget_fails_with_seed_too_slow_and_leaves_nothing() {
    let dir = temp_dir("bin-seed-slow");
    let mut child = spawn(&create_args(&dir, &["--start", "23:00", "--seed-budget-ms", "1"]));
    let _sock = connect(&dir.join("g.sock")).await;
    assert_eq!(exit_code(&mut child).await, Some(i32::from(EXIT_SEED_TOO_SLOW)));
    let err = stderr_of(&mut child).await;
    assert!(err.trim_end().lines().last().unwrap().starts_with("signalbox-game: seed_too_slow: preparing 07:00:00 to 23:00:00 took longer than 0.001 s"), "{err}");
    assert!(!dir.join("g.sqlite").exists());
    assert!(!game::seed::temp_path(&dir.join("g.sqlite")).exists());
}

#[tokio::test]
async fn a_shutdown_while_preparing_stops_and_leaves_nothing() {
    let dir = temp_dir("bin-seed-stop");
    let mut child = spawn(&create_args(&dir, &["--start", "23:59"]));
    let mut sock = connect(&dir.join("g.sock")).await;
    read_until(&mut sock, |m| preparing_of(m) == Some(Some(protocol::Preparing { from: 25200.0, to: 86340.0 }))).await;
    write_frame(&mut sock, &ToGame::Shutdown).await.unwrap();
    assert_eq!(exit_code(&mut child).await, Some(0));
    assert!(!dir.join("g.sqlite").exists());
    assert!(!game::seed::temp_path(&dir.join("g.sqlite")).exists());
}

/// A start before the world's still moves the world's start (unchanged).
#[tokio::test]
async fn an_earlier_start_is_not_prepared() {
    let dir = temp_dir("bin-early");
    let mut child = spawn(&create_args(&dir, &["--start", "06:00"]));
    let mut sock = connect(&dir.join("g.sock")).await;
    write_frame(&mut sock, &ToGame::Connect { player: s("ann") }).await.unwrap();
    let got = read_until(&mut sock, |m| matches!(m, FromGame::ToPlayer { msg: ServerMsg::View(_), .. })).await;
    assert!(got.iter().all(|m| preparing_of(m) != Some(Some(protocol::Preparing { from: 25200.0, to: 21600.0 }))));
    let Some(FromGame::ToPlayer { msg: ServerMsg::View(v), .. }) = got.last() else { unreachable!() };
    assert!((21600.0..21601.0).contains(&v.sim_time), "{}", v.sim_time);
    write_frame(&mut sock, &ToGame::Shutdown).await.unwrap();
    assert_eq!(exit_code(&mut child).await, Some(0));
    assert_eq!(game::save::read_summary(&dir.join("g.sqlite")).unwrap().tick, 0, "a snapshot at tick 0 only");
}
