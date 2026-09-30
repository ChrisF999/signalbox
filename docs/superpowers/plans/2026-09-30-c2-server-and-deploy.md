# signalbox multiplayer C2 — game process, front server, login and deployment — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put C1's in-process game on the network: a `signalbox-game` process per game behind a Unix-socket `ipc` link, a `signalbox-server` front (lobby, supervisor, WebSocket relay, limits, Authentik OIDC login, sessions, dev login for tests), a networked bot, end-to-end tests through the real processes, and the Docker/Authentik/tailnet deployment files — with the live deployment left as owner-gated controller steps.

**Architecture:** `ipc` is a tiny crate of front ⇄ game message enums and a length-prefixed JSON codec on tokio. A new `crates/server` package (lib `server`) holds both binaries: `signalbox-game` (module `server::process`, a tokio current-thread shell around the pure `game::Game`) and `signalbox-server` (the front: axum routes, `Supervisor` of game children, per-connection `Outbox`, OIDC, sessions). Both binaries live in one package so that its integration tests get both `CARGO_BIN_EXE_*` paths and one `cargo test -p signalbox-server` builds everything the end-to-end tests spawn; the `game` library stays sync and tokio-free. The bot gains a WebSocket client and a view-only strategy; the e2e tests start the front in process (`server::start`) and spawn real game processes.

**Tech Stack:** Rust 1.98 (edition 2024) via `scripts/cargo` (Docker). New: `tokio` 1 (current-thread runtimes only), `axum` 0.8 (`ws`), `axum-extra` 0.12 (`cookie-signed`), `openidconnect` 4 (`reqwest` + `rustls-tls`, no default features), `tokio-tungstenite` 0.29 (the version axum 0.8.9 already uses), `futures-util` 0.3, `base64` 0.22. No OpenSSL anywhere (checked: the resolved tree has `ring`/`rustls`, no `openssl-sys`, no `aws-lc-sys`).

**Spec:** `docs/superpowers/specs/2026-09-30-server-and-protocol-design.md` (sub-project C; §15 amendments). C1 plan (conventions, amendments 1–13): `docs/superpowers/plans/2026-09-30-c1-game-and-protocol.md`.

### Decisions (controller brief 2026-09-30 plus this plan's own)

1. **Where the binaries live.** New package `signalbox-server` (`crates/server`, lib `server`) with two bins: `signalbox-server` (the front) and `signalbox-game` (the game process). Cargo only sets `CARGO_BIN_EXE_<name>` for bins of the *same* package, and only builds a package's own bins for its tests; one package makes `cargo test -p signalbox-server` build and find both. The game process code is the module `server::process`; the front never calls into it except `process::normalise_start` (a time-string check) and the constant `process::EMPTY_EXIT_S`. `game` gains no tokio.
2. **The game process listens, the front connects** (as the brief says): the front spawns the child, then retries `UnixStream::connect` every 50 ms for up to 30 s, giving up early if the child exits. The game accepts exactly one connection, then removes its socket file.
3. **Client frames.** One socket carries lobby and game messages. `protocol::ClientFrame = Lobby(LobbyMsg) | Game(ClientMsg)` and `ServerFrame = Lobby(LobbyReply) | Game(ServerMsg)`, serialised untagged (each inner enum is already tagged by `"type"`) and deserialised by looking at `"type"` first. The two tag sets are disjoint (`list_games list_layouts create_game join leave` vs `claim release command vote resync`; `games layouts joined error` vs `layout view delta notice`), which a test pins. Looking at the tag first gives one precise error per bad frame (`unknown type` vs a field error) instead of serde's "did not match any variant".
4. **Status carries counters.** `ipc::FromGame::Status` includes `Counters {spads, collisions, invariant_violations, player_commands, robot_commands, save_busy_ms}` from `Game::status()`. The front stores them opaquely (it never interprets them) so the e2e test can assert "no SPAD / collision / invariant violation" for the robot's area too (area notices go only to holders, and the robot's area has none), and the long soak can report SQLite write cost (C1 parked item). `save_busy_ms` is measured inside `game::save` (the only module allowed to read clocks).
5. **Save errors reach the process shell.** `Game` records every save failure in a list as well as sending `save_failed` to connected players; `Game::take_save_errors()` drains it, and the shell sends each as `FromGame::Log {level: error}` and prints it to stderr, so failures with nobody connected are not lost.
6. **Game start time.** The front validates `create_game.start` with `server::process::normalise_start` (C1 amendment 11) and passes the normalised `HH:MM:SS` to the child as `--start`; the child rewrites `options.start_time` in the world JSON before `Game::create`. Valid starts are what `signalbox_core::time::parse_hms` accepts, below 24:00:00.
7. **Front state.** One `std::sync::Mutex` around the supervisor's maps, never held across an `.await`. Per-game ToGame channels are created when the game entry is created (state `Starting`), so `Connect` and client messages sent while the child is still starting queue in order and are delivered once connected. A second `join` of a starting game just joins it.
8. **Outbound queue (§4.3) is the front's `Outbox`:** a `VecDeque` of ≤ 64 frames + `Notify`. On overflow the queue is cleared, the game is sent `Client {player, Resync}`, and further `delta` frames for that player are dropped until the next full `view` passes — so the client sees one clean resync instead of a gap followed by its own resync.
9. **Limits.** WebSocket incoming message and frame size 64 KiB (axum `max_message_size`/`max_frame_size`; larger → the connection closes); > 20 client messages in any one-second window → close (code 1008); malformed JSON or unknown type → `error {code: bad_json | bad_message}`, connection kept; binary frames → `error {code: bad_message}`. At most `MAX_LIVE_GAMES = 8` game processes (`error {code: too_many_games}`), and at most `MAX_PENDING_LOGINS = 256` login attempts in flight (oldest dropped).
10. **Auth.** `openidconnect` 4 with its reqwest client (rustls, redirects disabled). Provider metadata and JWKS are fetched lazily on the first login and re-fetched after an hour, so the front starts even when Authentik is down (login answers 503 until it is back). The `groups` claim is read from the verified ID token's payload by a pure function (`server::oidc::admit`), which is tested directly; the whole code + PKCE + nonce + state + cookie flow is tested against a small in-test OIDC provider (axum, RSA test key fixture) — both, so the pure rules and the wiring are each pinned. The login `state` is also put in a short-lived signed cookie and must match on callback (login CSRF). Sessions: server-side map keyed by 32 random bytes (hex), in a signed cookie `signalbox_session` (HttpOnly, SameSite=Lax, Secure, Path=/, Max-Age 12 h); lost on restart. `/` without a session redirects to `/auth/login`; `/ws` without one answers 401 before the upgrade.
11. **Dev auth.** Cargo feature `dev-auth` on `signalbox-server` adds `/auth/dev?user=<name>` and makes the OIDC variables optional. Names given to it must be 1–32 of `[A-Za-z0-9_.-]`. A test in the default-feature build asserts `/auth/dev` answers 404; the release image is built without the feature.
12. **Networked bot strategy (honest label).** Over the network a bot cannot read the game's sim (C1's soak bots used `robot::commands(game.sim())`). `bot::Greedy` decides from the bot's own layout and view only: for each operable signal at red with a headcode in its berth and no route from it, try that signal's routes in layout order, one attempt per signal per 30 sim seconds, at most 2 commands per decision, 2 decisions per real second. It is a transport exerciser, not a good signaller: trains may stall in bot areas. The e2e tests assert safety (0 SPAD/collision/invariant, from the game's counters), that bot commands reach the sim, and view consistency — not traffic flow, which C1 already proved in process.
13. **Game ids and layouts.** Ids are `g-` + 12 chars of `a-z2-7` (spec §2.2); `join` accepts only ids of that shape (no path tricks). Layouts are the `*.json` files in `SIGNALBOX_LAYOUTS` whose stem matches `[a-z0-9-]{1,40}`, read once at startup (name + area names).
14. **Crash vs saved.** A game that exits 0 disappears from the live table (the lobby lists it from its save as `saved`). A game whose socket hits EOF or a bad frame, or whose process exits non-zero or by signal, without `Shutdown` from the front, becomes `crashed` with the last line of its stderr as the error text; its clients get `notice game_crashed` and are back in the lobby. `join` on a crashed game resumes it. Crashed entries live in memory only.
15. **Empty exit is configurable for tests:** `signalbox-game --empty-exit-s <secs>` (default 600).
16. **Front shutdown:** SIGTERM or SIGINT → stop accepting, send `Shutdown` to every game, wait up to 10 s for all to exit, kill the rest.
17. **Task split for the front.** The supervisor (lobby, children, relay, crashes, duplicate logins, outbound queue, shutdown) is one task (Task 5) tested by driving `Supervisor` directly — `attach` a user, `handle_frame`, `pop` the `Outbox` — against real `signalbox-game` children, with no HTTP. The HTTP side (config, sessions, dev login, the `/ws` gate and socket loop with its limits, `start`/`run`, the `signalbox-server` binary) is Task 6. `bot::net` (the WebSocket + dev-login client) arrives in Task 6 because the front's HTTP tests need a client and the bot is the project's client; Task 8 adds only the bot's strategy (tested in process), Task 9 the network player loop with the end-to-end tests that exercise it.
18. **The supervisor owns its directories:** `Supervisor::new` creates `saves/` and `sockets/` with mode 0700 and fails if it cannot; `SupervisorConfig::empty_exit_s` is passed to every child as `--empty-exit-s` (the front uses `EMPTY_EXIT_S`; tests use 1 s).

## Global Constraints

- License: GPL-2.0-or-later on every new crate (`license.workspace = true`).
- Every cargo command runs through `scripts/cargo` from the repo root (Docker, `rust:1.98-slim-bookworm`, repo at `/w`, network available, no environment forwarded). Paths inside the container are under `/w`.
- CI (`scripts/ci/test.sh`) builds with `-D warnings --locked --offline`: no unused imports, variables or dead code in any feature combination it builds (default features for the workspace, plus `-p signalbox-server --features dev-auth`); commit `Cargo.lock` with every dependency change.
- **After any task that changes `Cargo.lock`, the controller (not the subagent) reseeds the CI cache before anything is pushed:** `scripts/cargo fetch --locked` then `/opt/stack/apps/signalbox-runner/seed-cache.sh`. Subagents never push.
- Determinism rules stay for `game` and `protocol`: `BTreeMap`/`BTreeSet`/`Vec` only, no wall clock in `Game` (only `game::save` reads clocks — timestamps and `busy`). The process shell (`server::process`) and the front may use tokio time and `Instant`; that is outside `Game`.
- The `game` library stays free of tokio. tokio runtimes are current-thread (`#[tokio::main(flavor = "current_thread")]`, `#[tokio::test]`).
- No client input may panic anything (front or game); names are opaque strings (any UTF-8, e.g. `Hackney & Bow`, `39,1V1`).
- Keep dependencies minimal and rustls-only: the workspace adds exactly `tokio`, `axum`, `axum-extra`, `openidconnect`, `tokio-tungstenite`, `futures-util`, `base64` (versions in Tasks 3, 6 and 7; `rand` 0.9 is reused from the workspace). No OpenSSL: `cargo tree -p signalbox-server -i openssl-sys` must print nothing.
- Crate names: `signalbox-ipc` (lib `ipc`), `signalbox-server` (lib `server`, bins `signalbox-server`, `signalbox-game`).
- Wire JSON: every frame tagged `"type"`, snake_case; lobby errors are `{"type":"error","code":...,"message":...}`; game-side errors stay `notice {kind: error}`.
- Numbers: ipc max frame 4 MiB; WebSocket max incoming message 64 KiB; > 20 client messages in one second → close; outbound queue 64; game loop advance every 0.1 s real, flush every 0.2 s, `Status` every 1 s; empty game exits after 600 s; front shutdown waits 10 s; session 12 h; pending login 10 min; at most 8 live games; OIDC metadata re-fetched after 1 h.
- Config comes from the environment only: `SIGNALBOX_ADDR` (default `0.0.0.0:9160`), `SIGNALBOX_DATA` (default `/data`; holds `saves/` and `sockets/`), `SIGNALBOX_LAYOUTS` (default `/opt/signalbox/layouts`), `SIGNALBOX_PUBLIC_URL` (e.g. `https://ra.tail3e0c1e.ts.net:50160`), `OIDC_ISSUER` (e.g. `https://auth.skyes.lgbt/application/o/signalbox/`), `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET`, `SIGNALBOX_SESSION_KEY` (hex, ≥ 64 bytes), `SIGNALBOX_GAME_BIN` (default: `signalbox-game` next to the running server binary). Missing or bad config → exit 2 with one clear line on stderr.
- Infra actions (vault files, Authentik, `/opt/stack`, compose projects and long-running containers on ra, `tailscale serve`, the CI cache) are controller-only and owner-gated: they are in the final "Controller" section, never in a subagent task. Subagents may run `scripts/cargo` and, in Task 10 only, build a throwaway image and run it with `--rm` on `127.0.0.1:19160`; they touch no existing container, volume, network or compose project.

## Review Focus

1. **A second login of the same user while the first is in a game** (two browser tabs, or a bot restarted before its old socket times out): the new socket must get the player, their game and area; the old one gets `notice replaced` and closes; the old socket's close must NOT disconnect the player from the game. Pinned in Task 5 (`a_second_login_replaces_the_first_and_keeps_the_game`).
2. **A client that stops reading** (a stalled tab) while the game keeps sending: memory stays bounded, and when it reads again it recovers with exactly one full view rather than a seq gap. Pinned in Task 5 (`outbox_overflow_clears_asks_for_one_resync_and_drops_deltas_until_a_view`, and end to end through a real game in `a_stalled_client_gets_one_fresh_view_after_its_queue_overflows`).
3. **A game that dies while starting** (a save that no longer resumes, or a missing binary): `join` must end in `notice game_crashed`, the lobby must show the game `crashed` with the child's own error line, and the front and other games carry on. Pinned in Task 5 (`a_game_that_fails_to_resume_shows_as_crashed_with_its_error`, `a_game_whose_binary_is_missing_is_crashed_not_fatal`).
4. **Hostile or careless frames** — malformed JSON, an unknown `type`, a binary frame, a 70 KiB message, 30 messages in a second, a `create_game` with layout `../../etc/passwd` or start `25:00`, a `join` of `g-../x`: each gets an `error` or a close, never a panic and never a file outside `saves/`. Pinned in Task 5 (`lobby_rejects_bad_layouts_starts_and_ids`) and Task 6 (`limits_malformed_binary_oversize_and_flood`).
5. **A login callback that does not match its login** — wrong or replayed `state`, missing login cookie, a user outside `signalbox-users`, a token for another client id: no session is created (400/403). Pinned in Task 7 (`callback_rejects_bad_state_replay_and_foreign_audience`, `admit_requires_the_group_and_a_username`).

---

## File Structure

```
Cargo.toml                                    members += ipc, server; workspace deps (Tasks 3, 4, 6, 7)
crates/game/src/game.rs                       (Task 1) GameStatus, status, last_snapshot_tick, take_save_errors, pause_for_empty
crates/game/src/save.rs                       (Task 1) SaveDb::busy, SaveSummary, read_summary
crates/game/src/lib.rs                        (Task 1) re-export GameStatus
crates/game/tests/status.rs                   (Task 1)
crates/protocol/src/lobby.rs                  (Task 2) LobbyMsg, LobbyReply, GameInfo, GameState, AreaHolder, LayoutInfo, ClientFrame, ServerFrame
crates/protocol/src/lib.rs                    (Task 2) pub mod lobby, re-exports, new codes
crates/protocol/tests/lobby.rs                (Task 2)
crates/ipc/Cargo.toml, src/lib.rs             (Task 3) ToGame, FromGame, StatusMsg, Counters, LogLevel, read_frame, write_frame, IpcError
crates/ipc/tests/codec.rs                     (Task 3)
crates/server/Cargo.toml                      (Task 4; grows in 5, 6, 7, 9)
crates/server/src/lib.rs                      (Task 4) module list; (Task 5) front modules
crates/server/src/process.rs                  (Task 4) Args, normalise_start, set_start_time, open_game, Shell, serve
crates/server/src/bin/signalbox-game.rs       (Task 4) thin main
crates/server/tests/process.rs                (Task 4) spawns the real signalbox-game
crates/server/src/layouts.rs                  (Task 5) Layouts, valid_layout_name, valid_game_id, new_game_id
crates/server/src/outbox.rs                   (Task 5) Outbox, Pushed, OUTBOX_CAP
crates/server/src/supervisor.rs               (Task 5) Supervisor, SupervisorConfig, Attached, MAX_LIVE_GAMES
crates/server/tests/supervisor.rs             (Task 5) lobby, children, relay, replace, crashes, stall, shutdown
crates/server/src/config.rs                   (Task 6) Config, OidcConfig, from_env, from_lookup
crates/server/src/session.rs                  (Task 6) Sessions, SESSION_COOKIE, SESSION_TTL, cookie, removal, random_hex
crates/server/src/limit.rs                    (Task 6) RateLimit, MAX_MSGS_PER_S
crates/server/src/web.rs                      (Task 6) AppState, router, index page, logout, /ws, dev auth; (Task 7) OIDC routes
crates/server/src/lib.rs                      (Task 6) start, Running, run; (Task 7) builds the Oidc
crates/server/src/bin/signalbox-server.rs     (Task 6) thin main
crates/server/src/oidc.rs                     (Task 7) Oidc, admit, groups_from_id_token, PendingLogins, LoginError
crates/bot/src/net.rs                         (Task 6) http_get, dev_login, url_encode, Conn, NetError
crates/server/tests/common/mod.rs             (Task 6) Front harness (temp data dir, layouts, in-process server, dev login)
crates/server/tests/front.rs                  (Task 6, feature dev-auth) auth gate, lobby over WebSockets, limits, logout, stop
crates/server/tests/units.rs                  (Task 6) config, sessions, rate limit, page escaping
crates/server/tests/release.rs                (Task 6, default features only) /auth/dev is 404
crates/server/tests/oidc.rs                   (Task 7) admit, pending logins, and the whole flow against an in-test OIDC provider
crates/server/tests/fixtures/oidc-test-key.pem (Task 7) RSA key for the in-test provider only
crates/bot/src/lib.rs                         (Task 6) pub mod net; (Task 8) pub mod strategy; (Task 9) pub mod play
crates/bot/src/strategy.rs                    (Task 8) Greedy
crates/bot/src/play.rs                        (Task 9) NetPlayer: a Bot + Greedy on a Conn
crates/bot/tests/strategy.rs                  (Task 8)
crates/server/tests/e2e.rs                    (Task 9, feature dev-auth) Liverpool Street soak (fast + #[ignore] long), crash test
scripts/ci/test.sh                            (Task 6) dev-auth build + tests (Task 9's fast e2e runs there)
deploy/Dockerfile, deploy/Dockerfile.dockerignore (Task 10)
deploy/docker-compose.yml                     (Task 10)
deploy/authentik/signalbox-oidc-blueprint.yaml.example (Task 10)
deploy/authentik/signalbox-access.yaml.example (Task 10) group + binding snippets
deploy/README.md                              (Task 10)
deploy/smoke.sh                               (Task 10) outside-in check of a running front (used again by the controller)
CLAUDE.md                                     (Task 10) crates, commands, architecture notes
```

---

### Task 1: `game` — status, snapshot tick, save errors, empty pause, save summary

**Files:**
- Modify: `crates/game/src/game.rs` (new `GameStatus`, fields `last_snapshot`, `save_errors`; methods below)
- Modify: `crates/game/src/save.rs` (`SaveDb::busy`, `SaveSummary`, `read_summary`)
- Modify: `crates/game/src/lib.rs` (re-export `GameStatus`)
- Test: `crates/game/tests/status.rs` (new)

**Interfaces:**
- Consumes: C1's `Game`, `GameStats`, `GameClock` (pub fields `paused`, `speed`, `vote`), `SaveDb`, `signalbox_core::time::parse_hms`, `signalbox_core::sim::TICK_S`.
- Produces:
  - `pub struct GameStatus { pub sim_time: f64, pub tick: u64, pub paused: bool, pub speed: u8, pub holders: BTreeMap<String, Option<String>>, pub players: Vec<(String, bool)>, pub connected: usize, pub stats: GameStats, pub save_busy: std::time::Duration }` (`Clone, Debug, PartialEq`), re-exported as `game::GameStatus`.
  - `Game::status(&self) -> GameStatus`; `Game::last_snapshot_tick(&self) -> Option<u64>`; `Game::take_save_errors(&mut self) -> Vec<String>`; `Game::pause_for_empty(&mut self)`.
  - `SaveDb::busy(&self) -> Duration`.
  - `pub struct SaveSummary { pub layout: String, pub seed: u64, pub areas: Vec<String>, pub last_played: u64, pub tick: u64, pub sim_time: f64 }` (`Clone, Debug, PartialEq`) and `pub fn game::save::read_summary(path: &Path) -> Result<SaveSummary, SaveError>` (read-only connection; rejects a missing file and any schema but 2 with `SaveError::Bad`).

- [ ] **Step 1: Write the failing tests**

Create `crates/game/tests/status.rs`:
```rust
//! What the process shell reads from a game (C2): status, the newest
//! snapshot, save errors, the empty-game pause, and the lobby's read-only
//! save summary.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::*;
use game::Game;
use game::save::{SaveError, read_summary};
use protocol::*;
use rusqlite::Connection;

fn s(x: &str) -> String {
    x.to_string()
}

fn temp_save(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("signalbox-status-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.sqlite"));
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }
    path
}

fn open(path: &Path) -> Connection {
    Connection::open(path).unwrap()
}

#[test]
fn status_reports_clock_holders_and_players() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    join(&mut g, "bob", None);
    g.advance(1.0);
    let st = g.status();
    assert_eq!((st.tick, st.paused, st.speed, st.connected), (10, false, 1, 2));
    assert_eq!(st.sim_time, 7.0 * 3600.0 + 1.0);
    assert_eq!(st.holders, BTreeMap::from([(s("East"), None), (s("West"), Some(s("alice")))]));
    assert_eq!(st.players, [(s("alice"), true), (s("bob"), true)]);
    assert_eq!(st.save_busy, Duration::ZERO, "no save, no time spent saving");
    g.disconnect("alice");
    g.disconnect("bob");
    let st = g.status();
    assert_eq!(st.players, [(s("alice"), false)], "a holder stays through grace; a spectator is forgotten");
    assert_eq!(st.connected, 0);
}

#[test]
fn status_counts_what_reached_the_sim() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    command(&mut g, "alice", set_route("W1", ExitName::Signal(s("A"))));
    run_to_tick(&mut g, 20);
    let st = g.status();
    assert_eq!(st.stats.player_commands, 1);
    assert_eq!((st.stats.spads, st.stats.collisions, st.stats.invariant_violations), (0, 0, 0));
}

#[test]
fn pause_for_empty_stops_the_clock_and_drops_the_vote() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    join(&mut g, "bob", Some("East"));
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Speed { x: 4 } });
    assert!(g.clock().vote.is_some(), "bob has not agreed yet");
    g.pause_for_empty();
    assert!(g.clock().paused);
    assert!(g.clock().vote.is_none());
    let tick = g.sim().tick();
    g.advance(5.0);
    assert_eq!(g.sim().tick(), tick, "paused");
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Resume });
    send(&mut g, "bob", ClientMsg::Vote { proposal: Proposal::Resume });
    assert!(!g.clock().paused, "players resume it by vote");
    assert_eq!(g.clock().speed, 1);
}

#[test]
fn last_snapshot_tick_follows_saves_and_resumes() {
    assert_eq!(game().last_snapshot_tick(), None);
    let path = temp_save("last");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    assert_eq!(g.last_snapshot_tick(), Some(0));
    run_to_tick(&mut g, 25);
    assert!(g.save_now().is_empty());
    assert_eq!(g.last_snapshot_tick(), Some(25));
    run_to_tick(&mut g, 40);
    assert!(g.status().save_busy > Duration::ZERO);
    drop(g);
    let g = Game::resume(&path).unwrap();
    assert_eq!(g.last_snapshot_tick(), Some(25));
}

#[test]
fn save_errors_are_kept_for_the_shell_with_nobody_connected() {
    let path = temp_save("errors");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    open(&path).execute("DROP TABLE snapshots", []).unwrap();
    assert!(g.save_now().is_empty(), "nobody connected, nobody told");
    assert_eq!(g.last_snapshot_tick(), Some(0), "a failed snapshot does not count");
    let errors = g.take_save_errors();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("snapshots"), "{errors:?}");
    assert!(g.take_save_errors().is_empty(), "taken once");
    join(&mut g, "alice", Some("West"));
    let out = g.save_now();
    assert_eq!(error_codes(&out, "alice"), [codes::SAVE_FAILED]);
    assert_eq!(g.take_save_errors().len(), 1);
}

#[test]
fn read_summary_reads_meta_and_the_newest_snapshot() {
    let path = temp_save("summary");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    run_to_tick(&mut g, 30);
    assert!(g.save_now().is_empty());
    let while_running = read_summary(&path).unwrap();
    assert_eq!(while_running.tick, 30);
    drop(g);
    let sum = read_summary(&path).unwrap();
    assert_eq!((sum.layout.as_str(), sum.seed, sum.tick), ("twobox", 1, 30));
    assert_eq!(sum.areas, [s("West"), s("East")]);
    assert_eq!(sum.sim_time, 7.0 * 3600.0 + 3.0);
    assert!(sum.last_played > 1_700_000_000, "{}", sum.last_played);
    assert!(Game::resume(&path).is_ok(), "reading left the save usable");
}

#[test]
fn read_summary_rejects_missing_and_foreign_files() {
    assert!(matches!(read_summary(&temp_save("absent")), Err(SaveError::Bad(_))));
    let path = temp_save("foreign");
    let c = open(&path);
    c.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL); INSERT INTO meta VALUES ('schema', '1');")
        .unwrap();
    drop(c);
    let Err(SaveError::Bad(why)) = read_summary(&path) else { panic!("read a schema 1 save") };
    assert_eq!(why, "unsupported save schema 1");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-game --test status`
Expected: compile errors — no method `status`, `pause_for_empty`, `last_snapshot_tick`, `take_save_errors`; no `read_summary` in `game::save`.

- [ ] **Step 3: Implement in `crates/game/src/game.rs`**

Add `use std::time::Duration;` after `use std::path::Path;`.

After `pub struct GameStats { ... }` add:
```rust
/// What the process shell and the lobby need to know about a running game.
#[derive(Clone, Debug, PartialEq)]
pub struct GameStatus {
    /// Seconds since midnight.
    pub sim_time: f64,
    pub tick: u64,
    pub paused: bool,
    pub speed: u8,
    /// Every area, by name, with its holder (`None` = the robot).
    pub holders: BTreeMap<String, Option<String>>,
    /// Every known player in name order, and whether they are connected
    /// (a disconnected holder stays listed through the grace period).
    pub players: Vec<(String, bool)>,
    /// How many players are connected.
    pub connected: usize,
    pub stats: GameStats,
    /// Wall time spent writing the save so far.
    pub save_busy: Duration,
}
```

In `pub struct Game`, after `since_snapshot_s: f64,` add:
```rust
    /// Tick of the newest snapshot written or loaded.
    last_snapshot: Option<u64>,
    /// Save failures not yet taken by the caller.
    save_errors: Vec<String>,
```
and in `from_sim`'s struct literal, after `since_snapshot_s: 0.0,`:
```rust
            last_snapshot: None,
            save_errors: Vec::new(),
```

In `Game::create`, after `g.save = Some(db);` add `g.last_snapshot = Some(0);`.

In `Game::resume`, after `let world = World::from_json(&saved.world_json)?;` add `let snapshot_tick = saved.snapshot.tick;`, and after the final `g.save = Some(db);` add `g.last_snapshot = Some(snapshot_tick);`.

Replace `save_now` and `save_failed` (the `fn save_failed(&self, ...)` signature changes to `&mut self`; `submit` compiles unchanged because its borrow of `self.save` ends before the call):
```rust
    /// Snapshot now and restart the autosave timer. A failure goes to every
    /// connected player as `save_failed` and is kept for
    /// `take_save_errors`; the game carries on.
    pub fn save_now(&mut self) -> Vec<Out> {
        self.since_snapshot_s = 0.0;
        let Some(db) = &self.save else { return vec![] };
        match db.write_snapshot(&self.sim.snapshot()) {
            Ok(()) => {
                self.last_snapshot = Some(self.sim.tick());
                vec![]
            }
            Err(e) => self.save_failed(&e.to_string()),
        }
    }

    /// Tick of the newest snapshot this game wrote or resumed from; `None`
    /// for a game without a save.
    pub fn last_snapshot_tick(&self) -> Option<u64> {
        self.last_snapshot
    }

    /// Save failures since the last call, oldest first (each was also sent
    /// to the connected players as `save_failed`).
    pub fn take_save_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.save_errors)
    }

    /// Pause the clock without a vote and drop any open proposal: the last
    /// player has left (spec §2.2). Players resume it by vote.
    pub fn pause_for_empty(&mut self) {
        self.clock.paused = true;
        self.clock.vote = None;
    }

    pub fn status(&self) -> GameStatus {
        let net = &self.sim.world().net;
        GameStatus {
            sim_time: self.sim.now_s(),
            tick: self.sim.tick(),
            paused: self.clock.paused,
            speed: self.clock.speed,
            holders: net.areas.iter().zip(&self.holders).map(|(a, h)| (a.name.clone(), h.clone())).collect(),
            players: self.players.iter().map(|(n, p)| (n.clone(), p.connected)).collect(),
            connected: self.players.values().filter(|p| p.connected).count(),
            stats: self.stats.clone(),
            save_busy: self.save.as_ref().map_or(Duration::ZERO, SaveDb::busy),
        }
    }

    fn save_failed(&mut self, why: &str) -> Vec<Out> {
        self.save_errors.push(why.to_string());
        self.players
            .iter()
            .filter(|(_, p)| p.connected)
            .map(|(name, _)| error(name, codes::SAVE_FAILED, why))
            .collect()
    }
```

In `crates/game/src/lib.rs` add `GameStatus` to the `pub use game::{...}` list (after `GameStats`).

- [ ] **Step 4: Implement in `crates/game/src/save.rs`**

Replace the imports at the top with:
```rust
use std::cell::Cell;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use signalbox_core::events::Command;
use signalbox_core::sim::{Sim, SimState, TICK_S};
use signalbox_core::time::parse_hms;
use signalbox_core::world::World;
use signalbox_core::world::file::WorldFile;

use crate::game::{GameMeta, ROBOT};
```

Replace `pub struct SaveDb { conn: Connection }` with:
```rust
pub struct SaveDb {
    conn: Connection,
    /// Wall time spent in `append_command` and `write_snapshot`.
    busy: Cell<Duration>,
}

/// What the lobby shows for a save, read without writing anything.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveSummary {
    pub layout: String,
    pub seed: u64,
    /// Area names in world order.
    pub areas: Vec<String>,
    /// Unix seconds.
    pub last_played: u64,
    /// Tick of the newest snapshot.
    pub tick: u64,
    /// Sim time of the newest snapshot, seconds since midnight.
    pub sim_time: f64,
}

/// Read a save's meta and newest snapshot tick through a read-only
/// connection (spec §7.1: the front only ever reads `meta`). Safe while the
/// game process has the file open (WAL readers do not block the writer).
pub fn read_summary(path: &Path) -> Result<SaveSummary, SaveError> {
    if !path.exists() {
        return Err(SaveError::Bad(format!("no save file {}", path.display())));
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    let meta_value = |key: &str| -> Result<String, SaveError> {
        conn.query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| r.get(0))
            .optional()?
            .ok_or_else(|| SaveError::Bad(format!("meta `{key}` is missing")))
    };
    let schema = meta_value("schema")?;
    if schema != SAVE_SCHEMA.to_string() {
        return Err(SaveError::Bad(format!("unsupported save schema {schema}")));
    }
    let bad = |what: &str, e: &dyn std::fmt::Display| SaveError::Bad(format!("meta {what}: {e}"));
    let seed = meta_value("seed")?.parse::<u64>().map_err(|e| bad("seed", &e))?;
    let last_played = meta_value("last_played")?.parse::<u64>().map_err(|e| bad("last_played", &e))?;
    let areas: Vec<String> = serde_json::from_str(&meta_value("areas")?).map_err(|e| bad("areas", &e))?;
    let start = meta_value("start")?;
    let start_s = parse_hms(&start).ok_or_else(|| bad("start", &start))?;
    let tick: i64 = conn
        .query_row("SELECT MAX(tick) FROM snapshots", [], |r| r.get::<_, Option<i64>>(0))?
        .ok_or_else(|| SaveError::Bad("no snapshot".into()))?;
    let tick = tick as u64;
    Ok(SaveSummary {
        layout: meta_value("layout")?,
        seed,
        areas,
        last_played,
        tick,
        sim_time: f64::from(start_s) + tick as f64 * TICK_S,
    })
}
```

In `SaveDb::create` and `SaveDb::open`, change `Ok(SaveDb { conn })` to `Ok(SaveDb { conn, busy: Cell::new(Duration::ZERO) })`.

Replace `append_command` and the head of `write_snapshot` so both are timed (the body of the old `write_snapshot` moves unchanged into `write_snapshot_untimed`):
```rust
    /// Wall time spent writing commands and snapshots so far.
    pub fn busy(&self) -> Duration {
        self.busy.get()
    }

    fn timed<T>(&self, f: impl FnOnce() -> Result<T, SaveError>) -> Result<T, SaveError> {
        let t = Instant::now();
        let r = f();
        self.busy.set(self.busy.get() + t.elapsed());
        r
    }

    pub fn append_command(&self, tick: u64, player: &str, area: &str, cmd: &Command) -> Result<(), SaveError> {
        self.timed(|| {
            let json = serde_json::to_string(cmd).expect("commands serialise");
            self.conn.execute(
                "INSERT INTO commands (tick, player, area, command) VALUES (?1, ?2, ?3, ?4)",
                params![tick as i64, player, area, json],
            )?;
            Ok(())
        })
    }

    pub fn write_snapshot(&self, state: &SimState) -> Result<(), SaveError> {
        self.timed(|| self.write_snapshot_untimed(state))
    }

    fn write_snapshot_untimed(&self, state: &SimState) -> Result<(), SaveError> {
        let json = serde_json::to_string(state).expect("state serialises");
        let now = now_text();
        let tx = self.conn.unchecked_transaction()?;
        let last_seq: i64 = tx.query_row("SELECT COALESCE(MAX(seq), 0) FROM commands", [], |r| r.get(0))?;
        tx.execute(
            "INSERT OR REPLACE INTO snapshots (tick, saved_at, state, last_seq) VALUES (?1, ?2, ?3, ?4)",
            params![state.tick as i64, now, json, last_seq],
        )?;
        tx.execute(
            "DELETE FROM snapshots WHERE tick NOT IN (SELECT tick FROM snapshots ORDER BY tick DESC LIMIT ?1)",
            params![KEEP_SNAPSHOTS],
        )?;
        tx.execute("UPDATE meta SET value = ?1 WHERE key = 'last_played'", params![now])?;
        tx.commit()?;
        Ok(())
    }
```
(This is the old `write_snapshot` body, unchanged, under a new name.)

- [ ] **Step 5: Run the game tests**

Run: `scripts/cargo test -p signalbox-game`
Expected: PASS — the 7 new `status` tests and every C1 test (save tests included: the read-only reader works on a WAL save both while the game holds it open and after it closed).

- [ ] **Step 6: Commit**

```bash
git add crates/game/src/game.rs crates/game/src/save.rs crates/game/src/lib.rs crates/game/tests/status.rs
git commit -m "feat(game): status, snapshot tick, save errors, empty pause and save summary

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: `protocol` — lobby messages and one frame type per direction

**Files:**
- Create: `crates/protocol/src/lobby.rs`
- Modify: `crates/protocol/src/lib.rs` (module, re-exports, lobby error codes)
- Modify: `crates/protocol/Cargo.toml` (`serde_json` moves from dev-dependencies to dependencies)
- Test: `crates/protocol/tests/lobby.rs` (new)

**Interfaces:**
- Consumes: C1's `ClientMsg`, `ServerMsg`, `Notice`.
- Produces (module `protocol::lobby`, all re-exported at the crate root except the tag lists):
  - `enum LobbyMsg { ListGames, ListLayouts, CreateGame { layout: String, seed: Option<u64>, start: Option<String> }, Join { game: String }, Leave }`
  - `enum LobbyReply { Games { games: Vec<GameInfo> }, Layouts { layouts: Vec<LayoutInfo> }, Joined { game: String, you: String }, Error { code: String, message: String } }`
  - `enum GameState { Running, Saved, Crashed }`; `struct GameInfo { id, layout, state, sim_time: f64, areas: Vec<AreaHolder>, players: Vec<String>, error: Option<String> }`; `struct AreaHolder { name: String, holder: Option<String> }`; `struct LayoutInfo { name: String, areas: Vec<String> }`
  - `enum ClientFrame { Lobby(LobbyMsg), Game(ClientMsg) }` with `from_json(&str) -> Result<ClientFrame, FrameError>`, `from_value`, `to_json`; `enum ServerFrame { Lobby(LobbyReply), Game(ServerMsg) }` with the same plus `ServerFrame::error(code, message)`.
  - `enum FrameError { BadJson(String), BadMessage(String) }` with `code() -> &'static str`.
  - Tag lists `LOBBY_MSG_TYPES`, `CLIENT_MSG_TYPES`, `LOBBY_REPLY_TYPES`, `SERVER_MSG_TYPES`.
  - `protocol::codes::{BAD_JSON, BAD_MESSAGE, UNKNOWN_GAME, UNKNOWN_LAYOUT, BAD_START, NOT_IN_GAME, TOO_MANY_GAMES, GAME_STOPPED}` (`game_stopped`: the game you were in saved and exited, e.g. front shutdown or its empty timeout racing a join).

Why a hand-written `Deserialize` (decision 3): the frame's `"type"` picks the enum first, so a frame with a known type and a bad field reports that field, and an unknown type says so, where `#[serde(untagged)]` would only say "did not match any variant". `Serialize` stays derived and untagged (each inner enum is itself tagged).

- [ ] **Step 1: Write the failing tests**

Create `crates/protocol/tests/lobby.rs`:
```rust
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
                },
                GameInfo {
                    id: s("g-zzzzzzzzzzzz"),
                    layout: s("drain"),
                    state: GameState::Crashed,
                    sim_time: 3600.0,
                    areas: vec![],
                    players: vec![],
                    error: Some(s("resume: bad snapshot")),
                },
            ],
        }),
        json!({"type": "games", "games": [
            {"id": "g-abcdefgh2345", "layout": "liverpool-st", "state": "running", "sim_time": 25200.5,
             "areas": [{"name": "Liverpool Street", "holder": "ann"}, {"name": "Bethnal Green"}],
             "players": ["ann", "sam"]},
            {"id": "g-zzzzzzzzzzzz", "layout": "drain", "state": "crashed", "sim_time": 3600.0,
             "areas": [], "players": [], "error": "resume: bad snapshot"}
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
```

- [ ] **Step 2: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-protocol --test lobby`
Expected: compile error — no module `lobby`, no `ClientFrame`/`ServerFrame`/`LobbyMsg`.

- [ ] **Step 3: Implement**

In `crates/protocol/Cargo.toml`, move `serde_json.workspace = true` from `[dev-dependencies]` to `[dependencies]` (delete the then-empty `[dev-dependencies]` table). The frame dispatch needs `serde_json::Value` at run time; it is pure Rust and fine for the browser client later.

Create `crates/protocol/src/lobby.rs`:
```rust
//! The lobby (spec §3.1), handled by the front, and the frames that carry
//! lobby and game messages over one WebSocket. Every frame is a JSON object
//! tagged by `"type"`; lobby and game tags never collide, so a frame's type
//! alone says which enum it belongs to.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::msg::{ClientMsg, ServerMsg};

/// Client → front.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LobbyMsg {
    ListGames,
    ListLayouts,
    /// Start a new game; `seed` defaults to a random one, `start`
    /// ("HH:MM" or "HH:MM:SS") to the layout's own start time.
    CreateGame {
        layout: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seed: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start: Option<String>,
    },
    /// Join a game; a saved or crashed one is resumed.
    Join { game: String },
    /// Back to the lobby.
    Leave,
}

/// Front → client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LobbyReply {
    Games { games: Vec<GameInfo> },
    Layouts { layouts: Vec<LayoutInfo> },
    /// You are in `game` as `you`; its layout and view follow.
    Joined { game: String, you: String },
    Error { code: String, message: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameState {
    Running,
    Saved,
    Crashed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameInfo {
    pub id: String,
    pub layout: String,
    pub state: GameState,
    /// Seconds since midnight: live for a running game, the newest save's
    /// otherwise.
    pub sim_time: f64,
    pub areas: Vec<AreaHolder>,
    /// Connected players, in name order.
    pub players: Vec<String>,
    /// Why a crashed game stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AreaHolder {
    pub name: String,
    /// `None` = the robot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutInfo {
    pub name: String,
    pub areas: Vec<String>,
}

/// `"type"` tags of `LobbyMsg`.
pub const LOBBY_MSG_TYPES: [&str; 5] = ["list_games", "list_layouts", "create_game", "join", "leave"];
/// `"type"` tags of `ClientMsg`.
pub const CLIENT_MSG_TYPES: [&str; 5] = ["claim", "release", "command", "vote", "resync"];
/// `"type"` tags of `LobbyReply`.
pub const LOBBY_REPLY_TYPES: [&str; 4] = ["games", "layouts", "joined", "error"];
/// `"type"` tags of `ServerMsg`.
pub const SERVER_MSG_TYPES: [&str; 4] = ["layout", "view", "delta", "notice"];

/// Why a text frame could not be read. `code()` is the `error` code the
/// front answers with.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    #[error("not JSON: {0}")]
    BadJson(String),
    #[error("{0}")]
    BadMessage(String),
}

impl FrameError {
    pub fn code(&self) -> &'static str {
        match self {
            FrameError::BadJson(_) => crate::codes::BAD_JSON,
            FrameError::BadMessage(_) => crate::codes::BAD_MESSAGE,
        }
    }
}

/// Anything a client may send over the socket.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ClientFrame {
    Lobby(LobbyMsg),
    Game(ClientMsg),
}

/// Anything the front sends over the socket.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ServerFrame {
    Lobby(LobbyReply),
    Game(ServerMsg),
}

fn type_of(v: &Value) -> Result<&str, FrameError> {
    v.get("type").and_then(Value::as_str).ok_or_else(|| FrameError::BadMessage("a frame needs a string `type`".into()))
}

fn field_error(e: serde_json::Error) -> FrameError {
    FrameError::BadMessage(e.to_string())
}

impl ClientFrame {
    pub fn from_json(text: &str) -> Result<ClientFrame, FrameError> {
        let v: Value = serde_json::from_str(text).map_err(|e| FrameError::BadJson(e.to_string()))?;
        ClientFrame::from_value(v)
    }

    pub fn from_value(v: Value) -> Result<ClientFrame, FrameError> {
        let t = type_of(&v)?;
        if LOBBY_MSG_TYPES.contains(&t) {
            serde_json::from_value(v).map(ClientFrame::Lobby).map_err(field_error)
        } else if CLIENT_MSG_TYPES.contains(&t) {
            serde_json::from_value(v).map(ClientFrame::Game).map_err(field_error)
        } else {
            Err(FrameError::BadMessage(format!("unknown type `{t}`")))
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("frames serialise")
    }
}

impl ServerFrame {
    pub fn from_json(text: &str) -> Result<ServerFrame, FrameError> {
        let v: Value = serde_json::from_str(text).map_err(|e| FrameError::BadJson(e.to_string()))?;
        ServerFrame::from_value(v)
    }

    pub fn from_value(v: Value) -> Result<ServerFrame, FrameError> {
        let t = type_of(&v)?;
        if LOBBY_REPLY_TYPES.contains(&t) {
            serde_json::from_value(v).map(ServerFrame::Lobby).map_err(field_error)
        } else if SERVER_MSG_TYPES.contains(&t) {
            serde_json::from_value(v).map(ServerFrame::Game).map_err(field_error)
        } else {
            Err(FrameError::BadMessage(format!("unknown type `{t}`")))
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("frames serialise")
    }

    /// A lobby `error` frame.
    pub fn error(code: &str, message: impl Into<String>) -> ServerFrame {
        ServerFrame::Lobby(LobbyReply::Error { code: code.to_string(), message: message.into() })
    }
}

impl<'de> Deserialize<'de> for ClientFrame {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        ClientFrame::from_value(Value::deserialize(d)?).map_err(D::Error::custom)
    }
}

impl<'de> Deserialize<'de> for ServerFrame {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        ServerFrame::from_value(Value::deserialize(d)?).map_err(D::Error::custom)
    }
}
```

In `crates/protocol/src/lib.rs` add `pub mod lobby;` (between `diff` and `msg`), add after the `pub use diff::...` line:
```rust
pub use lobby::{AreaHolder, ClientFrame, FrameError, GameInfo, GameState, LayoutInfo, LobbyMsg, LobbyReply, ServerFrame};
```
and extend `pub mod codes` after `SAVE_FAILED`:
```rust
    // Lobby errors, sent by the front as `{"type": "error", ...}`.
    pub const BAD_JSON: &str = "bad_json";
    pub const BAD_MESSAGE: &str = "bad_message";
    pub const UNKNOWN_GAME: &str = "unknown_game";
    pub const UNKNOWN_LAYOUT: &str = "unknown_layout";
    pub const BAD_START: &str = "bad_start";
    pub const NOT_IN_GAME: &str = "not_in_game";
    pub const TOO_MANY_GAMES: &str = "too_many_games";
    pub const GAME_STOPPED: &str = "game_stopped";
```

- [ ] **Step 4: Run the protocol tests**

Run: `scripts/cargo test -p signalbox-protocol`
Expected: PASS (5 new lobby tests, all C1 golden/diff tests).

- [ ] **Step 5: Commit**

```bash
git add crates/protocol Cargo.lock
git commit -m "feat(protocol): lobby messages and client/server frames

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: `ipc` crate — front ⇄ game messages and the frame codec

**Files:**
- Modify: `Cargo.toml` (add `"crates/ipc"` to `members`; add `tokio = "1.53"` to `[workspace.dependencies]`)
- Create: `crates/ipc/Cargo.toml`, `crates/ipc/src/lib.rs`
- Test: `crates/ipc/tests/codec.rs`

**Interfaces:**
- Consumes: `protocol::{ClientMsg, ServerMsg}`.
- Produces (crate `ipc`):
  - `pub const MAX_FRAME: usize = 4 << 20;`
  - `enum ToGame { Connect { player: String }, Disconnect { player: String }, Client { player: String, msg: ClientMsg }, Shutdown }`
  - `enum FromGame { ToPlayer { player: String, msg: ServerMsg }, Status(StatusMsg), Saved { tick: u64 }, Log { level: LogLevel, message: String } }`
  - `struct StatusMsg { sim_time: f64, tick: u64, paused: bool, speed: u8, holders: BTreeMap<String, Option<String>>, players: Vec<PlayerStatus>, counters: Counters }`, `struct PlayerStatus { name: String, connected: bool }`, `struct Counters { spads, collisions, invariant_violations, player_commands, robot_commands, save_busy_ms: u64 }` (`Copy, Default`), `enum LogLevel { Info, Warn, Error }`
  - `enum IpcError { TooLarge(usize), Truncated, Json(serde_json::Error), Io(std::io::Error) }`
  - `async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(w: &mut W, msg: &T) -> Result<(), IpcError>` (flushes)
  - `async fn read_frame<R: AsyncRead + Unpin, T: DeserializeOwned>(r: &mut R) -> Result<Option<T>, IpcError>` — `Ok(None)` only at a clean end between frames. **Not cancel-safe**: every caller reads frames in a dedicated task and forwards them over a channel.

Why hand-rolled instead of `tokio-util`'s `LengthDelimitedCodec`: it is 30 lines, needs only `tokio`'s `io-util`, and keeps `tokio-util` + `bytes` framing out of the dependency list; the length check happens before any allocation.

Why 4 MiB instead of the brief's example 1 MiB: a spectator's `Layout` is the largest frame the game sends and grows with the layout; 4 MiB leaves room for bigger converted layouts while still bounding a hostile length. (The WebSocket limit for *client* messages stays 64 KiB.)

- [ ] **Step 1: Write the failing tests**

Create `crates/ipc/Cargo.toml`:
```toml
[package]
name = "signalbox-ipc"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "ipc"
path = "src/lib.rs"

[dependencies]
signalbox-protocol = { path = "../protocol" }
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tokio = { workspace = true, features = ["io-util"] }

[dev-dependencies]
tokio = { workspace = true, features = ["io-util", "rt", "macros"] }
```

In the root `Cargo.toml` change `members` to
`["crates/core", "crates/sim-cli", "crates/ts2-import", "crates/protocol", "crates/game", "crates/bot", "crates/ipc"]`
and append `tokio = "1.53"` to `[workspace.dependencies]` (no default features; each crate names the features it uses).

Create `crates/ipc/src/lib.rs` containing only `//! placeholder` so the crate builds, and `crates/ipc/tests/codec.rs`:
```rust
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
```

- [ ] **Step 2: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-ipc`
Expected: compile errors — `ToGame`, `FromGame`, `read_frame`, `write_frame` not found.

- [ ] **Step 3: Implement**

Replace `crates/ipc/src/lib.rs` with:
```rust
//! Front ⇄ game messages over a Unix stream socket (spec §9). Each frame is
//! a big-endian `u32` length followed by that many bytes of JSON. Both ends
//! run tokio; the codec works on any `AsyncRead`/`AsyncWrite`.

use std::collections::BTreeMap;

use protocol::{ClientMsg, ServerMsg};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Largest frame either side sends or accepts. A spectator's layout of the
/// biggest shipped world is well under this.
pub const MAX_FRAME: usize = 4 << 20;

/// Front → game.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToGame {
    Connect { player: String },
    Disconnect { player: String },
    Client { player: String, msg: ClientMsg },
    /// Save and exit.
    Shutdown,
}

/// Game → front.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FromGame {
    ToPlayer { player: String, msg: ServerMsg },
    /// For the lobby, at most once a second.
    Status(StatusMsg),
    /// A snapshot at `tick` is on disk.
    Saved { tick: u64 },
    /// Something the front should log (save failures, for one).
    Log { level: LogLevel, message: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatusMsg {
    /// Seconds since midnight.
    pub sim_time: f64,
    pub tick: u64,
    pub paused: bool,
    pub speed: u8,
    /// Area → holder (`None` = the robot).
    pub holders: BTreeMap<String, Option<String>>,
    pub players: Vec<PlayerStatus>,
    pub counters: Counters,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerStatus {
    pub name: String,
    pub connected: bool,
}

/// Running totals from the game, passed through the front untouched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counters {
    pub spads: u64,
    pub collisions: u64,
    pub invariant_violations: u64,
    pub player_commands: u64,
    pub robot_commands: u64,
    /// Milliseconds of wall time spent writing the save.
    pub save_busy_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("frame of {0} bytes is over the {MAX_FRAME} byte limit")]
    TooLarge(usize),
    #[error("the stream ended inside a frame")]
    Truncated,
    #[error("bad frame: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Write one frame and flush it.
pub async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(w: &mut W, msg: &T) -> Result<(), IpcError> {
    let body = serde_json::to_vec(msg)?;
    if body.len() > MAX_FRAME {
        return Err(IpcError::TooLarge(body.len()));
    }
    let mut buf = Vec::with_capacity(4 + body.len());
    buf.extend_from_slice(&(body.len() as u32).to_be_bytes());
    buf.extend_from_slice(&body);
    w.write_all(&buf).await?;
    w.flush().await?;
    Ok(())
}

/// Read one frame: `Ok(None)` at a clean end of stream (between frames).
/// The length is checked before anything is allocated. Not cancel-safe:
/// call it from a task of its own, never as a `select!` branch.
pub async fn read_frame<R: AsyncRead + Unpin, T: DeserializeOwned>(r: &mut R) -> Result<Option<T>, IpcError> {
    let mut len = [0u8; 4];
    let mut got = 0;
    while got < len.len() {
        let n = r.read(&mut len[got..]).await?;
        if n == 0 {
            return if got == 0 { Ok(None) } else { Err(IpcError::Truncated) };
        }
        got += n;
    }
    let n = u32::from_be_bytes(len) as usize;
    if n > MAX_FRAME {
        return Err(IpcError::TooLarge(n));
    }
    let mut body = vec![0u8; n];
    r.read_exact(&mut body).await.map_err(|e| match e.kind() {
        std::io::ErrorKind::UnexpectedEof => IpcError::Truncated,
        _ => IpcError::Io(e),
    })?;
    Ok(Some(serde_json::from_slice(&body)?))
}
```

- [ ] **Step 4: Run the tests**

Run: `scripts/cargo test -p signalbox-ipc`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit** (Cargo.lock changed: the controller reseeds the CI cache before any push)

```bash
git add Cargo.toml Cargo.lock crates/ipc
git commit -m "feat(ipc): front-game messages and a length-prefixed JSON frame codec

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: `signalbox-game` — the game process

**Files:**
- Modify: `Cargo.toml` (add `"crates/server"` to `members`)
- Create: `crates/server/Cargo.toml`, `crates/server/src/lib.rs`, `crates/server/src/process.rs`, `crates/server/src/bin/signalbox-game.rs`
- Test: `crates/server/tests/process.rs`

**Interfaces:**
- Consumes: Task 1 (`Game::status`, `last_snapshot_tick`, `take_save_errors`, `pause_for_empty`, `game::save::read_summary` in tests), Task 3 (`ipc::*`), `signalbox_core::time::{parse_hms, fmt_hms}`.
- Produces (module `server::process`):
  - `USAGE`, `EMPTY_EXIT_S = 600`, `ADVANCE_EVERY = 100 ms`, `FLUSH_EVERY_ADVANCES = 2`, `STATUS_EVERY = 1 s`, `ACCEPT_TIMEOUT = 60 s`
  - `struct Args { save: PathBuf, socket: PathBuf, empty_exit: Duration, create: Option<CreateArgs> }`, `struct CreateArgs { world: PathBuf, layout_name: String, seed: u64, start: Option<String> }`, `Args::parse(&[String]) -> Result<Args, String>`
  - `fn normalise_start(&str) -> Option<String>` (used by the front in Task 5), `fn set_start_time(world_json: &str, start: &str) -> Result<String, String>`, `fn open_game(&Args) -> Result<Game, String>`, `fn status_msg(&GameStatus) -> ipc::StatusMsg`
  - `enum Next { Continue, Exit }`; `struct Shell` with `new(Game, Duration)`, `game()`, `on_frame(ToGame) -> (Vec<FromGame>, Next)`, `on_advance(f64) -> (Vec<FromGame>, Next)`, `on_flush() -> Vec<FromGame>`, `status() -> FromGame`, `shutdown() -> Vec<FromGame>`
  - `async fn run(Args) -> Result<(), String>`, `async fn serve(Shell, UnixListener, &Path) -> Result<(), String>`
  - Binary `signalbox-game`: exit 0 after `Shutdown`, SIGTERM, the front going away, or the empty timeout (always saving first); exit 1 with `signalbox-game: <why>` on stderr when the game cannot be created or resumed; exit 2 with the usage on bad arguments. The front's supervisor (Task 5) uses the last stderr line as the crash reason.

Behaviour notes for the implementer:
- `Shell::on_frame(Disconnect)` pauses (`pause_for_empty`) and saves only when that disconnect takes the connected count from > 0 to 0 (spec §2.2); the empty timer counts real seconds with nobody connected and resets whenever someone is.
- `Saved {tick}` is sent whenever `last_snapshot_tick()` changes (autosave, empty, shutdown); a save that fails becomes `Log {level: error, message: "save failed: ..."}`, also printed to stderr.
- `read_frame` is not cancel-safe, so the socket is read by its own task feeding an unbounded channel that the `select!` loop drains. A bad frame or EOF from the front → save and exit 0 (the front is gone or broken; the save is what matters).

- [ ] **Step 1: Create the package and write the failing tests**

Add `"crates/server"` to the root `members` list (after `"crates/ipc"`).

Create `crates/server/Cargo.toml`:
```toml
[package]
name = "signalbox-server"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "server"
path = "src/lib.rs"

[[bin]]
name = "signalbox-game"
path = "src/bin/signalbox-game.rs"

[dependencies]
signalbox-core = { path = "../core" }
signalbox-game = { path = "../game" }
signalbox-ipc = { path = "../ipc" }
signalbox-protocol = { path = "../protocol" }
serde_json.workspace = true
tokio = { workspace = true, features = ["rt", "macros", "net", "time", "sync", "signal", "io-util", "process"] }

[dev-dependencies]
rusqlite.workspace = true
```

Create `crates/server/src/lib.rs`:
```rust
//! signalbox's server side: the game process (`process`, run by the
//! `signalbox-game` binary) and, from Task 5, the front.

pub mod process;
```

Create `crates/server/src/process.rs` containing only `//! placeholder`, `crates/server/src/bin/signalbox-game.rs` containing `fn main() {}`, and the tests `crates/server/tests/process.rs`:
```rust
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
        Some(CreateArgs { world: PathBuf::from("w.json"), layout_name: s("drain"), seed: 9, start: Some(s("06:05:00")) })
    );
    let err = |v: &[&str]| Args::parse(&args(v)).unwrap_err();
    assert_eq!(err(&["--socket", "x"]), "--save is required");
    assert_eq!(err(&["--save", "x"]), "--socket is required");
    assert_eq!(err(&["--save"]), "--save needs a value");
    assert_eq!(err(&["--save", "x", "--socket", "y", "--create"]), "--create needs --layout");
    assert_eq!(err(&["--save", "x", "--socket", "y", "--seed", "1"]), "--layout, --layout-name, --seed and --start need --create");
    assert_eq!(err(&["--save", "x", "--socket", "y", "--frobnicate"]), "unknown argument `--frobnicate`");
    assert_eq!(err(&["--save", "x", "--socket", "y", "--create", "--start", "24:00"]), "bad --start `24:00`");
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
```

- [ ] **Step 2: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-server --test process`
Expected: compile errors — `Args`, `Shell`, `normalise_start`, `set_start_time`, `Next`, `CreateArgs` not found in `server::process`.

- [ ] **Step 3: Implement the process module**

Replace `crates/server/src/process.rs` with:
```rust
//! The game process (spec §2.2, §6, §9): one `game::Game` behind one Unix
//! socket. `Shell` is the sync logic (what to send for each input, when to
//! exit) and is tested without sockets; `serve` is the tokio loop around it:
//! advance every 0.1 s of real time, flush every 0.2 s, `Status` every 1 s.

use std::path::{Path, PathBuf};
use std::time::Duration;

use game::{Game, GameMeta, GameStatus, Out};
use ipc::{Counters, FromGame, LogLevel, PlayerStatus, StatusMsg, ToGame, read_frame, write_frame};
use signalbox_core::time::{fmt_hms, parse_hms};
use tokio::net::UnixListener;
use tokio::net::unix::OwnedWriteHalf;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior, interval, timeout};

pub const USAGE: &str = "usage: signalbox-game --save <db> --socket <path> [--empty-exit-s <secs>] \
[--create --layout <world.json> --layout-name <name> --seed <u64> [--start HH:MM:SS]]";
/// Real seconds a game with nobody connected waits before it saves and exits.
pub const EMPTY_EXIT_S: u64 = 600;
pub const ADVANCE_EVERY: Duration = Duration::from_millis(100);
/// Flush on every second advance: 5 times a second.
pub const FLUSH_EVERY_ADVANCES: u64 = 2;
pub const STATUS_EVERY: Duration = Duration::from_secs(1);
/// How long the game waits for the front to connect.
pub const ACCEPT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateArgs {
    /// A converted world file.
    pub world: PathBuf,
    pub layout_name: String,
    pub seed: u64,
    /// Normalised "HH:MM:SS".
    pub start: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub save: PathBuf,
    pub socket: PathBuf,
    pub empty_exit: Duration,
    /// Create the save first; without it, resume the save.
    pub create: Option<CreateArgs>,
}

impl Args {
    /// Parse the arguments after the program name.
    pub fn parse(args: &[String]) -> Result<Args, String> {
        let (mut save, mut socket, mut empty_exit_s, mut create) = (None, None, EMPTY_EXIT_S, false);
        let (mut world, mut layout_name, mut seed, mut start) = (None, None, None, None);
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let mut value = || it.next().cloned().ok_or_else(|| format!("{a} needs a value"));
            match a.as_str() {
                "--save" => save = Some(PathBuf::from(value()?)),
                "--socket" => socket = Some(PathBuf::from(value()?)),
                "--empty-exit-s" => {
                    empty_exit_s = value()?.parse().map_err(|_| "--empty-exit-s takes whole seconds".to_string())?
                }
                "--create" => create = true,
                "--layout" => world = Some(PathBuf::from(value()?)),
                "--layout-name" => layout_name = Some(value()?),
                "--seed" => seed = Some(value()?.parse::<u64>().map_err(|_| "--seed takes a u64".to_string())?),
                "--start" => {
                    let v = value()?;
                    start = Some(normalise_start(&v).ok_or_else(|| format!("bad --start `{v}`"))?);
                }
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        let save = save.ok_or("--save is required")?;
        let socket = socket.ok_or("--socket is required")?;
        let create = if create {
            Some(CreateArgs {
                world: world.ok_or("--create needs --layout")?,
                layout_name: layout_name.ok_or("--create needs --layout-name")?,
                seed: seed.ok_or("--create needs --seed")?,
                start,
            })
        } else {
            if world.is_some() || layout_name.is_some() || seed.is_some() || start.is_some() {
                return Err("--layout, --layout-name, --seed and --start need --create".into());
            }
            None
        };
        Ok(Args { save, socket, empty_exit: Duration::from_secs(empty_exit_s), create })
    }
}

/// A game start time as "HH:MM:SS": what `parse_hms` accepts, before 24:00.
pub fn normalise_start(s: &str) -> Option<String> {
    let t = parse_hms(s)?;
    (t < 24 * 3600).then(|| fmt_hms(f64::from(t)))
}

/// The world JSON with `options.start_time` set to `start` (C1 amendment 11).
pub fn set_start_time(world_json: &str, start: &str) -> Result<String, String> {
    let mut v: serde_json::Value = serde_json::from_str(world_json).map_err(|e| format!("world: {e}"))?;
    let root = v.as_object_mut().ok_or("world: not a JSON object")?;
    let options = root.entry("options").or_insert_with(|| serde_json::json!({}));
    let options = options.as_object_mut().ok_or("world: `options` is not an object")?;
    options.insert("start_time".into(), serde_json::Value::String(start.to_string()));
    Ok(serde_json::to_string(&v).expect("JSON values serialise"))
}

/// Create or resume the game the arguments name.
pub fn open_game(args: &Args) -> Result<Game, String> {
    match &args.create {
        Some(c) => {
            let json = std::fs::read_to_string(&c.world).map_err(|e| format!("{}: {e}", c.world.display()))?;
            let json = match &c.start {
                Some(start) => set_start_time(&json, start)?,
                None => json,
            };
            let meta = GameMeta { layout: c.layout_name.clone(), seed: c.seed };
            Game::create(&args.save, &json, meta).map_err(|e| format!("create: {e}"))
        }
        None => Game::resume(&args.save).map_err(|e| e.to_string()),
    }
}

pub fn status_msg(st: &GameStatus) -> StatusMsg {
    let n = |x: usize| x as u64;
    StatusMsg {
        sim_time: st.sim_time,
        tick: st.tick,
        paused: st.paused,
        speed: st.speed,
        holders: st.holders.clone(),
        players: st.players.iter().map(|(name, connected)| PlayerStatus { name: name.clone(), connected: *connected }).collect(),
        counters: Counters {
            spads: n(st.stats.spads),
            collisions: n(st.stats.collisions),
            invariant_violations: n(st.stats.invariant_violations),
            player_commands: n(st.stats.player_commands),
            robot_commands: n(st.stats.robot_commands),
            save_busy_ms: st.save_busy.as_millis() as u64,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    Continue,
    Exit,
}

/// The game process's decisions, without sockets or clocks.
pub struct Shell {
    game: Game,
    empty_exit_s: f64,
    /// Real seconds with nobody connected.
    empty_s: f64,
    /// The newest snapshot tick already reported with `Saved`.
    reported: Option<u64>,
}

impl Shell {
    pub fn new(game: Game, empty_exit: Duration) -> Shell {
        let reported = game.last_snapshot_tick();
        Shell { game, empty_exit_s: empty_exit.as_secs_f64(), empty_s: 0.0, reported }
    }

    pub fn game(&self) -> &Game {
        &self.game
    }

    pub fn on_frame(&mut self, msg: ToGame) -> (Vec<FromGame>, Next) {
        let outs = match msg {
            ToGame::Connect { player } => self.game.connect(&player),
            ToGame::Client { player, msg } => self.game.handle(&player, msg),
            ToGame::Disconnect { player } => {
                let before = self.game.status().connected;
                self.game.disconnect(&player);
                if before > 0 && self.game.status().connected == 0 {
                    self.game.pause_for_empty();
                    self.game.save_now()
                } else {
                    vec![]
                }
            }
            ToGame::Shutdown => return (self.shutdown(), Next::Exit),
        };
        (self.wrap(outs), Next::Continue)
    }

    /// Run the game for `dt` real seconds; exits after `empty_exit` with
    /// nobody connected.
    pub fn on_advance(&mut self, dt: f64) -> (Vec<FromGame>, Next) {
        let outs = self.game.advance(dt);
        let mut out = self.wrap(outs);
        if self.game.status().connected == 0 {
            self.empty_s += if dt.is_finite() && dt > 0.0 { dt } else { 0.0 };
            if self.empty_s >= self.empty_exit_s {
                out.extend(self.shutdown());
                return (out, Next::Exit);
            }
        } else {
            self.empty_s = 0.0;
        }
        (out, Next::Continue)
    }

    pub fn on_flush(&mut self) -> Vec<FromGame> {
        let outs = self.game.flush();
        self.wrap(outs)
    }

    pub fn status(&self) -> FromGame {
        FromGame::Status(status_msg(&self.game.status()))
    }

    /// Save now (Shutdown, SIGTERM, the front going away, or empty).
    pub fn shutdown(&mut self) -> Vec<FromGame> {
        let outs = self.game.save_now();
        self.wrap(outs)
    }

    fn wrap(&mut self, outs: Vec<Out>) -> Vec<FromGame> {
        let mut v: Vec<FromGame> = outs.into_iter().map(|(player, msg)| FromGame::ToPlayer { player, msg }).collect();
        for message in self.game.take_save_errors() {
            v.push(FromGame::Log { level: LogLevel::Error, message: format!("save failed: {message}") });
        }
        let tick = self.game.last_snapshot_tick();
        if tick != self.reported {
            self.reported = tick;
            if let Some(tick) = tick {
                v.push(FromGame::Saved { tick });
            }
        }
        v
    }
}

fn log(m: &FromGame) {
    if let FromGame::Log { level, message } = m {
        eprintln!("signalbox-game: {level:?}: {message}");
    }
}

/// Send frames to the front; `Log` frames also go to stderr.
async fn send_all(w: &mut OwnedWriteHalf, out: Vec<FromGame>) -> Result<(), ipc::IpcError> {
    for m in out {
        log(&m);
        write_frame(w, &m).await?;
    }
    Ok(())
}

/// Open the game, listen on the socket, serve one front until told to stop.
pub async fn run(args: Args) -> Result<(), String> {
    let game = open_game(&args)?;
    let _ = std::fs::remove_file(&args.socket);
    let listener = UnixListener::bind(&args.socket).map_err(|e| format!("{}: {e}", args.socket.display()))?;
    serve(Shell::new(game, args.empty_exit), listener, &args.socket).await
}

/// Accept exactly one front connection, then run the game for it.
pub async fn serve(mut shell: Shell, listener: UnixListener, socket: &Path) -> Result<(), String> {
    let accepted = timeout(ACCEPT_TIMEOUT, listener.accept()).await;
    drop(listener);
    let _ = std::fs::remove_file(socket);
    let stream = match accepted {
        Ok(Ok((stream, _))) => stream,
        Ok(Err(e)) => return Err(format!("accept: {e}")),
        Err(_) => {
            shell.shutdown().iter().for_each(log);
            return Err("no front connected within 60 s".into());
        }
    };
    let (mut rd, mut wr) = stream.into_split();
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            match read_frame::<_, ToGame>(&mut rd).await {
                Ok(Some(m)) => {
                    if tx.send(Ok(m)).is_err() {
                        return;
                    }
                }
                Ok(None) => return,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            }
        }
    });
    let mut term = signal(SignalKind::terminate()).map_err(|e| format!("SIGTERM handler: {e}"))?;
    let mut ticker = interval(ADVANCE_EVERY);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last = Instant::now();
    let mut last_status = last;
    let mut advances = 0u64;
    loop {
        let (out, next) = tokio::select! {
            _ = ticker.tick() => {
                let now = Instant::now();
                let (mut out, next) = shell.on_advance((now - last).as_secs_f64());
                last = now;
                advances += 1;
                if advances % FLUSH_EVERY_ADVANCES == 0 {
                    out.extend(shell.on_flush());
                }
                if now - last_status >= STATUS_EVERY {
                    out.push(shell.status());
                    last_status = now;
                }
                (out, next)
            }
            m = rx.recv() => match m {
                Some(Ok(msg)) => shell.on_frame(msg),
                Some(Err(e)) => {
                    eprintln!("signalbox-game: bad frame from the front: {e}");
                    (shell.shutdown(), Next::Exit)
                }
                None => (shell.shutdown(), Next::Exit),
            },
            _ = term.recv() => (shell.shutdown(), Next::Exit),
        };
        if let Err(e) = send_all(&mut wr, out).await {
            eprintln!("signalbox-game: the front went away ({e}); saving and exiting");
            shell.shutdown().iter().for_each(log);
            return Ok(());
        }
        if next == Next::Exit {
            return Ok(());
        }
    }
}
```

Replace `crates/server/src/bin/signalbox-game.rs` with:
```rust
//! One game in its own process (spec §2.2). See `server::process`.

use std::process::ExitCode;

use server::process::{Args, USAGE};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(&args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("signalbox-game: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a tokio runtime");
    match rt.block_on(server::process::run(args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("signalbox-game: {e}");
            ExitCode::FAILURE
        }
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `scripts/cargo test -p signalbox-server --test process`
Expected: PASS (14 tests, about a second: the binary tests spawn `target/debug/signalbox-game` via `CARGO_BIN_EXE_signalbox-game`).

- [ ] **Step 5: Check the whole workspace still builds warning-free**

Run: `scripts/cargo build --workspace --all-targets --config 'build.rustflags=["-D","warnings"]'`
Expected: success, no warnings (this is how CI's `-D warnings` is reproduced through `scripts/cargo`, which forwards no environment).

- [ ] **Step 6: Commit** (Cargo.lock changed: controller reseeds the CI cache before any push)

```bash
git add Cargo.toml Cargo.lock crates/server
git commit -m "feat(server): signalbox-game, the game process behind a Unix socket

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: the front's supervisor — lobby, game children, relay, crashes, duplicate logins

**Files:**
- Modify: `crates/server/Cargo.toml` (add `rand.workspace = true` to `[dependencies]`)
- Modify: `crates/server/src/lib.rs` (module list)
- Create: `crates/server/src/layouts.rs`, `crates/server/src/outbox.rs`, `crates/server/src/supervisor.rs`
- Test: `crates/server/tests/supervisor.rs`

**Interfaces:**
- Consumes: Task 1 (`game::save::read_summary`, `SaveSummary`), Task 2 (`ClientFrame`, `ServerFrame`, `LobbyMsg`, `LobbyReply`, `GameInfo`, `GameState`, `AreaHolder`, `LayoutInfo`, `codes::*` including `GAME_STOPPED`), Task 3 (`ToGame`, `FromGame`, `StatusMsg`, `read_frame`, `write_frame`), Task 4 (`process::normalise_start`, `process::EMPTY_EXIT_S`, the `signalbox-game` binary and its exit codes / last stderr line).
- Produces:
  - `server::layouts`: `fn valid_layout_name(&str) -> bool` (1–40 of `a-z0-9-`), `fn valid_game_id(&str) -> bool` (`g-` + 12 of `a-z2-7`), `fn new_game_id() -> String`; `struct Layouts` with `Layouts::load(&Path) -> Result<Layouts, String>`, `infos(&self) -> Vec<LayoutInfo>`, `path(&self, name: &str) -> Option<PathBuf>` (only for listed names).
  - `server::outbox`: `const OUTBOX_CAP: usize = 64`; `enum Pushed { Queued, Dropped, Overflowed, Closed }`; `struct Outbox` with `new()`, `push(&self, ServerFrame) -> Pushed`, `close(&self)`, `len(&self) -> usize`, `is_empty(&self) -> bool`, `async pop(&self) -> Option<ServerFrame>` (cancel-safe; `None` once closed and drained).
  - `server::supervisor`: `const MAX_LIVE_GAMES: usize = 8`, `const CONNECT_TIMEOUT: Duration` (30 s); `struct SupervisorConfig { pub game_bin: PathBuf, pub saves_dir: PathBuf, pub sockets_dir: PathBuf, pub empty_exit_s: u64 }`; `struct Attached { pub conn: u64, pub outbox: Arc<Outbox> }`; `struct Supervisor` with
    `new(SupervisorConfig, Layouts) -> Result<Arc<Supervisor>, String>`, `layouts(&self) -> &Layouts`,
    `attach(&self, user: &str) -> Attached`, `detach(&self, user: &str, conn: u64)`, `reply(&self, user: &str, conn: u64, ServerFrame)`,
    `handle_text(self: &Arc<Self>, user: &str, conn: u64, text: &str)`, `handle_frame(self: &Arc<Self>, user: &str, conn: u64, ClientFrame)`,
    `list_games(&self) -> Vec<GameInfo>`, `async shutdown_all(&self, grace: Duration)`,
    `live_count(&self) -> usize`, `status(&self, game: &str) -> Option<StatusMsg>`, `pid(&self, game: &str) -> Option<u32>`, `game_of(&self, user: &str) -> Option<String>`.
  - Task 6's socket loop is the only other caller: it `attach`es on upgrade, feeds every text frame to `handle_text`, answers binary frames with `reply(.., error bad_message)`, writes whatever `outbox.pop()` yields, and `detach`es when the socket ends.

Behaviour notes for the implementer (they are what the tests pin):
- One `std::sync::Mutex<State>`; never hold it across an `.await` (every `.await` in the file is outside a `lock()` scope — keep it that way).
- A game's `ToGame` sender exists from the moment its entry is inserted (`Starting`), so `Connect` and client messages queue in order while the child starts; the writer task drains them once connected.
- `attach` for a user who already has a socket: the old outbox gets `notice replaced` and is closed; the new socket inherits the old one's game, receives `joined`, and the game gets a `Connect` (never a `Disconnect` — C1's contract on `Game::connect`/`disconnect`). `detach` and every handler check `conn`, so a replaced socket can no longer act.
- Crash rule (decision 14): a child whose socket ends and which exits 0 is `finished` (removed from the live table; the lobby lists its save as `saved`; anyone still in it gets `error game_stopped`); anything else — spawn failure, never listening, a bad frame, a non-zero exit, a signal — is `crashed` with the child's last stderr line (else the front's own reason); its players get `notice game_crashed` and are back in the lobby.
- The lobby lists every `saves/<valid id>.sqlite` through `read_summary` (an unreadable one is listed `crashed` with the reader's error) and overlays the live table.

- [ ] **Step 1: Add the dependency and write the failing tests**

In `crates/server/Cargo.toml`, add `rand.workspace = true` under `[dependencies]` (after `signalbox-protocol`). `rand` 0.9 is already a workspace dependency; `rand::random` (the thread RNG, a CSPRNG seeded from the OS) makes game ids here and session ids in Task 6.

Create `crates/server/tests/supervisor.rs`:
```rust
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
```

- [ ] **Step 2: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-server --test supervisor`
Expected: compile errors — no modules `server::layouts`, `server::outbox`, `server::supervisor`.

- [ ] **Step 3: Implement `crates/server/src/layouts.rs`**

```rust
//! Layouts the lobby offers (converted worlds in one directory, read once at
//! startup) and the names the front accepts from clients.

use std::path::{Path, PathBuf};

use protocol::LayoutInfo;

/// A layout name: 1–40 of `a-z`, `0-9`, `-` (it names a file).
pub fn valid_layout_name(s: &str) -> bool {
    (1..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A game id: `g-` and 12 of `a-z2-7` (spec §2.2; it names a file).
pub fn valid_game_id(s: &str) -> bool {
    s.len() == 14 && s.starts_with("g-") && s.bytes().skip(2).all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
}

pub fn new_game_id() -> String {
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let tail: String = (0..12).map(|_| ALPHABET[usize::from(rand::random::<u8>() % 32)] as char).collect();
    format!("g-{tail}")
}

#[derive(Clone, Debug)]
pub struct Layouts {
    dir: PathBuf,
    list: Vec<LayoutInfo>,
}

impl Layouts {
    /// Every `<name>.json` in `dir` whose name is valid, in name order. A
    /// file that is not a world with named areas is an error: the image is
    /// broken and the front should not start.
    pub fn load(dir: &Path) -> Result<Layouts, String> {
        let entries = std::fs::read_dir(dir).map_err(|e| format!("layouts {}: {e}", dir.display()))?;
        let mut list = Vec::new();
        for entry in entries {
            let path = entry.map_err(|e| format!("layouts {}: {e}", dir.display()))?.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".json")) else { continue };
            if !valid_layout_name(name) {
                continue;
            }
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            let areas = v["areas"]
                .as_array()
                .and_then(|a| a.iter().map(|x| x["name"].as_str().map(str::to_string)).collect::<Option<Vec<String>>>())
                .ok_or_else(|| format!("{}: no named areas", path.display()))?;
            list.push(LayoutInfo { name: name.to_string(), areas });
        }
        list.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Layouts { dir: dir.to_path_buf(), list })
    }

    pub fn infos(&self) -> Vec<LayoutInfo> {
        self.list.clone()
    }

    /// The world file of a listed layout; `None` for anything else.
    pub fn path(&self, name: &str) -> Option<PathBuf> {
        self.list.iter().any(|l| l.name == name).then(|| self.dir.join(format!("{name}.json")))
    }
}
```

- [ ] **Step 4: Implement `crates/server/src/outbox.rs`**

```rust
//! One client's outbound queue (spec §4.3): at most `OUTBOX_CAP` frames. On
//! overflow the queue is cleared and deltas are dropped until the next full
//! view, which the caller asks the game for (`Pushed::Overflowed`).

use std::collections::VecDeque;
use std::sync::Mutex;

use protocol::{ServerFrame, ServerMsg};
use tokio::sync::Notify;

pub const OUTBOX_CAP: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pushed {
    Queued,
    /// A delta while waiting for a full view: dropped.
    Dropped,
    /// The queue was full: it (and this frame) were dropped. Ask the game
    /// for a resync.
    Overflowed,
    /// The client is gone.
    Closed,
}

#[derive(Default)]
struct Inner {
    q: VecDeque<ServerFrame>,
    closed: bool,
    awaiting_view: bool,
}

#[derive(Default)]
pub struct Outbox {
    inner: Mutex<Inner>,
    notify: Notify,
}

impl Outbox {
    pub fn new() -> Outbox {
        Outbox::default()
    }

    pub fn push(&self, frame: ServerFrame) -> Pushed {
        let mut i = self.inner.lock().expect("outbox lock");
        if i.closed {
            return Pushed::Closed;
        }
        if i.awaiting_view {
            match &frame {
                ServerFrame::Game(ServerMsg::Delta(_)) => return Pushed::Dropped,
                ServerFrame::Game(ServerMsg::View(_)) => i.awaiting_view = false,
                _ => {}
            }
        }
        if i.q.len() >= OUTBOX_CAP {
            i.q.clear();
            i.awaiting_view = true;
            return Pushed::Overflowed;
        }
        i.q.push_back(frame);
        drop(i);
        self.notify.notify_one();
        Pushed::Queued
    }

    /// No more frames are accepted; `pop` drains what is queued, then ends.
    pub fn close(&self) {
        self.inner.lock().expect("outbox lock").closed = true;
        self.notify.notify_one();
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("outbox lock").q.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The next frame, waiting for one; `None` once closed and drained.
    /// Cancel-safe (a frame is only taken when it is returned).
    pub async fn pop(&self) -> Option<ServerFrame> {
        loop {
            {
                let mut i = self.inner.lock().expect("outbox lock");
                if let Some(f) = i.q.pop_front() {
                    return Some(f);
                }
                if i.closed {
                    return None;
                }
            }
            self.notify.notified().await;
        }
    }
}
```

- [ ] **Step 5: Implement `crates/server/src/supervisor.rs`**

```rust
//! The front's supervisor (spec §2.2, §3.6, §8): the lobby, one child
//! process per live game, who is in which game, routing between client
//! sockets and game sockets, duplicate logins, crashes and shutdown.
//!
//! All state sits behind one `std::sync::Mutex` that is never held across an
//! `.await`. Each game's `ToGame` channel exists from the moment the game is
//! created or joined (`Starting`), so `Connect` and client messages sent
//! while the child is still starting queue in order.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use game::save::read_summary;
use ipc::{FromGame, StatusMsg, ToGame, read_frame, write_frame};
use protocol::{
    AreaHolder, ClientFrame, ClientMsg, GameInfo, GameState, LobbyMsg, LobbyReply, Notice, ServerFrame, ServerMsg, codes,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::OwnedReadHalf;
use tokio::process::{Child, Command};
use tokio::sync::{Notify, mpsc};
use tokio::time::{Instant, sleep, timeout};

use crate::layouts::{Layouts, new_game_id, valid_game_id};
use crate::outbox::{Outbox, Pushed};
use crate::process::normalise_start;

/// Game processes running at once, at most.
pub const MAX_LIVE_GAMES: usize = 8;
/// How long a new game process has to start listening.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct SupervisorConfig {
    pub game_bin: PathBuf,
    pub saves_dir: PathBuf,
    /// 0700; one socket per game.
    pub sockets_dir: PathBuf,
    /// Passed to every game as `--empty-exit-s` (`process::EMPTY_EXIT_S`
    /// outside tests).
    pub empty_exit_s: u64,
}

/// A client socket's handle on the supervisor.
pub struct Attached {
    /// Identifies this socket among the user's connections over time.
    pub conn: u64,
    pub outbox: Arc<Outbox>,
}

struct Client {
    conn: u64,
    outbox: Arc<Outbox>,
    game: Option<String>,
}

enum Phase {
    Starting,
    Running,
    Crashed(String),
}

struct Entry {
    layout: String,
    phase: Phase,
    tx: Option<mpsc::UnboundedSender<ToGame>>,
    status: Option<StatusMsg>,
    pid: Option<u32>,
    /// Notified to kill the process (shutdown grace over).
    kill: Arc<Notify>,
}

#[derive(Default)]
struct State {
    clients: BTreeMap<String, Client>,
    games: BTreeMap<String, Entry>,
    /// Shutting down: no new games.
    closing: bool,
}

enum Start {
    Create { world: PathBuf, layout: String, seed: u64, start: Option<String> },
    Resume,
}

pub struct Supervisor {
    cfg: SupervisorConfig,
    layouts: Layouts,
    state: Mutex<State>,
    next_conn: AtomicU64,
}

fn frame(msg: LobbyReply) -> ServerFrame {
    ServerFrame::Lobby(msg)
}

fn notice(n: Notice) -> ServerFrame {
    ServerFrame::Game(ServerMsg::Notice(n))
}

fn send_to(st: &State, game: &str, msg: ToGame) -> bool {
    st.games.get(game).and_then(|e| e.tx.as_ref()).is_some_and(|tx| tx.send(msg).is_ok())
}

/// Games that have a save file in `dir`, with its summary or why it could
/// not be read.
fn scan_saves(dir: &Path) -> BTreeMap<String, Result<game::save::SaveSummary, String>> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(id) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".sqlite")) else { continue };
        if valid_game_id(id) {
            out.insert(id.to_string(), read_summary(&path).map_err(|e| e.to_string()));
        }
    }
    out
}

fn private_dir(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|e| format!("{}: {e}", path.display()))
}

async fn connect(socket: &Path, child: &mut Child) -> Result<UnixStream, String> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        if let Ok(s) = UnixStream::connect(socket).await {
            return Ok(s);
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("the game exited before listening ({status})"));
        }
        if Instant::now() >= deadline {
            return Err("the game did not listen within 30 s".into());
        }
        sleep(Duration::from_millis(50)).await;
    }
}

impl Supervisor {
    /// Creates the saves and sockets directories (mode 0700) if needed.
    pub fn new(cfg: SupervisorConfig, layouts: Layouts) -> Result<Arc<Supervisor>, String> {
        private_dir(&cfg.saves_dir)?;
        private_dir(&cfg.sockets_dir)?;
        Ok(Arc::new(Supervisor { cfg, layouts, state: Mutex::new(State::default()), next_conn: AtomicU64::new(0) }))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().expect("supervisor lock")
    }

    pub fn layouts(&self) -> &Layouts {
        &self.layouts
    }

    fn save_path(&self, id: &str) -> PathBuf {
        self.cfg.saves_dir.join(format!("{id}.sqlite"))
    }

    // ---- client sockets ----

    /// A socket for `user` opened. A socket the user already had gets
    /// `notice replaced` and is closed; the new one takes over its game
    /// (the game sees a `Connect` and resyncs, never a `Disconnect`).
    pub fn attach(&self, user: &str) -> Attached {
        let conn = self.next_conn.fetch_add(1, Ordering::Relaxed) + 1;
        let outbox = Arc::new(Outbox::new());
        let mut st = self.lock();
        let mut game = st.clients.remove(user).and_then(|old| {
            old.outbox.push(notice(Notice::Replaced));
            old.outbox.close();
            old.game
        });
        if let Some(g) = game.clone() {
            if send_to(&st, &g, ToGame::Connect { player: user.to_string() }) {
                outbox.push(frame(LobbyReply::Joined { game: g, you: user.to_string() }));
            } else {
                game = None;
            }
        }
        if st.closing {
            outbox.close();
        }
        st.clients.insert(user.to_string(), Client { conn, outbox: outbox.clone(), game });
        Attached { conn, outbox }
    }

    /// The socket `conn` of `user` closed. Only the user's current socket
    /// disconnects them from their game.
    pub fn detach(&self, user: &str, conn: u64) {
        let mut st = self.lock();
        if st.clients.get(user).is_some_and(|c| c.conn == conn) {
            if let Some(g) = st.clients.remove(user).and_then(|c| c.game) {
                send_to(&st, &g, ToGame::Disconnect { player: user.to_string() });
            }
        }
    }

    fn current<'a>(st: &'a State, user: &str, conn: u64) -> Option<&'a Client> {
        st.clients.get(user).filter(|c| c.conn == conn)
    }

    /// Send `f` to `user`'s socket `conn`, if it is still their current one.
    pub fn reply(&self, user: &str, conn: u64, f: ServerFrame) {
        if let Some(c) = Self::current(&self.lock(), user, conn) {
            c.outbox.push(f);
        }
    }

    /// One text frame from a client.
    pub fn handle_text(self: &Arc<Self>, user: &str, conn: u64, text: &str) {
        match ClientFrame::from_json(text) {
            Ok(f) => self.handle_frame(user, conn, f),
            Err(e) => self.reply(user, conn, ServerFrame::error(e.code(), e.to_string())),
        }
    }

    pub fn handle_frame(self: &Arc<Self>, user: &str, conn: u64, f: ClientFrame) {
        match f {
            ClientFrame::Game(msg) => self.to_game(user, conn, msg),
            ClientFrame::Lobby(LobbyMsg::ListGames) => self.reply(user, conn, frame(LobbyReply::Games { games: self.list_games() })),
            ClientFrame::Lobby(LobbyMsg::ListLayouts) => {
                self.reply(user, conn, frame(LobbyReply::Layouts { layouts: self.layouts.infos() }))
            }
            ClientFrame::Lobby(LobbyMsg::Leave) => {
                {
                    let mut st = self.lock();
                    if Self::current(&st, user, conn).is_some() {
                        Self::leave(&mut st, user);
                    }
                }
                self.reply(user, conn, frame(LobbyReply::Games { games: self.list_games() }));
            }
            ClientFrame::Lobby(LobbyMsg::CreateGame { layout, seed, start }) => self.create(user, conn, layout, seed, start),
            ClientFrame::Lobby(LobbyMsg::Join { game }) => self.join(user, conn, game),
        }
    }

    fn to_game(&self, user: &str, conn: u64, msg: ClientMsg) {
        let st = self.lock();
        let Some(c) = Self::current(&st, user, conn) else { return };
        let sent = c.game.as_deref().is_some_and(|g| send_to(&st, g, ToGame::Client { player: user.to_string(), msg }));
        if !sent {
            c.outbox.push(ServerFrame::error(codes::NOT_IN_GAME, "join a game first"));
        }
    }

    fn leave(st: &mut State, user: &str) {
        if let Some(g) = st.clients.get_mut(user).and_then(|c| c.game.take()) {
            send_to(st, &g, ToGame::Disconnect { player: user.to_string() });
        }
    }

    fn enter(st: &mut State, user: &str, game: &str) {
        let Some(c) = st.clients.get_mut(user) else { return };
        c.game = Some(game.to_string());
        c.outbox.push(frame(LobbyReply::Joined { game: game.to_string(), you: user.to_string() }));
        send_to(st, game, ToGame::Connect { player: user.to_string() });
    }

    fn room_for_one_more(st: &State) -> Result<(), ServerFrame> {
        if st.closing {
            return Err(ServerFrame::error(codes::TOO_MANY_GAMES, "the server is stopping"));
        }
        let live = st.games.values().filter(|e| !matches!(e.phase, Phase::Crashed(_))).count();
        if live >= MAX_LIVE_GAMES {
            return Err(ServerFrame::error(codes::TOO_MANY_GAMES, format!("at most {MAX_LIVE_GAMES} games run at once")));
        }
        Ok(())
    }

    fn insert_starting(st: &mut State, id: &str, layout: String) -> mpsc::UnboundedReceiver<ToGame> {
        let (tx, rx) = mpsc::unbounded_channel();
        let kill = Arc::new(Notify::new());
        st.games.insert(id.to_string(), Entry { layout, phase: Phase::Starting, tx: Some(tx), status: None, pid: None, kill });
        rx
    }

    fn create(self: &Arc<Self>, user: &str, conn: u64, layout: String, seed: Option<u64>, start: Option<String>) {
        let Some(world) = self.layouts.path(&layout) else {
            return self.reply(user, conn, ServerFrame::error(codes::UNKNOWN_LAYOUT, format!("no layout `{layout}`")));
        };
        let start = match start.as_deref().map(normalise_start) {
            None => None,
            Some(Some(s)) => Some(s),
            Some(None) => {
                return self.reply(user, conn, ServerFrame::error(codes::BAD_START, "a start time is HH:MM or HH:MM:SS before 24:00"));
            }
        };
        let mut st = self.lock();
        let Some(c) = Self::current(&st, user, conn) else { return };
        if let Err(f) = Self::room_for_one_more(&st) {
            c.outbox.push(f);
            return;
        }
        let id = loop {
            let id = new_game_id();
            if !st.games.contains_key(&id) && !self.save_path(&id).exists() {
                break id;
            }
        };
        Self::leave(&mut st, user);
        let rx = Self::insert_starting(&mut st, &id, layout.clone());
        Self::enter(&mut st, user, &id);
        drop(st);
        let seed = seed.unwrap_or_else(rand::random);
        self.spawn_game(id, rx, Start::Create { world, layout, seed, start });
    }

    fn join(self: &Arc<Self>, user: &str, conn: u64, game: String) {
        if !valid_game_id(&game) {
            return self.reply(user, conn, ServerFrame::error(codes::UNKNOWN_GAME, format!("no game `{game}`")));
        }
        {
            let mut st = self.lock();
            if Self::current(&st, user, conn).is_none() {
                return;
            }
            if matches!(st.games.get(&game).map(|e| &e.phase), Some(Phase::Starting | Phase::Running)) {
                Self::leave(&mut st, user);
                Self::enter(&mut st, user, &game);
                return;
            }
        }
        let path = self.save_path(&game);
        if !path.exists() {
            return self.reply(user, conn, ServerFrame::error(codes::UNKNOWN_GAME, format!("no game `{game}`")));
        }
        let layout = read_summary(&path).map(|s| s.layout).unwrap_or_else(|_| "?".into());
        let mut st = self.lock();
        let Some(c) = Self::current(&st, user, conn) else { return };
        if matches!(st.games.get(&game).map(|e| &e.phase), Some(Phase::Starting | Phase::Running)) {
            // Someone resumed it meanwhile.
            Self::leave(&mut st, user);
            Self::enter(&mut st, user, &game);
            return;
        }
        if let Err(f) = Self::room_for_one_more(&st) {
            c.outbox.push(f);
            return;
        }
        Self::leave(&mut st, user);
        let rx = Self::insert_starting(&mut st, &game, layout);
        Self::enter(&mut st, user, &game);
        drop(st);
        self.spawn_game(game, rx, Start::Resume);
    }

    // ---- the lobby ----

    /// Every game: live ones from memory, the rest from their save files.
    pub fn list_games(&self) -> Vec<GameInfo> {
        let saves = scan_saves(&self.cfg.saves_dir);
        let st = self.lock();
        let mut out: BTreeMap<String, GameInfo> = BTreeMap::new();
        for (id, sum) in &saves {
            let info = match sum {
                Ok(s) => GameInfo {
                    id: id.clone(),
                    layout: s.layout.clone(),
                    state: GameState::Saved,
                    sim_time: s.sim_time,
                    areas: s.areas.iter().map(|a| AreaHolder { name: a.clone(), holder: None }).collect(),
                    players: vec![],
                    error: None,
                },
                Err(e) => GameInfo {
                    id: id.clone(),
                    layout: "?".into(),
                    state: GameState::Crashed,
                    sim_time: 0.0,
                    areas: vec![],
                    players: vec![],
                    error: Some(e.clone()),
                },
            };
            out.insert(id.clone(), info);
        }
        for (id, e) in &st.games {
            let base = out.remove(id);
            let areas_order: Vec<String> = match saves.get(id) {
                Some(Ok(s)) => s.areas.clone(),
                _ => e.status.as_ref().map(|s| s.holders.keys().cloned().collect()).unwrap_or_default(),
            };
            let mut info = base.unwrap_or(GameInfo {
                id: id.clone(),
                layout: e.layout.clone(),
                state: GameState::Saved,
                sim_time: 0.0,
                areas: areas_order.iter().map(|a| AreaHolder { name: a.clone(), holder: None }).collect(),
                players: vec![],
                error: None,
            });
            info.layout = e.layout.clone();
            match &e.phase {
                Phase::Starting | Phase::Running => {
                    info.state = GameState::Running;
                    info.error = None;
                    if let Some(s) = &e.status {
                        info.sim_time = s.sim_time;
                        info.areas = areas_order
                            .iter()
                            .map(|a| AreaHolder { name: a.clone(), holder: s.holders.get(a).cloned().flatten() })
                            .collect();
                        info.players = s.players.iter().filter(|p| p.connected).map(|p| p.name.clone()).collect();
                    }
                }
                Phase::Crashed(why) => {
                    info.state = GameState::Crashed;
                    info.error = Some(why.clone());
                    info.players = vec![];
                }
            }
            out.insert(id.clone(), info);
        }
        out.into_values().collect()
    }

    // ---- game processes ----

    fn spawn_game(self: &Arc<Self>, id: String, rx: mpsc::UnboundedReceiver<ToGame>, start: Start) {
        let sup = self.clone();
        tokio::spawn(async move { sup.run_game(id, rx, start).await });
    }

    async fn run_game(self: Arc<Self>, id: String, rx: mpsc::UnboundedReceiver<ToGame>, start: Start) {
        let socket = self.cfg.sockets_dir.join(format!("{id}.sock"));
        let _ = std::fs::remove_file(&socket);
        let mut cmd = Command::new(&self.cfg.game_bin);
        cmd.arg("--save").arg(self.save_path(&id)).arg("--socket").arg(&socket);
        cmd.arg("--empty-exit-s").arg(self.cfg.empty_exit_s.to_string());
        if let Start::Create { world, layout, seed, start } = &start {
            cmd.arg("--create").arg("--layout").arg(world).arg("--layout-name").arg(layout).arg("--seed").arg(seed.to_string());
            if let Some(s) = start {
                cmd.arg("--start").arg(s);
            }
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return self.crashed(&id, format!("cannot start {}: {e}", self.cfg.game_bin.display())),
        };
        let last_line = Arc::new(Mutex::new(String::new()));
        let stderr = child.stderr.take().map(|err| {
            let (id, last_line) = (id.clone(), last_line.clone());
            tokio::spawn(async move {
                let mut lines = BufReader::new(err).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    eprintln!("[{id}] {line}");
                    *last_line.lock().expect("stderr lock") = line;
                }
            })
        });
        let kill = match self.lock().games.get(&id) {
            Some(e) => e.kill.clone(),
            None => Arc::new(Notify::new()),
        };
        let connected = tokio::select! {
            s = connect(&socket, &mut child) => s,
            _ = kill.notified() => Err("stopped while starting".into()),
        };
        let trouble = match connected {
            Ok(stream) => {
                self.set_running(&id, child.id());
                let (mut rd, mut wr) = stream.into_split();
                let mut rx = rx;
                let writer = tokio::spawn(async move {
                    while let Some(m) = rx.recv().await {
                        if write_frame(&mut wr, &m).await.is_err() {
                            break;
                        }
                    }
                });
                let r = tokio::select! {
                    r = self.read_game(&id, &mut rd) => r,
                    _ = kill.notified() => Some("killed: it did not stop in time".to_string()),
                };
                writer.abort();
                r
            }
            Err(e) => Some(e),
        };
        if trouble.is_some() {
            let _ = child.start_kill();
        }
        let status = match timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(s)) => Some(s),
            _ => {
                let _ = child.start_kill();
                child.wait().await.ok()
            }
        };
        if let Some(t) = stderr {
            let _ = timeout(Duration::from_secs(1), t).await;
        }
        if trouble.is_none() && status.is_some_and(|s| s.success()) {
            self.finished(&id);
            return;
        }
        let line = last_line.lock().expect("stderr lock").clone();
        let why = match (line.is_empty(), trouble) {
            (false, _) => line,
            (true, Some(t)) => t,
            (true, None) => match status {
                Some(s) => format!("the game process ended: {s}"),
                None => "the game process ended".into(),
            },
        };
        self.crashed(&id, why);
    }

    /// Frames from a game until it closes its socket; `Some(why)` if it
    /// sent something unreadable.
    async fn read_game(&self, id: &str, rd: &mut OwnedReadHalf) -> Option<String> {
        loop {
            match read_frame::<_, FromGame>(rd).await {
                Ok(Some(m)) => self.from_game(id, m),
                Ok(None) => return None,
                Err(e) => return Some(format!("bad frame from the game: {e}")),
            }
        }
    }

    fn from_game(&self, id: &str, m: FromGame) {
        match m {
            FromGame::ToPlayer { player, msg } => {
                let st = self.lock();
                let Some(c) = st.clients.get(&player).filter(|c| c.game.as_deref() == Some(id)) else { return };
                if c.outbox.push(ServerFrame::Game(msg)) == Pushed::Overflowed {
                    send_to(&st, id, ToGame::Client { player, msg: ClientMsg::Resync });
                }
            }
            FromGame::Status(s) => {
                if let Some(e) = self.lock().games.get_mut(id) {
                    e.status = Some(s);
                }
            }
            // The save file is the record; nothing to keep.
            FromGame::Saved { .. } => {}
            FromGame::Log { level, message } => eprintln!("[{id}] {level:?}: {message}"),
        }
    }

    fn set_running(&self, id: &str, pid: Option<u32>) {
        if let Some(e) = self.lock().games.get_mut(id) {
            e.phase = Phase::Running;
            e.pid = pid;
        }
    }

    /// Everyone in `id` goes back to the lobby with `f`.
    fn evict(st: &mut State, id: &str, f: &ServerFrame) {
        for c in st.clients.values_mut() {
            if c.game.as_deref() == Some(id) {
                c.game = None;
                c.outbox.push(f.clone());
            }
        }
    }

    /// The game exited cleanly: it is saved and leaves the live table.
    fn finished(&self, id: &str) {
        let mut st = self.lock();
        st.games.remove(id);
        let f = ServerFrame::error(codes::GAME_STOPPED, "the game stopped; join it again to resume it");
        Self::evict(&mut st, id, &f);
    }

    fn crashed(&self, id: &str, why: String) {
        eprintln!("[{id}] crashed: {why}");
        let mut st = self.lock();
        let layout = st.games.get(id).map_or_else(|| "?".to_string(), |e| e.layout.clone());
        st.games.insert(
            id.to_string(),
            Entry { layout, phase: Phase::Crashed(why), tx: None, status: None, pid: None, kill: Arc::new(Notify::new()) },
        );
        Self::evict(&mut st, id, &notice(Notice::GameCrashed));
    }

    /// Send `Shutdown` to every game, close every client, wait up to
    /// `grace` for the games to save and exit, then kill the rest.
    pub async fn shutdown_all(&self, grace: Duration) {
        let kills: Vec<Arc<Notify>> = {
            let mut st = self.lock();
            st.closing = true;
            for e in st.games.values() {
                if let Some(tx) = &e.tx {
                    let _ = tx.send(ToGame::Shutdown);
                }
            }
            for c in st.clients.values() {
                c.outbox.close();
            }
            st.games.values().map(|e| e.kill.clone()).collect()
        };
        let deadline = Instant::now() + grace;
        while self.live_count() > 0 && Instant::now() < deadline {
            sleep(Duration::from_millis(50)).await;
        }
        for k in &kills {
            k.notify_one();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.live_count() > 0 && Instant::now() < deadline {
            sleep(Duration::from_millis(50)).await;
        }
    }

    // ---- for tests and the index page ----

    /// Games with a process (starting or running).
    pub fn live_count(&self) -> usize {
        self.lock().games.values().filter(|e| !matches!(e.phase, Phase::Crashed(_))).count()
    }

    pub fn status(&self, game: &str) -> Option<StatusMsg> {
        self.lock().games.get(game)?.status.clone()
    }

    pub fn pid(&self, game: &str) -> Option<u32> {
        self.lock().games.get(game)?.pid
    }

    /// The game `user` is in.
    pub fn game_of(&self, user: &str) -> Option<String> {
        self.lock().clients.get(user)?.game.clone()
    }
}
```

- [ ] **Step 6: Register the modules**

Replace `crates/server/src/lib.rs` with:
```rust
//! signalbox's server side: the game process (`process`, run by the
//! `signalbox-game` binary) and the front's parts: the layouts it offers,
//! per-client outbound queues and the supervisor of game processes.

pub mod layouts;
pub mod outbox;
pub mod process;
pub mod supervisor;
```

- [ ] **Step 7: Run the tests**

Run: `scripts/cargo test -p signalbox-server --test supervisor`
Expected: PASS (15 tests, a few seconds; each spawns real `signalbox-game` children in a temp dir and shuts them down).

Run: `scripts/cargo test -p signalbox-server`
Expected: PASS (the Task 4 `process` tests still pass).

Run: `scripts/cargo build --workspace --all-targets --config 'build.rustflags=["-D","warnings"]'`
Expected: success, no warnings.

- [ ] **Step 8: Commit** (Cargo.lock changes only if `rand` was not yet in the server's tree; the controller reseeds the CI cache before any push either way)

```bash
git add crates/server Cargo.lock
git commit -m "feat(server): the front's supervisor: lobby, game children, relay, crashes and duplicate logins

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: the front over HTTP — config, sessions, dev login, the `/ws` gate and limits, `signalbox-server`

**Files:**
- Modify: `Cargo.toml` (workspace dependencies `axum`, `axum-extra`, `tokio-tungstenite`, `futures-util`)
- Modify: `crates/server/Cargo.toml` (bin `signalbox-server`, feature `dev-auth`, test `front`, dependencies)
- Modify: `crates/bot/Cargo.toml`, `crates/bot/src/lib.rs` (`pub mod net`)
- Create: `crates/bot/src/net.rs`
- Create: `crates/server/src/config.rs`, `crates/server/src/session.rs`, `crates/server/src/limit.rs`, `crates/server/src/web.rs`, `crates/server/src/bin/signalbox-server.rs`
- Modify: `crates/server/src/lib.rs` (modules, `start`, `Running`, `run`)
- Modify: `scripts/ci/test.sh` (dev-auth build and tests)
- Test: `crates/server/tests/common/mod.rs`, `crates/server/tests/front.rs`, `crates/server/tests/units.rs`, `crates/server/tests/release.rs`

**Interfaces:**
- Consumes: Task 5 (`Supervisor`, `SupervisorConfig`, `Attached`, `Layouts`, `Outbox::pop`), Task 4 (`process::EMPTY_EXIT_S`), Task 2 (`ClientFrame`, `ServerFrame`, `FrameError`, `codes`).
- Produces:
  - `server::config`: `struct OidcConfig { pub issuer: String, pub client_id: String, pub client_secret: String }`; `struct Config { pub addr: SocketAddr, pub data_dir: PathBuf, pub layouts_dir: PathBuf, pub public_url: String, pub oidc: Option<OidcConfig>, pub session_key: Vec<u8>, pub game_bin: PathBuf }` with `Config::from_env() -> Result<Config, String>` and `Config::from_lookup(impl Fn(&str) -> Option<String>) -> Result<Config, String>`; consts `DEFAULT_ADDR`, `DEFAULT_DATA`, `DEFAULT_LAYOUTS`, `MIN_KEY_BYTES = 64`; `fn decode_hex(&str) -> Option<Vec<u8>>`. `oidc` is `None` only in `dev-auth` builds with none of the three OIDC variables set.
  - `server::session`: `SESSION_COOKIE = "signalbox_session"`, `SESSION_TTL` (12 h), `fn random_hex(n: usize) -> String`, `fn cookie(name: &str, value: &str, max_age_s: u64) -> Cookie<'static>` (Path=/, HttpOnly, Secure, SameSite=Lax, Max-Age), `fn removal(name: &str) -> Cookie<'static>`; `struct Sessions` with `new`, `create(&self, user) -> String`, `create_at(&self, user, Instant) -> String`, `user(&self, id) -> Option<String>`, `user_at(&self, id, Instant) -> Option<String>`, `remove(&self, id)`, `len`, `is_empty`.
  - `server::limit`: `MAX_MSGS_PER_S: u32 = 20`; `struct RateLimit` with `new(Instant)`, `allow(&mut self, Instant) -> bool`.
  - `server::web`: `MAX_CLIENT_MESSAGE = 64 KiB`, `SEND_TIMEOUT` (10 s), `CLOSE_POLICY = 1008`; `#[derive(Clone)] struct AppState { pub sup: Arc<Supervisor>, pub sessions: Arc<Sessions>, pub key: Key }` (`FromRef<AppState> for Key`); `fn router(AppState) -> Router`; `fn user_of(&AppState, &SignedCookieJar) -> Option<String>`; `fn escape_html(&str) -> String`; `fn index_page(user: &str, games: &[GameInfo], layouts: &[LayoutInfo]) -> String`; with `dev-auth`, `web::dev::{login, valid_dev_user}`. Task 7 adds a field and two routes here.
  - `server` (lib root): `STOP_GRACE` (10 s); `struct Running { pub addr: SocketAddr, pub sup: Arc<Supervisor>, pub sessions: Arc<Sessions>, .. }` with `base(&self) -> String` (`http://<addr>`) and `async stop(self)`; `async fn start(Config) -> Result<Running, String>`; `async fn run(Config) -> Result<(), String>` (until SIGTERM/SIGINT).
  - Binary `signalbox-server`: exit 2 with `signalbox-server: <why>` on bad config, 1 if `run` fails, 0 after a clean stop.
  - `bot::net`: `enum NetError { Io, Url, Http, Status(u16), Ws, Frame, Timeout(String) }` (`Timeout` is for callers that bound a wait, e.g. Task 8's player loop); `struct HttpResponse { pub status: u16, pub headers: Vec<(String, String)>, pub body: String }` with `header(&self, name) -> Option<&str>`; `async fn http_get(base: &str, path: &str, cookie: Option<&str>) -> Result<HttpResponse, NetError>`; `fn url_encode(&str) -> String`; `async fn dev_login(base: &str, user: &str) -> Result<String, NetError>` (returns `name=value` for a `Cookie` header); `struct Conn` with `async connect(base: &str, cookie: Option<&str>) -> Result<Conn, NetError>` (a refused upgrade is `Status(code)`), `async send(&mut self, &ClientFrame)`, `async send_text(&mut self, String)`, `async send_binary(&mut self, Vec<u8>)`, `async recv(&mut self) -> Result<Option<ServerFrame>, NetError>` (`Ok(None)` once closed), `async close(self)`.

Dependency notes (all rustls-free here; no TLS is involved in this task):
- `axum` 0.8.9 with `ws` (the WebSocket upgrade; it pulls `tokio-tungstenite` 0.29, which the bot then shares), `axum-extra` 0.12 with `cookie-signed` (`SignedCookieJar`, HMAC-signed cookies), `tokio-tungstenite` 0.29 default features (plain `ws://` client for the bot; no TLS feature), `futures-util` 0.3 without defaults but `sink` + `std` (`SinkExt`/`StreamExt` for the bot's socket).

- [ ] **Step 1: Dependencies and manifests**

In the root `Cargo.toml`, append to `[workspace.dependencies]`:
```toml
axum = { version = "0.8.9", features = ["ws"] }
axum-extra = { version = "0.12", features = ["cookie-signed"] }
tokio-tungstenite = "0.29"
futures-util = { version = "0.3", default-features = false, features = ["sink", "std"] }
```

Replace `crates/server/Cargo.toml` with:
```toml
[package]
name = "signalbox-server"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "server"
path = "src/lib.rs"

[[bin]]
name = "signalbox-game"
path = "src/bin/signalbox-game.rs"

[[bin]]
name = "signalbox-server"
path = "src/bin/signalbox-server.rs"

[[test]]
name = "front"
required-features = ["dev-auth"]

[features]
# Adds /auth/dev?user=<name> (tests and bots only; never in the release image).
dev-auth = []

[dependencies]
signalbox-core = { path = "../core" }
signalbox-game = { path = "../game" }
signalbox-ipc = { path = "../ipc" }
signalbox-protocol = { path = "../protocol" }
axum.workspace = true
axum-extra.workspace = true
rand.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio = { workspace = true, features = ["rt", "macros", "net", "time", "sync", "signal", "io-util", "process"] }

[dev-dependencies]
signalbox-bot = { path = "../bot" }
rusqlite.workspace = true
```

In `crates/bot/Cargo.toml`, add to `[dependencies]` (after `signalbox-protocol`):
```toml
futures-util.workspace = true
thiserror.workspace = true
tokio = { workspace = true, features = ["net", "io-util"] }
tokio-tungstenite.workspace = true
```

- [ ] **Step 2: Write the failing tests**

Create `crates/server/tests/common/mod.rs`:
```rust
//! A front running in process with dev login, real game processes, and a
//! temp data directory; plus WebSocket helpers.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use bot::net::{Conn, dev_login};
use protocol::{ClientFrame, LobbyMsg, LobbyReply, ServerFrame, ServerMsg};
use server::Running;
use server::config::Config;
use tokio::time::timeout;

pub const TWOBOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../game/tests/fixtures/twobox.json");
pub const GAME_BIN: &str = env!("CARGO_BIN_EXE_signalbox-game");

pub fn s(x: &str) -> String {
    x.to_string()
}

pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sbx-front-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A dev-auth config serving on a free local port, without OIDC.
pub fn dev_config(root: &Path, layouts_dir: PathBuf) -> Config {
    Config {
        addr: "127.0.0.1:0".parse().unwrap(),
        data_dir: root.join("data"),
        layouts_dir,
        public_url: s("http://127.0.0.1"),
        oidc: None,
        session_key: vec![7; 64],
        game_bin: PathBuf::from(GAME_BIN),
    }
}

pub struct Front {
    pub running: Running,
    pub base: String,
    pub root: PathBuf,
}

/// A front whose layouts directory holds each `(name, world JSON)`.
pub async fn front_with(name: &str, layouts: &[(&str, String)]) -> Front {
    let root = temp_dir(name);
    let dir = root.join("layouts");
    std::fs::create_dir_all(&dir).unwrap();
    for (layout, json) in layouts {
        std::fs::write(dir.join(format!("{layout}.json")), json).unwrap();
    }
    let running = server::start(dev_config(&root, dir)).await.unwrap();
    let base = running.base();
    Front { running, base, root }
}

/// A front offering the twobox layout.
pub async fn front(name: &str) -> Front {
    front_with(name, &[("twobox", std::fs::read_to_string(TWOBOX).unwrap())]).await
}

impl Front {
    pub fn saves(&self) -> PathBuf {
        self.root.join("data/saves")
    }

    /// A logged-in WebSocket for `user`.
    pub async fn connect(&self, user: &str) -> Conn {
        let cookie = dev_login(&self.base, user).await.unwrap();
        Conn::connect(&self.base, Some(&cookie)).await.unwrap()
    }
}

/// The next frame (10 s); `None` once the server closed the socket.
pub async fn next(c: &mut Conn) -> Option<ServerFrame> {
    timeout(Duration::from_secs(10), c.recv()).await.expect("no frame within 10 s").ok().flatten()
}

/// Frames until one matches `stop` (included).
pub async fn until(c: &mut Conn, stop: impl Fn(&ServerFrame) -> bool) -> Vec<ServerFrame> {
    let mut got = Vec::new();
    loop {
        let f = next(c).await.unwrap_or_else(|| panic!("closed; got {got:?}"));
        let done = stop(&f);
        got.push(f);
        if done {
            return got;
        }
    }
}

pub fn is_view(f: &ServerFrame) -> bool {
    matches!(f, ServerFrame::Game(ServerMsg::View(_)))
}

pub fn lobby(msg: LobbyMsg) -> ClientFrame {
    ClientFrame::Lobby(msg)
}

/// Create a game of `layout` and wait for its first view; returns its id.
pub async fn create(c: &mut Conn, layout: &str) -> String {
    c.send(&lobby(LobbyMsg::CreateGame { layout: s(layout), seed: Some(5), start: None })).await.unwrap();
    let got = until(c, is_view).await;
    match got.first() {
        Some(ServerFrame::Lobby(LobbyReply::Joined { game, .. })) => game.clone(),
        other => panic!("{other:?}"),
    }
}
```

Create `crates/server/tests/front.rs` (built only with `--features dev-auth`, via `required-features` above):
```rust
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
    for q in ["", "?user=", "?user=a%20b", "?user=%3Cscript%3E", &format!("?user={}", "a".repeat(33))] {
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
```

Create `crates/server/tests/units.rs` (both builds):
```rust
//! The front's pure parts: configuration, sessions, the rate limit and the
//! placeholder page.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use protocol::{AreaHolder, GameInfo, GameState, LayoutInfo};
use server::config::Config;
use server::limit::{MAX_MSGS_PER_S, RateLimit};
use server::session::{SESSION_TTL, Sessions};
use server::web::{escape_html, index_page};

const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\
                   0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn cfg(vars: &[(&str, &str)]) -> Result<Config, String> {
    let m: BTreeMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    Config::from_lookup(|k| m.get(k).cloned())
}

const OIDC: [(&str, &str); 4] = [
    ("OIDC_ISSUER", "https://auth.example/application/o/signalbox/"),
    ("OIDC_CLIENT_ID", "sbx"),
    ("OIDC_CLIENT_SECRET", "s3cret"),
    ("SIGNALBOX_PUBLIC_URL", "https://ra.example:50160/"),
];

fn with(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut v = vec![("SIGNALBOX_SESSION_KEY", KEY)];
    v.extend(OIDC);
    v.extend(extra);
    v
}

#[test]
fn config_defaults_and_overrides() {
    let c = cfg(&with(&[])).unwrap();
    assert_eq!(c.addr.to_string(), "0.0.0.0:9160");
    assert_eq!((c.data_dir, c.layouts_dir), (PathBuf::from("/data"), PathBuf::from("/opt/signalbox/layouts")));
    assert_eq!(c.public_url, "https://ra.example:50160", "no trailing slash");
    assert_eq!(c.session_key.len(), 64);
    let o = c.oidc.unwrap();
    assert_eq!((o.issuer.as_str(), o.client_id.as_str(), o.client_secret.as_str()), (OIDC[0].1, "sbx", "s3cret"));
    assert!(c.game_bin.ends_with("signalbox-game"), "next to the running binary: {}", c.game_bin.display());
    let c = cfg(&with(&[
        ("SIGNALBOX_ADDR", "127.0.0.1:1"),
        ("SIGNALBOX_DATA", "/d"),
        ("SIGNALBOX_LAYOUTS", "/l"),
        ("SIGNALBOX_GAME_BIN", "/bin/g"),
    ]))
    .unwrap();
    assert_eq!((c.addr.to_string(), c.data_dir, c.layouts_dir, c.game_bin), (
        "127.0.0.1:1".to_string(),
        PathBuf::from("/d"),
        PathBuf::from("/l"),
        PathBuf::from("/bin/g")
    ));
}

#[test]
fn config_problems_are_one_clear_line() {
    let err = |vars: &[(&str, &str)]| cfg(vars).unwrap_err();
    let mut no_key: Vec<(&str, &str)> = OIDC.to_vec();
    assert_eq!(err(&no_key), "SIGNALBOX_SESSION_KEY is required (hex, at least 64 bytes)");
    no_key.push(("SIGNALBOX_SESSION_KEY", "zz"));
    assert_eq!(err(&no_key), "SIGNALBOX_SESSION_KEY is not hex");
    let short = &KEY[..126];
    assert_eq!(err(&with(&[("SIGNALBOX_SESSION_KEY", short)])[1..]), "SIGNALBOX_SESSION_KEY has 63 bytes; it needs at least 64");
    assert_eq!(err(&with(&[("SIGNALBOX_ADDR", "nowhere")])), "SIGNALBOX_ADDR `nowhere` is not host:port");
    assert_eq!(
        err(&with(&[("SIGNALBOX_PUBLIC_URL", "ra.example")])[..]),
        "SIGNALBOX_PUBLIC_URL `ra.example` must start with https:// or http://"
    );
    let partial = [("SIGNALBOX_SESSION_KEY", KEY), OIDC[0], OIDC[3]];
    assert_eq!(err(&partial), "OIDC_ISSUER, OIDC_CLIENT_ID and OIDC_CLIENT_SECRET are all required");
    let no_url = [("SIGNALBOX_SESSION_KEY", KEY), OIDC[0], OIDC[1], OIDC[2]];
    assert_eq!(err(&no_url), "SIGNALBOX_PUBLIC_URL is required with OIDC");
    let bare = [("SIGNALBOX_SESSION_KEY", KEY)];
    if cfg!(feature = "dev-auth") {
        let c = cfg(&bare).unwrap();
        assert_eq!((c.oidc, c.public_url.as_str()), (None, "http://0.0.0.0:9160"));
    } else {
        assert_eq!(err(&bare), "OIDC_ISSUER, OIDC_CLIENT_ID and OIDC_CLIENT_SECRET are all required");
    }
}

#[test]
fn sessions_live_twelve_hours_and_can_be_ended() {
    let s = Sessions::new();
    let t0 = Instant::now();
    let id = s.create_at("ann", t0);
    assert_eq!(id.len(), 64);
    assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_ne!(s.create_at("ann", t0), id, "every login gets its own id");
    assert_eq!(s.user_at(&id, t0 + SESSION_TTL - Duration::from_secs(1)).as_deref(), Some("ann"));
    assert_eq!(s.user_at(&id, t0 + SESSION_TTL), None, "expired");
    assert_eq!(s.user_at(&id, t0), None, "and forgotten");
    let id = s.create_at("bob", t0);
    s.remove(&id);
    assert_eq!(s.user_at(&id, t0), None);
    assert_eq!(s.user_at("not-an-id", t0), None);
}

#[test]
fn the_rate_limit_counts_a_one_second_window() {
    let t0 = Instant::now();
    let mut l = RateLimit::new(t0);
    for _ in 0..MAX_MSGS_PER_S {
        assert!(l.allow(t0 + Duration::from_millis(900)));
    }
    assert!(!l.allow(t0 + Duration::from_millis(999)), "the 21st in one second");
    assert!(l.allow(t0 + Duration::from_secs(1)), "a new window");
}

#[test]
fn the_placeholder_page_escapes_every_name() {
    assert_eq!(escape_html(r#"<a href="x">&'"#), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
    let games = [GameInfo {
        id: "g-abcdefgh2345".into(),
        layout: "<script>".into(),
        state: GameState::Crashed,
        sim_time: 0.0,
        areas: vec![AreaHolder { name: "Hackney & Bow".into(), holder: None }],
        players: vec![],
        error: Some("<b>bad</b>".into()),
    }];
    let page = index_page("a<b", &games, &[LayoutInfo { name: "drain".into(), areas: vec![] }]);
    assert!(!page.contains("<script>") && !page.contains("<b>bad"), "{page}");
    assert!(page.contains("Hackney &amp; Bow: robot") && page.contains("Signed in as a&lt;b"), "{page}");
}
```

Create `crates/server/tests/release.rs` (default features only; with `dev-auth` it compiles to an empty test crate):
```rust
//! The release build (no `dev-auth`): the dev login does not exist.

#![cfg(not(feature = "dev-auth"))]

use std::path::PathBuf;

use bot::net::{Conn, NetError, http_get};
use server::config::{Config, OidcConfig};

#[tokio::test]
async fn the_release_build_has_no_dev_login() {
    let root = std::env::temp_dir().join(format!("sbx-release-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("layouts")).unwrap();
    let cfg = Config {
        addr: "127.0.0.1:0".parse().unwrap(),
        data_dir: root.join("data"),
        layouts_dir: root.join("layouts"),
        public_url: "http://127.0.0.1".into(),
        // Nothing listens on port 9: the provider is only asked on a login.
        oidc: Some(OidcConfig { issuer: "http://127.0.0.1:9/".into(), client_id: "sbx".into(), client_secret: "x".into() }),
        session_key: vec![7; 64],
        game_bin: PathBuf::from(env!("CARGO_BIN_EXE_signalbox-game")),
    };
    let running = server::start(cfg).await.unwrap();
    let base = running.base();
    let r = http_get(&base, "/auth/dev?user=ann", None).await.unwrap();
    assert_eq!(r.status, 404);
    assert!(r.header("set-cookie").is_none());
    let e = Conn::connect(&base, None).await.err().expect("refused");
    assert!(matches!(e, NetError::Status(401)), "{e}");
    running.stop().await;
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-server --features dev-auth --test front --test units`
Expected: compile errors — no `bot::net`, no `server::config`, `server::session`, `server::limit`, `server::web`, `server::start`.

- [ ] **Step 4: The bot's network client**

In `crates/bot/src/lib.rs`, change the last line of the module doc to
`//! In C1 it talks to an in-process `Game`; `net` puts a WebSocket in between.`
and add `pub mod net;` after the doc comment (before the `use` line).

Create `crates/bot/src/net.rs`:
```rust
//! The bot's network side: a dev-auth login over plain HTTP and a WebSocket
//! carrying `ClientFrame`s out and `ServerFrame`s in. Plain `http://` only:
//! dev login exists only in test builds of the server, which are local.

use futures_util::{SinkExt, StreamExt};
use protocol::{ClientFrame, FrameError, ServerFrame};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("bad url `{0}` (want http://host:port)")]
    Url(String),
    #[error("bad http response: {0}")]
    Http(String),
    #[error("http status {0}")]
    Status(u16),
    #[error("websocket: {0}")]
    Ws(tungstenite::Error),
    #[error("bad frame from the server: {0}")]
    Frame(#[from] FrameError),
    #[error("timed out: {0}")]
    Timeout(String),
}

impl From<tungstenite::Error> for NetError {
    fn from(e: tungstenite::Error) -> NetError {
        match e {
            tungstenite::Error::Http(r) => NetError::Status(r.status().as_u16()),
            e => NetError::Ws(e),
        }
    }
}

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    /// Header names lower-cased, in order.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }
}

fn host_port(base: &str) -> Result<&str, NetError> {
    base.strip_prefix("http://").map(|h| h.trim_end_matches('/')).filter(|h| !h.is_empty()).ok_or_else(|| NetError::Url(base.into()))
}

/// A bare HTTP/1.0 GET (no redirects followed), enough for the dev login
/// and for tests. `base` is `http://host:port`; `path` starts with `/`.
pub async fn http_get(base: &str, path: &str, cookie: Option<&str>) -> Result<HttpResponse, NetError> {
    let host = host_port(base)?;
    let mut stream = TcpStream::connect(host).await?;
    let mut req = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\n");
    if let Some(c) = cookie {
        req.push_str(&format!("Cookie: {c}\r\n"));
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).await?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await?;
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n").ok_or_else(|| NetError::Http("no end of headers".into()))?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let status = status_line
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| NetError::Http(format!("status line `{status_line}`")))?;
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    Ok(HttpResponse { status, headers, body: body.to_string() })
}

/// Percent-encode everything but unreserved characters.
pub fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Log in through a dev-auth server's `/auth/dev`; returns the session
/// cookie as `name=value`, ready for a `Cookie` header.
pub async fn dev_login(base: &str, user: &str) -> Result<String, NetError> {
    let r = http_get(base, &format!("/auth/dev?user={}", url_encode(user)), None).await?;
    if !(200..400).contains(&r.status) {
        return Err(NetError::Status(r.status));
    }
    let set = r.header("set-cookie").ok_or_else(|| NetError::Http("no session cookie".into()))?;
    Ok(set.split(';').next().unwrap_or_default().trim().to_string())
}

/// One WebSocket connection to the front.
pub struct Conn {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl Conn {
    /// Open `/ws` on `base` (`http://host:port`), sending `cookie` if given.
    /// A refused upgrade is `NetError::Status(code)`.
    pub async fn connect(base: &str, cookie: Option<&str>) -> Result<Conn, NetError> {
        let url = format!("ws://{}/ws", host_port(base)?);
        let mut req = url.into_client_request()?;
        if let Some(c) = cookie {
            let v = HeaderValue::from_str(c).map_err(|_| NetError::Http("cookie is not a header value".into()))?;
            req.headers_mut().insert("cookie", v);
        }
        let (ws, _) = connect_async(req).await?;
        Ok(Conn { ws })
    }

    pub async fn send(&mut self, frame: &ClientFrame) -> Result<(), NetError> {
        self.send_text(frame.to_json()).await
    }

    /// Send any text (tests use it for malformed frames).
    pub async fn send_text(&mut self, text: String) -> Result<(), NetError> {
        self.ws.send(Message::text(text)).await?;
        Ok(())
    }

    pub async fn send_binary(&mut self, bytes: Vec<u8>) -> Result<(), NetError> {
        self.ws.send(Message::binary(bytes)).await?;
        Ok(())
    }

    /// The next frame; `Ok(None)` once the server has closed the socket.
    pub async fn recv(&mut self) -> Result<Option<ServerFrame>, NetError> {
        while let Some(m) = self.ws.next().await {
            match m {
                Ok(Message::Text(t)) => return Ok(Some(ServerFrame::from_json(t.as_str())?)),
                Ok(Message::Close(_)) => return Ok(None),
                Ok(_) => continue,
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => return Ok(None),
                Err(tungstenite::Error::Protocol(tungstenite::error::ProtocolError::ResetWithoutClosingHandshake)) => {
                    return Ok(None);
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(None)
    }

    pub async fn close(mut self) {
        let _ = self.ws.close(None).await;
    }
}
```

- [ ] **Step 5: Config, sessions, rate limit**

Create `crates/server/src/config.rs`:
```rust
//! Configuration from the environment (spec §8, §10). Every problem is one
//! clear line; the binary prints it and exits 2.

use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OidcConfig {
    /// e.g. `https://auth.skyes.lgbt/application/o/signalbox/`
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub addr: SocketAddr,
    /// Holds `saves/` and `sockets/`.
    pub data_dir: PathBuf,
    /// Converted worlds, `<name>.json`.
    pub layouts_dir: PathBuf,
    /// Where browsers reach us, without a trailing `/`; the OIDC redirect
    /// URI is this + `/auth/callback`.
    pub public_url: String,
    /// `None` only in `dev-auth` builds.
    pub oidc: Option<OidcConfig>,
    /// Cookie signing key, at least 64 bytes.
    pub session_key: Vec<u8>,
    pub game_bin: PathBuf,
}

pub const DEFAULT_ADDR: &str = "0.0.0.0:9160";
pub const DEFAULT_DATA: &str = "/data";
pub const DEFAULT_LAYOUTS: &str = "/opt/signalbox/layouts";
pub const MIN_KEY_BYTES: usize = 64;

pub fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

impl Config {
    pub fn from_env() -> Result<Config, String> {
        Config::from_lookup(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
    }

    /// `get` answers one variable (tests pass a map).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Config, String> {
        let addr = get("SIGNALBOX_ADDR").unwrap_or_else(|| DEFAULT_ADDR.into());
        let addr: SocketAddr = addr.parse().map_err(|_| format!("SIGNALBOX_ADDR `{addr}` is not host:port"))?;
        let data_dir = PathBuf::from(get("SIGNALBOX_DATA").unwrap_or_else(|| DEFAULT_DATA.into()));
        let layouts_dir = PathBuf::from(get("SIGNALBOX_LAYOUTS").unwrap_or_else(|| DEFAULT_LAYOUTS.into()));
        let key_hex = get("SIGNALBOX_SESSION_KEY").ok_or("SIGNALBOX_SESSION_KEY is required (hex, at least 64 bytes)")?;
        let session_key = decode_hex(&key_hex).ok_or("SIGNALBOX_SESSION_KEY is not hex")?;
        if session_key.len() < MIN_KEY_BYTES {
            return Err(format!("SIGNALBOX_SESSION_KEY has {} bytes; it needs at least {MIN_KEY_BYTES}", session_key.len()));
        }
        let oidc = match (get("OIDC_ISSUER"), get("OIDC_CLIENT_ID"), get("OIDC_CLIENT_SECRET")) {
            (Some(issuer), Some(client_id), Some(client_secret)) => Some(OidcConfig { issuer, client_id, client_secret }),
            (None, None, None) if cfg!(feature = "dev-auth") => None,
            _ => return Err("OIDC_ISSUER, OIDC_CLIENT_ID and OIDC_CLIENT_SECRET are all required".into()),
        };
        let public_url = match get("SIGNALBOX_PUBLIC_URL") {
            Some(u) if u.starts_with("https://") || u.starts_with("http://") => u.trim_end_matches('/').to_string(),
            Some(u) => return Err(format!("SIGNALBOX_PUBLIC_URL `{u}` must start with https:// or http://")),
            None if oidc.is_none() => format!("http://{addr}"),
            None => return Err("SIGNALBOX_PUBLIC_URL is required with OIDC".into()),
        };
        let game_bin = match get("SIGNALBOX_GAME_BIN") {
            Some(p) => PathBuf::from(p),
            None => std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(|d| d.join("signalbox-game")))
                .ok_or("cannot find signalbox-game; set SIGNALBOX_GAME_BIN")?,
        };
        Ok(Config { addr, data_dir, layouts_dir, public_url, oidc, session_key, game_bin })
    }
}
```

Create `crates/server/src/session.rs`:
```rust
//! Server-side sessions (spec §8): a random id in a signed cookie, the
//! player's name in memory. Lost on restart; players log in again.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum_extra::extract::cookie::Cookie;

pub const SESSION_COOKIE: &str = "signalbox_session";
pub const SESSION_TTL: Duration = Duration::from_secs(12 * 3600);

/// `n` random bytes as lower-case hex.
pub fn random_hex(n: usize) -> String {
    (0..n).map(|_| format!("{:02x}", rand::random::<u8>())).collect()
}

/// A cookie with the attributes every signalbox cookie carries.
pub fn cookie(name: &str, value: &str, max_age_s: u64) -> Cookie<'static> {
    Cookie::parse(format!("{name}={value}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age={max_age_s}"))
        .expect("a well-formed cookie")
}

/// Clears `name` in the browser.
pub fn removal(name: &str) -> Cookie<'static> {
    cookie(name, "", 0)
}

#[derive(Default)]
pub struct Sessions {
    map: Mutex<BTreeMap<String, (String, Instant)>>,
}

impl Sessions {
    pub fn new() -> Sessions {
        Sessions::default()
    }

    /// A new session for `user`; returns its id.
    pub fn create(&self, user: &str) -> String {
        self.create_at(user, Instant::now())
    }

    pub fn create_at(&self, user: &str, now: Instant) -> String {
        let id = random_hex(32);
        let mut map = self.map.lock().expect("sessions lock");
        map.retain(|_, (_, expires)| *expires > now);
        map.insert(id.clone(), (user.to_string(), now + SESSION_TTL));
        id
    }

    /// The user of a live session.
    pub fn user(&self, id: &str) -> Option<String> {
        self.user_at(id, Instant::now())
    }

    pub fn user_at(&self, id: &str, now: Instant) -> Option<String> {
        let mut map = self.map.lock().expect("sessions lock");
        match map.get(id) {
            Some((user, expires)) if *expires > now => Some(user.clone()),
            Some(_) => {
                map.remove(id);
                None
            }
            None => None,
        }
    }

    pub fn remove(&self, id: &str) {
        self.map.lock().expect("sessions lock").remove(id);
    }

    pub fn len(&self) -> usize {
        self.map.lock().expect("sessions lock").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
```

Create `crates/server/src/limit.rs`:
```rust
//! Per-connection message rate limit (spec §8): more than
//! `MAX_MSGS_PER_S` messages in one window of a second closes the socket.

use std::time::{Duration, Instant};

pub const MAX_MSGS_PER_S: u32 = 20;
const WINDOW: Duration = Duration::from_secs(1);

pub struct RateLimit {
    window_start: Instant,
    count: u32,
}

impl RateLimit {
    pub fn new(now: Instant) -> RateLimit {
        RateLimit { window_start: now, count: 0 }
    }

    /// Count one message at `now`; `false` = over the limit.
    pub fn allow(&mut self, now: Instant) -> bool {
        if now.saturating_duration_since(self.window_start) >= WINDOW {
            self.window_start = now;
            self.count = 0;
        }
        self.count += 1;
        self.count <= MAX_MSGS_PER_S
    }
}
```

- [ ] **Step 6: The routes and the socket loop**

Create `crates/server/src/web.rs`:
```rust
//! HTTP routes (spec §8): the placeholder page, the WebSocket, and the
//! auth routes. Nothing but the login flow answers without a session.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRef, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::get;
use axum_extra::extract::cookie::{Cookie, Key, SignedCookieJar};
use protocol::{GameInfo, LayoutInfo, codes};

use crate::limit::RateLimit;
use crate::session::{SESSION_COOKIE, Sessions};
use crate::supervisor::Supervisor;

/// Largest message or frame a client may send.
pub const MAX_CLIENT_MESSAGE: usize = 64 * 1024;
/// A client that takes longer than this to accept one frame is dropped.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// WebSocket close code for too many messages ("policy violation").
pub const CLOSE_POLICY: u16 = 1008;

#[derive(Clone)]
pub struct AppState {
    pub sup: Arc<Supervisor>,
    pub sessions: Arc<Sessions>,
    pub key: Key,
}

impl FromRef<AppState> for Key {
    fn from_ref(s: &AppState) -> Key {
        s.key.clone()
    }
}

pub fn router(state: AppState) -> Router {
    let r = Router::new().route("/", get(index)).route("/ws", get(ws)).route("/auth/logout", get(logout));
    #[cfg(feature = "dev-auth")]
    let r = r.route("/auth/dev", get(dev::login));
    r.with_state(state)
}

/// The logged-in user, from the signed session cookie.
pub fn user_of(state: &AppState, jar: &SignedCookieJar) -> Option<String> {
    let c = jar.get(SESSION_COOKIE)?;
    state.sessions.user(c.value())
}

pub fn escape_html(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '"' => "&quot;".into(),
            '\'' => "&#39;".into(),
            c => c.to_string(),
        })
        .collect()
}

/// The placeholder page until the browser client (sub-project D).
pub fn index_page(user: &str, games: &[GameInfo], layouts: &[LayoutInfo]) -> String {
    let e = escape_html;
    let mut rows = String::new();
    for g in games {
        let holders: Vec<String> =
            g.areas.iter().map(|a| format!("{}: {}", e(&a.name), e(a.holder.as_deref().unwrap_or("robot")))).collect();
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{:?}</td><td>{}</td><td>{}</td></tr>\n",
            e(&g.id),
            e(&g.layout),
            g.state,
            holders.join(", "),
            e(g.error.as_deref().unwrap_or("")),
        ));
    }
    let layouts: Vec<String> = layouts.iter().map(|l| e(&l.name)).collect();
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>signalbox</title></head><body>\n\
         <h1>signalbox</h1>\n<p>Signed in as {}. <a href=\"/auth/logout\">Sign out</a></p>\n\
         <p>The browser client is not built yet; bots play over <code>/ws</code>.</p>\n\
         <h2>Games</h2>\n<table><tr><th>Game</th><th>Layout</th><th>State</th><th>Areas</th><th>Error</th></tr>\n{}</table>\n\
         <h2>Layouts</h2>\n<p>{}</p>\n</body></html>\n",
        e(user),
        rows,
        layouts.join(", "),
    )
}

async fn index(State(state): State<AppState>, jar: SignedCookieJar) -> Response {
    let Some(user) = user_of(&state, &jar) else { return Redirect::to("/auth/login").into_response() };
    Html(index_page(&user, &state.sup.list_games(), &state.sup.layouts().infos())).into_response()
}

async fn logout(State(state): State<AppState>, jar: SignedCookieJar) -> Response {
    if let Some(c) = jar.get(SESSION_COOKIE) {
        state.sessions.remove(c.value());
    }
    let jar = jar.remove(Cookie::build(SESSION_COOKIE).path("/"));
    (jar, Html("<!doctype html><p>Signed out. <a href=\"/auth/login\">Sign in</a></p>")).into_response()
}

async fn ws(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    let Some(user) = user_of(&state, &jar) else { return (StatusCode::UNAUTHORIZED, "sign in first").into_response() };
    let upgrade = match upgrade {
        Ok(u) => u,
        Err(e) => return e.into_response(),
    };
    upgrade
        .max_message_size(MAX_CLIENT_MESSAGE)
        .max_frame_size(MAX_CLIENT_MESSAGE)
        .on_upgrade(move |socket| client_loop(state.sup, user, socket))
}

async fn send(socket: &mut WebSocket, m: Message) -> bool {
    matches!(tokio::time::timeout(SEND_TIMEOUT, socket.send(m)).await, Ok(Ok(())))
}

/// One client socket: frames in go to the supervisor, the outbox goes out.
async fn client_loop(sup: Arc<Supervisor>, user: String, mut socket: WebSocket) {
    let me = sup.attach(&user);
    let mut limit = RateLimit::new(Instant::now());
    loop {
        tokio::select! {
            m = socket.recv() => {
                let text = match m {
                    Some(Ok(Message::Text(t))) => Some(t),
                    Some(Ok(Message::Binary(_))) => None,
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                };
                if !limit.allow(Instant::now()) {
                    let close = CloseFrame { code: CLOSE_POLICY, reason: "too many messages".into() };
                    let _ = send(&mut socket, Message::Close(Some(close))).await;
                    break;
                }
                match text {
                    Some(t) => sup.handle_text(&user, me.conn, t.as_str()),
                    None => sup.reply(&user, me.conn, protocol::ServerFrame::error(codes::BAD_MESSAGE, "binary frames are not used")),
                }
            }
            f = me.outbox.pop() => match f {
                Some(f) => {
                    if !send(&mut socket, Message::Text(f.to_json().into())).await {
                        break;
                    }
                }
                None => {
                    let _ = send(&mut socket, Message::Close(None)).await;
                    break;
                }
            },
        }
    }
    sup.detach(&user, me.conn);
}

#[cfg(feature = "dev-auth")]
pub mod dev {
    //! `/auth/dev?user=<name>`: a session for anyone, for tests and bots.
    //! Compiled only with the `dev-auth` feature.

    use axum::extract::{Query, State};
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Redirect, Response};
    use axum_extra::extract::cookie::SignedCookieJar;

    use super::AppState;
    use crate::session::{SESSION_COOKIE, SESSION_TTL, cookie};

    #[derive(serde::Deserialize)]
    pub struct DevQuery {
        user: String,
    }

    /// 1–32 of `A-Z a-z 0-9 _ . -`.
    pub fn valid_dev_user(s: &str) -> bool {
        (1..=32).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    }

    pub async fn login(State(state): State<AppState>, jar: SignedCookieJar, Query(q): Query<DevQuery>) -> Response {
        if !valid_dev_user(&q.user) {
            return (StatusCode::BAD_REQUEST, "user: 1 to 32 of A-Z a-z 0-9 _ . -").into_response();
        }
        let id = state.sessions.create(&q.user);
        (jar.add(cookie(SESSION_COOKIE, &id, SESSION_TTL.as_secs())), Redirect::to("/")).into_response()
    }
}
```

Notes: the session check runs before the upgrade is looked at, so `/ws` without a session is a plain 401 and no WebSocket is ever opened (spec §1 criterion 5). axum closes a socket whose message or frame exceeds 64 KiB. Pings/pongs are not counted by the rate limit; every text or binary message is. The socket loop is the only place that writes to the client, and `Outbox::pop` is cancel-safe, so the `select!` is sound.

- [ ] **Step 7: `start`, `run` and the binary**

Replace `crates/server/src/lib.rs` with:
```rust
//! signalbox's server side: the game process (`process`, run by the
//! `signalbox-game` binary) and the front (`signalbox-server`): config,
//! sessions, layouts, the supervisor of game processes, and the web routes.

pub mod config;
pub mod layouts;
pub mod limit;
pub mod outbox;
pub mod process;
pub mod session;
pub mod supervisor;
pub mod web;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum_extra::extract::cookie::Key;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::layouts::Layouts;
use crate::session::Sessions;
use crate::supervisor::{Supervisor, SupervisorConfig};
use crate::web::AppState;

/// How long games get to save and exit when the front stops (spec §2.2).
pub const STOP_GRACE: Duration = Duration::from_secs(10);

/// A front serving on `addr`.
pub struct Running {
    pub addr: SocketAddr,
    pub sup: Arc<Supervisor>,
    pub sessions: Arc<Sessions>,
    stop: Arc<Notify>,
    server: JoinHandle<()>,
}

/// Start the front: data directories, layouts, supervisor, listener.
pub async fn start(cfg: Config) -> Result<Running, String> {
    let layouts = Layouts::load(&cfg.layouts_dir)?;
    let sup = Supervisor::new(
        SupervisorConfig {
            game_bin: cfg.game_bin.clone(),
            saves_dir: cfg.data_dir.join("saves"),
            sockets_dir: cfg.data_dir.join("sockets"),
            empty_exit_s: process::EMPTY_EXIT_S,
        },
        layouts,
    )?;
    let sessions = Arc::new(Sessions::new());
    let state = AppState { sup: sup.clone(), sessions: sessions.clone(), key: Key::from(&cfg.session_key) };
    let listener = tokio::net::TcpListener::bind(cfg.addr).await.map_err(|e| format!("{}: {e}", cfg.addr))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let stop = Arc::new(Notify::new());
    let app = web::router(state);
    let server = tokio::spawn({
        let stop = stop.clone();
        async move {
            let _ = axum::serve(listener, app).with_graceful_shutdown(async move { stop.notified().await }).await;
        }
    });
    Ok(Running { addr, sup, sessions, stop, server })
}

impl Running {
    /// `http://<addr>`.
    pub fn base(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Shut every game down (≤ `STOP_GRACE`), then stop serving.
    pub async fn stop(self) {
        self.sup.shutdown_all(STOP_GRACE).await;
        self.stop.notify_one();
        let _ = tokio::time::timeout(Duration::from_secs(2), self.server).await;
    }
}

/// The binary: serve until SIGTERM or SIGINT, then stop cleanly.
pub async fn run(cfg: Config) -> Result<(), String> {
    let running = start(cfg).await?;
    eprintln!("signalbox-server: listening on {}", running.addr);
    let mut term = signal(SignalKind::terminate()).map_err(|e| e.to_string())?;
    let mut int = signal(SignalKind::interrupt()).map_err(|e| e.to_string())?;
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
    }
    eprintln!("signalbox-server: stopping");
    running.stop().await;
    Ok(())
}
```

Create `crates/server/src/bin/signalbox-server.rs`:
```rust
//! The front (spec §8). Configuration comes from the environment; see
//! `server::config`.

use std::process::ExitCode;

fn main() -> ExitCode {
    let cfg = match server::config::Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("signalbox-server: {e}");
            return ExitCode::from(2);
        }
    };
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a tokio runtime");
    match rt.block_on(server::run(cfg)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("signalbox-server: {e}");
            ExitCode::FAILURE
        }
    }
}
```

- [ ] **Step 8: Run the tests in both builds**

Run: `scripts/cargo test -p signalbox-server --features dev-auth`
Expected: PASS — `front` (6), `units` (5), `supervisor` (15), `process` (14); `release` runs 0 tests in this build.

Run: `scripts/cargo test -p signalbox-server`
Expected: PASS — `release` (1), `units` (5), `supervisor`, `process`; `front` is skipped (its required feature is off).

Run: `scripts/cargo test -p signalbox-bot`
Expected: PASS (C1's bot tests; `net` is exercised by the server's tests).

- [ ] **Step 9: CI runs the dev-auth build too**

Replace `scripts/ci/test.sh` with:
```bash
#!/usr/bin/env bash
# CI test gate: build and test the whole workspace, warnings as errors.
# Runs natively on the CI runner (offline, against its seeded cargo cache);
# locally use `scripts/cargo test` instead.
set -euo pipefail
export RUSTFLAGS="${RUSTFLAGS:-} -D warnings"
cargo --version
cargo build --workspace --all-targets --locked --offline
cargo test --workspace --locked --offline
# The dev-login build of the front (tests and bots only; the release image
# never has it) and the tests that need it.
cargo build -p signalbox-server --features dev-auth --all-targets --locked --offline
cargo test -p signalbox-server --features dev-auth --locked --offline
```

Reproduce the CI builds through Docker (which forwards no `RUSTFLAGS`):

Run: `scripts/cargo build --workspace --all-targets --config 'build.rustflags=["-D","warnings"]'`
Expected: success, no warnings.

Run: `scripts/cargo build -p signalbox-server --features dev-auth --all-targets --config 'build.rustflags=["-D","warnings"]'`
Expected: success, no warnings.

Run: `scripts/cargo tree -p signalbox-server -i openssl-sys`
Expected: `error: package ID specification `openssl-sys` did not match any packages` — OpenSSL is not in the graph.

- [ ] **Step 10: Commit** (Cargo.lock changed: the controller reseeds the CI cache before any push)

```bash
git add Cargo.toml Cargo.lock crates/server crates/bot scripts/ci/test.sh
git commit -m "feat(server): signalbox-server: sessions, dev login, the /ws gate, limits and the bot's network client

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: login with Authentik — OIDC code flow with PKCE, the group gate, sessions

**Files:**
- Modify: `Cargo.toml` (workspace dependencies `openidconnect`, `base64`)
- Modify: `crates/server/Cargo.toml` (dependencies `base64`, `openidconnect`, `thiserror`)
- Create: `crates/server/src/oidc.rs`
- Modify: `crates/server/src/web.rs` (`AppState::oidc`, routes `/auth/login` and `/auth/callback`)
- Modify: `crates/server/src/lib.rs` (`pub mod oidc`, `start` builds the `Oidc`)
- Create: `crates/server/tests/fixtures/oidc-test-key.pem`
- Test: `crates/server/tests/oidc.rs`

**Interfaces:**
- Consumes: Task 6 (`OidcConfig`, `Config::public_url`, `Sessions::create`, `session::{cookie, SESSION_COOKIE, SESSION_TTL}`, `AppState`, `router`, `start`, `Running::sessions`, `bot::net::{http_get, HttpResponse, Conn, NetError}`).
- Produces (module `server::oidc`):
  - consts `REQUIRED_GROUP = "signalbox-users"`, `LOGIN_COOKIE = "signalbox_login"`, `PENDING_TTL` (600 s), `MAX_PENDING_LOGINS = 256`, `METADATA_TTL` (3600 s), `HTTP_TIMEOUT` (10 s)
  - `enum Denied { NoUsername, NotInGroup }`; `fn admit(preferred_username: Option<&str>, groups: &[String]) -> Result<String, Denied>`; `fn groups_from_id_token(jwt: &str) -> Result<Vec<String>, String>`
  - `struct Pending { pub nonce: String, pub pkce_verifier: String, pub created: Instant }`; `struct PendingLogins` with `new`, `insert_at(&self, state: String, nonce: String, pkce_verifier: String, now: Instant)`, `take_at(&self, state: &str, now: Instant) -> Option<Pending>`, `len`, `is_empty`
  - `enum LoginError { Unavailable(String), BadState, Rejected(String), Denied(Denied) }` (thiserror)
  - `struct Oidc` with `new(&OidcConfig, public_url: &str) -> Result<Oidc, String>` (no network), `redirect_url(&self) -> &str`, `async begin(&self) -> Result<(String /*provider URL*/, String /*state*/), LoginError>`, `async finish(&self, state: &str, code: &str) -> Result<String /*player*/, LoginError>`, field `pub pending: PendingLogins`
  - `web::AppState` gains `pub oidc: Option<Arc<Oidc>>`.
- HTTP behaviour: `/auth/login` → 303 to the provider with a signed `signalbox_login` cookie holding the state (10 min), or 503 if the provider cannot be reached (404 in a dev-auth build without OIDC). `/auth/callback?state&code` → 303 to `/` with a new session cookie; 400 for a missing/mismatched/unknown/used state or a cancelled login; 403 when the token does not verify (issuer, audience, signature, expiry, nonce) or the user is not admitted; 502 when the provider is unreachable mid-login. The login cookie is cleared on every callback.

Why `openidconnect` 4 (not hand-rolled): it is the maintained OIDC client for Rust, does discovery, JWKS, PKCE and full ID-token verification (issuer, audience, expiry, RS256/ES256 signature, nonce), and with `default-features = false, features = ["reqwest", "rustls-tls"]` it brings reqwest on rustls + ring + webpki roots — no OpenSSL, no system CA bundle needed in the runtime image. It adds about 150 crates to `Cargo.lock` (RSA/EC crypto, reqwest, hyper-rustls); that is the cost of not writing JWT verification by hand. `base64` 0.22 decodes the verified token's payload for the `groups` claim (Authentik's `profile` scope puts group names there); reading it from the payload after verification keeps the client type the stock `CoreClient` instead of a custom additional-claims client.

How login is tested (the brief asked to say which): **both**. `admit`, `groups_from_id_token` and `PendingLogins` are pure and tested directly; the whole flow (discovery, JWKS, authorize URL with PKCE S256 + nonce + state, token exchange with client-secret basic auth, RS256 ID token verification, the login cookie, sessions) is tested against a small OpenID provider served by axum inside the test, signing with a committed test-only RSA key. Real Authentik is exercised only at deployment (controller step D6).

- [ ] **Step 1: Dependencies**

In the root `Cargo.toml`, append to `[workspace.dependencies]`:
```toml
openidconnect = { version = "4", default-features = false, features = ["reqwest", "rustls-tls"] }
base64 = "0.22"
```

In `crates/server/Cargo.toml` `[dependencies]`, add after `axum-extra.workspace = true`:
```toml
base64.workspace = true
openidconnect.workspace = true
```
and after `serde_json.workspace = true`:
```toml
thiserror.workspace = true
```

- [ ] **Step 2: The test key and the failing tests**

Create `crates/server/tests/fixtures/oidc-test-key.pem` — a PKCS#1 RSA key used only by the in-test provider (it signs nothing anywhere else; any 2048-bit PKCS#1 key works, e.g. `openssl genrsa -traditional 2048`):
```text
-----BEGIN RSA PRIVATE KEY-----
MIIEowIBAAKCAQEAkGO+Zgpsn8IA0/FkSpK+r5iloTLqss2MefFzM+qPK+EqMCIR
ZX5LC74FPQUqn9meXJ67aiMz/dhc9YHLO6U3+UCMNBL46VaKe7z5qa3EZxTrCAL/
7cdofs6mFm9Rdjati+NSLxrgjHSD4BkSIvEiOEW5dC+Oi6Is/36qSMEdJBqlXVXM
Oif7J2JjZDbslf/arrrDL9gz86ss3yh5CeTuvhSl65Hzs4TPXjExpJOjJDiP2sWX
hwj9saLR7eTRIl6pBJXb3tZQI/iL10JTEEpXI+ZB/yi8h/idikpGkDmMuwrlni7+
nK9Y6dyduk88XG/yyFRUVOAVW2+w5GRNOwriJQIDAQABAoIBACre2qDMdo0Gmp1T
Hk5//IsBfSf8CLBXFF7+gBCJk8HZBGAvNVAXq+uMG10PRCUbBYiFfqrYUe8MRymD
xJZsi05/ykEJ4wrQ7aQoq04kcFyU2uXRkjCE1PNVov2lRqAdQvD2aSfgSIybaa5n
czmZs+nWVeZ32lB+MfMYJjIc2Gyn5Hs96i2jIeMFjIvhS1Dd1i6r4f65sC/lcf89
iAVXl6D15ThrKzERqHDuIRZkZnfJFSxjmFPlI+rsoSky9EA57Tq5wDcElmsBQM44
vofVhaL5Hm4esofaQL0kYAvoicTMcbuboknv73MRCiaLkolmVLP6kmfV++D/nzcM
dYtXwV8CgYEAxdfueyJNfg3JDyZVHUL6UFLKhhD01x3jMiz1tKD+WTPOTwXdxHbf
SO/MD8hWV06Rqg7+9C9gcwYEu+3th1uIuif5PZM3FVLqmpNqwar36B0Q880dkfvE
+TyyBdU54wce6zyAgSUnOCl38b8pFRimis56o3ooarAmdKVL0RexoEsCgYEAutVQ
08lGIeYvcnFWjIIm2EHFFz4V7f4x/k4V9wDs/dkxIsZcU3CMvsqPVk6nDDIRKS0F
50SdfIHCrBaNvJNLPa23fZyiSGff+9YbAonB5mf5q6QPaDd6hrdXwjd36jQIuwOX
FinZFgyHQbDiyuONBJBJtfrcCL/UWKKjyBW6YU8CgYAdPkCS1MwtgK4iXiEglSDY
tJQY4vK9xT4q0Xhz/YP6/WxWQ+C2xHdWmZNJFeylNQjU4SiQQVx5Q/95I78DTeVU
1snBbzwqG7pvpLCX9cR0+67gyoW/aT6BNJZ/xDetNgU88hFwhWRZqc9/3SieZKlh
RQndlhXZRzY38aLWQjFQJwKBgQCUoS5zzSLlxODqAhAAJ2oPkALiwplfg2DyFdyT
a0EdkLLuHy9Dkb3l6e6tklSB9zJ/tzmDCarfabsce7S119d7cb8PRpQzVa9yAJns
IvsF+KE+Un2PQtNOaHyAHPBgeJSZcfm3wALa74yKTdWd4fhFMSPyiWaR70lWWcxB
WEe1MQKBgBBLsfFNFCpoNlOkf160G8pqIW9UsXj27xsifQcrlpP0e0y8FPbnOCEN
h0FD4q4FNRzgKCr8BoGucSrI+TOvEYkUJn+KNXJCJZNyoydayv19hYep+BQv3wEo
o1wu1Gme7SrBJzua+FurkHSkenIn2z/iAMPbrJJbs1BWv1M220tz
-----END RSA PRIVATE KEY-----
```

Create `crates/server/tests/oidc.rs`:
```rust
//! Login (spec §8): the pure admission rules, the pending-login table, and
//! the whole authorization code + PKCE + nonce + state flow against a small
//! OpenID provider run inside the test (discovery, JWKS, token endpoint,
//! RS256 ID tokens signed with a test-only key). Runs in both builds: this
//! is the release login path.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{Form, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use bot::net::{Conn, HttpResponse, NetError, http_get};
use openidconnect::core::{
    CoreGenderClaim, CoreJweContentEncryptionAlgorithm, CoreJwsSigningAlgorithm, CoreRsaPrivateSigningKey,
};
use openidconnect::url::Url;
use openidconnect::{
    AdditionalClaims, IdToken, IdTokenClaims, JsonWebKeyId, JsonWebKeySet, PkceCodeChallenge, PkceCodeVerifier,
    PrivateSigningKey,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use server::config::{Config, OidcConfig};
use server::oidc::*;

const KEY_PEM: &str = include_str!("fixtures/oidc-test-key.pem");
const CLIENT_ID: &str = "signalbox";
const CLIENT_SECRET: &str = "test-secret";

fn s(x: &str) -> String {
    x.to_string()
}

// ---- the pure parts ----

#[test]
fn admit_requires_the_group_and_a_username() {
    let groups = |g: &[&str]| g.iter().map(|x| s(x)).collect::<Vec<String>>();
    assert_eq!(admit(Some("ann"), &groups(&["staff", "signalbox-users"])), Ok(s("ann")));
    assert_eq!(admit(Some("Hackney & Bow"), &groups(&["signalbox-users"])), Ok(s("Hackney & Bow")), "names are opaque");
    assert_eq!(admit(Some("ann"), &groups(&[])), Err(Denied::NotInGroup));
    assert_eq!(admit(Some("ann"), &groups(&["Signalbox-Users", "signalbox-users-old"])), Err(Denied::NotInGroup));
    assert_eq!(admit(None, &groups(&["signalbox-users"])), Err(Denied::NoUsername));
    assert_eq!(admit(Some(""), &groups(&["signalbox-users"])), Err(Denied::NoUsername));
}

fn jwt_with(payload: serde_json::Value) -> String {
    format!("eyJhbGciOiJub25lIn0.{}.sig", URL_SAFE_NO_PAD.encode(payload.to_string()))
}

#[test]
fn groups_come_from_the_token_payload() {
    assert_eq!(groups_from_id_token(&jwt_with(json!({"groups": ["a", "signalbox-users"]}))), Ok(vec![s("a"), s("signalbox-users")]));
    assert_eq!(groups_from_id_token(&jwt_with(json!({"sub": "x"}))), Ok(vec![]), "no claim, no groups");
    assert_eq!(groups_from_id_token(&jwt_with(json!({"groups": null}))), Ok(vec![]));
    assert!(groups_from_id_token(&jwt_with(json!({"groups": "signalbox-users"}))).is_err(), "a string is not a list");
    assert!(groups_from_id_token(&jwt_with(json!({"groups": [1]}))).is_err());
    assert!(groups_from_id_token("not a jwt").is_err());
    assert!(groups_from_id_token("a.!!!.c").is_err());
}

#[test]
fn pending_logins_are_one_shot_bounded_and_expire() {
    let p = PendingLogins::new();
    let t0 = Instant::now();
    p.insert_at(s("st"), s("n"), s("v"), t0);
    let got = p.take_at("st", t0).unwrap();
    assert_eq!((got.nonce.as_str(), got.pkce_verifier.as_str()), ("n", "v"));
    assert!(p.take_at("st", t0).is_none(), "taken once");
    p.insert_at(s("old"), s("n"), s("v"), t0);
    assert!(p.take_at("old", t0 + PENDING_TTL).is_none(), "expired");
    for i in 0..MAX_PENDING_LOGINS + 5 {
        p.insert_at(format!("s{i}"), s("n"), s("v"), t0 + Duration::from_millis(i as u64));
    }
    assert_eq!(p.len(), MAX_PENDING_LOGINS);
    assert!(p.take_at("s0", t0).is_none(), "the oldest went first");
    assert!(p.take_at(&format!("s{}", MAX_PENDING_LOGINS + 4), t0).is_some());
}

// ---- a small OpenID provider ----

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Groups {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    groups: Option<Vec<String>>,
}

impl AdditionalClaims for Groups {}

type TestIdToken = IdToken<Groups, CoreGenderClaim, CoreJweContentEncryptionAlgorithm, CoreJwsSigningAlgorithm>;

/// Who "logs in" at the provider, and what the token says.
#[derive(Clone, Debug)]
struct Login {
    username: Option<String>,
    groups: Option<Vec<String>>,
    audience: String,
    /// `None` = the nonce the front asked for.
    nonce: Option<String>,
}

fn member(name: &str) -> Login {
    Login { username: Some(s(name)), groups: Some(vec![s("signalbox-users")]), audience: s(CLIENT_ID), nonce: None }
}

struct Issued {
    login: Login,
    nonce: String,
    challenge: String,
    redirect_uri: String,
}

struct ProviderState {
    issuer: String,
    key: CoreRsaPrivateSigningKey,
    codes: Mutex<BTreeMap<String, Issued>>,
    next_code: Mutex<u32>,
}

struct Provider {
    state: Arc<ProviderState>,
}

fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

async fn discovery(State(p): State<Arc<ProviderState>>) -> Json<serde_json::Value> {
    Json(json!({
        "issuer": p.issuer,
        "authorization_endpoint": format!("{}authorize", p.issuer),
        "token_endpoint": format!("{}token", p.issuer),
        "jwks_uri": format!("{}jwks", p.issuer),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
    }))
}

async fn jwks(State(p): State<Arc<ProviderState>>) -> Json<serde_json::Value> {
    Json(serde_json::to_value(JsonWebKeySet::new(vec![p.key.as_verification_key()])).unwrap())
}

async fn token(State(p): State<Arc<ProviderState>>, headers: HeaderMap, Form(f): Form<BTreeMap<String, String>>) -> Response {
    let bad = |why: &str| (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_grant", "error_description": why}))).into_response();
    let basic = format!("Basic {}", STANDARD.encode(format!("{CLIENT_ID}:{CLIENT_SECRET}")));
    if headers.get("authorization").and_then(|v| v.to_str().ok()) != Some(basic.as_str()) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "invalid_client"}))).into_response();
    }
    if f.get("grant_type").map(String::as_str) != Some("authorization_code") {
        return bad("grant_type");
    }
    let Some(issued) = f.get("code").and_then(|c| p.codes.lock().unwrap().remove(c)) else { return bad("unknown code") };
    let verifier = PkceCodeVerifier::new(f.get("code_verifier").cloned().unwrap_or_default());
    if PkceCodeChallenge::from_code_verifier_sha256(&verifier).as_str() != issued.challenge {
        return bad("PKCE");
    }
    if f.get("redirect_uri") != Some(&issued.redirect_uri) {
        return bad("redirect_uri");
    }
    let l = &issued.login;
    let mut claims = json!({
        "iss": p.issuer,
        "aud": [l.audience],
        "sub": "user-1",
        "iat": now_s(),
        "exp": now_s() + 300,
        "nonce": l.nonce.clone().unwrap_or(issued.nonce),
    });
    if let Some(u) = &l.username {
        claims["preferred_username"] = json!(u);
    }
    if let Some(g) = &l.groups {
        claims["groups"] = json!(g);
    }
    let claims: IdTokenClaims<Groups, CoreGenderClaim> = serde_json::from_value(claims).unwrap();
    let id_token = TestIdToken::new(claims, &p.key, CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256, None, None).unwrap();
    Json(json!({"access_token": "at", "token_type": "bearer", "expires_in": 300, "id_token": id_token.to_string()})).into_response()
}

impl Provider {
    async fn start() -> Provider {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}/", listener.local_addr().unwrap());
        let key = CoreRsaPrivateSigningKey::from_pem(KEY_PEM, Some(JsonWebKeyId::new(s("test-1")))).unwrap();
        let state = Arc::new(ProviderState { issuer, key, codes: Mutex::new(BTreeMap::new()), next_code: Mutex::new(0) });
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/jwks", get(jwks))
            .route("/token", post(token))
            .with_state(state.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Provider { state }
    }

    fn issuer(&self) -> String {
        self.state.issuer.clone()
    }

    /// The user signs in at the provider's authorize URL; returns the code
    /// the provider would hand back through the browser.
    fn sign_in(&self, authorize_url: &str, login: Login) -> (String, String) {
        let url = Url::parse(authorize_url).unwrap();
        assert!(authorize_url.starts_with(&format!("{}authorize?", self.state.issuer)), "{authorize_url}");
        let q: BTreeMap<String, String> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["client_id"], CLIENT_ID);
        assert_eq!(q["code_challenge_method"], "S256");
        assert!(q["scope"].split(' ').any(|x| x == "openid"), "{}", q["scope"]);
        let mut n = self.state.next_code.lock().unwrap();
        *n += 1;
        let code = format!("code-{n}");
        let issued = Issued {
            login,
            nonce: q["nonce"].clone(),
            challenge: q["code_challenge"].clone(),
            redirect_uri: q["redirect_uri"].clone(),
        };
        self.state.codes.lock().unwrap().insert(code.clone(), issued);
        (code, q["state"].clone())
    }
}

// ---- the front against it ----

fn config(name: &str, issuer: &str) -> Config {
    let root = std::env::temp_dir().join(format!("sbx-oidc-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("layouts")).unwrap();
    Config {
        addr: "127.0.0.1:0".parse().unwrap(),
        data_dir: root.join("data"),
        layouts_dir: root.join("layouts"),
        public_url: s("https://signalbox.test:50160"),
        oidc: Some(OidcConfig { issuer: s(issuer), client_id: s(CLIENT_ID), client_secret: s(CLIENT_SECRET) }),
        session_key: vec![9; 64],
        game_bin: PathBuf::from(env!("CARGO_BIN_EXE_signalbox-game")),
    }
}

fn set_cookies(r: &HttpResponse) -> Vec<&str> {
    r.headers.iter().filter(|(k, _)| k == "set-cookie").map(|(_, v)| v.as_str()).collect()
}

/// `name=value` of the cookie called `name` that `r` sets.
fn cookie_of(r: &HttpResponse, name: &str) -> Option<String> {
    set_cookies(r)
        .into_iter()
        .map(|c| c.split(';').next().unwrap().trim().to_string())
        .find(|c| c.starts_with(&format!("{name}=")) && c.len() > name.len() + 1)
}

/// `/auth/login`: the provider URL it redirects to and the login cookie.
async fn start_login(base: &str) -> (String, String) {
    let r = http_get(base, "/auth/login", None).await.unwrap();
    assert_eq!(r.status, 303, "{}", r.body);
    let login_cookie = cookie_of(&r, LOGIN_COOKIE).expect("a login cookie");
    let c = set_cookies(&r).join(" | ");
    for attr in ["HttpOnly", "SameSite=Lax", "Secure", "Max-Age=600"] {
        assert!(c.contains(attr), "{attr} in {c}");
    }
    (r.header("location").unwrap().to_string(), login_cookie)
}

async fn callback(base: &str, state: &str, code: &str, cookie: Option<&str>) -> HttpResponse {
    http_get(base, &format!("/auth/callback?state={state}&code={code}"), cookie).await.unwrap()
}

#[tokio::test]
async fn a_member_logs_in_through_the_provider() {
    let p = Provider::start().await;
    let running = server::start(config("member", &p.issuer())).await.unwrap();
    let base = running.base();
    let (to, login_cookie) = start_login(&base).await;
    assert!(to.contains("redirect_uri=https%3A%2F%2Fsignalbox.test%3A50160%2Fauth%2Fcallback"), "{to}");
    let (code, state) = p.sign_in(&to, member("ann"));
    let r = callback(&base, &state, &code, Some(&login_cookie)).await;
    assert_eq!((r.status, r.header("location")), (303, Some("/")), "{}", r.body);
    let session = cookie_of(&r, "signalbox_session").expect("a session");
    let page = http_get(&base, "/", Some(&session)).await.unwrap();
    assert!(page.body.contains("Signed in as ann"), "{}", page.body);
    assert!(Conn::connect(&base, Some(&session)).await.is_ok(), "the socket opens");
    assert_eq!(running.sessions.len(), 1);
    running.stop().await;
}

#[tokio::test]
async fn callback_rejects_bad_state_replay_and_foreign_audience() {
    let p = Provider::start().await;
    let running = server::start(config("reject", &p.issuer())).await.unwrap();
    let base = running.base();
    let no_session = |r: &HttpResponse| cookie_of(r, "signalbox_session").is_none();

    // A state that is not this browser's.
    let (to, login_cookie) = start_login(&base).await;
    let (code, _) = p.sign_in(&to, member("ann"));
    let r = callback(&base, "someone-elses-state", &code, Some(&login_cookie)).await;
    assert!(r.status == 400 && no_session(&r), "{} {}", r.status, r.body);

    // No login cookie at all.
    let (to, _) = start_login(&base).await;
    let (code, state) = p.sign_in(&to, member("ann"));
    let r = callback(&base, &state, &code, None).await;
    assert!(r.status == 400 && no_session(&r), "{} {}", r.status, r.body);

    // The provider said no, or the user cancelled.
    let r = http_get(&base, "/auth/callback?error=access_denied", None).await.unwrap();
    assert!(r.status == 400 && no_session(&r), "{}", r.status);

    // A good login once; the same callback again is refused.
    let (to, login_cookie) = start_login(&base).await;
    let (code, state) = p.sign_in(&to, member("ann"));
    assert_eq!(callback(&base, &state, &code, Some(&login_cookie)).await.status, 303);
    let r = callback(&base, &state, &code, Some(&login_cookie)).await;
    assert!(r.status == 400 && no_session(&r), "replayed: {} {}", r.status, r.body);

    // Tokens the front must not accept.
    let refuse = [
        (Login { audience: s("another-app"), ..member("ann") }, 403),
        (Login { groups: Some(vec![s("staff")]), ..member("ann") }, 403),
        (Login { groups: None, ..member("ann") }, 403),
        (Login { username: None, ..member("ann") }, 403),
        (Login { nonce: Some(s("not-the-nonce")), ..member("ann") }, 403),
    ];
    for (login, status) in refuse {
        let (to, login_cookie) = start_login(&base).await;
        let (code, state) = p.sign_in(&to, login.clone());
        let r = callback(&base, &state, &code, Some(&login_cookie)).await;
        assert!(r.status == status && no_session(&r), "{login:?}: {} {}", r.status, r.body);
    }
    assert_eq!(running.sessions.len(), 1, "only the one good login");
    running.stop().await;
}

#[tokio::test]
async fn login_answers_503_while_the_provider_is_down() {
    // Nothing listens on port 9; the front still starts.
    let running = server::start(config("down", "http://127.0.0.1:9/")).await.unwrap();
    let r = http_get(&running.base(), "/auth/login", None).await.unwrap();
    assert_eq!(r.status, 503);
    assert!(set_cookies(&r).is_empty());
    let e = Conn::connect(&running.base(), None).await.err().expect("refused");
    assert!(matches!(e, NetError::Status(401)), "{e}");
    running.stop().await;
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-server --test oidc`
Expected: compile errors — no module `server::oidc`.

- [ ] **Step 4: Implement `crates/server/src/oidc.rs`**

```rust
//! Login with Authentik (spec §8): the OpenID Connect authorization code
//! flow with PKCE, a nonce and a one-shot `state`. The ID token is verified
//! by `openidconnect` (issuer, audience, expiry, signature against the
//! provider's JWKS, nonce); who may play is then decided by `admit`, a pure
//! function of the verified token's `preferred_username` and `groups`.
//!
//! Provider metadata and keys are fetched on the first login and again
//! after `METADATA_TTL`, so the front starts while Authentik is down.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet, EndpointSet, IssuerUrl, Nonce,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RequestTokenError, Scope, TokenResponse, reqwest,
};

use crate::config::OidcConfig;

/// Only members of this Authentik group may play.
pub const REQUIRED_GROUP: &str = "signalbox-users";
/// Signed cookie holding a login's `state` between `/auth/login` and
/// `/auth/callback`.
pub const LOGIN_COOKIE: &str = "signalbox_login";
/// A login must come back within this long.
pub const PENDING_TTL: Duration = Duration::from_secs(600);
/// Logins in flight at most; the oldest is dropped beyond this.
pub const MAX_PENDING_LOGINS: usize = 256;
/// Provider metadata and keys are re-fetched after this long.
pub const METADATA_TTL: Duration = Duration::from_secs(3600);
/// Every request to the provider gives up after this long.
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// An error with its sources, `a: b: c` (reqwest hides the useful part in them).
fn chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut cause = e.source();
    while let Some(c) = cause {
        s.push_str(": ");
        s.push_str(&c.to_string());
        cause = c.source();
    }
    s
}

/// A client made from discovered metadata (auth URL set, token URL maybe).
type Client = CoreClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointMaybeSet, EndpointMaybeSet>;

/// Why a verified user may not play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Denied {
    /// The token has no (or an empty) `preferred_username`.
    NoUsername,
    /// `groups` does not contain `signalbox-users`.
    NotInGroup,
}

/// The player name for a verified token, if its user may play.
pub fn admit(preferred_username: Option<&str>, groups: &[String]) -> Result<String, Denied> {
    let name = preferred_username.filter(|n| !n.is_empty()).ok_or(Denied::NoUsername)?;
    if !groups.iter().any(|g| g == REQUIRED_GROUP) {
        return Err(Denied::NotInGroup);
    }
    Ok(name.to_string())
}

/// The `groups` claim from a JWT's payload (absent or null = none). Call it
/// only on a token whose signature and claims were already verified.
pub fn groups_from_id_token(jwt: &str) -> Result<Vec<String>, String> {
    let payload = jwt.split('.').nth(1).ok_or("the ID token is not a JWT")?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).map_err(|e| format!("ID token payload: {e}"))?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| format!("ID token payload: {e}"))?;
    match v.get("groups") {
        None | Some(serde_json::Value::Null) => Ok(vec![]),
        Some(serde_json::Value::Array(a)) => {
            a.iter().map(|g| g.as_str().map(str::to_string).ok_or_else(|| "`groups` holds a non-string".to_string())).collect()
        }
        Some(_) => Err("`groups` is not a list".into()),
    }
}

/// What a login remembers until its callback.
#[derive(Clone, Debug)]
pub struct Pending {
    pub nonce: String,
    pub pkce_verifier: String,
    pub created: Instant,
}

/// Logins in flight, by `state`. Each can be taken once.
#[derive(Default)]
pub struct PendingLogins {
    map: Mutex<BTreeMap<String, Pending>>,
}

impl PendingLogins {
    pub fn new() -> PendingLogins {
        PendingLogins::default()
    }

    pub fn insert_at(&self, state: String, nonce: String, pkce_verifier: String, now: Instant) {
        let mut map = self.map.lock().expect("pending lock");
        map.retain(|_, p| now.saturating_duration_since(p.created) < PENDING_TTL);
        while map.len() >= MAX_PENDING_LOGINS {
            let oldest = map.iter().min_by_key(|(_, p)| p.created).map(|(k, _)| k.clone()).expect("not empty");
            map.remove(&oldest);
        }
        map.insert(state, Pending { nonce, pkce_verifier, created: now });
    }

    /// The login `state` started, if it is still live; it is gone afterwards.
    pub fn take_at(&self, state: &str, now: Instant) -> Option<Pending> {
        let p = self.map.lock().expect("pending lock").remove(state)?;
        (now.saturating_duration_since(p.created) < PENDING_TTL).then_some(p)
    }

    pub fn len(&self) -> usize {
        self.map.lock().expect("pending lock").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoginError {
    /// The provider could not be reached or its metadata is unusable.
    #[error("the login provider is unavailable: {0}")]
    Unavailable(String),
    /// Unknown, expired or already used `state`.
    #[error("unknown or expired login")]
    BadState,
    /// The code exchange failed or the ID token did not verify.
    #[error("the login was not accepted: {0}")]
    Rejected(String),
    /// Verified, but not allowed to play.
    #[error("not allowed: {0:?}")]
    Denied(Denied),
}

pub struct Oidc {
    cfg: OidcConfig,
    redirect: RedirectUrl,
    http: reqwest::Client,
    provider: tokio::sync::Mutex<Option<(CoreProviderMetadata, Instant)>>,
    pub pending: PendingLogins,
}

impl Oidc {
    /// Checks the URLs; talks to nobody.
    pub fn new(cfg: &OidcConfig, public_url: &str) -> Result<Oidc, String> {
        IssuerUrl::new(cfg.issuer.clone()).map_err(|e| format!("OIDC_ISSUER `{}`: {e}", cfg.issuer))?;
        let redirect = RedirectUrl::new(format!("{public_url}/auth/callback"))
            .map_err(|e| format!("SIGNALBOX_PUBLIC_URL `{public_url}`: {e}"))?;
        let http = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        Ok(Oidc { cfg: cfg.clone(), redirect, http, provider: tokio::sync::Mutex::new(None), pending: PendingLogins::new() })
    }

    /// `SIGNALBOX_PUBLIC_URL` + `/auth/callback`.
    pub fn redirect_url(&self) -> &str {
        self.redirect.as_str()
    }

    async fn client(&self) -> Result<Client, LoginError> {
        let mut provider = self.provider.lock().await;
        let fresh = matches!(&*provider, Some((_, at)) if at.elapsed() < METADATA_TTL);
        if !fresh {
            let issuer = IssuerUrl::new(self.cfg.issuer.clone()).map_err(|e| LoginError::Unavailable(e.to_string()))?;
            let meta = CoreProviderMetadata::discover_async(issuer, &self.http)
                .await
                .map_err(|e| LoginError::Unavailable(format!("discovery: {}", chain(&e))))?;
            *provider = Some((meta, Instant::now()));
        }
        let meta = provider.as_ref().expect("fetched above").0.clone();
        Ok(CoreClient::from_provider_metadata(
            meta,
            ClientId::new(self.cfg.client_id.clone()),
            Some(ClientSecret::new(self.cfg.client_secret.clone())),
        )
        .set_redirect_uri(self.redirect.clone()))
    }

    /// Start a login: the provider URL to send the browser to, and the
    /// `state` the caller keeps in the login cookie.
    pub async fn begin(&self) -> Result<(String, String), LoginError> {
        let client = self.client().await?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state, nonce) = client
            .authorize_url(CoreAuthenticationFlow::AuthorizationCode, CsrfToken::new_random, Nonce::new_random)
            .add_scope(Scope::new("profile".into()))
            .add_scope(Scope::new("email".into()))
            .set_pkce_challenge(challenge)
            .url();
        let state = state.secret().clone();
        self.pending.insert_at(state.clone(), nonce.secret().clone(), verifier.secret().clone(), Instant::now());
        Ok((url.to_string(), state))
    }

    /// Finish a login: exchange the code, verify the ID token, admit the
    /// user. Returns the player name.
    pub async fn finish(&self, state: &str, code: &str) -> Result<String, LoginError> {
        let pending = self.pending.take_at(state, Instant::now()).ok_or(LoginError::BadState)?;
        let client = self.client().await?;
        let request = client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .map_err(|e| LoginError::Unavailable(format!("the provider has no token endpoint: {e}")))?;
        let token = request.set_pkce_verifier(PkceCodeVerifier::new(pending.pkce_verifier)).request_async(&self.http).await.map_err(
            |e| match e {
                RequestTokenError::Request(e) => LoginError::Unavailable(format!("token request: {}", chain(&e))),
                e => LoginError::Rejected(format!("token request: {}", chain(&e))),
            },
        )?;
        let id_token = token.id_token().ok_or_else(|| LoginError::Rejected("no ID token".into()))?;
        let claims = id_token
            .claims(&client.id_token_verifier(), &Nonce::new(pending.nonce))
            .map_err(|e| LoginError::Rejected(format!("ID token: {e}")))?;
        let groups = groups_from_id_token(&id_token.to_string()).map_err(LoginError::Rejected)?;
        admit(claims.preferred_username().map(|u| u.as_str()), &groups).map_err(LoginError::Denied)
    }
}
```

- [ ] **Step 5: The login routes**

Replace `crates/server/src/web.rs` with (changes from Task 6: the `Query` and `Deserialize` imports, the `oidc` and `session` imports, `AppState::oidc`, the two routes, `login` and `callback`; everything else is unchanged):
```rust
//! HTTP routes (spec §8): the placeholder page, the WebSocket, and the
//! auth routes. Nothing but the login flow answers without a session.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRef, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::get;
use axum_extra::extract::cookie::{Cookie, Key, SignedCookieJar};
use protocol::{GameInfo, LayoutInfo, codes};
use serde::Deserialize;

use crate::limit::RateLimit;
use crate::oidc::{Denied, LOGIN_COOKIE, LoginError, Oidc, PENDING_TTL};
use crate::session::{SESSION_COOKIE, SESSION_TTL, Sessions, cookie};
use crate::supervisor::Supervisor;

/// Largest message or frame a client may send.
pub const MAX_CLIENT_MESSAGE: usize = 64 * 1024;
/// A client that takes longer than this to accept one frame is dropped.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// WebSocket close code for too many messages ("policy violation").
pub const CLOSE_POLICY: u16 = 1008;

#[derive(Clone)]
pub struct AppState {
    pub sup: Arc<Supervisor>,
    pub sessions: Arc<Sessions>,
    pub key: Key,
    /// `None` only in a `dev-auth` build started without OIDC settings.
    pub oidc: Option<Arc<Oidc>>,
}

impl FromRef<AppState> for Key {
    fn from_ref(s: &AppState) -> Key {
        s.key.clone()
    }
}

pub fn router(state: AppState) -> Router {
    let r = Router::new()
        .route("/", get(index))
        .route("/ws", get(ws))
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", get(logout));
    #[cfg(feature = "dev-auth")]
    let r = r.route("/auth/dev", get(dev::login));
    r.with_state(state)
}

/// The logged-in user, from the signed session cookie.
pub fn user_of(state: &AppState, jar: &SignedCookieJar) -> Option<String> {
    let c = jar.get(SESSION_COOKIE)?;
    state.sessions.user(c.value())
}

pub fn escape_html(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '"' => "&quot;".into(),
            '\'' => "&#39;".into(),
            c => c.to_string(),
        })
        .collect()
}

/// The placeholder page until the browser client (sub-project D).
pub fn index_page(user: &str, games: &[GameInfo], layouts: &[LayoutInfo]) -> String {
    let e = escape_html;
    let mut rows = String::new();
    for g in games {
        let holders: Vec<String> =
            g.areas.iter().map(|a| format!("{}: {}", e(&a.name), e(a.holder.as_deref().unwrap_or("robot")))).collect();
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{:?}</td><td>{}</td><td>{}</td></tr>\n",
            e(&g.id),
            e(&g.layout),
            g.state,
            holders.join(", "),
            e(g.error.as_deref().unwrap_or("")),
        ));
    }
    let layouts: Vec<String> = layouts.iter().map(|l| e(&l.name)).collect();
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>signalbox</title></head><body>\n\
         <h1>signalbox</h1>\n<p>Signed in as {}. <a href=\"/auth/logout\">Sign out</a></p>\n\
         <p>The browser client is not built yet; bots play over <code>/ws</code>.</p>\n\
         <h2>Games</h2>\n<table><tr><th>Game</th><th>Layout</th><th>State</th><th>Areas</th><th>Error</th></tr>\n{}</table>\n\
         <h2>Layouts</h2>\n<p>{}</p>\n</body></html>\n",
        e(user),
        rows,
        layouts.join(", "),
    )
}

async fn index(State(state): State<AppState>, jar: SignedCookieJar) -> Response {
    let Some(user) = user_of(&state, &jar) else { return Redirect::to("/auth/login").into_response() };
    Html(index_page(&user, &state.sup.list_games(), &state.sup.layouts().infos())).into_response()
}

async fn login(State(state): State<AppState>, jar: SignedCookieJar) -> Response {
    let Some(oidc) = &state.oidc else { return (StatusCode::NOT_FOUND, "no login provider is configured").into_response() };
    match oidc.begin().await {
        Ok((url, login_state)) => (jar.add(cookie(LOGIN_COOKIE, &login_state, PENDING_TTL.as_secs())), Redirect::to(&url)).into_response(),
        Err(e) => {
            eprintln!("signalbox-server: login: {e}");
            (StatusCode::SERVICE_UNAVAILABLE, "the login service is unavailable; try again shortly").into_response()
        }
    }
}

#[derive(Deserialize)]
struct CallbackQuery {
    state: Option<String>,
    code: Option<String>,
}

/// The provider sends the browser back here. The `state` must be the one
/// this browser's login cookie holds (login CSRF) and a live, unused login.
async fn callback(State(state): State<AppState>, jar: SignedCookieJar, Query(q): Query<CallbackQuery>) -> Response {
    let Some(oidc) = &state.oidc else { return (StatusCode::NOT_FOUND, "no login provider is configured").into_response() };
    let started = jar.get(LOGIN_COOKIE).map(|c| c.value().to_string());
    let jar = jar.remove(Cookie::build(LOGIN_COOKIE).path("/"));
    let (Some(login_state), Some(code)) = (q.state, q.code) else {
        return (StatusCode::BAD_REQUEST, jar, "the login was cancelled or failed; start again at /auth/login").into_response();
    };
    if started.as_deref() != Some(login_state.as_str()) {
        return (StatusCode::BAD_REQUEST, jar, "this login was not started in this browser").into_response();
    }
    match oidc.finish(&login_state, &code).await {
        Ok(user) => {
            let id = state.sessions.create(&user);
            (jar.add(cookie(SESSION_COOKIE, &id, SESSION_TTL.as_secs())), Redirect::to("/")).into_response()
        }
        Err(LoginError::BadState) => (StatusCode::BAD_REQUEST, jar, "unknown or expired login; start again").into_response(),
        Err(LoginError::Denied(Denied::NotInGroup)) => {
            (StatusCode::FORBIDDEN, jar, "your account is not in signalbox-users").into_response()
        }
        Err(LoginError::Denied(Denied::NoUsername)) => {
            (StatusCode::FORBIDDEN, jar, "your account has no username").into_response()
        }
        Err(e @ LoginError::Rejected(_)) => {
            eprintln!("signalbox-server: callback: {e}");
            (StatusCode::FORBIDDEN, jar, "the login could not be verified").into_response()
        }
        Err(e @ LoginError::Unavailable(_)) => {
            eprintln!("signalbox-server: callback: {e}");
            (StatusCode::BAD_GATEWAY, jar, "the login service is unavailable; try again shortly").into_response()
        }
    }
}

async fn logout(State(state): State<AppState>, jar: SignedCookieJar) -> Response {
    if let Some(c) = jar.get(SESSION_COOKIE) {
        state.sessions.remove(c.value());
    }
    let jar = jar.remove(Cookie::build(SESSION_COOKIE).path("/"));
    (jar, Html("<!doctype html><p>Signed out. <a href=\"/auth/login\">Sign in</a></p>")).into_response()
}

async fn ws(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    let Some(user) = user_of(&state, &jar) else { return (StatusCode::UNAUTHORIZED, "sign in first").into_response() };
    let upgrade = match upgrade {
        Ok(u) => u,
        Err(e) => return e.into_response(),
    };
    upgrade
        .max_message_size(MAX_CLIENT_MESSAGE)
        .max_frame_size(MAX_CLIENT_MESSAGE)
        .on_upgrade(move |socket| client_loop(state.sup, user, socket))
}

async fn send(socket: &mut WebSocket, m: Message) -> bool {
    matches!(tokio::time::timeout(SEND_TIMEOUT, socket.send(m)).await, Ok(Ok(())))
}

/// One client socket: frames in go to the supervisor, the outbox goes out.
async fn client_loop(sup: Arc<Supervisor>, user: String, mut socket: WebSocket) {
    let me = sup.attach(&user);
    let mut limit = RateLimit::new(Instant::now());
    loop {
        tokio::select! {
            m = socket.recv() => {
                let text = match m {
                    Some(Ok(Message::Text(t))) => Some(t),
                    Some(Ok(Message::Binary(_))) => None,
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                };
                if !limit.allow(Instant::now()) {
                    let close = CloseFrame { code: CLOSE_POLICY, reason: "too many messages".into() };
                    let _ = send(&mut socket, Message::Close(Some(close))).await;
                    break;
                }
                match text {
                    Some(t) => sup.handle_text(&user, me.conn, t.as_str()),
                    None => sup.reply(&user, me.conn, protocol::ServerFrame::error(codes::BAD_MESSAGE, "binary frames are not used")),
                }
            }
            f = me.outbox.pop() => match f {
                Some(f) => {
                    if !send(&mut socket, Message::Text(f.to_json().into())).await {
                        break;
                    }
                }
                None => {
                    let _ = send(&mut socket, Message::Close(None)).await;
                    break;
                }
            },
        }
    }
    sup.detach(&user, me.conn);
}

#[cfg(feature = "dev-auth")]
pub mod dev {
    //! `/auth/dev?user=<name>`: a session for anyone, for tests and bots.
    //! Compiled only with the `dev-auth` feature.

    use axum::extract::{Query, State};
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Redirect, Response};
    use axum_extra::extract::cookie::SignedCookieJar;

    use super::AppState;
    use crate::session::{SESSION_COOKIE, SESSION_TTL, cookie};

    #[derive(serde::Deserialize)]
    pub struct DevQuery {
        user: String,
    }

    /// 1–32 of `A-Z a-z 0-9 _ . -`.
    pub fn valid_dev_user(s: &str) -> bool {
        (1..=32).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    }

    pub async fn login(State(state): State<AppState>, jar: SignedCookieJar, Query(q): Query<DevQuery>) -> Response {
        if !valid_dev_user(&q.user) {
            return (StatusCode::BAD_REQUEST, "user: 1 to 32 of A-Z a-z 0-9 _ . -").into_response();
        }
        let id = state.sessions.create(&q.user);
        (jar.add(cookie(SESSION_COOKIE, &id, SESSION_TTL.as_secs())), Redirect::to("/")).into_response()
    }
}
```

Note on the tuple responses: axum wants the status first, then response parts (the cookie jar), then the body — `(StatusCode::BAD_REQUEST, jar, "...")`.

- [ ] **Step 6: `start` builds the `Oidc`**

Replace `crates/server/src/lib.rs` with (changes from Task 6: the module doc, `pub mod oidc`, the `Oidc` import, and the `oidc` value put into `AppState`):
```rust
//! signalbox's server side: the game process (`process`, run by the
//! `signalbox-game` binary) and the front (`signalbox-server`): config,
//! sessions, login, layouts, the supervisor of game processes, and the web
//! routes.

pub mod config;
pub mod layouts;
pub mod limit;
pub mod oidc;
pub mod outbox;
pub mod process;
pub mod session;
pub mod supervisor;
pub mod web;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum_extra::extract::cookie::Key;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::layouts::Layouts;
use crate::oidc::Oidc;
use crate::session::Sessions;
use crate::supervisor::{Supervisor, SupervisorConfig};
use crate::web::AppState;

/// How long games get to save and exit when the front stops (spec §2.2).
pub const STOP_GRACE: Duration = Duration::from_secs(10);

/// A front serving on `addr`.
pub struct Running {
    pub addr: SocketAddr,
    pub sup: Arc<Supervisor>,
    pub sessions: Arc<Sessions>,
    stop: Arc<Notify>,
    server: JoinHandle<()>,
}

/// Start the front: data directories, layouts, supervisor, listener.
pub async fn start(cfg: Config) -> Result<Running, String> {
    let layouts = Layouts::load(&cfg.layouts_dir)?;
    let sup = Supervisor::new(
        SupervisorConfig {
            game_bin: cfg.game_bin.clone(),
            saves_dir: cfg.data_dir.join("saves"),
            sockets_dir: cfg.data_dir.join("sockets"),
            empty_exit_s: process::EMPTY_EXIT_S,
        },
        layouts,
    )?;
    let oidc = match &cfg.oidc {
        Some(o) => Some(Arc::new(Oidc::new(o, &cfg.public_url)?)),
        None => None,
    };
    let sessions = Arc::new(Sessions::new());
    let state = AppState { sup: sup.clone(), sessions: sessions.clone(), key: Key::from(&cfg.session_key), oidc };
    let listener = tokio::net::TcpListener::bind(cfg.addr).await.map_err(|e| format!("{}: {e}", cfg.addr))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let stop = Arc::new(Notify::new());
    let app = web::router(state);
    let server = tokio::spawn({
        let stop = stop.clone();
        async move {
            let _ = axum::serve(listener, app).with_graceful_shutdown(async move { stop.notified().await }).await;
        }
    });
    Ok(Running { addr, sup, sessions, stop, server })
}

impl Running {
    /// `http://<addr>`.
    pub fn base(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Shut every game down (≤ `STOP_GRACE`), then stop serving.
    pub async fn stop(self) {
        self.sup.shutdown_all(STOP_GRACE).await;
        self.stop.notify_one();
        let _ = tokio::time::timeout(Duration::from_secs(2), self.server).await;
    }
}

/// The binary: serve until SIGTERM or SIGINT, then stop cleanly.
pub async fn run(cfg: Config) -> Result<(), String> {
    let running = start(cfg).await?;
    eprintln!("signalbox-server: listening on {}", running.addr);
    let mut term = signal(SignalKind::terminate()).map_err(|e| e.to_string())?;
    let mut int = signal(SignalKind::interrupt()).map_err(|e| e.to_string())?;
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
    }
    eprintln!("signalbox-server: stopping");
    running.stop().await;
    Ok(())
}
```

- [ ] **Step 7: Run the tests**

Run: `scripts/cargo test -p signalbox-server --test oidc`
Expected: PASS (6 tests, under a second).

Run: `scripts/cargo test -p signalbox-server` and `scripts/cargo test -p signalbox-server --features dev-auth`
Expected: PASS (everything from Tasks 4–6 still passes; `oidc` runs in both builds).

Run: `scripts/cargo build --workspace --all-targets --config 'build.rustflags=["-D","warnings"]'` and `scripts/cargo build -p signalbox-server --features dev-auth --all-targets --config 'build.rustflags=["-D","warnings"]'`
Expected: both succeed with no warnings.

Run: `for c in openssl-sys aws-lc-sys; do scripts/cargo tree -p signalbox-server -i $c; done`
Expected: `error: package ID specification ... did not match any packages` for both (TLS is rustls on ring).

- [ ] **Step 8: Commit** (Cargo.lock changed a lot: the controller reseeds the CI cache before any push)

```bash
git add Cargo.toml Cargo.lock crates/server
git commit -m "feat(server): Authentik login: OIDC code flow with PKCE, group gate and sessions

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: `bot` — `Greedy`, a strategy that sees only its own screen

**Files:**
- Create: `crates/bot/src/strategy.rs`
- Modify: `crates/bot/src/lib.rs` (`pub mod strategy`)
- Test: `crates/bot/tests/strategy.rs`

**Interfaces:**
- Consumes: C1's `protocol::{Layout, View, BerthInfo, RouteInfo, PlayerCommand, ExitName, Aspect, RouteView, RouteState}`, `bot::Bot`; for the in-process test C1's `game::Game` and `ts2_import` (already dev-dependencies of the bot).
- Produces (module `bot::strategy`): `const RETRY_S: f64 = 30.0`, `const MAX_PER_DECISION: usize = 2`; `#[derive(Clone, Debug, Default)] struct Greedy` with `new() -> Greedy` and `decide(&mut self, layout: &Layout, view: &View) -> Vec<PlayerCommand>` (only `SetRoute`s, only on operable routes from operable berths' signals). Task 9's `NetPlayer` calls it twice a real second.

The rule (decision 12): for each operable berth, in layout order, whose signal shows red with a headcode in the berth and no route from that signal in the view, send `SetRoute` for that signal's next operable route (rotating through them), at most once per signal per 30 sim seconds and at most 2 commands per decision. It needs no knowledge of the sim beyond the view, which is the point: over the network a bot has nothing else.

- [ ] **Step 1: Write the failing tests**

Create `crates/bot/tests/strategy.rs`:
```rust
//! `Greedy` decides from a bot's own layout and view: the rules on a
//! hand-made screen, then Liverpool Street in process (no network) to show
//! its commands stay in the bot's area and the game stays safe.

use std::collections::BTreeMap;

use bot::Bot;
use bot::strategy::{Greedy, MAX_PER_DECISION, RETRY_S};
use game::{Game, GameMeta, Out};
use protocol::*;
use signalbox_core::world::World;

fn s(x: &str) -> String {
    x.to_string()
}

fn berth(name: &str, signal: Option<&str>, operable: bool) -> BerthInfo {
    BerthInfo { name: s(name), signal: signal.map(s), boundary: None, area: s("A"), operable }
}

fn route(entrance: &str, exit: &str, operable: bool) -> RouteInfo {
    RouteInfo {
        name: format!("{entrance}-{exit}"),
        entrance: s(entrance),
        exit: ExitName::Signal(s(exit)),
        automatic: false,
        operable,
    }
}

fn screen(berths: Vec<BerthInfo>, routes: Vec<RouteInfo>) -> Layout {
    Layout {
        title: s("test"),
        you: s("ann"),
        area: Some(s("A")),
        areas: vec![s("A"), s("B")],
        sections: vec![],
        segments: vec![],
        signals: vec![],
        points: vec![],
        berths,
        platforms: vec![],
        routes,
    }
}

fn view(t: f64, reds: &[&str], berths: &[(&str, &str)]) -> View {
    View {
        seq: 1,
        sim_time: t,
        speed: 1,
        paused: false,
        vote: None,
        holders: BTreeMap::new(),
        score: Some(0),
        signals: reds.iter().map(|r| (s(r), Aspect::Red)).collect(),
        routes: BTreeMap::new(),
        points: BTreeMap::new(),
        sections: BTreeMap::new(),
        berths: berths.iter().map(|(b, h)| (s(b), s(h))).collect(),
    }
}

fn entrances(cmds: &[PlayerCommand]) -> Vec<String> {
    cmds.iter()
        .map(|c| match c {
            PlayerCommand::SetRoute { entrance, exit: ExitName::Signal(x) } => format!("{entrance}-{x}"),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn greedy_routes_a_waiting_train_and_rotates_after_thirty_seconds() {
    let l = screen(vec![berth("B1", Some("S1"), true)], vec![route("S1", "X", true), route("S1", "Y", true), route("S2", "X", true)]);
    let mut g = Greedy::new();
    let v = view(100.0, &["S1"], &[("B1", "1A01")]);
    assert_eq!(entrances(&g.decide(&l, &v)), ["S1-X"]);
    assert!(g.decide(&l, &v).is_empty(), "no second attempt at once");
    let later = view(100.0 + RETRY_S - 0.1, &["S1"], &[("B1", "1A01")]);
    assert!(g.decide(&l, &later).is_empty());
    let later = view(100.0 + RETRY_S, &["S1"], &[("B1", "1A01")]);
    assert_eq!(entrances(&g.decide(&l, &later)), ["S1-Y"], "the next route after a failed attempt");
    let mut set = view(200.0, &["S1"], &[("B1", "1A01")]);
    set.routes.insert(s("S1-X"), RouteView { state: RouteState::Setting, auto_working: false });
    assert!(g.decide(&l, &set).is_empty(), "a route from it is already set");
}

#[test]
fn greedy_leaves_alone_what_is_not_waiting_or_not_its_own() {
    let mut g = Greedy::new();
    let l = screen(vec![berth("B1", Some("S1"), true)], vec![route("S1", "X", true)]);
    assert!(g.decide(&l, &view(0.0, &["S1"], &[])).is_empty(), "no headcode waiting");
    assert!(g.decide(&l, &view(0.0, &[], &[("B1", "1A01")])).is_empty(), "the signal is not at red");
    let fringe = screen(vec![berth("B1", Some("S1"), false)], vec![route("S1", "X", true)]);
    assert!(g.decide(&fringe, &view(0.0, &["S1"], &[("B1", "1A01")])).is_empty(), "a fringe berth");
    let theirs = screen(vec![berth("B1", Some("S1"), true)], vec![route("S1", "X", false)]);
    assert!(g.decide(&theirs, &view(0.0, &["S1"], &[("B1", "1A01")])).is_empty(), "no route it may set");
    let bare = screen(vec![berth("B1", None, true)], vec![]);
    assert!(g.decide(&bare, &view(0.0, &[], &[("B1", "1A01")])).is_empty(), "a berth at no signal");
}

#[test]
fn greedy_sends_at_most_two_commands_a_decision() {
    let names = ["S1", "S2", "S3"];
    let l = screen(
        names.iter().enumerate().map(|(i, n)| berth(&format!("B{i}"), Some(n), true)).collect(),
        names.iter().map(|n| route(n, "X", true)).collect(),
    );
    let v = view(0.0, &names, &[("B0", "1A01"), ("B1", "1A02"), ("B2", "1A03")]);
    let mut g = Greedy::new();
    assert_eq!(g.decide(&l, &v).len(), MAX_PER_DECISION);
    assert_eq!(entrances(&g.decide(&l, &v)), ["S3-X"], "the rest next time");
}

fn liverpool_json() -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    let ts2 = std::fs::read_to_string(format!("{dir}/../ts2-import/tests/data/liverpool-st.json")).unwrap();
    let text = std::fs::read_to_string(format!("{dir}/../../layouts/liverpool-st.areas.json")).unwrap();
    let mut w = ts2_import::convert(&ts2).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&text).unwrap()).unwrap();
    serde_json::to_string(&w).unwrap()
}

/// Deliver the game's messages to the bots; the bots' replies go back.
fn deliver(g: &mut Game, bots: &mut BTreeMap<String, Bot>, mut outs: Vec<Out>, not_your_area: &mut usize) {
    while !outs.is_empty() {
        let mut replies = Vec::new();
        for (name, msg) in outs {
            if matches!(msg, ServerMsg::Notice(Notice::NotYourArea { .. })) {
                *not_your_area += 1;
            }
            if let Some(b) = bots.get_mut(&name) {
                replies.extend(b.receive(msg).map(|r| (name.clone(), r)));
            }
        }
        outs = replies.into_iter().flat_map(|(n, m)| g.handle(&n, m)).collect();
    }
}

#[test]
fn greedy_bots_play_liverpool_street_in_their_own_areas() {
    let mut g = Game::new(World::from_json(&liverpool_json()).unwrap(), GameMeta { layout: s("liverpool-st"), seed: 7 });
    let seats = [("ann", "Liverpool Street"), ("hal", "Hackney & Bow")];
    let mut bots: BTreeMap<String, Bot> = BTreeMap::new();
    let mut greedy: BTreeMap<String, Greedy> = BTreeMap::new();
    let mut nya = 0;
    for (name, area) in seats {
        bots.insert(s(name), Bot::new());
        greedy.insert(s(name), Greedy::new());
        let outs = g.connect(name);
        deliver(&mut g, &mut bots, outs, &mut nya);
        let outs = g.handle(name, ClientMsg::Claim { area: s(area) });
        deliver(&mut g, &mut bots, outs, &mut nya);
    }
    for (name, _) in seats {
        let outs = g.handle(name, ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
        deliver(&mut g, &mut bots, outs, &mut nya);
    }
    assert_eq!(g.clock().speed, 8);
    // 20 sim minutes: a decision and a flush every 0.5 s of real time (40 ticks).
    while g.sim().tick() < 12_000 {
        for (name, _) in seats {
            let (l, v) = (bots[name].layout().unwrap().clone(), bots[name].view().unwrap().clone());
            for cmd in greedy.get_mut(name).unwrap().decide(&l, &v) {
                let outs = g.handle(name, ClientMsg::Command { cmd });
                deliver(&mut g, &mut bots, outs, &mut nya);
            }
        }
        let outs = g.advance(0.5);
        deliver(&mut g, &mut bots, outs, &mut nya);
        let outs = g.flush();
        deliver(&mut g, &mut bots, outs, &mut nya);
    }
    let st = g.stats().clone();
    assert_eq!((st.spads, st.collisions, st.invariant_violations), (0, 0, 0), "{st:?}");
    assert!(st.player_commands > 0, "the bots' commands reached the sim: {st:?}");
    assert_eq!(nya, 0, "Greedy only works its own area");
    for (name, _) in seats {
        assert_eq!(bots[name].view(), g.view_of(name).as_ref(), "{name}");
    }
    eprintln!("greedy, 20 sim minutes: {st:?}");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-bot --test strategy`
Expected: compile error — no module `bot::strategy`.

- [ ] **Step 3: Implement**

Create `crates/bot/src/strategy.rs`:
```rust
//! `Greedy`: a signaller that sees only what its client sees (the layout
//! and view the game sent), for bots playing over the network.
//!
//! Honest label (C2 decision 12): it is a transport exerciser, not a good
//! signaller. For each signal it may operate that shows red with a headcode
//! waiting in its berth and no route set from it, it tries that signal's
//! routes in layout order, one attempt per signal per `RETRY_S` sim seconds,
//! at most `MAX_PER_DECISION` commands per decision. The interlocking keeps
//! it safe; trains may still wait longer than under the robot.

use std::collections::BTreeMap;

use protocol::{Aspect, Layout, PlayerCommand, View};

/// Sim seconds before the same signal is tried again.
pub const RETRY_S: f64 = 30.0;
/// Commands one decision sends at most.
pub const MAX_PER_DECISION: usize = 2;

#[derive(Clone, Debug, Default)]
pub struct Greedy {
    /// Signal → sim time of its last attempt.
    tried: BTreeMap<String, f64>,
    /// Signal → how many attempts so far (picks the next route).
    attempts: BTreeMap<String, usize>,
}

impl Greedy {
    pub fn new() -> Greedy {
        Greedy::default()
    }

    /// The commands to send for this layout and view, in berth order.
    pub fn decide(&mut self, layout: &Layout, view: &View) -> Vec<PlayerCommand> {
        let now = view.sim_time;
        let mut out = Vec::new();
        for berth in layout.berths.iter().filter(|b| b.operable) {
            if out.len() >= MAX_PER_DECISION {
                break;
            }
            let Some(signal) = &berth.signal else { continue };
            if !view.berths.contains_key(&berth.name) || view.signals.get(signal) != Some(&Aspect::Red) {
                continue;
            }
            let routes: Vec<_> = layout.routes.iter().filter(|r| r.operable && &r.entrance == signal).collect();
            if routes.is_empty() || routes.iter().any(|r| view.routes.contains_key(&r.name)) {
                continue;
            }
            if self.tried.get(signal).is_some_and(|t| now - t < RETRY_S) {
                continue;
            }
            let n = self.attempts.entry(signal.clone()).or_insert(0);
            let route = routes[*n % routes.len()];
            *n += 1;
            self.tried.insert(signal.clone(), now);
            out.push(PlayerCommand::SetRoute { entrance: route.entrance.clone(), exit: route.exit.clone() });
        }
        out
    }
}
```

In `crates/bot/src/lib.rs`, add `pub mod strategy;` after `pub mod net;`.

- [ ] **Step 4: Run the tests**

Run: `scripts/cargo test -p signalbox-bot --test strategy -- --nocapture`
Expected: PASS (4 tests, a few seconds). The Liverpool Street test prints the game's stats; the prototype run gave `player_commands: 30, robot_commands: 7, sim_rejections: 21` and zero SPADs, collisions and invariant violations over 20 sim minutes (rejections are Greedy trying routes the interlocking refuses; it rotates to the next one).

Run: `scripts/cargo build --workspace --all-targets --config 'build.rustflags=["-D","warnings"]'`
Expected: success, no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/bot
git commit -m "feat(bot): Greedy, a strategy that decides from the bot's own layout and view

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: end to end — the network player, Liverpool Street through the real processes, a crash

**Files:**
- Modify: `crates/bot/Cargo.toml` (tokio features `time`, `macros`), `crates/bot/src/lib.rs` (`pub mod play`)
- Create: `crates/bot/src/play.rs`
- Modify: `crates/server/Cargo.toml` (`[[test]] e2e` with `required-features = ["dev-auth"]`; dev-dependency `ts2-import`)
- Test: `crates/server/tests/e2e.rs`

**Interfaces:**
- Consumes: Task 6 (`bot::net::{Conn, NetError, dev_login}`, the `tests/common` front harness: `Front`, `front_with`, `is_view`, `s`), Task 8 (`Greedy`), Task 5 (`Supervisor::status`, `Supervisor::pid`), Task 3 (`Counters`), C1's `Bot`, `ts2_import::{convert, areas}`, `layouts/liverpool-st.areas.json`.
- Produces (module `bot::play`): `struct NetPlayer { pub name: String, pub conn: Conn, pub bot: Bot, pub greedy: Greedy, pub game: Option<String>, pub lobby: Vec<LobbyReply>, pub commands_sent: usize }` with
  `async login(base: &str, name: &str) -> Result<NetPlayer, NetError>`, `new(name: &str, conn: Conn) -> NetPlayer`,
  `async lobby_msg(&mut self, LobbyMsg)`, `async game_msg(&mut self, ClientMsg)`, `async take(&mut self, ServerFrame)`,
  `async until(&mut self, limit: Duration, stop: impl Fn(&ServerFrame) -> bool) -> Result<ServerFrame, NetError>`,
  `async until_view(&mut self, limit: Duration, ok: impl Fn(&View) -> bool) -> Result<(), NetError>`,
  `async decide(&mut self) -> Result<usize, NetError>`,
  `async play_until(&mut self, until_s: f64, every: Duration, limit: Duration) -> Result<(), NetError>`,
  `async resync_and_compare(&mut self, limit: Duration) -> Result<(Option<View>, Option<View>), NetError>` (the delta-built view before, the fresh full view after).
  All return `NetError::Timeout` rather than hang. `Conn::recv` is cancel-safe (it only awaits the stream's `next`), so `play_until`'s `select!` between frames and decisions loses nothing.

What the tests prove (spec §1 criteria 1, 4, 5; §12), and what they do not:
- `liverpool_street_two_bots_and_the_robot_fast` (4 sim minutes at 8x, about 30 s; in the normal dev-auth run and so in CI) and `..._long` (`#[ignore]`, one sim hour, about 7.5 min): two bots on Liverpool Street and Hackney & Bow, the robot on Bethnal Green, all through the real front and real game process. After play both vote pause; each bot's delta-built view must equal a fresh full view (seq aside); the game's own counters must show no SPAD, collision or invariant violation anywhere (the robot's area included — its notices go to nobody, so counters are the only witness) and that bot commands reached the sim; no `not_your_area` ever. The long run also requires robot commands and prints the SQLite write time (`save_busy_ms`) — the C1 parked item; no hard limit on it.
- `a_crashed_game_leaves_the_front_and_the_other_game_running`: `/ws` without a session is 401; two Liverpool Street games; `kill -KILL` one game process; its player gets `notice game_crashed`; the other game's clock keeps moving; the lobby shows `crashed` / `running`; `join` resumes the crashed one from its save (paused).
- Not proved: that `Greedy` moves traffic well (decision 12). C1's in-process soak already proved traffic flow with robot-quality decisions.

- [ ] **Step 1: Manifests**

In `crates/bot/Cargo.toml`, change the tokio line to:
```toml
tokio = { workspace = true, features = ["net", "io-util", "time", "macros"] }
```

In `crates/server/Cargo.toml`, add after the `[[test]] front` table:
```toml
[[test]]
name = "e2e"
required-features = ["dev-auth"]
```
and add `ts2-import = { path = "../ts2-import" }` to `[dev-dependencies]` (after `signalbox-bot`).

- [ ] **Step 2: Write the failing tests**

Create `crates/server/tests/e2e.rs`:
```rust
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
```

- [ ] **Step 3: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-server --features dev-auth --test e2e`
Expected: compile error — no module `bot::play`.

- [ ] **Step 4: Implement `crates/bot/src/play.rs`**

```rust
//! A bot playing over the network: one `Conn`, a `Bot` keeping the layout
//! and view, and `Greedy` deciding what to send.

use std::time::Duration;

use protocol::{ClientFrame, ClientMsg, LobbyMsg, LobbyReply, ServerFrame, ServerMsg, View};
use tokio::time::{Instant, MissedTickBehavior, interval, timeout_at};

use crate::Bot;
use crate::net::{Conn, NetError, dev_login};
use crate::strategy::Greedy;

pub struct NetPlayer {
    pub name: String,
    pub conn: Conn,
    pub bot: Bot,
    pub greedy: Greedy,
    /// The game this socket is in, from the last `joined`.
    pub game: Option<String>,
    /// Lobby replies other than `joined`, oldest first.
    pub lobby: Vec<LobbyReply>,
    /// Commands the strategy sent.
    pub commands_sent: usize,
}

impl NetPlayer {
    /// Log in on a dev-auth front and open a socket.
    pub async fn login(base: &str, name: &str) -> Result<NetPlayer, NetError> {
        let cookie = dev_login(base, name).await?;
        let conn = Conn::connect(base, Some(&cookie)).await?;
        Ok(NetPlayer::new(name, conn))
    }

    pub fn new(name: &str, conn: Conn) -> NetPlayer {
        NetPlayer {
            name: name.to_string(),
            conn,
            bot: Bot::new(),
            greedy: Greedy::new(),
            game: None,
            lobby: Vec::new(),
            commands_sent: 0,
        }
    }

    pub async fn lobby_msg(&mut self, msg: LobbyMsg) -> Result<(), NetError> {
        self.conn.send(&ClientFrame::Lobby(msg)).await
    }

    pub async fn game_msg(&mut self, msg: ClientMsg) -> Result<(), NetError> {
        self.conn.send(&ClientFrame::Game(msg)).await
    }

    /// Take one frame: game messages go to the `Bot` (a resync it asks for
    /// is sent at once); `joined` records the game; other lobby replies are
    /// kept in `lobby`.
    pub async fn take(&mut self, f: ServerFrame) -> Result<(), NetError> {
        match f {
            ServerFrame::Game(msg) => {
                if let Some(reply) = self.bot.receive(msg) {
                    self.game_msg(reply).await?;
                }
            }
            ServerFrame::Lobby(LobbyReply::Joined { game, .. }) => self.game = Some(game),
            ServerFrame::Lobby(other) => self.lobby.push(other),
        }
        Ok(())
    }

    /// Take frames until one matches `stop` (it is taken too); returns a
    /// copy of it. Fails if the socket closes or `limit` passes first.
    pub async fn until(&mut self, limit: Duration, stop: impl Fn(&ServerFrame) -> bool) -> Result<ServerFrame, NetError> {
        let deadline = Instant::now() + limit;
        loop {
            let f = match timeout_at(deadline, self.conn.recv()).await {
                Ok(r) => r?.ok_or_else(|| NetError::Http(format!("{}: the server closed the socket", self.name)))?,
                Err(_) => return Err(NetError::Timeout(format!("{} waited {limit:?}", self.name))),
            };
            let hit = stop(&f);
            let copy = hit.then(|| f.clone());
            self.take(f).await?;
            if let Some(f) = copy {
                return Ok(f);
            }
        }
    }

    /// Take frames until the bot's view satisfies `ok`. Fails if the
    /// socket closes or `limit` passes first.
    pub async fn until_view(&mut self, limit: Duration, ok: impl Fn(&View) -> bool) -> Result<(), NetError> {
        let deadline = Instant::now() + limit;
        while !self.bot.view().is_some_and(&ok) {
            let f = match timeout_at(deadline, self.conn.recv()).await {
                Ok(r) => r?.ok_or_else(|| NetError::Http(format!("{}: the server closed the socket", self.name)))?,
                Err(_) => return Err(NetError::Timeout(format!("{} waited {limit:?} for its view", self.name))),
            };
            self.take(f).await?;
        }
        Ok(())
    }

    /// Send what the strategy decides for the current view.
    pub async fn decide(&mut self) -> Result<usize, NetError> {
        let (Some(layout), Some(view)) = (self.bot.layout(), self.bot.view()) else { return Ok(0) };
        let cmds = self.greedy.decide(layout, view);
        let n = cmds.len();
        for cmd in cmds {
            self.game_msg(ClientMsg::Command { cmd }).await?;
        }
        self.commands_sent += n;
        Ok(n)
    }

    /// Play until the view's sim time reaches `until_s`: take every frame
    /// and decide every `every` of real time. Fails after `limit`.
    pub async fn play_until(&mut self, until_s: f64, every: Duration, limit: Duration) -> Result<(), NetError> {
        let deadline = Instant::now() + limit;
        let mut tick = interval(every);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        while !self.bot.view().is_some_and(|v| v.sim_time >= until_s) {
            tokio::select! {
                r = timeout_at(deadline, self.conn.recv()) => {
                    let f = r
                        .map_err(|_| NetError::Timeout(format!("{} did not reach {until_s} s within {limit:?}", self.name)))??
                        .ok_or_else(|| NetError::Http(format!("{}: the server closed the socket", self.name)))?;
                    self.take(f).await?;
                }
                _ = tick.tick() => {
                    self.decide().await?;
                }
            }
        }
        Ok(())
    }

    /// Ask for a full view and wait for it; returns the view the bot had
    /// built from deltas just before, and the fresh one.
    pub async fn resync_and_compare(&mut self, limit: Duration) -> Result<(Option<View>, Option<View>), NetError> {
        let built = self.bot.view().cloned();
        self.game_msg(ClientMsg::Resync).await?;
        self.until(limit, |f| matches!(f, ServerFrame::Game(ServerMsg::View(_)))).await?;
        Ok((built, self.bot.view().cloned()))
    }
}
```

In `crates/bot/src/lib.rs`, add `pub mod play;` after `pub mod net;` (so the list reads `net`, `play`, `strategy`).

- [ ] **Step 5: Run the tests**

Run: `scripts/cargo test -p signalbox-server --features dev-auth --test e2e -- --nocapture`
Expected: PASS (2 tests, 1 ignored; about 35 s). The fast run prints a line like
`e2e 4 sim min at 8x: 30.0s real; commands player 10 robot 0; resyncs ann 0 hal 0; SQLite writes 30 ms in total (1.00 ms per real second)` (prototype run).

Run the long soak once and keep its printed line for the task report (the controller relays the SQLite write cost to the owner):
`scripts/cargo test --release -p signalbox-server --features dev-auth --test e2e -- --ignored --nocapture`
Expected: PASS in about 8 minutes, printing a line like `e2e 60 sim min at 8x: 450.1s real; commands player 62 robot 26; resyncs ann 0 hal 0; SQLite writes 361 ms in total (0.80 ms per real second)` (prototype run, release build).

Run: `scripts/cargo test -p signalbox-server --features dev-auth` and `scripts/cargo test -p signalbox-bot`
Expected: PASS.

Run: `scripts/cargo build --workspace --all-targets --config 'build.rustflags=["-D","warnings"]'` and `scripts/cargo build -p signalbox-server --features dev-auth --all-targets --config 'build.rustflags=["-D","warnings"]'`
Expected: both succeed with no warnings. `scripts/ci/test.sh` (from Task 6) already runs the dev-auth tests, so CI now runs the fast e2e and the crash test.

- [ ] **Step 6: Commit** (Cargo.lock changes: the controller reseeds the CI cache before any push)

```bash
git add Cargo.lock crates/bot crates/server
git commit -m "test(server): end to end: two network bots and the robot on Liverpool Street, and a crashed game

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: deployment files — image, compose, Authentik blueprints, README, smoke check

**Files:**
- Create: `deploy/Dockerfile`, `deploy/Dockerfile.dockerignore`, `deploy/docker-compose.yml`
- Create: `deploy/authentik/signalbox-oidc-blueprint.yaml.example`, `deploy/authentik/signalbox-access.yaml.example`
- Create: `deploy/smoke.sh` (executable), `deploy/README.md`
- Modify: `CLAUDE.md` (crates, commands, a server architecture section)

**Interfaces:**
- Consumes: the binaries `signalbox-server` and `signalbox-game` (Tasks 4, 6, 7; release build, no `dev-auth`), `ts2-import` and `layouts/*.areas.json` (C1), the config variables of Task 6 and the HTTP behaviour of Tasks 6–7 (`/` → 303 `/auth/login`, `/ws` → 401, `/auth/dev` → 404, `/auth/logout` → 200, `/auth/login` → 303 to the provider or 503).
- Produces: image layout `/opt/signalbox/bin/{signalbox-server,signalbox-game}`, `/opt/signalbox/layouts/{liverpool-st,drain,gretz-armainvilliers}.json`, user `signalbox` (uid 10160), volume `/data`; compose service/container `signalbox` on `127.0.0.1:9160`, image `local/signalbox:current`, env file `/srv/vault/creds/signalbox/oidc.env`; `deploy/smoke.sh <base-url> <303|503>` (exit 0 = all checks pass). The Controller section uses all of these.

Choices worth knowing:
- The builder converts the three vendored TS2 layouts with their area files at image build time (brief item 9), so the image is self-contained and a layout change is an image rebuild. Layout names are the file stems the lobby offers.
- rustls with bundled webpki roots means the runtime image needs no CA bundle and no OpenSSL; `debian:bookworm-slim` matches the builder's glibc.
- `init: true` puts tini in front of the server as PID 1 so SIGTERM reaches it and exited children are reaped; `stop_grace_period: 20s` covers the front's 10 s save window. `read_only: true`, `cap_drop: [ALL]` and `no-new-privileges` fit a process that only needs its volume and outbound HTTPS to Authentik.
- The env file is on the vault like `ops-next`'s and `dbbrowser`'s. Compose reads it at `up`; nothing is bind-mounted from the vault, so a boot where dockerd autostarts the container before the vault is unlocked still works (the `vault-docker-retry@` pattern on ra restarts only containers whose start failed on a path under the vault — the controller step D8 confirms this against the live wiring).
- The application entry in the blueprint carries `name: signalbox`: the reference pattern (grafana) omits it because that application already existed; a new one needs it.

- [ ] **Step 1: The image**

Create `deploy/Dockerfile`:
```dockerfile
# syntax=docker/dockerfile:1
# signalbox: the front (signalbox-server), the game process (signalbox-game)
# and the three converted TS2 layouts. Release build: no dev login.
# Build from the repository root:
#   docker build -f deploy/Dockerfile -t local/signalbox:$(git rev-parse --short HEAD) .

FROM rust:1.98-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p signalbox-server -p ts2-import --bins \
 && mkdir -p /out/bin /out/layouts \
 && cp target/release/signalbox-server target/release/signalbox-game /out/bin/ \
 && for n in liverpool-st drain gretz-armainvilliers; do \
      target/release/ts2-import "crates/ts2-import/tests/data/$n.json" -o "/out/layouts/$n.json" \
        --areas "layouts/$n.areas.json" || exit 1; \
    done

FROM debian:bookworm-slim
# TLS to Authentik is rustls with bundled webpki roots: no CA bundle, no OpenSSL.
RUN useradd --system --uid 10160 --user-group --home-dir /data --no-create-home --shell /usr/sbin/nologin signalbox \
 && install -d -o signalbox -g signalbox -m 0700 /data
COPY --from=build /out/bin/ /opt/signalbox/bin/
COPY --from=build /out/layouts/ /opt/signalbox/layouts/
USER signalbox
ENV SIGNALBOX_ADDR=0.0.0.0:9160 \
    SIGNALBOX_DATA=/data \
    SIGNALBOX_LAYOUTS=/opt/signalbox/layouts \
    SIGNALBOX_GAME_BIN=/opt/signalbox/bin/signalbox-game
EXPOSE 9160
VOLUME ["/data"]
ENTRYPOINT ["/opt/signalbox/bin/signalbox-server"]
```

Create `deploy/Dockerfile.dockerignore` (BuildKit reads `<Dockerfile>.dockerignore` next to the Dockerfile given with `-f`; without it the build context would include `target/`, which is gigabytes):
```text
# Build context for deploy/Dockerfile (BuildKit reads <Dockerfile>.dockerignore).
target
.cargo-home
.git
.superpowers
docs
deploy
```

- [ ] **Step 2: The smoke check — write it and see it fail with nothing running**

Create `deploy/smoke.sh`:
```bash
#!/usr/bin/env bash
# Smoke-check a running signalbox front from outside: nothing but the login
# answers without a session, and the build has no dev login.
# usage: deploy/smoke.sh <base-url> <303|503>
#   303: /auth/login must redirect to the provider's authorize endpoint
#   503: the provider is not reachable (a local test run)
set -euo pipefail
base=${1:?usage: deploy/smoke.sh <base-url> <303|503>}
login=${2:?usage: deploy/smoke.sh <base-url> <303|503>}
fail=0
# check <path> <status> [location glob]
check() {
  local out got loc
  out=$(curl -sk -o /dev/null -w '%{http_code} %{redirect_url}' "$base$1" || true)
  got=${out%% *}
  loc=${out#* }
  if [[ $got != "$2" ]] || { [[ -n ${3:-} ]] && [[ $loc != $3 ]]; }; then
    echo "FAIL $1: $got $loc (want $2 ${3:-})"
    fail=1
  else
    echo "ok   $1: $got $loc"
  fi
}
check / 303 "$base/auth/login"
check /ws 401
check "/auth/dev?user=smoke" 404
check /auth/logout 200
if [[ $login == 303 ]]; then
  check /auth/login 303 "https://*/application/o/authorize/*"
else
  check /auth/login 503
fi
exit $fail
```

Run: `chmod +x deploy/smoke.sh && deploy/smoke.sh http://127.0.0.1:19160 503; echo "exit $?"`
Expected: five `FAIL` lines with status `000` (nothing listens there yet) and `exit 1`.

- [ ] **Step 3: Build the image and check it** (a throwaway image and a `--rm` container on port 19160 only; touch no other container, volume or compose project)

Run: `docker build -f deploy/Dockerfile -t local/signalbox:test .`
Expected: success; the log shows the three `ts2-import` conversions (per-area section and signal counts; Liverpool Street: `Liverpool Street`, `Bethnal Green`, `Hackney & Bow`).

Run:
```bash
docker run --rm -d --name signalbox-test -p 127.0.0.1:19160:9160 \
  -e SIGNALBOX_SESSION_KEY=$(openssl rand -hex 64) -e OIDC_ISSUER=http://127.0.0.1:9/ \
  -e OIDC_CLIENT_ID=x -e OIDC_CLIENT_SECRET=y -e SIGNALBOX_PUBLIC_URL=http://127.0.0.1:19160 \
  local/signalbox:test
sleep 1
deploy/smoke.sh http://127.0.0.1:19160 503
docker exec signalbox-test ls /opt/signalbox/layouts
docker exec signalbox-test id
time docker stop signalbox-test
```
Expected: smoke prints five `ok` lines and exits 0 (`/` 303 to `/auth/login`, `/ws` 401, `/auth/dev` 404 — the release image has no dev login, `/auth/logout` 200, `/auth/login` 503 because no provider listens on port 9); the layouts are `drain.json gretz-armainvilliers.json liverpool-st.json`; `id` shows `uid=10160(signalbox)`; `docker stop` returns in about a second (SIGTERM handled, no 10 s kill).

Run: `docker rmi local/signalbox:test`
Expected: the throwaway image is gone.

- [ ] **Step 4: Compose, blueprints, README**

Create `deploy/docker-compose.yml`:
```yaml
# signalbox — multiplayer UK railway signalling (repo: ~/projects/signalbox, deploy/).
#
# Reach: tailnet only, https://ra.tail3e0c1e.ts.net:50160 via `tailscale serve` ->
# 127.0.0.1:9160 below. No caddy label: never on the public edge.
# Login: Authentik OIDC (provider/app `signalbox`, group `signalbox-users`; the provider
# blueprint with its client secret is in /srv/vault/creds/signalbox/). The front checks
# the `groups` claim itself as well.
# Secrets: /srv/vault/creds/signalbox/oidc.env (OIDC_CLIENT_ID, OIDC_CLIENT_SECRET,
# SIGNALBOX_SESSION_KEY). Compose reads it at `up` time, so the vault must be mounted
# then; the values live in the container config afterwards, and nothing is bind-mounted
# from the vault, so dockerd's autostart at boot does not need it.
# Data: the named volume below holds saves/ (one SQLite file per game) and sockets/.
services:
  signalbox:
    image: local/signalbox:current
    container_name: signalbox
    restart: unless-stopped
    # tini as PID 1: forwards SIGTERM to the front, which saves every game (<= 10 s).
    init: true
    stop_grace_period: 20s
    env_file: /srv/vault/creds/signalbox/oidc.env
    environment:
      SIGNALBOX_PUBLIC_URL: https://ra.tail3e0c1e.ts.net:50160
      OIDC_ISSUER: https://auth.skyes.lgbt/application/o/signalbox/
    ports:
      - "127.0.0.1:9160:9160"
    volumes:
      - signalbox-data:/data
    read_only: true
    cap_drop: [ALL]
    security_opt: ["no-new-privileges:true"]
    mem_limit: 2g
    pids_limit: 256
volumes:
  signalbox-data:
```

Create `deploy/authentik/signalbox-oidc-blueprint.yaml.example`:
```yaml
# Authentik OAuth2/OIDC provider and application for signalbox.
# NOT applied from here: the controller renders it with the real client id and secret
# into /srv/vault/creds/signalbox/oidc-blueprint.yaml (root, 0600) and applies that
# (deploy/README.md). The placeholders are the only differences.
version: 1
metadata: {name: signalbox-oidc}
entries:
  - model: authentik_providers_oauth2.oauth2provider
    id: signalbox-oidc
    identifiers: {name: signalbox-oidc}
    attrs:
      client_type: confidential
      grant_types: [authorization_code, refresh_token]   # authentik 5.x creates NO grant types unless listed
      client_id: "<OIDC_CLIENT_ID>"
      client_secret: "<OIDC_CLIENT_SECRET>"
      authorization_flow: !Find [authentik_flows.flow, [slug, default-provider-authorization-implicit-consent]]
      invalidation_flow: !Find [authentik_flows.flow, [slug, default-provider-invalidation-flow]]
      signing_key: !Find [authentik_crypto.certificatekeypair, [name, authentik Self-signed Certificate]]
      sub_mode: user_username
      include_claims_in_id_token: true
      issuer_mode: per_provider
      access_code_validity: minutes=1
      access_token_validity: hours=1
      refresh_token_validity: days=30
      redirect_uris: [{matching_mode: strict, url: https://ra.tail3e0c1e.ts.net:50160/auth/callback}]
      property_mappings:
        - !Find [authentik_providers_oauth2.scopemapping, [scope_name, openid]]
        - !Find [authentik_providers_oauth2.scopemapping, [scope_name, email]]
        - !Find [authentik_providers_oauth2.scopemapping, [scope_name, profile]]
  - model: authentik_core.application
    identifiers: {slug: signalbox}
    # `name` is required when the application is created (grafana's existed already).
    attrs: {name: signalbox, provider: !KeyOf signalbox-oidc, meta_launch_url: https://ra.tail3e0c1e.ts.net:50160}
```

Create `deploy/authentik/signalbox-access.yaml.example`:
```yaml
# signalbox's access group and binding, as snippets for the stack's blueprints in
# /opt/stack/apps/authentik/blueprints/ (see that directory's README). Members are
# the owner's decision; skye is listed because superuser status does NOT bypass
# application bindings.

# ---- append under `entries:` in 10-access-groups.yaml ----
  - model: authentik_core.group
    identifiers: {name: signalbox-users}
    attrs:
      users:
        - !Find [authentik_core.user, [username, skye]]

# ---- append under `entries:` in 40-access-bindings.yaml ----
  - model: authentik_policies.policybinding
    identifiers:
      target: !Find [authentik_core.application, [slug, signalbox]]
      group: !Find [authentik_core.group, [name, "signalbox-users"]]
    attrs: {order: 0, enabled: true, negate: false}
```

Create `deploy/README.md`:
````markdown
# Deploying signalbox on ra

One container runs the front (`signalbox-server`), which starts one
`signalbox-game` process per live game. It listens on `127.0.0.1:9160` only;
`tailscale serve` publishes it on the tailnet as
`https://ra.tail3e0c1e.ts.net:50160`. Login is Authentik OIDC, restricted to the
group `signalbox-users` (by an Authentik binding and again by the front, which
checks the `groups` claim). Nothing is on the public edge.

| File | What |
|---|---|
| `Dockerfile` | release image: both binaries and the converted layouts `liverpool-st`, `drain`, `gretz-armainvilliers` (no dev login) |
| `Dockerfile.dockerignore` | keeps `target/`, `.cargo-home/`, `.git/` out of the build context |
| `docker-compose.yml` | the service; copied to `/opt/stack/apps/signalbox/` |
| `authentik/signalbox-oidc-blueprint.yaml.example` | OAuth2 provider + application; rendered with the real secret into the vault |
| `authentik/signalbox-access.yaml.example` | the `signalbox-users` group and its binding, for the stack's blueprints |
| `smoke.sh` | checks a running front from outside |

## Configuration

All from the environment (see `crates/server/src/config.rs`); a missing or bad
value stops the front with exit code 2 and one line saying what is wrong.

| Variable | Where | Value |
|---|---|---|
| `SIGNALBOX_ADDR` | image | `0.0.0.0:9160` |
| `SIGNALBOX_DATA` | image | `/data` (volume: `saves/`, `sockets/`) |
| `SIGNALBOX_LAYOUTS` | image | `/opt/signalbox/layouts` |
| `SIGNALBOX_GAME_BIN` | image | `/opt/signalbox/bin/signalbox-game` |
| `SIGNALBOX_PUBLIC_URL` | compose | `https://ra.tail3e0c1e.ts.net:50160` (redirect URI = this + `/auth/callback`) |
| `OIDC_ISSUER` | compose | `https://auth.skyes.lgbt/application/o/signalbox/` |
| `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET` | vault `oidc.env` | must match the provider blueprint |
| `SIGNALBOX_SESSION_KEY` | vault `oidc.env` | hex, at least 64 bytes; signs the cookies |

Sessions live in memory: restarting the front logs everyone out (games are saved
and resume on the next join).

## Secrets

`/srv/vault/creds/signalbox/` (root, 0700) on the LUKS vault holds `oidc.env`
and `oidc-blueprint.yaml` (root, 0600). Compose reads `oidc.env` at `up` time,
so the vault must be mounted for `up`; nothing is bind-mounted from the vault,
so Docker's autostart at boot does not need it.

To create them (new client secret and session key):

```bash
sudo bash -c 'set -euo pipefail; umask 077
  d=/srv/vault/creds/signalbox; install -d -m 0700 "$d"
  id=$(openssl rand -hex 20); secret=$(openssl rand -hex 32); key=$(openssl rand -hex 64)
  printf "OIDC_CLIENT_ID=%s\nOIDC_CLIENT_SECRET=%s\nSIGNALBOX_SESSION_KEY=%s\n" "$id" "$secret" "$key" > "$d/oidc.env"
  t=$(cat /home/skye-fi/projects/signalbox/deploy/authentik/signalbox-oidc-blueprint.yaml.example)
  t=${t//"<OIDC_CLIENT_ID>"/$id}; t=${t//"<OIDC_CLIENT_SECRET>"/$secret}
  printf "%s\n" "$t" > "$d/oidc-blueprint.yaml"'
```

(Pure bash substitution, so the secret never appears in a process list.)

## Authentik

Follow `/opt/stack/apps/authentik/blueprints/README.md`. In order:

1. The provider and application, from the vault:

   ```bash
   dir=$(sudo mktemp -d)
   sudo install -m 0644 /srv/vault/creds/signalbox/oidc-blueprint.yaml "$dir/signalbox-oidc.yaml"
   sudo docker cp "$dir/signalbox-oidc.yaml" authentik-server:/blueprints/signalbox-oidc.yaml
   sudo rm -r "$dir"
   docker exec authentik-server ak apply_blueprint signalbox-oidc.yaml
   docker exec -u 0 authentik-server rm /blueprints/signalbox-oidc.yaml
   ```

2. The group: append the group entry of `authentik/signalbox-access.yaml.example`
   to `10-access-groups.yaml` (members are the owner's decision) and apply it as
   that README shows.
3. The binding: append the binding entry to `40-access-bindings.yaml` and apply
   it. Without a binding Authentik admits every authenticated user to the app.
4. Add `signalbox` to the README's list of vault-held OIDC blueprints.

## Build and run

```bash
cd /home/skye-fi/projects/signalbox            # at the commit to deploy
rev=$(git rev-parse --short HEAD)
docker build -f deploy/Dockerfile -t local/signalbox:$rev -t local/signalbox:current .
sudo install -d -o root -g docker -m 2755 /opt/stack/apps/signalbox
sudo install -o root -g docker -m 0644 deploy/docker-compose.yml /opt/stack/apps/signalbox/docker-compose.yml
cd /opt/stack/apps/signalbox && sudo docker compose up -d
docker logs signalbox            # "signalbox-server: listening on 0.0.0.0:9160"
sudo -n tailscale serve --bg --https=50160 http://127.0.0.1:9160
/home/skye-fi/projects/signalbox/deploy/smoke.sh https://ra.tail3e0c1e.ts.net:50160 303
```

Then sign in with a browser at `https://ra.tail3e0c1e.ts.net:50160/`: the page
says "Signed in as <username>".

## Update and roll back

Build a new `local/signalbox:<rev>`, retag it `current`, and
`sudo docker compose up -d` in `/opt/stack/apps/signalbox`. The front saves
every game on SIGTERM (up to 10 s; compose allows 20). To roll back, retag the
previous `<rev>` as `current` and `up -d` again. Saves are forward-compatible
only within save schema 2; a newer schema refuses old saves with
`unsupported save schema N`.

## Local check of an image (no Authentik)

```bash
docker build -f deploy/Dockerfile -t local/signalbox:test .
docker run --rm -d --name signalbox-test -p 127.0.0.1:19160:9160 \
  -e SIGNALBOX_SESSION_KEY=$(openssl rand -hex 64) -e OIDC_ISSUER=http://127.0.0.1:9/ \
  -e OIDC_CLIENT_ID=x -e OIDC_CLIENT_SECRET=y -e SIGNALBOX_PUBLIC_URL=http://127.0.0.1:19160 \
  local/signalbox:test
deploy/smoke.sh http://127.0.0.1:19160 503
docker exec signalbox-test ls /opt/signalbox/layouts
docker stop signalbox-test
```
````

Check the secret-rendering snippet from the README without root, in a scratch directory (it must substitute both placeholders and create 0600 files):
```bash
d=$(mktemp -d); bash -c 'set -euo pipefail; umask 077
  id=$(openssl rand -hex 20); secret=$(openssl rand -hex 32)
  t=$(cat deploy/authentik/signalbox-oidc-blueprint.yaml.example)
  t=${t//"<OIDC_CLIENT_ID>"/$id}; t=${t//"<OIDC_CLIENT_SECRET>"/$secret}
  printf "%s\n" "$t" > "'"$d"'/b.yaml"'
grep -c '<OIDC_' "$d/b.yaml"; stat -c %a "$d/b.yaml"; rm -r "$d"
```
Expected: `0` (no placeholder left) and `600`.

- [ ] **Step 5: CLAUDE.md**

In `CLAUDE.md`, replace
```
`crates/game` (`signalbox-game`: the multiplayer game library + SQLite saves),
`crates/bot` (`signalbox-bot`: headless client). The multiplayer design is
`docs/superpowers/specs/2026-09-30-server-and-protocol-design.md`.
```
with
```
`crates/game` (`signalbox-game`: the multiplayer game library + SQLite saves),
`crates/bot` (`signalbox-bot`: headless client, its network client and the `Greedy` strategy),
`crates/ipc` (`signalbox-ipc`: front ⇄ game frames over a Unix socket),
`crates/server` (`signalbox-server`: lib `server`; bins `signalbox-server`, the front,
and `signalbox-game`, one process per game). The multiplayer design is
`docs/superpowers/specs/2026-09-30-server-and-protocol-design.md`; deployment is in `deploy/`
(see `deploy/README.md`).
```

In the Commands block, after the `signalbox-bot --test soak` line, add:
```
scripts/cargo test -p signalbox-server                        # game process, supervisor, OIDC, release build (no /auth/dev)
scripts/cargo test -p signalbox-server --features dev-auth    # + the front over WebSockets, 4-min Liverpool St end to end, crash
scripts/cargo test --release -p signalbox-server --features dev-auth --test e2e -- --ignored --nocapture   # 1 sim hour; prints SQLite write cost
```

After the `### Multiplayer (`protocol`, `game`, `bot`)` section (before `### Tests`), add:
```markdown
### Server (`ipc`, `server`)
- `signalbox-game` (`server::process`) wraps one `Game`: `Shell` is the sync
  logic (tested without sockets), `serve` the tokio loop (advance every 0.1 s,
  flush every 0.2 s, `Status` every 1 s). It accepts exactly one front
  connection on its socket, and exits 0 after `Shutdown`, SIGTERM, the front
  going away or 10 minutes empty (always saving first), 1 if the save will not
  open (its last stderr line is the crash reason the lobby shows).
- `ipc` frames are a u32 BE length plus JSON, at most 4 MiB; `read_frame` is not
  cancel-safe, so every socket is read by a task of its own.
- The front's `Supervisor` holds the lobby, the children and the routing behind
  one `std::sync::Mutex` never held across an `.await`. Each client socket has an
  `Outbox` (64 frames; on overflow it is cleared, the game is asked for a resync
  and deltas are dropped until the next full view). A second login of a name
  replaces the old socket without a `Disconnect` (C1's contract on
  `Game::connect`). A child that ends other than by exiting 0 is `crashed`.
- Login is `server::oidc` (openidconnect: code flow, PKCE, nonce, one-shot state
  plus a signed login cookie); `admit` requires `groups` ∋ `signalbox-users`.
  Sessions are server-side and in memory. The `dev-auth` feature adds
  `/auth/dev?user=` for tests and bots; the release image is built without it.
- Front tests: `crates/server/tests/common/mod.rs` starts a front in process
  (`server::start`, dev login, free port, real `signalbox-game` children, temp
  data dir); `tests/supervisor.rs` drives `Supervisor` without HTTP;
  `tests/oidc.rs` runs a small OpenID provider in the test.
```

- [ ] **Step 6: Nothing else moved**

Run: `scripts/cargo build --workspace --all-targets --config 'build.rustflags=["-D","warnings"]'`
Expected: success (this task changes no Rust).

Run: `git status --short`
Expected: only `deploy/` and `CLAUDE.md`.

- [ ] **Step 7: Commit**

```bash
git add deploy CLAUDE.md
git commit -m "feat(deploy): image, compose, Authentik blueprints, README and smoke check

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Controller (not for subagents)

Everything below is done by the controller, in the main session, and **each numbered step needs the owner's explicit OK at the time** (show the step, wait for "yes"). Subagents never run these. Stop and report on any unexpected output; none of these steps is retried blindly.

### C. CI cache reseed (during execution, before any push)

After a task whose commit changed `Cargo.lock` with **new crates** — Tasks 3 (tokio), 6 (axum, axum-extra, tokio-tungstenite, futures-util) and 7 (openidconnect, base64, about 150 crates) — and before anything is pushed:

```bash
cd /home/skye-fi/projects/signalbox
scripts/cargo fetch --locked
sudo /opt/stack/apps/signalbox-runner/seed-cache.sh
```
Expected: the fetch downloads nothing new or only the new crates; the seed script ends with `du` lines for `/cache/cargo-home` (about 200 MB for the whole registry as of Task 7). Tasks 4, 5 and 9 change `Cargo.lock` only by adding workspace packages or edges (no new crates); reseeding after them is harmless but not needed. Note that `.cargo-home/registry` is shared with every scratch build run through `scripts/cargo`, so the seed may carry a few extra crates; that is only size.

Then push (both remotes, per the repo's usual practice) and watch the Forgejo run: it must pass `scripts/ci/test.sh`, which now also builds and tests `signalbox-server` with `dev-auth` (the 4-minute end-to-end run takes about 30 s of it).

### D. Deployment on ra (after the branch is merged and CI is green)

**D0. Preconditions.** Check, and show the owner:
```bash
ss -ltn | grep -E ':9160\b' || echo "9160 free"
sudo -n tailscale serve status            # 50160 must not be in use; 50100-50150 are taken
mountpoint /srv/vault
docker ps --format '{{.Names}}' | grep -x signalbox || echo "no signalbox container"
```

**D1. Secrets** (new OIDC client id and secret, new session key; nothing printed):
```bash
sudo bash -c 'set -euo pipefail; umask 077
  d=/srv/vault/creds/signalbox; install -d -m 0700 "$d"
  test ! -e "$d/oidc.env" || { echo "oidc.env exists; not overwriting"; exit 1; }
  id=$(openssl rand -hex 20); secret=$(openssl rand -hex 32); key=$(openssl rand -hex 64)
  printf "OIDC_CLIENT_ID=%s\nOIDC_CLIENT_SECRET=%s\nSIGNALBOX_SESSION_KEY=%s\n" "$id" "$secret" "$key" > "$d/oidc.env"
  t=$(cat /home/skye-fi/projects/signalbox/deploy/authentik/signalbox-oidc-blueprint.yaml.example)
  t=${t//"<OIDC_CLIENT_ID>"/$id}; t=${t//"<OIDC_CLIENT_SECRET>"/$secret}
  printf "%s\n" "$t" > "$d/oidc-blueprint.yaml"'
sudo ls -l /srv/vault/creds/signalbox/ /srv/vault/creds/ops-oidc.env
sudo grep -c '<OIDC_' /srv/vault/creds/signalbox/oidc-blueprint.yaml
```
Expected: both files `-rw------- root root`, like `ops-oidc.env` (if `ops-oidc.env`'s owner or mode differs, match it and say so); the grep prints `0`.

**D2. Authentik provider and application** (from the vault; the procedure of `/opt/stack/apps/authentik/blueprints/README.md` with the file kept out of `/tmp`):
```bash
dir=$(sudo mktemp -d)
sudo install -m 0644 /srv/vault/creds/signalbox/oidc-blueprint.yaml "$dir/signalbox-oidc.yaml"
sudo docker cp "$dir/signalbox-oidc.yaml" authentik-server:/blueprints/signalbox-oidc.yaml
sudo rm -r "$dir"
docker exec authentik-server ak apply_blueprint signalbox-oidc.yaml
docker exec -u 0 authentik-server rm /blueprints/signalbox-oidc.yaml
```
Expected: `apply_blueprint` reports success (no validation errors). If it complains about the application's fields, show the owner the message before changing anything.

Then check that the provider signs its ID tokens with RS256, i.e. that the blueprint's `signing_key` lookup (`!Find`) found the certificate-key pair:
```bash
curl -s https://auth.skyes.lgbt/application/o/signalbox/jwks/ | python3 -c 'import json,sys; ks=[k for k in json.load(sys.stdin).get("keys",[]) if k.get("kty")=="RSA"]; print(len(ks), "RSA key(s)")'
```
Expected: at least `1 RSA key(s)`. If it prints `0` (an empty `keys` list), the `signing_key` `!Find` matched nothing and the provider would not sign with an RSA key the front can check — stop and show the owner; do not go on to D3.

**D3. Group and binding.** Ask the owner **who is in `signalbox-users`** (the snippet lists only `skye`; superuser does not bypass bindings). Append the group entry of `deploy/authentik/signalbox-access.yaml.example`, with the owner's members, under `entries:` of `/opt/stack/apps/authentik/blueprints/10-access-groups.yaml`, and the binding entry under `entries:` of `40-access-bindings.yaml` (with `sudo`, keeping the files' owner and mode). Show the diff, then apply both, 10 first, exactly as that README shows:
```bash
cd /opt/stack/apps/authentik/blueprints
for f in 10-access-groups.yaml 40-access-bindings.yaml; do
  t=$(mktemp); cat "$f" > "$t"; chmod 644 "$t"
  sudo docker cp "$t" "authentik-server:/blueprints/$f"; rm "$t"
  docker exec authentik-server ak apply_blueprint "$f"
  docker exec -u 0 authentik-server rm "/blueprints/$f"
done
```
Then add `signalbox` to that README's list of vault-held OIDC blueprints (`/srv/vault/creds/{wazuh,thoth-webui,grafana,signalbox}/oidc-blueprint.yaml`) and to its membership note.

**D4. Build the image** from the merged `main`:
```bash
cd /home/skye-fi/projects/signalbox && git switch main && git status --short
rev=$(git rev-parse --short HEAD)
docker build -f deploy/Dockerfile -t "local/signalbox:$rev" -t local/signalbox:current .
```

**D5. Install and start:**
```bash
sudo install -d -o root -g docker -m 2755 /opt/stack/apps/signalbox
sudo install -o root -g docker -m 0644 deploy/docker-compose.yml /opt/stack/apps/signalbox/docker-compose.yml
cd /opt/stack/apps/signalbox
sudo docker compose config --quiet && sudo docker compose up -d
sleep 2; docker logs signalbox
```
Expected: `signalbox-server: listening on 0.0.0.0:9160` and nothing else. Exit code 2 with a config line means a variable is wrong — fix `oidc.env`, not the image.

**D6. Tailnet and smoke check:**
```bash
sudo -n tailscale serve --bg --https=50160 http://127.0.0.1:9160
sudo -n tailscale serve status
/home/skye-fi/projects/signalbox/deploy/smoke.sh https://ra.tail3e0c1e.ts.net:50160 303
```
Expected: five `ok` lines. `/auth/login` answering 303 to `https://auth.skyes.lgbt/application/o/authorize/...` proves the container reached Authentik's discovery document and JWKS over HTTPS. If it is 503, `docker logs signalbox` shows `login: the login provider is unavailable: discovery: ...` with the cause (DNS, egress, TLS) — report it to the owner; the fallback (joining the container to Authentik's network with an internal issuer URL) changes the issuer the browser sees and needs its own design, so do not improvise it. A 503 can also be an **issuer mismatch**, not network trouble: `openidconnect` compares the discovery document's `issuer` with `OIDC_ISSUER` exactly, trailing slash included. Read the `discovery:` cause in `docker logs signalbox` first; if it names an issuer mismatch, set `OIDC_ISSUER` in the compose file to exactly the `issuer` that `curl -s https://auth.skyes.lgbt/application/o/signalbox/.well-known/openid-configuration` reports, and `up -d` again.

Then the owner signs in with a browser at `https://ra.tail3e0c1e.ts.net:50160/`: the page must say `Signed in as <their username>`. A user outside `signalbox-users` must be stopped by Authentik (binding) — if the owner wants that checked, use a test account, not a real person's.

Then a game that saves under the hardened compose (read-only root filesystem, dropped capabilities). In the owner's signed-in browser, on the signalbox page, in the developer console:
```js
ws = new WebSocket('wss://ra.tail3e0c1e.ts.net:50160/ws'); ws.onmessage = e => console.log(e.data);
ws.send(JSON.stringify({type: 'create_game', layout: 'drain'}));   // once it is open: `joined` with the game id, then `layout`, `view`
// wait more than 60 s, note the last `sim_time`, then:
ws.send(JSON.stringify({type: 'leave'}));
```
Note the game id and the last `sim_time`; the game saves when it empties. D7 then restarts the front and resumes it.

**D7. Shutdown and resume check:**
```bash
cd /opt/stack/apps/signalbox && sudo docker compose restart && sleep 2 && docker logs --since 1m signalbox
```
Expected: `signalbox-server: stopping` followed by a new `listening` line within 20 s (games, if any, were saved; players sign in again because sessions are in memory).

Then resume D6's game: the owner signs in again at `https://ra.tail3e0c1e.ts.net:50160/`, opens the socket as in D6 and sends `{"type":"join","game":"<the id>"}`. Expected: `joined`, then a `view` whose `sim_time` is at least the one noted in D6 (a resumed game starts paused). `docker logs signalbox | grep -c 'save failed'` prints `0`; any `save failed` line means the read-only root filesystem or the volume is in the way — stop and report it.

**D8. Boot durability and the vault.** Check how vault-dependent plain containers are wired on ra and follow it:
```bash
systemctl cat vault-docker-retry@srv-vault.service
sudo cat /etc/systemd/system/stack-vault.service.d/remount.conf
docker inspect signalbox --format '{{json .Mounts}}'
```
Expected, per the current design: `vault-docker-retry@srv-vault` restarts only containers whose `State.Error` names a path under `/srv/vault`, and signalbox mounts nothing from there (only the `signalbox_signalbox-data` volume), so dockerd's own autostart at boot suffices — nothing to add. If the retry script instead works from a list of containers, or signalbox does mount a vault path, add `signalbox` the same way the existing containers are added and show the owner the change first.

**D9. Record it.** Tell the owner the new tailnet port (50160) and the data volume `signalbox_signalbox-data`, and ask whether it goes into the borg backup set (the owner's decision; saves are small SQLite files). Update the controller's memory with the deployment facts (port, paths, vault files, image tags).

**Rollback** (any step after D5): `cd /opt/stack/apps/signalbox && sudo docker compose down` (the volume stays), `sudo -n tailscale serve --https=50160 off`; the Authentik provider/application can stay disabled or be removed with a `state: absent` blueprint entry — ask the owner.
