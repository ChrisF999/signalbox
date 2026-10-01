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
scripts/cargo test -p signalbox-game --test lessons -- --nocapture   # every lesson loads, draws and is played through
scripts/cargo test --release -p signalbox-game --test seed_timing -- --ignored --nocapture --test-threads 1   # seeding cost; the real WTT at 07:30
scripts/cargo test -p signalbox-server --features dev-auth --test client   # client-core over the real front
scripts/wasm-build                                            # the browser client into target/web-dist/ (tools image on first use)
deploy/browser-check.sh                                       # Chromium: WebGL2 fallback, WebGPU, no renderer (Docker, Playwright image)
scripts/ci/test.sh                                            # the CI gate (native cargo, offline, -D warnings; builds the web client when wasm is present, always on the runner via SIGNALBOX_REQUIRE_WASM=1)

scripts/cargo run -p sim-cli -- run crates/core/tests/fixtures/junction.json --robot --hours 1 --record /w/target/log.json
scripts/cargo run -p sim-cli -- replay crates/core/tests/fixtures/junction.json /w/target/log.json
scripts/cargo run -p ts2-import -- crates/ts2-import/tests/data/drain.json -o /w/target/drain.json
scripts/cargo run -p ts2-import -- crates/ts2-import/tests/data/liverpool-st.json -o /w/target/lst.json --areas /w/layouts/liverpool-st.areas.json --lines /w/layouts/liverpool-st.lines.json
scripts/cargo run -p ts2-import -- crates/ts2-import/tests/data/drain.json -o /w/target/drain.json --areas /w/layouts/drain.areas.json --lines /w/layouts/drain.lines.json --wtt /w/external/wtt/wtt.bbox.html   # the real WTT: external/wtt/README.md
scripts/cargo test --release -p ts2-import --test wtt_day -- --ignored --nocapture   # the real WTT, a whole day under the robot (skips without it)
```

Paths passed through `scripts/cargo` resolve inside the container (`/w` = repo root).

The stock Rust image has no wasm32 target: `scripts/wasm-build` runs
`scripts/build-web.sh` in `local/signalbox-wasm-tools:<wasm-bindgen version>`,
built on first use from the `wasm-tools` stage of `deploy/Dockerfile`. The
`wasm-bindgen` crate is pinned exactly and the CLI must be the same version;
bumping it means a new tools image and a new CI runner image (maintainer).
The tools image also has `brotli`: build-web.sh writes a `.br` and a `.gz`
beside every file it outputs and fails if either tool is missing (an older
local tools image: remove it and run `scripts/wasm-build` again).

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
expects red). `robot.rs` is a deterministic auto-signaller (it plans each
train's whole remaining journey in one Dijkstra search, booked platforms/lines
first, memoised per `Sim` by (entrance, service, call)); `robot::soak` runs a
world under it and is the integration oracle (no SPADs, collisions, invariant
violations or stuck trains). The robot sets a train's routes only all the way to its next
stop, or to a signal where it fouls no route from another signal, or
(`robot::may_stand`, polish spec P22) to an automatic signal on plain line
whose routes all end there.

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
  stops at nodes holding boundary signals (`ts2_import::areas`). The file also
  names the box (`prefix`, 1-3 capitals) and each area's `workstation` letter;
  both go into the world's client-only `layout` JSON (`box_prefix`,
  `workstations`), never into the sim. Optional line names come from
  `layouts/<name>.lines.json` (`ts2-import --lines`): `direction` there is the
  world's `up`/`down` (the direction of the line's signals), not the railway's.
- Drain's timetable can be the real Waterloo & City WTT
  (`ts2-import --wtt`, `ts2_import::wtt`, polish spec §4): it reads
  `pdftotext -bbox` output of the owner's PDF, checks it against the WTT's
  own figures (running times, workings, trains in service, intervals; a
  failure stops the conversion) and replaces Drain's services, entries,
  start time (05:40) and dwell (20–30 s): Wednesday, headcodes
  `<train>/<trip>` shown as the train number (display headcodes, below),
  the depot and siding both roads 5–7. The PDF and anything made from it
  are never committed (`external/wtt/`, git-ignored); CI tests only the
  synthetic `tests/data/wtt-synthetic.bbox.html` (written by
  `wtt-synthetic.py`, fictional trains 301–303).
  The image converts Drain with it when `external/wtt/` holds exactly one
  PDF (the Dockerfile's `wtt` stage runs `pdftotext -bbox`); otherwise
  Drain keeps its TS2 timetable.
- `game::display` reads those display keys once per game (defaults: the
  title's first letter, A, B, C... in area order) and builds each area's
  simplifier from the timetable; every `Layout` carries them.
- A service may have a display headcode (`ServiceFile.display`, absent =
  its headcode, so other worlds are written unchanged): the WTT's trips
  are `<train>/<trip>` but show the train number (polish spec P18,
  amended). The sim, the describer, saves and the wire keep the unique
  headcode; `Layout::display_headcodes` (only those that differ, whole
  world) lets the client show the display one wherever a headcode is shown
  (`client_core::Names::headcode`: berths, train list, simplifier, enquiry,
  hover, menus, the log). A berth holding a typed train number opens the
  enquiry of the one running train shown by it (`simplifier::resolve`).
- Clock votes (realism owner decision 12): holders vote; while nobody holds
  an area every connected player does (`Game::voters`), re-settled on every
  claim, release, grace expiry, connect and spectator disconnect.
- A vote lists who has still to agree (`VoteView.waiting`); any voter may
  Decline it (`vote_decline`; Withdrawn if they had agreed), ending it at
  once; Agree (`vote_agree`) only agrees to the proposal still open, never
  opening one. `flush` tells every player how each vote ended
  (`Notice::VoteEnded`), except a lone voter's proposal that applied at once
  (polish spec M8, U14).
- Deleting games (owner decision 13): lobby `delete_game`, saved or crashed
  games only, by the creator (meta row `creator`, written by the game
  process; save schema still 2) or a `SIGNALBOX_ADMINS` user (comma-separated
  usernames; the deploy compose file sets `skye`).
- `game::Game` is pure: `connect`/`handle`/`advance(real_dt)`/`flush` return
  `(player, ServerMsg)` pairs. It maps every command to its subject's area
  (`game::areas::AreaMap`), refuses commands outside the sender's area, and runs
  `robot::commands` every `ROBOT_EVERY_TICKS` for areas nobody holds.
- A refusal names the route in its way when the interlocking can say
  (`Notice::Rejected.by`, polish spec M4). Core finds it as it refuses:
  `Interlocking::conflict` is the one set of route checks (`check_set_route`
  is it without the route) and a points swing names the route holding them;
  `Event::CommandRejected.by` carries it (events are never stored, so no state
  or hash change). The client outlines that route's entrance too, if its
  layout lists the route (else the text says "another route").
- Views are built from sim state per player (`game::view::build_view`) — own area
  plus a fringe walked along the track to the first signal — and sent as deltas
  (`protocol::diff`, `View::apply`); a client that sees a `seq` gap resyncs.
- Wire commands carry names (`protocol::PlayerCommand`); the save logs core
  `Command`s with ids, since the world is copied into each save.
- Resume (`game::save::resume_sim`) restores the newest snapshot and replays the
  log rows after its `last_seq` up to the last logged tick, leaving
  that tick's commands queued (and the robot marked as run if it logged there).
- Late starts (timetables spec §3.4, P7/P8; `game::seed`): a game created
  with a start later than its world's is run there first by the robot for
  every area (`Seeding`; its runs logged as `seed`, one batch per run),
  built at `<save>.seeding`, snapshotted at the start, given meta `seed_to`
  and renamed into place; the world keeps its own start. An earlier start
  still rewrites `options.start_time`. The game process does it after the
  front connects (a `Status` with `preparing` every second, the front's
  frames held until ready) within 60 s real time (`process::SEED_BUDGET`;
  `--seed-budget-ms` for tests), else it exits 3 and the lobby gets
  `seed_too_slow`. Any other failure while preparing exits 4
  (`EXIT_NOT_PREPARED`, error prefix `preparing failed`); a stop (Shutdown,
  SIGTERM) exits 0 with no save. The front decides by the save alone: a
  create whose process is gone and left no save was never created, whatever
  its exit status (lost if the front had to kill it). It is not listed; the
  half-built save is removed and players get `not_created` (or
  `seed_too_slow`, also read from the last stderr line). The front sweeps stale `*.sqlite.seeding*` at startup;
  more than `MAX_HELD` (1000) front frames while preparing fail the create.
  The budget is per game: several big seeds at once share the CPU.
  Cost (release): Liverpool St 05:00→23:00 about 9 s (`--test seed_timing`).
- `Game::resume_with_layout(path, Some(current))` (polish spec §5) takes the
  world's display-only `layout` from the layout file as it is now when the
  saved network (`game::save::NETWORK_KEYS`) is identical, so old saves get
  today's prefixes, line names and drawing; in memory only, never written
  back, and the sim never reads it. Services, entries and options stay the save's.
- Saves are WAL with `synchronous=NORMAL` (a power cut may lose the last
  moments; the owner accepted that). Every command is logged before
  `sim.submit`; a robot run's commands are one transaction
  (`SaveDb::begin_batch`/`commit_batch`) committed before the sim steps
  with them, so a resume replays exactly what was committed. A run is
  saved whole or not at all (SQLite may roll a transaction back by itself
  on a full disk). Any command the log lost makes every tick end with a
  snapshot attempt until one succeeds (the first failure is reported once).
  Only if that snapshot cannot be written and the process then dies does a
  resume run the robot at the lost tick again, which matches what clients
  saw only if nobody held an area then (resumed games are unclaimed).

### Server (`ipc`, `server`)
- `signalbox-game` (`server::process`) wraps one `Game`: `Shell` is the sync
  logic (tested without sockets), `serve` the tokio loop (advance every 0.1 s,
  flush every 0.2 s, `Status` every 1 s). It accepts exactly one front
  connection on its socket, and exits 0 after `Shutdown`, SIGTERM, the front
  going away or 10 minutes empty (always saving first), 1 if the save will not
  open (its last stderr line is the crash reason the lobby shows). On a resume the front passes
  `--current-layout <world.json>` when it still lists the save's layout; the process logs whether it took
  that file's display data (only when the saved network matches it exactly; never written back).
- `ipc` frames are a u32 BE length plus JSON, at most 4 MiB; `read_frame` is not
  cancel-safe, so every socket is read by a task of its own.
- The front's `Supervisor` holds the lobby, the children and the routing behind
  one `std::sync::Mutex` never held across an `.await`. Each client socket has an
  `Outbox` (64 frames; on overflow it is cleared, the game is asked for a resync
  and deltas are dropped until the next full view). A second login of a name
  replaces the old socket without a `Disconnect` (C1's contract on
  `Game::connect`). A child that ends other than by exiting 0 is `crashed`.
- The front sends the games list to every client in the lobby when a game's holders or
  connected players change (`broadcast_lobby_games`, polish spec M9); never for a tutorial, and not at all
  when nobody is in the lobby (no save is read then). A game starting or stopping being prepared still goes to
  everyone (`broadcast_games`). Every pushed list is built on a blocking thread (`push_games`; the front's runtime
  is single-threaded, so a save scan inline would stall every game): one build at a time, and whatever asks
  meanwhile shares one more; a build that panics clears the flag and still starts the one asked for meanwhile.
  Tests: `slow_games_scans`, `panic_next_games_scan`, `games_pushes_settled`. The client's Leave releases a held
  area first (not in a tutorial).
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
  the front. Names are refused when they are `robot` or `seed` (any case: `is_reserved`). `/` redirects (303) to
  `/auth/login`, `/ws` and the web client's files under `/app/` are 401
  without a session, `/auth/dev` is 404 in the release build, `/auth/login`
  is 303 to the provider or 503 if it is unreachable.
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
- `deploy/browser-check.sh` proves the renderers in a real browser: plain headless Chromium has the WebGPU API but no adapter (the WebGL2 fallback), `--enable-unsafe-webgpu` gives WebGPU, `--disable-webgl` the explanation page; it puts the page into a game by sending `create_game` on the page's own socket (Playwright `route_web_socket`), and fails if the wasm was not served compressed.
- Diagram geometry is `game::geometry::WorldGeometry`, read once from the
  world's `layout` (ts2-import writes it); points legs, signal facings and
  exit positions are found by walking up to 4 nodes to a drawn line. The
  train list is `game::view::build_trains`, from sim state only.
- The front serves `SIGNALBOX_WEB` from memory (`server::assets`) behind the
  session, the `.br` or `.gz` copy when the browser accepts it (brotli first;
  `Vary: Accept-Encoding`, an ETag per copy; copies are never served by
  their own names); without that directory `/` is the placeholder page (no web client
  installed).
- The workspace `rand` has no default features (getrandom does not build for
  wasm32-unknown-unknown); the server turns on `thread_rng`.
- The look follows `docs/superpowers/specs/2026-10-01-panel-realism-design.md`
  (IECC conventions): signals are shown as `<box><workstation><number>`
  (`client_core::Names`; display only, wire names stay plain). Points are shown the same way
  with a `P` (`LAP153`), berths by their signal, track only by the platform on it (polish spec M1); place codes by the
  names ts2-import writes into `layout.places` (M2): tables keep codes and show names on hover. Settings
  (aspects red/green or real, headcode enquiry, signal numbers) live behind
  `client_core::SettingsStore`, which `client-web` backs with `localStorage`
  (`LocalStore`). Nothing flashes except points moving, the selected entrance
  and a cancelling route's lamp; the tutorial highlight is a calm pulse.
  Arrows and the ○A button are shapes: egui's default fonts have no arrow
  glyphs.
- A new game's creator may choose an area in the lobby ("Signal", default watch); the client claims it when the
  game's first layout comes (`App::create_game_in`, polish spec H2). With a late start that layout comes only when
  the game is ready, so nothing is claimed while it is "Preparing"; a game that is not created leaves no claim.
  A click that chooses nothing logs why, once (`select::why_not_entrance`, `InGame::log_once`).
- ○A (spec decision 6, amended) sits beside a controlled signal the player
  works and makes a set route stay set for following trains (real
  auto-working), not beside permanently automatic signals; there is none on
  fringe signals, and spectators see it grey and read-only.
- Legibility (polish spec §3): `paint::draw` emits every text at its own
  spot and records which may move (`labels::Movable`: role, other spots) and
  what must stay clear (`labels::KeepClear`: track bars, lamps, ○A circles,
  every berth box, exits); `labels::plan` places them greedily in priority
  order (own numbers, ○A letters, line names, platform numbers, labels,
  fringe numbers) and hides what has no room; headcodes never move. Plans
  depend only on scene, zoom and settings, and `UiApp` caches one per settled zoom (it replans every frame
  while zooming).
  ○A buttons exist (drawn and hit) only where numbers are drawn.
  `tests/legibility.rs` is the acceptance measurement (`--nocapture` prints it).
- `hit_test` takes the view as well as the scene; points and exits win over
  an empty berth under the pointer. The side panel starts as wide as the simplifier for the layout's longest
  displayed headcode (polish spec H3).
- The diagram shows a pointing hand over what you can work and ends hover text with what a click does
  (`App::hint`); a left click on your points opens their menu and never swings them (polish spec M3, U9).
- The headcode enquiry window opens 16 px right of and below the pointer (egui keeps it on screen), with a labelled
  State/Next/Runs grid and headed call rows; `Enquiry::next_text` says `depart|arrive|pass <place> <platform> at <time>`
  (polish spec M7, U13).  Every headcode click places it (`current_pos`, one frame; draggable after).
- The train list is headed (Train, State, Next, Arr, Dep, Late) and writes lateness as
  the simplifier does, `OT`/`3L` (polish spec M6).
- The top bar never reflows (polish spec M5): buttons right-aligned in a fixed order, fixed-width clock controls, the vote on
  the second row; Release area asks first (M10).
- The side panel can be hidden (top bar: Hide panel / Show panel) or dragged to 240 pt, where the simplifier
  scrolls sideways, header and all; an untouched Fit follows the window's size, a view the player has panned or
  zoomed is left alone (polish spec H7).
- Zoom: ×1.2 a wheel notch, + and − buttons and keys ×1.25 (they count as moving the view; Fit clears that); signal
  glyphs grow with the zoom once the track is at its widest, up to twice their size (`paint::glyph`, a function of
  the zoom alone so placement never jumps; polish spec M11).
- Fit shows a player's area at no less than 0.55 px per unit, round its busiest station when it is too long
  (`Scene::fit_camera`, polish spec M13).
- Points show which way they lie (polish spec M12, U17): the non-lying leg, and a crossover's middle while no end lies
  over it (`paint::unused_crossovers`), are drawn at `UNUSED_W` (0.4) of the current track width; the leg moving points
  swing to blinks its first half at 2 Hz (only moving points flash). Labels still keep clear of the full width.
- The lobby (polish spec M14, M15): who you are and Sign out (`UiApp::wants_logout`, followed by `client-web`), layouts
  by title with the areas file's `description` (the front reads only area names, title and `layout.description`,
  leniently: a malformed one is shown empty), By and Last played, and a form checked as typed (`client_core::form`).

### Tutorials (`lessons/`, `game::lesson`)
- Design: `docs/superpowers/specs/2026-10-01-tutorial-design.md`; decisions in
  `docs/superpowers/plans/2026-10-01-tutorial.md`. Each `lessons/<id>/` holds a
  hand-made `world.json` (core format, drawn like a converted layout, all trains
  `on_demand` entries) and a `lesson.json` (steps: `say`, `highlight`, `do`,
  `wait_for`, `solution`; `deny_unknown_fields`).
- `game::lesson::check` validates a lesson against its world (the front at
  startup leaves a broken one out with one log line; `SIGNALBOX_LESSONS`,
  default `/opt/signalbox/lessons`). `game::lesson::Runner` runs one over a
  `Game`: a `(GameSnapshot, seen)` at each step's start, then its actions; the
  condition is checked after every tick (`Game::advance_with`) and every player
  message; Restart step restores the snapshot. The clock runs only while the
  lesson's player is connected.
- `signalbox-game --lesson <dir>` runs a tutorial with no save at all. The
  front keeps tutorials private (`Entry.owner`: never listed, others get
  `unknown_game`), caps them separately (`MAX_TUTORIALS`, transiently +1 while
  a user replaces their own tutorial), starting anything ends the user's other
  tutorials, ends one when its player leaves, and gives a dropped socket 60 s
  to reconnect and rejoin (a page reload lands in the lobby, where tutorials
  aren't listed, so the player starts again).
- `Sim::offer_entry` (on-demand entries) and `Event::SignalPassed` exist for
  lessons; neither is logged state, and no converted world uses them.
- Changing a lesson: `crates/game/tests/lessons.rs` plays every lesson through
  with a scripted player (its `solution`, else what `wait_for` asks for, robot
  routing for train steps); keep texts naming signals as the client shows them.
- Client: the tutorial list's ticks live behind a second store
  (`client_core::lessons::LESSONS_KEY`, `UiApp::with_stores`; `client-web`
  gives each its own `LocalStore::new(key)`). The lesson highlight is UI, not
  panel state.
- A step may carry a `done` text (polish spec H5): once its task is done it says so and waits for Next; the CI
  play-through presses Next there. The lesson box's buttons sit above the text and never move (Next alone on the
  left, greyed while the step waits for something else; the rest on the right); Enter is Next whenever Next is
  enabled and no text field has focus (H4). Real aspects is never forced: texts say "a proceed aspect", and lesson 1
  shows it before the train, with S3 set to S5 (H6).

### Tests
- Core fixtures: `crates/core/tests/fixtures/{plain_line,terminus,junction}.json`.
  `tests/common/mod.rs` has `load_with(name, |json| ...)` for mutating a fixture
  per test, `Rig` for interlocking-only tests, and `run_until`/`count` for sim tests.
- Converter data: the real ts2-data layouts (GPL-2.0) plus a hand-made `mini.json`
  in `crates/ts2-import/tests/data/`. Facts about the TS2 format that the converter
  relies on are asserted by tests against these files.
