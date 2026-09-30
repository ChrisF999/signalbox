# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

signalbox is a deterministic UK railway signalling simulation in Rust (a rewrite of
the TS2 game), headed for multiplayer signal boxes in the browser. The design is in
`docs/superpowers/specs/2026-09-29-core-and-converter-design.md`; the task-by-task
implementation plans it was built from are in `docs/superpowers/plans/`.

## Commands

Use `scripts/cargo` in place of `cargo` where no local toolchain is installed (it
runs the official Rust image in Docker with the repo mounted at `/w`; it does **not**
forward environment variables, and the image has no clippy or rustfmt).

```bash
scripts/cargo test                                            # everything
scripts/cargo test -p signalbox-core --test release           # one test file
scripts/cargo test -p signalbox-core --test release cancel_keeps_section_under_train   # one test
scripts/cargo test --release -p ts2-import --test soak -- --ignored   # slow Liverpool St soak
scripts/cargo test -p signalbox-game                          # game library (twobox fixture, saves in a temp dir)
scripts/cargo test --release -p signalbox-bot --test soak -- --ignored   # 3 h Liverpool St: two bots + robot
scripts/cargo test -p signalbox-server                        # game process, supervisor, OIDC, release build (no /auth/dev)
scripts/cargo test -p signalbox-server --features dev-auth    # + the front over WebSockets, 4-min Liverpool St end to end, crash
scripts/cargo test --release -p signalbox-server --features dev-auth --test e2e -- --ignored --nocapture   # 1 sim hour; prints SQLite write cost
scripts/cargo test -p signalbox-client-core                   # client logic: connection, lobby, clicks (in-process game)
scripts/cargo test -p signalbox-client-ui                     # diagram and screens, headless egui (no GPU)
scripts/cargo test -p signalbox-server --features dev-auth --test client   # client-core over the real front
scripts/wasm-build                                            # the browser client into target/web-dist/ (tools image on first use)
scripts/ci/test.sh                                            # the CI gate (native cargo, offline, -D warnings)

scripts/cargo run -p sim-cli -- run crates/core/tests/fixtures/junction.json --robot --hours 1 --record /w/target/log.json
scripts/cargo run -p sim-cli -- replay crates/core/tests/fixtures/junction.json /w/target/log.json
scripts/cargo run -p ts2-import -- crates/ts2-import/tests/data/drain.json -o /w/target/drain.json
scripts/cargo run -p ts2-import -- crates/ts2-import/tests/data/liverpool-st.json -o /w/target/lst.json --areas /w/layouts/liverpool-st.areas.json
```

Paths passed through `scripts/cargo` resolve inside the container (`/w` = repo root).

The stock Rust image has no wasm32 target: `scripts/wasm-build` runs
`scripts/build-web.sh` in `local/signalbox-wasm-tools:<wasm-bindgen version>`,
built on first use from the `wasm-tools` stage of `deploy/Dockerfile`. The
`wasm-bindgen` crate is pinned exactly and the CLI must be the same version;
bumping it means a new tools image and a new CI runner image (maintainer).

CI (`.forgejo/workflows/ci.yml`) runs on a self-hosted Forgejo runner that builds
**offline** with warnings as errors; adding a crate dependency means the runner's
cargo cache must be reseeded by the maintainer. The GitHub repo is a mirror without CI.

Converter warning snapshots live in `crates/ts2-import/tests/expected/`. To re-record
after an intended change, run the `convert` test with `UPDATE_EXPECTED=1` set inside
the container (e.g. `docker run ... -e UPDATE_EXPECTED=1 rust:1.98-slim-bookworm cargo test -p ts2-import --test convert`), then review the diff.

## Architecture

Workspace crates: `crates/core` (library `signalbox-core`), `crates/sim-cli`
(headless run/replay), `crates/ts2-import` (TS2 → signalbox converter, lib + CLI),
`crates/protocol` (`signalbox-protocol`: wire messages, views, deltas),
`crates/game` (`signalbox-game`: the multiplayer game library + SQLite saves),
`crates/bot` (`signalbox-bot`: headless client, its network client and the `Greedy` strategy),
`crates/ipc` (`signalbox-ipc`: front ⇄ game frames over a Unix socket),
`crates/server` (`signalbox-server`: lib `server`; bins `signalbox-server`, the front,
and `signalbox-game`, one process per game), `crates/client-core` (`signalbox-client-core`: the
browser client's logic), `crates/client-ui` (`signalbox-client-ui`: its egui
screens) and `crates/client-web` (`signalbox-client-web`: the wasm shell;
design in `docs/superpowers/specs/2026-09-30-browser-client-design.md`). The multiplayer design is
`docs/superpowers/specs/2026-09-30-server-and-protocol-design.md`; deployment is in `deploy/`
(see `deploy/README.md`).

### World vs state
- `world::World` is static: loaded from a JSON `WorldFile` (`world/file.rs`, names
  everywhere), resolved to typed arena ids (`ids.rs`) and validated in
  `world/load.rs`. Invalid worlds are rejected with `LoadError`, never at runtime.
- `sim::SimState` is everything dynamic and is serde-serialisable; `Sim` owns a
  `World`, a `SimState`, the RNG and a derived `Occupancy` rebuilt every tick.
  `snapshot`/`restore`/`replay` depend on all mutable state living in `SimState`.

### The tick (`Sim::step`, 100 ms of sim time)
Order matters and tests depend on it: apply queued commands (logged with their tick)
→ points movement → timetable entries → `Interlocking::update` (locking, sectional
release, overlap/cancel timers) → `refresh_aspects` → move trains (driver model,
signal passes/SPADs, calls, describer, exits) → rebuild occupancy → detect
collisions → scoring. Interlocking and the driver therefore see start-of-tick
occupancy and aspects.

### Determinism rules (replay must be bit-exact)
No `HashMap`/`HashSet` in sim or converter logic (use `BTreeMap`/`BTreeSet`/`Vec`),
no wall clock (sim time = `start_s + tick * TICK_S`), one seeded `ChaCha8Rng`
drawn in a fixed order, everything processed in index order. `serde_json` has
`float_roundtrip` on so snapshots survive JSON.

### Interlocking model (`interlocking.rs`)
- Each track section has at most one owner: `Owner::Path(route)` or
  `Owner::Overlap(route)`. Setting a route claims its path and overlap sections;
  a route may take over the overlap of the route it continues (whose exit is its
  entrance). Points are locked implicitly by their section being owned.
- Per-route `progress` per path section (`Untouched → Occupied → Released`) drives
  sectional release in running order; out-of-sequence occupation releases nothing.
- A signal shows proceed only if `proceed()` holds (route locked, not cancelled,
  path untouched and clear, overlap held and clear, points detected). Aspects are
  computed recursively from the exit signal (`aspect::cleared_aspect`).
- Cancel = approach locking with route holding: sections under or ahead of a train
  on the route stay locked.

### Routes are validated against the track
`Network::trace_route` (in `network.rs`) walks from an entrance signal with the
route's points positions and reports the real path, exit and overlap. The loader
requires a route's declared path/exit/overlap/points to match it exactly, so routes
are always **signal to signal** (exit = the first same-direction signal) and every
signal used by a route must stand on a section boundary. Trailing points must be
listed too (at the leg the route runs over). The converter reuses the same tracer.

### Trains and the robot
`trains.rs` keeps each train as a deque of `(segment, dir)` from tail to head plus
`head_m`; `advance` returns every swept stretch so signals and platforms passed in
one tick are all seen (half-open `(from, to]`). `driver.rs` reads signals within
sighting distance and expects the rest from the last aspect passed (a new train
expects red). `robot.rs` is a deterministic auto-signaller; `robot::soak` runs a
world under it and is the integration oracle (no SPADs, collisions, invariant
violations or stuck trains).

### TS2 converter (`crates/ts2-import`)
Pipeline in `lib.rs::convert`: parse TS2 JSON (`ts2.rs`) → `graph::build` (TS2's
item chain → nodes/segments/sections; points get 1 m legs, touching zero-length
items get 1 m spacers, sections break at signals and points, flat crossings share a
section) → `timetable::build` → load the world without routes → `routes::build`
(walk each TS2 route, split at signals via `trace_route`, merge, generate routes
for signals TS2 never started one from, overlaps ≤ 180 m) → `routes::finish` (drop
any route the loader rejects, with a warning). Anything the target can't express is
a `report::Report` warning; the output always passes `World::from_file`. Generated
names are derived from TS2 ids (`L<tiId>`, `N<tiId>`, `P<tiId><p|n|r>`, `T1..`), so
output is byte-identical for the same input.

### Multiplayer (`protocol`, `game`, `bot`)
- Areas for converted layouts come from `layouts/<name>.areas.json`, applied by
  `ts2-import --areas`: each area floods the section graph from its seeds and
  stops at nodes holding boundary signals (`ts2_import::areas`).
- `game::Game` is pure: `connect`/`handle`/`advance(real_dt)`/`flush` return
  `(player, ServerMsg)` pairs. It maps every command to its subject's area
  (`game::areas::AreaMap`), refuses commands outside the sender's area, and runs
  `robot::commands` every `ROBOT_EVERY_TICKS` for areas nobody holds.
- Views are built from sim state per player (`game::view::build_view`) — own area
  plus a fringe walked along the track to the first signal — and sent as deltas
  (`protocol::diff`, `View::apply`); a client that sees a `seq` gap resyncs.
- Wire commands carry names (`protocol::PlayerCommand`); the save logs core
  `Command`s with ids, since the world is copied into each save.
- Resume (`game::save::resume_sim`) restores the newest snapshot and replays the
  log rows after its `last_seq` up to the last logged tick, leaving
  that tick's commands queued (and the robot marked as run if it logged there).

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
- The front stops accepting connections before it shuts the games down; game
  children run in their own process group, so a terminal's Ctrl-C reaches only
  the front. Names are refused when they are `robot`. `/` redirects (303) to
  `/auth/login`, `/ws` is 401 without a session, `/auth/dev` is 404 in the
  release build, `/auth/login` is 303 to the provider or 503 if it is unreachable.
  `deploy/smoke.sh` checks exactly these against a running front.

### Browser client (`client-core`, `client-ui`, `client-web`)
- `client_core::App` is pure: the shell calls `tick(now)` every frame and
  hands it a `Transport`. It keeps the layout and view in a `bot::Bot`, which
  is why `signalbox-bot` has a default `net` feature (without it: no tokio).
  Reconnect backoff 0.5 s doubling to 10 s, reset only once a connection
  carries a frame; a reconnect sends one `join` (its layout and view are the
  resync), never `resync`; `notice replaced` stops reconnecting.
- `client-ui` uses egui only (no eframe): `UiApp::ui` is one whole frame. The
  scene, camera, hit-testing and drawing are pure and tested as shapes; the
  screens run in `Context::run_ui` with synthetic events (tests clear
  `textures_delta`, there is no GPU).
- `client-web` builds only for wasm32 (natively it is empty). WebGPU falls
  back to WebGL2 inside egui-wgpu. A socket that closes without opening is
  followed by `GET /ws`: 401 means the session is gone → `/auth/login`.
- Diagram geometry is `game::geometry::WorldGeometry`, read once from the
  world's `layout` (ts2-import writes it); points legs, signal facings and
  exit positions are found by walking up to 4 nodes to a drawn line. The
  train list is `game::view::build_trains`, from sim state only.
- The front serves `SIGNALBOX_WEB` from memory (`server::assets`) behind the
  session; without that directory `/` is the C2 placeholder page.
- The workspace `rand` has no default features (getrandom does not build for
  wasm32-unknown-unknown); the server turns on `thread_rng`.

### Tests
- Core fixtures: `crates/core/tests/fixtures/{plain_line,terminus,junction}.json`.
  `tests/common/mod.rs` has `load_with(name, |json| ...)` for mutating a fixture
  per test, `Rig` for interlocking-only tests, and `run_until`/`count` for sim tests.
- Converter data: the real ts2-data layouts (GPL-2.0) plus a hand-made `mini.json`
  in `crates/ts2-import/tests/data/`. Facts about the TS2 format that the converter
  relies on are asserted by tests against these files.
