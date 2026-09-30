# signalbox D1 — browser client — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A signaller's screen in the browser for the C server: Rust compiled to WASM, egui/eframe on wgpu (WebGPU, falling back to WebGL2), served by the front at `/`, with a modern UK VDU diagram, route setting by clicks, the lobby, the train list, alarms, clock votes and automatic reconnect — plus the protocol and game additions it needs (diagram geometry and a train list in every view).

**Architecture:** Three new crates. `client-core` (lib `client_core`) is all client logic behind a `Transport` trait — the connection state machine with backoff, the lobby, the game you are in (layout and view kept by the existing `bot::Bot`), the alarm log, and what clicks mean — pure, tested natively against an in-memory transport and an in-process `Game`. `client-ui` (lib `client_ui`) draws it with `egui` alone (no eframe, no GPU): the diagram scene, camera, hit-testing and painter are pure functions tested headless, and the screens run in headless `egui::Context::run_ui` tests. `client-web` (cdylib, wasm32 only) is the browser shell: an eframe `WebRunner` with wgpu, a `web-sys` WebSocket transport, the 401 → `/auth/login` hand-off and a no-GPU fallback page. The front serves the built files from `SIGNALBOX_WEB`.

**Tech Stack:** Rust 1.98 (edition 2024) via `scripts/cargo` (Docker). New: `egui` 0.36.2 and `eframe` 0.36.2 (`wgpu` feature; brings `egui-wgpu` 0.36.2 and `wgpu` 30.0.1), `wasm-bindgen` =0.2.129 with the same `wasm-bindgen-cli` 0.2.129, `wasm-bindgen-futures` 0.4.79, `web-sys` 0.3.106. The wasm target and the CLI live in a `wasm-tools` stage of `deploy/Dockerfile`, used by `scripts/wasm-build` locally, by the release image build, and (after the controller updates it) by the CI runner image.

**Spec:** `docs/superpowers/specs/2026-09-30-browser-client-design.md` (D1). Server spec: `docs/superpowers/specs/2026-09-30-server-and-protocol-design.md`; C1/C2 plans in `docs/superpowers/plans/` (conventions).

### Decisions (controller brief 2026-09-30 plus this plan's own)

1. **Crates.** `signalbox-client-core` (lib `client_core`; native + wasm32; no egui, no web-sys), `signalbox-client-ui` (lib `client_ui`; depends on `egui` only, not `eframe`, so its native tests need no windowing or GPU crates), `signalbox-client-web` (cdylib + rlib; every dependency and all code are behind `cfg(target_arch = "wasm32")`, so the native workspace build compiles it as an empty crate and CI's native build and cache are unaffected by eframe/wgpu). No `client-desktop` (D2).
2. **Versions (checked 2026-09-30 on crates.io, compiled in scratch):** `egui`/`eframe` 0.36.2 (2026-09-08, `rust-version` 1.95 ≤ 1.98) with `eframe` `default-features = false, features = ["wgpu", "default_fonts"]` (no accesskit, no winit on wasm, no glow); it pulls `egui-wgpu` 0.36.2 whose default features include `wgpu/webgl`, and `wgpu` 30.0.1. `wasm-bindgen = "=0.2.129"` (pinned exactly; `wasm-bindgen-cli --version 0.2.129` installs with `--locked` on `rust:1.98-slim-bookworm`), `wasm-bindgen-futures = "0.4.79"`, `web-sys = "0.3.106"`. The build script refuses to run if the CLI's version differs from the `wasm-bindgen` version in `Cargo.lock`.
3. **WebGPU first, WebGL2 fallback** is egui-wgpu's own web behaviour ("By default on web, WebGPU is preferred with WebGL as a fallback (requires the `webgl` feature of crate `wgpu`)", egui-wgpu 0.36.2 `setup.rs`); `client-web` also sets `backends = BROWSER_WEBGPU | GL` explicitly so it is visible in our code. WebGPU is only offered in secure contexts (HTTPS or localhost; egui-wgpu drops it otherwise), which the tailnet URL is. If eframe cannot start at all, the page shows a plain explanation (`#fallback`).
4. **getrandom.** `signalbox-core` pulled `getrandom` 0.3 through `rand`'s default `os_rng`/`thread_rng` features, and getrandom 0.3 does not compile for `wasm32-unknown-unknown` without a cfg flag (verified: `compile_error!` "The wasm32-unknown-unknown targets are not supported by default"). The workspace `rand` becomes `default-features = false, features = ["std"]` (the sim only uses `Rng`/`SeedableRng` with `ChaCha8Rng`), and `signalbox-server` adds `features = ["thread_rng"]` for `rand::random`. No behaviour change; determinism untouched.
5. **`bot` without tokio.** `signalbox-bot` gains a default feature `net` gating `net` and `play` (tokio, tokio-tungstenite, futures-util, thiserror become optional); `client-core` depends on it with `default-features = false` and reuses `bot::Bot` as the spec says. `Bot::request_resync()` is added for unreadable frames.
6. **Geometry (spec §4.1), refined.** `Layout.geometry: Option<Geometry>` with `lines`, `points`, `signals`, `platforms`, `labels` as the spec lists, plus: points carry their three leg end positions (`toe`, `normal`, `reverse`: where the next drawn line starts, found by walking the track up to 4 nodes — TS2 points legs are undrawn 1 m spacers), signals carry `facing` (the direction of travel past them, from their segment's line or the nearest line in rear), and `nodes` gives positions for route exits at buffer stops/boundaries and for boundary berths (on Liverpool Street 22 of 22 exit nodes are only reachable this way; checked in scratch). Built once per game (`game::geometry::WorldGeometry`) and filtered per player. Labels are kept if within 40 units of the visible drawing's bounding box. A world whose `layout` is absent or unreadable gives `None`.
7. **Trains (spec §4.2), refined.** `View.trains: BTreeMap<headcode, TrainRow>`, diffed sparsely like `berths`. `TrainRow { next_place, next_platform, booked, late_s, state }`, `booked` = the next call's arrival else departure; `late_s` = whole minutes late now against `booked`, as seconds, never negative (minutes, so a late train changes its row once a sim minute, not every tick). `state`: `at_platform` (dwelling) > `in_area` (head in your area; always for spectators) > `approaching` (running elsewhere); `due` = not yet on the railway. Rows: running trains on your visible track, or whose next call is at a platform in your area; entries waiting at, or due within 30 sim minutes at, a boundary (or start position) in your area. Spectators: every running train and every entry due within the window. A headcode already listed keeps its first row. Core gains read-only `Sim::pending_entries()` and `Sim::next_entry()`.
8. **Reconnect.** Backoff 0.5 s doubling to a 10 s cap; the attempt counter resets only when a connection carries a frame (a front that accepts then drops keeps backing off). On reconnect the client sends exactly one `join` for its game — the `joined`, layout and full view that follow are the one resync; it never sends `resync` for a reconnect. `notice replaced` (another tab) stops reconnecting until the user asks. A failed rejoin (`error`) returns to the lobby with the reason.
9. **401 detection.** Browsers do not expose the HTTP status of a refused WebSocket upgrade, so when a socket closes without ever opening, the web transport does a plain `GET /ws`: the front answers 401 before looking at the upgrade when there is no session (C2), and something else (400/426) when there is. 401 → `ConnState::Unauthorized` → the app asks the shell to navigate to `/auth/login`.
10. **Controls (spec §3.2).** Left-click signal = entrance (only signals with an operable route from them), then a lit exit (signal or exit marker) sends `set_route`; the entrance again, Esc, or a dead click clears; another entrance takes over. Right-click menus: signal (cancel route if one is set from it; auto-working on/off for an active automatic route), points (swing to the other position), berth (cancel the headcode; interpose box when the berth is yours). Refusals flash the entrance for 2 s and log an alarm. Hover text for everything; fringe and spectators get hover only.
11. **Serving.** The front reads `SIGNALBOX_WEB` (default `/opt/signalbox/web`) once at start: `index.html` plus the files of `app/` whose names are `[A-Za-z0-9_.-]+`, into memory, with a content type by extension and a strong ETag. `/` with a session serves `index.html` (without a session: 303 to `/auth/login`, unchanged); `/app/{file}` needs a session too (401 otherwise, 404 for unknown names), answers 304 to a matching `If-None-Match`, and sends `Cache-Control: no-cache`. A missing directory keeps C2's placeholder page (tests and dev builds). No request ever touches the filesystem.
12. **Build.** `scripts/build-web.sh <out>` (runs wherever the wasm32 target and the CLI are: the tools image, the release Docker stage, the CI runner): `cargo build -p signalbox-client-web --target wasm32-unknown-unknown --profile web --locked`, then `wasm-bindgen --target web --no-typescript --out-name signalbox_web`, then copies `index.html`; output `<out>/index.html`, `<out>/app/signalbox_web.js`, `<out>/app/signalbox_web_bg.wasm`. `scripts/wasm-build` runs it in `local/signalbox-wasm-tools:<wasm-bindgen version>` (built on first use from the `wasm-tools` stage of `deploy/Dockerfile`). Profile `web` inherits `release` with `opt-level = "s"`, `lto = true`, `codegen-units = 1`, `panic = "abort"` (sizes in Task 7).
13. **End to end (native, CI).** In `crates/server/tests/client.rs` (feature `dev-auth`): a test-only `Transport` over `bot::net::Conn` on a tokio task drives a real `client_core::App` against the real front (C2's `tests/common` harness) on Liverpool Street: log in, create, claim, click entrance then exit, see the route set; a dropped socket rejoins with one `join`; no session → `Unauthorized`.
14. **CI.** `scripts/ci/test.sh` also builds `signalbox-bot` without default features, and builds the web client for wasm32 when the runner has the target and `wasm-bindgen` — required (not skipped) when `SIGNALBOX_REQUIRE_WASM=1`, which the controller sets in the updated runner image. Until then CI prints a loud `SKIP`.

### Deferred (deliberately not in D1)

- **Bundle size.** The wasm is 8.0 MB (2.85 MB gzipped; JS glue 150 KB) with `opt-level = "s"` and fat LTO; `opt-level = "z"` measured larger (8.9 MB). No wasm-opt/binaryen pass, no precompressed gzip/brotli, no hashed file names with long-lived caching: `no-cache` plus the ETag means a reload costs one 304 per file, a new build one full download. Revisit if first loads over the tailnet feel slow.
- **A menu fallback for routes to undrawn exits.** The geometry search finds every route exit on the three shipped layouts (tested for Liverpool Street), so exits are always clickable markers; there is no "Route to …" menu entry.
- Spec §7's out-of-scope list stands: exact train positions, touch/mobile, sounds, chat, layout editing, accessibility beyond Esc and hover text (eframe's accesskit feature is off), and the desktop client and its login (D2).
- **A browser in CI.** Headless Chromium needs the Playwright image and Docker; the runner has neither by design. The browser check is the controller's (C4).

## Global Constraints

- License: GPL-2.0-or-later on every new crate (`license.workspace = true`).
- Every cargo command runs through `scripts/cargo` from the repo root (Docker, `rust:1.98-slim-bookworm`, repo at `/w`, network available, no environment forwarded). wasm32 builds run through `scripts/wasm-build` (Task 7). Paths inside the containers are under `/w`.
- CI (`scripts/ci/test.sh`) builds with `-D warnings --locked --offline`: no unused imports, variables or dead code in any configuration it builds (workspace default features; `signalbox-server --features dev-auth`; `signalbox-bot --no-default-features`; the wasm32 web build). Deprecated egui/eframe APIs are warnings, hence errors: use the 0.36 names shown in this plan (`Panel::top(..).show(ui, ..)`, `CentralPanel::default().show(ui, ..)`, `Context::run_ui`, `App::ui`). Commit `Cargo.lock` with every dependency change.
- **After any task that changes `Cargo.lock`, the controller (not the subagent) reseeds the CI cache before anything is pushed:** `scripts/cargo fetch --locked` then `/opt/stack/apps/signalbox-runner/seed-cache.sh`. Subagents never push.
- Determinism rules stay for `core`, `game` and `protocol`: `BTreeMap`/`BTreeSet`/`Vec` only, no wall clock in `Game`. Geometry and train rows are built from world and sim state only.
- Nothing received may panic the client or the front: every `ServerFrame` is parsed with `ServerFrame::from_json` and errors handled; geometry may be missing or partial; names are opaque strings (e.g. `Hackney & Bow`, `39,1V1`, `512#113`).
- `client-core` and `client-ui` never touch the browser, the clock or the network: the shell passes time in (`now` seconds) and a `Transport`.
- Keep dependencies minimal: the workspace adds exactly `egui`, `eframe`, `wasm-bindgen`, `wasm-bindgen-futures`, `web-sys` (plus their trees). No `tower-http`, no JS toolchain, no npm, no `console_error_panic_hook`, no `log` crate (eframe's `WebRunner` installs its own panic handler).
- Crate names: `signalbox-client-core` (lib `client_core`), `signalbox-client-ui` (lib `client_ui`), `signalbox-client-web` (lib `signalbox_web`, cdylib).
- Numbers: backoff 0.5 s doubling, cap 10 s; refused-command flash 2 s; alarm log 200 lines; due window 30 sim minutes; label margin 40 units; leg/facing/exit search depth 4 nodes; hit radius 8 px; zoom scale clamped to [0.02, 50] px per unit; berth box 34 × 14 px.
- Colours (spec §3.1): background `#0B0B0F`; track free `#6E6E6E`, route `#EBEBEB`, overlap `#A0A0A0`, occupied `#E62828`; aspects red `#E61E1E`, yellow `#FAD200`, green `#00DC50`; headcode `#FAD200`; empty berth outline `#464646`; selection / lit exits `#00C8FF`; refused flash `#FF3CFF`; platforms `#23234A`; labels `#9696AA`. Fringe elements at half brightness.
- Infra actions (the CI runner image and its cache, `/opt/stack`, compose projects and long-running containers on ra, `tailscale serve`, Authentik) are controller-only and owner-gated: they are in the final "Controller" section, never in a subagent task. Subagents may run `scripts/cargo`, `scripts/wasm-build` (which may build the local image `local/signalbox-wasm-tools:<version>`), and in Task 10 only build a throwaway image and run it with `--rm` on `127.0.0.1:19160`; they touch no existing container, volume, network or compose project, and remove any image they tag other than the wasm-tools one.

## Review Focus

1. **A page reload or a second tab while in a game.** The front's `attach` hands the new socket the old one's game and pushes `joined` before the client asks for anything; the client must enter the game from that unasked `joined`, and the old tab (which gets `notice replaced`) must stop reconnecting rather than fight for the login. Pinned in Task 3 (`an_unasked_joined_enters_the_game`, `replaced_by_another_tab_stays_down_until_asked`).
2. **A layout with no geometry or with gaps** (hand-made worlds, a signal the drawing lacks, points with a leg the search could not find, an exit with no position): the lobby, train list and alarms still work, the diagram draws what it has and says "No diagram for this layout" when there is none, and nothing panics. Pinned in Task 5 (`a_scene_skips_what_the_drawing_lacks`) and Task 6 (`a_layout_without_geometry_says_so`).
3. **Degenerate fits and zoom extremes** — one point, all lines on one spot, coordinates in the tens of thousands, a zero-size or negative screen rectangle, a hundred wheel clicks: the camera never produces NaN or infinity and zoom stays within its clamp. Pinned in Task 5 (`fit_survives_degenerate_bounds`, `zoom_is_clamped`).
4. **Asset paths and a missing build** — `/app/../Cargo.toml`, `/app/%2e%2e%2fx`, `/app/` and `/app/sub/dir.js` must be 404 (400 where the path does not even decode) without reading any file; no `SIGNALBOX_WEB` directory must keep C2's placeholder page and `/app/*` 404. Pinned in Task 8 (`assets_are_only_the_listed_names`, `without_a_web_dir_the_placeholder_stays`).
5. **A session that expires while the tab is open.** The socket keeps working until it drops; the reconnect is refused and the client must go to `/auth/login` rather than retry forever, which depends on `GET /ws` answering 401 without a session and not 401 with one. Pinned in Task 8 (`ws_without_an_upgrade_is_401_only_without_a_session`) and Task 9 (`without_a_session_the_client_asks_for_a_login`).

---

## File Structure

```
Cargo.toml                                        (T3) members += client-core, rand without default features; (T5) client-ui, egui; (T7) client-web, eframe, wasm-bindgen*, web-sys, [profile.web]
crates/protocol/src/view.rs                       (T1) Geometry + parts, Layout.geometry, TrainRow, TrainState, View.trains, Delta.trains
crates/protocol/src/diff.rs                       (T1) trains in diff and apply
crates/protocol/tests/golden.rs, tests/diff.rs    (T1)
crates/bot/tests/{bot,strategy,play}.rs           (T1) new fields in literals; (T3) request_resync test in bot.rs
crates/server/tests/supervisor.rs                 (T1) new field in the `view` literal
crates/core/src/sim.rs                            (T2) pending_entries, next_entry
crates/core/tests/timetable.rs                    (T2)
crates/game/Cargo.toml                            (T2) + serde
crates/game/src/geometry.rs                       (T2) WorldGeometry, LABEL_MARGIN
crates/game/src/{lib,layout,game,view}.rs         (T2) geometry wiring; build_trains, late_s, DUE_WINDOW_S
crates/game/tests/fixtures/twobox-layout.json     (T2) a drawing of twobox (also used by client-ui tests)
crates/game/tests/common/mod.rs                   (T2) TWOBOX_LAYOUT
crates/game/tests/{geometry,trains}.rs            (T2) new; tests/{layout,game}.rs updated
crates/bot/tests/soak.rs                          (T2) trains and geometry in the consistency soak
crates/bot/Cargo.toml, src/lib.rs                 (T3) feature `net`; Bot::request_resync
crates/bot/src/net.rs                             (T8) http_get_with (request headers, for the front's tests)
crates/server/Cargo.toml                          (T3) rand + thread_rng; (T9) dev-dep client-core, [[test]] client
crates/client-core/Cargo.toml                     (T3)
crates/client-core/src/lib.rs                     (T3; T4 adds modules)
crates/client-core/src/transport.rs               (T3) ConnState, Transport, MemTransport, MemHandle
crates/client-core/src/app.rs                     (T3) App, InGame, Link, backoff_s, entrance_of
crates/client-core/src/log.rs                     (T3) Log, LogEntry, LOG_CAP
crates/client-core/src/text.rs                    (T3) fmt_hms, command_text, notice_text, vote_text, ...
crates/client-core/src/select.rs                  (T4) click, exits_from, menus, describe_*
crates/client-core/src/input.rs                   (T4) Target; App::click/escape/valid_exits/menu/interpose/describe
crates/client-core/src/trains.rs                  (T4) train_list
crates/client-core/tests/common/mod.rs            (T4) Table: the app against an in-process twobox Game
crates/client-core/tests/{app,text}.rs            (T3)
crates/client-core/tests/input.rs                 (T4)
crates/client-ui/Cargo.toml, src/lib.rs           (T5)
crates/client-ui/src/scene.rs                     (T5) Scene from a Layout
crates/client-ui/src/camera.rs                    (T5) Camera: fit, zoom, pan
crates/client-ui/src/hit.rs                       (T5) hit_test
crates/client-ui/src/paint.rs                     (T5) colours, paint
crates/client-ui/src/screens.rs                   (T6) UiApp: lobby, top bar, diagram, trains, alarms, menus
crates/client-ui/tests/common/mod.rs              (T5) drawn twobox layouts and views
crates/client-ui/tests/{scene,camera,hit,paint}.rs (T5)
crates/client-ui/tests/screens.rs                 (T6)
crates/client-web/Cargo.toml, index.html          (T7)
crates/client-web/src/{lib,transport}.rs          (T7) wasm32 only
deploy/Dockerfile                                 (T7) wasm-tools stage; (T10) web stage, /opt/signalbox/web
scripts/build-web.sh, scripts/wasm-build          (T7)
crates/server/src/assets.rs                       (T8) WebAssets
crates/server/src/{config,web,lib}.rs             (T8) web_dir, routes, AppState.web
crates/server/tests/{units,front,release,oidc}.rs, tests/common/mod.rs (T8)
crates/server/tests/client.rs                     (T9) client-core end to end over the real front
scripts/ci/test.sh                                (T3) bot without net; (T10) wasm build
deploy/README.md, deploy/smoke.sh, CLAUDE.md      (T10)
```

---
### Task 1: `protocol` — diagram geometry and the train list on the wire

**Files:**
- Modify: `crates/protocol/src/view.rs` (new types; `Layout.geometry`; `View.trains`; `Delta.trains`)
- Modify: `crates/protocol/src/diff.rs` (trains in `diff` and `View::apply`)
- Modify: `crates/game/src/layout.rs`, `crates/game/src/view.rs` (fill the new fields with `None` / empty for now; Task 2 fills them)
- Modify (struct literals only): `crates/bot/tests/bot.rs`, `crates/bot/tests/strategy.rs`, `crates/bot/tests/play.rs`, `crates/server/tests/supervisor.rs`
- Test: `crates/protocol/tests/golden.rs`, `crates/protocol/tests/diff.rs`

**Interfaces:**
- Consumes: C1's `Layout`, `View`, `Delta`, `diff`, `View::apply`.
- Produces (all `pub` in `protocol`, re-exported by `pub use view::*`):
  - `Layout.geometry: Option<Geometry>` (`#[serde(default)]`: a layout without the field reads as `None`).
  - `struct Geometry { lines: Vec<LineGeom>, points: Vec<PointsGeom>, signals: Vec<SignalGeom>, platforms: Vec<PlatformGeom>, labels: Vec<LabelGeom>, nodes: Vec<NodeGeom> }` (`Clone, Debug, Default, PartialEq, Serialize, Deserialize`).
  - `LineGeom { segment: String, x1: f64, y1: f64, x2: f64, y2: f64 }` — (x1, y1) is the segment's `from` node.
  - `PointsGeom { node: String, x: f64, y: f64, toe: Option<[f64; 2]>, normal: Option<[f64; 2]>, reverse: Option<[f64; 2]> }`.
  - `SignalGeom { signal: String, x: f64, y: f64, berth_x: f64, berth_y: f64, facing: Option<[f64; 2]> }`.
  - `PlatformGeom { place: String, platform: String, x1: f64, y1: f64, x2: f64, y2: f64 }`, `LabelGeom { text: String, x: f64, y: f64 }`, `NodeGeom { node: String, x: f64, y: f64 }`.
  - `View.trains: BTreeMap<String, TrainRow>` (`#[serde(default)]`), `Delta.trains: BTreeMap<String, Option<TrainRow>>` (absent when empty; `null` = gone).
  - `enum TrainState { Due, Approaching, InArea, AtPlatform }` (`Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord`, snake_case on the wire; the order is the order a train goes through them).
  - `struct TrainRow { next_place: Option<String>, next_platform: Option<String>, booked: Option<f64>, late_s: i64, state: TrainState }` (`Clone, Debug, PartialEq`).

- [ ] **Step 1: Write the failing tests**

In `crates/protocol/tests/golden.rs`, add `geometry: None,` as the last field of the `Layout` literal in `small_layout()`, and in `fn layout()` change the end of the expected JSON from
```rust
            "routes": [{"name": "A-E", "entrance": "A", "exit": {"kind": "node", "name": "E"},
                        "automatic": false, "operable": true}]
        }),
```
to
```rust
            "routes": [{"name": "A-E", "entrance": "A", "exit": {"kind": "node", "name": "E"},
                        "automatic": false, "operable": true}],
            "geometry": null
        }),
```
Then add these two tests after `fn layout()`:
```rust
#[test]
fn layout_geometry() {
    let mut l = small_layout();
    l.geometry = Some(Geometry {
        lines: vec![LineGeom { segment: s("pa"), x1: 10.0, y1: 625.0, x2: 205.5, y2: 625.0 }],
        points: vec![PointsGeom {
            node: s("P"),
            x: 220.0,
            y: 625.0,
            toe: Some([215.0, 625.0]),
            normal: Some([225.0, 625.0]),
            reverse: None,
        }],
        signals: vec![SignalGeom {
            signal: s("A"),
            x: 300.0,
            y: 25.0,
            berth_x: 260.0,
            berth_y: 30.0,
            facing: Some([1.0, 0.0]),
        }],
        platforms: vec![PlatformGeom { place: s("EST"), platform: s("1"), x1: 0.0, y1: 0.0, x2: 300.0, y2: 15.0 }],
        labels: vec![LabelGeom { text: s("Hackney & Bow"), x: -20.0, y: 615.0 }],
        nodes: vec![NodeGeom { node: s("E"), x: 400.0, y: 0.0 }],
    });
    let want = json!({
        "lines": [{"segment": "pa", "x1": 10.0, "y1": 625.0, "x2": 205.5, "y2": 625.0}],
        "points": [{"node": "P", "x": 220.0, "y": 625.0, "toe": [215.0, 625.0], "normal": [225.0, 625.0], "reverse": null}],
        "signals": [{"signal": "A", "x": 300.0, "y": 25.0, "berth_x": 260.0, "berth_y": 30.0, "facing": [1.0, 0.0]}],
        "platforms": [{"place": "EST", "platform": "1", "x1": 0.0, "y1": 0.0, "x2": 300.0, "y2": 15.0}],
        "labels": [{"text": "Hackney & Bow", "x": -20.0, "y": 615.0}],
        "nodes": [{"node": "E", "x": 400.0, "y": 0.0}]
    });
    let json = serde_json::to_value(ServerMsg::Layout(l.clone())).unwrap();
    assert_eq!(json["geometry"], want);
    let back: ServerMsg = serde_json::from_value(json).unwrap();
    assert_eq!(back, ServerMsg::Layout(l));
}

#[test]
fn a_layout_or_view_from_before_d1_still_reads() {
    let mut json = serde_json::to_value(ServerMsg::Layout(small_layout())).unwrap();
    json.as_object_mut().unwrap().remove("geometry");
    let ServerMsg::Layout(l) = serde_json::from_value(json).unwrap() else { panic!() };
    assert_eq!(l.geometry, None);
    let view = json!({
        "type": "view", "seq": 1, "sim_time": 0.0, "speed": 1, "paused": false, "vote": null,
        "holders": {}, "score": null, "signals": {}, "routes": {}, "points": {}, "sections": {}, "berths": {}
    });
    let ServerMsg::View(v) = serde_json::from_value(view).unwrap() else { panic!() };
    assert!(v.trains.is_empty());
}
```
In `fn view()`, add after `berths: BTreeMap::from([(s("BA"), s("1E01"))]),`:
```rust
        trains: BTreeMap::from([
            (
                s("1E01"),
                TrainRow {
                    next_place: Some(s("EST")),
                    next_platform: Some(s("1")),
                    booked: Some(25500.0),
                    late_s: 120,
                    state: TrainState::InArea,
                },
            ),
            (
                s("2W03"),
                TrainRow { next_place: None, next_platform: None, booked: None, late_s: 0, state: TrainState::Due },
            ),
        ]),
```
and replace its expected `"berths": {"BA": "1E01"}` line with:
```rust
            "berths": {"BA": "1E01"},
            "trains": {
                "1E01": {"next_place": "EST", "next_platform": "1", "booked": 25500.0, "late_s": 120, "state": "in_area"},
                "2W03": {"next_place": null, "next_platform": null, "booked": null, "late_s": 0, "state": "due"}
            }
```
In `delta_sends_only_changes_and_null_for_cleared`, add to the `Delta` literal after its `berths` line:
```rust
        trains: BTreeMap::from([
            (s("1E01"), None),
            (
                s("2W03"),
                Some(TrainRow {
                    next_place: Some(s("WST")),
                    next_platform: None,
                    booked: Some(26100.0),
                    late_s: 0,
                    state: TrainState::AtPlatform,
                }),
            ),
        ]),
```
and replace its expected `"signals": {"A": "red"}, "routes": {"A-E": null}, "berths": {"BA": null, "BW1": "2W03"}` line with:
```rust
            "signals": {"A": "red"}, "routes": {"A-E": null}, "berths": {"BA": null, "BW1": "2W03"},
            "trains": {"1E01": null, "2W03": {"next_place": "WST", "next_platform": null, "booked": 26100.0,
                                             "late_s": 0, "state": "at_platform"}}
```

In `crates/protocol/tests/diff.rs`, add to `base()`'s literal after `berths`:
```rust
        trains: BTreeMap::from([(s("1E01"), row(TrainState::InArea, 0)), (s("2W03"), row(TrainState::Due, 0))]),
```
add this helper after `base()`:
```rust
fn row(state: TrainState, late_s: i64) -> TrainRow {
    TrainRow { next_place: Some(s("EST")), next_platform: Some(s("1")), booked: Some(25500.0), late_s, state }
}
```
add to `changed()` before its final `v`:
```rust
    v.trains.insert(s("1E01"), row(TrainState::AtPlatform, 60));
    v.trains.remove("2W03");
    v.trains.insert(s("1W05"), row(TrainState::Approaching, 0));
```
and append:
```rust
#[test]
fn trains_travel_like_berths_changed_added_and_removed() {
    let d = diff(&base(), &changed()).unwrap();
    assert_eq!(
        d.trains,
        BTreeMap::from([
            (s("1E01"), Some(row(TrainState::AtPlatform, 60))),
            (s("1W05"), Some(row(TrainState::Approaching, 0))),
            (s("2W03"), None),
        ])
    );
    let json = serde_json::to_value(&d).unwrap();
    assert!(json["trains"]["2W03"].is_null(), "{json}");
    let mut same = base();
    same.seq = 2;
    same.trains.insert(s("1E01"), row(TrainState::InArea, 0));
    assert_eq!(diff(&base(), &same), None, "an unchanged row is not sent");
}
```
(`applying_the_diff_rebuilds_the_new_view` now covers trains too, since `changed()` changes them.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `scripts/cargo test -p signalbox-protocol`
Expected: compile errors — `cannot find struct Geometry`, `no field trains`, `TrainRow` not found.

- [ ] **Step 3: Add the types**

In `crates/protocol/src/view.rs`, add the field at the end of `Layout` (after `routes`):
```rust
    /// The diagram of the visible part; `None` when the world has none.
    #[serde(default)]
    pub geometry: Option<Geometry>,
```
and after the `Layout` struct:
```rust
/// Diagram geometry in the layout's own coordinates (TS2 scene units, y
/// grows downwards), limited to what the player sees.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    pub lines: Vec<LineGeom>,
    pub points: Vec<PointsGeom>,
    pub signals: Vec<SignalGeom>,
    pub platforms: Vec<PlatformGeom>,
    pub labels: Vec<LabelGeom>,
    /// Where route exits at nodes (buffer stops, boundaries) and boundary
    /// berths are drawn.
    pub nodes: Vec<NodeGeom>,
}

/// A segment drawn from (x1, y1) at its `from` node to (x2, y2) at its `to` node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineGeom {
    pub segment: String,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

/// Points at (x, y); each leg ends where the next drawn line starts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointsGeom {
    pub node: String,
    pub x: f64,
    pub y: f64,
    pub toe: Option<[f64; 2]>,
    pub normal: Option<[f64; 2]>,
    pub reverse: Option<[f64; 2]>,
}

/// A signal at (x, y), its berth box at (berth_x, berth_y), and `facing`,
/// the direction a train passing it travels (not normalised).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignalGeom {
    pub signal: String,
    pub x: f64,
    pub y: f64,
    pub berth_x: f64,
    pub berth_y: f64,
    pub facing: Option<[f64; 2]>,
}

/// A platform rectangle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlatformGeom {
    pub place: String,
    pub platform: String,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeGeom {
    pub node: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LabelGeom {
    pub text: String,
    pub x: f64,
    pub y: f64,
}
```
Add at the end of `View` (after `berths`):
```rust
    /// Trains this player should know about, by headcode (spec D1 §4.2).
    #[serde(default)]
    pub trains: BTreeMap<String, TrainRow>,
```
and after the `View` struct:
```rust
/// In the order a train goes through them (the train list sorts by it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainState {
    /// Not on the railway yet.
    Due,
    /// Running, but outside your area.
    Approaching,
    InArea,
    /// Standing at a platform (dwelling).
    AtPlatform,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainRow {
    /// The next call, `None` once the timetable is done.
    pub next_place: Option<String>,
    pub next_platform: Option<String>,
    /// Booked time at the next call (arrival, else departure), seconds since midnight.
    pub booked: Option<f64>,
    /// How late against `booked` right now, in whole minutes, as seconds; never negative.
    pub late_s: i64,
    pub state: TrainState,
}
```
Add at the end of `Delta` (after `berths`):
```rust
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub trains: BTreeMap<String, Option<TrainRow>>,
```

In `crates/protocol/src/diff.rs`, in `diff` after `d.berths = sparse(&old.berths, &new.berths);`:
```rust
    d.trains = sparse(&old.trains, &new.trains);
```
and in `View::apply`, after the `for (k, v) in &d.berths { ... }` loop and before `Ok(())`:
```rust
        for (k, v) in &d.trains {
            match v {
                Some(row) => {
                    self.trains.insert(k.clone(), row.clone());
                }
                None => {
                    self.trains.remove(k);
                }
            }
        }
```

- [ ] **Step 4: Keep every other literal compiling**

The new fields are required in struct literals. Find them all with
`grep -rn "View {\|Layout {" crates --include=*.rs | grep -v "SectionView\|PointsView\|RouteView\|VoteView\|ServerMsg::Layout(l)"`
and add the missing field to each literal (there are exactly these, as of C2's merge):
- `crates/game/src/layout.rs` `build_layout`: add `geometry: None,` after the `routes` field (Task 2 replaces it).
- `crates/game/src/view.rs` `build_view`: add `trains: BTreeMap::new(),` after `berths` (Task 2 replaces it).
- `crates/bot/tests/strategy.rs`: `screen()` gets `geometry: None,`; `view()` gets `trains: BTreeMap::new(),`.
- `crates/bot/tests/bot.rs` `view()`: `trains: BTreeMap::new(),`.
- `crates/bot/tests/play.rs`: the `Layout` literal gets `geometry: None,`, the `View` literal `trains: BTreeMap::new(),`.
- `crates/server/tests/supervisor.rs` `fn view(seq)`: `trains: Default::default(),`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `scripts/cargo test -p signalbox-protocol`
Expected: PASS (golden 9 tests, diff 6 tests, lobby unchanged).
Run: `scripts/cargo build --workspace --all-targets` and `scripts/cargo build -p signalbox-server --features dev-auth --all-targets`
Expected: both finish with no errors and no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/protocol crates/game/src/layout.rs crates/game/src/view.rs crates/bot/tests crates/server/tests/supervisor.rs
git commit -m "feat(protocol): diagram geometry in the layout and a train list in the view"
```

---

### Task 2: `game` — build geometry and train rows from the world and sim state

**Files:**
- Create: `crates/game/src/geometry.rs`
- Modify: `crates/game/Cargo.toml` (+ `serde`), `crates/game/src/lib.rs` (`pub mod geometry;`), `crates/game/src/layout.rs` (`build_layout` takes the geometry), `crates/game/src/game.rs` (keeps a `WorldGeometry`), `crates/game/src/view.rs` (`build_trains`, `late_s`, `DUE_WINDOW_S`)
- Modify: `crates/core/src/sim.rs` (`pending_entries`, `next_entry`)
- Create: `crates/game/tests/fixtures/twobox-layout.json`, `crates/game/tests/geometry.rs`, `crates/game/tests/trains.rs`
- Modify: `crates/game/tests/common/mod.rs` (`TWOBOX_LAYOUT`), `crates/game/tests/layout.rs` (new `build_layout` argument), `crates/game/tests/game.rs` (consistency test checks trains), `crates/bot/tests/soak.rs` (Liverpool soak checks trains and geometry), `crates/core/tests/timetable.rs`

**Interfaces:**
- Consumes: Task 1's protocol types; `World.layout: serde_json::Value` (ts2-import writes `{"source": "ts2", "lines": [{segment, x1, y1, x2, y2}], "points": [{node, x, y}], "signals": [{signal, x, y, berth_x, berth_y}], "platforms": [{place, platform, x1, y1, x2, y2}], "labels": [{text, x, y}]}` — `crates/ts2-import/src/layout.rs`); `Visibility` (`sections`, `signals`, `points`, `berths`, `routes`, `area`); `NodeKind::Points { toe, normal, reverse, .. }`; `Segment { a, b, section, .. }`, `Segment::end_node(dir)`; `Network::{segment, node, signal}` name lookups; `timetable::{Call, EntryStart}`; `Train { service, headcode, path, next_call, dwell, .. }`.
- Produces:
  - `signalbox_core::sim::Sim::pending_entries(&self) -> &[PendingEntry]` and `Sim::next_entry(&self) -> usize`.
  - `game::geometry::WorldGeometry` (`Clone, Debug, PartialEq`) with `WorldGeometry::from_world(w: &World) -> Option<WorldGeometry>` and `fn visible(&self, w: &World, vis: &Visibility) -> protocol::Geometry`; `pub const LABEL_MARGIN: f64 = 40.0`.
  - `game::layout::build_layout(w: &World, map: &AreaMap, vis: &Visibility, you: &str, geo: Option<&WorldGeometry>) -> Layout` (new last argument).
  - `game::view::build_trains(sim: &Sim, vis: &Visibility) -> BTreeMap<String, TrainRow>`, `game::view::late_s(now: f64, booked: Option<f64>) -> i64`, `pub const DUE_WINDOW_S: f64 = 1800.0`; `build_view` fills `trains`.
  - Test fixture `crates/game/tests/fixtures/twobox-layout.json` (Task 5 reads it too).

- [ ] **Step 1: Write the twobox drawing fixture**

Create `crates/game/tests/fixtures/twobox-layout.json` (twobox drawn left to right; the points' legs are undrawn like TS2's; plus an unknown segment, a joint named as points, a signal the world lacks and a label far away, which must all be dropped):
```json
{
  "source": "ts2",
  "lines": [
    {"segment": "w1", "x1": 0.0, "y1": 0.0, "x2": 100.0, "y2": 0.0},
    {"segment": "w2", "x1": 100.0, "y1": 0.0, "x2": 200.0, "y2": 0.0},
    {"segment": "e", "x1": 215.0, "y1": 0.0, "x2": 400.0, "y2": 0.0},
    {"segment": "n", "x1": 215.0, "y1": 10.0, "x2": 400.0, "y2": 60.0},
    {"segment": "nowhere", "x1": 0.0, "y1": 0.0, "x2": 1.0, "y2": 1.0}
  ],
  "points": [
    {"node": "P", "x": 207.5, "y": 0.0},
    {"node": "J0", "x": 100.0, "y": 0.0}
  ],
  "signals": [
    {"signal": "W1", "x": 100.0, "y": -5.0, "berth_x": 90.0, "berth_y": -15.0},
    {"signal": "A", "x": 200.0, "y": -5.0, "berth_x": 190.0, "berth_y": -15.0},
    {"signal": "W2", "x": 100.0, "y": 5.0, "berth_x": 110.0, "berth_y": 15.0},
    {"signal": "C", "x": 215.0, "y": 5.0, "berth_x": 225.0, "berth_y": 15.0},
    {"signal": "D", "x": 215.0, "y": 15.0, "berth_x": 225.0, "berth_y": 25.0},
    {"signal": "Z", "x": 0.0, "y": 0.0, "berth_x": 0.0, "berth_y": 0.0}
  ],
  "platforms": [
    {"place": "EST", "platform": "1", "x1": 300.0, "y1": -8.0, "x2": 360.0, "y2": -3.0},
    {"place": "NST", "platform": "1", "x1": 300.0, "y1": 45.0, "x2": 360.0, "y2": 50.0}
  ],
  "labels": [
    {"text": "West", "x": 50.0, "y": -30.0},
    {"text": "East", "x": 300.0, "y": -30.0},
    {"text": "Far away", "x": 5000.0, "y": 5000.0}
  ]
}
```
In `crates/game/tests/common/mod.rs`, after `pub const TWOBOX`:
```rust
/// A drawing of twobox, for the world's `layout` field.
pub const TWOBOX_LAYOUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/twobox-layout.json");
```

- [ ] **Step 2: Write the failing tests**

Create `crates/game/tests/geometry.rs`:
```rust
//! Diagram geometry (spec D1 §4.1): read once from the world's `layout`,
//! cut down to each player's visible set.

mod common;

use common::*;
use game::areas::{AreaMap, Visibility};
use game::geometry::WorldGeometry;
use game::layout::build_layout;
use game::{Game, GameMeta};
use protocol::{Geometry, LabelGeom, LineGeom, NodeGeom, PlatformGeom, PointsGeom, ServerMsg, SignalGeom};
use serde_json::{Value, json};
use signalbox_core::world::World;

/// Twobox drawn left to right (`fixtures/twobox-layout.json`): W at x 0,
/// the points at x 207.5 with their legs undrawn (like TS2's), E and N on
/// the right; plus an unknown segment, a joint named as points, a signal
/// the world lacks and a label far away.
fn twobox_layout() -> Value {
    serde_json::from_str(&std::fs::read_to_string(TWOBOX_LAYOUT).unwrap()).unwrap()
}

fn twobox_with(layout: Value) -> World {
    let mut json: Value = serde_json::from_str(&twobox_json()).unwrap();
    json["layout"] = layout;
    World::from_json(&json.to_string()).unwrap()
}

fn geometry_for(w: &World, area_name: Option<&str>) -> Option<Geometry> {
    let m = AreaMap::new(w);
    let vis = match area_name {
        Some(a) => Visibility::of_area(w, &m, area(w, a)),
        None => Visibility::spectator(w, &m),
    };
    build_layout(w, &m, &vis, "alice", WorldGeometry::from_world(w).as_ref()).geometry
}

fn line(segment: &str, x1: f64, y1: f64, x2: f64, y2: f64) -> LineGeom {
    LineGeom { segment: segment.into(), x1, y1, x2, y2 }
}

fn node(name: &str, x: f64, y: f64) -> NodeGeom {
    NodeGeom { node: name.into(), x, y }
}

#[test]
fn an_area_sees_its_own_drawing_and_the_fringe() {
    let g = geometry_for(&twobox_with(twobox_layout()), Some("West")).unwrap();
    assert_eq!(g.lines, [line("w1", 0.0, 0.0, 100.0, 0.0), line("w2", 100.0, 0.0, 200.0, 0.0)]);
    assert_eq!(
        g.points,
        [PointsGeom {
            node: "P".into(),
            x: 207.5,
            y: 0.0,
            toe: Some([200.0, 0.0]),
            normal: Some([215.0, 0.0]),
            reverse: Some([215.0, 10.0]),
        }],
        "each leg ends where the next drawn line starts"
    );
    let signals: Vec<(&str, Option<[f64; 2]>)> = g.signals.iter().map(|s| (s.signal.as_str(), s.facing)).collect();
    assert_eq!(signals, [("W1", Some([100.0, 0.0])), ("A", Some([100.0, 0.0])), ("W2", Some([-100.0, 0.0]))]);
    assert_eq!(
        g.signals[1],
        SignalGeom { signal: "A".into(), x: 200.0, y: -5.0, berth_x: 190.0, berth_y: -15.0, facing: Some([100.0, 0.0]) }
    );
    assert!(g.platforms.is_empty(), "EST and NST are on East's track, not in West's fringe");
    assert_eq!(g.labels, [LabelGeom { text: "West".into(), x: 50.0, y: -30.0 }]);
    assert_eq!(
        g.nodes,
        [node("W", 0.0, 0.0), node("E", 400.0, 0.0), node("N", 400.0, 60.0)],
        "the exits of West's routes and its boundary berth, found along the track"
    );
}

#[test]
fn a_spectator_sees_the_whole_drawing_and_labels_near_it() {
    let g = geometry_for(&twobox_with(twobox_layout()), None).unwrap();
    let lines: Vec<&str> = g.lines.iter().map(|l| l.segment.as_str()).collect();
    assert_eq!(lines, ["w1", "w2", "e", "n"], "unknown segments are dropped");
    assert_eq!(g.points.len(), 1, "a joint named as points is dropped");
    let signals: Vec<&str> = g.signals.iter().map(|s| s.signal.as_str()).collect();
    assert_eq!(signals, ["W1", "A", "W2", "C", "D"]);
    assert_eq!(g.signals[4].facing, Some([-185.0, -50.0]));
    assert_eq!(
        g.platforms[0],
        PlatformGeom { place: "EST".into(), platform: "1".into(), x1: 300.0, y1: -8.0, x2: 360.0, y2: -3.0 }
    );
    assert_eq!(g.platforms.len(), 2);
    let labels: Vec<&str> = g.labels.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(labels, ["West", "East"]);
}

#[test]
fn a_signal_on_an_undrawn_segment_faces_along_the_line_behind_it() {
    let mut layout = twobox_layout();
    layout["lines"].as_array_mut().unwrap().retain(|l| l["segment"] != "w2");
    let g = geometry_for(&twobox_with(layout), Some("West")).unwrap();
    let a = g.signals.iter().find(|s| s.signal == "A").unwrap();
    assert_eq!(a.facing, Some([100.0, 0.0]), "from w1, the line in rear of A");
    let w2 = g.signals.iter().find(|s| s.signal == "W2").unwrap();
    assert_eq!(w2.facing, Some([-185.0, 0.0]), "through P's undrawn legs to e, the nearest line in rear");
}

#[test]
fn no_or_unreadable_layout_means_no_geometry() {
    assert_eq!(geometry_for(&twobox(), Some("West")), None);
    assert_eq!(geometry_for(&twobox_with(json!({"lines": 5})), Some("West")), None);
    assert_eq!(geometry_for(&twobox_with(json!("a picture")), None), None);
    let empty = geometry_for(&twobox_with(json!({})), None).unwrap();
    assert_eq!(empty, Geometry::default());
}

#[test]
fn the_game_sends_geometry_with_every_layout() {
    let mut g = Game::new(twobox_with(twobox_layout()), GameMeta { layout: "twobox".into(), seed: 1 });
    let out = join(&mut g, "alice", Some("East"));
    let layouts: Vec<&protocol::Layout> = out
        .iter()
        .filter_map(|(_, m)| match m {
            ServerMsg::Layout(l) => Some(l),
            _ => None,
        })
        .collect();
    assert_eq!(layouts.len(), 2, "one on connect, one on claim");
    assert_eq!(layouts[0].geometry.as_ref().unwrap().signals.len(), 5, "the spectator's");
    let east = layouts[1].geometry.as_ref().unwrap();
    let signals: Vec<&str> = east.signals.iter().map(|s| s.signal.as_str()).collect();
    assert_eq!(signals, ["A", "W2", "C", "D"]);
    assert_eq!(g.layout_of("alice").unwrap().geometry.as_ref(), Some(east));
}

/// Every signal, points and route exit a player of converted Liverpool
/// Street sees is drawn: signals face a way, points have all three legs.
#[test]
fn liverpool_street_draws_everything_each_player_sees() {
    let w = World::from_json(&liverpool_json()).unwrap();
    let m = AreaMap::new(&w);
    let geo = WorldGeometry::from_world(&w).expect("ts2-import writes a layout");
    let mut visions: Vec<Visibility> = (0..w.net.areas.len())
        .map(|a| Visibility::of_area(&w, &m, signalbox_core::ids::AreaId::from_idx(a)))
        .collect();
    visions.push(Visibility::spectator(&w, &m));
    for vis in &visions {
        let l = build_layout(&w, &m, vis, "ann", Some(&geo));
        let g = l.geometry.as_ref().unwrap();
        for s in &l.signals {
            let sg = g.signals.iter().find(|x| x.signal == s.name).unwrap_or_else(|| panic!("signal {} undrawn", s.name));
            assert!(sg.facing.is_some(), "signal {} faces nowhere", s.name);
        }
        for p in &l.points {
            let pg = g.points.iter().find(|x| x.node == p.name).unwrap_or_else(|| panic!("points {} undrawn", p.name));
            assert!(pg.toe.is_some() && pg.normal.is_some() && pg.reverse.is_some(), "{pg:?}");
        }
        for r in &l.routes {
            if let protocol::ExitName::Node(n) = &r.exit {
                assert!(g.nodes.iter().any(|x| x.node == *n), "exit {n} of {} has no position", r.name);
            }
        }
        assert!(!g.lines.is_empty() && !g.labels.is_empty());
        let bytes = serde_json::to_string(&l).unwrap().len();
        assert!(bytes < 512 * 1024, "{bytes} bytes");
    }
}
```

Create `crates/game/tests/trains.rs`:
```rust
//! The train list in each view (spec D1 §4.2), built from sim state.

mod common;

use std::collections::BTreeSet;

use common::*;
use game::areas::{AreaMap, Visibility};
use game::view::{DUE_WINDOW_S, build_trains, late_s};
use game::{Game, GameMeta};
use protocol::{TrainRow, TrainState};
use signalbox_core::sim::Sim;

fn states(rows: &std::collections::BTreeMap<String, TrainRow>) -> Vec<(&str, TrainState)> {
    rows.iter().map(|(h, r)| (h.as_str(), r.state)).collect()
}

#[test]
fn late_is_whole_minutes_and_never_negative() {
    assert_eq!(late_s(25_300.0, Some(25_200.0)), 60);
    assert_eq!(late_s(25_259.9, Some(25_200.0)), 0, "59.9 s is not a minute");
    assert_eq!(late_s(25_200.0, Some(25_200.0)), 0);
    assert_eq!(late_s(25_000.0, Some(25_200.0)), 0, "early");
    assert_eq!(late_s(25_000.0, None), 0);
    assert_eq!(late_s(26_000.0, Some(25_200.0)), 780);
}

/// At 07:00 nothing has entered: each side sees the entries due at its own
/// boundaries within the window, a spectator sees them all.
#[test]
fn before_anything_enters_each_area_sees_what_is_due_at_its_boundaries() {
    let w = twobox();
    let m = AreaMap::new(&w);
    let west = Visibility::of_area(&w, &m, area(&w, "West"));
    let east = Visibility::of_area(&w, &m, area(&w, "East"));
    let all = Visibility::spectator(&w, &m);
    let sim = Sim::new(w, 1);
    let rows = build_trains(&sim, &west);
    assert_eq!(states(&rows), [("1E01", TrainState::Due), ("1N02", TrainState::Due)]);
    assert_eq!(
        rows["1E01"],
        TrainRow {
            next_place: Some("EST".into()),
            next_platform: Some("1".into()),
            booked: Some(25_440.0),
            late_s: 0,
            state: TrainState::Due,
        }
    );
    assert_eq!(states(&build_trains(&sim, &east)), [("2W03", TrainState::Due), ("2W04", TrainState::Due)]);
    assert_eq!(build_trains(&sim, &east)["2W03"].next_place, None, "no calls");
    assert_eq!(build_trains(&sim, &all).len(), 4);
}

#[test]
fn the_window_is_thirty_sim_minutes() {
    assert_eq!(DUE_WINDOW_S, 1800.0);
    let mut json: serde_json::Value = serde_json::from_str(&twobox_json()).unwrap();
    json["entries"][2]["time"] = "07:31".into();
    let w = signalbox_core::world::World::from_json(&json.to_string()).unwrap();
    let m = AreaMap::new(&w);
    let west = Visibility::of_area(&w, &m, area(&w, "West"));
    let mut sim = Sim::new(w, 1);
    assert_eq!(states(&build_trains(&sim, &west)), [("1E01", TrainState::Due)], "1N02 at 07:31 is beyond 07:30");
    sim.run_for(60.0);
    assert!(build_trains(&sim, &west).contains_key("1N02"), "at 07:01 it is inside the window");
}

/// 1E01 enters at W, runs through West, is handed to East and stops at
/// EST 1. West lists it before it enters and while it is on West's track;
/// East lists it from the moment it runs, because its next call is at
/// East's platform, although West's track is not East's to see.
#[test]
fn a_train_is_due_then_in_area_then_approaching_then_at_the_platform() {
    let mut g = Game::new(twobox(), GameMeta { layout: "twobox".into(), seed: 1 });
    join(&mut g, "west", Some("West"));
    join(&mut g, "east", Some("East"));
    let first = |p: &str| g.view_of(p).unwrap().trains.get("1E01").map(|r| r.state);
    assert_eq!((first("west"), first("east")), (Some(TrainState::Due), None), "it enters at West's boundary");
    let mut seen_west: BTreeSet<TrainState> = BTreeSet::from([TrainState::Due]);
    let mut seen_east: Vec<TrainState> = Vec::new();
    for _ in 0..900 {
        play_as_robot(&mut g, "west");
        play_as_robot(&mut g, "east");
        g.advance(1.0);
        if let Some(r) = g.view_of("west").and_then(|v| v.trains.get("1E01").cloned()) {
            seen_west.insert(r.state);
        }
        if let Some(r) = g.view_of("east").and_then(|v| v.trains.get("1E01").cloned()) {
            if seen_east.last() != Some(&r.state) {
                seen_east.push(r.state);
            }
            if r.state == TrainState::AtPlatform {
                break;
            }
        }
    }
    assert_eq!(
        seen_west,
        BTreeSet::from([TrainState::Due, TrainState::Approaching, TrainState::InArea]),
        "approaching: on West's fringe with its head in East"
    );
    assert_eq!(seen_east, [TrainState::Approaching, TrainState::InArea, TrainState::AtPlatform]);
}

/// Rows follow the sim only, so a client applying deltas keeps them
/// exactly (the consistency tests in game.rs and the bot soak compare
/// whole views, trains included).
#[test]
fn trains_are_a_function_of_state() {
    let mut g = Game::new(twobox(), GameMeta { layout: "twobox".into(), seed: 1 });
    join(&mut g, "sam", None);
    let mut c = Client::default();
    c.take(&g.resync("sam"), "sam");
    let mut ever: BTreeSet<String> = BTreeSet::new();
    for _ in 0..2400 {
        let out = g.advance(0.5);
        c.take(&out, "sam");
        c.take(&g.flush(), "sam");
        assert_eq!(c.view, g.view_of("sam"));
        ever.extend(c.view.as_ref().unwrap().trains.keys().cloned());
    }
    assert_eq!(ever.len(), 4, "{ever:?}");
}
```

Append to `crates/core/tests/timetable.rs`:
```rust
#[test]
fn offered_entries_wait_in_pending_until_they_enter() {
    let w = load_with("terminus", |j| j["options"]["entry_delay_s"] = json!([120, 120])).unwrap();
    let mut sim = Sim::new(w, 1);
    assert_eq!((sim.next_entry(), sim.pending_entries().len()), (0, 0));
    sim.step();
    assert_eq!(sim.next_entry(), 1, "06:00's entry is offered");
    assert_eq!(sim.pending_entries().len(), 1);
    assert_eq!((sim.pending_entries()[0].entry, sim.pending_entries()[0].due_s), (0, 6.0 * 3600.0 + 120.0));
    assert!(sim.trains().is_empty());
    sim.run_for(130.0);
    assert!(sim.pending_entries().is_empty());
    assert_eq!(sim.trains().len(), 1);
}
```

Extend the two consistency checks so they cover the new fields. In `crates/game/tests/game.rs`, `every_client_rebuilds_the_servers_view_from_deltas`: after `assert_eq!(g.clock().speed, 8);` add
```rust
    let mut trains_seen: BTreeMap<&'static str, usize> = BTreeMap::new();
```
inside the `for (p, c) in &clients` check loop, after the layout assertion, add
```rust
                *trains_seen.entry(p).or_default() += c.view.as_ref().map_or(0, |v| v.trains.len());
```
and before `let st = g.stats();` add
```rust
    assert!(trains_seen.values().all(|&n| n > 0), "every view listed trains: {trains_seen:?}");
```
In `crates/bot/tests/soak.rs`: add a field to `Table` after `not_your_area`:
```rust
    /// Rows in the bots' train lists, summed over every check.
    train_rows: usize,
```
initialise it (`train_rows: 0,`) in `Table::new`; in `period()`, after the layout assertion inside the bots loop, add
```rust
                self.train_rows += bot.view().map_or(0, |v| v.trains.len());
```
and at the end of `play_liverpool`:
```rust
    assert!(t.train_rows > 0, "the views listed trains");
    assert!(t.bots.values().all(|b| b.layout().is_some_and(|l| l.geometry.is_some())), "and carried the diagram");
```
In `crates/game/tests/layout.rs`, `layout_for` calls `build_layout(&w, &m, &vis, "alice", None)`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `scripts/cargo test -p signalbox-game --test geometry --test trains`
Expected: compile errors — no module `geometry`, `build_trains` / `late_s` / `DUE_WINDOW_S` not found, `build_layout` takes 4 arguments.

- [ ] **Step 4: Core accessors**

In `crates/core/src/sim.rs`, after `pub fn trains(&self)`:
```rust
    /// Entries already offered at the fringe and waiting to enter, oldest first.
    pub fn pending_entries(&self) -> &[PendingEntry] {
        &self.st.pending
    }

    /// Index into `world().entries` of the first entry not yet offered.
    pub fn next_entry(&self) -> usize {
        self.st.next_entry
    }
```

- [ ] **Step 5: The geometry module**

`crates/game/Cargo.toml`: add `serde.workspace = true` to `[dependencies]` (after `rusqlite`). `crates/game/src/lib.rs`: add `pub mod geometry;` after `pub mod game;`.

Create `crates/game/src/geometry.rs`:
```rust
//! Diagram geometry for clients (spec D1 §4.1), read once from the world's
//! `layout` JSON (ts2-import writes it) and cut down to each player's
//! visible set. Names in the JSON that the world does not know are dropped.

use std::collections::{BTreeMap, BTreeSet};

use protocol::{Geometry, LabelGeom, LineGeom, NodeGeom, PlatformGeom, PointsGeom, SignalGeom};
use serde::Deserialize;
use signalbox_core::ids::*;
use signalbox_core::network::{Dir, Network, NodeKind};
use signalbox_core::routes::Exit;
use signalbox_core::world::World;

use crate::areas::Visibility;

/// Labels this far outside the visible drawing are still shown.
pub const LABEL_MARGIN: f64 = 40.0;
/// How many nodes away from a points leg, a signal or an exit to look for a drawn line.
const SEARCH_DEPTH: usize = 4;

#[derive(Deserialize)]
struct RawLayout {
    #[serde(default)]
    lines: Vec<RawLine>,
    #[serde(default)]
    points: Vec<RawPoints>,
    #[serde(default)]
    signals: Vec<RawSignal>,
    #[serde(default)]
    platforms: Vec<PlatformGeom>,
    #[serde(default)]
    labels: Vec<LabelGeom>,
}

#[derive(Deserialize)]
struct RawLine {
    segment: String,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

#[derive(Deserialize)]
struct RawPoints {
    node: String,
    x: f64,
    y: f64,
}

#[derive(Deserialize)]
struct RawSignal {
    signal: String,
    x: f64,
    y: f64,
    berth_x: f64,
    berth_y: f64,
}

/// The whole world's geometry, resolved to ids.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldGeometry {
    lines: Vec<(SegmentId, LineGeom)>,
    points: Vec<(NodeId, PointsGeom)>,
    signals: Vec<(SignalId, SignalGeom)>,
    /// With the segments their (place, platform) covers in the world.
    platforms: Vec<(Vec<SegmentId>, PlatformGeom)>,
    labels: Vec<LabelGeom>,
    /// Route exit nodes and boundary-berth nodes, where a line can be found.
    nodes: BTreeMap<NodeId, NodeGeom>,
}

impl WorldGeometry {
    /// `None` when the world has no usable `layout` (hand-made worlds).
    pub fn from_world(w: &World) -> Option<WorldGeometry> {
        if !w.layout.is_object() {
            return None;
        }
        let raw: RawLayout = serde_json::from_value(w.layout.clone()).ok()?;
        let net = &w.net;
        let lines: Vec<(SegmentId, LineGeom)> = raw
            .lines
            .into_iter()
            .filter_map(|l| {
                let seg = net.segment(&l.segment)?;
                Some((seg, LineGeom { segment: l.segment, x1: l.x1, y1: l.y1, x2: l.x2, y2: l.y2 }))
            })
            .collect();
        let line_of: BTreeMap<SegmentId, usize> = lines.iter().enumerate().map(|(i, (s, _))| (*s, i)).collect();
        let points = raw
            .points
            .into_iter()
            .filter_map(|p| {
                let n = net.node(&p.node)?;
                let NodeKind::Points { toe, normal, reverse, .. } = net.nodes[n.idx()].kind else { return None };
                let leg = |seg: SegmentId| {
                    let s = &net.segments[seg.idx()];
                    let far = if s.a == n { s.b } else { s.a };
                    line_from(net, &lines, &line_of, far, Some(n)).map(|(end, _)| end)
                };
                Some((n, PointsGeom { node: p.node, x: p.x, y: p.y, toe: leg(toe), normal: leg(normal), reverse: leg(reverse) }))
            })
            .collect();
        let signals = raw
            .signals
            .into_iter()
            .filter_map(|s| {
                let id = net.signal(&s.signal)?;
                let facing = facing(net, &lines, &line_of, id);
                Some((id, SignalGeom { signal: s.signal, x: s.x, y: s.y, berth_x: s.berth_x, berth_y: s.berth_y, facing }))
            })
            .collect();
        let platforms = raw
            .platforms
            .into_iter()
            .map(|p| {
                let segs = net
                    .platforms
                    .iter()
                    .filter(|q| q.place == p.place && q.platform == p.platform)
                    .map(|q| q.segment)
                    .collect();
                (segs, p)
            })
            .collect();
        let wanted: BTreeSet<NodeId> = w
            .routes
            .iter()
            .filter_map(|r| match r.exit {
                Exit::Node(n) => Some(n),
                Exit::Signal(_) => None,
            })
            .chain(net.berths.iter().filter_map(|b| b.boundary))
            .collect();
        let nodes = wanted
            .into_iter()
            .filter_map(|n| {
                let ([x, y], _) = line_from(net, &lines, &line_of, n, None)?;
                Some((n, NodeGeom { node: net.nodes[n.idx()].name.clone(), x, y }))
            })
            .collect();
        Some(WorldGeometry { lines, points, signals, platforms, labels: raw.labels, nodes })
    }

    /// What `vis` sees: drawn segments, points and signals it can see,
    /// platforms on a visible segment, labels near all that, and the
    /// positions of its routes' exit nodes and boundary berths.
    pub fn visible(&self, w: &World, vis: &Visibility) -> Geometry {
        let net = &w.net;
        let sections: BTreeSet<SectionId> = vis.sections.iter().copied().collect();
        let seg_visible = |s: SegmentId| sections.contains(&net.segments[s.idx()].section);
        let points_seen: BTreeSet<NodeId> = vis.points.iter().copied().collect();
        let signals_seen: BTreeSet<SignalId> = vis.signals.iter().copied().collect();
        let lines: Vec<LineGeom> = self.lines.iter().filter(|(s, _)| seg_visible(*s)).map(|(_, l)| l.clone()).collect();
        let points: Vec<PointsGeom> =
            self.points.iter().filter(|(n, _)| points_seen.contains(n)).map(|(_, p)| p.clone()).collect();
        let signals: Vec<SignalGeom> =
            self.signals.iter().filter(|(s, _)| signals_seen.contains(s)).map(|(_, g)| g.clone()).collect();
        let platforms: Vec<PlatformGeom> = self
            .platforms
            .iter()
            .filter(|(segs, _)| segs.iter().any(|&s| seg_visible(s)))
            .map(|(_, p)| p.clone())
            .collect();
        let mut bounds = Bounds::default();
        for l in &lines {
            bounds.add(l.x1, l.y1);
            bounds.add(l.x2, l.y2);
        }
        for p in &points {
            bounds.add(p.x, p.y);
        }
        for s in &signals {
            bounds.add(s.x, s.y);
            bounds.add(s.berth_x, s.berth_y);
        }
        for p in &platforms {
            bounds.add(p.x1, p.y1);
            bounds.add(p.x2, p.y2);
        }
        let labels = self.labels.iter().filter(|l| bounds.near(l.x, l.y, LABEL_MARGIN)).cloned().collect();
        let mut wanted: BTreeSet<NodeId> = vis
            .routes
            .iter()
            .filter_map(|r| match w.routes[r.idx()].exit {
                Exit::Node(n) => Some(n),
                Exit::Signal(_) => None,
            })
            .collect();
        wanted.extend(vis.berths.iter().filter_map(|b| net.berths[b.idx()].boundary));
        let nodes = wanted.iter().filter_map(|n| self.nodes.get(n).cloned()).collect();
        Geometry { lines, points, signals, platforms, labels, nodes }
    }
}

#[derive(Default)]
struct Bounds {
    min: Option<(f64, f64)>,
    max: (f64, f64),
}

impl Bounds {
    fn add(&mut self, x: f64, y: f64) {
        match self.min {
            None => {
                self.min = Some((x, y));
                self.max = (x, y);
            }
            Some((mx, my)) => {
                self.min = Some((mx.min(x), my.min(y)));
                self.max = (self.max.0.max(x), self.max.1.max(y));
            }
        }
    }

    fn near(&self, x: f64, y: f64, margin: f64) -> bool {
        self.min.is_some_and(|(mx, my)| {
            x >= mx - margin && x <= self.max.0 + margin && y >= my - margin && y <= self.max.1 + margin
        })
    }
}

/// The nearest drawn line reached from node `start` without passing
/// `avoid`, walking at most `SEARCH_DEPTH` nodes: the line's end at the
/// node it was reached through, and the direction along the line towards
/// that end.
fn line_from(
    net: &Network,
    lines: &[(SegmentId, LineGeom)],
    line_of: &BTreeMap<SegmentId, usize>,
    start: NodeId,
    avoid: Option<NodeId>,
) -> Option<([f64; 2], [f64; 2])> {
    let mut seen: BTreeSet<NodeId> = avoid.into_iter().collect();
    let mut frontier = vec![start];
    for _ in 0..SEARCH_DEPTH {
        let mut next = Vec::new();
        for n in frontier {
            if !seen.insert(n) {
                continue;
            }
            for &seg in &net.nodes[n.idx()].segments {
                let s = &net.segments[seg.idx()];
                if let Some(&i) = line_of.get(&seg) {
                    let l = &lines[i].1;
                    return Some(if s.a == n {
                        ([l.x1, l.y1], [l.x1 - l.x2, l.y1 - l.y2])
                    } else {
                        ([l.x2, l.y2], [l.x2 - l.x1, l.y2 - l.y1])
                    });
                }
                next.push(if s.a == n { s.b } else { s.a });
            }
        }
        frontier = next;
    }
    None
}

/// The direction a train passing signal `s` travels: along its segment's
/// line, or else along the nearest line in rear of it.
fn facing(net: &Network, lines: &[(SegmentId, LineGeom)], line_of: &BTreeMap<SegmentId, usize>, s: SignalId) -> Option<[f64; 2]> {
    let at = &net.signals[s.idx()].at;
    let seg = &net.segments[at.segment.idx()];
    let v = match line_of.get(&at.segment) {
        Some(&i) => {
            let l = &lines[i].1;
            match at.dir {
                Dir::Up => [l.x2 - l.x1, l.y2 - l.y1],
                Dir::Down => [l.x1 - l.x2, l.y1 - l.y2],
            }
        }
        None => line_from(net, lines, line_of, seg.end_node(at.dir.rev()), Some(seg.end_node(at.dir)))?.1,
    };
    (v[0] != 0.0 || v[1] != 0.0).then_some(v)
}
```

- [ ] **Step 6: Wire the geometry into layouts**

In `crates/game/src/layout.rs`: add `use crate::geometry::WorldGeometry;`, change the signature to
```rust
/// `geo` is the world's geometry (`WorldGeometry::from_world`), built once per game.
pub fn build_layout(w: &World, map: &AreaMap, vis: &Visibility, you: &str, geo: Option<&WorldGeometry>) -> Layout {
```
and replace Task 1's `geometry: None,` with `geometry: geo.map(|g| g.visible(w, vis)),`.

In `crates/game/src/game.rs`: add `use crate::geometry::WorldGeometry;`; add a field to `Game` after `by_area`:
```rust
    /// The diagram, read once from the world.
    geometry: Option<WorldGeometry>,
```
in `from_sim`, after `let holders = ...;` add `let geometry = WorldGeometry::from_world(w);` and put `geometry,` in the struct literal after `by_area,`; in `layout_of` and `resync` pass `self.geometry.as_ref()` as the new last argument of `build_layout`.

- [ ] **Step 7: Train rows**

In `crates/game/src/view.rs`, replace the imports with
```rust
use std::collections::BTreeMap;

use protocol::{Held, PointsPos, PointsView, RouteState, RouteView, SectionView, TrainRow, TrainState, View, VoteView};
use signalbox_core::ids::SectionId;
use signalbox_core::interlocking::{Owner, RouteState as IlState};
use signalbox_core::points::PointsState;
use signalbox_core::sim::Sim;
use signalbox_core::timetable::{Call, EntryStart};

use crate::areas::Visibility;
```
replace Task 1's `trains: BTreeMap::new(),` in `build_view` with `trains: build_trains(sim, vis),`, and append:
```rust
/// Trains not yet on the railway are listed this long before their entry.
pub const DUE_WINDOW_S: f64 = 30.0 * 60.0;

/// Whole minutes late at `now` against `booked`, as seconds; 0 when early,
/// on time or unbooked. Minutes, so a late train changes its row once a
/// sim minute rather than every tick.
pub fn late_s(now: f64, booked: Option<f64>) -> i64 {
    match booked {
        Some(b) if now > b => ((now - b) / 60.0).floor() as i64 * 60,
        _ => 0,
    }
}

fn row(call: Option<&Call>, now: f64, state: TrainState) -> TrainRow {
    let booked = call.and_then(|c| c.arr_s.or(c.dep_s));
    TrainRow {
        next_place: call.map(|c| c.place.clone()),
        next_platform: call.and_then(|c| c.platform.clone()),
        booked,
        late_s: late_s(now, booked),
        state,
    }
}

/// The train list (spec D1 §4.2), from sim state only. A player sees every
/// train on their visible track, or whose next call is at a platform in
/// their area, or that is due to enter at a boundary of their area within
/// `DUE_WINDOW_S` (or is waiting there); a spectator sees every train
/// running and every one due within the window. A headcode shown twice
/// keeps its first row (running trains in sim order, then entries).
pub fn build_trains(sim: &Sim, vis: &Visibility) -> BTreeMap<String, TrainRow> {
    let w = sim.world();
    let net = &w.net;
    let now = sim.now_s();
    let mut visible = vec![false; net.sections.len()];
    for s in &vis.sections {
        visible[s.idx()] = true;
    }
    let in_area = |sec: SectionId| vis.area.is_none_or(|a| net.sections[sec.idx()].area == a);
    let platform_in_area = |c: &Call| {
        c.platform.as_deref().is_some_and(|pf| {
            net.platforms.iter().any(|p| p.place == c.place && p.platform == pf && in_area(net.segments[p.segment.idx()].section))
        })
    };
    let mut rows: BTreeMap<String, TrainRow> = BTreeMap::new();
    for t in sim.trains() {
        let call = w.services[t.service.idx()].calls.get(t.next_call);
        let on_visible = t.path.iter().any(|(s, _)| visible[net.segments[s.idx()].section.idx()]);
        if !(vis.area.is_none() || on_visible || call.is_some_and(platform_in_area)) {
            continue;
        }
        let head_in_area = t.path.back().is_some_and(|(s, _)| in_area(net.segments[s.idx()].section));
        let state = if t.dwell.is_some() {
            TrainState::AtPlatform
        } else if head_in_area {
            TrainState::InArea
        } else {
            TrainState::Approaching
        };
        rows.entry(t.headcode.clone()).or_insert_with(|| row(call, now, state));
    }
    let waiting = sim.pending_entries().iter().map(|p| p.entry);
    let coming = (sim.next_entry()..w.entries.len()).take_while(|&i| w.entries[i].time_s <= now + DUE_WINDOW_S);
    for i in waiting.chain(coming) {
        let e = &w.entries[i];
        let sec = match e.start {
            EntryStart::Boundary(n) => net.segments[net.nodes[n.idx()].segments[0].idx()].section,
            EntryStart::At(p) => net.segments[p.segment.idx()].section,
        };
        if !in_area(sec) {
            continue;
        }
        let svc = &w.services[e.service.idx()];
        rows.entry(svc.headcode.clone()).or_insert_with(|| row(svc.calls.first(), now, TrainState::Due));
    }
    rows
}
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `scripts/cargo test -p signalbox-game`
Expected: PASS, including `geometry` (6 tests), `trains` (5), `game` (the consistency test now also sees trains) and `layout`.
Run: `scripts/cargo test -p signalbox-core --test timetable`
Expected: PASS (7 tests).
Run: `scripts/cargo test -p signalbox-bot --test soak`
Expected: PASS (`liverpool_street_twenty_minutes_with_two_bots`, `liverpool_street_save_and_resume_match_an_uninterrupted_run`; the 3-hour one is ignored). The bots' views and layouts, now with trains and geometry, still equal the game's at every check.
Run: `scripts/cargo build -p signalbox-server --features dev-auth --all-targets`
Expected: no errors, no warnings.

- [ ] **Step 9: Commit**

```bash
git add crates/core crates/game crates/bot/tests/soak.rs
git commit -m "feat(game): diagram geometry per player and train rows from sim state"
```

---
### Task 3: `client-core` — transport, connection state machine, lobby and game state

**Files:**
- Modify: `Cargo.toml` (member `crates/client-core`; `rand` without default features)
- Modify: `crates/server/Cargo.toml` (`rand` with `thread_rng`)
- Modify: `crates/bot/Cargo.toml`, `crates/bot/src/lib.rs` (feature `net`; `Bot::request_resync`), `crates/bot/tests/bot.rs`
- Modify: `scripts/ci/test.sh` (build `signalbox-bot` without default features; keep mode 100755)
- Create: `crates/client-core/Cargo.toml`, `crates/client-core/src/{lib,transport,app,log,text}.rs`
- Test: `crates/client-core/tests/common/mod.rs`, `crates/client-core/tests/app.rs`, `crates/client-core/tests/text.rs`

**Interfaces:**
- Consumes: `bot::Bot` (`new`, `receive(ServerMsg) -> Option<ClientMsg>`, `layout`, `view`, `area`, `resyncs`, `take_notices`); `protocol::{ClientFrame, ServerFrame, LobbyMsg, LobbyReply, ClientMsg, ServerMsg, Notice, PlayerCommand, Proposal, GameInfo, LayoutInfo, Layout, View, VoteView, ExitName, PointsPos, Rejection}`; Task 1's view types.
- Produces (crate `signalbox-client-core`, lib `client_core`):
  - `transport::ConnState { Closed (default), Connecting, Open, Unauthorized }`; `trait Transport { fn connect(&mut self); fn state(&self) -> ConnState; fn send(&mut self, text: String); fn poll(&mut self) -> Vec<String>; }`; `MemTransport::new() -> (MemTransport, MemHandle)`; `MemHandle::{open, close, unauthorized, push(ServerFrame), push_text(&str), take_sent() -> Vec<ClientFrame>, connects() -> usize}`.
  - `app::{FIRST_BACKOFF_S = 0.5, MAX_BACKOFF_S = 10.0, FLASH_S = 2.0}`, `backoff_s(attempt: u32) -> f64`, `entrance_of(&PlayerCommand) -> Option<&str>`.
  - `app::Link { Connecting, Open, Waiting { retry_at: f64 }, LoginNeeded, Replaced }` (`Clone, Copy, Debug, PartialEq`).
  - `app::InGame { pub id: String, pub you: String, .. }` with `layout() -> Option<&Layout>`, `view() -> Option<&View>`, `layout_gen() -> u64`, `area() -> Option<&str>`, `selected() -> Option<&str>`, `flashing() -> Option<&str>`, `log() -> &Log`, `resyncs() -> usize`. Its fields are `pub(crate)`: `bot`, `layout_gen`, `selected: Option<String>`, `flash: Option<(String, f64)>`, `log`.
  - `app::App`: `new(Box<dyn Transport>, now: f64) -> App` (connects at once), `tick(now)`, `link()`, `wants_login()`, `banner() -> Option<String>`, `reconnect_now()`, lobby `games()`, `layouts()`, `lobby_note()`, `refresh()`, `create_game(&str, Option<u64>, Option<String>)`, `join(&str)`, `leave()`, game `game() -> Option<&InGame>`, `claim(&str)`, `release()`, `vote(Proposal)`, `command(PlayerCommand)`. Fields are `pub(crate)` (Task 4 adds an `impl App` in another module); `pub(crate) fn send(&mut self, ClientFrame) -> bool` and `pub(crate) fn send_game(&mut self, ClientMsg)`.
  - `log::{Log, LogEntry { sim_time: Option<f64>, text: String, alarm: bool }, LOG_CAP = 200}`; `Log::{push(Option<f64>, String, bool), entries() (oldest first, double-ended), len, is_empty}`.
  - `text::{fmt_hms(f64) -> String, exit_text(&ExitName) -> &str, pos_text(PointsPos) -> &'static str, command_text(&PlayerCommand) -> String, rejection_text(Rejection) -> &'static str, notice_text(&Notice) -> (String, bool), proposal_text(Proposal) -> String, vote_text(&VoteView) -> String}`.
  - `bot::Bot::request_resync(&mut self) -> Option<ClientMsg>`; `signalbox-bot` feature `net` (default).

- [ ] **Step 1: Make the shared crates build for the browser**

`bot` without tokio: in `crates/bot/Cargo.toml` replace the `[dependencies]` section with
```toml
[features]
default = ["net"]
# The WebSocket client and the network player (tokio). The browser client
# uses `Bot` alone, without it.
net = ["dep:futures-util", "dep:thiserror", "dep:tokio", "dep:tokio-tungstenite"]

[dependencies]
signalbox-protocol = { path = "../protocol" }
futures-util = { workspace = true, optional = true }
thiserror = { workspace = true, optional = true }
tokio = { workspace = true, features = ["net", "io-util", "time", "macros"], optional = true }
tokio-tungstenite = { workspace = true, optional = true }
```
(keep `[dev-dependencies]` as it is). In `crates/bot/src/lib.rs` replace the module list and its doc comment with
```rust
//! A headless signalbox client (spec §2.1): keeps the layout and view a game
//! sends, applies deltas, and asks for one resync when a delta goes missing.
//! In C1 it talks to an in-process `Game`; `net` puts a WebSocket in between.
//! Without the `net` feature this is `Bot` and `Greedy` only (no tokio), as
//! the browser client uses it.

#[cfg(feature = "net")]
pub mod net;
#[cfg(feature = "net")]
pub mod play;
pub mod strategy;
```

`rand` without OS randomness (getrandom does not build for `wasm32-unknown-unknown`): in the root `Cargo.toml` replace `rand = "0.9"` with
```toml
# No default features: the sim needs no OS randomness, and getrandom does not
# build for wasm32-unknown-unknown (the browser client). The server adds `thread_rng`.
rand = { version = "0.9", default-features = false, features = ["std"] }
```
and in `crates/server/Cargo.toml` replace `rand.workspace = true` with `rand = { workspace = true, features = ["thread_rng"] }`.

Add `"crates/client-core"` to the workspace `members` (one member per line from now on, in the order they are listed today, `client-core` last).

In `scripts/ci/test.sh`, after the `cargo test --workspace` line, add:
```bash
# The browser client's view of the bot: Bot and Greedy without tokio.
cargo build -p signalbox-bot --no-default-features --locked --offline
```
(edit in place; `git ls-files -s scripts/ci/test.sh` must still show mode `100755`).

- [ ] **Step 2: Write the failing tests**

Append to `crates/bot/tests/bot.rs`:
```rust
#[test]
fn an_outside_resync_request_is_sent_once_until_the_next_view() {
    let mut b = Bot::new();
    b.receive(ServerMsg::View(view(1)));
    assert_eq!(b.request_resync(), Some(ClientMsg::Resync));
    assert_eq!(b.request_resync(), None, "one is on its way");
    assert_eq!(b.receive(delta(2, Aspect::Green)), None, "deltas wait for the view");
    assert_eq!(b.view().unwrap().seq, 1);
    b.receive(ServerMsg::View(view(3)));
    assert_eq!(b.request_resync(), Some(ClientMsg::Resync));
    assert_eq!(b.resyncs(), 2);
}
```

Create `crates/client-core/Cargo.toml`:
```toml
[package]
name = "signalbox-client-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "client_core"
path = "src/lib.rs"

[dependencies]
signalbox-protocol = { path = "../protocol" }
signalbox-bot = { path = "../bot", default-features = false }
```

Create `crates/client-core/tests/common/mod.rs`:
```rust
//! Helpers for the client-core tests.

#![allow(dead_code)]

use client_core::{App, MemHandle, MemTransport};

pub fn s(x: &str) -> String {
    x.to_string()
}

/// An app whose first connection is open, its lobby requests taken.
pub fn open_app() -> (App, MemHandle) {
    let (t, h) = MemTransport::new();
    let mut app = App::new(Box::new(t), 0.0);
    h.open();
    app.tick(0.0);
    h.take_sent();
    (app, h)
}
```

Create `crates/client-core/tests/app.rs`:
```rust
//! The connection, the lobby and the game state (spec D1 §2, §5) against
//! an in-memory transport.

mod common;

use std::collections::BTreeMap;

use client_core::app::{FIRST_BACKOFF_S, MAX_BACKOFF_S, backoff_s};
use client_core::log::LOG_CAP;
use client_core::{App, Link, MemTransport};
use common::*;
use protocol::*;

fn lobby(m: LobbyMsg) -> ClientFrame {
    ClientFrame::Lobby(m)
}

fn view(seq: u64) -> ServerFrame {
    ServerFrame::Game(ServerMsg::View(View {
        seq,
        sim_time: 25_200.0,
        speed: 1,
        paused: false,
        vote: None,
        holders: BTreeMap::new(),
        score: None,
        signals: BTreeMap::new(),
        routes: BTreeMap::new(),
        points: BTreeMap::new(),
        sections: BTreeMap::new(),
        berths: BTreeMap::new(),
        trains: BTreeMap::new(),
    }))
}

fn delta(seq: u64) -> ServerFrame {
    ServerFrame::Game(ServerMsg::Delta(Delta { seq, sim_time: Some(25_200.0 + seq as f64), ..Delta::default() }))
}

fn joined(game: &str) -> ServerFrame {
    ServerFrame::Lobby(LobbyReply::Joined { game: s(game), you: s("ann") })
}

fn notice(n: Notice) -> ServerFrame {
    ServerFrame::Game(ServerMsg::Notice(n))
}

/// An open app inside game `g-one` with a first view.
fn in_game() -> (App, client_core::MemHandle) {
    let (mut app, h) = open_app();
    h.push(joined("g-one"));
    h.push(view(1));
    app.tick(1.0);
    (app, h)
}

#[test]
fn it_connects_at_once_and_asks_for_the_lobby_when_open() {
    let (t, h) = MemTransport::new();
    let mut app = App::new(Box::new(t), 0.0);
    assert_eq!((h.connects(), app.link()), (1, Link::Connecting));
    assert_eq!(app.banner().as_deref(), Some("Connecting…"));
    app.tick(0.1);
    assert!(h.take_sent().is_empty(), "nothing is sent before the socket opens");
    h.open();
    app.tick(0.2);
    assert_eq!(app.link(), Link::Open);
    assert_eq!(app.banner(), None);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::ListLayouts), lobby(LobbyMsg::ListGames)]);
}

#[test]
fn the_lobby_lists_games_and_layouts_and_sends_what_you_ask() {
    let (mut app, h) = open_app();
    let info = GameInfo {
        id: s("g-one"),
        layout: s("twobox"),
        state: GameState::Running,
        sim_time: 25_200.0,
        areas: vec![AreaHolder { name: s("West"), holder: None }],
        players: vec![s("bob")],
        error: None,
    };
    h.push(ServerFrame::Lobby(LobbyReply::Games { games: vec![info.clone()] }));
    h.push(ServerFrame::Lobby(LobbyReply::Layouts { layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West")] }] }));
    app.tick(1.0);
    assert_eq!(app.games(), [info]);
    assert_eq!(app.layouts()[0].name, "twobox");
    app.refresh();
    app.create_game("twobox", Some(5), Some(s("07:30")));
    app.join("g-one");
    assert_eq!(
        h.take_sent(),
        [
            lobby(LobbyMsg::ListGames),
            lobby(LobbyMsg::CreateGame { layout: s("twobox"), seed: Some(5), start: Some(s("07:30")) }),
            lobby(LobbyMsg::Join { game: s("g-one") }),
        ]
    );
    h.push(ServerFrame::error(codes::UNKNOWN_LAYOUT, "no layout `x`"));
    app.tick(2.0);
    assert_eq!(app.lobby_note(), Some("no layout `x`"));
}

#[test]
fn joined_puts_you_in_the_game_and_leave_takes_you_out() {
    let (mut app, h) = in_game();
    let g = app.game().unwrap();
    assert_eq!((g.id.as_str(), g.you.as_str(), g.view().unwrap().seq), ("g-one", "ann", 1));
    h.push(delta(2));
    app.tick(2.0);
    assert_eq!(app.game().unwrap().view().unwrap().seq, 2);
    app.claim("West");
    app.vote(Proposal::Speed { x: 4 });
    app.release();
    app.leave();
    assert!(app.game().is_none());
    assert_eq!(
        h.take_sent(),
        [
            ClientFrame::Game(ClientMsg::Claim { area: s("West") }),
            ClientFrame::Game(ClientMsg::Vote { proposal: Proposal::Speed { x: 4 } }),
            ClientFrame::Game(ClientMsg::Release),
            lobby(LobbyMsg::Leave),
        ]
    );
    h.push(view(3));
    app.tick(3.0);
    assert!(app.game().is_none(), "game frames after leaving are dropped");
}

#[test]
fn backoff_doubles_from_half_a_second_to_ten() {
    let waits: Vec<f64> = (0..8).map(backoff_s).collect();
    assert_eq!(waits, [0.5, 1.0, 2.0, 4.0, 8.0, 10.0, 10.0, 10.0]);
    assert_eq!((FIRST_BACKOFF_S, MAX_BACKOFF_S), (0.5, 10.0));
    assert_eq!(backoff_s(u32::MAX), 10.0);
}

/// Spec D1 §5: a lost connection shows a banner, retries with backoff,
/// and on reconnect rejoins with exactly one `join` — its layout and view
/// are the one resync. The game state and log survive meanwhile.
#[test]
fn a_lost_connection_backs_off_and_rejoins_once() {
    let (mut app, h) = in_game();
    h.push(notice(Notice::AreaTaken { area: s("East"), holder: s("bob") }));
    app.tick(1.5);
    h.close();
    app.tick(2.0);
    assert_eq!(app.link(), Link::Waiting { retry_at: 2.5 });
    assert_eq!(app.banner().as_deref(), Some("Connection lost. Reconnecting in 1 s…"));
    let mut t = 2.0;
    for want in [0.5, 1.0, 2.0, 4.0, 8.0, 10.0, 10.0] {
        let Link::Waiting { retry_at } = app.link() else { panic!("{:?}", app.link()) };
        assert_eq!(retry_at - t, want);
        let before = h.connects();
        app.tick(retry_at - 0.01);
        assert_eq!(h.connects(), before, "not before the wait is over");
        t = retry_at;
        app.tick(t);
        assert_eq!((h.connects(), app.link()), (before + 1, Link::Connecting));
        assert_eq!(app.banner().as_deref(), Some("Reconnecting…"));
        h.close();
        app.tick(t);
    }
    assert!(app.game().is_some(), "the game is kept, drawn stale under the banner");
    let Link::Waiting { retry_at } = app.link() else { panic!() };
    t = retry_at;
    app.tick(t);
    h.open();
    app.tick(t + 0.1);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::Join { game: s("g-one") })], "one join, no resync, no lobby lists");
    h.push(joined("g-one"));
    h.push(view(40));
    app.tick(t + 0.2);
    let g = app.game().unwrap();
    assert_eq!((g.view().unwrap().seq, g.resyncs(), g.log().len()), (40, 0, 1));
    h.close();
    app.tick(t + 0.3);
    assert_eq!(app.link(), Link::Waiting { retry_at: t + 0.3 + 0.5 }, "a connection that carried frames resets the backoff");
}

#[test]
fn a_connection_that_opens_and_closes_without_a_frame_keeps_backing_off() {
    let (mut app, h) = open_app();
    h.close();
    app.tick(1.0);
    app.tick(1.5);
    h.open();
    app.tick(1.6);
    h.close();
    app.tick(1.7);
    assert_eq!(app.link(), Link::Waiting { retry_at: 2.7 }, "second wait is 1 s");
}

#[test]
fn a_failed_rejoin_goes_back_to_the_lobby_with_the_reason() {
    let (mut app, h) = in_game();
    h.close();
    app.tick(2.0);
    app.tick(2.5);
    h.open();
    app.tick(2.6);
    h.take_sent();
    h.push(ServerFrame::error(codes::UNKNOWN_GAME, "no game `g-one`"));
    app.tick(2.7);
    assert!(app.game().is_none());
    assert_eq!(app.lobby_note(), Some("Could not rejoin the game: no game `g-one`"));
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::ListGames)]);
    h.close();
    app.tick(3.0);
    app.tick(3.5);
    h.open();
    app.tick(3.6);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::ListLayouts), lobby(LobbyMsg::ListGames)], "no rejoin any more");
}

#[test]
fn an_expired_session_asks_for_a_login_and_stops_retrying() {
    let (t, h) = MemTransport::new();
    let mut app = App::new(Box::new(t), 0.0);
    h.unauthorized();
    app.tick(0.5);
    assert!(app.wants_login());
    assert_eq!(app.banner().as_deref(), Some("Your session has expired. Signing in again…"));
    app.tick(100.0);
    assert_eq!(h.connects(), 1);
}

#[test]
fn replaced_by_another_tab_stays_down_until_asked() {
    let (mut app, h) = in_game();
    h.push(notice(Notice::Replaced));
    app.tick(2.0);
    h.close();
    app.tick(3.0);
    app.tick(60.0);
    assert_eq!((app.link(), h.connects()), (Link::Replaced, 1));
    assert_eq!(app.banner().as_deref(), Some("This login is now open in another tab or window."));
    app.reconnect_now();
    assert_eq!((app.link(), h.connects()), (Link::Connecting, 2));
    h.open();
    app.tick(61.0);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::Join { game: s("g-one") })]);
}

#[test]
fn a_crashed_game_returns_to_the_lobby_with_a_banner() {
    let (mut app, h) = in_game();
    h.push(notice(Notice::GameCrashed));
    h.push(view(2));
    app.tick(2.0);
    assert!(app.game().is_none());
    assert_eq!(app.lobby_note(), Some("The game stopped unexpectedly. Join it again to resume it."));
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::ListGames)]);
}

#[test]
fn an_unreadable_frame_is_logged_and_asks_for_one_resync() {
    let (mut app, h) = in_game();
    h.push_text("{\"type\": \"view\", \"seq\": \"soon\"}");
    h.push_text("not json at all");
    app.tick(2.0);
    assert_eq!(h.take_sent(), [ClientFrame::Game(ClientMsg::Resync)], "one resync for both");
    let g = app.game().unwrap();
    assert_eq!(g.log().len(), 2);
    assert!(g.log().entries().all(|e| e.text.starts_with("Unreadable message from the server")));
    h.push(view(9));
    h.push(delta(11));
    app.tick(3.0);
    assert_eq!(h.take_sent(), [ClientFrame::Game(ClientMsg::Resync)], "a gap after the new view asks again");
}

/// Spec D1 §1 criterion 3: nothing the server sends can panic the client.
#[test]
fn hostile_frames_never_panic() {
    let junk = [
        "",
        "null",
        "[]",
        "{}",
        "{\"type\": 7}",
        "{\"type\": \"layout\"}",
        "{\"type\": \"view\", \"seq\": -1}",
        "{\"type\": \"delta\", \"seq\": 18446744073709551615}",
        "{\"type\": \"delta\", \"seq\": 2, \"signals\": {\"nowhere\": \"red\"}, \"trains\": {\"x\": null}}",
        "{\"type\": \"notice\", \"kind\": \"rejected\"}",
        "{\"type\": \"notice\", \"kind\": \"late\", \"train\": \"1A01\", \"place\": \"X\", \"platform\": \"1\", \"late_s\": -9223372036854775808}",
        "{\"type\": \"joined\", \"game\": \"\", \"you\": \"\"}",
        "{\"type\": \"games\", \"games\": [{\"id\": 1}]}",
        "{\"type\": \"error\"}",
        "\u{0}\u{feff}{",
    ];
    let (mut app, h) = open_app();
    for j in junk {
        h.push_text(j);
    }
    app.tick(1.0);
    let (mut app2, h2) = in_game();
    for j in junk {
        h2.push_text(j);
    }
    app2.tick(2.0);
    h.push(joined("g-two"));
    h.push(view(1));
    app.tick(3.0);
    assert_eq!(app.game().unwrap().view().unwrap().seq, 1, "still works afterwards");
}

#[test]
fn notices_are_logged_with_alarms_and_the_log_is_bounded() {
    let (mut app, h) = in_game();
    h.push(notice(Notice::Spad { signal: s("A"), train: s("1A01") }));
    h.push(notice(Notice::Handover { headcode: s("2W03"), from_area: s("East") }));
    app.tick(2.0);
    let lines: Vec<(String, bool, Option<f64>)> =
        app.game().unwrap().log().entries().map(|e| (e.text.clone(), e.alarm, e.sim_time)).collect();
    assert_eq!(
        lines,
        [
            (s("SPAD: 1A01 passed A at danger"), true, Some(25_200.0)),
            (s("2W03 offered from East"), false, Some(25_200.0)),
        ]
    );
    for i in 0..(LOG_CAP + 50) {
        h.push(notice(Notice::Collision { section: format!("T{i}") }));
    }
    app.tick(3.0);
    let log = app.game().unwrap().log();
    assert_eq!(log.len(), LOG_CAP);
    assert_eq!(log.entries().last().unwrap().text, format!("COLLISION on T{}", LOG_CAP + 49));
}

#[test]
fn commands_without_a_connection_are_not_sent_and_say_so() {
    let (mut app, h) = in_game();
    h.close();
    app.tick(2.0);
    app.command(PlayerCommand::CancelRoute { entrance: s("A") });
    assert!(h.take_sent().is_empty());
    let last = app.game().unwrap().log().entries().last().unwrap().clone();
    assert_eq!((last.text.as_str(), last.alarm), ("Not connected: nothing was sent", true));
}

/// A reload or second tab: the front hands the new socket the old one's
/// game and says `joined` before the client asks for anything.
#[test]
fn an_unasked_joined_enters_the_game() {
    let (mut app, h) = open_app();
    h.push(joined("g-one"));
    h.push(view(7));
    app.tick(1.0);
    assert_eq!(app.game().unwrap().view().unwrap().seq, 7);
    assert!(h.take_sent().is_empty(), "nothing to ask for");
    h.close();
    app.tick(2.0);
    app.tick(2.5);
    h.open();
    app.tick(2.6);
    assert_eq!(h.take_sent(), [lobby(LobbyMsg::Join { game: s("g-one") })], "and it is the game to rejoin");
}
```

Create `crates/client-core/tests/text.rs`:
```rust
//! Words on the screen.

use client_core::text::*;
use protocol::*;

fn s(x: &str) -> String {
    x.to_string()
}

#[test]
fn clock_times() {
    assert_eq!(fmt_hms(25_215.9), "07:00:15");
    assert_eq!(fmt_hms(0.0), "00:00:00");
    assert_eq!(fmt_hms(86_399.0), "23:59:59");
    assert_eq!(fmt_hms(86_400.0 + 61.0), "00:01:01", "wraps at midnight");
    assert_eq!(fmt_hms(f64::NAN), "00:00:00");
    assert_eq!(fmt_hms(-5.0), "00:00:00");
}

#[test]
fn commands_and_refusals() {
    let c = PlayerCommand::SetRoute { entrance: s("39,1V1"), exit: ExitName::Node(s("N12")) };
    assert_eq!(command_text(&c), "set route 39,1V1 to N12");
    assert_eq!(
        notice_text(&Notice::Rejected { cmd: c, reason: Rejection::PointsLocked }),
        (s("Refused: set route 39,1V1 to N12 (points locked)"), true)
    );
    assert_eq!(
        command_text(&PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse }),
        "swing P reverse"
    );
    assert_eq!(
        notice_text(&Notice::Late { train: s("1A01"), place: s("EST"), platform: s("1"), late_s: 125 }),
        (s("1A01 at EST 1, 2 min late"), false)
    );
    assert_eq!(notice_text(&Notice::Error { code: s("bad_speed"), message: s("no") }), (s("Error: no"), true));
}

#[test]
fn votes() {
    let v = VoteView { proposal: Proposal::Speed { x: 4 }, agreed: vec![s("ann"), s("bob")], expires_in_s: 25 };
    assert_eq!(vote_text(&v), "Vote: 4× — ann, bob agreed, 25 s left");
    assert_eq!(proposal_text(Proposal::Pause), "pause");
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `scripts/cargo test -p signalbox-client-core`
Expected: FAIL to compile — `can't find crate client_core` / no `src/lib.rs`.
Run: `scripts/cargo test -p signalbox-bot --test bot`
Expected: FAIL to compile — no method `request_resync`.

- [ ] **Step 4: `Bot::request_resync`**

In `crates/bot/src/lib.rs`, add to `impl Bot` after `take_notices`:
```rust
    /// Ask for a resync from outside (e.g. after a frame that would not
    /// parse): the `Resync` to send, or `None` if one is already on its way.
    pub fn request_resync(&mut self) -> Option<ClientMsg> {
        if self.awaiting_resync {
            return None;
        }
        self.awaiting_resync = true;
        self.resyncs += 1;
        Some(ClientMsg::Resync)
    }
```

- [ ] **Step 5: The transport**

Create `crates/client-core/src/transport.rs`:
```rust
//! How the client talks to the front: text frames over something that can
//! connect, fail and reconnect. The browser shell implements it over a
//! WebSocket; `MemTransport` is an in-memory one for tests.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use protocol::{ClientFrame, ServerFrame};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConnState {
    /// Never connected, or the last connection ended.
    #[default]
    Closed,
    Connecting,
    Open,
    /// The front refused us for want of a session (HTTP 401): log in again.
    Unauthorized,
}

/// A connection to the front's `/ws`. Every method returns at once; the
/// app polls from its frame loop.
pub trait Transport {
    /// Open a new connection, dropping any old one and anything unread.
    fn connect(&mut self);
    fn state(&self) -> ConnState;
    /// Send one text frame; dropped unless the connection is open.
    fn send(&mut self, text: String);
    /// Text frames received since the last call, oldest first.
    fn poll(&mut self) -> Vec<String>;
}

#[derive(Debug, Default)]
struct Mem {
    state: ConnState,
    inbox: VecDeque<String>,
    sent: Vec<String>,
    connects: usize,
}

/// An in-memory transport. The test holds the `MemHandle` and plays the
/// front: it opens and closes the connection, pushes frames, and reads what
/// the app sent.
pub struct MemTransport(Rc<RefCell<Mem>>);

#[derive(Clone)]
pub struct MemHandle(Rc<RefCell<Mem>>);

impl MemTransport {
    pub fn new() -> (MemTransport, MemHandle) {
        let m = Rc::new(RefCell::new(Mem::default()));
        (MemTransport(m.clone()), MemHandle(m))
    }
}

impl Transport for MemTransport {
    fn connect(&mut self) {
        let mut m = self.0.borrow_mut();
        m.connects += 1;
        m.state = ConnState::Connecting;
        m.inbox.clear();
    }

    fn state(&self) -> ConnState {
        self.0.borrow().state
    }

    fn send(&mut self, text: String) {
        let mut m = self.0.borrow_mut();
        if m.state == ConnState::Open {
            m.sent.push(text);
        }
    }

    fn poll(&mut self) -> Vec<String> {
        self.0.borrow_mut().inbox.drain(..).collect()
    }
}

impl MemHandle {
    pub fn open(&self) {
        self.0.borrow_mut().state = ConnState::Open;
    }

    pub fn close(&self) {
        self.0.borrow_mut().state = ConnState::Closed;
    }

    pub fn unauthorized(&self) {
        self.0.borrow_mut().state = ConnState::Unauthorized;
    }

    pub fn push(&self, f: ServerFrame) {
        self.push_text(&f.to_json());
    }

    /// Any text, e.g. a malformed frame.
    pub fn push_text(&self, text: &str) {
        self.0.borrow_mut().inbox.push_back(text.to_string());
    }

    /// Everything the app sent since the last call, parsed.
    pub fn take_sent(&self) -> Vec<ClientFrame> {
        let sent: Vec<String> = self.0.borrow_mut().sent.drain(..).collect();
        sent.iter().map(|t| ClientFrame::from_json(t).expect("the app sends valid frames")).collect()
    }

    /// How many times the app called `connect`.
    pub fn connects(&self) -> usize {
        self.0.borrow().connects
    }
}
```

- [ ] **Step 6: The log and the words**

Create `crates/client-core/src/log.rs`:
```rust
//! The alarms and event log: a bounded list of lines, newest last.

use std::collections::VecDeque;

/// Lines kept; older ones fall off.
pub const LOG_CAP: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub struct LogEntry {
    /// Sim time when it arrived, if a view was held.
    pub sim_time: Option<f64>,
    pub text: String,
    pub alarm: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Log {
    entries: VecDeque<LogEntry>,
}

impl Log {
    pub fn push(&mut self, sim_time: Option<f64>, text: String, alarm: bool) {
        if self.entries.len() == LOG_CAP {
            self.entries.pop_front();
        }
        self.entries.push_back(LogEntry { sim_time, text, alarm });
    }

    /// Oldest first.
    pub fn entries(&self) -> impl DoubleEndedIterator<Item = &LogEntry> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
```

Create `crates/client-core/src/text.rs`:
```rust
//! Words for the screen: times, commands, refusals, notices, votes.

use protocol::{ExitName, Notice, PlayerCommand, PointsPos, Proposal, Rejection, VoteView};

/// Seconds since midnight as `HH:MM:SS` (wrapping at 24 h; bad input is 00:00:00).
pub fn fmt_hms(s: f64) -> String {
    let t = if s.is_finite() && s >= 0.0 { s.floor() as u64 % 86_400 } else { 0 };
    format!("{:02}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60)
}

pub fn exit_text(e: &ExitName) -> &str {
    match e {
        ExitName::Signal(s) | ExitName::Node(s) => s,
    }
}

pub fn pos_text(p: PointsPos) -> &'static str {
    match p {
        PointsPos::Normal => "normal",
        PointsPos::Reverse => "reverse",
    }
}

pub fn command_text(c: &PlayerCommand) -> String {
    match c {
        PlayerCommand::SetRoute { entrance, exit } => format!("set route {entrance} to {}", exit_text(exit)),
        PlayerCommand::CancelRoute { entrance } => format!("cancel route from {entrance}"),
        PlayerCommand::SetAutoWorking { entrance, on } => {
            format!("auto-working {} at {entrance}", if *on { "on" } else { "off" })
        }
        PlayerCommand::SwingPoints { points, to } => format!("swing {points} {}", pos_text(*to)),
        PlayerCommand::Interpose { berth, headcode } => format!("interpose {headcode} in {berth}"),
        PlayerCommand::CancelBerth { berth } => format!("cancel berth {berth}"),
    }
}

pub fn rejection_text(r: Rejection) -> &'static str {
    match r {
        Rejection::UnknownId => "unknown name",
        Rejection::NoSuchRoute => "no such route",
        Rejection::AlreadySet => "already set",
        Rejection::ConflictingRoute => "conflicts with a route already set",
        Rejection::PointsLocked => "points locked",
        Rejection::PointsOccupied => "points occupied",
        Rejection::RouteNotSet => "no route set",
        Rejection::RouteIsAutomatic => "route is automatic",
        Rejection::NotPoints => "not points",
    }
}

/// A notice as one log line, and whether it is an alarm.
pub fn notice_text(n: &Notice) -> (String, bool) {
    match n {
        Notice::Rejected { cmd, reason } => (format!("Refused: {} ({})", command_text(cmd), rejection_text(*reason)), true),
        Notice::NotYourArea { area } => (format!("Not your area: that is in {area}"), true),
        Notice::Spad { signal, train } => (format!("SPAD: {train} passed {signal} at danger"), true),
        Notice::Collision { section } => (format!("COLLISION on {section}"), true),
        Notice::Late { train, place, platform, late_s } => {
            (format!("{train} at {place} {platform}, {} min late", late_s / 60), false)
        }
        Notice::WrongPlatform { train, place, platform, expected } => {
            (format!("{train} at {place} platform {platform}, booked {expected}"), true)
        }
        Notice::Handover { headcode, from_area } => (format!("{headcode} offered from {from_area}"), false),
        Notice::AreaTaken { area, holder } => (format!("{area} is now {holder}'s"), false),
        Notice::Replaced => ("This login was opened somewhere else".to_string(), true),
        Notice::GameCrashed => ("The game stopped unexpectedly".to_string(), true),
        Notice::Error { message, .. } => (format!("Error: {message}"), true),
    }
}

pub fn proposal_text(p: Proposal) -> String {
    match p {
        Proposal::Pause => "pause".to_string(),
        Proposal::Resume => "resume".to_string(),
        Proposal::Speed { x } => format!("{x}×"),
    }
}

/// `Vote: 4× — ann, bob agreed, 25 s left`
pub fn vote_text(v: &VoteView) -> String {
    format!("Vote: {} — {} agreed, {} s left", proposal_text(v.proposal), v.agreed.join(", "), v.expires_in_s)
}
```

- [ ] **Step 7: The app**

Create `crates/client-core/src/app.rs`:
```rust
//! The client's state (spec D1 §2, §5): the connection with its reconnect
//! backoff, the lobby, and the game you are in (layout and view kept by a
//! `bot::Bot`, the log, the selection). Pure: the shell calls `tick` with
//! its clock every frame and reads the state back; nothing here waits,
//! sleeps or panics on anything the server sends.

use bot::Bot;
use protocol::{
    ClientFrame, ClientMsg, GameInfo, Layout, LayoutInfo, LobbyMsg, LobbyReply, Notice, PlayerCommand, Proposal, ServerFrame,
    ServerMsg, View,
};

use crate::log::Log;
use crate::text::notice_text;
use crate::transport::{ConnState, Transport};

/// The first retry waits this long; each failure doubles it up to `MAX_BACKOFF_S`.
pub const FIRST_BACKOFF_S: f64 = 0.5;
pub const MAX_BACKOFF_S: f64 = 10.0;
/// How long a refused command's entrance signal flashes.
pub const FLASH_S: f64 = 2.0;

/// Seconds to wait before retry number `attempt` (0-based).
pub fn backoff_s(attempt: u32) -> f64 {
    (FIRST_BACKOFF_S * f64::from(1u32 << attempt.min(8))).min(MAX_BACKOFF_S)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Link {
    Connecting,
    Open,
    /// Lost; the next attempt is at `retry_at` on the shell's clock.
    Waiting { retry_at: f64 },
    /// The session has expired: the shell sends the browser to `/auth/login`.
    LoginNeeded,
    /// This login was opened elsewhere; nothing reconnects until `reconnect_now`.
    Replaced,
}

/// The game you are in.
pub struct InGame {
    pub id: String,
    pub you: String,
    pub(crate) bot: Bot,
    pub(crate) layout_gen: u64,
    pub(crate) selected: Option<String>,
    pub(crate) flash: Option<(String, f64)>,
    pub(crate) log: Log,
}

impl InGame {
    fn new(id: String, you: String) -> InGame {
        InGame { id, you, bot: Bot::new(), layout_gen: 0, selected: None, flash: None, log: Log::default() }
    }

    pub fn layout(&self) -> Option<&Layout> {
        self.bot.layout()
    }

    pub fn view(&self) -> Option<&View> {
        self.bot.view()
    }

    /// Bumped by every layout received: redraw caches keyed on it.
    pub fn layout_gen(&self) -> u64 {
        self.layout_gen
    }

    /// The area you hold; `None` while spectating.
    pub fn area(&self) -> Option<&str> {
        self.bot.area()
    }

    /// The chosen entrance signal.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// The signal flashing for a refused command.
    pub fn flashing(&self) -> Option<&str> {
        self.flash.as_ref().map(|(s, _)| s.as_str())
    }

    pub fn log(&self) -> &Log {
        &self.log
    }

    /// Resyncs asked for in this game (gaps and unreadable frames).
    pub fn resyncs(&self) -> usize {
        self.bot.resyncs()
    }

    fn sim_time(&self) -> Option<f64> {
        self.bot.view().map(|v| v.sim_time)
    }
}

pub struct App {
    pub(crate) transport: Box<dyn Transport>,
    pub(crate) link: Link,
    /// Failed attempts since a connection last carried a frame.
    pub(crate) attempt: u32,
    /// A connection was open at least once (so "reconnecting", not "connecting").
    pub(crate) was_open: bool,
    pub(crate) now: f64,
    pub(crate) games: Vec<GameInfo>,
    pub(crate) layouts: Vec<LayoutInfo>,
    /// A message for the lobby screen (a crash, a failed rejoin, an error).
    pub(crate) lobby_note: Option<String>,
    pub(crate) game: Option<InGame>,
    /// The game to rejoin after a reconnect.
    pub(crate) rejoin: Option<String>,
    /// A rejoin was sent and its `joined` has not come yet.
    pub(crate) rejoining: bool,
}

impl App {
    /// Starts connecting at once. `now` is the shell's clock in seconds.
    pub fn new(mut transport: Box<dyn Transport>, now: f64) -> App {
        transport.connect();
        App {
            transport,
            link: Link::Connecting,
            attempt: 0,
            was_open: false,
            now,
            games: Vec::new(),
            layouts: Vec::new(),
            lobby_note: None,
            game: None,
            rejoin: None,
            rejoining: false,
        }
    }

    /// Take what arrived and move the connection on. Call every frame.
    pub fn tick(&mut self, now: f64) {
        self.now = now;
        for text in self.transport.poll() {
            self.receive(&text);
        }
        match (self.link, self.transport.state()) {
            (Link::Replaced | Link::LoginNeeded, _) => {}
            (Link::Connecting, ConnState::Open) => {
                self.link = Link::Open;
                self.was_open = true;
                self.on_open();
            }
            (Link::Connecting | Link::Open, ConnState::Closed) => {
                self.link = Link::Waiting { retry_at: now + backoff_s(self.attempt) };
                self.attempt = self.attempt.saturating_add(1);
            }
            (Link::Connecting | Link::Open, ConnState::Unauthorized) => self.link = Link::LoginNeeded,
            (Link::Waiting { retry_at }, _) if now >= retry_at => {
                self.transport.connect();
                self.link = Link::Connecting;
            }
            _ => {}
        }
        if let Some(g) = self.game.as_mut() {
            if g.flash.as_ref().is_some_and(|(_, until)| now >= *until) {
                g.flash = None;
            }
        }
    }

    pub fn link(&self) -> Link {
        self.link
    }

    /// The shell should send the browser to `/auth/login`.
    pub fn wants_login(&self) -> bool {
        self.link == Link::LoginNeeded
    }

    /// Words for the connection banner; `None` when all is well.
    pub fn banner(&self) -> Option<String> {
        match self.link {
            Link::Open => None,
            Link::Connecting if !self.was_open => Some("Connecting…".into()),
            Link::Connecting => Some("Reconnecting…".into()),
            Link::Waiting { retry_at } => {
                Some(format!("Connection lost. Reconnecting in {} s…", (retry_at - self.now).max(0.0).ceil()))
            }
            Link::LoginNeeded => Some("Your session has expired. Signing in again…".into()),
            Link::Replaced => Some("This login is now open in another tab or window.".into()),
        }
    }

    /// Connect again now (after `Replaced`, or to skip a backoff wait).
    pub fn reconnect_now(&mut self) {
        if matches!(self.link, Link::Replaced | Link::Waiting { .. }) {
            self.attempt = 0;
            self.transport.connect();
            self.link = Link::Connecting;
        }
    }

    fn on_open(&mut self) {
        match self.rejoin.clone() {
            // The full layout and view that follow `joined` are the one resync.
            Some(game) => {
                self.rejoining = true;
                self.send(ClientFrame::Lobby(LobbyMsg::Join { game }));
            }
            None => {
                self.send(ClientFrame::Lobby(LobbyMsg::ListLayouts));
                self.send(ClientFrame::Lobby(LobbyMsg::ListGames));
            }
        }
    }

    pub(crate) fn send(&mut self, f: ClientFrame) -> bool {
        if self.link != Link::Open {
            return false;
        }
        self.transport.send(f.to_json());
        true
    }

    /// Send a game message; logged as an alarm when there is no connection.
    pub(crate) fn send_game(&mut self, m: ClientMsg) {
        if !self.send(ClientFrame::Game(m)) {
            if let Some(g) = self.game.as_mut() {
                let t = g.sim_time();
                g.log.push(t, "Not connected: nothing was sent".into(), true);
            }
        }
    }

    fn to_lobby(&mut self, note: Option<String>) {
        self.game = None;
        self.rejoin = None;
        self.rejoining = false;
        self.lobby_note = note;
        self.send(ClientFrame::Lobby(LobbyMsg::ListGames));
    }

    fn receive(&mut self, text: &str) {
        if self.link == Link::Open {
            self.attempt = 0;
        }
        match ServerFrame::from_json(text) {
            Ok(ServerFrame::Lobby(r)) => self.lobby_reply(r),
            Ok(ServerFrame::Game(m)) => self.game_msg(m),
            Err(e) => match self.game.as_mut() {
                Some(g) => {
                    let t = g.sim_time();
                    g.log.push(t, format!("Unreadable message from the server ({e}); resyncing"), false);
                    if let Some(m) = g.bot.request_resync() {
                        self.send_game(m);
                    }
                }
                None => self.lobby_note = Some(format!("Unreadable message from the server ({e})")),
            },
        }
    }

    fn lobby_reply(&mut self, r: LobbyReply) {
        match r {
            LobbyReply::Games { games } => self.games = games,
            LobbyReply::Layouts { layouts } => self.layouts = layouts,
            LobbyReply::Joined { game, you } => {
                self.rejoining = false;
                self.rejoin = Some(game.clone());
                self.lobby_note = None;
                if self.game.as_ref().is_none_or(|g| g.id != game) {
                    self.game = Some(InGame::new(game, you));
                }
            }
            LobbyReply::Error { message, .. } => {
                if self.rejoining {
                    self.to_lobby(Some(format!("Could not rejoin the game: {message}")));
                } else if let Some(g) = self.game.as_mut() {
                    let t = g.sim_time();
                    g.log.push(t, format!("Error: {message}"), true);
                } else {
                    self.lobby_note = Some(message);
                }
            }
        }
    }

    fn game_msg(&mut self, m: ServerMsg) {
        if m == ServerMsg::Notice(Notice::Replaced) {
            self.link = Link::Replaced;
            if let Some(g) = self.game.as_mut() {
                let t = g.sim_time();
                g.log.push(t, notice_text(&Notice::Replaced).0, true);
            }
            return;
        }
        let Some(g) = self.game.as_mut() else { return };
        match &m {
            ServerMsg::Notice(Notice::GameCrashed) => {
                return self.to_lobby(Some("The game stopped unexpectedly. Join it again to resume it.".into()));
            }
            ServerMsg::Notice(n) => {
                let (text, alarm) = notice_text(n);
                let t = g.sim_time();
                g.log.push(t, text, alarm);
                if let Notice::Rejected { cmd, .. } = n {
                    if let Some(e) = entrance_of(cmd) {
                        g.flash = Some((e.to_string(), self.now + FLASH_S));
                    }
                }
            }
            ServerMsg::Layout(_) => g.layout_gen += 1,
            ServerMsg::View(_) | ServerMsg::Delta(_) => {}
        }
        let reply = g.bot.receive(m);
        g.bot.take_notices();
        if let Some(r) = reply {
            self.send_game(r);
        }
    }

    // ---- lobby ----

    pub fn games(&self) -> &[GameInfo] {
        &self.games
    }

    pub fn layouts(&self) -> &[LayoutInfo] {
        &self.layouts
    }

    pub fn lobby_note(&self) -> Option<&str> {
        self.lobby_note.as_deref()
    }

    pub fn refresh(&mut self) {
        self.send(ClientFrame::Lobby(LobbyMsg::ListGames));
    }

    /// `start` is "HH:MM" or "HH:MM:SS"; the front checks it.
    pub fn create_game(&mut self, layout: &str, seed: Option<u64>, start: Option<String>) {
        self.send(ClientFrame::Lobby(LobbyMsg::CreateGame { layout: layout.to_string(), seed, start }));
    }

    pub fn join(&mut self, game: &str) {
        self.send(ClientFrame::Lobby(LobbyMsg::Join { game: game.to_string() }));
    }

    /// Back to the lobby (the front answers with the games list).
    pub fn leave(&mut self) {
        self.send(ClientFrame::Lobby(LobbyMsg::Leave));
        self.game = None;
        self.rejoin = None;
        self.rejoining = false;
    }

    // ---- game ----

    pub fn game(&self) -> Option<&InGame> {
        self.game.as_ref()
    }

    pub fn claim(&mut self, area: &str) {
        self.send_game(ClientMsg::Claim { area: area.to_string() });
    }

    pub fn release(&mut self) {
        self.send_game(ClientMsg::Release);
    }

    /// Propose a clock change, or agree to the open proposal.
    pub fn vote(&mut self, proposal: Proposal) {
        self.send_game(ClientMsg::Vote { proposal });
    }

    pub fn command(&mut self, cmd: PlayerCommand) {
        self.send_game(ClientMsg::Command { cmd });
    }
}

/// The signal a command was about, to flash when it is refused.
pub fn entrance_of(cmd: &PlayerCommand) -> Option<&str> {
    match cmd {
        PlayerCommand::SetRoute { entrance, .. }
        | PlayerCommand::CancelRoute { entrance }
        | PlayerCommand::SetAutoWorking { entrance, .. } => Some(entrance),
        _ => None,
    }
}
```

Create `crates/client-core/src/lib.rs`:
```rust
//! The signalbox client's logic (spec D1 §2): connection, lobby, the game
//! you are in, and what clicks mean, behind a `Transport`. No drawing and
//! no browser: tested natively; `client-ui` draws it and `client-web` runs
//! it in a browser.

pub mod app;
pub mod log;
pub mod text;
pub mod transport;

pub use app::{App, InGame, Link};
pub use transport::{ConnState, MemHandle, MemTransport, Transport};
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `scripts/cargo test -p signalbox-client-core`
Expected: PASS (`app` 15 tests, `text` 3).
Run: `scripts/cargo test -p signalbox-bot`
Expected: PASS (the `net` feature is on by default, so `play` and the soak still build).
Run: `scripts/cargo build -p signalbox-bot --no-default-features`
Expected: builds, no warnings.
Run: `scripts/cargo tree -p signalbox-client-core --target wasm32-unknown-unknown -e normal | grep -e getrandom -e tokio`
Expected: no output (grep exits 1): the browser client's tree has neither.
Run: `scripts/cargo test -p signalbox-server` and `scripts/cargo test -p signalbox-core`
Expected: PASS (`rand::random` still works in the server; the sim is unchanged).

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock crates/bot crates/server/Cargo.toml crates/client-core scripts/ci/test.sh
git commit -m "feat(client-core): connection, lobby and game state behind a Transport"
```

---

### Task 4: `client-core` — what clicks mean: selection, menus, hover, train list

**Files:**
- Create: `crates/client-core/src/select.rs`, `crates/client-core/src/input.rs`, `crates/client-core/src/trains.rs`
- Modify: `crates/client-core/src/lib.rs` (modules, `Target`), `crates/client-core/src/app.rs` (a new layout drops an entrance you can no longer work), `crates/client-core/Cargo.toml` (dev-dependencies)
- Modify: `crates/client-core/tests/common/mod.rs` (`Table`: the app against an in-process twobox `Game`)
- Test: `crates/client-core/tests/input.rs`

**Interfaces:**
- Consumes: Task 3's `App` (its `pub(crate)` fields `game`, and `App::command`), `InGame` (`pub(crate)` `bot`, `selected`); `protocol::{Layout, View, RouteInfo, ExitName, PlayerCommand, PointsPos, Aspect, Held, RouteState, TrainRow}`; `game::{Game, GameMeta, Out}` in tests.
- Produces:
  - `select::Click { Select(String), Send(PlayerCommand), Clear, Ignore }`; `select::MenuItem { label: String, cmd: PlayerCommand }` (`Clone, Debug, PartialEq, Eq`).
  - `select::{routes_from(&Layout, &str) -> impl Iterator<Item = &RouteInfo>, can_enter(&Layout, &str) -> bool, exits_from(&Layout, &str) -> Vec<ExitName>, click(&Layout, Option<&str>, &ExitName) -> Click, signal_menu / points_menu / berth_menu(&Layout, &View, &str) -> Vec<MenuItem>, operable_berth(&Layout, &str) -> bool, interpose(&str, &str) -> Option<PlayerCommand>, describe_signal / describe_points / describe_berth / describe_section(&Layout, &View, &str) -> String}`.
  - `input::Target { Signal(String), Points(String), Berth(String), Exit(String), Section(String) }` (`Clone, Debug, PartialEq, Eq`), re-exported as `client_core::Target`; `Exit` is a node name.
  - `impl App { fn click(&mut self, &Target); fn escape(&mut self); fn valid_exits(&self) -> Vec<ExitName>; fn menu(&self, &Target) -> Vec<MenuItem>; fn can_interpose(&self, berth: &str) -> bool; fn interpose(&mut self, berth: &str, typed: &str); fn describe(&self, &Target) -> String }`.
  - `trains::train_list(&View) -> Vec<(&str, &TrainRow)>`: at platform, in area, approaching, due; then booked time (unbooked last), then headcode.
  - Test harness `common::Table { game, app, h, me, now }` with `Table::new(me, area: Option<&str>)`, `deliver(Vec<Out>)`, `pump()`, `run(secs)`, `layout()`, `view()`, `log_lines()`.

- [ ] **Step 1: Write the failing tests**

Add to `crates/client-core/Cargo.toml`:
```toml

[dev-dependencies]
signalbox-core = { path = "../core" }
signalbox-game = { path = "../game" }
```

Replace `crates/client-core/tests/common/mod.rs` with:
```rust
//! A front in miniature: the app's game frames go to an in-process twobox
//! `Game`, and its answers come back through a `MemTransport`.

#![allow(dead_code)]

use client_core::{App, MemHandle, MemTransport};
use game::{Game, GameMeta, Out};
use protocol::*;
use signalbox_core::world::World;

pub const TWOBOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../game/tests/fixtures/twobox.json");

pub fn s(x: &str) -> String {
    x.to_string()
}

/// An app whose first connection is open, its lobby requests taken.
pub fn open_app() -> (App, MemHandle) {
    let (t, h) = MemTransport::new();
    let mut app = App::new(Box::new(t), 0.0);
    h.open();
    app.tick(0.0);
    h.take_sent();
    (app, h)
}

pub struct Table {
    pub game: Game,
    pub app: App,
    pub h: MemHandle,
    pub me: String,
    pub now: f64,
}

impl Table {
    /// `me` has joined a twobox game (as the front would tell it) and, if
    /// given, claimed `area`.
    pub fn new(me: &str, area: Option<&str>) -> Table {
        let world = World::from_json(&std::fs::read_to_string(TWOBOX).unwrap()).unwrap();
        let game = Game::new(world, GameMeta { layout: s("twobox"), seed: 1 });
        let (app, h) = open_app();
        let mut t = Table { game, app, h, me: s(me), now: 0.0 };
        t.h.push(ServerFrame::Lobby(LobbyReply::Joined { game: s("g-test"), you: s(me) }));
        let out = t.game.connect(me);
        t.deliver(out);
        if let Some(a) = area {
            t.app.claim(a);
            t.pump();
        }
        t
    }

    /// The game's messages for me, to the app.
    pub fn deliver(&mut self, out: Vec<Out>) {
        for (p, m) in out {
            if p == self.me {
                self.h.push(ServerFrame::Game(m));
            }
        }
        self.app.tick(self.now);
    }

    /// The app's game messages to the game and the answers back, until quiet.
    pub fn pump(&mut self) {
        loop {
            let sent = self.h.take_sent();
            if sent.is_empty() {
                return;
            }
            for f in sent {
                if let ClientFrame::Game(m) = f {
                    let me = self.me.clone();
                    let out = self.game.handle(&me, m);
                    self.deliver(out);
                }
            }
        }
    }

    /// Run the game for `secs` of real time at 1x, flushing to the app.
    pub fn run(&mut self, secs: f64) {
        for _ in 0..(secs / 0.1).round() as u64 {
            self.now += 0.1;
            let mut out = self.game.advance(0.1);
            out.extend(self.game.flush());
            self.deliver(out);
            self.pump();
        }
    }

    pub fn layout(&self) -> &Layout {
        self.app.game().unwrap().layout().unwrap()
    }

    pub fn view(&self) -> &View {
        self.app.game().unwrap().view().unwrap()
    }

    pub fn log_lines(&self) -> Vec<(String, bool)> {
        self.app.game().unwrap().log().entries().map(|e| (e.text.clone(), e.alarm)).collect()
    }
}
```

Create `crates/client-core/tests/input.rs`:
```rust
//! Clicks, menus, hover text and the train list (spec D1 §3.2, §6)
//! against a real twobox game in process.

mod common;

use std::collections::BTreeMap;

use client_core::Target;
use client_core::app::FLASH_S;
use client_core::select::{self, Click, MenuItem};
use client_core::trains::train_list;
use common::*;
use protocol::*;

fn sig(n: &str) -> Target {
    Target::Signal(s(n))
}

fn set_route(entrance: &str, exit: ExitName) -> PlayerCommand {
    PlayerCommand::SetRoute { entrance: s(entrance), exit }
}

#[test]
fn entrance_then_exit_sets_the_route() {
    let mut t = Table::new("ann", Some("West"));
    t.app.click(&sig("W1"));
    assert_eq!(t.app.game().unwrap().selected(), Some("W1"));
    assert_eq!(t.app.valid_exits(), [ExitName::Signal(s("A"))], "only exits of routes from W1 light up");
    t.app.click(&sig("A"));
    assert_eq!(t.app.game().unwrap().selected(), None);
    assert_eq!(t.h.take_sent(), [ClientFrame::Game(ClientMsg::Command { cmd: set_route("W1", ExitName::Signal(s("A"))) })]);
    t.game.handle("ann", ClientMsg::Command { cmd: set_route("W1", ExitName::Signal(s("A"))) });
    t.run(1.0);
    assert!(t.view().routes.contains_key("W1-A"), "{:?}", t.view().routes);
}

#[test]
fn routes_to_a_boundary_end_at_its_exit_marker() {
    let mut t = Table::new("ann", Some("West"));
    t.app.click(&sig("A"));
    assert_eq!(t.app.valid_exits(), [ExitName::Node(s("E")), ExitName::Node(s("N"))]);
    t.app.click(&Target::Exit(s("N")));
    t.pump();
    t.run(10.0);
    assert!(t.view().routes.contains_key("A-N"), "{:?}", t.view().routes);
    assert_eq!(t.view().points["P"].position, PointsPos::Reverse);
}

#[test]
fn the_selection_clears_on_the_entrance_again_esc_or_a_dead_click() {
    let mut t = Table::new("ann", Some("West"));
    t.app.click(&sig("W1"));
    t.app.click(&sig("W1"));
    assert_eq!(t.app.game().unwrap().selected(), None, "the entrance again");
    t.app.click(&sig("W1"));
    t.app.escape();
    assert_eq!(t.app.game().unwrap().selected(), None, "Esc");
    t.app.click(&sig("W1"));
    t.app.click(&sig("W2"));
    assert_eq!(t.app.game().unwrap().selected(), Some("W2"), "another entrance takes over");
    t.app.click(&Target::Exit(s("E")));
    assert_eq!(t.app.game().unwrap().selected(), None, "not an exit of W2's routes");
    t.app.click(&Target::Points(s("P")));
    t.app.click(&Target::Berth(s("BA")));
    assert!(t.h.take_sent().is_empty(), "nothing was sent");
}

#[test]
fn fringe_and_spectators_get_hover_only() {
    let mut t = Table::new("ann", Some("West"));
    t.app.click(&sig("C"));
    assert_eq!(t.app.game().unwrap().selected(), None, "C is East's, seen on West's fringe");
    assert!(t.app.menu(&Target::Points(s("P"))).is_empty());
    assert_eq!(t.app.describe(&Target::Points(s("P"))), "Points P (East): normal");
    let mut spec = Table::new("sam", None);
    spec.app.click(&sig("W1"));
    assert_eq!(spec.app.game().unwrap().selected(), None);
    assert!(spec.app.menu(&sig("W1")).is_empty());
    assert!(!spec.app.can_interpose("BA"));
    assert_eq!(spec.app.describe(&sig("W1")), "Signal W1 (West): red");
}

#[test]
fn right_click_cancels_a_route_and_swings_points() {
    let mut t = Table::new("eve", Some("East"));
    assert_eq!(
        t.app.menu(&Target::Points(s("P"))),
        [MenuItem { label: s("Swing P reverse"), cmd: PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse } }]
    );
    assert!(t.app.menu(&sig("C")).is_empty(), "no route set from C");
    t.app.click(&sig("C"));
    t.app.click(&sig("W2"));
    t.pump();
    t.run(10.0);
    assert!(t.view().routes.contains_key("C-W2"));
    assert_eq!(
        t.app.menu(&sig("C")),
        [MenuItem { label: s("Cancel route C to W2"), cmd: PlayerCommand::CancelRoute { entrance: s("C") } }]
    );
    assert_eq!(t.app.describe(&sig("C")), "Signal C: yellow; route to W2 set");
    let MenuItem { cmd, .. } = t.app.menu(&sig("C")).remove(0);
    t.app.command(cmd);
    t.pump();
    t.run(60.0);
    assert!(!t.view().routes.contains_key("C-W2"));
}

#[test]
fn a_refused_command_flashes_its_entrance_and_raises_an_alarm() {
    let mut t = Table::new("eve", Some("East"));
    t.app.click(&sig("C"));
    t.app.click(&sig("W2"));
    t.pump();
    t.run(0.5);
    t.app.click(&sig("D"));
    t.app.click(&sig("W2"));
    t.pump();
    t.run(0.2);
    let g = t.app.game().unwrap();
    assert_eq!(g.flashing(), Some("D"));
    assert_eq!(
        t.log_lines().last().unwrap(),
        &(s("Refused: set route D to W2 (conflicts with a route already set)"), true)
    );
    t.run(FLASH_S);
    assert_eq!(t.app.game().unwrap().flashing(), None);
}

#[test]
fn berths_interpose_a_typed_headcode_and_cancel_it() {
    let mut t = Table::new("ann", Some("West"));
    assert!(t.app.can_interpose("BA"));
    assert!(t.app.menu(&Target::Berth(s("BA"))).is_empty(), "nothing to cancel yet");
    t.app.interpose("BA", "   ");
    assert!(t.h.take_sent().is_empty(), "a blank headcode sends nothing");
    t.app.interpose("BA", " 2Z99 ");
    t.pump();
    t.run(0.5);
    assert_eq!(t.view().berths.get("BA").map(String::as_str), Some("2Z99"));
    assert_eq!(t.app.describe(&Target::Berth(s("BA"))), "Berth BA: 2Z99");
    assert_eq!(
        t.app.menu(&Target::Berth(s("BA"))),
        [MenuItem { label: s("Cancel 2Z99"), cmd: PlayerCommand::CancelBerth { berth: s("BA") } }]
    );
}

fn auto_layout() -> Layout {
    let route = |name: &str, exit: &str, automatic: bool| RouteInfo {
        name: s(name),
        entrance: s("S1"),
        exit: ExitName::Signal(s(exit)),
        automatic,
        operable: true,
    };
    Layout {
        title: s("t"),
        you: s("ann"),
        area: Some(s("A")),
        areas: vec![s("A")],
        sections: vec![],
        segments: vec![],
        signals: vec![SignalInfo {
            name: s("S1"),
            area: s("A"),
            segment: s("x"),
            offset_m: 0.0,
            direction: Dir::Up,
            aspects: 3,
            operable: true,
        }],
        points: vec![],
        berths: vec![],
        platforms: vec![],
        routes: vec![route("S1-S2", "S2", true), route("S1-S3", "S3", false)],
        geometry: None,
    }
}

fn empty_view() -> View {
    View {
        seq: 1,
        sim_time: 0.0,
        speed: 1,
        paused: false,
        vote: None,
        holders: BTreeMap::new(),
        score: Some(0),
        signals: BTreeMap::new(),
        routes: BTreeMap::new(),
        points: BTreeMap::new(),
        sections: BTreeMap::new(),
        berths: BTreeMap::new(),
        trains: BTreeMap::new(),
    }
}

#[test]
fn automatic_routes_offer_auto_working_on_and_off() {
    let l = auto_layout();
    let mut v = empty_view();
    assert!(select::signal_menu(&l, &v, "S1").is_empty());
    v.routes.insert(s("S1-S2"), RouteView { state: RouteState::Locked, auto_working: true });
    let labels: Vec<String> = select::signal_menu(&l, &v, "S1").into_iter().map(|m| m.label).collect();
    assert_eq!(labels, ["Cancel route S1 to S2", "Auto-working off"]);
    v.routes.insert(s("S1-S2"), RouteView { state: RouteState::Locked, auto_working: false });
    assert_eq!(
        select::signal_menu(&l, &v, "S1")[1].cmd,
        PlayerCommand::SetAutoWorking { entrance: s("S1"), on: true }
    );
    assert_eq!(select::click(&l, None, &ExitName::Signal(s("S9"))), Click::Ignore, "no routes from S9");
    assert_eq!(select::click(&l, Some("S1"), &ExitName::Node(s("Z"))), Click::Clear);
}

#[test]
fn the_train_list_puts_platforms_first_then_by_booked_time() {
    let row = |state, booked: Option<f64>| TrainRow { next_place: None, next_platform: None, booked, late_s: 0, state };
    let mut v = empty_view();
    v.trains.insert(s("1A"), row(TrainState::Due, Some(100.0)));
    v.trains.insert(s("1B"), row(TrainState::InArea, Some(300.0)));
    v.trains.insert(s("1C"), row(TrainState::InArea, Some(200.0)));
    v.trains.insert(s("1D"), row(TrainState::AtPlatform, None));
    v.trains.insert(s("1E"), row(TrainState::InArea, None));
    v.trains.insert(s("1F"), row(TrainState::Approaching, Some(50.0)));
    let order: Vec<&str> = train_list(&v).into_iter().map(|(h, _)| h).collect();
    assert_eq!(order, ["1D", "1C", "1B", "1E", "1F", "1A"]);
}

#[test]
fn hover_describes_track_and_names_other_areas() {
    let t = Table::new("ann", Some("West"));
    assert_eq!(t.app.describe(&Target::Section(s("TW1"))), "Track TW1: clear");
    assert_eq!(t.app.describe(&Target::Section(s("TP"))), "Track TP (East): clear");
    assert_eq!(t.app.describe(&Target::Exit(s("W"))), "Exit W");
    assert_eq!(t.app.describe(&sig("nowhere")), "Signal nowhere");
}

#[test]
fn a_new_layout_drops_an_entrance_you_can_no_longer_work() {
    let mut t = Table::new("ann", Some("West"));
    t.app.click(&sig("W1"));
    t.app.release();
    t.pump();
    assert_eq!(t.app.game().unwrap().area(), None);
    assert_eq!(t.app.game().unwrap().selected(), None);
    assert!(t.app.valid_exits().is_empty());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `scripts/cargo test -p signalbox-client-core --test input`
Expected: FAIL to compile — no `client_core::Target`, no module `select`, `trains`, no method `click` on `App`.

- [ ] **Step 3: Selection, menus and hover as pure functions**

Create `crates/client-core/src/select.rs`:
```rust
//! What clicks mean (spec D1 §3.2), as pure functions of the layout and
//! view: route setting by entrance then exit, the right-click menus, and
//! hover text. Only operable things (your own area) can be worked; the
//! fringe and spectators get hover text only.

use protocol::{Aspect, ExitName, Held, Layout, PlayerCommand, PointsPos, RouteInfo, RouteState, View};

use crate::text::{exit_text, pos_text};

/// What a left click on a signal or an exit node does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Click {
    /// Nothing selected yet and this can be an entrance: select it.
    Select(String),
    /// Send this (a `SetRoute`); the selection clears.
    Send(PlayerCommand),
    Clear,
    Ignore,
}

/// A right-click menu entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub label: String,
    pub cmd: PlayerCommand,
}

/// Operable routes from `entrance`, in layout order.
pub fn routes_from<'a>(l: &'a Layout, entrance: &'a str) -> impl Iterator<Item = &'a RouteInfo> + 'a {
    l.routes.iter().filter(move |r| r.operable && r.entrance == entrance)
}

/// A signal you can start a route from.
pub fn can_enter(l: &Layout, signal: &str) -> bool {
    routes_from(l, signal).next().is_some()
}

/// Where routes from `entrance` can end: the exits to light up.
pub fn exits_from(l: &Layout, entrance: &str) -> Vec<ExitName> {
    let mut out: Vec<ExitName> = Vec::new();
    for r in routes_from(l, entrance) {
        if !out.contains(&r.exit) {
            out.push(r.exit.clone());
        }
    }
    out
}

/// A left click on `target` (a signal, or a buffer stop / boundary node)
/// with `selected` as the chosen entrance, if any.
pub fn click(l: &Layout, selected: Option<&str>, target: &ExitName) -> Click {
    let as_entrance = |t: &ExitName| match t {
        ExitName::Signal(s) if can_enter(l, s) => Some(s.clone()),
        _ => None,
    };
    match selected {
        None => as_entrance(target).map_or(Click::Ignore, Click::Select),
        Some(e) if *target == ExitName::Signal(e.to_string()) => Click::Clear,
        Some(e) if exits_from(l, e).contains(target) => {
            Click::Send(PlayerCommand::SetRoute { entrance: e.to_string(), exit: target.clone() })
        }
        Some(_) => as_entrance(target).map_or(Click::Clear, Click::Select),
    }
}

/// Routes from `entrance` that are set (not idle) in the view.
fn active_from<'a>(l: &'a Layout, v: &'a View, entrance: &'a str) -> impl Iterator<Item = &'a RouteInfo> + 'a {
    l.routes.iter().filter(move |r| r.entrance == entrance && v.routes.contains_key(&r.name))
}

pub fn signal_menu(l: &Layout, v: &View, signal: &str) -> Vec<MenuItem> {
    if !l.signals.iter().any(|s| s.name == signal && s.operable) {
        return vec![];
    }
    let mut items = Vec::new();
    if let Some(r) = active_from(l, v, signal).next() {
        items.push(MenuItem {
            label: format!("Cancel route {signal} to {}", exit_text(&r.exit)),
            cmd: PlayerCommand::CancelRoute { entrance: signal.to_string() },
        });
    }
    if let Some(r) = active_from(l, v, signal).find(|r| r.automatic) {
        let on = v.routes.get(&r.name).is_some_and(|rv| rv.auto_working);
        items.push(MenuItem {
            label: format!("Auto-working {}", if on { "off" } else { "on" }),
            cmd: PlayerCommand::SetAutoWorking { entrance: signal.to_string(), on: !on },
        });
    }
    items
}

pub fn points_menu(l: &Layout, v: &View, points: &str) -> Vec<MenuItem> {
    if !l.points.iter().any(|p| p.name == points && p.operable) {
        return vec![];
    }
    let now = v.points.get(points).map_or(PointsPos::Normal, |p| p.position);
    let to = match now {
        PointsPos::Normal => PointsPos::Reverse,
        PointsPos::Reverse => PointsPos::Normal,
    };
    vec![MenuItem { label: format!("Swing {points} {}", pos_text(to)), cmd: PlayerCommand::SwingPoints { points: points.to_string(), to } }]
}

/// Cancelling a berth's headcode; interposing needs a headcode typed in,
/// so the screen offers it separately (`operable_berth`).
pub fn berth_menu(l: &Layout, v: &View, berth: &str) -> Vec<MenuItem> {
    if !operable_berth(l, berth) {
        return vec![];
    }
    match v.berths.get(berth) {
        Some(h) => vec![MenuItem { label: format!("Cancel {h}"), cmd: PlayerCommand::CancelBerth { berth: berth.to_string() } }],
        None => vec![],
    }
}

pub fn operable_berth(l: &Layout, berth: &str) -> bool {
    l.berths.iter().any(|b| b.name == berth && b.operable)
}

/// `Interpose` for a typed headcode (trimmed; the game checks its form).
pub fn interpose(berth: &str, typed: &str) -> Option<PlayerCommand> {
    let h = typed.trim();
    (!h.is_empty()).then(|| PlayerCommand::Interpose { berth: berth.to_string(), headcode: h.to_string() })
}

fn aspect_text(a: Aspect) -> &'static str {
    match a {
        Aspect::Red => "red",
        Aspect::Yellow => "yellow",
        Aspect::DoubleYellow => "double yellow",
        Aspect::Green => "green",
    }
}

fn area_note(l: &Layout, area: &str) -> String {
    if l.area.as_deref() == Some(area) { String::new() } else { format!(" ({area})") }
}

pub fn describe_signal(l: &Layout, v: &View, signal: &str) -> String {
    let Some(s) = l.signals.iter().find(|s| s.name == signal) else { return format!("Signal {signal}") };
    let mut out = format!("Signal {signal}{}: {}", area_note(l, &s.area), v.signals.get(signal).map_or("?", |a| aspect_text(*a)));
    for r in active_from(l, v, signal) {
        let rv = &v.routes[&r.name];
        let state = match rv.state {
            RouteState::Setting => "setting",
            RouteState::Locked => "set",
            RouteState::Cancelling => "cancelling",
        };
        out.push_str(&format!("; route to {} {state}", exit_text(&r.exit)));
        if rv.auto_working {
            out.push_str(", auto-working");
        }
    }
    out
}

pub fn describe_points(l: &Layout, v: &View, points: &str) -> String {
    let area = l.points.iter().find(|p| p.name == points).map(|p| area_note(l, &p.area)).unwrap_or_default();
    match v.points.get(points) {
        Some(p) => format!(
            "Points {points}{area}: {}{}{}",
            pos_text(p.position),
            if p.moving { ", moving" } else { "" },
            if p.locked { ", locked" } else { "" }
        ),
        None => format!("Points {points}{area}"),
    }
}

pub fn describe_berth(l: &Layout, v: &View, berth: &str) -> String {
    let area = l.berths.iter().find(|b| b.name == berth).map(|b| area_note(l, &b.area)).unwrap_or_default();
    format!("Berth {berth}{area}: {}", v.berths.get(berth).map_or("empty", String::as_str))
}

pub fn describe_section(l: &Layout, v: &View, section: &str) -> String {
    let area = l.sections.iter().find(|s| s.name == section).map(|s| area_note(l, &s.area)).unwrap_or_default();
    let state = match v.sections.get(section) {
        Some(s) if s.occupied => "occupied",
        Some(s) if s.held == Held::Path => "route set",
        Some(s) if s.held == Held::Overlap => "overlap",
        Some(_) => "clear",
        None => "?",
    };
    format!("Track {section}{area}: {state}")
}
```

- [ ] **Step 4: The app's controls and the train list**

Create `crates/client-core/src/input.rs`:
```rust
//! The app's side of the controls (spec D1 §3.2): clicks, Esc, menus,
//! interposing and hover text, on top of `select`.

use protocol::ExitName;

use crate::app::App;
use crate::select::{self, Click, MenuItem};

/// Something on the diagram under the pointer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Signal(String),
    Points(String),
    Berth(String),
    /// A buffer stop or boundary node a route can end at.
    Exit(String),
    Section(String),
}

impl App {
    /// A left click. Only signals and exits do anything.
    pub fn click(&mut self, target: &Target) {
        let exit = match target {
            Target::Signal(s) => ExitName::Signal(s.clone()),
            Target::Exit(n) => ExitName::Node(n.clone()),
            _ => return,
        };
        let Some(g) = self.game.as_mut() else { return };
        let Some(l) = g.bot.layout() else { return };
        match select::click(l, g.selected.as_deref(), &exit) {
            Click::Select(s) => g.selected = Some(s),
            Click::Clear => g.selected = None,
            Click::Send(cmd) => {
                g.selected = None;
                self.command(cmd);
            }
            Click::Ignore => {}
        }
    }

    /// Esc: forget the chosen entrance.
    pub fn escape(&mut self) {
        if let Some(g) = self.game.as_mut() {
            g.selected = None;
        }
    }

    /// The exits to light up for the chosen entrance.
    pub fn valid_exits(&self) -> Vec<ExitName> {
        let Some(g) = self.game.as_ref() else { return vec![] };
        match (g.bot.layout(), g.selected.as_deref()) {
            (Some(l), Some(e)) => select::exits_from(l, e),
            _ => vec![],
        }
    }

    /// The right-click menu for `target` (empty: no menu).
    pub fn menu(&self, target: &Target) -> Vec<MenuItem> {
        let Some(g) = self.game.as_ref() else { return vec![] };
        let (Some(l), Some(v)) = (g.bot.layout(), g.bot.view()) else { return vec![] };
        match target {
            Target::Signal(s) => select::signal_menu(l, v, s),
            Target::Points(p) => select::points_menu(l, v, p),
            Target::Berth(b) => select::berth_menu(l, v, b),
            Target::Exit(_) | Target::Section(_) => vec![],
        }
    }

    /// Whether the berth menu should offer a headcode box.
    pub fn can_interpose(&self, berth: &str) -> bool {
        self.game.as_ref().and_then(|g| g.bot.layout()).is_some_and(|l| select::operable_berth(l, berth))
    }

    /// Interpose a typed headcode; blank input sends nothing.
    pub fn interpose(&mut self, berth: &str, typed: &str) {
        if let Some(cmd) = select::interpose(berth, typed) {
            self.command(cmd);
        }
    }

    /// Hover text.
    pub fn describe(&self, target: &Target) -> String {
        let Some(g) = self.game.as_ref() else { return String::new() };
        let (Some(l), Some(v)) = (g.bot.layout(), g.bot.view()) else { return String::new() };
        match target {
            Target::Signal(s) => select::describe_signal(l, v, s),
            Target::Points(p) => select::describe_points(l, v, p),
            Target::Berth(b) => select::describe_berth(l, v, b),
            Target::Exit(n) => format!("Exit {n}"),
            Target::Section(s) => select::describe_section(l, v, s),
        }
    }
}
```

Create `crates/client-core/src/trains.rs`:
```rust
//! The train list (spec D1 §3): trains at platforms first, then in your
//! area, approaching, and due; within each by booked time (unbooked last),
//! then headcode.

use std::cmp::Ordering;

use protocol::{TrainRow, View};

fn by_booked(a: Option<f64>, b: Option<f64>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

pub fn train_list(v: &View) -> Vec<(&str, &TrainRow)> {
    let mut rows: Vec<(&str, &TrainRow)> = v.trains.iter().map(|(h, r)| (h.as_str(), r)).collect();
    rows.sort_by(|(ha, a), (hb, b)| b.state.cmp(&a.state).then_with(|| by_booked(a.booked, b.booked)).then_with(|| ha.cmp(hb)));
    rows
}
```

Replace `crates/client-core/src/lib.rs` with:
```rust
//! The signalbox client's logic (spec D1 §2): connection, lobby, the game
//! you are in, and what clicks mean, behind a `Transport`. No drawing and
//! no browser: tested natively; `client-ui` draws it and `client-web` runs
//! it in a browser.

pub mod app;
pub mod input;
pub mod log;
pub mod select;
pub mod text;
pub mod trains;
pub mod transport;

pub use app::{App, InGame, Link};
pub use input::Target;
pub use transport::{ConnState, MemHandle, MemTransport, Transport};
```

In `crates/client-core/src/app.rs`, `game_msg`, between `g.bot.take_notices();` and `if let Some(r) = reply {`, add:
```rust
        if let (Some(sel), Some(l)) = (g.selected.as_deref(), g.bot.layout()) {
            if !crate::select::can_enter(l, sel) {
                g.selected = None;
            }
        }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `scripts/cargo test -p signalbox-client-core`
Expected: PASS (`app` 15, `input` 11, `text` 3).

- [ ] **Step 6: Commit**

```bash
git add crates/client-core Cargo.lock
git commit -m "feat(client-core): route setting by entrance and exit, menus, hover text and the train list"
```

---

### Task 5: `client-ui` — the diagram: scene, camera, hit-testing, drawing

**Files:**
- Modify: `Cargo.toml` (member `crates/client-ui`; workspace dependency `egui`)
- Create: `crates/client-ui/Cargo.toml`, `crates/client-ui/src/{lib,scene,camera,hit,paint}.rs`
- Test: `crates/client-ui/tests/common/mod.rs`, `crates/client-ui/tests/{scene,camera,hit,paint}.rs`

**Interfaces:**
- Consumes: `protocol::{Layout, View, Geometry, ExitName, Aspect, Held, PointsPos, SectionView}` (Task 1); `client_core::Target` (Task 4); `egui` 0.36.2 (`Pos2`, `Vec2`, `Rect`, `Color32`, `Shape`, `Stroke`, `Painter`); Task 2's fixture `crates/game/tests/fixtures/twobox-layout.json` and `game::Game` in tests.
- Produces (crate `signalbox-client-ui`, lib `client_ui`):
  - `scene::{Scene, TrackLine, PointsMark, SignalMark, BerthMark, ExitMark, PlatformMark, LabelMark, BOUNDARY_BERTH_OFFSET_PX}`; `Scene::build(&Layout) -> Option<Scene>` (`None` without geometry), `Scene::fit_bounds(&self) -> Option<Rect>` (own area, else everything). Fields: `Scene { tracks, points, signals, berths, exits, platforms, labels, own: Option<Rect>, all: Option<Rect> }`; `SignalMark { name, at, facing: Vec2 (unit or zero), fringe, operable, auto_routes: Vec<String> }`; `BerthMark { name, at, offset_px: Vec2, fringe, operable }`; `PointsMark { name, section, at, toe, normal, reverse: Option<Pos2>, fringe, operable }`; `TrackLine { segment, section, a, b, fringe }`; `ExitMark { node, at }`.
  - `camera::{Camera { centre: Pos2, scale: f32 }, MIN_SCALE = 0.02, MAX_SCALE = 50.0, FIT_MARGIN = 0.05}` with `Camera::fit(world: Rect, screen: Rect) -> Camera`, `to_screen(&self, Rect, Pos2) -> Pos2`, `to_world(&self, Rect, Pos2) -> Pos2`, `zoom_at(&mut self, Rect, Pos2, f32)`, `pan(&mut self, Vec2)`.
  - `hit::{Hit { target: Target, clickable: bool }, hit_test(&Scene, &Camera, Rect, Pos2) -> Option<Hit>, berth_rect(&Camera, Rect, Pos2, Vec2) -> Rect, HIT_PX = 8.0, BERTH_W = 34.0, BERTH_H = 14.0}`.
  - `paint::{draw(&Scene, &Camera, Rect, &PaintState) -> Drawing, paint(&Painter, Drawing), PaintState { view, selected, exits, flashing, time }, Drawing { shapes: Vec<Shape>, texts: Vec<TextItem> }, TextItem, track_colour, dim, lamps, blink_on}` and the colour constants of the Global Constraints (`BG`, `TRACK_FREE`, `ROUTE`, `OVERLAP`, `OCCUPIED`, `RED`, `YELLOW`, `GREEN`, `HEADCODE`, `BERTH_EMPTY`, `SELECT`, `FLASH`, `PLATFORM`, `LABEL`, `AUTO_ON`, `AUTO_OFF`), `TRACK_W`, `LAMP_R`, `STUB_PX`, `GAP`.

- [ ] **Step 1: The crate and its dependency**

In the root `Cargo.toml`, add `"crates/client-ui"` to `members` (after `client-core`) and, at the end of `[workspace.dependencies]`:
```toml
# The browser client (D1). egui for the screens; eframe (web runner, wgpu) only in client-web.
egui = "0.36.2"
```
Create `crates/client-ui/Cargo.toml`:
```toml
[package]
name = "signalbox-client-ui"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "client_ui"
path = "src/lib.rs"

[dependencies]
signalbox-client-core = { path = "../client-core" }
signalbox-protocol = { path = "../protocol" }
egui.workspace = true

[dev-dependencies]
signalbox-core = { path = "../core" }
signalbox-game = { path = "../game" }
serde_json.workspace = true
```
(`egui` with its default features: `default_fonts`, which the headless screen tests in Task 6 need to lay out text.)

- [ ] **Step 2: Write the failing tests**

Create `crates/client-ui/tests/common/mod.rs`:
```rust
//! Drawn twobox layouts and views, from a real in-process game.

#![allow(dead_code)]

use game::{Game, GameMeta};
use protocol::{ClientMsg, Layout, View};
use serde_json::Value;
use signalbox_core::world::World;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../game/tests/fixtures");

pub fn s(x: &str) -> String {
    x.to_string()
}

/// twobox with `twobox-layout.json` as its drawing.
pub fn drawn_twobox() -> World {
    let mut w: Value = serde_json::from_str(&std::fs::read_to_string(format!("{FIXTURES}/twobox.json")).unwrap()).unwrap();
    w["layout"] = serde_json::from_str(&std::fs::read_to_string(format!("{FIXTURES}/twobox-layout.json")).unwrap()).unwrap();
    World::from_json(&w.to_string()).unwrap()
}

/// A game with "ann" holding `area` (a spectator for `None`).
pub fn game_for(area: Option<&str>) -> Game {
    let mut g = Game::new(drawn_twobox(), GameMeta { layout: s("twobox"), seed: 1 });
    g.connect("ann");
    if let Some(a) = area {
        g.handle("ann", ClientMsg::Claim { area: s(a) });
    }
    g
}

pub fn layout_for(area: Option<&str>) -> Layout {
    game_for(area).layout_of("ann").unwrap()
}

pub fn view_for(area: Option<&str>) -> View {
    game_for(area).view_of("ann").unwrap()
}
```

Create `crates/client-ui/tests/scene.rs`:
```rust
//! The scene built from a layout's geometry and lists.

mod common;

use client_ui::scene::{BOUNDARY_BERTH_OFFSET_PX, Scene};
use common::*;
use egui::{Rect, Vec2, pos2, vec2};

#[test]
fn an_area_scene_marks_its_fringe_and_what_it_can_work() {
    let sc = Scene::build(&layout_for(Some("West"))).unwrap();
    let tracks: Vec<(&str, &str, bool)> = sc.tracks.iter().map(|t| (t.segment.as_str(), t.section.as_str(), t.fringe)).collect();
    assert_eq!(tracks, [("w1", "TW1", false), ("w2", "TW2", false)]);
    assert_eq!((sc.tracks[1].a, sc.tracks[1].b), (pos2(100.0, 0.0), pos2(200.0, 0.0)));
    let p = &sc.points[0];
    assert_eq!((p.name.as_str(), p.section.as_str(), p.fringe, p.operable), ("P", "TP", true, false));
    assert_eq!((p.toe, p.normal, p.reverse), (Some(pos2(200.0, 0.0)), Some(pos2(215.0, 0.0)), Some(pos2(215.0, 10.0))));
    let signals: Vec<(&str, Vec2, bool, bool)> = sc.signals.iter().map(|s| (s.name.as_str(), s.facing, s.fringe, s.operable)).collect();
    assert_eq!(
        signals,
        [("W1", vec2(1.0, 0.0), false, true), ("A", vec2(1.0, 0.0), false, true), ("W2", vec2(-1.0, 0.0), false, true)]
    );
    let berths: Vec<(&str, Vec2)> = sc.berths.iter().map(|b| (b.name.as_str(), b.offset_px)).collect();
    assert_eq!(
        berths,
        [("BW1", Vec2::ZERO), ("BA", Vec2::ZERO), ("BW2", Vec2::ZERO), ("BW", BOUNDARY_BERTH_OFFSET_PX)],
        "signal berths at their boxes, the boundary berth above its exit"
    );
    let exits: Vec<(&str, egui::Pos2)> = sc.exits.iter().map(|e| (e.node.as_str(), e.at)).collect();
    assert_eq!(exits, [("E", pos2(400.0, 0.0)), ("N", pos2(400.0, 60.0)), ("W", pos2(0.0, 0.0))]);
    assert_eq!(sc.labels.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["West"]);
    assert_eq!(sc.own, Some(Rect::from_min_max(pos2(0.0, -5.0), pos2(200.0, 5.0))), "own track and signals");
    assert_eq!(sc.fit_bounds(), sc.own);
    assert_eq!(sc.all, Some(Rect::from_min_max(pos2(0.0, -5.0), pos2(207.5, 5.0))));
}

#[test]
fn the_neighbours_signals_are_fringe() {
    let sc = Scene::build(&layout_for(Some("East"))).unwrap();
    let a = sc.signals.iter().find(|s| s.name == "A").unwrap();
    assert!(a.fringe && !a.operable);
    let c = sc.signals.iter().find(|s| s.name == "C").unwrap();
    assert!(!c.fringe && c.operable);
    assert!(sc.points[0].operable && !sc.points[0].fringe);
    assert_eq!(sc.platforms.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), ["EST 1", "NST 1"]);
}

#[test]
fn a_spectator_fits_everything() {
    let sc = Scene::build(&layout_for(None)).unwrap();
    assert_eq!(sc.own, None);
    assert_eq!(sc.fit_bounds(), sc.all);
    assert!(sc.signals.iter().all(|s| !s.fringe && !s.operable));
}

#[test]
fn no_geometry_no_scene() {
    let mut l = layout_for(Some("West"));
    l.geometry = None;
    assert_eq!(Scene::build(&l), None);
}

#[test]
fn a_scene_skips_what_the_drawing_lacks() {
    let mut l = layout_for(Some("West"));
    let g = l.geometry.as_mut().unwrap();
    g.signals.retain(|s| s.signal != "A");
    g.points[0].reverse = None;
    g.lines[0].x1 = f64::NAN;
    g.nodes.retain(|n| n.node != "E");
    g.signals[0].facing = None;
    g.labels.push(protocol::LabelGeom { text: s("far"), x: f64::INFINITY, y: 0.0 });
    let sc = Scene::build(&l).unwrap();
    assert_eq!(sc.tracks.len(), 1, "the line with a NaN end is skipped");
    assert!(sc.signals.iter().all(|s| s.name != "A"));
    assert!(sc.berths.iter().all(|b| b.name != "BA"), "A's berth goes with A");
    assert_eq!(sc.points[0].reverse, None);
    assert_eq!(sc.signals[0].facing, Vec2::ZERO);
    assert!(sc.exits.iter().all(|e| e.node != "E"));
    assert_eq!(sc.labels.len(), 1);
}

#[test]
fn automatic_routes_are_listed_on_their_entrance() {
    let mut l = layout_for(Some("West"));
    l.routes[0].automatic = true;
    let sc = Scene::build(&l).unwrap();
    assert_eq!(sc.signals[0].auto_routes, [s("W1-A")]);
    assert!(sc.signals[1].auto_routes.is_empty());
}
```

Create `crates/client-ui/tests/camera.rs`:
```rust
//! Fit, zoom and pan.

use client_ui::camera::{Camera, MAX_SCALE, MIN_SCALE};
use egui::{Pos2, Rect, pos2, vec2};

fn screen() -> Rect {
    Rect::from_min_size(pos2(0.0, 40.0), vec2(1000.0, 600.0))
}

fn close(a: Pos2, b: Pos2) -> bool {
    a.distance(b) < 1e-3
}

#[test]
fn fit_centres_the_drawing_and_keeps_a_margin() {
    let world = Rect::from_min_max(pos2(0.0, -5.0), pos2(200.0, 5.0));
    let cam = Camera::fit(world, screen());
    assert_eq!(cam.centre, pos2(100.0, 0.0));
    assert_eq!(cam.scale, 4.5, "900 px of room across 200 units");
    assert!(close(cam.to_screen(screen(), pos2(0.0, 0.0)), pos2(50.0, 340.0)));
    let tall = Camera::fit(Rect::from_min_max(pos2(0.0, 0.0), pos2(10.0, 540.0)), screen());
    assert_eq!(tall.scale, 1.0, "height decides");
}

#[test]
fn screen_and_world_round_trip() {
    let mut cam = Camera::fit(Rect::from_min_max(pos2(-50.0, 10.0), pos2(3000.0, 700.0)), screen());
    cam.pan(vec2(33.0, -7.0));
    for p in [pos2(0.0, 0.0), pos2(1234.5, 99.0), pos2(-50.0, 700.0)] {
        assert!(close(cam.to_world(screen(), cam.to_screen(screen(), p)), p));
    }
}

#[test]
fn zoom_keeps_the_point_under_the_pointer() {
    let mut cam = Camera { centre: pos2(100.0, 0.0), scale: 2.0 };
    let at = pos2(700.0, 100.0);
    let before = cam.to_world(screen(), at);
    cam.zoom_at(screen(), at, 1.25);
    assert_eq!(cam.scale, 2.5);
    assert!(close(cam.to_world(screen(), at), before));
}

#[test]
fn zoom_is_clamped() {
    let mut cam = Camera { centre: pos2(0.0, 0.0), scale: 1.0 };
    for _ in 0..100 {
        cam.zoom_at(screen(), pos2(500.0, 340.0), 1.5);
    }
    assert_eq!(cam.scale, MAX_SCALE);
    for _ in 0..100 {
        cam.zoom_at(screen(), pos2(500.0, 340.0), 0.5);
    }
    assert_eq!(cam.scale, MIN_SCALE);
    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        cam.zoom_at(screen(), pos2(1.0, 1.0), bad);
    }
    assert_eq!(cam.scale, MIN_SCALE);
    assert!(cam.centre.is_finite());
}

#[test]
fn fit_survives_degenerate_bounds() {
    let point = Rect::from_min_max(pos2(5.0, 5.0), pos2(5.0, 5.0));
    assert_eq!(Camera::fit(point, screen()), Camera { centre: pos2(5.0, 5.0), scale: 1.0 });
    let flat = Rect::from_min_max(pos2(0.0, 7.0), pos2(90_000.0, 7.0));
    let cam = Camera::fit(flat, screen());
    assert!(cam.scale.is_finite() && cam.scale >= MIN_SCALE, "{cam:?}");
    assert_eq!(cam.scale, MIN_SCALE, "90 000 units would need 0.01 px each");
    let nothing = Rect::from_min_size(pos2(0.0, 0.0), vec2(0.0, 0.0));
    assert_eq!(Camera::fit(Rect::from_min_max(pos2(0.0, 0.0), pos2(10.0, 10.0)), nothing).scale, 1.0);
    assert_eq!(Camera::fit(Rect::NOTHING, screen()).scale, 1.0);
    let odd = Rect::from_min_max(pos2(f32::NAN, 0.0), pos2(1.0, 1.0));
    assert_eq!(Camera::fit(odd, screen()), Camera { centre: Pos2::ZERO, scale: 1.0 });
}
```

Create `crates/client-ui/tests/hit.rs`:
```rust
//! What is under the pointer.

mod common;

use client_core::Target;
use client_ui::camera::Camera;
use client_ui::hit::{BERTH_H, HIT_PX, Hit, berth_rect, hit_test};
use client_ui::scene::Scene;
use common::*;
use egui::{Pos2, Rect, pos2, vec2};

fn setup(area: Option<&str>) -> (Scene, Camera, Rect) {
    let sc = Scene::build(&layout_for(area)).unwrap();
    let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0));
    let cam = Camera::fit(sc.all.unwrap(), screen);
    (sc, cam, screen)
}

fn at(cam: &Camera, screen: Rect, x: f32, y: f32) -> Pos2 {
    cam.to_screen(screen, pos2(x, y))
}

fn hit(t: Target, clickable: bool) -> Option<Hit> {
    Some(Hit { target: t, clickable })
}

#[test]
fn signals_berths_exits_points_and_track() {
    let (sc, cam, screen) = setup(Some("West"));
    assert_eq!(hit_test(&sc, &cam, screen, at(&cam, screen, 200.0, -5.0)), hit(Target::Signal(s("A")), true));
    let near = at(&cam, screen, 200.0, -5.0) + vec2(HIT_PX - 1.0, 0.0);
    assert_eq!(hit_test(&sc, &cam, screen, near), hit(Target::Signal(s("A")), true), "within the radius");
    assert_eq!(hit_test(&sc, &cam, screen, at(&cam, screen, 190.0, -15.0)), hit(Target::Berth(s("BA")), true));
    let bw = berth_rect(&cam, screen, pos2(0.0, 0.0), sc.berths[3].offset_px);
    assert_eq!(hit_test(&sc, &cam, screen, bw.center()), hit(Target::Berth(s("BW")), true));
    assert_eq!(hit_test(&sc, &cam, screen, at(&cam, screen, 0.0, 0.0)), hit(Target::Exit(s("W")), true), "under the berth box");
    assert_eq!(hit_test(&sc, &cam, screen, at(&cam, screen, 207.5, 0.0)), hit(Target::Points(s("P")), false), "East's points");
    assert_eq!(hit_test(&sc, &cam, screen, at(&cam, screen, 50.0, 0.0)), hit(Target::Section(s("TW1")), false));
    assert_eq!(hit_test(&sc, &cam, screen, at(&cam, screen, 50.0, 0.0) + vec2(0.0, HIT_PX + 1.0)), None);
    assert_eq!(hit_test(&sc, &cam, screen, pos2(999.0, 599.0)), None);
    assert!(BERTH_H < HIT_PX * 2.0);
}

#[test]
fn the_nearest_signal_wins() {
    let (sc, _, screen) = setup(Some("West"));
    let far = Camera { centre: pos2(100.0, 0.0), scale: 0.1 };
    let p = far.to_screen(screen, pos2(100.0, -5.0)) + vec2(0.0, 0.3);
    assert_eq!(hit_test(&sc, &far, screen, p), hit(Target::Signal(s("W1")), true), "W1 and W2 overlap when zoomed out; W1 is nearer");
}

#[test]
fn a_spectator_can_hover_but_not_work() {
    let (sc, cam, screen) = setup(None);
    assert_eq!(hit_test(&sc, &cam, screen, at(&cam, screen, 215.0, 5.0)), hit(Target::Signal(s("C")), false));
    assert_eq!(hit_test(&sc, &cam, screen, at(&cam, screen, 207.5, 0.0)), hit(Target::Points(s("P")), false));
}
```

Create `crates/client-ui/tests/paint.rs`:
```rust
//! The drawing: colours and marks, checked as shapes (no GPU, no fonts).

mod common;

use client_ui::camera::Camera;
use client_ui::paint::*;
use client_ui::scene::Scene;
use common::*;
use egui::{Color32, Pos2, Rect, Shape, pos2, vec2};
use protocol::*;

fn screen() -> Rect {
    Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0))
}

fn lines_of(d: &Drawing, colour: Color32) -> Vec<[Pos2; 2]> {
    d.shapes
        .iter()
        .filter_map(|s| match s {
            Shape::LineSegment { points, stroke } if stroke.color == colour && stroke.width == TRACK_W => Some(*points),
            _ => None,
        })
        .collect()
}

fn circles(d: &Drawing) -> Vec<(Pos2, f32, Color32, Color32)> {
    d.shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Circle(c) => Some((c.center, c.radius, c.fill, c.stroke.color)),
            _ => None,
        })
        .collect()
}

struct Rig {
    sc: Scene,
    cam: Camera,
    view: View,
}

impl Rig {
    fn new(area: Option<&str>) -> Rig {
        let sc = Scene::build(&layout_for(area)).unwrap();
        let cam = Camera::fit(sc.all.unwrap(), screen());
        Rig { sc, cam, view: view_for(area) }
    }

    fn draw(&self, selected: Option<&str>, exits: &[ExitName], flashing: Option<&str>, time: f64) -> Drawing {
        draw(&self.sc, &self.cam, screen(), &PaintState { view: Some(&self.view), selected, exits, flashing, time })
    }

    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.cam.to_screen(screen(), pos2(x, y))
    }
}

#[test]
fn colours_follow_the_spec() {
    let v = |occupied, held| SectionView { occupied, held };
    assert_eq!(track_colour(None), TRACK_FREE);
    assert_eq!(track_colour(Some(&v(false, Held::Free))), TRACK_FREE);
    assert_eq!(track_colour(Some(&v(false, Held::Path))), ROUTE);
    assert_eq!(track_colour(Some(&v(false, Held::Overlap))), OVERLAP);
    assert_eq!(track_colour(Some(&v(true, Held::Path))), OCCUPIED, "occupied wins");
    assert_eq!(dim(Color32::from_rgb(200, 100, 51)), Color32::from_rgb(100, 50, 25));
    assert_eq!(lamps(Aspect::DoubleYellow), (YELLOW, Some(YELLOW)));
    assert_eq!(lamps(Aspect::Green), (GREEN, None));
    assert_eq!(BG, Color32::from_rgb(0x0B, 0x0B, 0x0F));
    assert!(blink_on(0.0) && !blink_on(0.3) && blink_on(0.5) && blink_on(-0.6) == blink_on(0.4));
}

#[test]
fn track_is_grey_white_along_a_route_red_when_occupied_and_dim_on_the_fringe() {
    let mut r = Rig::new(Some("West"));
    let d = r.draw(None, &[], None, 0.0);
    assert_eq!(lines_of(&d, TRACK_FREE), [[r.at(0.0, 0.0), r.at(100.0, 0.0)], [r.at(100.0, 0.0), r.at(200.0, 0.0)]]);
    assert_eq!(lines_of(&d, dim(TRACK_FREE)).len(), 3, "P's three legs are East's, on West's fringe");
    r.view.sections.insert(s("TW2"), SectionView { occupied: false, held: Held::Path });
    r.view.sections.insert(s("TW1"), SectionView { occupied: true, held: Held::Free });
    let d = r.draw(None, &[], None, 0.0);
    assert_eq!(lines_of(&d, ROUTE), [[r.at(100.0, 0.0), r.at(200.0, 0.0)]]);
    assert_eq!(lines_of(&d, OCCUPIED), [[r.at(0.0, 0.0), r.at(100.0, 0.0)]]);
}

#[test]
fn points_show_the_lying_leg_whole_and_a_gap_in_the_other() {
    let mut r = Rig::new(Some("East"));
    let (c, n, rv) = (r.at(207.5, 0.0), r.at(215.0, 0.0), r.at(215.0, 10.0));
    let d = r.draw(None, &[], None, 0.0);
    let legs = lines_of(&d, TRACK_FREE);
    assert!(legs.contains(&[c, n]), "normal lies: whole");
    assert!(legs.contains(&[c + (rv - c) * GAP, rv]), "reverse: from the gap");
    r.view.points.insert(s("P"), PointsView { position: PointsPos::Reverse, moving: true, locked: false });
    let lit = lines_of(&r.draw(None, &[], None, 0.0), TRACK_FREE);
    assert!(lit.contains(&[c, rv]) && lit.contains(&[c + (n - c) * GAP, n]));
    let dark = lines_of(&r.draw(None, &[], None, 0.3), TRACK_FREE);
    assert!(!dark.contains(&[c, rv]), "while moving, the lying leg flashes");
}

#[test]
fn signals_show_their_aspect_and_selection() {
    let mut r = Rig::new(Some("West"));
    r.view.signals.insert(s("W1"), Aspect::DoubleYellow);
    let exits = [ExitName::Signal(s("A"))];
    let d = r.draw(Some("W1"), &exits, None, 0.0);
    let cs = circles(&d);
    let w1 = r.at(100.0, -5.0);
    assert!(cs.contains(&(w1, LAMP_R, YELLOW, Color32::TRANSPARENT)));
    assert!(cs.iter().any(|c| c.0 == w1 + vec2(LAMP_R * 2.2, 0.0) && c.2 == YELLOW), "second yellow lamp along the direction of travel");
    assert!(cs.iter().any(|c| c.0 == w1 && c.3 == SELECT), "the entrance is ringed");
    assert!(cs.iter().any(|c| c.0 == r.at(200.0, -5.0) && c.3 == SELECT), "so is the lit exit");
    assert!(cs.contains(&(r.at(100.0, 5.0), LAMP_R, RED, Color32::TRANSPARENT)), "W2 red");
}

#[test]
fn a_refused_entrance_flashes() {
    let r = Rig::new(Some("West"));
    let flash = |t| circles(&r.draw(None, &[], Some("A"), t)).iter().any(|c| c.3 == FLASH);
    assert!(flash(0.0));
    assert!(!flash(0.3));
}

#[test]
fn berths_show_headcodes_in_yellow_and_exits_light_up() {
    let mut r = Rig::new(Some("West"));
    r.view.berths.insert(s("BA"), s("1E01"));
    let lit = [ExitName::Node(s("E"))];
    let d = r.draw(Some("A"), &lit, None, 0.0);
    let t = d.texts.iter().find(|t| t.text == "1E01").unwrap();
    assert_eq!((t.colour, t.monospace), (HEADCODE, true));
    let squares: Vec<Color32> = d
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Rect(rs) if rs.rect.width() == 7.0 => Some(rs.stroke.color),
            _ => None,
        })
        .collect();
    assert_eq!(squares.iter().filter(|c| **c == SELECT).count(), 1, "E lit, N and W not");
    assert_eq!(squares.len(), 3);
    assert!(d.texts.iter().any(|t| t.text == "West" && t.colour == LABEL));
}

#[test]
fn automatic_signals_carry_an_a_lit_while_auto_working() {
    let mut l = layout_for(Some("West"));
    l.routes[0].automatic = true;
    let sc = Scene::build(&l).unwrap();
    let cam = Camera::fit(sc.all.unwrap(), screen());
    let mut v = view_for(Some("West"));
    let a_colour = |v: &View| {
        let d = draw(&sc, &cam, screen(), &PaintState { view: Some(v), selected: None, exits: &[], flashing: None, time: 0.0 });
        d.texts.iter().find(|t| t.text == "A").map(|t| t.colour)
    };
    assert_eq!(a_colour(&v), Some(AUTO_OFF));
    v.routes.insert(s("W1-A"), RouteView { state: RouteState::Locked, auto_working: true });
    assert_eq!(a_colour(&v), Some(AUTO_ON));
}

#[test]
fn no_view_yet_draws_everything_idle() {
    let r = Rig::new(Some("West"));
    let d = draw(&r.sc, &r.cam, screen(), &PaintState { view: None, selected: None, exits: &[], flashing: None, time: 0.0 });
    assert_eq!(lines_of(&d, TRACK_FREE).len(), 2);
    assert!(circles(&d).iter().filter(|c| c.2 == RED).count() == 3, "signals default to red");
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `scripts/cargo test -p signalbox-client-ui`
Expected: FAIL to compile — no `src/lib.rs` / unresolved `client_ui::scene`.

- [ ] **Step 4: The scene**

Create `crates/client-ui/src/scene.rs`:
```rust
//! The diagram as shapes in layout coordinates, built once per layout from
//! its geometry and its lists. What the geometry lacks is left out; what
//! has no geometry at all gives no scene.

use std::collections::{BTreeMap, BTreeSet};

use egui::{Pos2, Rect, Vec2, pos2, vec2};
use protocol::{ExitName, Layout};

#[derive(Clone, Debug, PartialEq)]
pub struct TrackLine {
    pub segment: String,
    pub section: String,
    pub a: Pos2,
    pub b: Pos2,
    pub fringe: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PointsMark {
    pub name: String,
    pub section: String,
    pub at: Pos2,
    pub toe: Option<Pos2>,
    pub normal: Option<Pos2>,
    pub reverse: Option<Pos2>,
    pub fringe: bool,
    pub operable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignalMark {
    pub name: String,
    pub at: Pos2,
    /// Unit direction of travel past the signal, or zero when unknown.
    pub facing: Vec2,
    pub fringe: bool,
    pub operable: bool,
    /// Automatic routes starting here (an "A" is drawn, lit while one auto-works).
    pub auto_routes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BerthMark {
    pub name: String,
    pub at: Pos2,
    /// Drawn this far from `at` on screen (boundary berths sit above their exit).
    pub offset_px: Vec2,
    pub fringe: bool,
    pub operable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExitMark {
    pub node: String,
    pub at: Pos2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlatformMark {
    pub rect: Rect,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LabelMark {
    pub text: String,
    pub at: Pos2,
}

/// Where a boundary berth's box is drawn relative to its exit node.
pub const BOUNDARY_BERTH_OFFSET_PX: Vec2 = vec2(0.0, -18.0);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub tracks: Vec<TrackLine>,
    pub points: Vec<PointsMark>,
    pub signals: Vec<SignalMark>,
    pub berths: Vec<BerthMark>,
    pub exits: Vec<ExitMark>,
    pub platforms: Vec<PlatformMark>,
    pub labels: Vec<LabelMark>,
    /// Bounds of your own area's drawing (`None` for a spectator).
    pub own: Option<Rect>,
    /// Bounds of everything drawn.
    pub all: Option<Rect>,
}

fn pt(x: f64, y: f64) -> Option<Pos2> {
    let p = pos2(x as f32, y as f32);
    (p.x.is_finite() && p.y.is_finite()).then_some(p)
}

fn grow(r: &mut Option<Rect>, p: Pos2) {
    *r = Some(match r {
        Some(r) => r.union(Rect::from_min_max(p, p)),
        None => Rect::from_min_max(p, p),
    });
}

impl Scene {
    /// `None` when the layout carries no geometry.
    pub fn build(l: &Layout) -> Option<Scene> {
        let g = l.geometry.as_ref()?;
        let fringe_of: BTreeMap<&str, bool> = l.sections.iter().map(|s| (s.name.as_str(), s.fringe)).collect();
        let seg_of: BTreeMap<&str, (&str, &str, &str)> =
            l.segments.iter().map(|s| (s.name.as_str(), (s.section.as_str(), s.from.as_str(), s.to.as_str()))).collect();
        let other_area = |area: &str| l.area.as_deref().is_some_and(|mine| mine != area);
        let mut sc = Scene::default();
        for line in &g.lines {
            let (Some(a), Some(b), Some(&(section, _, _))) = (pt(line.x1, line.y1), pt(line.x2, line.y2), seg_of.get(line.segment.as_str()))
            else {
                continue;
            };
            sc.tracks.push(TrackLine {
                segment: line.segment.clone(),
                section: section.to_string(),
                a,
                b,
                fringe: fringe_of.get(section).copied().unwrap_or(true),
            });
        }
        for p in &g.points {
            let (Some(at), Some(info)) = (pt(p.x, p.y), l.points.iter().find(|i| i.name == p.node)) else { continue };
            let leg = |v: Option<[f64; 2]>| v.and_then(|[x, y]| pt(x, y));
            sc.points.push(PointsMark {
                name: p.node.clone(),
                section: info.section.clone(),
                at,
                toe: leg(p.toe),
                normal: leg(p.normal),
                reverse: leg(p.reverse),
                fringe: fringe_of.get(info.section.as_str()).copied().unwrap_or(true),
                operable: info.operable,
            });
        }
        for s in &g.signals {
            let (Some(at), Some(info)) = (pt(s.x, s.y), l.signals.iter().find(|i| i.name == s.signal)) else { continue };
            let facing = s.facing.map(|[x, y]| vec2(x as f32, y as f32)).filter(|v| v.length() > 0.0 && v.is_finite());
            sc.signals.push(SignalMark {
                name: s.signal.clone(),
                at,
                facing: facing.map_or(Vec2::ZERO, Vec2::normalized),
                fringe: other_area(&info.area),
                operable: info.operable,
                auto_routes: l.routes.iter().filter(|r| r.automatic && r.entrance == s.signal).map(|r| r.name.clone()).collect(),
            });
            for b in l.berths.iter().filter(|b| b.signal.as_deref() == Some(s.signal.as_str())) {
                if let Some(bat) = pt(s.berth_x, s.berth_y) {
                    sc.berths.push(BerthMark {
                        name: b.name.clone(),
                        at: bat,
                        offset_px: Vec2::ZERO,
                        fringe: other_area(&b.area),
                        operable: b.operable,
                    });
                }
            }
        }
        let node_at: BTreeMap<&str, Pos2> =
            g.nodes.iter().filter_map(|n| Some((n.node.as_str(), pt(n.x, n.y)?))).collect();
        let exit_nodes: BTreeSet<&str> = l
            .routes
            .iter()
            .filter_map(|r| match &r.exit {
                ExitName::Node(n) => Some(n.as_str()),
                ExitName::Signal(_) => None,
            })
            .collect();
        for n in &exit_nodes {
            if let Some(&at) = node_at.get(n) {
                sc.exits.push(ExitMark { node: n.to_string(), at });
            }
        }
        for b in &l.berths {
            if let Some(&at) = b.boundary.as_deref().and_then(|n| node_at.get(n)) {
                sc.berths.push(BerthMark {
                    name: b.name.clone(),
                    at,
                    offset_px: BOUNDARY_BERTH_OFFSET_PX,
                    fringe: other_area(&b.area),
                    operable: b.operable,
                });
            }
        }
        for p in &g.platforms {
            if let (Some(a), Some(b)) = (pt(p.x1, p.y1), pt(p.x2, p.y2)) {
                sc.platforms.push(PlatformMark { rect: Rect::from_two_pos(a, b), label: format!("{} {}", p.place, p.platform) });
            }
        }
        for t in &g.labels {
            if let Some(at) = pt(t.x, t.y) {
                sc.labels.push(LabelMark { text: t.text.clone(), at });
            }
        }
        for t in &sc.tracks {
            grow(&mut sc.all, t.a);
            grow(&mut sc.all, t.b);
            if !t.fringe && l.area.is_some() {
                grow(&mut sc.own, t.a);
                grow(&mut sc.own, t.b);
            }
        }
        for s in &sc.signals {
            grow(&mut sc.all, s.at);
            if !s.fringe && l.area.is_some() {
                grow(&mut sc.own, s.at);
            }
        }
        for p in &sc.points {
            grow(&mut sc.all, p.at);
        }
        Some(sc)
    }

    /// What "Fit" frames: your own area, or everything.
    pub fn fit_bounds(&self) -> Option<Rect> {
        self.own.or(self.all)
    }
}
```

- [ ] **Step 5: The camera**

Create `crates/client-ui/src/camera.rs`:
```rust
//! Layout coordinates ⇄ screen pixels: fit, zoom about a point, pan.

use egui::{Pos2, Rect, Vec2};

/// Pixels per layout unit, at the least and the most.
pub const MIN_SCALE: f32 = 0.02;
pub const MAX_SCALE: f32 = 50.0;
/// Fit leaves this fraction of the screen around the drawing.
pub const FIT_MARGIN: f32 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// The layout point at the centre of the screen rectangle.
    pub centre: Pos2,
    /// Pixels per layout unit.
    pub scale: f32,
}

fn usable(r: Rect) -> bool {
    r.min.is_finite() && r.max.is_finite() && r.width() >= 1.0 && r.height() >= 1.0
}

impl Camera {
    /// Frame `world` in `screen`, keeping the aspect ratio.
    pub fn fit(world: Rect, screen: Rect) -> Camera {
        if !world.min.is_finite() || !world.max.is_finite() {
            return Camera { centre: Pos2::ZERO, scale: 1.0 };
        }
        let centre = world.center();
        if !usable(screen) {
            return Camera { centre, scale: 1.0 };
        }
        let room = screen.size() * (1.0 - 2.0 * FIT_MARGIN);
        let fits = [room.x / world.width(), room.y / world.height()];
        let scale = fits.into_iter().filter(|s| s.is_finite() && *s > 0.0).fold(f32::INFINITY, f32::min);
        let scale = if scale.is_finite() { scale } else { 1.0 };
        Camera { centre, scale: scale.clamp(MIN_SCALE, MAX_SCALE) }
    }

    pub fn to_screen(&self, screen: Rect, p: Pos2) -> Pos2 {
        screen.center() + (p - self.centre) * self.scale
    }

    pub fn to_world(&self, screen: Rect, p: Pos2) -> Pos2 {
        self.centre + (p - screen.center()) / self.scale
    }

    /// Zoom by `factor` keeping the layout point under `at` where it is.
    pub fn zoom_at(&mut self, screen: Rect, at: Pos2, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 || !at.is_finite() {
            return;
        }
        let before = self.to_world(screen, at);
        self.scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let after = self.to_world(screen, at);
        self.centre += before - after;
    }

    /// Drag the drawing by `delta` pixels.
    pub fn pan(&mut self, delta: Vec2) {
        if delta.is_finite() {
            self.centre -= delta / self.scale;
        }
    }
}
```

- [ ] **Step 6: Hit-testing**

Create `crates/client-ui/src/hit.rs`:
```rust
//! What is under the pointer: signals first, then berths, exits, points
//! and finally track, each within a fixed distance in pixels.

use client_core::Target;
use egui::{Pos2, Rect, vec2};

use crate::camera::Camera;
use crate::scene::Scene;

/// How near (pixels) the pointer must be to a signal, exit, points or track.
pub const HIT_PX: f32 = 8.0;
/// A berth box, in pixels.
pub const BERTH_W: f32 = 34.0;
pub const BERTH_H: f32 = 14.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub target: Target,
    /// Yours to work: clicks and menus apply. Otherwise hover only.
    pub clickable: bool,
}

/// The berth box on screen.
pub fn berth_rect(cam: &Camera, screen: Rect, at: Pos2, offset_px: egui::Vec2) -> Rect {
    Rect::from_center_size(cam.to_screen(screen, at) + offset_px, vec2(BERTH_W, BERTH_H))
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 == 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

fn nearest<'a, T>(items: impl Iterator<Item = (&'a T, f32)>) -> Option<&'a T>
where
    T: 'a,
{
    items.filter(|(_, d)| *d <= HIT_PX).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(t, _)| t)
}

pub fn hit_test(scene: &Scene, cam: &Camera, screen: Rect, p: Pos2) -> Option<Hit> {
    let at = |q: Pos2| cam.to_screen(screen, q);
    if let Some(s) = nearest(scene.signals.iter().map(|s| (s, at(s.at).distance(p)))) {
        return Some(Hit { target: Target::Signal(s.name.clone()), clickable: s.operable });
    }
    if let Some(b) = scene.berths.iter().find(|b| berth_rect(cam, screen, b.at, b.offset_px).contains(p)) {
        return Some(Hit { target: Target::Berth(b.name.clone()), clickable: b.operable });
    }
    if let Some(e) = nearest(scene.exits.iter().map(|e| (e, at(e.at).distance(p)))) {
        return Some(Hit { target: Target::Exit(e.node.clone()), clickable: true });
    }
    if let Some(pm) = nearest(scene.points.iter().map(|m| (m, at(m.at).distance(p)))) {
        return Some(Hit { target: Target::Points(pm.name.clone()), clickable: pm.operable });
    }
    nearest(scene.tracks.iter().map(|t| (t, dist_to_segment(p, at(t.a), at(t.b)))))
        .map(|t| Hit { target: Target::Section(t.section.clone()), clickable: false })
}
```

- [ ] **Step 7: Drawing**

Create `crates/client-ui/src/paint.rs`:
```rust
//! Drawing the diagram (spec D1 §3.1): an IECC-style VDU on a near-black
//! background. `draw` is pure (shapes and text in screen pixels, testable
//! without a GPU or fonts); `paint` hands them to an egui `Painter`.

use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, vec2};
use protocol::{Aspect, ExitName, Held, PointsPos, SectionView, View};

use crate::camera::Camera;
use crate::hit::berth_rect;
use crate::scene::{PointsMark, Scene};

pub const BG: Color32 = Color32::from_rgb(0x0B, 0x0B, 0x0F);
pub const TRACK_FREE: Color32 = Color32::from_rgb(0x6E, 0x6E, 0x6E);
pub const ROUTE: Color32 = Color32::from_rgb(0xEB, 0xEB, 0xEB);
pub const OVERLAP: Color32 = Color32::from_rgb(0xA0, 0xA0, 0xA0);
pub const OCCUPIED: Color32 = Color32::from_rgb(0xE6, 0x28, 0x28);
pub const RED: Color32 = Color32::from_rgb(0xE6, 0x1E, 0x1E);
pub const YELLOW: Color32 = Color32::from_rgb(0xFA, 0xD2, 0x00);
pub const GREEN: Color32 = Color32::from_rgb(0x00, 0xDC, 0x50);
pub const HEADCODE: Color32 = YELLOW;
pub const BERTH_EMPTY: Color32 = Color32::from_rgb(0x46, 0x46, 0x46);
pub const SELECT: Color32 = Color32::from_rgb(0x00, 0xC8, 0xFF);
pub const FLASH: Color32 = Color32::from_rgb(0xFF, 0x3C, 0xFF);
pub const PLATFORM: Color32 = Color32::from_rgb(0x23, 0x23, 0x4A);
pub const LABEL: Color32 = Color32::from_rgb(0x96, 0x96, 0xAA);
pub const AUTO_ON: Color32 = ROUTE;
pub const AUTO_OFF: Color32 = BERTH_EMPTY;

/// Line widths and sizes in pixels, whatever the zoom.
pub const TRACK_W: f32 = 3.0;
pub const LAMP_R: f32 = 4.0;
pub const STUB_PX: f32 = 9.0;
/// Where the non-lying leg of points starts, as a fraction of its length.
pub const GAP: f32 = 0.5;

/// A section's colour: occupied red, route white, overlap dim white, else grey.
pub fn track_colour(v: Option<&SectionView>) -> Color32 {
    match v {
        Some(s) if s.occupied => OCCUPIED,
        Some(s) if s.held == Held::Path => ROUTE,
        Some(s) if s.held == Held::Overlap => OVERLAP,
        _ => TRACK_FREE,
    }
}

/// Half brightness, for the fringe.
pub fn dim(c: Color32) -> Color32 {
    Color32::from_rgb(c.r() / 2, c.g() / 2, c.b() / 2)
}

fn shade(c: Color32, fringe: bool) -> Color32 {
    if fringe { dim(c) } else { c }
}

/// The lit lamps: one, or two for double yellow.
pub fn lamps(a: Aspect) -> (Color32, Option<Color32>) {
    match a {
        Aspect::Red => (RED, None),
        Aspect::Yellow => (YELLOW, None),
        Aspect::DoubleYellow => (YELLOW, Some(YELLOW)),
        Aspect::Green => (GREEN, None),
    }
}

/// On for the first half of every quarter second pair: a 2 Hz flash.
pub fn blink_on(time: f64) -> bool {
    (time * 4.0).floor().rem_euclid(2.0) == 0.0
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextItem {
    pub at: Pos2,
    pub anchor: Align2,
    pub text: String,
    pub size: f32,
    pub colour: Color32,
    pub monospace: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawing {
    pub shapes: Vec<Shape>,
    pub texts: Vec<TextItem>,
}

/// What changes from frame to frame.
pub struct PaintState<'a> {
    pub view: Option<&'a View>,
    /// The chosen entrance.
    pub selected: Option<&'a str>,
    /// Exits to light up.
    pub exits: &'a [ExitName],
    /// The signal flashing for a refusal.
    pub flashing: Option<&'a str>,
    /// Seconds, for flashing.
    pub time: f64,
}

fn points_shapes(out: &mut Vec<Shape>, p: &PointsMark, to: &dyn Fn(Pos2) -> Pos2, st: &PaintState, colour: Color32) {
    let pv = st.view.and_then(|v| v.points.get(&p.name));
    let lying = pv.map_or(PointsPos::Normal, |v| v.position);
    let moving = pv.is_some_and(|v| v.moving);
    let (lie, other) = match lying {
        PointsPos::Normal => (p.normal, p.reverse),
        PointsPos::Reverse => (p.reverse, p.normal),
    };
    let c = to(p.at);
    let stroke = Stroke::new(TRACK_W, colour);
    if let Some(t) = p.toe {
        out.push(Shape::line_segment([c, to(t)], stroke));
    }
    if let Some(l) = lie {
        if !moving || blink_on(st.time) {
            out.push(Shape::line_segment([c, to(l)], stroke));
        }
    }
    if let Some(o) = other {
        let end = to(o);
        out.push(Shape::line_segment([c + (end - c) * GAP, end], stroke));
    }
}

pub fn draw(scene: &Scene, cam: &Camera, screen: Rect, st: &PaintState) -> Drawing {
    let to = |p: Pos2| cam.to_screen(screen, p);
    let mut d = Drawing::default();
    let section = |name: &str| st.view.and_then(|v| v.sections.get(name));
    for p in &scene.platforms {
        let r = Rect::from_two_pos(to(p.rect.min), to(p.rect.max));
        d.shapes.push(Shape::rect_filled(r, CornerRadius::ZERO, PLATFORM));
        d.texts.push(TextItem { at: r.center(), anchor: Align2::CENTER_CENTER, text: p.label.clone(), size: 9.0, colour: LABEL, monospace: false });
    }
    for t in &scene.tracks {
        let c = shade(track_colour(section(&t.section)), t.fringe);
        d.shapes.push(Shape::line_segment([to(t.a), to(t.b)], Stroke::new(TRACK_W, c)));
    }
    for p in &scene.points {
        let c = shade(track_colour(section(&p.section)), p.fringe);
        points_shapes(&mut d.shapes, p, &to, st, c);
    }
    for e in &scene.exits {
        let lit = st.exits.contains(&ExitName::Node(e.node.clone()));
        let r = Rect::from_center_size(to(e.at), vec2(7.0, 7.0));
        d.shapes.push(Shape::rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.5, if lit { SELECT } else { TRACK_FREE }), StrokeKind::Middle));
    }
    for s in &scene.signals {
        let at = to(s.at);
        let aspect = st.view.and_then(|v| v.signals.get(&s.name)).copied().unwrap_or(Aspect::Red);
        let (first, second) = lamps(aspect);
        d.shapes.push(Shape::line_segment([at - s.facing * STUB_PX, at], Stroke::new(1.5, shade(ROUTE, s.fringe))));
        d.shapes.push(Shape::circle_filled(at, LAMP_R, shade(first, s.fringe)));
        if let Some(c) = second {
            d.shapes.push(Shape::circle_filled(at + s.facing * (LAMP_R * 2.2), LAMP_R, shade(c, s.fringe)));
        }
        if st.selected == Some(s.name.as_str()) || st.exits.contains(&ExitName::Signal(s.name.clone())) {
            d.shapes.push(Shape::circle_stroke(at, LAMP_R + 3.5, Stroke::new(2.0, SELECT)));
        }
        if st.flashing == Some(s.name.as_str()) && blink_on(st.time) {
            d.shapes.push(Shape::circle_stroke(at, LAMP_R + 6.0, Stroke::new(2.0, FLASH)));
        }
        if !s.auto_routes.is_empty() {
            let on = st.view.is_some_and(|v| s.auto_routes.iter().any(|r| v.routes.get(r).is_some_and(|rv| rv.auto_working)));
            d.texts.push(TextItem {
                at: at + vec2(LAMP_R + 3.0, -(LAMP_R + 3.0)),
                anchor: Align2::LEFT_BOTTOM,
                text: "A".into(),
                size: 9.0,
                colour: shade(if on { AUTO_ON } else { AUTO_OFF }, s.fringe),
                monospace: true,
            });
        }
    }
    for b in &scene.berths {
        let r = berth_rect(cam, screen, b.at, b.offset_px);
        match st.view.and_then(|v| v.berths.get(&b.name)) {
            Some(h) => {
                d.shapes.push(Shape::rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, shade(BERTH_EMPTY, b.fringe)), StrokeKind::Middle));
                d.texts.push(TextItem {
                    at: r.center(),
                    anchor: Align2::CENTER_CENTER,
                    text: h.clone(),
                    size: 11.0,
                    colour: shade(HEADCODE, b.fringe),
                    monospace: true,
                });
            }
            None => d.shapes.push(Shape::rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, dim(shade(BERTH_EMPTY, b.fringe))), StrokeKind::Middle)),
        }
    }
    for l in &scene.labels {
        d.texts.push(TextItem { at: to(l.at), anchor: Align2::LEFT_TOP, text: l.text.clone(), size: 11.0, colour: LABEL, monospace: false });
    }
    d
}

/// Put a drawing on screen.
pub fn paint(p: &Painter, d: Drawing) {
    p.extend(d.shapes);
    for t in d.texts {
        let font = if t.monospace { FontId::monospace(t.size) } else { FontId::proportional(t.size) };
        p.text(t.at, t.anchor, t.text, font, t.colour);
    }
}
```

Create `crates/client-ui/src/lib.rs`:
```rust
//! The signalbox client's screens (spec D1 §3), drawn with egui over
//! `client_core::App`. No windowing, no GPU and no browser here: the web
//! shell (`client-web`, D2's desktop shell later) runs `UiApp::ui` in its
//! frame loop. The diagram's scene, camera, hit-testing and drawing are
//! pure and tested headless.

pub mod camera;
pub mod hit;
pub mod paint;
pub mod scene;
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `scripts/cargo test -p signalbox-client-ui`
Expected: PASS (`scene` 6, `camera` 5, `hit` 3, `paint` 8).
Run: `scripts/cargo build --workspace --all-targets`
Expected: no warnings.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock crates/client-ui
git commit -m "feat(client-ui): the diagram's scene, camera, hit-testing and drawing, tested headless"
```

---

### Task 6: `client-ui` — the screens: lobby, top bar, diagram, trains, alarms, menus

**Files:**
- Create: `crates/client-ui/src/screens.rs`
- Modify: `crates/client-ui/src/lib.rs` (`pub mod screens; pub use screens::UiApp;`)
- Test: `crates/client-ui/tests/screens.rs`

**Interfaces:**
- Consumes: Task 3/4's `client_core::App` (`tick`, `banner`, `link`, `reconnect_now`, lobby and game methods, `click`, `escape`, `valid_exits`, `menu`, `can_interpose`, `interpose`, `describe`), `InGame` accessors, `text::{fmt_hms, proposal_text, vote_text}`, `trains::train_list`; Task 5's `Scene`, `Camera`, `hit_test`, `paint::{draw, paint, PaintState}`; egui 0.36 (`Panel::top/right(..).show(ui, ..)`, `CentralPanel::default().frame(..).show(ui, ..)`, `Ui::allocate_painter`, `Response::{clicked, secondary_clicked, dragged_by, drag_delta, hover_pos, interact_pointer_pos, on_hover_text_at_pointer, context_menu}`, `Ui::close`, `Context::run_ui` in tests).
- Produces: `client_ui::UiApp` with `pub core: App`, `UiApp::new(App) -> UiApp`, `UiApp::ui(&mut self, &mut egui::Ui)` (one whole frame: ticks the app with `ui.input(|i| i.time)`, draws, asks for a repaint within 250 ms), `camera() -> Option<Camera>`, `diagram_rect() -> Option<Rect>`; `screens::ALARM` colour.

Text on screen uses only characters egui's default fonts have (they have no arrow: routes read "W1 to A", holders "West: ann"); `every_character_on_screen_has_a_glyph` guards it.

Layout per spec §3: a banner while the connection is not open (with "Use it here" after `replaced`); in the lobby, a "New game" row (layout, seed, start, Create) and the games grid (Join, or Resume for saved/crashed); in a game, a top bar (`<game> · <area or spectating> (<you>)`, the clock, speed, Pause/Resume and 1×/2×/4×/8× vote buttons, the open vote, penalty points, Fit, Release area, Leave; then `Players:` with `area: holder` and Claim buttons for robot-held areas while spectating), the right panel (TRAINS grid: headcode, state, next call, booked HH:MM, +minutes late; ALARMS newest first, alarms in red), and the diagram (fitted to your area on join and whenever your area changes; wheel or pinch zooms about the pointer, primary drag pans, click works, right-click opens the menu, Esc clears, hover shows `describe`). "No diagram for this layout" when the layout has no geometry.

- [ ] **Step 1: Write the failing tests**

Create `crates/client-ui/tests/screens.rs`:
```rust
//! The screens, run headless: egui frames with synthetic input, the app
//! talking to an in-process twobox game through a `MemTransport`.

mod common;

use client_core::{App, MemHandle, MemTransport};
use client_ui::UiApp;
use common::*;
use egui::{Event, FullOutput, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape, pos2, vec2};
use game::{Game, GameMeta};
use protocol::*;

struct Rig {
    ctx: egui::Context,
    ui: UiApp,
    h: MemHandle,
    game: Game,
    t: f64,
    events: Vec<Event>,
    /// Lobby frames the app sent (game frames go to the game).
    lobby_sent: Vec<LobbyMsg>,
}

impl Rig {
    /// Open, in the lobby, the front's lists delivered.
    fn lobby(world: signalbox_core::world::World) -> Rig {
        let (tr, h) = MemTransport::new();
        let core = App::new(Box::new(tr), 0.0);
        h.open();
        let game = Game::new(world, GameMeta { layout: s("twobox"), seed: 1 });
        let mut r = Rig { ctx: egui::Context::default(), ui: UiApp::new(core), h, game, t: 0.0, events: vec![], lobby_sent: vec![] };
        r.frame();
        r.lobby_sent.clear();
        r.h.push(ServerFrame::Lobby(LobbyReply::Layouts { layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West"), s("East")] }] }));
        r.frame();
        r
    }

    /// In the game as "ann", holding `area`.
    fn in_game(world: signalbox_core::world::World, area: Option<&str>) -> Rig {
        let mut r = Rig::lobby(world);
        r.h.push(ServerFrame::Lobby(LobbyReply::Joined { game: s("g-test"), you: s("ann") }));
        for (_, m) in r.game.connect("ann") {
            r.h.push(ServerFrame::Game(m));
        }
        if let Some(a) = area {
            for (_, m) in r.game.handle("ann", ClientMsg::Claim { area: s(a) }) {
                r.h.push(ServerFrame::Game(m));
            }
        }
        r.frame();
        r.frame();
        r
    }

    /// One frame of 0.1 s; the game runs alongside and answers.
    fn frame(&mut self) -> FullOutput {
        self.t += 0.1;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
            time: Some(self.t),
            events: std::mem::take(&mut self.events),
            ..RawInput::default()
        };
        let mut out = self.ctx.run_ui(input, |ui| self.ui.ui(ui));
        out.textures_delta.clear(); // no GPU to upload the font atlas to
        for f in self.h.take_sent() {
            match f {
                ClientFrame::Game(m) => {
                    for (p, reply) in self.game.handle("ann", m) {
                        if p == "ann" {
                            self.h.push(ServerFrame::Game(reply));
                        }
                    }
                }
                ClientFrame::Lobby(m) => self.lobby_sent.push(m),
            }
        }
        let mut out_msgs = self.game.advance(0.1);
        out_msgs.extend(self.game.flush());
        for (p, m) in out_msgs {
            if p == "ann" {
                self.h.push(ServerFrame::Game(m));
            }
        }
        out
    }

    fn click(&mut self, at: Pos2, button: PointerButton) {
        self.events.push(Event::PointerMoved(at));
        self.frame();
        self.events.push(Event::PointerButton { pos: at, button, pressed: true, modifiers: Modifiers::default() });
        self.frame();
        self.events.push(Event::PointerButton { pos: at, button, pressed: false, modifiers: Modifiers::default() });
        self.frame();
    }

    /// Where a layout point is on screen now.
    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.ui.camera().unwrap().to_screen(self.ui.diagram_rect().unwrap(), pos2(x, y))
    }

    fn view(&self) -> View {
        self.ui.core.game().unwrap().view().unwrap().clone()
    }
}

/// Every piece of text drawn, with where it was drawn.
fn texts(out: &FullOutput) -> Vec<(String, Rect)> {
    out.shapes
        .iter()
        .filter_map(|c| match &c.shape {
            Shape::Text(t) => Some((t.galley.text().to_string(), t.galley.rect.translate(t.pos.to_vec2()))),
            _ => None,
        })
        .collect()
}

fn has_text(out: &FullOutput, want: &str) -> bool {
    texts(out).iter().any(|(t, _)| t.contains(want))
}

#[test]
fn the_lobby_lists_games_and_creates_one() {
    let mut r = Rig::lobby(drawn_twobox());
    let info = GameInfo {
        id: s("g-abc"),
        layout: s("twobox"),
        state: GameState::Crashed,
        sim_time: 25_300.0,
        areas: vec![AreaHolder { name: s("West"), holder: Some(s("bob")) }, AreaHolder { name: s("East"), holder: None }],
        players: vec![s("bob")],
        error: Some(s("disk full")),
    };
    r.h.push(ServerFrame::Lobby(LobbyReply::Games { games: vec![info] }));
    r.frame();
    let out = r.frame();
    for want in ["signalbox", "New game", "twobox", "g-abc", "crashed: disk full", "07:01:40", "West (bob), East (robot)", "Resume"] {
        assert!(has_text(&out, want), "{want} in {:?}", texts(&out));
    }
    let create = texts(&out).into_iter().find(|(t, _)| t == "Create").unwrap().1.center();
    r.click(create, PointerButton::Primary);
    assert_eq!(r.lobby_sent, [LobbyMsg::CreateGame { layout: s("twobox"), seed: None, start: None }]);
}

#[test]
fn the_game_screen_shows_bar_trains_alarms_and_the_fitted_diagram() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let out = r.frame();
    for want in ["g-test · West (ann)", "07:00:0", "1×", "TRAINS", "ALARMS", "West: ann", "East: robot", "1E01", "Leave"] {
        assert!(has_text(&out, want), "{want} in {:?}", texts(&out));
    }
    let rect = r.ui.diagram_rect().unwrap();
    let own = Rect::from_min_max(pos2(0.0, -5.0), pos2(200.0, 5.0));
    assert_eq!(r.ui.camera(), Some(client_ui::camera::Camera::fit(own, rect)), "fitted to West on join");
    let lines = out.shapes.iter().filter(|c| matches!(c.shape, Shape::LineSegment { .. })).count();
    assert!(lines >= 5, "track and points drawn: {lines}");
}

#[test]
fn clicking_entrance_then_exit_sets_a_route() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let w1 = r.at(100.0, -5.0);
    r.click(w1, PointerButton::Primary);
    assert_eq!(r.ui.core.game().unwrap().selected(), Some("W1"));
    let a = r.at(200.0, -5.0);
    r.click(a, PointerButton::Primary);
    for _ in 0..20 {
        r.frame();
    }
    assert!(r.view().routes.contains_key("W1-A"), "{:?}", r.view().routes);
    assert_eq!(r.ui.core.game().unwrap().selected(), None);
}

#[test]
fn esc_clears_the_entrance_and_dragging_pans() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let w1 = r.at(100.0, -5.0);
    r.click(w1, PointerButton::Primary);
    r.events.push(Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::default() });
    r.frame();
    assert_eq!(r.ui.core.game().unwrap().selected(), None);
    let before = r.ui.camera().unwrap().centre;
    let start = r.at(50.0, 40.0);
    r.events.push(Event::PointerMoved(start));
    r.frame();
    r.events.push(Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::default() });
    r.frame();
    for i in 1..=5 {
        r.events.push(Event::PointerMoved(start + vec2(20.0 * i as f32, 0.0)));
        r.frame();
    }
    r.events.push(Event::PointerButton { pos: start + vec2(100.0, 0.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::default() });
    r.frame();
    let after = r.ui.camera().unwrap().centre;
    assert!(after.x < before.x, "dragging right moves the drawing right: {before:?} → {after:?}");
    assert_eq!(r.ui.core.game().unwrap().selected(), None, "a drag is not a click");
}

#[test]
fn right_click_opens_the_menu_for_what_is_under_the_pointer() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let ba = r.at(190.0, -15.0);
    r.click(ba, PointerButton::Secondary);
    let out = r.frame();
    assert!(has_text(&out, "Berth BA: empty"), "{:?}", texts(&out));
    assert!(has_text(&out, "Interpose"));
}

#[test]
fn a_layout_without_geometry_says_so() {
    let world = signalbox_core::world::World::from_json(
        &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../game/tests/fixtures/twobox.json")).unwrap(),
    )
    .unwrap();
    let mut r = Rig::in_game(world, Some("West"));
    let out = r.frame();
    assert!(has_text(&out, "No diagram for this layout"), "{:?}", texts(&out));
    assert!(has_text(&out, "TRAINS") && has_text(&out, "1E01"));
}

#[test]
fn a_lost_connection_shows_the_banner() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    r.h.close();
    r.frame();
    let out = r.frame();
    assert!(has_text(&out, "Connection lost. Reconnecting in"), "{:?}", texts(&out));
    assert!(has_text(&out, "g-test · West (ann)"), "the game stays on screen");
}

/// egui's default fonts lack some symbols (an arrow, for one) and draw a box
/// instead: everything the screens show must have a glyph.
#[test]
fn every_character_on_screen_has_a_glyph() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let (w1, a) = (r.at(100.0, -5.0), r.at(200.0, -5.0));
    r.click(w1, PointerButton::Primary);
    r.click(a, PointerButton::Primary);
    r.click(w1, PointerButton::Primary);
    r.click(a, PointerButton::Primary);
    for _ in 0..10 {
        r.frame();
    }
    r.click(w1, PointerButton::Secondary);
    let with_menu = r.frame();
    r.h.close();
    r.frame();
    let with_banner = r.frame();
    let shown: String = texts(&with_menu).into_iter().chain(texts(&with_banner)).map(|(t, _)| t).collect();
    assert!(shown.contains("Refused") && shown.contains("Cancel route W1 to A"), "{shown}");
    let missing: Vec<char> = r.ctx.fonts_mut(|f| {
        shown
            .chars()
            .filter(|c| !c.is_whitespace())
            .filter(|&c| !f.has_glyph(&egui::FontId::proportional(14.0), c) && !f.has_glyph(&egui::FontId::monospace(14.0), c))
            .collect()
    });
    assert!(missing.is_empty(), "no glyph for {missing:?}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `scripts/cargo test -p signalbox-client-ui --test screens`
Expected: FAIL to compile — no `client_ui::UiApp`.

- [ ] **Step 3: The screens**

Create `crates/client-ui/src/screens.rs`:
```rust
//! The screens (spec D1 §3): the lobby, and in a game the top bar (game,
//! area, clock, votes, players), the diagram, and the train list and alarms
//! on the right. `UiApp::ui` is the whole frame; the shell calls it.

use std::time::Duration;

use client_core::text::{fmt_hms, proposal_text, vote_text};
use client_core::trains::train_list;
use client_core::{App, Link, Target};
use egui::{Align2, Color32, CornerRadius, FontId, Frame, Key, PointerButton, Rect, RichText, Sense, Ui};
use protocol::{GameState, Proposal, TrainState};

use crate::camera::Camera;
use crate::hit::hit_test;
use crate::paint::{self, BG, PaintState};
use crate::scene::Scene;

/// Alarms and the connection banner.
pub const ALARM: Color32 = Color32::from_rgb(0xFF, 0x5A, 0x5A);
/// How far one wheel "line" (egui points of scroll) zooms.
const ZOOM_PER_POINT: f32 = 1.0 / 200.0;

#[derive(Default)]
struct NewGame {
    layout: usize,
    seed: String,
    start: String,
}

pub struct UiApp {
    pub core: App,
    scene: Option<Scene>,
    /// (game, layout generation) the scene was built for.
    scene_key: Option<(String, u64)>,
    cam: Option<Camera>,
    /// (game, area) the camera was fitted for; a change fits again.
    fitted: Option<(String, Option<String>)>,
    diagram: Option<Rect>,
    /// What the open right-click menu is about.
    menu_target: Option<Target>,
    headcode: String,
    new_game: NewGame,
}

impl UiApp {
    pub fn new(core: App) -> UiApp {
        UiApp {
            core,
            scene: None,
            scene_key: None,
            cam: None,
            fitted: None,
            diagram: None,
            menu_target: None,
            headcode: String::new(),
            new_game: NewGame::default(),
        }
    }

    /// The diagram's camera (tests and the "Fit" button).
    pub fn camera(&self) -> Option<Camera> {
        self.cam
    }

    /// Where the diagram was drawn last frame.
    pub fn diagram_rect(&self) -> Option<Rect> {
        self.diagram
    }

    /// One frame: move the app on with egui's clock, then draw.
    pub fn ui(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        self.core.tick(now);
        if let Some(b) = self.core.banner() {
            egui::Panel::top("banner").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(b).color(ALARM).strong());
                    if self.core.link() == Link::Replaced && ui.button("Use it here").clicked() {
                        self.core.reconnect_now();
                    }
                });
            });
        }
        if self.core.game().is_some() {
            self.game(ui, now);
        } else {
            self.lobby(ui);
        }
        // Clocks, flashing and reconnect timers move without input.
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }

    fn lobby(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("signalbox");
            if let Some(n) = self.core.lobby_note() {
                ui.label(RichText::new(n).color(ALARM));
            }
            ui.separator();
            ui.label(RichText::new("New game").strong());
            let layouts: Vec<String> = self.core.layouts().iter().map(|l| l.name.clone()).collect();
            if layouts.is_empty() {
                ui.label("No layouts yet.");
            } else {
                self.new_game.layout = self.new_game.layout.min(layouts.len() - 1);
                ui.horizontal(|ui| {
                    egui::ComboBox::from_label("Layout").selected_text(layouts[self.new_game.layout].as_str()).show_ui(ui, |ui| {
                        for (i, name) in layouts.iter().enumerate() {
                            ui.selectable_value(&mut self.new_game.layout, i, name.as_str());
                        }
                    });
                    ui.label("Seed");
                    ui.add(egui::TextEdit::singleline(&mut self.new_game.seed).desired_width(90.0).hint_text("random"));
                    ui.label("Start");
                    ui.add(egui::TextEdit::singleline(&mut self.new_game.start).desired_width(70.0).hint_text("HH:MM"));
                    if ui.button("Create").clicked() {
                        let seed = self.new_game.seed.trim().parse().ok();
                        let start = Some(self.new_game.start.trim().to_string()).filter(|s| !s.is_empty());
                        self.core.create_game(&layouts[self.new_game.layout], seed, start);
                    }
                });
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Games").strong());
                if ui.button("Refresh").clicked() {
                    self.core.refresh();
                }
            });
            let games = self.core.games().to_vec();
            if games.is_empty() {
                ui.label("No games yet.");
                return;
            }
            let mut join = None;
            egui::Grid::new("games").striped(true).show(ui, |ui| {
                for h in ["Game", "Layout", "State", "Time", "Areas", "Players", ""] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for g in &games {
                    ui.label(&g.id);
                    ui.label(&g.layout);
                    let state = match g.state {
                        GameState::Running => "running".to_string(),
                        GameState::Saved => "saved".to_string(),
                        GameState::Crashed => format!("crashed: {}", g.error.as_deref().unwrap_or("?")),
                    };
                    ui.label(state);
                    ui.label(fmt_hms(g.sim_time));
                    let areas: Vec<String> =
                        g.areas.iter().map(|a| format!("{} ({})", a.name, a.holder.as_deref().unwrap_or("robot"))).collect();
                    ui.label(areas.join(", "));
                    ui.label(g.players.join(", "));
                    if ui.button(if g.state == GameState::Running { "Join" } else { "Resume" }).clicked() {
                        join = Some(g.id.clone());
                    }
                    ui.end_row();
                }
            });
            if let Some(id) = join {
                self.core.join(&id);
            }
        });
    }

    fn game(&mut self, ui: &mut Ui, now: f64) {
        self.top_bar(ui);
        egui::Panel::right("side").default_size(330.0).show(ui, |ui| self.side(ui));
        egui::CentralPanel::default().frame(Frame::NONE.fill(BG)).show(ui, |ui| self.diagram_ui(ui, now));
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        let Some(g) = self.core.game() else { return };
        let title = format!("{} · {} ({})", g.id, g.area().unwrap_or("spectating"), g.you);
        let view = g.view().cloned();
        let areas: Vec<String> = g.layout().map(|l| l.areas.clone()).unwrap_or_default();
        let holding = g.area().is_some();
        let mut act: Vec<Box<dyn FnOnce(&mut App)>> = Vec::new();
        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(title).strong());
                if let Some(v) = &view {
                    ui.label(RichText::new(fmt_hms(v.sim_time)).monospace().size(16.0));
                    ui.label(if v.paused { "paused".to_string() } else { format!("{}×", v.speed) });
                    let pause = if v.paused { Proposal::Resume } else { Proposal::Pause };
                    if ui.button(proposal_text(pause)).clicked() {
                        act.push(Box::new(move |a| a.vote(pause)));
                    }
                    for x in [1u8, 2, 4, 8] {
                        if ui.selectable_label(!v.paused && v.speed == x, format!("{x}×")).clicked() {
                            act.push(Box::new(move |a| a.vote(Proposal::Speed { x })));
                        }
                    }
                    if let Some(vote) = &v.vote {
                        ui.label(RichText::new(vote_text(vote)).color(paint::YELLOW));
                    }
                    if let Some(score) = v.score {
                        ui.label(format!("Penalty {score}"));
                    }
                }
                if ui.button("Fit").clicked() {
                    self.fitted = None;
                }
                if holding {
                    if ui.button("Release area").clicked() {
                        act.push(Box::new(|a| a.release()));
                    }
                }
                if ui.button("Leave").clicked() {
                    act.push(Box::new(|a| a.leave()));
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Players:");
                for area in &areas {
                    let holder = view.as_ref().and_then(|v| v.holders.get(area)).map_or("robot", String::as_str);
                    ui.label(format!("{area}: {holder}"));
                    if !holding && holder == "robot" && ui.small_button("Claim").clicked() {
                        let area = area.clone();
                        act.push(Box::new(move |a| a.claim(&area)));
                    }
                }
            });
        });
        for f in act {
            f(&mut self.core);
        }
    }

    fn side(&mut self, ui: &mut Ui) {
        let Some(g) = self.core.game() else { return };
        ui.label(RichText::new("TRAINS").strong());
        egui::ScrollArea::vertical().id_salt("trains").max_height(ui.available_height() * 0.5).show(ui, |ui| {
            let Some(v) = g.view() else { return };
            egui::Grid::new("train_list").striped(true).show(ui, |ui| {
                for (h, r) in train_list(v) {
                    ui.label(RichText::new(h).monospace().color(paint::HEADCODE));
                    ui.label(match r.state {
                        TrainState::AtPlatform => "at platform",
                        TrainState::InArea => "in area",
                        TrainState::Approaching => "approaching",
                        TrainState::Due => "due",
                    });
                    let next = match (&r.next_place, &r.next_platform) {
                        (Some(p), Some(pf)) => format!("{p} {pf}"),
                        (Some(p), None) => p.clone(),
                        (None, _) => "—".to_string(),
                    };
                    ui.label(next);
                    ui.label(r.booked.map_or(String::new(), |b| fmt_hms(b)[..5].to_string()));
                    ui.label(if r.late_s > 0 { format!("+{}", r.late_s / 60) } else { String::new() });
                    ui.end_row();
                }
            });
        });
        ui.separator();
        ui.label(RichText::new("ALARMS").strong());
        egui::ScrollArea::vertical().id_salt("alarms").show(ui, |ui| {
            for e in g.log().entries().rev() {
                let when = e.sim_time.map(fmt_hms).unwrap_or_default();
                let text = RichText::new(format!("{when} {}", e.text));
                ui.label(if e.alarm { text.color(ALARM) } else { text });
            }
        });
    }

    fn diagram_ui(&mut self, ui: &mut Ui, now: f64) {
        let Some(g) = self.core.game() else { return };
        let key = (g.id.clone(), g.layout_gen());
        if self.scene_key.as_ref() != Some(&key) {
            self.scene = g.layout().and_then(Scene::build);
            self.scene_key = Some(key);
        }
        let fit_key = (g.id.clone(), g.area().map(str::to_string));
        let has_layout = g.layout().is_some();
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        self.diagram = Some(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, BG);
        let Some(scene) = &self.scene else {
            let msg = if has_layout { "No diagram for this layout" } else { "Waiting for the layout…" };
            painter.text(rect.center(), Align2::CENTER_CENTER, msg, FontId::proportional(16.0), paint::LABEL);
            return;
        };
        if self.fitted.as_ref() != Some(&fit_key) || self.cam.is_none() {
            self.cam = Some(scene.fit_bounds().map_or(Camera { centre: rect.center(), scale: 1.0 }, |b| Camera::fit(b, rect)));
            self.fitted = Some(fit_key);
        }
        let Some(cam) = self.cam.as_mut() else { return };
        if resp.dragged_by(PointerButton::Primary) {
            cam.pan(resp.drag_delta());
        }
        if let Some(p) = resp.hover_pos() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            if scroll != 0.0 {
                cam.zoom_at(rect, p, (scroll * ZOOM_PER_POINT).exp());
            }
            if zoom != 1.0 {
                cam.zoom_at(rect, p, zoom);
            }
        }
        let cam = *cam;
        let hit_at = |p: Option<egui::Pos2>| p.and_then(|p| hit_test(scene, &cam, rect, p));
        let hover = hit_at(resp.hover_pos());
        let mut click = None;
        if resp.clicked() {
            click = hit_at(resp.interact_pointer_pos()).filter(|h| h.clickable).map(|h| h.target);
        }
        if resp.secondary_clicked() {
            self.menu_target = hit_at(resp.interact_pointer_pos()).map(|h| h.target);
        }
        let exits = self.core.valid_exits();
        let Some(g) = self.core.game() else { return };
        let st = PaintState { view: g.view(), selected: g.selected(), exits: &exits, flashing: g.flashing(), time: now };
        paint::paint(&painter, paint::draw(scene, &cam, rect, &st));
        if let Some(t) = click {
            self.core.click(&t);
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.core.escape();
            self.menu_target = None;
        }
        let resp = match &hover {
            Some(h) => resp.on_hover_text_at_pointer(self.core.describe(&h.target)),
            None => resp,
        };
        resp.context_menu(|ui| self.menu_ui(ui));
    }

    fn menu_ui(&mut self, ui: &mut Ui) {
        let Some(t) = self.menu_target.clone() else {
            ui.close();
            return;
        };
        ui.label(self.core.describe(&t));
        let items = self.core.menu(&t);
        let interpose = match &t {
            Target::Berth(b) if self.core.can_interpose(b) => Some(b.clone()),
            _ => None,
        };
        if items.is_empty() && interpose.is_none() {
            return;
        }
        ui.separator();
        for item in items {
            if ui.button(&item.label).clicked() {
                self.core.command(item.cmd);
                ui.close();
            }
        }
        if let Some(b) = interpose {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.headcode).desired_width(60.0).hint_text("1A01"));
                if ui.button("Interpose").clicked() {
                    self.core.interpose(&b, &self.headcode);
                    self.headcode.clear();
                    ui.close();
                }
            });
        }
    }
}
```

In `crates/client-ui/src/lib.rs` append:
```rust
pub mod screens;

pub use screens::UiApp;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/cargo test -p signalbox-client-ui`
Expected: PASS (`screens` 8 plus Task 5's 22). The screen tests run real egui frames with synthetic pointer and key events and read back the text and shapes egui produced; no window or GPU is involved (`textures_delta` is cleared each frame because there is nothing to upload the font atlas to).

- [ ] **Step 5: Commit**

```bash
git add crates/client-ui
git commit -m "feat(client-ui): lobby, top bar, diagram, train list, alarms and menus"
```

---

### Task 7: `client-web` — the browser shell, and the wasm build

**Files:**
- Modify: `Cargo.toml` (member `crates/client-web`; workspace dependencies `eframe`, `wasm-bindgen`, `wasm-bindgen-futures`, `web-sys`; `[profile.web]`)
- Create: `crates/client-web/Cargo.toml`, `crates/client-web/src/lib.rs`, `crates/client-web/src/transport.rs`, `crates/client-web/index.html`
- Create: `scripts/build-web.sh`, `scripts/wasm-build` (both mode 100755: `git add --chmod=+x`)
- Modify: `deploy/Dockerfile` (a `wasm-tools` stage before `build`; the release image does not use it until Task 10)

**Interfaces:**
- Consumes: `client_core::{App, ConnState, Transport}` (Task 3), `client_ui::UiApp` (Task 6); eframe 0.36.2 on wasm32 (`eframe::App::ui(&mut self, &mut egui::Ui, &mut eframe::Frame)`, `eframe::WebRunner::new().start(canvas, WebOptions, AppCreator).await`, `WebOptions::wgpu_options.wgpu_setup` = `egui_wgpu::WgpuSetup::CreateNew(WgpuSetupCreateNew { instance_descriptor, .. })`, `eframe::wgpu::Backends`); web-sys `WebSocket`, `MessageEvent`, `CloseEvent`, `Window::fetch_with_str`, `Response::status`.
- Produces: `crates/client-web` (package `signalbox-client-web`, lib `signalbox_web`, `crate-type = ["cdylib"]`, empty natively) with a `#[wasm_bindgen(start)]` entry; `WebSocketTransport` (private); `index.html` with `#signalbox_canvas`, `#fallback`, `#fallback_reason`; `scripts/build-web.sh [OUT] [cargo args…]` → `OUT/index.html`, `OUT/app/signalbox_web.js`, `OUT/app/signalbox_web_bg.wasm` (default OUT `target/web`); `scripts/wasm-build [build-web.sh args…]` (tools image `local/signalbox-wasm-tools:<wasm-bindgen version>`, override `SIGNALBOX_WASM_IMAGE`); Dockerfile stage `wasm-tools` (`ARG WASM_BINDGEN_VERSION=0.2.129`). Task 10 uses all three.

What the shell does: eframe's `WebRunner` on the canvas with wgpu (WebGPU in secure contexts, else WebGL2 — egui-wgpu's own fallback, stated explicitly in `run`), a `UiApp` over an `App` whose transport is a browser WebSocket to `ws(s)://<page host>/ws` (the session cookie goes with it). Every socket event checks a generation counter so a replaced socket's late events are ignored; every event asks egui for a repaint. A socket that closes without having opened is followed by `GET /ws`: 401 → `ConnState::Unauthorized` (decision 9). When the app `wants_login()`, the shell sets `location.href = "/auth/login"` once. The theme is always dark (a VDU), whatever the browser prefers, and the console gets one line, `signalbox: drawing with BrowserWebGpu` or `… Gl`, for bug reports and the browser check. If eframe cannot start (no WebGPU and no WebGL2), `#fallback` explains it in plain HTML.

This crate has no native tests: everything in it is the browser's API. What it relies on is tested natively in Tasks 3–6 (the app, the transport contract with `MemTransport`), Task 8 (`GET /ws` answers 401 only without a session) and Task 9 (the same app over a real socket); the browser itself is the controller's check (Controller section, C4).

- [ ] **Step 1: Write the failing check**

Run: `test -s target/web/app/signalbox_web_bg.wasm && echo built`
Expected: nothing printed (no web build yet).

- [ ] **Step 2: Workspace entries**

In the root `Cargo.toml`: add `"crates/client-web"` to `members` (after `client-ui`); append to `[workspace.dependencies]` (after `egui`):
```toml
eframe = { version = "0.36.2", default-features = false, features = ["wgpu", "default_fonts"] }
# Pinned exactly: wasm-bindgen-cli must be this same version (scripts/build-web.sh checks).
wasm-bindgen = "=0.2.129"
wasm-bindgen-futures = "0.4.79"
web-sys = "0.3.106"
```
and at the end of the file:
```toml

# The browser client's wasm (scripts/build-web.sh): small rather than fast.
[profile.web]
inherits = "release"
opt-level = "s"
lto = true
codegen-units = 1
panic = "abort"
```

- [ ] **Step 3: The crate**

Create `crates/client-web/Cargo.toml`:
```toml
[package]
name = "signalbox-client-web"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "signalbox_web"
path = "src/lib.rs"
crate-type = ["cdylib"]

# Everything is for the browser: natively this crate builds empty, so the
# native workspace build and CI's cache never see eframe or wgpu.
[target.'cfg(target_arch = "wasm32")'.dependencies]
signalbox-client-core = { path = "../client-core" }
signalbox-client-ui = { path = "../client-ui" }
eframe.workspace = true
wasm-bindgen.workspace = true
wasm-bindgen-futures.workspace = true
web-sys = { workspace = true, features = [
    "CloseEvent",
    "console",
    "Document",
    "Element",
    "HtmlCanvasElement",
    "Location",
    "MessageEvent",
    "Response",
    "WebSocket",
    "Window",
] }
```

Create `crates/client-web/src/lib.rs`:
```rust
//! The browser shell (spec D1 §2): eframe's web runner on wgpu (WebGPU,
//! falling back to WebGL2), a WebSocket `Transport`, and the hand-off to
//! `/auth/login` when the session has expired. wasm32 only: natively this
//! crate is empty.

#![cfg(target_arch = "wasm32")]

mod transport;

use client_core::App;
use client_ui::UiApp;
use eframe::egui;
use eframe::egui_wgpu::WgpuSetup;
use eframe::wgpu::Backends;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::wasm_bindgen;

use crate::transport::WebSocketTransport;

struct WebApp {
    ui: UiApp,
    sent_to_login: bool,
}

impl eframe::App for WebApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui.ui(ui);
        if self.ui.core.wants_login() && !self.sent_to_login {
            self.sent_to_login = true;
            if let Some(w) = web_sys::window() {
                let _ = w.location().set_href("/auth/login");
            }
        }
    }
}

/// Runs when the module is instantiated (`init()` in index.html).
#[wasm_bindgen(start)]
pub fn start() {
    wasm_bindgen_futures::spawn_local(async {
        if let Err(why) = run().await {
            fallback(&why);
        }
    });
}

async fn run() -> Result<(), String> {
    let doc = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
    let canvas = doc
        .get_element_by_id("signalbox_canvas")
        .ok_or("no #signalbox_canvas")?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| "#signalbox_canvas is not a canvas")?;
    let mut options = eframe::WebOptions::default();
    // egui-wgpu's own default on the web, stated here: WebGPU where the
    // browser has it (secure contexts only), else WebGL2.
    if let WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        setup.instance_descriptor.backends = Backends::BROWSER_WEBGPU | Backends::GL;
    }
    eframe::WebRunner::new()
        .start(
            canvas,
            options,
            Box::new(|cc| {
                if let Some(rs) = &cc.wgpu_render_state {
                    // Which of WebGPU and WebGL2 we got: for bug reports and the browser check.
                    let backend = format!("signalbox: drawing with {:?}", rs.adapter.get_info().backend);
                    web_sys::console::log_1(&backend.into());
                }
                // A VDU is dark whatever the browser's theme.
                cc.egui_ctx.set_theme(egui::Theme::Dark);
                let now = cc.egui_ctx.input(|i| i.time);
                let transport = WebSocketTransport::new(cc.egui_ctx.clone());
                Ok(Box::new(WebApp { ui: UiApp::new(App::new(Box::new(transport), now)), sent_to_login: false }))
            }),
        )
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))
}

/// Neither WebGPU nor WebGL2 (or something else stopped eframe): say so
/// in plain HTML instead of a blank page.
fn fallback(why: &str) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    if let Some(c) = doc.get_element_by_id("signalbox_canvas") {
        c.remove();
    }
    if let Some(r) = doc.get_element_by_id("fallback_reason") {
        r.set_text_content(Some(&format!("The signal box could not start: {why}")));
    }
    if let Some(f) = doc.get_element_by_id("fallback") {
        let _ = f.remove_attribute("hidden");
    }
}
```

Create `crates/client-web/src/transport.rs`:
```rust
//! `Transport` over the browser's WebSocket to `/ws` on the page's own
//! origin (the session cookie goes with it). Browsers hide the status of a
//! refused upgrade, so a socket that closes without ever opening is
//! followed by a plain `GET /ws`: 401 there means the session is gone.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use client_core::{ConnState, Transport};
use eframe::egui;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

#[derive(Default)]
struct Shared {
    state: ConnState,
    inbox: VecDeque<String>,
    /// Bumped by every `connect`; events from older sockets are ignored.
    generation: u64,
    opened: bool,
}

pub struct WebSocketTransport {
    ctx: egui::Context,
    shared: Rc<RefCell<Shared>>,
    ws: Option<WebSocket>,
    /// Kept alive as long as their socket.
    _handlers: Option<(Closure<dyn FnMut(Event)>, Closure<dyn FnMut(MessageEvent)>, Closure<dyn FnMut(CloseEvent)>)>,
}

impl WebSocketTransport {
    pub fn new(ctx: egui::Context) -> WebSocketTransport {
        WebSocketTransport { ctx, shared: Rc::new(RefCell::new(Shared::default())), ws: None, _handlers: None }
    }
}

/// `wss://host/ws` on an https page, else `ws://host/ws`.
fn ws_url() -> Option<String> {
    let loc = web_sys::window()?.location();
    let scheme = if loc.protocol().ok()? == "https:" { "wss:" } else { "ws:" };
    Some(format!("{scheme}//{}/ws", loc.host().ok()?))
}

/// Is `/ws` refusing us for want of a session?
async fn unauthorized() -> bool {
    let Some(w) = web_sys::window() else { return false };
    match wasm_bindgen_futures::JsFuture::from(w.fetch_with_str("/ws")).await {
        Ok(r) => r.dyn_into::<web_sys::Response>().is_ok_and(|r| r.status() == 401),
        Err(_) => false,
    }
}

impl Transport for WebSocketTransport {
    fn connect(&mut self) {
        if let Some(old) = self.ws.take() {
            old.set_onopen(None);
            old.set_onmessage(None);
            old.set_onclose(None);
            let _ = old.close();
        }
        let generation = {
            let mut s = self.shared.borrow_mut();
            s.generation += 1;
            s.state = ConnState::Connecting;
            s.inbox.clear();
            s.opened = false;
            s.generation
        };
        let Some(ws) = ws_url().and_then(|u| WebSocket::new(&u).ok()) else {
            self.shared.borrow_mut().state = ConnState::Closed;
            return;
        };
        let (shared, ctx) = (self.shared.clone(), self.ctx.clone());
        let on_open = Closure::<dyn FnMut(Event)>::new(move |_| {
            let mut s = shared.borrow_mut();
            if s.generation == generation {
                s.state = ConnState::Open;
                s.opened = true;
                ctx.request_repaint();
            }
        });
        let (shared, ctx) = (self.shared.clone(), self.ctx.clone());
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            let mut s = shared.borrow_mut();
            if s.generation == generation {
                if let Some(text) = e.data().as_string() {
                    s.inbox.push_back(text);
                    ctx.request_repaint();
                }
            }
        });
        let (shared, ctx) = (self.shared.clone(), self.ctx.clone());
        let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |_| {
            let opened = {
                let s = shared.borrow();
                if s.generation != generation {
                    return;
                }
                s.opened
            };
            if opened {
                shared.borrow_mut().state = ConnState::Closed;
                ctx.request_repaint();
                return;
            }
            let (shared, ctx) = (shared.clone(), ctx.clone());
            wasm_bindgen_futures::spawn_local(async move {
                let state = if unauthorized().await { ConnState::Unauthorized } else { ConnState::Closed };
                let mut s = shared.borrow_mut();
                if s.generation == generation {
                    s.state = state;
                    ctx.request_repaint();
                }
            });
        });
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        self.ws = Some(ws);
        self._handlers = Some((on_open, on_message, on_close));
    }

    fn state(&self) -> ConnState {
        self.shared.borrow().state
    }

    fn send(&mut self, text: String) {
        if self.state() == ConnState::Open {
            if let Some(ws) = &self.ws {
                let _ = ws.send_with_str(&text);
            }
        }
    }

    fn poll(&mut self) -> Vec<String> {
        self.shared.borrow_mut().inbox.drain(..).collect()
    }
}
```

Create `crates/client-web/index.html`:
```html
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>signalbox</title>
<style>
  html, body { margin: 0; height: 100%; overflow: hidden; background: #0B0B0F; color: #D0D0D8; font-family: sans-serif; }
  #signalbox_canvas { display: block; width: 100%; height: 100%; }
  #fallback { padding: 2em; max-width: 42em; line-height: 1.5; }
</style>
</head>
<body>
<canvas id="signalbox_canvas"></canvas>
<div id="fallback" hidden>
  <h1>signalbox</h1>
  <p id="fallback_reason"></p>
  <p>signalbox draws with WebGPU, or with WebGL2 where WebGPU is not available. This browser offers
  neither, or has them turned off, so the signal box cannot be shown. Try a current Chrome, Edge or
  Firefox with hardware acceleration enabled.</p>
</div>
<noscript>signalbox needs JavaScript and WebAssembly.</noscript>
<script type="module">
  import init from "./app/signalbox_web.js";
  init().catch((e) => {
    document.getElementById("signalbox_canvas")?.remove();
    document.getElementById("fallback_reason").textContent = "The signal box could not start: " + e;
    document.getElementById("fallback").hidden = false;
  });
</script>
</body>
</html>
```

(`#signalbox_canvas` is removed rather than hidden on failure: its `display: block` rule would beat the `hidden` attribute.)

- [ ] **Step 4: The build scripts and the tools stage**

Create `scripts/build-web.sh`:
```bash
#!/usr/bin/env bash
# Build the browser client (crates/client-web) into OUT (default
# target/web): index.html, app/signalbox_web.js, app/signalbox_web_bg.wasm.
# Needs the wasm32-unknown-unknown target and wasm-bindgen-cli at the
# version Cargo.lock pins: the wasm-tools stage of deploy/Dockerfile has
# both (locally, run this through scripts/wasm-build). Arguments after OUT
# go to cargo (CI passes --offline).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$(realpath -m "${1:-$root/target/web}")"
shift || true
cd "$root"
want="$(sed -n '/^name = "wasm-bindgen"$/{n;s/^version = "\(.*\)"$/\1/p;q}' Cargo.lock)"
have="$(wasm-bindgen --version | awk '{print $2}')"
if [ "$want" != "$have" ]; then
  echo "build-web: wasm-bindgen-cli is $have but Cargo.lock has wasm-bindgen $want" >&2
  exit 1
fi
cargo build -p signalbox-client-web --target wasm32-unknown-unknown --profile web --locked "$@"
rm -rf "$out"
mkdir -p "$out/app"
wasm-bindgen --target web --no-typescript --out-dir "$out/app" --out-name signalbox_web \
  "${CARGO_TARGET_DIR:-$root/target}/wasm32-unknown-unknown/web/signalbox_web.wasm"
install -m 0644 crates/client-web/index.html "$out/index.html"
# The release image's front runs as its own user: everything world-readable.
chmod -R a+rX "$out"
ls -l "$out" "$out/app"
```

Create `scripts/wasm-build`:
```bash
#!/usr/bin/env bash
# Build the browser client (scripts/build-web.sh; output target/web/) in a
# local tools image: the Rust image plus the wasm32 target and
# wasm-bindgen-cli, from the wasm-tools stage of deploy/Dockerfile. The
# image is built on first use and tagged by the wasm-bindgen version in
# Cargo.lock; override it with SIGNALBOX_WASM_IMAGE. Arguments go to
# build-web.sh (an output directory under /w, then cargo flags).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
version="$(sed -n '/^name = "wasm-bindgen"$/{n;s/^version = "\(.*\)"$/\1/p;q}' "$root/Cargo.lock")"
image="${SIGNALBOX_WASM_IMAGE:-local/signalbox-wasm-tools:$version}"
if ! docker image inspect "$image" >/dev/null 2>&1; then
  docker build -f "$root/deploy/Dockerfile" --target wasm-tools \
    --build-arg "WASM_BINDGEN_VERSION=$version" -t "$image" "$root"
fi
exec docker run --rm -i \
  -u "$(id -u):$(id -g)" \
  -v "$root:/w" -w /w \
  -e CARGO_HOME=/w/.cargo-home \
  "$image" scripts/build-web.sh "$@"
```

In `deploy/Dockerfile`, replace the header comment and insert a stage before `FROM rust:1.98-slim-bookworm AS build`:
```dockerfile
# syntax=docker/dockerfile:1
# signalbox: the front (signalbox-server), the game process (signalbox-game),
# the browser client and the three converted TS2 layouts. Release build: no
# dev login.
# Build from the repository root:
#   docker build -f deploy/Dockerfile -t local/signalbox:$(git rev-parse --short HEAD) .

# The browser client's toolchain: the wasm32 target and wasm-bindgen-cli at
# the version Cargo.lock pins (scripts/build-web.sh refuses a mismatch).
# scripts/wasm-build builds this stage alone as local/signalbox-wasm-tools:<version>.
FROM rust:1.98-slim-bookworm AS wasm-tools
ARG WASM_BINDGEN_VERSION=0.2.129
RUN rustup target add wasm32-unknown-unknown \
 && cargo install wasm-bindgen-cli --version "=${WASM_BINDGEN_VERSION}" --locked \
 && rm -rf /usr/local/cargo/registry
```
(BuildKit sends no build context for `--target wasm-tools`: the stage copies nothing. Checked in scratch: the stage builds in about a minute on ra.)

```bash
git add --chmod=+x scripts/build-web.sh scripts/wasm-build
```

- [ ] **Step 5: Build and check**

Run: `scripts/cargo build --workspace --all-targets`
Expected: no errors and no warnings; `signalbox-client-web` compiles as an empty crate natively (its dependencies are wasm32-only).
Run: `scripts/wasm-build 2>&1 | tee target/wasm-build.log`
Expected: the first run builds `local/signalbox-wasm-tools:0.2.129` (about a minute), then `Finished \`web\` profile [optimized]` (about 2.5 minutes cold on ra) and a listing of `target/web/index.html`, `target/web/app/signalbox_web.js` (≈ 150 KB) and `target/web/app/signalbox_web_bg.wasm` (≈ 8 MB; ≈ 2.9 MB gzipped).
Run: `grep -E '^(warning|error)' target/wasm-build.log`
Expected: no output (CI builds this with `-D warnings`).
Run: `test -s target/web/app/signalbox_web_bg.wasm && echo built`
Expected: `built`.
Run: `grep -c 'signalbox_web_bg.wasm' target/web/app/signalbox_web.js`
Expected: at least 1 (the JS glue loads the wasm relative to itself, i.e. from `/app/`).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/client-web scripts/build-web.sh scripts/wasm-build deploy/Dockerfile
git commit -m "feat(client-web): eframe web shell with a WebSocket transport, and the wasm build"
```

---

### Task 8: the front serves the web client

**Files:**
- Create: `crates/server/src/assets.rs`
- Modify: `crates/server/src/config.rs` (`web_dir`, `SIGNALBOX_WEB`), `crates/server/src/lib.rs` (`pub mod assets;` load at start), `crates/server/src/web.rs` (`AppState.web`, `/` and `/app/{file}`, the placeholder text)
- Modify: `crates/bot/src/net.rs` (`http_get_with`, for request headers in tests)
- Modify: `crates/server/tests/common/mod.rs` (`web_dir` in `dev_config`; `front_with_web`), `crates/server/tests/release.rs`, `crates/server/tests/oidc.rs` (`web_dir` in their `Config` literals)
- Test: `crates/server/tests/units.rs`, `crates/server/tests/front.rs`

**Interfaces:**
- Consumes: C2's `Config`, `AppState`, `router`, `user_of`, `index_page`, `server::start`; axum 0.8 (`Path`, `HeaderMap`, `axum::body::Bytes`, `axum::http::header::*`); the files Task 7's `build-web.sh` writes.
- Produces:
  - `server::assets::{WebAssets { index: Asset, app: BTreeMap<String, Asset> }, Asset { body: Bytes, content_type: &'static str, etag: String }, valid_asset_name(&str) -> bool, content_type(&str) -> &'static str, etag(&[u8]) -> String}`; `WebAssets::load(&Path) -> Result<Option<WebAssets>, String>` (`Ok(None)` when the directory does not exist).
  - `Config.web_dir: PathBuf` (`SIGNALBOX_WEB`, default `config::DEFAULT_WEB = "/opt/signalbox/web"`); `AppState.web: Option<Arc<WebAssets>>`.
  - Routes: `/` → `index.html` with a session when installed (else the placeholder page; 303 to `/auth/login` without a session, as in C2); `/app/{file}` → 401 without a session, 404 for any name not loaded, else the file with `Content-Type`, `ETag`, `Cache-Control: no-cache`, `X-Content-Type-Options: nosniff`, and 304 for a matching `If-None-Match`.
  - `bot::net::http_get_with(base: &str, path: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, NetError>`; `http_get` now calls it.
  - Test harness: `common::front_with_web(name: &str, files: &[(&str, &[u8])]) -> Front` (a twobox front whose `web/` holds `files`).

- [ ] **Step 1: Write the failing tests**

In `crates/server/tests/units.rs`: make the module comment
```rust
//! The front's pure parts: configuration, sessions, the rate limit, the
//! placeholder page and the web client's files.
```
add `use server::assets::{WebAssets, content_type, etag, valid_asset_name};`, in `config_defaults_and_overrides` add after the `game_bin` assertion
```rust
    assert_eq!(c.web_dir, PathBuf::from("/opt/signalbox/web"));
```
add `("SIGNALBOX_WEB", "/w"),` to the overrides list and after that `.unwrap();`
```rust
    assert_eq!(c.web_dir, PathBuf::from("/w"));
```
and append:
```rust
#[test]
fn asset_names_are_plain_file_names() {
    for ok in ["index.html", "signalbox_web.js", "signalbox_web_bg.wasm", "a-b.c_d"] {
        assert!(valid_asset_name(ok), "{ok}");
    }
    let long = "a".repeat(101);
    for bad in ["", ".", "..", ".hidden", "../x", "a/b", "a\\b", "%2e%2e", "a b", "é.js", long.as_str()] {
        assert!(!valid_asset_name(bad), "{bad}");
    }
    assert_eq!(content_type("x.wasm"), "application/wasm");
    assert_eq!(content_type("x.js"), "text/javascript; charset=utf-8");
    assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
    assert_eq!(content_type("README"), "application/octet-stream");
    assert_eq!(etag(b"abc"), etag(b"abc"));
    assert_ne!(etag(b"abc"), etag(b"abd"));
    assert!(etag(b"").starts_with('"') && etag(b"").ends_with('"'));
}

#[test]
fn web_assets_load_index_and_plain_app_files_only() {
    let dir = std::env::temp_dir().join(format!("sbx-web-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(WebAssets::load(&dir), Ok(None), "no directory: the placeholder stays");
    std::fs::create_dir_all(dir.join("app/sub")).unwrap();
    let err = WebAssets::load(&dir).unwrap_err();
    assert!(err.contains("index.html"), "{err}");
    std::fs::write(dir.join("index.html"), "<canvas>").unwrap();
    std::fs::write(dir.join("app/signalbox_web.js"), "import x").unwrap();
    std::fs::write(dir.join("app/signalbox_web_bg.wasm"), b"\0asm").unwrap();
    std::fs::write(dir.join("app/.hidden"), "no").unwrap();
    std::fs::write(dir.join("app/sub/deep.js"), "no").unwrap();
    std::fs::write(dir.join("elsewhere.js"), "no").unwrap();
    let w = WebAssets::load(&dir).unwrap().unwrap();
    assert_eq!(&w.index.body[..], b"<canvas>");
    assert_eq!(w.app.keys().collect::<Vec<_>>(), ["signalbox_web.js", "signalbox_web_bg.wasm"]);
    assert_eq!(w.app["signalbox_web_bg.wasm"].content_type, "application/wasm");
    assert_eq!(w.app["signalbox_web.js"].etag, etag(b"import x"));
    let _ = std::fs::remove_dir_all(&dir);
}
```

In `crates/server/tests/front.rs`, import `http_get_with` too (`use bot::net::{Conn, NetError, dev_login, http_get, http_get_with};`) and append:
```rust
const INDEX: &[u8] = b"<!doctype html><canvas id=\"signalbox_canvas\"></canvas>";
const WASM: &[u8] = b"\0asm-not-really";

async fn web_front(name: &str) -> Front {
    front_with_web(
        name,
        &[("index.html", INDEX), ("app/signalbox_web.js", b"export default 1;"), ("app/signalbox_web_bg.wasm", WASM)],
    )
    .await
}

#[tokio::test]
async fn the_web_client_is_served_to_a_session() {
    let f = web_front("web").await;
    let r = http_get(&f.base, "/", None).await.unwrap();
    assert_eq!((r.status, r.header("location")), (303, Some("/auth/login")), "no session: log in first, as before");
    let cookie = dev_login(&f.base, "ann").await.unwrap();
    let r = http_get(&f.base, "/", Some(&cookie)).await.unwrap();
    assert_eq!((r.status, r.header("content-type")), (200, Some("text/html; charset=utf-8")));
    assert_eq!(r.body.as_bytes(), INDEX);
    assert_eq!(r.header("cache-control"), Some("no-cache"));
    let r = http_get(&f.base, "/app/signalbox_web_bg.wasm", Some(&cookie)).await.unwrap();
    assert_eq!((r.status, r.header("content-type")), (200, Some("application/wasm")));
    assert_eq!(r.body.as_bytes(), WASM);
    assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
    let tag = r.header("etag").unwrap().to_string();
    let r = http_get_with(&f.base, "/app/signalbox_web_bg.wasm", &[("Cookie", &cookie), ("If-None-Match", &tag)]).await.unwrap();
    assert_eq!((r.status, r.body.as_str()), (304, ""));
    let r = http_get_with(&f.base, "/app/signalbox_web_bg.wasm", &[("Cookie", &cookie), ("If-None-Match", "\"old\"")]).await.unwrap();
    assert_eq!(r.status, 200);
    let r = http_get(&f.base, "/app/signalbox_web.js", None).await.unwrap();
    assert_eq!(r.status, 401, "assets need a session too");
    f.running.stop().await;
}

#[tokio::test]
async fn assets_are_only_the_listed_names() {
    let f = web_front("web-names").await;
    let cookie = dev_login(&f.base, "ann").await.unwrap();
    for path in [
        "/app/../Cargo.toml",
        "/app/%2e%2e%2fCargo.toml",
        "/app/..%2findex.html",
        "/app/",
        "/app/sub/dir.js",
        "/app/.hidden",
        "/app/nope.js",
        "/app/index.html",
        "/index.html",
    ] {
        let r = http_get(&f.base, path, Some(&cookie)).await.unwrap();
        assert!(r.status == 404 || r.status == 400, "{path}: {}", r.status);
        assert!(!r.body.contains("canvas"), "{path}");
    }
    f.running.stop().await;
}

#[tokio::test]
async fn without_a_web_dir_the_placeholder_stays() {
    let f = front("no-web").await;
    let cookie = dev_login(&f.base, "ann").await.unwrap();
    let r = http_get(&f.base, "/", Some(&cookie)).await.unwrap();
    assert!(r.body.contains("Signed in as ann"), "{}", r.body);
    assert_eq!(http_get(&f.base, "/app/signalbox_web.js", Some(&cookie)).await.unwrap().status, 404);
    f.running.stop().await;
}

/// The browser cannot see why an upgrade was refused, so the web client
/// asks `GET /ws` (D1 decision 9): 401 must mean "no session" and nothing else.
#[tokio::test]
async fn ws_without_an_upgrade_is_401_only_without_a_session() {
    let f = front("ws-probe").await;
    assert_eq!(http_get(&f.base, "/ws", None).await.unwrap().status, 401);
    assert_eq!(http_get(&f.base, "/ws", Some("signalbox_session=forged")).await.unwrap().status, 401);
    let cookie = dev_login(&f.base, "ann").await.unwrap();
    let r = http_get(&f.base, "/ws", Some(&cookie)).await.unwrap();
    assert!((400..500).contains(&r.status) && r.status != 401, "{}", r.status);
    f.running.stop().await;
}
```

In `crates/server/tests/common/mod.rs`: add `web_dir: root.join("web"),` to `dev_config`'s literal (after `game_bin`; the directory does not exist, so fronts keep the placeholder page), and replace `front_with` with:
```rust
/// A front whose layouts directory holds each `(name, world JSON)`.
pub async fn front_with(name: &str, layouts: &[(&str, String)]) -> Front {
    front_in(temp_dir(name), layouts).await
}

/// A twobox front whose web directory holds `files` (paths relative to it).
pub async fn front_with_web(name: &str, files: &[(&str, &[u8])]) -> Front {
    let root = temp_dir(name);
    for (path, bytes) in files {
        let p = root.join("web").join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    front_in(root, &[("twobox", std::fs::read_to_string(TWOBOX).unwrap())]).await
}

async fn front_in(root: PathBuf, layouts: &[(&str, String)]) -> Front {
    let dir = root.join("layouts");
    std::fs::create_dir_all(&dir).unwrap();
    for (layout, json) in layouts {
        std::fs::write(dir.join(format!("{layout}.json")), json).unwrap();
    }
    let running = server::start(dev_config(&root, dir)).await.unwrap();
    let base = running.base();
    Front { running, base, root }
}
```
In `crates/server/tests/release.rs` and `crates/server/tests/oidc.rs` (`fn config`), add `web_dir: root.join("web"),` after `game_bin` in the `Config` literal.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `scripts/cargo test -p signalbox-server --features dev-auth --test units --test front`
Expected: FAIL to compile — no module `server::assets`, no field `web_dir`, no `http_get_with`.

- [ ] **Step 3: `http_get_with`**

In `crates/bot/src/net.rs`, replace the head of `http_get` (its doc comment through `req.push_str("\r\n");`) with:
```rust
/// A bare HTTP/1.0 GET (no redirects followed), enough for the dev login
/// and for tests. `base` is `http://host:port`; `path` starts with `/` and
/// is sent as it is.
pub async fn http_get(base: &str, path: &str, cookie: Option<&str>) -> Result<HttpResponse, NetError> {
    let headers: Vec<(&str, &str)> = cookie.map(|c| ("Cookie", c)).into_iter().collect();
    http_get_with(base, path, &headers).await
}

/// `http_get` with any request headers.
pub async fn http_get_with(base: &str, path: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, NetError> {
    let host = host_port(base)?;
    let mut stream = TcpStream::connect(host).await?;
    let mut req = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
```
(the rest of the old body — write, read, parse — is now `http_get_with`'s.)

- [ ] **Step 4: The assets**

Create `crates/server/src/assets.rs`:
```rust
//! The browser client's files (D1 decision 11), read once at start from
//! `SIGNALBOX_WEB`: `index.html` and the plainly named files of `app/`.
//! Requests are answered from memory; no request touches the filesystem.

use std::collections::BTreeMap;
use std::path::Path;

use axum::body::Bytes;

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub body: Bytes,
    pub content_type: &'static str,
    /// A strong ETag, quoted.
    pub etag: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WebAssets {
    pub index: Asset,
    /// By file name, as served under `/app/`.
    pub app: BTreeMap<String, Asset>,
}

/// 1–100 of `A-Z a-z 0-9 _ . -`, not starting with a dot: no paths.
pub fn valid_asset_name(s: &str) -> bool {
    (1..=100).contains(&s.len()) && !s.starts_with('.') && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}

pub fn content_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

/// FNV-1a over the bytes, and the length: changes whenever the file does.
pub fn etag(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("\"{h:016x}-{:x}\"", bytes.len())
}

fn asset(path: &Path, name: &str) -> Result<Asset, String> {
    let body = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Asset { etag: etag(&body), content_type: content_type(name), body: Bytes::from(body) })
}

impl WebAssets {
    /// `Ok(None)` when `dir` does not exist (dev and test fronts keep the
    /// placeholder page); an error when it exists without `index.html` or
    /// a file cannot be read.
    pub fn load(dir: &Path) -> Result<Option<WebAssets>, String> {
        if !dir.exists() {
            return Ok(None);
        }
        let index = asset(&dir.join("index.html"), "index.html")?;
        let mut app = BTreeMap::new();
        let app_dir = dir.join("app");
        if app_dir.is_dir() {
            let entries = std::fs::read_dir(&app_dir).map_err(|e| format!("{}: {e}", app_dir.display()))?;
            for entry in entries {
                let entry = entry.map_err(|e| format!("{}: {e}", app_dir.display()))?;
                let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
                if valid_asset_name(&name) && entry.path().is_file() {
                    app.insert(name.clone(), asset(&entry.path(), &name)?);
                }
            }
        }
        Ok(Some(WebAssets { index, app }))
    }
}
```

- [ ] **Step 5: Config, start and routes**

`crates/server/src/config.rs`: add to `Config` after `game_bin`
```rust
    /// The built browser client (`index.html`, `app/`); when missing, `/`
    /// keeps the placeholder page.
    pub web_dir: PathBuf,
```
add `pub const DEFAULT_WEB: &str = "/opt/signalbox/web";` after `DEFAULT_LAYOUTS`, read it in `from_lookup` after `layouts_dir`
```rust
        let web_dir = PathBuf::from(get("SIGNALBOX_WEB").unwrap_or_else(|| DEFAULT_WEB.into()));
```
and put `web_dir` last in the returned `Config { .. }`.

`crates/server/src/lib.rs`: `pub mod assets;` (before `pub mod config;`), `use crate::assets::WebAssets;`, and in `start` replace the two lines that create `sessions` and `state` with
```rust
    let web = WebAssets::load(&cfg.web_dir)?.map(Arc::new);
    if web.is_none() {
        eprintln!("signalbox-server: no web client at {}; serving the placeholder page", cfg.web_dir.display());
    }
    let sessions = Arc::new(Sessions::new());
    let state = AppState { sup: sup.clone(), sessions: sessions.clone(), key: Key::from(&cfg.session_key), oidc, web };
```
(an existing directory without `index.html`, or an unreadable file, stops the front at start with that path in the error, exit 2 — a broken image should not come up.)

`crates/server/src/web.rs`:
- module comment:
```rust
//! HTTP routes (spec §8): the browser client (or C2's placeholder page
//! when it is not installed), the WebSocket, and the auth routes. Nothing
//! but the login flow answers without a session.
```
- imports: `use axum::extract::{FromRef, Path, Query, State};`, `use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH, X_CONTENT_TYPE_OPTIONS};`, `use axum::http::{HeaderMap, StatusCode};`, `use crate::assets::{Asset, WebAssets};`
- `AppState` gains, after `oidc`:
```rust
    /// The browser client; `None` serves the placeholder page.
    pub web: Option<Arc<WebAssets>>,
```
- in `router`, after `.route("/", get(index))`: `.route("/app/{file}", get(app_file))`
- `index_page`'s doc becomes `/// The page served when the browser client is not installed.` and its sentence `The browser client is not built yet; bots play over <code>/ws</code>.` becomes `The browser client is not installed on this server (<code>SIGNALBOX_WEB</code>); bots play over <code>/ws</code>.`
- replace `async fn index` with:
```rust
async fn index(State(state): State<AppState>, jar: SignedCookieJar, headers: HeaderMap) -> Response {
    let Some(user) = user_of(&state, &jar) else { return Redirect::to("/auth/login").into_response() };
    match &state.web {
        Some(w) => serve(&w.index, &headers),
        None => Html(index_page(&user, &state.sup.list_games(), &state.sup.layouts().infos())).into_response(),
    }
}

/// `/app/{file}`: only names loaded at start; 401 without a session.
async fn app_file(State(state): State<AppState>, jar: SignedCookieJar, Path(file): Path<String>, headers: HeaderMap) -> Response {
    if user_of(&state, &jar).is_none() {
        return (StatusCode::UNAUTHORIZED, "sign in first").into_response();
    }
    match state.web.as_ref().and_then(|w| w.app.get(&file)) {
        Some(a) => serve(a, &headers),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// An asset from memory; 304 when the browser already has this version.
/// `no-cache`: browsers revalidate every load, so a new build is picked up
/// at once while an unchanged one costs a 304.
fn serve(a: &Asset, headers: &HeaderMap) -> Response {
    let fresh = headers
        .get(IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == a.etag));
    let common = [(ETAG, a.etag.clone()), (CACHE_CONTROL, "no-cache".to_string())];
    if fresh {
        return (StatusCode::NOT_MODIFIED, common).into_response();
    }
    let typed = [(CONTENT_TYPE, a.content_type.to_string()), (X_CONTENT_TYPE_OPTIONS, "nosniff".to_string())];
    (common, typed, a.body.clone()).into_response()
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `scripts/cargo test -p signalbox-server --features dev-auth --test units --test front`
Expected: PASS (`units` 8, `front` 11 — the four new ones: `the_web_client_is_served_to_a_session`, `assets_are_only_the_listed_names`, `without_a_web_dir_the_placeholder_stays`, `ws_without_an_upgrade_is_401_only_without_a_session`).
Run: `scripts/cargo test -p signalbox-server` and `scripts/cargo test -p signalbox-server --features dev-auth`
Expected: PASS (release, oidc, supervisor, process, e2e fast tests unchanged).
Run: `scripts/cargo test -p signalbox-bot`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/server crates/bot/src/net.rs
git commit -m "feat(server): serve the browser client from SIGNALBOX_WEB, from memory, behind the session"
```

---

### Task 9: end to end — `client-core` over the real front

**Files:**
- Modify: `crates/server/Cargo.toml` (dev-dependency `signalbox-client-core`; `[[test]] client` with `required-features = ["dev-auth"]`)
- Test: `crates/server/tests/client.rs`

**Interfaces:**
- Consumes: `client_core::{App, ConnState, Link, Target, Transport}` and `App`'s lobby/game/input methods (Tasks 3–4); C2's `tests/common` (`front`, `front_with`, `Front`, `s`), `bot::net::{Conn, NetError, dev_login}` (`Conn::connect`, `recv`, `send_text`); `ts2_import::{convert, areas}` for Liverpool Street.
- Produces: a test-only `NetTransport` (tokio task per connection; `NetHandle::{drop_connection, take_sent}`) — not exported.

These run in CI with the rest of `cargo test -p signalbox-server --features dev-auth` (about 6 s together on ra).

- [ ] **Step 1: Write the test**

In `crates/server/Cargo.toml`, add after the `e2e` test entry:
```toml
[[test]]
name = "client"
required-features = ["dev-auth"]
```
and to `[dev-dependencies]`:
```toml
signalbox-client-core = { path = "../client-core" }
```
Create `crates/server/tests/client.rs`:
```rust
//! The browser client's logic end to end (spec D1 §6): a real
//! `client_core::App` over a WebSocket to the real front with dev login and
//! real game processes. The transport here is test-only (tokio-tungstenite
//! through `bot::net::Conn`); the browser's is web-sys, the app the same.

mod common;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bot::net::{Conn, NetError, dev_login};
use client_core::{App, ConnState, Link, Target, Transport};
use common::*;
use protocol::*;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

const WAIT: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Shared {
    state: ConnState,
    inbox: VecDeque<String>,
    /// Bumped by every connect; a replaced socket's task stops touching state.
    generation: u64,
    /// Every frame the app sent, in order.
    sent: Vec<ClientFrame>,
    task: Option<AbortHandle>,
}

/// `Transport` over `bot::net::Conn` on a tokio task.
struct NetTransport {
    base: String,
    cookie: Option<String>,
    shared: Arc<Mutex<Shared>>,
    out: Option<mpsc::UnboundedSender<String>>,
}

/// The test's handle on the transport the app owns.
#[derive(Clone)]
struct NetHandle(Arc<Mutex<Shared>>);

impl NetTransport {
    fn new(base: &str, cookie: Option<String>) -> (NetTransport, NetHandle) {
        let shared = Arc::new(Mutex::new(Shared::default()));
        (NetTransport { base: base.to_string(), cookie, shared: shared.clone(), out: None }, NetHandle(shared))
    }
}

fn set(shared: &Mutex<Shared>, generation: u64, state: ConnState) {
    let mut s = shared.lock().unwrap();
    if s.generation == generation {
        s.state = state;
    }
}

impl Transport for NetTransport {
    fn connect(&mut self) {
        let generation = {
            let mut s = self.shared.lock().unwrap();
            if let Some(t) = s.task.take() {
                t.abort();
            }
            s.generation += 1;
            s.state = ConnState::Connecting;
            s.inbox.clear();
            s.generation
        };
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        self.out = Some(tx);
        let (base, cookie, shared) = (self.base.clone(), self.cookie.clone(), self.shared.clone());
        let task = tokio::spawn(async move {
            let mut conn = match Conn::connect(&base, cookie.as_deref()).await {
                Ok(c) => c,
                Err(NetError::Status(401)) => return set(&shared, generation, ConnState::Unauthorized),
                Err(_) => return set(&shared, generation, ConnState::Closed),
            };
            set(&shared, generation, ConnState::Open);
            loop {
                tokio::select! {
                    f = conn.recv() => match f {
                        Ok(Some(frame)) => {
                            let mut s = shared.lock().unwrap();
                            if s.generation == generation {
                                s.inbox.push_back(frame.to_json());
                            }
                        }
                        _ => break,
                    },
                    t = rx.recv() => match t {
                        Some(text) => {
                            if conn.send_text(text).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    },
                }
            }
            set(&shared, generation, ConnState::Closed);
        });
        self.shared.lock().unwrap().task = Some(task.abort_handle());
    }

    fn state(&self) -> ConnState {
        self.shared.lock().unwrap().state
    }

    fn send(&mut self, text: String) {
        if self.state() != ConnState::Open {
            return;
        }
        if let Ok(f) = ClientFrame::from_json(&text) {
            self.shared.lock().unwrap().sent.push(f);
        }
        if let Some(out) = &self.out {
            let _ = out.send(text);
        }
    }

    fn poll(&mut self) -> Vec<String> {
        self.shared.lock().unwrap().inbox.drain(..).collect()
    }
}

impl NetHandle {
    /// The network goes away under the client (the TCP connection drops).
    fn drop_connection(&self) {
        let mut s = self.0.lock().unwrap();
        if let Some(t) = s.task.take() {
            t.abort();
        }
        s.state = ConnState::Closed;
    }

    fn take_sent(&self) -> Vec<ClientFrame> {
        std::mem::take(&mut self.0.lock().unwrap().sent)
    }
}

/// Tick the app every 20 ms on a real clock until `done` holds.
async fn drive(app: &mut App, clock: Instant, what: &str, mut done: impl FnMut(&App) -> bool) {
    let deadline = Instant::now() + WAIT;
    loop {
        app.tick(clock.elapsed().as_secs_f64());
        if done(app) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn logged_in(f: &Front, user: &str) -> (App, NetHandle, Instant) {
    let cookie = dev_login(&f.base, user).await.unwrap();
    let (t, h) = NetTransport::new(&f.base, Some(cookie));
    let clock = Instant::now();
    (App::new(Box::new(t), 0.0), h, clock)
}

fn liverpool_json() -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    let ts2 = std::fs::read_to_string(format!("{dir}/../ts2-import/tests/data/liverpool-st.json")).unwrap();
    let text = std::fs::read_to_string(format!("{dir}/../../layouts/liverpool-st.areas.json")).unwrap();
    let mut w = ts2_import::convert(&ts2).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&text).unwrap()).unwrap();
    serde_json::to_string(&w).unwrap()
}

/// Spec D1 §6: log in, create Liverpool Street, claim it, click an
/// entrance then its exit, and see the route set in the view.
#[tokio::test]
async fn a_client_sets_a_route_on_liverpool_street() {
    let f = front_with("client-lst", &[("liverpool-st", liverpool_json())]).await;
    let (mut app, _h, clock) = logged_in(&f, "ann").await;
    drive(&mut app, clock, "the lobby", |a| a.layouts().iter().any(|l| l.name == "liverpool-st")).await;
    app.create_game("liverpool-st", Some(7), None);
    drive(&mut app, clock, "the first view", |a| a.game().is_some_and(|g| g.view().is_some())).await;
    app.claim("Liverpool Street");
    drive(&mut app, clock, "the area", |a| a.game().and_then(|g| g.area()) == Some("Liverpool Street")).await;
    let g = app.game().unwrap();
    let layout = g.layout().unwrap().clone();
    assert!(layout.geometry.as_ref().is_some_and(|geo| !geo.lines.is_empty()), "the diagram came with the layout");
    assert!(!g.view().unwrap().trains.is_empty(), "trains due at 07:00 are listed");
    let candidates: Vec<RouteInfo> = layout.routes.iter().filter(|r| r.operable && !r.automatic).cloned().collect();
    let mut set = None;
    for r in candidates.iter().take(8) {
        let refusals = app.game().unwrap().log().entries().filter(|e| e.text.starts_with("Refused")).count();
        app.click(&Target::Signal(r.entrance.clone()));
        assert_eq!(app.game().unwrap().selected(), Some(r.entrance.as_str()));
        assert!(app.valid_exits().contains(&r.exit));
        app.click(&match &r.exit {
            ExitName::Signal(s) => Target::Signal(s.clone()),
            ExitName::Node(n) => Target::Exit(n.clone()),
        });
        drive(&mut app, clock, "the route or a refusal", |a| {
            let g = a.game().unwrap();
            g.view().unwrap().routes.contains_key(&r.name) || g.log().entries().filter(|e| e.text.starts_with("Refused")).count() > refusals
        })
        .await;
        if app.game().unwrap().view().unwrap().routes.contains_key(&r.name) {
            set = Some(r.name.clone());
            break;
        }
    }
    let name = set.expect("one of the first routes could be set");
    drive(&mut app, clock, "the route to lock", |a| {
        a.game().unwrap().view().unwrap().routes.get(&name).is_some_and(|rv| rv.state == RouteState::Locked)
    })
    .await;
    f.running.stop().await;
}

/// Spec D1 §5: a dropped connection comes back by itself with exactly one
/// `join`, no `resync`, and the area is still yours.
#[tokio::test]
async fn a_dropped_connection_rejoins_with_one_join() {
    let f = front("client-drop").await;
    let (mut app, h, clock) = logged_in(&f, "ann").await;
    drive(&mut app, clock, "the lobby", |a| !a.layouts().is_empty()).await;
    app.create_game("twobox", Some(1), None);
    drive(&mut app, clock, "the first view", |a| a.game().is_some_and(|g| g.view().is_some())).await;
    app.claim("West");
    drive(&mut app, clock, "the area", |a| a.game().and_then(|g| g.area()) == Some("West")).await;
    let game = app.game().unwrap().id.clone();
    let layouts_before = app.game().unwrap().layout_gen();
    h.take_sent();
    h.drop_connection();
    drive(&mut app, clock, "the loss to show", |a| matches!(a.link(), Link::Waiting { .. })).await;
    assert!(app.banner().unwrap().starts_with("Connection lost"));
    drive(&mut app, clock, "the rejoin's layout", |a| a.link() == Link::Open && a.game().is_some_and(|g| g.layout_gen() > layouts_before))
        .await;
    assert_eq!(h.take_sent(), [ClientFrame::Lobby(LobbyMsg::Join { game })], "one join, no resync");
    let g = app.game().unwrap();
    assert_eq!((g.area(), g.resyncs()), (Some("West"), 0));
    f.running.stop().await;
}

/// Spec D1 §5: without a session the socket is refused and the client asks
/// the shell to log in, rather than retrying.
#[tokio::test]
async fn without_a_session_the_client_asks_for_a_login() {
    let f = front("client-401").await;
    let (t, _h) = NetTransport::new(&f.base, None);
    let clock = Instant::now();
    let mut app = App::new(Box::new(t), 0.0);
    drive(&mut app, clock, "the refusal", |a| a.wants_login()).await;
    f.running.stop().await;
}
```

- [ ] **Step 2: Run it**

Run: `scripts/cargo test -p signalbox-server --features dev-auth --test client`
Expected: PASS, 3 tests (`a_client_sets_a_route_on_liverpool_street`, `a_dropped_connection_rejoins_with_one_join`, `without_a_session_the_client_asks_for_a_login`). No production code changes in this task: if one fails, the bug is in Tasks 3–4 or 8 — fix it there, with a unit test in that crate that reproduces it, before touching this file.

Note on the rejoin assertion: after a disconnect the game forgets the player's delta base, so the rejoin's view starts again at `seq` 1; the test waits for the rejoin's new *layout* (`layout_gen` grows), not for a larger `seq`.

- [ ] **Step 3: Run the server suites and the CI gate's native part**

Run: `scripts/cargo test -p signalbox-server --features dev-auth`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/server/Cargo.toml crates/server/tests/client.rs Cargo.lock
git commit -m "test(server): the browser client's logic end to end over the real front"
```

---

### Task 10: the release image, CI and the docs

**Files:**
- Modify: `deploy/Dockerfile` (a `web` stage; the image carries `/opt/signalbox/web` and `SIGNALBOX_WEB`)
- Modify: `scripts/ci/test.sh` (the wasm build; keep mode 100755)
- Modify: `deploy/smoke.sh` (the web client's files need a session; keep mode 100755)
- Modify: `deploy/README.md`, `CLAUDE.md`

**Interfaces:**
- Consumes: Task 7's `wasm-tools` stage and `scripts/build-web.sh`; Task 8's `SIGNALBOX_WEB`.
- Produces: an image whose front serves the browser client; a CI gate that builds it once the runner can (Controller C1).

- [ ] **Step 1: Write the failing check**

Add the web client's files to `deploy/smoke.sh`: change its comment to
```bash
# Smoke-check a running signalbox front from outside: nothing but the login
# answers without a session (the web client's files included), and the
# build has no dev login.
```
and after `check /ws 401` add
```bash
check /app/signalbox_web.js 401
check /app/signalbox_web_bg.wasm 401
```
(a front without the client answers 404 there, so the smoke check now fails against an image built before this task.)

- [ ] **Step 2: The image**

In `deploy/Dockerfile`, add after the `wasm-tools` stage (before `FROM rust:1.98-slim-bookworm AS build`):
```dockerfile
FROM wasm-tools AS web
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    scripts/build-web.sh /out/web
```
in the final stage, after `COPY --from=build /out/layouts/ /opt/signalbox/layouts/`:
```dockerfile
COPY --from=web /out/web/ /opt/signalbox/web/
```
and end the `ENV` list with `SIGNALBOX_WEB=/opt/signalbox/web` (a `\` after the `SIGNALBOX_GAME_BIN` line). `build-web.sh` makes everything world-readable, since the files are owned by root and the front runs as `signalbox`.

- [ ] **Step 3: CI**

In `scripts/ci/test.sh`, append after the dev-auth test line:
```bash
# The browser client for wasm32, where the runner has the target and
# wasm-bindgen-cli. The controller's runner image sets SIGNALBOX_REQUIRE_WASM=1
# once it has them; until then a runner without them skips, loudly.
if rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown && command -v wasm-bindgen >/dev/null; then
  scripts/build-web.sh target/web --offline
elif [ "${SIGNALBOX_REQUIRE_WASM:-0}" = 1 ]; then
  echo "ci: SIGNALBOX_REQUIRE_WASM=1 but the wasm32 target or wasm-bindgen-cli is missing" >&2
  exit 1
else
  echo "ci: SKIP the web client build: no wasm32 target or wasm-bindgen-cli on this runner"
fi
```
(`RUSTFLAGS=-D warnings` is already exported at the top, so the wasm build is warning-free too; `build-web.sh` honours the runner's `CARGO_TARGET_DIR`.)

- [ ] **Step 4: Docs**

`deploy/README.md`:
- the `Dockerfile` row of the file table becomes
```markdown
| `Dockerfile` | release image: both binaries, the browser client (`/opt/signalbox/web`, built by the `wasm-tools` and `web` stages) and the converted layouts `liverpool-st`, `drain`, `gretz-armainvilliers` (no dev login) |
```
- add a configuration row after the `SIGNALBOX_GAME_BIN` row:
```markdown
| `SIGNALBOX_WEB` | image | `/opt/signalbox/web` (the browser client: `index.html`, `app/`; read once at start) |
```
- replace the paragraph that ends `says "Signed in as <username>".` with:
```markdown
Then open `https://ra.tail3e0c1e.ts.net:50160/` in a browser: after the
Authentik login the signalbox lobby loads (WebGPU in Chrome/Edge, WebGL2 in
Firefox).
```
- in "What `smoke.sh` expects", replace `` `/ws` 401 without a session;`` with
```markdown
`/ws` and the web client's files under
`/app/` 401 without a session;
```
- in "Local check of an image", the `ls` line becomes
```bash
docker exec signalbox-test ls /opt/signalbox/layouts /opt/signalbox/web /opt/signalbox/web/app
```

`CLAUDE.md`:
- in the Commands block, before the `scripts/ci/test.sh` line:
```bash
scripts/cargo test -p signalbox-client-core                   # client logic: connection, lobby, clicks (in-process game)
scripts/cargo test -p signalbox-client-ui                     # diagram and screens, headless egui (no GPU)
scripts/cargo test -p signalbox-server --features dev-auth --test client   # client-core over the real front
scripts/wasm-build                                            # the browser client into target/web/ (tools image on first use)
```
- after the paragraph that begins `Paths passed through`, add:
```markdown
The stock Rust image has no wasm32 target: `scripts/wasm-build` runs
`scripts/build-web.sh` in `local/signalbox-wasm-tools:<wasm-bindgen version>`,
built on first use from the `wasm-tools` stage of `deploy/Dockerfile`. The
`wasm-bindgen` crate is pinned exactly and the CLI must be the same version;
bumping it means a new tools image and a new CI runner image (maintainer).
```
- in the Architecture crate list, replace `one process per game). The multiplayer design is` (the rest of that sentence stays) with
```markdown
one process per game), `crates/client-core` (`signalbox-client-core`: the
browser client's logic), `crates/client-ui` (`signalbox-client-ui`: its egui
screens) and `crates/client-web` (`signalbox-client-web`: the wasm shell;
design in `docs/superpowers/specs/2026-09-30-browser-client-design.md`). The multiplayer design is
```
- a new section before `### Tests`:
```markdown
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
```

- [ ] **Step 5: Verify the image (throwaway, local only)**

```bash
docker build -f deploy/Dockerfile -t local/signalbox:d1-test .
docker run --rm -d --name signalbox-d1-test -p 127.0.0.1:19160:9160 \
  -e SIGNALBOX_SESSION_KEY=$(openssl rand -hex 64) -e OIDC_ISSUER=http://127.0.0.1:9/ \
  -e OIDC_CLIENT_ID=x -e OIDC_CLIENT_SECRET=y -e SIGNALBOX_PUBLIC_URL=http://127.0.0.1:19160 \
  local/signalbox:d1-test
deploy/smoke.sh http://127.0.0.1:19160 503
docker exec signalbox-d1-test ls -l /opt/signalbox/web /opt/signalbox/web/app
docker logs signalbox-d1-test 2>&1 | head -3
docker stop signalbox-d1-test
docker image rm local/signalbox:d1-test
```
Expected: the build finishes (the `web` stage about 3 minutes cold); every smoke line `ok` (`/app/...` 401); `index.html`, `signalbox_web.js`, `signalbox_web_bg.wasm` listed and world-readable; the log says `listening on 0.0.0.0:9160` and does **not** say `no web client`. (Checked in scratch, 2026-09-30.)

Run: `scripts/ci/test.sh` is for the runner; locally run its parts: `scripts/cargo build --workspace --all-targets --locked`, `scripts/cargo test --workspace --locked`, `scripts/cargo build -p signalbox-bot --no-default-features --locked`, `scripts/cargo test -p signalbox-server --features dev-auth --locked`, and `scripts/wasm-build`.
Expected: all PASS, no warnings.
Run: `git ls-files -s scripts/ci/test.sh deploy/smoke.sh scripts/build-web.sh scripts/wasm-build`
Expected: every line starts `100755`.

- [ ] **Step 6: Commit**

```bash
git add deploy/Dockerfile scripts/ci/test.sh deploy/smoke.sh deploy/README.md CLAUDE.md
git commit -m "feat(deploy): the release image serves the browser client; CI builds it for wasm32"
```

---

## Controller section (owner-gated infra; never a subagent)

Everything here touches ra's shared infrastructure or the live deployment. Do each step only with the owner's go-ahead, in this order, and record what was done in the branch's final report.

### C0. Reseed the CI cache after every task that changes `Cargo.lock`

Tasks 3, 5, 7 and 9 change it (Task 7 adds the wasm-only crates: `cargo fetch` without `--target` fetches every platform's packages, so they are included). Before pushing any of them:
```bash
cd /home/skye-fi/projects/signalbox
scripts/cargo fetch --locked
sudo /opt/stack/apps/signalbox-runner/seed-cache.sh
```
Until C1 is done, CI prints `ci: SKIP the web client build` and passes on the native part.

### C1. The CI runner gets the wasm toolchain

In `/opt/stack/apps/signalbox-runner/Dockerfile`, before the `RUN useradd ...` line (still root, before `CARGO_HOME` is pointed at `/cache`):
```dockerfile
# The browser client (signalbox D1): the wasm32 target and wasm-bindgen-cli
# at the version signalbox's Cargo.lock pins (scripts/build-web.sh checks).
ARG WASM_BINDGEN_VERSION=0.2.129
RUN rustup target add wasm32-unknown-unknown \
 && cargo install wasm-bindgen-cli --version "=${WASM_BINDGEN_VERSION}" --locked \
 && rm -rf /usr/local/cargo/registry
```
and add `SIGNALBOX_REQUIRE_WASM=1` to the final `ENV` (so a runner that loses the toolchain fails CI instead of skipping). Bump the tag in `docker-compose.yml` (`image: local/signalbox-ci-runner:13.2.0-wasm1`) and in `seed-cache.sh` (it runs that image), then:
```bash
cd /opt/stack/apps/signalbox-runner
sudo docker compose build runner
sudo docker compose up -d runner
docker exec signalbox-runner sh -c 'rustup target list --installed; wasm-bindgen --version; echo $SIGNALBOX_REQUIRE_WASM'
```
Expected: `wasm32-unknown-unknown`, `wasm-bindgen 0.2.129`, `1`. The runner container is `mem_limit: 6g`; the web build (LTO, one codegen unit) peaked well under that in scratch. The `ci-cache` volume is untouched; reseed (C0) once more, push, and check the CI log shows `Finished \`web\` profile` and the three files from `build-web.sh`, not `SKIP`. Keep the old `13.2.0` image until one green run, then remove it.

### C2. Deploy

As `deploy/README.md` "Build and run" (the image now builds the `wasm-tools` and `web` stages too; the first build installs wasm-bindgen-cli, about a minute), then:
```bash
docker logs signalbox 2>&1 | head -3     # "listening on 0.0.0.0:9160", and no "no web client" line
/home/skye-fi/projects/signalbox/deploy/smoke.sh https://ra.tail3e0c1e.ts.net:50160 303
```
Expected: every line `ok`, including `/app/signalbox_web.js: 401`. Nothing changes in compose, Authentik or `tailscale serve`.

### C3. The owner's own look

In Chrome or Edge (WebGPU) and in Firefox (WebGL2) on the tailnet: sign in, create a Liverpool Street game, claim an area, set and cancel a route, swing points, interpose a headcode, vote a speed; a second player in a neighbouring area at the same time (spec §1 criterion 1). The browser console shows `signalbox: drawing with BrowserWebGpu` in Chrome/Edge and `... Gl` in Firefox.

### C4. Headless browser check (Playwright on ra, against a dev-auth test build)

The release image has no dev login, so this runs a throwaway dev-auth front from the working copy on `127.0.0.1:19161` (localhost is a secure context, so WebGPU is allowed). Headless Chromium has no GPU: SwiftShader stands in, for WebGPU with `--enable-unsafe-webgpu --enable-features=Vulkan --use-vulkan=swiftshader --use-webgpu-adapter=swiftshader --use-angle=swiftshader`, and for the WebGL2 fallback with `--disable-webgpu`. Run the sequence once each way. All files go under `target/d1-check/`. (This whole check was run in scratch against the prototype on 2026-09-30: both paths drew the lobby; on WebGL2 it went on through create, claim, a route set by two clicks and the signal's menu.)

```bash
cd /home/skye-fi/projects/signalbox
scripts/wasm-build                                           # target/web
scripts/cargo build -p signalbox-server --features dev-auth --bins
mkdir -p target/d1-check/layouts
scripts/cargo run -q -p ts2-import -- crates/ts2-import/tests/data/liverpool-st.json \
  -o /w/target/d1-check/layouts/liverpool-st.json --areas /w/layouts/liverpool-st.areas.json
docker run --rm -d --name sbx-d1-check --network host -u "$(id -u):$(id -g)" -v "$PWD:/w" -w /w \
  -e SIGNALBOX_ADDR=127.0.0.1:19161 -e SIGNALBOX_DATA=/w/target/d1-check/data \
  -e SIGNALBOX_LAYOUTS=/w/target/d1-check/layouts -e SIGNALBOX_WEB=/w/target/web \
  -e SIGNALBOX_SESSION_KEY="$(openssl rand -hex 64)" \
  rust:1.98-slim-bookworm target/debug/signalbox-server
```
Save this as `target/d1-check/check.py`:
```python
"""Drive the signalbox browser client in headless Chromium.

usage: CHROMIUM_ARGS="..." python3 check.py BASE OUTDIR USER ACTION...
  ACTION: shot:NAME | click:X,Y | rclick:X,Y | wait:MS | key:NAME
Logs in through the dev front's /auth/dev?user=USER (which redirects to
/), runs the actions in order, and prints the browser console at the end.
"""

import os
import sys

from playwright.sync_api import sync_playwright

base, out, user, actions = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4:]
flags = [a for a in os.environ.get("CHROMIUM_ARGS", "").split(" ") if a]
logs = []
with sync_playwright() as p:
    browser = p.chromium.launch(args=flags)
    page = browser.new_page(viewport={"width": 1280, "height": 800})
    page.on("console", lambda m: logs.append(f"{m.type}: {m.text}"))
    page.on("pageerror", lambda e: logs.append(f"pageerror: {e}"))
    page.goto(f"{base}/auth/dev?user={user}")
    page.wait_for_timeout(3000)
    for a in actions:
        kind, _, arg = a.partition(":")
        if kind == "shot":
            page.screenshot(path=f"{out}/{arg}.png")
        elif kind in ("click", "rclick"):
            x, y = (float(v) for v in arg.split(","))
            page.mouse.move(x, y)
            page.wait_for_timeout(100)
            page.mouse.click(x, y, button="right" if kind == "rclick" else "left")
            page.wait_for_timeout(300)
        elif kind == "wait":
            page.wait_for_timeout(int(arg))
        elif kind == "key":
            page.keyboard.press(arg)
            page.wait_for_timeout(200)
        else:
            sys.exit(f"unknown action {a}")
    browser.close()
print("\n".join(logs))
```
Run it (the image needs the Python package installed on each run; outputs are root-owned):
```bash
pw() { flags=$1; shift; docker run --rm --network host -e CHROMIUM_ARGS="$flags" \
  -v "$PWD/target/d1-check:/out" -w /out mcr.microsoft.com/playwright/python:v1.55.0-noble \
  sh -c "pip install -q playwright==1.55.0 && python3 check.py http://127.0.0.1:19161 /out $*"; \
  docker run --rm -v "$PWD/target/d1-check:/out" alpine chown -R "$(id -u):$(id -g)" /out; }
WEBGPU="--enable-unsafe-webgpu --enable-features=Vulkan --use-vulkan=swiftshader --use-webgpu-adapter=swiftshader --use-angle=swiftshader"
pw "$WEBGPU" ann shot:gpu-1-lobby
pw "--disable-webgpu" ann shot:gl-1-lobby
```
Everything is drawn on one canvas, so there are no DOM selectors: look at each screenshot, read the coordinates of the next thing to click, and run again with the sequence so far. Each run is a new page and a new socket: the front keeps the game (it pauses while nobody is connected, and your area is kept for 120 s), so later runs start with `click:<Join>` and `click:<resume>` instead of `Create`. In the prototype run at 1280 × 800 the points were: Create (435, 68); Claim for Liverpool Street (233, 33); the platform 5 starter (270, 297); its exit towards Bishopsgate (664, 412); Join (763, 141); resume (406, 11). What to see:
1. `*-1-lobby.png`: the dark lobby with `liverpool-st`; the console says `signalbox: drawing with BrowserWebGpu` (first run) and `... Gl` (second), and has no `panicked` or `pageerror`.
2. `click:<Create> wait:4000 shot:2-game`: the game as a spectator, the whole diagram drawn, TRAINS listing trains.
3. `click:<Claim> wait:3000 shot:3-area`: fitted to Liverpool Street, the fringe dimmer, `Liverpool Street: ann` in the players line.
4. `click:<entrance> wait:500 shot:4-entrance`: a cyan ring on the entrance and on each exit its routes can end at.
5. `click:<a lit exit> wait:4000 shot:5-route`: the route's track white, the entrance showing a proceed aspect.
6. `rclick:<the entrance> wait:500 shot:6-menu`: hover text and `Cancel route <entrance> to <exit>`.

Attach the screenshots to the branch report. Then clean up:
```bash
docker stop sbx-d1-check
rm -rf target/d1-check
```
