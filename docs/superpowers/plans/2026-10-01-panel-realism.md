# signalbox D1.1 — panel realism pass — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the signaller's screen look and behave like a UK IECC / Westcad workstation — black VDU, thick track with joint gaps, white routes and overlaps, disc-on-hooked-post signals with `<box><workstation><number>` names, cyan in-track headcodes, ochre platforms, blue ○A buttons, direction arrows, hollow fringe — with the owner's switchable concessions (real aspects, headcode enquiry, signal numbers) and a simplifier panel; plus two owner requests of the same day: spectator clock votes when every area is robot-run, and deleting saved games from the lobby (creator or admin).

**Architecture:** Display data is added end to end without touching the simulation. `ts2-import` writes a box prefix, workstation letters (from `layouts/<name>.areas.json`) and optional line-name labels (from a new `layouts/<name>.lines.json`) into the world's client-only `layout` JSON; `game` reads them once per game (with defaults for older worlds and saves), builds a per-area simplifier from the timetable, and sends all three in the `Layout` (new `#[serde(default)]` fields). `client-core` turns them into display names, the simplifier model, the enquiry model and a settings model behind a small `SettingsStore` interface; `client-ui` redraws the diagram (pure shapes, tested headless) and adds the settings menu, the simplifier tab and the enquiry window; `client-web` stores the settings in the browser's `localStorage`. The two owner requests are self-contained: the voter set lives in `game::Game` (Task 1), deletion in the front's `Supervisor` with the creator recorded in the save's `meta` table (Task 3).

**Tech Stack:** Rust 1.98 (edition 2024) via `scripts/cargo` (Docker) and `scripts/wasm-build`; egui 0.36.2 (client-ui), eframe 0.36.2 + web-sys 0.3.106 (client-web, one new web-sys feature: `Storage`). No new crates.

**Spec:** `docs/superpowers/specs/2026-10-01-panel-realism-design.md` (approved; read it fully — every owner decision in its table is binding). Research behind it: `docs/superpowers/research/2026-10-01-uk-vdu-conventions.md`. It builds on D1: spec `docs/superpowers/specs/2026-09-30-browser-client-design.md`, plan `docs/superpowers/plans/2026-09-30-d1-browser-client.md` (structure and conventions this plan follows). Task 1 also amends the server spec `docs/superpowers/specs/2026-09-30-server-and-protocol-design.md` §3.5 (owner decision 12, recorded in the realism spec).

### Decisions (controller brief 2026-10-01 plus this plan's own)

1. **Owner decision 12 (added 2026-10-01, Task 1): spectators vote when nobody holds an area.** The clock's voters are the holders (connected or in their grace period) if any area is held; otherwise every connected player. Unanimity among voters as before, so a lone spectator's proposal applies at once; the robot never votes; a claim, a release, a grace expiry, a connect and a spectator's disconnect all re-settle the open proposal (so a spectator who agreed and then claims completes it). The wire code for a refused vote stays `not_a_holder`. The client shows the clock buttons exactly when `InGame::can_vote()` (you hold an area, or every holder in the view is `robot`).
2. **Where the prefixes live: the world's `layout` JSON, not `WorldFile`/core.** `layout` is already the client-only part of a world ("never read by the simulation"), saves copy the world, and old worlds simply lack the keys. ts2-import writes `"box_prefix": "L"` and `"workstations": {"Liverpool Street": "A", …}` next to `lines`/`signals`; `game::display::prefixes` reads them, falling back to the spec's defaults (box = first ASCII letter of the title, uppercased, or none; workstations A, B, C… in world area order) for any key that is missing or malformed. No core or `WorldFile` change, so every existing world and save still loads and replays bit-exact.
3. **Areas file additions.** Optional top-level `"prefix"` (1–3 ASCII capitals) and per-area `"workstation"` (one ASCII capital); defaults as above, resolved at conversion. New hard errors: a bad prefix, a bad workstation letter, the same letter on two areas (after defaults). The file keeps `deny_unknown_fields`, `schema: 1`.
4. **Lines file (`ts2-import --lines layouts/<name>.lines.json`).** A JSON list of `{"name": "DOWN MAIN", "direction": "up"|"down", "through": [signal or section names]}`. `direction` is the **world's** direction of travel (the same `up`/`down` as the world's signals, *not* the railway's Up/Down: on Liverpool Street the railway's Down lines run in the world's `up` direction); every signal named in `through` must face that way (hard error otherwise), which catches an authoring slip. Unknown names, an empty name or `through`, and a stretch with no drawn track are hard errors, like the areas file. Each line becomes **one** label at the end of the stretch that trains run towards (text inside the stretch, the arrow pointing out of it, as `UP WESTBURY →` / `← DOWN WESTBURY` sit at the edges of real IECC views), 12 units above the track. One label rather than one per end: a label's width is unknown to the converter, and a text anchored at its arrow end is the only placement that never runs off the stretch.
5. **Arrows are shapes, never glyphs.** egui's default fonts have no arrow glyphs (checked: `→ ← ↔ ▶ ◀ ● ⬤` all missing; `½ ○ · — ×` present, `½` only in the proportional font). So `LabelGeom` gains `arrow: Option<[f64; 2]>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`, the direction of travel), the client draws a small filled triangle at the label's anchor, and the ○A button is a circle shape plus an `A`. The glyph test asserts that no text on screen contains an arrow or a circle character, and that `½` (shown by the simplifier) has a glyph.
6. **Protocol.** `Layout` gains `box_prefix: String`, `workstations: BTreeMap<String, String>` (area → letter; the spec's `AreaInfo.workstation`, as a map because `Layout.areas` is a plain `Vec<String>` that older clients read) and `simplifier: Vec<SimplifierRow>`, all `#[serde(default)]`. `SimplifierRow { headcode, origin: Option<String>, destination: Option<String>, calls: Vec<SimplifierCall> }`, `SimplifierCall { place, platform: Option<String>, arr: Option<f64>, dep: Option<f64>, stops: bool }` (seconds since midnight, like `TrainRow.booked`).
7. **Simplifier rows (game).** Built once per game for each area and for spectators (like the geometry) from the world only: a service is listed if one of its calls is in the area — a call naming a platform counts if that (place, platform) has a platform segment in the area; a call without a platform counts if any platform of its place is in the area; spectators get every service with every call. `origin`/`destination` are the first and last calls' places (`None` without calls). Rows sort by the time of their first listed call (arrival, else departure; untimed last), then headcode, then service order. Liverpool Street's spectator list is all 1373 services (the Liverpool Street box lists all 1373 too: every service calls there); the whole layout message measured 636 KB for a spectator and 326 KB for that box in scratch — well under the 4 MiB ipc frame limit, and sent only on join, claim and resync. The client draws the list with `ScrollArea::show_rows`, so its length costs nothing per frame.
8. **Display names (client-core `names`).** A signal is shown as `<box><workstation><name>` (`LA9`, `LB72`, `LC121`); a single-area layout drops the workstation letter (`L9`); an empty prefix or a missing letter just contributes nothing; a name the layout does not know stays plain. Only signals are renamed (and route exits that are signals); berths, points, sections, nodes and every wire name stay as they are. Hover text, menus, alarms (`notice_text`/`command_text` take the names), the simplifier and the enquiry window use the displayed form; the top bar reads `g-1 · Workstation B · Bethnal Green (ann)` (no `Workstation X ·` for a single-area layout; `spectating` for a spectator).
9. **Settings (client-core `settings`).** `Settings { aspects: AspectMode::{RedGreen, Real}, enquiry: bool, numbers: bool }`, default red/green, enquiry off, numbers on. Stored as a few `key=value` lines (`aspects=real`, `enquiry=on`, `numbers=off`; unknown keys and bad values fall back to the defaults, so nothing stored can break the client) through `trait SettingsStore { fn load(&self) -> Option<String>; fn save(&mut self, text: &str); }`. `MemStore` (shared, for tests) is in client-core; `client-web` implements it over `window.localStorage` under the key `signalbox.settings`. The spec says "eframe storage": eframe's web storage *is* `localStorage`, but enabling eframe's `persistence` feature would add `ron` and serde features to the wasm build (new crates in `Cargo.lock`, a CI cache reseed) for one string; web-sys's `Storage` binding is one feature flag and no new crate. D2 implements the same trait natively (a file).
10. **Drawing numbers (spec §2 values, "tuned by eye").** Background `#000000`; idle track `#7D7D7D`; route and overlap `#FFFFFF`; occupied `#E8141C`; headcode `#39E0FF`; auto button `#1D4FD8`; platform `#B8860B` (number in black); labels and signal numbers `#9A9A9A`; fringe signals and fringe headcodes `#6E6E6E` (grey, not dimmed colours); lamps red `#E61E1E`, yellow `#FAD200`, green `#00DC50` (unchanged); selection / lit exits `#00C8FF`; refused-command outline `#FF3CFF`. Track width `clamp(9 × scale, 4, 14)` px (3 px in D1); joint gap 2 px (1 px off each touching end); fringe track two 1 px lines at the bar's edges; overlap tick `track width + 8` px long, 2 px wide; lamp radius 4 px; post 9 px out from the track, hook 5 px towards travel; dashed post for automatic signals (3 px dashes, 2 px gaps); signal numbers `clamp(16 × scale, …, 11)` px, hidden below 7 px; headcode 11 px mono in a black 34 × 14 px knock-out, centred 24 px behind its signal along the track; ○A circle radius 4 px, 16 px ahead of the lamp; direction arrows 7 px triangles, 6 px off the bar on the right of travel, at loose ends (inset 24 px) and every 400 px along runs longer than 60 px. Locked points need no mark of their own: their section is held by the route, so their legs are already white (spec §2 "locked points ends white").
11. **What flashes (owner decision 9).** Points moving (the gap flashes, as D1); the selected entrance (its cyan ring blinks); a route cancelling under approach locking (its entrance lamp blinks red/unlit). Nothing else blinks: a refused command gives a steady alarm line and a steady magenta outline on its signal for 2 s (`InGame::refused()`, renamed from D1's `flashing()`; `FLASH_S` becomes `REFUSED_S`).
12. **Signals.** Disc on a post from the track, perpendicular to the **left** of the direction of travel (screen `y` grows downwards, so left of `facing` is `(facing.y, −facing.x)`), hooked 5 px towards travel; the post starts at the signal's position projected onto its own segment's drawn line (TS2 signals already sit on the line; hand-drawn fixtures may not). Unknown facing: the disc alone at the signal's position. "Automatic signal" means a signal with an automatic route from it (D1's `auto_routes`), which gets the dashed post and the ○A button; the post of any signal turns white while any route from it is set, setting or cancelling. Colours: red/green mode shows red for `Red` and green for any proceed aspect; real mode shows D1's lamps (a second yellow for double yellow); fringe signals are grey in both; the hover text always gives the real aspect. Hit-testing a signal measures to the nearest of its disc, the foot of its post and its own drawn point (so D1's clicks at the signal's point still land).
13. **Headcodes and berths.** A signal's berth is drawn in-track, 24 px behind the signal along the track (`offset_px = −facing × 24`); a boundary berth sits 24 px inside its boundary along the track that ends there (D1's 18 px-above offset only when no such track is drawn). A berth holding a headcode is drawn as the knock-out plus cyan text (grey on the fringe); an empty berth draws nothing but stays hit-testable, so hover and the interpose menu still reach it.
14. **○A button.** `Target::Auto(signal)`: hollow circle while no automatic route from it auto-works, filled while one does; a click sends exactly the signal menu's `Auto-working on|off` command when that menu offers it, and otherwise does nothing (hover explains); it never touches the selection. Clickable only on your own signals.
15. **Headcode enquiry and clicks.** With enquiry on, a left click on a berth holding a headcode, or on a headcode in the train list, opens a small window for that headcode (its simplifier rows plus live state and lateness) and changes nothing else — no selection, no route, no clearing. With enquiry off, a berth click is D1's dead click (it clears the entrance).
16. **Side panel.** The upper half becomes two tabs, `TRAINS` and `SIMPLIFIER` (a headcode search box; columns `Train Late From To At Plat Arr Dep`, one line per call in the area, rows in first-call order, `HH:MM` with `½`, `pass` for a passing call, `OT`/`nL` lateness beside a running train); `ALARMS` stays always visible in the lower half (an alarm must never be hidden behind a tab; real IECC gives alarms their own VDU).
17. **Direction arrows (client-ui).** Drawn lines are chained into runs through nodes where exactly two visible segments meet and both are drawn (plain joints and signal nodes; runs stop at points, buffer stops and the edge of what you see). A run's directions come from the facing of the signals on its segments: one way gives a single arrow, both ways a double arrow, none gives no arrows (sidings). Arrows go 24 px in from each loose end (nothing else meets it, or it is a route exit — a buffer stop or boundary) and every 400 px along runs; a short run between points and signals carries none.
18. **Owner decision 13 (added 2026-10-01, Task 3): deleting games.** Lobby `delete_game {game}` deletes a saved or crashed game (save file plus `-wal`/`-shm`, under `saves/` only, id checked as `join` does; a crashed entry goes too); starting/running games are refused (`game_running`); only the creator (a `creator` row in the save's `meta`, written by the game process at create — no schema bump, see Task 3) or a `SIGNALBOX_ADMINS` user (`not_allowed` otherwise; saves without a creator: admins only). Success sends every client the new `games` list; `GameInfo` gains `creator` and per-user `can_delete`; the lobby shows Delete only where `can_delete`, with an in-page confirm. Deploy sets `SIGNALBOX_ADMINS=skye`.
19. **Deploy.** Controller-only, owner OK already given for redeploying as D1 did: the image converts the three layouts with `--lines` too; smoke check; a headless Playwright screenshot pass of the new look against a dev-auth front.

### Deferred (deliberately not in this pass)

- **Line names for Gretz.** The TS2 data names its tracks only `1`, `2`, `1bis`, … and the French line names are not known to us, so `gretz-armainvilliers.lines.json` is `[]` (arrows only). Liverpool Street gets `UP|DOWN SUBURBAN|MAIN|ELECTRIC|FAST` from its `SL_`/`ML_`/`EL_`/`FL_` track codes and TS2 labels; Drain (the Waterloo & City line, Bank ↔ Waterloo) gets `EASTBOUND`/`WESTBOUND`, LU's names for its two roads.
- **Filling the 10-unit TS2 signal gaps** in the drawn track (TS2 signal items occupy 10 units between two lines). They read as wide joints; drawing through them needs the undrawn items' geometry.
- **Real automatic-signal semantics.** In the sim an automatic route is always set and already re-sets itself; the ○A button keeps D1's command (auto-working on the automatic route) as the spec says, so on today's layouts it changes a flag the sim already behaves as. A controlled signal's auto-working (the real ○A) needs a spec change.
- **Placing line names by hand.** A line's label goes at its downstream end, 12 units above the track; on Liverpool Street `UP ELECTRIC` lands on TS2's own `ELECTRIC` group label at Bethnal Green, and at a spectator's whole-layout zoom the `DOWN …` names crowd each other (seen in the scratch browser check). A position override in the lines file is left for when the owner has looked.
- **Legibility at whole-layout zoom.** Signal numbers hide below 7 px, so Gretz (fit ≈ 0.35–0.42 px per unit per box) shows them only after zooming in once; the 34 × 14 px headcode knock-outs, as in D1, are wider than Gretz's track spacing at that zoom.
- **The simplifier scrolls from the first train of the day**, not from "now"; rows already run stay listed, as on paper.
- Spec §6's out-of-scope list stands (exact NR fonts, detail views, ARS, level crossings, train graphs, D2 specifics beyond the settings trait).

## Global Constraints

- License GPL-2.0-or-later (`license.workspace = true`), no new crates. The only dependency changes: `crates/client-ui` gains the workspace crate `ts2-import` as a dev-dependency (Task 9; one line in `Cargo.lock`, no new package, so the CI runner's offline cache needs no reseed), and `crates/client-web` turns on web-sys's `Storage` feature (Task 11; web-sys is already locked, so `Cargo.lock` does not change).
- Every cargo command runs through `scripts/cargo` from the repo root (Docker `rust:1.98-slim-bookworm`, repo at `/w`, no environment forwarded); wasm32 builds through `scripts/wasm-build`. Paths inside the containers are under `/w`.
- CI (`scripts/ci/test.sh`) builds with `-D warnings --locked --offline`: no unused imports, variables or dead code in the workspace default build, `signalbox-server --features dev-auth`, `signalbox-bot --no-default-features`, or the wasm32 web build. rustfmt and clippy are not available: match the surrounding style by hand (4-space indent, 120-ish columns, trailing commas as the file does).
- Determinism rules stay for `core`, `game`, `protocol` and `ts2-import`: `BTreeMap`/`BTreeSet`/`Vec` only, no `HashMap`/`HashSet`, no wall clock; converter output byte-identical for the same input; simplifier rows built from the world alone.
- Nothing received may panic the client: every new `Layout` field may be absent (old servers), empty, or name things the layout does not list; names are opaque strings (`Hackney & Bow`, `39,1V1`, `33,4P`).
- Wire names, commands, saves and the areas/lines files keep the plain TS2 names; the `<box><ws><number>` form is display only.
- `client-core` and `client-ui` never touch the browser, the clock or storage directly: time comes in as `now`, storage through `SettingsStore`.
- Infra (the CI runner and its cache, `/opt/stack`, compose projects and containers on ra, `tailscale serve`, Authentik) is controller-only and in the final Controller section. Subagents may run `scripts/cargo` and `scripts/wasm-build`, touch no existing container, image, volume or network, and remove any image they tag.

## Review Focus

1. **Data from before this pass** — a front or save without `box_prefix`/`workstations`/`simplifier`/`creator`, a world whose `layout` JSON lacks the keys or holds junk (`"box_prefix": 7`, a lower-case letter), a label with a NaN arrow: everything reads, names fall back to plain or default forms, the simplifier says it is empty, nothing panics. Pinned in Task 4 (`a_layout_or_view_from_before_d1_still_reads`, `a_games_list_from_before_deletion_still_reads` in Task 3), Task 5 (`prefixes_come_from_the_worlds_layout_and_bad_ones_fall_back`), Task 6 (`a_layout_from_an_older_server_shows_plain_names`) and Task 9 (`a_line_names_arrow_is_kept_as_a_unit_vector`).
2. **The real layouts' odd geometry** (Gretz 7000 units wide, diagonal legs, TS2's undrawn spacers and 10-unit signal gaps, buffer stops behind spacers, fringe cut mid-line): every box and a spectator of all three shipped layouts must draw finite shapes with arrows and without panicking. Pinned in Task 9 (`every_shipped_layout_draws_for_every_box`, which also checks Liverpool Street's platform roads get their arrows).
3. **Deleting the wrong thing** — ids like `../victim`, `notes`, a running game, someone else's game, a legacy save, a resume racing a delete: only the named save and its `-wal`/`-shm` go, only for its creator or an admin, never while starting or running, and a join after a delete says `unknown_game`. Pinned in Task 3 (`deleting_checks_the_id_like_join`, `a_running_game_is_never_deleted_not_even_by_an_admin`, `the_creator_deletes_their_saved_game_and_everyone_sees_it_go`, `an_admin_deletes_anyones_game_and_saves_from_before_creators`).
4. **Whatever is in `localStorage`** — another version's text, junk, `=`-only lines, a blocked store: the client starts with the defaults and never fails. Pinned in Task 7 (`anything_else_in_the_store_gives_the_defaults`) and, for the menu and a reload, Task 10 (`settings_change_from_the_menu_and_are_kept`); the browser's own blocked-storage case is the Controller's C2 step 4.
5. **A headcode click in the middle of setting a route** (an entrance chosen, enquiry on or off): with the enquiry on it opens the window and leaves the entrance chosen and nothing sent; with it off it is D1's dead click. And the ○A button never selects. Pinned in Task 10 (`a_headcode_click_opens_the_enquiry_only_when_it_is_on`), Task 7 (`the_enquiry_has_the_rows_and_the_live_state_and_never_routes`) and Task 6 (`clicking_an_auto_button_never_touches_the_selection`).

---

## File Structure

```
docs/superpowers/specs/2026-10-01-panel-realism-design.md   (T1) owner decision 12 + §7; (T3) decision 13 + §8
crates/game/src/clock.rs, game.rs                   (T1) voters; (T3) Game::set_creator; (T5) Display wiring
crates/game/src/save.rs                             (T3) SaveDb::set_creator, SaveSummary.creator
crates/game/src/display.rs                          (T5) new: prefixes with defaults, simplifier per area, Display
crates/game/src/layout.rs                           (T4) the new Layout fields, empty
crates/ts2-import/src/areas.rs                      (T2) prefix, workstation, validation, written into `layout`
crates/ts2-import/src/lines.rs, main.rs, lib.rs     (T2) new lines module, --lines
layouts/*.areas.json, layouts/*.lines.json          (T2) prefixes and letters; line names (Gretz: [])
crates/protocol/src/view.rs                         (T4) Layout.box_prefix/workstations/simplifier, SimplifierRow/Call, LabelGeom.arrow
crates/protocol/src/lobby.rs, lib.rs                (T3) DeleteGame, GameInfo.creator/can_delete, NOT_ALLOWED, GAME_RUNNING
crates/server/src/{process,config,lib,supervisor}.rs (T3) --creator, SIGNALBOX_ADMINS, delete_game, per-user lists
crates/client-core/src/app.rs                       (T1) ROBOT, can_vote; (T3) delete_game; (T6) names, refused
crates/client-core/src/names.rs                     (T6) new: Names
crates/client-core/src/{text,select,input}.rs       (T6) names in words, auto_toggle, Target::Auto; (T7) train_state_text, headcode_at
crates/client-core/src/settings.rs, simplifier.rs   (T7) new: Settings, SettingsStore, MemStore; the simplifier and enquiry models
crates/client-ui/src/scene.rs                       (T8) joints, base, routes; (T9) in-track berths, labels, runs
crates/client-ui/src/paint.rs                       (T8) the VDU palette, track, overlaps, signals; (T9) headcodes, ○A, platforms, labels, arrows
crates/client-ui/src/hit.rs                         (T8) signal_disc; (T9) auto_button, Target::Auto
crates/client-ui/src/screens.rs                     (T1) vote buttons; (T3) Delete; (T8) PaintState; (T10) workstation, Settings menu, tabs, simplifier, enquiry
crates/client-ui/Cargo.toml                         (T9) dev-dependency ts2-import
crates/client-web/src/store.rs, lib.rs, Cargo.toml  (T11) LocalStore over localStorage
deploy/docker-compose.yml, deploy/README.md          (T3) SIGNALBOX_ADMINS; (T12) Dockerfile row
deploy/Dockerfile, CLAUDE.md                        (T12) --lines; the pass documented
tests: game/tests/{game,clock,status,display,geometry}.rs, ts2-import/tests/{areas,lines,cli}.rs,
       protocol/tests/{golden,lobby}.rs, server/tests/{supervisor,process,units,release,oidc,client}.rs + common,
       client-core/tests/{votes,names,settings,simplifier,input,text,app}.rs,
       client-ui/tests/{paint,scene,hit,screens,layouts}.rs + common, bot/tests/{strategy,play}.rs (literals)
```

---

### Task 1: `game` + client — spectators vote on the clock while nobody holds an area (owner decision 12)

**Files:**
- Modify: `docs/superpowers/specs/2026-10-01-panel-realism-design.md` (decision 12 in the owner table and a new §7; the only spec edit in this plan)
- Modify: `crates/game/src/clock.rs` (voters, `VoteError::NotAVoter`)
- Modify: `crates/game/src/game.rs` (`Game::voters` replaces `holder_set`; settle on connect and on a spectator's disconnect)
- Modify: `crates/client-core/src/app.rs` (`ROBOT`, `InGame::can_vote`)
- Modify: `crates/client-ui/src/screens.rs` (clock buttons only for voters)
- Test: `crates/game/tests/game.rs`, `crates/game/tests/clock.rs`, `crates/client-core/tests/votes.rs` (new), `crates/client-ui/tests/screens.rs`

**Interfaces:**
- Consumes: C1's `GameClock`, `Game`; D1's `InGame`, `UiApp::top_bar`.
- Produces:
  - `game::clock::GameClock::vote(&mut self, voter: &str, proposal: Proposal, voters: &BTreeSet<String>) -> Result<Option<Proposal>, VoteError>` and `settle(&mut self, voters: &BTreeSet<String>) -> Option<Proposal>` (same shapes, parameter renamed); `VoteError::NotAVoter` replaces `NotAHolder`.
  - `pub fn game::Game::voters(&self) -> BTreeSet<String>`.
  - `pub const client_core::app::ROBOT: &str = "robot"`; `pub fn client_core::InGame::can_vote(&self) -> bool`.

- [ ] **Step 1: Record the decision in the spec**

In `docs/superpowers/specs/2026-10-01-panel-realism-design.md`, add a row to the owner-decisions table, after the row for decision 9:
```markdown
| 12 | Clock votes with nobody holding an area | When every area is robot-run, every connected spectator votes on pause and speed (unanimity as before, so a lone spectator's vote applies at once); as soon as anyone holds an area only holders vote again. Amends the server spec §3.5 (see §7) |
```
and a new last section, after §6:
```markdown
## 7. Clock votes (owner decision 12, 2026-10-01)

Amends `2026-09-30-server-and-protocol-design.md` §3.5, which let only holders vote and kept the clock as it was
while nobody held an area. The voters are now the holders (connected or within their grace period) when any area is
held, and otherwise every connected player; the robot never votes. A proposal still needs every voter and still lapses
after 30 s. Whenever the voters change — a claim, a release, a grace period running out, a player connecting, a
spectator leaving — the open proposal is settled again, so a spectator who agreed and then claims an area completes
it, and a claim leaves a spectators' proposal waiting for the new holder. A vote from someone who is not a voter is
still refused with `not_a_holder`. The client offers the clock buttons exactly to voters.
```

- [ ] **Step 2: Write the failing tests**

In `crates/game/tests/clock.rs`, replace both `VoteError::NotAHolder` with `VoteError::NotAVoter` and rename `fn only_holders_vote_and_only_listed_speeds` to `fn only_voters_vote_and_only_listed_speeds` (the clock itself is unchanged: it checks whatever voter set it is given).

In `crates/game/tests/game.rs`, change `use std::collections::BTreeMap;` to `use std::collections::{BTreeMap, BTreeSet};` and add before `fn resync_restarts_the_delta_base`:
```rust
/// Owner decision 12: with every area robot-run, the spectators vote.
#[test]
fn a_lone_spectator_runs_the_clock_of_a_robot_only_game() {
    let mut g = game();
    join(&mut g, "sam", None);
    assert_eq!(g.voters(), BTreeSet::from([s("sam")]));
    assert!(send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Pause }).is_empty());
    assert!(g.clock().paused, "a lone voter's proposal applies at once");
    send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Speed { x: 4 } });
    send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Resume });
    assert_eq!((g.clock().paused, g.clock().speed), (false, 4));
}

#[test]
fn spectators_of_a_robot_only_game_must_all_agree() {
    let mut g = game();
    join(&mut g, "sam", None);
    join(&mut g, "tom", None);
    send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Pause });
    assert!(!g.clock().paused && g.clock().vote.is_some(), "tom has not agreed");
    send(&mut g, "tom", ClientMsg::Vote { proposal: Proposal::Pause });
    assert!(g.clock().paused);
}

#[test]
fn a_spectator_who_leaves_can_complete_a_spectators_vote() {
    let mut g = game();
    join(&mut g, "sam", None);
    join(&mut g, "tom", None);
    send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Pause });
    g.disconnect("tom");
    assert!(g.clock().paused, "sam is the only voter left, and agreed");
}

#[test]
fn a_claim_stops_the_spectators_votes_counting() {
    let mut g = game();
    join(&mut g, "sam", None);
    join(&mut g, "tom", None);
    send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Pause });
    join(&mut g, "alice", Some("West"));
    assert_eq!(g.voters(), BTreeSet::from([s("alice")]));
    assert!(!g.clock().paused && g.clock().vote.is_some(), "re-settled: alice has not agreed");
    let out = send(&mut g, "tom", ClientMsg::Vote { proposal: Proposal::Pause });
    assert_eq!(error_codes(&out, "tom"), [codes::NOT_A_HOLDER]);
    assert!(!g.clock().paused);
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Pause });
    assert!(g.clock().paused);
    send(&mut g, "alice", ClientMsg::Release);
    assert_eq!(g.voters(), BTreeSet::from([s("alice"), s("sam"), s("tom")]), "nobody holds an area again");
}

#[test]
fn a_spectator_who_agreed_and_then_claims_completes_the_vote() {
    let mut g = game();
    join(&mut g, "sam", None);
    join(&mut g, "tom", None);
    send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
    claim(&mut g, "sam", "East");
    assert_eq!(g.clock().speed, 8, "sam is now the only voter, and agreed");
}

#[test]
fn a_holder_in_grace_still_counts_and_spectators_still_do_not() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    join(&mut g, "sam", None);
    g.disconnect("alice");
    assert_eq!(g.voters(), BTreeSet::from([s("alice")]), "alice holds West through her grace period");
    let out = send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Pause });
    assert_eq!(error_codes(&out, "sam"), [codes::NOT_A_HOLDER]);
    g.advance(GRACE_S);
    assert_eq!(g.holder("West"), None);
    assert_eq!(g.voters(), BTreeSet::from([s("sam")]), "the grace ran out: now sam decides");
    send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Pause });
    assert!(g.clock().paused);
}
```
(`the_clock_follows_votes` stays as it is: `sam` is refused there because `alice` holds West.)

Create `crates/client-core/tests/votes.rs`:
```rust
//! Who may vote on the clock (owner decision 12), as the client sees it.

mod common;

use common::*;
use protocol::ClientMsg;

#[test]
fn the_robots_name_is_the_games() {
    assert_eq!(client_core::app::ROBOT, game::ROBOT);
}

#[test]
fn you_vote_when_you_hold_an_area_or_nobody_does() {
    let t = Table::new("ann", Some("West"));
    assert!(t.app.game().unwrap().can_vote(), "a holder");
    let mut t = Table::new("sam", None);
    assert!(t.app.game().unwrap().can_vote(), "every area is the robot's");
    t.game.connect("bob");
    t.game.handle("bob", ClientMsg::Claim { area: s("East") });
    t.run(0.3);
    assert_eq!(t.view().holders["East"], "bob");
    assert!(!t.app.game().unwrap().can_vote(), "bob holds East: only holders vote");
    t.game.handle("bob", ClientMsg::Release);
    t.run(0.3);
    assert!(t.app.game().unwrap().can_vote(), "nobody holds an area again");
}
```

In `crates/client-ui/tests/screens.rs`, add before `fn a_layout_without_geometry_says_so`:
```rust
/// Owner decision 12: a spectator gets the clock buttons exactly while
/// nobody holds an area.
#[test]
fn a_spectator_votes_only_while_nobody_holds_an_area() {
    let mut r = Rig::in_game(drawn_twobox(), None);
    let out = r.frame();
    let pause = texts(&out).into_iter().find(|(t, _)| t == "pause").expect("every area is the robot's").1.center();
    r.click(pause, PointerButton::Primary);
    for _ in 0..3 {
        r.frame();
    }
    assert!(r.view().paused, "a lone spectator's pause applies at once");
    r.game.connect("bob");
    r.game.handle("bob", ClientMsg::Claim { area: s("East") });
    for _ in 0..3 {
        r.frame();
    }
    let out = r.frame();
    assert!(has_text(&out, "East: bob"), "{:?}", texts(&out));
    assert!(!texts(&out).iter().any(|(t, _)| t == "resume" || t == "2×"), "no clock buttons now: {:?}", texts(&out));
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-game --test game --test clock`
Expected: compile errors — no `NotAVoter` variant, no method `voters`.
Run: `scripts/cargo test -p signalbox-client-core --test votes`
Expected: compile errors — no `ROBOT` in `client_core::app`, no method `can_vote`.

- [ ] **Step 4: The voters in the game**

In `crates/game/src/clock.rs`, replace the module comment with
```rust
//! The game clock: pause and speed change only when every voter agrees
//! (spec §3.5, amended by the realism spec's owner decision 12: the voters
//! are the holders, or every connected player while nobody holds an area;
//! `Game` decides who they are). Real time is whatever the caller says it is.
```
replace the `NotAHolder` variant and its comment with
```rust
    /// Not one of the voters (a spectator while someone holds an area).
    NotAVoter,
```
and replace `vote`'s signature and first check, its last line, and the whole of `settle` down to the `all(...)` check with:
```rust
    /// `voter` proposes, or agrees to, `proposal`. Returns it if it applied.
    pub fn vote(&mut self, voter: &str, proposal: Proposal, voters: &BTreeSet<String>) -> Result<Option<Proposal>, VoteError> {
        if !voters.contains(voter) {
            return Err(VoteError::NotAVoter);
        }
```
```rust
        Ok(self.settle(voters))
    }

    /// Apply the open proposal if every voter has agreed. With no voters at
    /// all nobody can agree, so the proposal is dropped.
    pub fn settle(&mut self, voters: &BTreeSet<String>) -> Option<Proposal> {
        let v = self.vote.as_ref()?;
        if voters.is_empty() {
            self.vote = None;
            return None;
        }
        if !voters.iter().all(|h| v.agreed.contains(h)) {
```
(the rest of `settle` is unchanged).

In `crates/game/src/game.rs`, replace `holder_set` and `settle_vote` with:
```rust
    /// Who decides the clock (owner decision 12, amending spec §3.5): every
    /// holder, connected or in their grace period; while nobody holds an
    /// area, every connected player. The robot never votes.
    pub fn voters(&self) -> BTreeSet<String> {
        let holders: BTreeSet<String> = self.holders.iter().flatten().cloned().collect();
        if !holders.is_empty() {
            return holders;
        }
        self.players.iter().filter(|(_, p)| p.connected).map(|(name, _)| name.clone()).collect()
    }

    /// The voters changed: an open proposal may now be complete, or have
    /// nobody left to agree to it.
    fn settle_vote(&mut self) {
        let voters = self.voters();
        self.clock.settle(&voters);
    }
```
replace the start of `fn vote` with:
```rust
    fn vote(&mut self, player: &str, proposal: Proposal) -> Vec<Out> {
        let voters = self.voters();
        match self.clock.vote(player, proposal, &voters) {
            Ok(_) => vec![],
            Err(VoteError::NotAVoter) => {
                vec![error(player, codes::NOT_A_HOLDER, "while anyone holds an area, only holders vote")]
            }
```
(the `BadSpeed` arm stays). In `connect`, add `self.settle_vote();` between `p.gone_s = 0.0;` and `self.resync(player)`; in `disconnect`, add `self.settle_vote();` after `self.players.remove(player);` (before its `return`).

- [ ] **Step 5: The client offers the buttons to voters only**

In `crates/client-core/src/app.rs`, add above `pub fn backoff_s`:
```rust
/// The holder the view names for an area nobody holds (`game::ROBOT`).
pub const ROBOT: &str = "robot";
```
and in `impl InGame`, before `pub fn selected`:
```rust
    /// Whether your clock votes count (owner decision 12): you hold an
    /// area, or nobody does (every area is the robot's).
    pub fn can_vote(&self) -> bool {
        self.area().is_some() || self.view().is_some_and(|v| v.holders.values().all(|h| h == ROBOT))
    }
```

In `crates/client-ui/src/screens.rs` `top_bar`, add `let can_vote = g.can_vote();` after `let holding = g.area().is_some();`, and wrap the pause button and the speed loop in it:
```rust
                    // Only voters get the buttons (owner decision 12).
                    if can_vote {
                        let pause = if v.paused { Proposal::Resume } else { Proposal::Pause };
                        if ui.button(proposal_text(pause)).clicked() {
                            act.push(Box::new(move |a| a.vote(pause)));
                        }
                        for x in [1u8, 2, 4, 8] {
                            if ui.selectable_label(!v.paused && v.speed == x, format!("{x}×")).clicked() {
                                act.push(Box::new(move |a| a.vote(Proposal::Speed { x })));
                            }
                        }
                    }
```

- [ ] **Step 6: Run the tests**

Run: `scripts/cargo test -p signalbox-game`
Expected: PASS (the six new tests among them).
Run: `scripts/cargo test -p signalbox-client-core --test votes` and `scripts/cargo test -p signalbox-client-ui --test screens`
Expected: PASS.
Run: `scripts/cargo build --workspace --all-targets --locked`
Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add docs/superpowers/specs/2026-10-01-panel-realism-design.md crates/game/src/clock.rs crates/game/src/game.rs \
  crates/game/tests/clock.rs crates/game/tests/game.rs crates/client-core/src/app.rs crates/client-core/tests/votes.rs \
  crates/client-ui/src/screens.rs crates/client-ui/tests/screens.rs
git commit -m "feat(game): spectators vote on the clock while nobody holds an area (owner decision 12)"
```


---

### Task 2: `ts2-import` — box prefix, workstation letters and line names

**Files:**
- Modify: `crates/ts2-import/src/areas.rs` (`prefix`, `workstation`, validation, defaults; written into the world's `layout`)
- Create: `crates/ts2-import/src/lines.rs` (the lines file: parse, apply)
- Modify: `crates/ts2-import/src/lib.rs` (`pub mod lines;`), `crates/ts2-import/src/main.rs` (`--lines`)
- Modify: `layouts/liverpool-st.areas.json`, `layouts/drain.areas.json`, `layouts/gretz-armainvilliers.areas.json` (explicit prefixes and letters)
- Create: `layouts/liverpool-st.lines.json`, `layouts/drain.lines.json`, `layouts/gretz-armainvilliers.lines.json`
- Test: `crates/ts2-import/tests/areas.rs` (helper gains the new fields; new tests), `crates/ts2-import/tests/lines.rs` (new), `crates/ts2-import/tests/cli.rs`

**Interfaces:**
- Consumes: `ts2_import::areas::{AreasFile, AreaSpec, apply, parse}`; the world's `layout` JSON written by `ts2_import::layout::build` (`lines` of `{segment, x1, y1, x2, y2}`, `labels` of `{text, x, y}`).
- Produces:
  - `AreasFile.prefix: Option<String>`, `AreaSpec.workstation: Option<String>` (both `#[serde(default)]`).
  - `pub fn areas::{valid_prefix(&str) -> bool, valid_workstation(&str) -> bool, default_prefix(title: &str) -> String, default_workstation(i: usize) -> String}`.
  - `AreasError::{BadPrefix(String), BadWorkstations(Vec<String>), DuplicateWorkstations(Vec<String>)}`.
  - After `areas::apply`, a world whose `layout` is a JSON object carries `layout.box_prefix: String` and `layout.workstations: {area name: letter}` (Task 4 reads these keys).
  - `ts2_import::lines::{LineSpec { name: String, direction: Dir, through: Vec<String> }, LinesError, LABEL_ABOVE: f64 = 12.0, parse(&str) -> Result<Vec<LineSpec>, LinesError>, apply(&mut WorldFile, &[LineSpec]) -> Result<(), LinesError>}`; each line adds `{"text", "x", "y", "arrow": [±1.0, 0.0]}` to `layout.labels` (Task 3 adds `arrow` to `LabelGeom`).
  - CLI: `ts2-import <in> -o <out> [--strict] [--areas <file>] [--lines <file>]`; a bad lines file exits 1 with `<path>: <error>` and writes nothing.

- [ ] **Step 1: Write the failing area tests**

In `crates/ts2-import/tests/areas.rs`, add `use serde_json::json;` above `use signalbox_core::world::World;`, and give the `spec` helper the new fields:
```rust
    AreasFile {
        schema: 1,
        prefix: None,
        boundaries: boundaries.iter().map(|s| s.to_string()).collect(),
        areas: areas
            .iter()
            .map(|(n, seeds)| AreaSpec {
                name: n.to_string(),
                seeds: seeds.iter().map(|s| s.to_string()).collect(),
                workstation: None,
            })
            .collect(),
    }
```
Add before `fn check_shipped`:
```rust
/// plain_line split West | East, with an (empty) drawing to carry the prefixes.
fn drawn_split(prefix: Option<&str>, east: Option<&str>) -> Result<WorldFile, AreasError> {
    let mut w = plain_line();
    w.layout = json!({"lines": []});
    let mut sp = spec(&["S2"], &[("West", vec!["TA"]), ("East", vec!["TC"])]);
    sp.prefix = prefix.map(str::to_string);
    sp.areas[1].workstation = east.map(str::to_string);
    areas::apply(&mut w, &sp).map(|_| w)
}

#[test]
fn prefixes_default_to_the_title_and_the_area_order() {
    let w = drawn_split(None, None).unwrap();
    assert_eq!(w.title, "Plain line");
    assert_eq!(w.layout["box_prefix"], "P");
    assert_eq!(w.layout["workstations"], json!({"West": "A", "East": "B"}));
    assert_eq!(w.layout["lines"], json!([]), "the drawing is kept");
    World::from_file(w).unwrap();
}

#[test]
fn prefixes_from_the_file_win() {
    let w = drawn_split(Some("XYZ"), Some("Q")).unwrap();
    assert_eq!(w.layout["box_prefix"], "XYZ");
    assert_eq!(w.layout["workstations"], json!({"West": "A", "East": "Q"}));
}

#[test]
fn a_world_without_a_drawing_gets_no_prefixes() {
    let mut w = plain_line();
    areas::apply(&mut w, &spec(&["S2"], &[("West", vec!["TA"]), ("East", vec!["TC"])])).unwrap();
    assert!(w.layout.is_null(), "the game's defaults apply: {}", w.layout);
}

#[test]
fn bad_prefixes_and_letters_are_hard_errors() {
    for bad in ["", "ab", "ABCD", "L1", "É"] {
        assert_eq!(drawn_split(Some(bad), None).unwrap_err(), AreasError::BadPrefix(bad.to_string()), "{bad:?}");
    }
    for bad in ["", "b", "BB", "7"] {
        assert_eq!(drawn_split(None, Some(bad)).unwrap_err(), AreasError::BadWorkstations(names(&["East"])), "{bad:?}");
    }
    let e = drawn_split(None, Some("A")).unwrap_err();
    assert_eq!(e, AreasError::DuplicateWorkstations(names(&["A"])), "West has A by default");
    assert!(e.to_string().contains("`A`"), "{e}");
}

#[test]
fn prefix_defaults() {
    assert_eq!(areas::default_prefix("London Liverpool Street Station"), "L");
    assert_eq!(areas::default_prefix("2 boxes, été"), "B");
    assert_eq!(areas::default_prefix("42 — ½"), "");
    assert_eq!(areas::default_workstation(0), "A");
    assert_eq!(areas::default_workstation(25), "Z");
    assert_eq!(areas::default_workstation(26), "");
    assert!(areas::valid_prefix("LST") && !areas::valid_prefix("LSTX"));
    assert!(areas::valid_workstation("C") && !areas::valid_workstation("c"));
}
```
At the end of `parse_checks_schema_and_fields`, add:
```rust
    let full = areas::parse(r#"{"schema": 1, "prefix": "L", "areas": [{"name": "West", "seeds": ["TA"], "workstation": "B"}]}"#)
        .unwrap();
    assert_eq!((full.prefix.as_deref(), full.areas[0].workstation.as_deref()), (Some("L"), Some("B")));
```
At the end of `liverpool_street_has_three_boxes`:
```rust
    assert_eq!(w.layout["box_prefix"], "L");
    assert_eq!(w.layout["workstations"], json!({"Liverpool Street": "A", "Bethnal Green": "B", "Hackney & Bow": "C"}));
```
at the end of `drain_has_two_boxes`:
```rust
    assert_eq!(w.layout["box_prefix"], "W", "the Waterloo & City, not the title's L");
    assert_eq!(w.layout["workstations"], json!({"Bank": "A", "Waterloo": "B"}));
```
and make `gretz_has_three_boxes` keep the world and check it:
```rust
fn gretz_has_three_boxes() {
    let w = check_shipped(
        "gretz-armainvilliers",
        &[("Gretz", 123, 47), ("Tournan & Marles", 68, 26), ("Mortcerf & Coulommiers", 36, 22)],
    );
    assert_eq!(w.layout["box_prefix"], "G");
    assert_eq!(w.layout["workstations"], json!({"Gretz": "A", "Tournan & Marles": "B", "Mortcerf & Coulommiers": "C"}));
}
```

- [ ] **Step 2: Write the failing lines tests**

Create `crates/ts2-import/tests/lines.rs`:
```rust
//! Line-name files: one label per named line, hard errors, and the shipped files.

use serde_json::{Value, json};
use signalbox_core::network::Dir;
use signalbox_core::world::World;
use signalbox_core::world::file::WorldFile;
use ts2_import::lines::{self, LABEL_ABOVE, LineSpec, LinesError};

fn converted(name: &str) -> WorldFile {
    let dir = env!("CARGO_MANIFEST_DIR");
    ts2_import::convert(&std::fs::read_to_string(format!("{dir}/tests/data/{name}.json")).unwrap()).unwrap().world
}

fn shipped(name: &str) -> Vec<LineSpec> {
    let text = std::fs::read_to_string(format!("{}/../../layouts/{name}.lines.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    lines::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn line(name: &str, direction: Dir, through: &[&str]) -> LineSpec {
    LineSpec { name: name.to_string(), direction, through: through.iter().map(|s| s.to_string()).collect() }
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// The labels carrying an arrow (the converter's own labels have none).
fn line_labels(w: &WorldFile) -> Vec<Value> {
    w.layout["labels"].as_array().unwrap().iter().filter(|l| l.get("arrow").is_some()).cloned().collect()
}

/// Drain (the Waterloo & City): Bank at the left, Waterloo at the right. The
/// top road's signals 72–75 face right (the world's `up`), the bottom
/// road's 83–86 face left.
#[test]
fn each_line_gets_one_label_where_its_trains_run_to() {
    let mut w = converted("drain");
    let before = w.layout["labels"].as_array().unwrap().len();
    lines::apply(&mut w, &[line("WESTBOUND", Dir::Up, &["73", "74", "75"]), line("EASTBOUND", Dir::Down, &["T22", "84"])])
        .unwrap();
    assert_eq!(w.layout["labels"].as_array().unwrap().len(), before + 2);
    assert_eq!(
        line_labels(&w),
        [
            json!({"text": "WESTBOUND", "x": 1070.0, "y": 100.0 - LABEL_ABOVE, "arrow": [1.0, 0.0]}),
            json!({"text": "EASTBOUND", "x": 260.0, "y": 150.0 - LABEL_ABOVE, "arrow": [-1.0, 0.0]}),
        ],
        "WESTBOUND at the Waterloo end of 73–75, EASTBOUND at the Bank end of 83–84"
    );
    World::from_file(w).unwrap();
}

#[test]
fn unknown_names_and_signals_facing_the_wrong_way_are_hard_errors() {
    let mut w = converted("drain");
    let e = lines::apply(&mut w, &[line("X", Dir::Up, &["73", "nope", "T999"])]).unwrap_err();
    assert_eq!(e, LinesError::UnknownNames(names(&["T999", "nope"])));
    assert!(e.to_string().contains("`nope`"), "{e}");
    let e = lines::apply(&mut w, &[line("X", Dir::Up, &["73", "84", "83"])]).unwrap_err();
    assert_eq!(e, LinesError::WrongDirection(names(&["83", "84"])));
    assert_eq!(lines::apply(&mut w, &[line(" ", Dir::Up, &["73"])]), Err(LinesError::Empty));
    assert_eq!(lines::apply(&mut w, &[line("X", Dir::Up, &[])]), Err(LinesError::Empty));
    let e = lines::apply(&mut w, &[line("POINTS ONLY", Dir::Up, &["T1"])]).unwrap_err();
    assert_eq!(e, LinesError::Undrawn(names(&["POINTS ONLY"])), "T1 is a points section: only its legs, never drawn");
}

#[test]
fn a_failed_apply_leaves_the_world_untouched() {
    let mut w = converted("drain");
    let before = serde_json::to_string(&w).unwrap();
    assert!(lines::apply(&mut w, &[line("A", Dir::Up, &["73"]), line("B", Dir::Up, &["nope"])]).is_err());
    assert_eq!(serde_json::to_string(&w).unwrap(), before);
}

#[test]
fn a_world_without_a_drawing_cannot_be_labelled() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/fixtures/plain_line.json")).unwrap();
    let mut w: WorldFile = serde_json::from_str(&text).unwrap();
    assert_eq!(lines::apply(&mut w, &[line("X", Dir::Up, &["S1"])]), Err(LinesError::NoDrawing));
}

#[test]
fn parse_checks_the_fields() {
    assert!(matches!(lines::parse("{}"), Err(LinesError::Parse(_))));
    assert!(matches!(lines::parse(r#"[{"name": "X", "direction": "left", "through": ["1"]}]"#), Err(LinesError::Parse(_))));
    assert!(matches!(lines::parse(r#"[{"name": "X", "direction": "up", "through": [], "extra": 1}]"#), Err(LinesError::Parse(_))));
    assert_eq!(lines::parse("[]"), Ok(vec![]));
    assert_eq!(
        lines::parse(r#"[{"name": "UP MAIN", "direction": "down", "through": ["39,1V1"]}]"#),
        Ok(vec![line("UP MAIN", Dir::Down, &["39,1V1"])])
    );
}

#[test]
fn the_shipped_line_files_apply() {
    let texts = |w: &WorldFile| line_labels(w).iter().map(|l| l["text"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    let mut w = converted("liverpool-st");
    lines::apply(&mut w, &shipped("liverpool-st")).unwrap();
    assert_eq!(
        texts(&w),
        [
            "DOWN SUBURBAN", "UP SUBURBAN", "DOWN MAIN", "UP MAIN", "DOWN ELECTRIC", "UP ELECTRIC", "DOWN FAST", "UP FAST"
        ]
    );
    for l in line_labels(&w) {
        let down = l["text"].as_str().unwrap().starts_with("DOWN");
        assert_eq!(l["arrow"], json!([if down { 1.0 } else { -1.0 }, 0.0]), "Down is away from Liverpool Street: {l}");
    }
    World::from_file(w).unwrap();
    let mut w = converted("drain");
    lines::apply(&mut w, &shipped("drain")).unwrap();
    assert_eq!(texts(&w), ["WESTBOUND", "EASTBOUND"]);
    let mut w = converted("gretz-armainvilliers");
    let before = serde_json::to_string(&w).unwrap();
    lines::apply(&mut w, &shipped("gretz-armainvilliers")).unwrap();
    assert_eq!(serde_json::to_string(&w).unwrap(), before, "no line names known for Gretz yet");
}
```

In `crates/ts2-import/tests/cli.rs`, change the module comment to ``//! The converter CLI's `--areas` and `--lines` flags.``, add after the `DRAIN_AREAS` constant
```rust
const DRAIN_LINES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../layouts/drain.lines.json");
```
and append:
```rust
#[test]
fn lines_flag_labels_the_lines() {
    let dir = temp_dir("lines");
    let out = dir.join("drain.json");
    let o = cli(&[DRAIN, "-o", out.to_str().unwrap(), "--areas", DRAIN_AREAS, "--lines", DRAIN_LINES]);
    assert!(o.status.success(), "{}", stderr(&o));
    let w: WorldFile = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    let texts: Vec<&str> = w.layout["labels"].as_array().unwrap().iter().filter_map(|l| l["text"].as_str()).collect();
    assert!(texts.contains(&"WESTBOUND") && texts.contains(&"EASTBOUND"), "{texts:?}");
    assert_eq!(w.layout["box_prefix"], "W");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bad_lines_file_fails_with_the_names_and_writes_nothing() {
    let dir = temp_dir("badlines");
    let bad = dir.join("bad.lines.json");
    std::fs::write(&bad, r#"[{"name": "UP", "direction": "up", "through": ["nope"]}]"#).unwrap();
    let out = dir.join("drain.json");
    let o = cli(&[DRAIN, "-o", out.to_str().unwrap(), "--lines", bad.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(stderr(&o).contains("`nope`"), "{}", stderr(&o));
    assert!(!out.exists());
    let o = cli(&[DRAIN, "-o", out.to_str().unwrap(), "--lines"]);
    assert_eq!(o.status.code(), Some(2), "--lines needs a value");
    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `scripts/cargo test -p ts2-import --test areas --test lines --test cli`
Expected: compile errors — no field `prefix`/`workstation`, no module `lines`.

- [ ] **Step 4: Prefixes in the areas file**

In `crates/ts2-import/src/areas.rs`, append to the module comment:
```rust
//! The file also names the box (`prefix`) and each area's workstation letter
//! (realism spec §2.1); both go into the world's client-only `layout`.
```
add `use serde_json::{Map, Value};` after `use serde::Deserialize;`, and replace the bodies of `AreasFile` and `AreaSpec` (their `#[derive(...)]` and `#[serde(deny_unknown_fields)]` lines stay) and add the helpers after them:
```rust
pub struct AreasFile {
    pub schema: u32,
    /// The box's signal prefix, 1 to 3 capital letters; defaults to the
    /// first letter of the world's title.
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub boundaries: Vec<String>,
    pub areas: Vec<AreaSpec>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AreaSpec {
    pub name: String,
    pub seeds: Vec<String>,
    /// This area's workstation letter; defaults to A, B, C… in file order.
    #[serde(default)]
    pub workstation: Option<String>,
}

/// A box prefix: 1 to 3 ASCII capital letters.
pub fn valid_prefix(p: &str) -> bool {
    (1..=3).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_uppercase())
}

/// A workstation letter: one ASCII capital.
pub fn valid_workstation(w: &str) -> bool {
    w.len() == 1 && w.bytes().all(|b| b.is_ascii_uppercase())
}

/// The first ASCII letter of `title`, as a capital; empty if it has none.
pub fn default_prefix(title: &str) -> String {
    title.chars().find(char::is_ascii_alphabetic).map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_default()
}

/// A, B, C… for areas 0, 1, 2…; nothing past Z.
pub fn default_workstation(i: usize) -> String {
    u8::try_from(i).ok().filter(|&i| i < 26).map(|i| char::from(b'A' + i).to_string()).unwrap_or_default()
}
```
Add three variants at the end of `AreasError`:
```rust
    #[error("prefix `{0}` is not 1 to 3 capital letters")]
    BadPrefix(String),
    #[error("areas whose workstation is not one capital letter: {}", list(.0))]
    BadWorkstations(Vec<String>),
    #[error("workstation letters on more than one area: {}", list(.0))]
    DuplicateWorkstations(Vec<String>),
```
In `apply`, make the first lines
```rust
    let (prefix, letters) = prefixes(world, spec)?;
    let owner = assign(world, spec)?;
```
and replace its end (from `World::from_file(out.clone())...`) with:
```rust
    World::from_file(out.clone()).map_err(AreasError::Invalid)?;
    // Display data for clients, beside the drawing (a world without a
    // drawing has no `layout` object and gets the game's defaults).
    if let Some(layout) = out.layout.as_object_mut() {
        let ws: Map<String, Value> =
            spec.areas.iter().zip(letters).map(|(a, l)| (a.name.clone(), Value::String(l))).collect();
        layout.insert("box_prefix".into(), Value::String(prefix));
        layout.insert("workstations".into(), Value::Object(ws));
    }
    *world = out;
    Ok(counts)
}

/// The box prefix and each area's workstation letter, defaults filled in.
fn prefixes(world: &WorldFile, spec: &AreasFile) -> Result<(String, Vec<String>), AreasError> {
    let prefix = match &spec.prefix {
        Some(p) if !valid_prefix(p) => return Err(AreasError::BadPrefix(p.clone())),
        Some(p) => p.clone(),
        None => default_prefix(&world.title),
    };
    let letters: Vec<String> = spec
        .areas
        .iter()
        .enumerate()
        .map(|(i, a)| a.workstation.clone().unwrap_or_else(|| default_workstation(i)))
        .collect();
    let bad: Vec<String> =
        spec.areas.iter().zip(&letters).filter(|(_, l)| !valid_workstation(l)).map(|(a, _)| a.name.clone()).collect();
    if !bad.is_empty() {
        return Err(AreasError::BadWorkstations(bad));
    }
    let mut seen = BTreeSet::new();
    let twice: BTreeSet<String> = letters.iter().filter(|l| !seen.insert(l.as_str())).cloned().collect();
    if !twice.is_empty() {
        return Err(AreasError::DuplicateWorkstations(twice.into_iter().collect()));
    }
    Ok((prefix, letters))
}
```
(`serde_json`'s map is a `BTreeMap` here — the workspace does not enable `preserve_order` — so the written keys are sorted and the output stays byte-identical.)

- [ ] **Step 5: The lines module and the CLI flag**

Create `crates/ts2-import/src/lines.rs`:
```rust
//! Line names for converted worlds, from an optional hand-made per-layout
//! file (realism spec §2.1): each named stretch of line becomes one label in
//! the world's `layout`, with an arrow for the direction trains run on it.
//! Names are opaque; unknown ones are hard errors, as in the areas file.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Value, json};
use signalbox_core::network::Dir;
use signalbox_core::world::file::WorldFile;

/// How far above the end of its stretch a line's label sits, in layout units.
pub const LABEL_ABOVE: f64 = 12.0;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineSpec {
    /// Shown as it is, e.g. `DOWN MAIN`.
    pub name: String,
    /// The world's direction of travel on this line (as its signals' `direction`).
    pub direction: Dir,
    /// Signal or section names along the line.
    pub through: Vec<String>,
}

/// Names quoted, so names containing commas stay readable.
fn list(names: &[String]) -> String {
    names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LinesError {
    #[error("not a lines file: {0}")]
    Parse(String),
    #[error("lines without a name or without `through` names")]
    Empty,
    #[error("unknown names: {}", list(.0))]
    UnknownNames(Vec<String>),
    #[error("signals facing against their line's direction: {}", list(.0))]
    WrongDirection(Vec<String>),
    #[error("the world has no drawing to label")]
    NoDrawing,
    #[error("lines with no drawn track: {}", list(.0))]
    Undrawn(Vec<String>),
}

pub fn parse(json: &str) -> Result<Vec<LineSpec>, LinesError> {
    serde_json::from_str(json).map_err(|e| LinesError::Parse(e.to_string()))
}

#[derive(Deserialize)]
struct DrawnLine {
    segment: String,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

/// Add one label per line to the world's `layout` labels: at the end of
/// the stretch trains run towards, `LABEL_ABOVE` units above it, with
/// `arrow` the direction of travel on screen (`[1, 0]` right, `[-1, 0]`
/// left). All or nothing.
pub fn apply(world: &mut WorldFile, lines: &[LineSpec]) -> Result<(), LinesError> {
    if lines.iter().any(|l| l.name.trim().is_empty() || l.through.is_empty()) {
        return Err(LinesError::Empty);
    }
    let drawn: Vec<DrawnLine> = match world.layout.get("lines") {
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| LinesError::NoDrawing)?,
        None => return Err(LinesError::NoDrawing),
    };
    let sections: BTreeSet<&str> = world.sections.iter().map(|s| s.name.as_str()).collect();
    let seg_section: BTreeMap<&str, &str> =
        world.segments.iter().map(|g| (g.name.as_str(), g.section.as_str())).collect();
    let signals: BTreeMap<&str, (&str, Dir)> =
        world.signals.iter().map(|s| (s.name.as_str(), (s.segment.as_str(), s.direction))).collect();
    let (mut unknown, mut against, mut undrawn) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    let mut labels = Vec::new();
    for line in lines {
        let mut secs: BTreeSet<&str> = BTreeSet::new();
        for name in &line.through {
            if let Some(s) = sections.get(name.as_str()) {
                secs.insert(s);
            } else if let Some(&(seg, dir)) = signals.get(name.as_str()) {
                if dir != line.direction {
                    against.insert(name.clone());
                }
                if let Some(s) = seg_section.get(seg) {
                    secs.insert(s);
                }
            } else {
                unknown.insert(name.clone());
            }
        }
        let stretch: Vec<&DrawnLine> =
            drawn.iter().filter(|d| seg_section.get(d.segment.as_str()).is_some_and(|s| secs.contains(s))).collect();
        if stretch.is_empty() {
            undrawn.insert(line.name.clone());
            continue;
        }
        // Up runs from x1 (the segment's `from` node) to x2.
        let dx: f64 = stretch
            .iter()
            .map(|d| match line.direction {
                Dir::Up => d.x2 - d.x1,
                Dir::Down => d.x1 - d.x2,
            })
            .sum();
        let right = dx >= 0.0;
        let ends = stretch.iter().flat_map(|d| [(d.x1, d.y1), (d.x2, d.y2)]);
        let (x, y) = ends
            .reduce(|a, b| if (right && b.0 > a.0) || (!right && b.0 < a.0) { b } else { a })
            .expect("a stretch has lines");
        let arrow = if right { [1.0, 0.0] } else { [-1.0, 0.0] };
        labels.push(json!({"text": line.name, "x": x, "y": y - LABEL_ABOVE, "arrow": arrow}));
    }
    if !unknown.is_empty() {
        return Err(LinesError::UnknownNames(unknown.into_iter().collect()));
    }
    if !against.is_empty() {
        return Err(LinesError::WrongDirection(against.into_iter().collect()));
    }
    if !undrawn.is_empty() {
        return Err(LinesError::Undrawn(undrawn.into_iter().collect()));
    }
    let Some(layout) = world.layout.as_object_mut() else { return Err(LinesError::NoDrawing) };
    match layout.entry("labels").or_insert_with(|| Value::Array(vec![])) {
        Value::Array(all) => all.extend(labels),
        _ => return Err(LinesError::NoDrawing),
    }
    Ok(())
}
```
In `crates/ts2-import/src/lib.rs` add `pub mod lines;` after `pub mod layout;`.

In `crates/ts2-import/src/main.rs`: import `lines` (`use ts2_import::{areas, convert, lines, report};`); the usage line becomes
```rust
const USAGE: &str = "usage: ts2-import <input.json> -o <world.json> [--strict] [--areas <areas.json>] [--lines <lines.json>]";
```
the argument tuple gains `lines_path` (`let (mut input, mut output, mut strict, mut areas_path, mut lines_path) = (None, None, false, None, None);`); add an arm before `"--strict"`:
```rust
            "--lines" => {
                i += 1;
                match args.get(i) {
                    Some(l) => lines_path = Some(l.clone()),
                    None => return usage(),
                }
            }
```
read and parse the file right after the areas file (before the input is read):
```rust
    let line_specs = match &lines_path {
        Some(p) => match std::fs::read_to_string(p).map_err(|e| e.to_string()).and_then(|t| lines::parse(&t).map_err(|e| e.to_string())) {
            Ok(l) => Some((p.clone(), l)),
            Err(e) => {
                eprintln!("{p}: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
```
and apply it after the areas, just before the world is serialised:
```rust
            if let Some((p, l)) = &line_specs {
                if let Err(e) = lines::apply(&mut c.world, l) {
                    eprintln!("{p}: {e}");
                    return ExitCode::FAILURE;
                }
            }
```

- [ ] **Step 6: The shipped files**

`layouts/liverpool-st.areas.json`:
```json
{
  "schema": 1,
  "prefix": "L",
  "boundaries": ["61", "64", "63", "66", "65", "68", "91", "90", "93", "92", "95", "94"],
  "areas": [
    {"name": "Liverpool Street", "seeds": ["9"], "workstation": "A"},
    {"name": "Bethnal Green", "seeds": ["72"], "workstation": "B"},
    {"name": "Hackney & Bow", "seeds": ["121"], "workstation": "C"}
  ]
}
```
`layouts/drain.areas.json`:
```json
{
  "schema": 1,
  "prefix": "W",
  "boundaries": ["73", "84"],
  "areas": [
    {"name": "Bank", "seeds": ["72"], "workstation": "A"},
    {"name": "Waterloo", "seeds": ["75"], "workstation": "B"}
  ]
}
```
`layouts/gretz-armainvilliers.areas.json`:
```json
{
  "schema": 1,
  "prefix": "G",
  "boundaries": ["39,1V1", "39,1V2", "52,1"],
  "areas": [
    {"name": "Gretz", "seeds": ["3618"], "workstation": "A"},
    {"name": "Tournan & Marles", "seeds": ["504"], "workstation": "B"},
    {"name": "Mortcerf & Coulommiers", "seeds": ["725"], "workstation": "C"}
  ]
}
```
`layouts/liverpool-st.lines.json` (the sections are the converter's names for the TS2 lines tagged `SL_`/`ML_`/`EL_`/`FL_` + `DN`/`UP`, in x order, plus the signals on them; the railway's Down lines run in the world's `up` direction, away from Liverpool Street):
```json
[
  {"name": "DOWN SUBURBAN", "direction": "up", "through": ["T226", "T256", "T278", "91"]},
  {"name": "UP SUBURBAN", "direction": "down", "through": ["T191", "T223", "T263", "T279", "54", "351", "90", "120"]},
  {"name": "DOWN MAIN", "direction": "up", "through": ["T230", "T254", "T297", "93"]},
  {"name": "UP MAIN", "direction": "down", "through": ["T207", "T224", "T265", "T300", "72", "220"]},
  {"name": "DOWN ELECTRIC", "direction": "up", "through": ["T219", "T264", "T307", "75", "95"]},
  {"name": "UP ELECTRIC", "direction": "down", "through": ["T225", "T262", "T308", "74", "222"]},
  {"name": "DOWN FAST", "direction": "up", "through": ["T284"]},
  {"name": "UP FAST", "direction": "down", "through": ["T285", "122"]}
]
```
`layouts/drain.lines.json` (the top road's signals 72–75 face towards Waterloo, the bottom road's 83–86 towards Bank):
```json
[
  {"name": "WESTBOUND", "direction": "up", "through": ["73", "74", "75"]},
  {"name": "EASTBOUND", "direction": "down", "through": ["83", "84", "85", "86"]}
]
```
`layouts/gretz-armainvilliers.lines.json`:
```json
[]
```

- [ ] **Step 7: Run the tests**

Run: `scripts/cargo test -p ts2-import`
Expected: PASS (the convert warning snapshots are unchanged: nothing here runs inside `convert`).
Run: `scripts/cargo run -q -p ts2-import -- crates/ts2-import/tests/data/liverpool-st.json -o /w/target/lst.json --areas /w/layouts/liverpool-st.areas.json --lines /w/layouts/liverpool-st.lines.json && grep -c '"arrow"' target/lst.json`
Expected: exit 0, the same three `area …` lines as before, then `8`.

- [ ] **Step 8: Commit**

```bash
git add crates/ts2-import/src/areas.rs crates/ts2-import/src/lines.rs crates/ts2-import/src/lib.rs crates/ts2-import/src/main.rs \
  crates/ts2-import/tests/areas.rs crates/ts2-import/tests/lines.rs crates/ts2-import/tests/cli.rs layouts/
git commit -m "feat(ts2-import): box prefix, workstation letters and line-name labels for the shipped layouts"
```

---

### Task 3: deleting games from the lobby (owner decision 13) — save, front, protocol, client

**Files:**
- Modify: `docs/superpowers/specs/2026-10-01-panel-realism-design.md` (decision 13 and a new §8, amending the server spec §3.1)
- Modify: `crates/protocol/src/lobby.rs` (`LobbyMsg::DeleteGame`, `GameInfo.creator`, `GameInfo.can_delete`), `crates/protocol/src/lib.rs` (`codes::NOT_ALLOWED`, `codes::GAME_RUNNING`)
- Modify: `crates/game/src/save.rs` (`SaveDb::set_creator`, `SaveSummary.creator`), `crates/game/src/game.rs` (`Game::set_creator`)
- Modify: `crates/server/src/process.rs` (`--creator`), `crates/server/src/config.rs` (`SIGNALBOX_ADMINS`), `crates/server/src/lib.rs`, `crates/server/src/supervisor.rs` (admins, `delete_game`, per-user lists, broadcast, a join re-check)
- Modify: `crates/client-core/src/app.rs` (`App::delete_game`), `crates/client-ui/src/screens.rs` (Delete with an in-page confirm)
- Modify: `deploy/docker-compose.yml` (`SIGNALBOX_ADMINS: skye`), `deploy/README.md` (its row)
- Test: `crates/protocol/tests/lobby.rs`, `crates/game/tests/status.rs`, `crates/server/tests/{supervisor,process,units,release,oidc}.rs`, `crates/server/tests/common/mod.rs`, `crates/client-core/tests/app.rs`, `crates/client-ui/tests/screens.rs`

**Decision (creator in `meta`, no schema bump):** the creator is one more row in the save's key/value `meta` table (`creator`), written by the game process right after it creates the save. `SAVE_SCHEMA` stays 2: `meta` rows are looked up by key, so an older front reading a newer save ignores the row, and this front reads a missing row as `None` (a save from before this task) — which only admins may delete. Bumping the schema would make every existing save unreadable for no gain.

**Interfaces:**
- Consumes: C2's `Supervisor` (`valid_game_id`, `read_summary`, `push`, `current`, `save_path`), `process::Args`, `Config::from_lookup`.
- Produces:
  - `protocol::LobbyMsg::DeleteGame { game: String }` (`{"type": "delete_game", "game": …}`), answered with `LobbyReply::Games` to **every** client, or `error` with `codes::NOT_ALLOWED` (`"not_allowed"`), `codes::GAME_RUNNING` (`"game_running"`), `codes::UNKNOWN_GAME`, or `codes::SAVE_FAILED` if a file cannot be removed.
  - `GameInfo.creator: Option<String>` (absent when `None`), `GameInfo.can_delete: bool` (absent when false), both `#[serde(default)]`.
  - `game::save::SaveDb::set_creator(&self, user: &str) -> Result<(), SaveError>`, `SaveSummary.creator: Option<String>`, `game::Game::set_creator(&mut self, user: &str) -> Result<(), GameError>`.
  - `server::process::CreateArgs.creator: Option<String>` (`--creator <user>`, needs `--create`).
  - `server::config::Config.admins: Vec<String>`, `pub fn server::config::parse_admins(&str) -> Vec<String>`; `SupervisorConfig.admins: BTreeSet<String>`.
  - `Supervisor::list_games_for(&self, user: &str) -> Vec<GameInfo>` (`list_games` stays, with `can_delete` false).
  - `client_core::App::delete_game(&mut self, game: &str)`.

- [ ] **Step 1: Record the decision in the spec**

In `docs/superpowers/specs/2026-10-01-panel-realism-design.md`, add a row to the owner-decisions table after decision 12:
```markdown
| 13 | Deleting games | A saved or crashed game can be deleted from the lobby by its creator or by an admin (`SIGNALBOX_ADMINS`); never a running one. Amends the server spec §3.1 (see §8) |
```
and append:
```markdown
## 8. Deleting games (owner decision 13, 2026-10-01)

Amends `2026-09-30-server-and-protocol-design.md` §3.1 (the lobby). A new lobby message `delete_game {game}` deletes
a **saved or crashed** game for good: its save file and the SQLite `-wal`/`-shm` files beside it, under `saves/` only,
with the id checked exactly as `join` checks it; a crashed game's entry goes too. Starting and running games are
refused (`game_running`). Only the game's creator — recorded as a `creator` row in the save's `meta` table when the
game is created — or an admin may delete it (`not_allowed` otherwise); admins are the usernames in the optional
`SIGNALBOX_ADMINS` (comma-separated, default none), and saves from before this change, which have no creator, are
theirs alone. On success every connected client gets the new `games` list. `GameInfo` gains `creator` and
`can_delete` (computed for the user the list is sent to); the lobby shows a Delete button only where `can_delete`
holds, with a confirm step in the page, and the front re-checks everything regardless.
```

- [ ] **Step 2: Write the failing tests**

`crates/protocol/tests/lobby.rs` — at the end of `lobby_messages`:
```rust
    check_client(
        ClientFrame::Lobby(LobbyMsg::DeleteGame { game: s("g-abcdefgh2345") }),
        json!({"type": "delete_game", "game": "g-abcdefgh2345"}),
    );
```
in `lobby_replies`, the first `GameInfo` gains `creator: None, can_delete: false,` (its JSON is unchanged: both fields are left out when empty), the second gains `creator: Some(s("sam")), can_delete: true,` and its expected JSON object ends `"error": "resume: bad snapshot", "creator": "sam", "can_delete": true}`; at the end of `lobby_replies` add
```rust
    check_server(
        ServerFrame::error(codes::NOT_ALLOWED, "only its creator or an admin may delete a game"),
        json!({"type": "error", "code": "not_allowed", "message": "only its creator or an admin may delete a game"}),
    );
    assert_eq!(codes::GAME_RUNNING, "game_running");
```
and a new test:
```rust
#[test]
fn a_games_list_from_before_deletion_still_reads() {
    let old = json!({"type": "games", "games": [{"id": "g-abcdefgh2345", "layout": "drain", "state": "saved",
                                                "sim_time": 0.0, "areas": [], "players": []}]});
    let Ok(ServerFrame::Lobby(LobbyReply::Games { games })) = ServerFrame::from_json(&old.to_string()) else { panic!() };
    assert_eq!((games[0].creator.clone(), games[0].can_delete), (None, false));
}
```

`crates/game/tests/status.rs` — in `read_summary_reads_meta_and_the_newest_snapshot`, before its last line add `assert_eq!(sum.creator, None, "nobody recorded");`, and add:
```rust
/// Owner decision 13: the save names its creator; a save without the row
/// (every save from before) reads as `None`.
#[test]
fn the_creator_is_kept_in_the_save() {
    let path = temp_save("creator");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    g.set_creator("Hackney & Bow's ann").unwrap();
    drop(g);
    assert_eq!(read_summary(&path).unwrap().creator.as_deref(), Some("Hackney & Bow's ann"));
    let mut g = Game::resume(&path).unwrap();
    assert!(g.save_now().is_empty());
    drop(g);
    assert_eq!(read_summary(&path).unwrap().creator.as_deref(), Some("Hackney & Bow's ann"), "resuming keeps it");
    let mut unsaved = Game::new(twobox(), meta());
    unsaved.set_creator("ann").unwrap();
}
```

`crates/server/tests/process.rs` — in `arguments_parse_and_bad_ones_are_explained`, the expected `CreateArgs` gains `creator: None` (write it one field per line), the `--seed` without `--create` message becomes `"--layout, --layout-name, --seed, --start and --creator need --create"`, and after it add:
```rust
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
```

`crates/server/tests/units.rs` — the `GameInfo` literal gains `creator: None, can_delete: false,`; in `config_defaults_and_overrides`, after the first `web_dir` assertion:
```rust
    assert!(c.admins.is_empty(), "nobody is an admin unless named");
    let c = cfg(&with(&[("SIGNALBOX_ADMINS", " skye, ,ann ,")])).unwrap();
    assert_eq!(c.admins, ["skye", "ann"]);
```
`crates/server/tests/release.rs`, `crates/server/tests/oidc.rs` (`fn config`) and `crates/server/tests/common/mod.rs` (`dev_config`): each `Config` literal gains `admins: vec![],` after `web_dir`.

`crates/server/tests/supervisor.rs` — add `use std::collections::BTreeSet;` at the top; the `SupervisorConfig` in `fn rig` gains `admins: BTreeSet::from([s("root")]),` and the two others (`a_game_whose_binary_is_missing_is_crashed_not_fatal`, `a_game_still_starting_at_shutdown_is_stopped_not_crashed`) gain `admins: BTreeSet::new(),`. Append:
```rust
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
```

`crates/client-core/tests/app.rs` — in `the_lobby_lists_games_and_layouts_and_sends_what_you_ask`, the `GameInfo` gains `creator: Some(s("bob")), can_delete: false,`; after `app.join("g-one");` add `app.delete_game("g-old");` and add `lobby(LobbyMsg::DeleteGame { game: s("g-old") }),` as the last expected frame.

`crates/client-ui/tests/screens.rs` — the `GameInfo` in `the_lobby_lists_games_and_creates_one` gains `creator: Some(s("bob")), can_delete: false,`; add before `fn the_game_screen_shows_bar_trains_alarms_and_the_fitted_diagram`:
```rust
/// Owner decision 13: Delete only where the front says you may, and only
/// after an in-page confirmation.
#[test]
fn deleting_a_game_asks_first() {
    let mut r = Rig::lobby(drawn_twobox());
    let game = |id: &str, can_delete: bool| GameInfo {
        id: s(id),
        layout: s("twobox"),
        state: GameState::Saved,
        sim_time: 25_200.0,
        areas: vec![],
        players: vec![],
        error: None,
        creator: Some(s("ann")),
        can_delete,
    };
    r.h.push(ServerFrame::Lobby(LobbyReply::Games { games: vec![game("g-mine", true), game("g-theirs", false)] }));
    r.frame();
    let out = r.frame();
    let find = |out: &FullOutput, want: &str| texts(out).into_iter().filter(|(t, _)| t == want).map(|(_, at)| at.center()).collect::<Vec<_>>();
    let deletes = find(&out, "Delete");
    assert_eq!(deletes.len(), 1, "only g-mine: {:?}", texts(&out));
    r.click(deletes[0], PointerButton::Primary);
    let out = r.frame();
    assert!(has_text(&out, "Delete for good?"));
    r.click(find(&out, "Cancel")[0], PointerButton::Primary);
    let out = r.frame();
    assert!(!has_text(&out, "Delete for good?") && r.lobby_sent.is_empty(), "cancelled: nothing sent");
    r.click(find(&out, "Delete")[0], PointerButton::Primary);
    let out = r.frame();
    r.click(find(&out, "Yes, delete")[0], PointerButton::Primary);
    assert_eq!(r.lobby_sent, [LobbyMsg::DeleteGame { game: s("g-mine") }]);
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-protocol --test lobby` and `scripts/cargo test -p signalbox-server --test supervisor`
Expected: compile errors — no variant `DeleteGame`, no field `creator`/`can_delete`/`admins`.

- [ ] **Step 4: Protocol**

In `crates/protocol/src/lobby.rs`, add to `LobbyMsg` after `Leave`:
```rust
    /// Delete a saved or crashed game for good (owner decision 13): its
    /// creator or an admin only. Answered with the new `games` list.
    DeleteGame { game: String },
```
add to `GameInfo` after `error`:
```rust
    /// Who created it; `None` for saves from before owner decision 13.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creator: Option<String>,
    /// Whether the user this list was sent to may delete it now.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub can_delete: bool,
```
and make the tag list
```rust
pub const LOBBY_MSG_TYPES: [&str; 6] = ["list_games", "list_layouts", "create_game", "join", "leave", "delete_game"];
```
In `crates/protocol/src/lib.rs`, add at the end of `mod codes`:
```rust
    /// `delete_game` by someone who is neither the creator nor an admin.
    pub const NOT_ALLOWED: &str = "not_allowed";
    /// `delete_game` for a game that is starting or running.
    pub const GAME_RUNNING: &str = "game_running";
```

- [ ] **Step 5: The creator in the save**

In `crates/game/src/save.rs`, add to `SaveSummary` after `sim_time`:
```rust
    /// Who created the game (meta `creator`); `None` in saves from before
    /// owner decision 13, which only admins may delete.
    pub creator: Option<String>,
```
in `read_summary`, read it after `let tick = tick as u64;` and return it as the struct's last field:
```rust
    let creator: Option<String> =
        conn.query_row("SELECT value FROM meta WHERE key = 'creator'", [], |r| r.get(0)).optional()?;
```
```rust
        sim_time: f64::from(start_s) + tick as f64 * TICK_S,
        creator,
    })
```
and add to `impl SaveDb`, before `pub fn busy`:
```rust
    /// Record who created the game (meta `creator`, owner decision 13). A
    /// plain meta row: older readers ignore it and older saves lack it, so
    /// the save schema stays 2.
    pub fn set_creator(&self, user: &str) -> Result<(), SaveError> {
        self.conn.execute("INSERT OR REPLACE INTO meta (key, value) VALUES ('creator', ?1)", params![user])?;
        Ok(())
    }
```
In `crates/game/src/game.rs`, add before `pub fn save_now`:
```rust
    /// Record `user` as the game's creator in its save (owner decision 13);
    /// nothing for a game without a save.
    pub fn set_creator(&mut self, user: &str) -> Result<(), GameError> {
        if let Some(db) = &self.save {
            db.set_creator(user)?;
        }
        Ok(())
    }
```

- [ ] **Step 6: The game process and the configuration**

In `crates/server/src/process.rs`: `USAGE`'s second line ends `[--start HH:MM:SS] [--creator <user>]]";`; `CreateArgs` gains, after `start`,
```rust
    /// Recorded in the save as its creator (owner decision 13).
    pub creator: Option<String>,
```
`Args::parse` gets a fifth optional (`let (mut world, mut layout_name, mut seed, mut start, mut creator) = (None, None, None, None, None);`), an arm `"--creator" => creator = Some(value()?),` after `"--start"`, `creator,` in the `CreateArgs` it builds, and the no-`--create` check becomes
```rust
            if world.is_some() || layout_name.is_some() || seed.is_some() || start.is_some() || creator.is_some() {
                return Err("--layout, --layout-name, --seed, --start and --creator need --create".into());
            }
```
In `open_game`, the create arm ends:
```rust
            let meta = GameMeta { layout: c.layout_name.clone(), seed: c.seed };
            let mut g = Game::create(&args.save, &json, meta).map_err(|e| format!("create: {e}"))?;
            if let Some(user) = &c.creator {
                g.set_creator(user).map_err(|e| format!("create: {e}"))?;
            }
            Ok(g)
```
In `crates/server/src/config.rs`: `Config` gains, after `web_dir`,
```rust
    /// Usernames that may delete any saved game (owner decision 13).
    pub admins: Vec<String>,
```
add above `pub fn decode_hex`
```rust
/// `SIGNALBOX_ADMINS`: comma-separated usernames, blanks dropped.
pub fn parse_admins(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|u| !u.is_empty()).map(str::to_string).collect()
}
```
and end `from_lookup` with
```rust
        let admins = get("SIGNALBOX_ADMINS").map(|a| parse_admins(&a)).unwrap_or_default();
        Ok(Config { addr, data_dir, layouts_dir, public_url, oidc, session_key, game_bin, web_dir, admins })
```
In `crates/server/src/lib.rs`, the `SupervisorConfig` in `start` gains `admins: cfg.admins.iter().cloned().collect(),`.

- [ ] **Step 7: The supervisor**

In `crates/server/src/supervisor.rs`:
- `use std::collections::{BTreeMap, BTreeSet};`
- `SupervisorConfig` gains, after `empty_exit_s`:
```rust
    /// May delete any saved or crashed game (owner decision 13).
    pub admins: BTreeSet<String>,
```
- `Start::Create` gains `creator: String` (`Create { world: PathBuf, layout: String, seed: u64, start: Option<String>, creator: String },`); `create` passes `creator: user.to_string()` in it; in `run_game`, destructure `creator` too and add `cmd.arg("--creator").arg(creator);` after the `--start` block, inside the `if let Start::Create`.
- `handle_frame`: the `ListGames` and `Leave` replies use `self.list_games_for(user)` (the `ListGames` arm becomes a block), and a new arm:
```rust
            ClientFrame::Lobby(LobbyMsg::DeleteGame { game }) => self.delete_game(user, conn, game),
```
- In `join`, in the second locked section, right after the "Someone resumed it meanwhile" block:
```rust
        if !path.exists() {
            // Deleted meanwhile (deletion holds the lock while it unlinks).
            push(&st, user, c, ServerFrame::error(codes::UNKNOWN_GAME, format!("no game `{game}`")));
            return;
        }
```
- Replace the `// ---- the lobby ----` line and the doc comment of `list_games` with:
```rust
    // ---- deleting (owner decision 13) ----

    /// The game's creator, or an admin.
    fn may_delete(&self, user: &str, creator: Option<&str>) -> bool {
        self.cfg.admins.contains(user) || creator == Some(user)
    }

    /// Delete a saved or crashed game: its save file and the SQLite files
    /// beside it, and a crashed game's entry. Everyone gets the new list.
    fn delete_game(&self, user: &str, conn: u64, game: String) {
        if !valid_game_id(&game) {
            return self.reply(user, conn, ServerFrame::error(codes::UNKNOWN_GAME, format!("no game `{game}`")));
        }
        let path = self.save_path(&game);
        let creator = read_summary(&path).ok().and_then(|s| s.creator);
        {
            let mut st = self.lock();
            let Some(c) = Self::current(&st, user, conn) else { return };
            let refuse = |code: &str, why: String| push(&st, user, c, ServerFrame::error(code, why));
            match st.games.get(&game).map(|e| &e.phase) {
                Some(Phase::Starting | Phase::Running) => {
                    return refuse(codes::GAME_RUNNING, format!("`{game}` is running; it can be deleted once it is saved"));
                }
                None if !path.exists() => return refuse(codes::UNKNOWN_GAME, format!("no game `{game}`")),
                Some(Phase::Crashed(_)) | None => {}
            }
            if !self.may_delete(user, creator.as_deref()) {
                return refuse(codes::NOT_ALLOWED, "only its creator or an admin may delete a game".into());
            }
            for suffix in ["", "-wal", "-shm"] {
                let file = self.cfg.saves_dir.join(format!("{game}.sqlite{suffix}"));
                match std::fs::remove_file(&file) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        eprintln!("signalbox-server: cannot delete {}: {e}", file.display());
                        return refuse(codes::SAVE_FAILED, format!("could not delete `{game}`: {e}"));
                    }
                }
            }
            st.games.remove(&game);
        }
        eprintln!("signalbox-server: {user} deleted {game}");
        self.broadcast_games();
    }

    /// `games` as `user` may act on them.
    fn for_user(&self, mut games: Vec<GameInfo>, user: &str) -> Vec<GameInfo> {
        for g in &mut games {
            g.can_delete = g.state != GameState::Running && self.may_delete(user, g.creator.as_deref());
        }
        games
    }

    /// Every client gets the games list, as they may act on it.
    fn broadcast_games(&self) {
        let games = self.list_games();
        let st = self.lock();
        for (user, c) in &st.clients {
            push(&st, user, c, frame(LobbyReply::Games { games: self.for_user(games.clone(), user) }));
        }
    }

    // ---- the lobby ----

    /// The games list as `user` sees it (`can_delete` set for them).
    pub fn list_games_for(&self, user: &str) -> Vec<GameInfo> {
        self.for_user(self.list_games(), user)
    }

    /// Every game: live ones from memory, the rest from their save files
    /// (`can_delete` false: see `list_games_for`).
```
- In `list_games`, each of the three `GameInfo` literals gains `creator` and `can_delete: false`: `creator: s.creator.clone(),` for a readable save, `creator: None,` for an unreadable one and for a live game without a save file (a live game with a readable save starts from that save's entry and keeps its creator).

The lock is held from the phase check to the last unlink, and `join` re-checks the file under the same lock, so a deletion and a resume of the same game cannot interleave: whichever takes the lock second sees the other's result (`game_running`, or `unknown_game`).

- [ ] **Step 8: The client**

In `crates/client-core/src/app.rs`, add before `pub fn join`:
```rust
    /// Delete a saved or crashed game (owner decision 13). The front checks
    /// who may and answers with the new games list, or an error for the lobby.
    pub fn delete_game(&mut self, game: &str) {
        self.send(ClientFrame::Lobby(LobbyMsg::DeleteGame { game: game.to_string() }));
    }
```
In `crates/client-ui/src/screens.rs`: `UiApp` gains
```rust
    /// The game whose Delete was pressed and awaits "Yes, delete".
    confirm_delete: Option<String>,
```
(`confirm_delete: None` in `new`). In `lobby`, add `let mut delete = None;` after `let mut join = None;`, replace the row's Join/Resume button with
```rust
                    ui.horizontal(|ui| {
                        if ui.button(if g.state == GameState::Running { "Join" } else { "Resume" }).clicked() {
                            join = Some(g.id.clone());
                        }
                        // Owner decision 13: the front re-checks all of it.
                        if g.can_delete {
                            if self.confirm_delete.as_deref() == Some(g.id.as_str()) {
                                ui.label(RichText::new("Delete for good?").color(ALARM));
                                if ui.button("Yes, delete").clicked() {
                                    delete = Some(g.id.clone());
                                }
                                if ui.button("Cancel").clicked() {
                                    self.confirm_delete = None;
                                }
                            } else if ui.button("Delete").clicked() {
                                self.confirm_delete = Some(g.id.clone());
                            }
                        }
                    });
```
and after the `if let Some(id) = join { … }` block:
```rust
            if let Some(id) = delete {
                self.confirm_delete = None;
                self.core.delete_game(&id);
            }
```

- [ ] **Step 9: Deployment configuration**

`deploy/docker-compose.yml`, in `environment` after `OIDC_ISSUER`:
```yaml
      # May delete anyone's saved game from the lobby (creators may delete their own).
      SIGNALBOX_ADMINS: skye
```
`deploy/README.md`, in the configuration table after the `SIGNALBOX_WEB` row:
```markdown
| `SIGNALBOX_ADMINS` | compose | `skye` (comma-separated usernames that may delete any saved or crashed game; a game's creator may always delete their own; default nobody) |
```

- [ ] **Step 10: Run the tests**

Run: `scripts/cargo test -p signalbox-protocol -p signalbox-game -p signalbox-client-core -p signalbox-client-ui`
Expected: PASS.
Run: `scripts/cargo test -p signalbox-server --features dev-auth`
Expected: PASS (the five new supervisor tests among them; the front's own tests are unchanged).
Run: `scripts/cargo build --workspace --all-targets --locked`
Expected: no warnings.

- [ ] **Step 11: Commit**

```bash
git add docs/superpowers/specs/2026-10-01-panel-realism-design.md crates/protocol crates/game/src/save.rs crates/game/src/game.rs \
  crates/game/tests/status.rs crates/server crates/client-core/src/app.rs crates/client-core/tests/app.rs \
  crates/client-ui/src/screens.rs crates/client-ui/tests/screens.rs deploy/docker-compose.yml deploy/README.md
git commit -m "feat(server): creators and admins delete saved or crashed games from the lobby (owner decision 13)"
```

---

### Task 4: `protocol` — prefixes, workstation letters, the simplifier and label arrows on the wire

**Files:**
- Modify: `crates/protocol/src/view.rs` (`Layout.box_prefix`, `Layout.workstations`, `Layout.simplifier`, `SimplifierRow`, `SimplifierCall`, `LabelGeom.arrow`)
- Modify: `crates/game/src/layout.rs` (the new `Layout` fields, empty; Task 5 fills them)
- Modify (struct literals only): `crates/bot/tests/strategy.rs`, `crates/bot/tests/play.rs`, `crates/client-core/tests/app.rs`, `crates/client-core/tests/input.rs`, `crates/game/tests/geometry.rs`, `crates/client-ui/tests/scene.rs`
- Test: `crates/protocol/tests/golden.rs`

**Interfaces:**
- Consumes: D1's `Layout`, `Geometry`, `LabelGeom`.
- Produces (all `pub`, re-exported by `pub use view::*`):
  - `Layout.box_prefix: String`, `Layout.workstations: BTreeMap<String, String>` (area → letter), `Layout.simplifier: Vec<SimplifierRow>` — all `#[serde(default)]`, always serialised.
  - `struct SimplifierRow { headcode: String, origin: Option<String>, destination: Option<String>, calls: Vec<SimplifierCall> }` and `struct SimplifierCall { place: String, platform: Option<String>, arr: Option<f64>, dep: Option<f64>, stops: bool }` (`Clone, Debug, PartialEq, Serialize, Deserialize`; times in seconds since midnight).
  - `LabelGeom.arrow: Option<[f64; 2]>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`: a label without an arrow is written exactly as in D1).

- [ ] **Step 1: Write the failing tests**

In `crates/protocol/tests/golden.rs`: in `small_layout()` add after `geometry: None,`
```rust
        box_prefix: String::new(),
        workstations: BTreeMap::new(),
        simplifier: vec![],
```
in `fn layout()` the expected JSON's last line becomes
```rust
            "geometry": null, "box_prefix": "", "workstations": {}, "simplifier": []
```
in `layout_geometry` the label becomes `LabelGeom { text: s("Hackney & Bow"), x: -20.0, y: 615.0, arrow: None }` (its expected JSON is unchanged); replace the first four lines of `a_layout_or_view_from_before_d1_still_reads` with
```rust
    let mut json = serde_json::to_value(ServerMsg::Layout(small_layout())).unwrap();
    for key in ["geometry", "box_prefix", "workstations", "simplifier"] {
        json.as_object_mut().unwrap().remove(key);
    }
    let ServerMsg::Layout(l) = serde_json::from_value(json).unwrap() else { panic!() };
    assert_eq!(l.geometry, None);
    assert_eq!((l.box_prefix.as_str(), l.workstations.len(), l.simplifier.len()), ("", 0, 0));
```
and add before it:
```rust
/// Realism spec §2.1 and §3: prefixes, workstation letters, the simplifier
/// and a line name's arrow.
#[test]
fn layout_display_data() {
    let mut l = small_layout();
    l.box_prefix = s("L");
    l.workstations = BTreeMap::from([(s("West"), s("A")), (s("East"), s("B"))]);
    l.simplifier = vec![SimplifierRow {
        headcode: s("1A07"),
        origin: Some(s("BOWJ")),
        destination: Some(s("LIVST")),
        calls: vec![
            SimplifierCall { place: s("WSJ"), platform: Some(s("ML_UP")), arr: None, dep: Some(34_170.0), stops: false },
            SimplifierCall { place: s("LIVST"), platform: Some(s("12")), arr: Some(34_380.0), dep: None, stops: true },
        ],
    }];
    l.geometry = Some(Geometry {
        labels: vec![
            LabelGeom { text: s("UP MAIN"), x: 885.0, y: 488.0, arrow: Some([-1.0, 0.0]) },
            LabelGeom { text: s("BANK"), x: 60.0, y: 50.0, arrow: None },
        ],
        ..Geometry::default()
    });
    let json = serde_json::to_value(ServerMsg::Layout(l.clone())).unwrap();
    assert_eq!(json["box_prefix"], "L");
    assert_eq!(json["workstations"], json!({"East": "B", "West": "A"}));
    assert_eq!(
        json["simplifier"],
        json!([{"headcode": "1A07", "origin": "BOWJ", "destination": "LIVST", "calls": [
            {"place": "WSJ", "platform": "ML_UP", "arr": null, "dep": 34170.0, "stops": false},
            {"place": "LIVST", "platform": "12", "arr": 34380.0, "dep": null, "stops": true}
        ]}])
    );
    assert_eq!(
        json["geometry"]["labels"],
        json!([{"text": "UP MAIN", "x": 885.0, "y": 488.0, "arrow": [-1.0, 0.0]}, {"text": "BANK", "x": 60.0, "y": 50.0}]),
        "a label without an arrow is written as before"
    );
    let back: ServerMsg = serde_json::from_value(json).unwrap();
    assert_eq!(back, ServerMsg::Layout(l));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-protocol --test golden`
Expected: compile errors — no field `box_prefix` on `Layout`, no `SimplifierRow`, no field `arrow` on `LabelGeom`.

- [ ] **Step 3: The types**

In `crates/protocol/src/view.rs`, add to `Layout` after `geometry`:
```rust
    /// The box's signal prefix (`L` for Liverpool Street); may be empty.
    #[serde(default)]
    pub box_prefix: String,
    /// Area → its workstation letter (realism spec §2, owner decision 11).
    #[serde(default)]
    pub workstations: BTreeMap<String, String>,
    /// The timetable for your area (spectators: all of it), in running
    /// order (realism spec §3).
    #[serde(default)]
    pub simplifier: Vec<SimplifierRow>,
```
add after `Layout`:
```rust
/// One service in the simplifier: where it runs from and to, and its calls
/// in the area.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimplifierRow {
    pub headcode: String,
    /// The first call's place, the last call's place.
    pub origin: Option<String>,
    pub destination: Option<String>,
    pub calls: Vec<SimplifierCall>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimplifierCall {
    pub place: String,
    pub platform: Option<String>,
    /// Booked times, seconds since midnight.
    pub arr: Option<f64>,
    pub dep: Option<f64>,
    /// `false`: booked to pass.
    pub stops: bool,
}
```
and add to `LabelGeom` after `y`:
```rust
    /// A line name's direction of travel: the arrow is drawn at (x, y)
    /// pointing this way, the text on the other side of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrow: Option<[f64; 2]>,
```
(`game::geometry` reads the world's labels straight into `LabelGeom`, so Task 2's arrows now travel to clients with no change there.)

In `crates/game/src/layout.rs`, `build_layout` ends:
```rust
        geometry: geo.map(|g| g.visible(w, vis)),
        // Display data: `Game` adds it from what it builds once per game.
        box_prefix: String::new(),
        workstations: Default::default(),
        simplifier: vec![],
    }
```

- [ ] **Step 4: Update the other literals**

After every `geometry: None,` in a `Layout` literal in `crates/bot/tests/strategy.rs` (`fn screen`), `crates/bot/tests/play.rs`, `crates/client-core/tests/app.rs` (`fn layout`) and `crates/client-core/tests/input.rs` (`fn auto_layout`), add the same three lines as in `small_layout()` (each file already imports `BTreeMap`). Add `arrow: None` to the `LabelGeom` literals in `crates/game/tests/geometry.rs` (`LabelGeom { text: "West".into(), x: 50.0, y: -30.0, arrow: None }`) and `crates/client-ui/tests/scene.rs` (both `g.labels.push(protocol::LabelGeom { …, arrow: None })`).

- [ ] **Step 5: Run the tests**

Run: `scripts/cargo test -p signalbox-protocol -p signalbox-game -p signalbox-bot -p signalbox-client-core -p signalbox-client-ui`
Expected: PASS.
Run: `scripts/cargo build --workspace --all-targets --locked`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/protocol crates/game/src/layout.rs crates/game/tests/geometry.rs crates/bot/tests crates/client-core/tests \
  crates/client-ui/tests/scene.rs
git commit -m "feat(protocol): box prefix, workstation letters, simplifier rows and label arrows in the layout"
```

---

### Task 5: `game` — prefixes with their defaults, and the simplifier per area

**Files:**
- Create: `crates/game/src/display.rs` (`prefixes`, `simplifier`, `Display`)
- Modify: `crates/game/src/lib.rs` (`pub mod display;`), `crates/game/src/game.rs` (build a `Display` once; fill every layout)
- Test: `crates/game/tests/display.rs` (new), `crates/game/tests/geometry.rs`

**Interfaces:**
- Consumes: Task 2's `layout.box_prefix` / `layout.workstations` keys in the world's `layout` JSON; Task 4's `Layout` fields and `SimplifierRow`/`SimplifierCall`; core's `World` (`services`, `net.platforms`, `net.areas`, `title`, `layout`).
- Produces:
  - `pub fn game::display::default_box_prefix(title: &str) -> String`, `default_workstation(i: usize) -> String` (the same rules as ts2-import's, which the game cannot depend on).
  - `pub fn game::display::prefixes(w: &World) -> (String, BTreeMap<String, String>)` — every world area gets a letter.
  - `pub fn game::display::simplifier(w: &World, area: Option<AreaId>) -> Vec<SimplifierRow>`.
  - `pub struct game::display::Display { pub box_prefix: String, pub workstations: BTreeMap<String, String>, .. }` with `from_world(&World) -> Display`, `simplifier(&self, Option<AreaId>) -> &[SimplifierRow]`, `fill(&self, &mut Layout, Option<AreaId>)`.
  - Every `Layout` a `Game` sends (connect, claim, release, resync) and `Game::layout_of` carry the prefix, the letters and the player's simplifier.

- [ ] **Step 1: Write the failing tests**

Create `crates/game/tests/display.rs`:
```rust
//! Display data (realism spec §2.1, §3): prefixes and workstation letters
//! with their defaults, and the simplifier per area and for spectators.

mod common;

use common::*;
use game::display::{Display, default_box_prefix, default_workstation, prefixes, simplifier};
use game::{Game, GameMeta};
use protocol::{ServerMsg, SimplifierCall, SimplifierRow};
use serde_json::{Value, json};
use signalbox_core::world::World;

fn twobox_mut(f: impl FnOnce(&mut Value)) -> World {
    let mut json: Value = serde_json::from_str(&twobox_json()).unwrap();
    f(&mut json);
    World::from_json(&json.to_string()).unwrap()
}

fn s(x: &str) -> String {
    x.to_string()
}

fn call(place: &str, arr: Option<f64>, dep: Option<f64>) -> SimplifierCall {
    SimplifierCall { place: s(place), platform: Some(s("1")), arr, dep, stops: true }
}

#[test]
fn prefixes_default_to_the_title_and_the_area_order() {
    let (b, ws) = prefixes(&twobox());
    assert_eq!(b, "T", "Two boxes");
    assert_eq!(ws, map(&[("West", s("A")), ("East", s("B"))]));
    assert_eq!(default_box_prefix("42 — été"), "T");
    assert_eq!(default_box_prefix("½"), "");
    assert_eq!((default_workstation(2), default_workstation(26)), (s("C"), s("")));
}

#[test]
fn prefixes_come_from_the_worlds_layout_and_bad_ones_fall_back() {
    let w = twobox_mut(|j| j["layout"] = json!({"box_prefix": "XY", "workstations": {"East": "Q", "West": "ab"}}));
    assert_eq!(prefixes(&w), (s("XY"), map(&[("West", s("A")), ("East", s("Q"))])), "West's `ab` is not a letter");
    let w = twobox_mut(|j| j["layout"] = json!({"box_prefix": "", "workstations": {}}));
    assert_eq!(prefixes(&w).0, "", "an empty prefix is a choice: no prefix");
    for bad in [json!("TOOLONG"), json!("lc"), json!(7), json!(null)] {
        let w = twobox_mut(|j| j["layout"] = json!({"box_prefix": bad}));
        assert_eq!(prefixes(&w).0, "T", "{bad}");
    }
    let w = twobox_mut(|j| j["layout"] = json!("not an object"));
    assert_eq!(prefixes(&w), (s("T"), map(&[("West", s("A")), ("East", s("B"))])));
}

/// twobox: 1E01 calls at EST 1 07:04–07:05 and 1N02 at NST 1 07:18–07:19,
/// both platforms in East; 2W03 and 2W04 have no calls.
#[test]
fn each_area_lists_the_services_calling_at_its_platforms() {
    let w = twobox();
    let east = simplifier(&w, w.net.area("East"));
    assert_eq!(
        east,
        [
            SimplifierRow {
                headcode: s("1E01"),
                origin: Some(s("EST")),
                destination: Some(s("EST")),
                calls: vec![call("EST", Some(25_440.0), Some(25_500.0))],
            },
            SimplifierRow {
                headcode: s("1N02"),
                origin: Some(s("NST")),
                destination: Some(s("NST")),
                calls: vec![call("NST", Some(26_280.0), Some(26_340.0))],
            },
        ]
    );
    assert!(simplifier(&w, w.net.area("West")).is_empty(), "West has no platforms");
    let all: Vec<String> = simplifier(&w, None).into_iter().map(|r| r.headcode).collect();
    assert_eq!(all, ["1E01", "1N02", "2W03", "2W04"], "spectators: every service, untimed ones last");
}

#[test]
fn calls_count_by_platform_or_by_place_and_rows_run_in_time_order() {
    let w = twobox_mut(|j| {
        j["services"].as_array_mut().unwrap().extend([
            json!({"headcode": "9Z99", "train_type": "EMU", "calls": [
                {"place": "EST", "dep": "06:59:30", "stop": false},
                {"place": "NST", "platform": "1", "arr": "07:30"}
            ]}),
            json!({"headcode": "0A00", "train_type": "EMU", "calls": [{"place": "EST", "platform": "1", "arr": "07:04"}]}),
        ]);
    });
    let east = simplifier(&w, w.net.area("East"));
    let order: Vec<&str> = east.iter().map(|r| r.headcode.as_str()).collect();
    assert_eq!(order, ["9Z99", "0A00", "1E01", "1N02"], "by first listed call, then headcode");
    let z = &east[0];
    assert_eq!((z.origin.as_deref(), z.destination.as_deref()), (Some("EST"), Some("NST")));
    assert_eq!(
        z.calls,
        [
            SimplifierCall { place: s("EST"), platform: None, arr: None, dep: Some(25_170.0), stops: false },
            SimplifierCall { place: s("NST"), platform: Some(s("1")), arr: Some(27_000.0), dep: None, stops: true },
        ],
        "a call without a platform counts when a platform of its place is in the area"
    );
    assert!(simplifier(&w, w.net.area("West")).is_empty());
}

#[test]
fn the_game_puts_them_in_every_layout() {
    let mut g = Game::new(twobox(), GameMeta { layout: s("twobox"), seed: 1 });
    let out = g.connect("sam");
    let ServerMsg::Layout(l) = &out[0].1 else { panic!("{out:?}") };
    assert_eq!((l.box_prefix.as_str(), l.workstations.len(), l.simplifier.len()), ("T", 2, 4));
    let out = g.handle("sam", protocol::ClientMsg::Claim { area: s("West") });
    let ServerMsg::Layout(l) = &out[0].1 else { panic!("{out:?}") };
    assert!(l.simplifier.is_empty(), "the claim's resync carries West's (empty) simplifier");
    assert_eq!(g.layout_of("sam").unwrap(), *l);
    let d = Display::from_world(g.sim().world());
    assert_eq!(d.simplifier(g.sim().world().net.area("East")).len(), 2);
}

/// The three boxes of Liverpool Street, as shipped.
#[test]
fn liverpool_street_has_its_prefixes_and_a_simplifier_per_box() {
    let w = World::from_json(&liverpool_json()).unwrap();
    let (b, ws) = prefixes(&w);
    assert_eq!(b, "L");
    assert_eq!(ws, map(&[("Liverpool Street", s("A")), ("Bethnal Green", s("B")), ("Hackney & Bow", s("C"))]));
    let d = Display::from_world(&w);
    assert_eq!(d.simplifier(None).len(), w.services.len());
    for a in ["Liverpool Street", "Bethnal Green", "Hackney & Bow"] {
        let rows = d.simplifier(w.net.area(a));
        assert!(!rows.is_empty(), "{a}");
        let times: Vec<f64> = rows.iter().filter_map(|r| r.calls.iter().find_map(|c| c.arr.or(c.dep))).collect();
        assert!(times.windows(2).all(|p| p[0] <= p[1]), "{a} runs in time order");
    }
    let mut g = Game::new(w, GameMeta { layout: s("liverpool-st"), seed: 1 });
    let out = g.connect("sam");
    let size = serde_json::to_string(&out[0].1).unwrap().len();
    assert!(size < 1 << 20, "a spectator's layout is {size} bytes; the ipc frame limit is 4 MiB");
}
```
Append to `crates/game/tests/geometry.rs`:
```rust
/// A line name written by `ts2-import --lines` keeps its arrow.
#[test]
fn a_line_names_arrow_reaches_the_client() {
    let mut layout = twobox_layout();
    layout["labels"].as_array_mut().unwrap().push(json!({"text": "UP MAIN", "x": 20.0, "y": -12.0, "arrow": [-1.0, 0.0]}));
    let g = geometry_for(&twobox_with(layout), Some("West")).unwrap();
    let up = LabelGeom { text: "UP MAIN".into(), x: 20.0, y: -12.0, arrow: Some([-1.0, 0.0]) };
    assert!(g.labels.contains(&up), "{:?}", g.labels);
}
```
(Calls in a test world must name places and platforms the world has — the loader refuses anything else — so "a call outside the area" is West's empty list rather than a made-up place.)

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-game --test display --test geometry`
Expected: `display` fails to compile (no module `game::display`); `a_line_names_arrow_reaches_the_client` passes already (Task 4 carried the arrow through), which is fine: it pins it.

- [ ] **Step 3: The module**

Create `crates/game/src/display.rs`:
```rust
//! Display data for clients (realism spec §2.1, §3), built once per game
//! from the world alone: the box prefix, each area's workstation letter,
//! and the simplifier for each area and for spectators.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use protocol::{Layout, SimplifierCall, SimplifierRow};
use signalbox_core::ids::{AreaId, SectionId};
use signalbox_core::timetable::Call;
use signalbox_core::world::World;

/// The first ASCII letter of `title`, as a capital; empty if it has none.
pub fn default_box_prefix(title: &str) -> String {
    title.chars().find(char::is_ascii_alphabetic).map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_default()
}

/// A, B, C… for areas 0, 1, 2…; nothing past Z.
pub fn default_workstation(i: usize) -> String {
    u8::try_from(i).ok().filter(|&i| i < 26).map(|i| char::from(b'A' + i).to_string()).unwrap_or_default()
}

fn capitals(s: &str, len: std::ops::RangeInclusive<usize>) -> bool {
    len.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_uppercase())
}

/// The box prefix and workstation letters from the world's `layout` JSON
/// (`box_prefix`, `workstations`, written by `ts2-import --areas`), each
/// falling back to its default when missing or malformed: older worlds and
/// saves, and hand-made worlds, simply have none.
pub fn prefixes(w: &World) -> (String, BTreeMap<String, String>) {
    let box_prefix = match w.layout.get("box_prefix").and_then(|v| v.as_str()) {
        Some(p) if p.is_empty() || capitals(p, 1..=3) => p.to_string(),
        _ => default_box_prefix(&w.title),
    };
    let given = w.layout.get("workstations");
    let workstations = w
        .net
        .areas
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let letter = match given.and_then(|g| g.get(&a.name)).and_then(|v| v.as_str()) {
                Some(l) if capitals(l, 1..=1) => l.to_string(),
                _ => default_workstation(i),
            };
            (a.name.clone(), letter)
        })
        .collect();
    (box_prefix, workstations)
}

fn call_time(c: &SimplifierCall) -> Option<f64> {
    c.arr.or(c.dep)
}

/// Every service with a call in `area` (a spectator: every service, every
/// call), in running order: by the time of the first listed call (untimed
/// last), then headcode, then world order.
pub fn simplifier(w: &World, area: Option<AreaId>) -> Vec<SimplifierRow> {
    let net = &w.net;
    let in_area = |s: SectionId| area.is_none_or(|a| net.sections[s.idx()].area == a);
    let listed = |c: &Call| {
        area.is_none()
            || net.platforms.iter().any(|p| {
                p.place == c.place
                    && c.platform.as_deref().is_none_or(|pf| p.platform == pf)
                    && in_area(net.segments[p.segment.idx()].section)
            })
    };
    let mut rows: Vec<SimplifierRow> = w
        .services
        .iter()
        .filter_map(|svc| {
            let calls: Vec<SimplifierCall> = svc
                .calls
                .iter()
                .filter(|c| listed(c))
                .map(|c| SimplifierCall {
                    place: c.place.clone(),
                    platform: c.platform.clone(),
                    arr: c.arr_s,
                    dep: c.dep_s,
                    stops: c.stop,
                })
                .collect();
            (area.is_none() || !calls.is_empty()).then(|| SimplifierRow {
                headcode: svc.headcode.clone(),
                origin: svc.calls.first().map(|c| c.place.clone()),
                destination: svc.calls.last().map(|c| c.place.clone()),
                calls,
            })
        })
        .collect();
    let first = |r: &SimplifierRow| r.calls.iter().find_map(call_time);
    // A stable sort: services equal in time and headcode keep world order.
    rows.sort_by(|a, b| {
        match (first(a), first(b)) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
        .then_with(|| a.headcode.cmp(&b.headcode))
    });
    rows
}

/// What every layout of one game shares, per area.
#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    pub box_prefix: String,
    pub workstations: BTreeMap<String, String>,
    spectator: Vec<SimplifierRow>,
    by_area: Vec<Vec<SimplifierRow>>,
}

impl Display {
    pub fn from_world(w: &World) -> Display {
        let (box_prefix, workstations) = prefixes(w);
        let by_area = (0..w.net.areas.len()).map(|a| simplifier(w, Some(AreaId::from_idx(a)))).collect();
        Display { box_prefix, workstations, spectator: simplifier(w, None), by_area }
    }

    /// The simplifier for `area` (a spectator's for `None`).
    pub fn simplifier(&self, area: Option<AreaId>) -> &[SimplifierRow] {
        match area {
            Some(a) => self.by_area.get(a.idx()).map_or(&[], Vec::as_slice),
            None => &self.spectator,
        }
    }

    /// Put the display data for a player of `area` into their layout.
    pub fn fill(&self, l: &mut Layout, area: Option<AreaId>) {
        l.box_prefix = self.box_prefix.clone();
        l.workstations = self.workstations.clone();
        l.simplifier = self.simplifier(area).to_vec();
    }
}
```
Add `pub mod display;` after `pub mod clock;` in `crates/game/src/lib.rs`.

- [ ] **Step 4: Every layout carries it**

In `crates/game/src/game.rs`: `use crate::display::Display;` after the `crate::clock` import; a field after `geometry`:
```rust
    /// Prefixes and simplifiers, built once from the world.
    display: Display,
```
built in `from_sim` next to the geometry (`let display = Display::from_world(w);`, and `display,` after `geometry,` in the struct literal); `layout_of` becomes
```rust
    pub fn layout_of(&self, player: &str) -> Option<Layout> {
        let p = self.players.get(player)?;
        let mut layout = build_layout(self.sim.world(), &self.map, &p.vis, player, self.geometry.as_ref());
        self.display.fill(&mut layout, p.vis.area);
        Some(layout)
    }
```
and in `resync` the layout line becomes
```rust
        let mut layout = build_layout(self.sim.world(), &self.map, &p.vis, player, self.geometry.as_ref());
        self.display.fill(&mut layout, p.vis.area);
```
(`p` borrows `self.players` mutably there; `self.display` is a different field, so this compiles as it is.)

- [ ] **Step 5: Run the tests**

Run: `scripts/cargo test -p signalbox-game`
Expected: PASS — including `every_client_rebuilds_the_servers_view_from_deltas`, which compares every client's layout with `layout_of` through claims and releases.
Run: `scripts/cargo test -p signalbox-server --features dev-auth --test front` and `scripts/cargo build --workspace --all-targets --locked`
Expected: PASS, no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/game/src/display.rs crates/game/src/lib.rs crates/game/src/game.rs crates/game/tests/display.rs crates/game/tests/geometry.rs
git commit -m "feat(game): box prefix, workstation letters and per-area simplifier in every layout"
```

---

### Task 6: `client-core` — display names everywhere, a steady refusal outline, and the ○A target

**Files:**
- Create: `crates/client-core/src/names.rs` (`Names`)
- Modify: `crates/client-core/src/lib.rs` (`pub mod names; pub use names::Names;`)
- Modify: `crates/client-core/src/text.rs` (`command_text`, `notice_text` take `&Names`)
- Modify: `crates/client-core/src/app.rs` (`InGame::names`, rebuilt with each layout; `flash`/`flashing`/`FLASH_S` → `refused`/`refused()`/`REFUSED_S`)
- Modify: `crates/client-core/src/select.rs` (names in menus and hover; `auto_toggle`, `auto_working`, `describe_auto`)
- Modify: `crates/client-core/src/input.rs` (`Target::Auto`)
- Modify: `crates/client-ui/src/screens.rs` (one call: `g.refused()`), `crates/server/tests/client.rs` (one call: `command_text` with the names)
- Test: `crates/client-core/tests/names.rs` (new), `crates/client-core/tests/text.rs`, `crates/client-core/tests/input.rs`, `crates/client-ui/tests/screens.rs`

**Interfaces:**
- Consumes: Task 4's `Layout.box_prefix` and `Layout.workstations`; D1's `select`, `text`, `input`, `App`, `InGame`.
- Produces:
  - `pub struct client_core::Names` (`Clone, Debug, Default, PartialEq`): `new(&Layout) -> Names`, `signal(&self, &str) -> String`, `exit(&self, &ExitName) -> String`, `workstation(&self, area: &str) -> Option<&str>` (`None` on a single-area layout).
  - `pub fn InGame::names(&self) -> &Names`; `pub fn InGame::refused(&self) -> Option<&str>`; `pub const client_core::app::REFUSED_S: f64 = 2.0` (`flashing` and `FLASH_S` are gone).
  - `pub fn text::command_text(c: &PlayerCommand, names: &Names) -> String`; `pub fn text::notice_text(n: &Notice, names: &Names) -> (String, bool)`.
  - `client_core::Target::Auto(String)` (the ○A button beside a signal): `App::click` sends `select::auto_toggle`'s command if any and never touches the selection; `App::menu` is empty; `App::describe` is `select::describe_auto`.
  - `pub fn select::auto_toggle(l: &Layout, v: &View, signal: &str) -> Option<PlayerCommand>`, `select::auto_working(l, v, signal) -> bool`, `select::describe_auto(l, v, signal) -> String` (`Auto-working TAW1: off`).

**Deliberate test changes (reasons):** twobox's title is "Two boxes" and it has no `box_prefix`, so the game gives it the default prefix `T` and letters West `A`, East `B` (Task 5): every signal in a hover text, menu or alarm on twobox is now shown as `T<letter><name>` — `Signal TAW1 (West): red`, `Cancel route TBC to TAW2`, `Refused: set route TBD to TAW2 (…)`, and the glyph test's `Cancel route TAW1 to TAA`. Hand-made layouts in tests (`auto_layout`) have an empty prefix and no letters, so their names stay plain. D1's refusal test is renamed: the refusal is now a steady outline (owner decision 9).

- [ ] **Step 1: Write the failing tests**

Create `crates/client-core/tests/names.rs`:
```rust
//! Display names (realism spec §2, owner decision 11).

mod common;

use std::collections::BTreeMap;

use client_core::Names;
use client_core::text::{command_text, notice_text};
use common::*;
use protocol::*;

fn one_box(prefix: &str, letters: &[(&str, &str)]) -> Layout {
    let signal = |name: &str, area: &str| SignalInfo {
        name: s(name),
        area: s(area),
        segment: s("x"),
        offset_m: 0.0,
        direction: Dir::Up,
        aspects: 3,
        operable: true,
    };
    Layout {
        title: s("t"),
        you: s("ann"),
        area: Some(s("A")),
        areas: vec![s("A")],
        sections: vec![],
        segments: vec![],
        signals: vec![signal("121", "A"), signal("39,1V1", "A")],
        points: vec![],
        berths: vec![],
        platforms: vec![],
        routes: vec![],
        geometry: None,
        box_prefix: s(prefix),
        workstations: letters.iter().map(|(a, l)| (s(a), s(l))).collect::<BTreeMap<_, _>>(),
        simplifier: vec![],
    }
}

#[test]
fn signals_get_the_box_and_their_areas_workstation() {
    let t = Table::new("eve", Some("East"));
    let n = t.app.game().unwrap().names();
    assert_eq!((n.signal("C"), n.signal("A"), n.signal("W2")), (s("TBC"), s("TAA"), s("TAW2")), "twobox: T, West A, East B");
    assert_eq!(n.signal("W1"), "W1", "West's W1 is not in East's layout: shown as it is");
    assert_eq!(n.exit(&ExitName::Signal(s("W2"))), "TAW2");
    assert_eq!(n.exit(&ExitName::Node(s("E"))), "E", "nodes keep their names");
    assert_eq!((n.workstation("West"), n.workstation("East"), n.workstation("Nowhere")), (Some("A"), Some("B"), None));
}

#[test]
fn a_single_area_layout_has_no_workstation_letter() {
    let n = Names::new(&one_box("L", &[("A", "A")]));
    assert_eq!((n.signal("121"), n.signal("39,1V1")), (s("L121"), s("L39,1V1")));
    assert_eq!(n.workstation("A"), None);
}

#[test]
fn a_layout_from_an_older_server_shows_plain_names() {
    let n = Names::new(&one_box("", &[]));
    assert_eq!(n.signal("121"), "121");
    assert_eq!(Names::default().signal("121"), "121", "before any layout");
}

#[test]
fn alarms_and_commands_use_the_shown_names() {
    let t = Table::new("eve", Some("East"));
    let n = t.app.game().unwrap().names();
    let cmd = PlayerCommand::SetRoute { entrance: s("A"), exit: ExitName::Node(s("E")) };
    assert_eq!(command_text(&cmd, n), "set route TAA to E");
    assert_eq!(command_text(&PlayerCommand::CancelRoute { entrance: s("C") }, n), "cancel route from TBC");
    assert_eq!(
        command_text(&PlayerCommand::SetAutoWorking { entrance: s("D"), on: true }, n),
        "auto-working on at TBD"
    );
    assert_eq!(
        notice_text(&Notice::Spad { signal: s("C"), train: s("1E01") }, n),
        (s("SPAD: 1E01 passed TBC at danger"), true)
    );
}
```

`crates/client-core/tests/text.rs`: add `use client_core::Names;`; in `commands_and_refusals` add `let plain = Names::default();` as its second line and pass `&plain` as the new last argument of every `command_text` and `notice_text` call (the expected strings do not change: plain names stay plain).

`crates/client-core/tests/input.rs`:
- `use client_core::app::FLASH_S;` becomes `use client_core::app::REFUSED_S;`
- in `fringe_and_spectators_get_hover_only`: `assert_eq!(spec.app.describe(&sig("W1")), "Signal TAW1 (West): red", "twobox: box T, West is A");`
- in `right_click_cancels_a_route_and_swings_points`:
```rust
        [MenuItem { label: s("Cancel route TBC to TAW2"), cmd: PlayerCommand::CancelRoute { entrance: s("C") } }],
        "C is East's (B), W2 West's (A)"
    );
    assert_eq!(t.app.describe(&sig("C")), "Signal TBC: yellow; route to TAW2 set");
```
- rename `a_refused_command_flashes_its_entrance_and_raises_an_alarm` to `a_refused_command_outlines_its_entrance_and_raises_an_alarm` and make its end
```rust
    let g = t.app.game().unwrap();
    assert_eq!(g.refused(), Some("D"));
    assert_eq!(
        t.log_lines().last().unwrap(),
        &(s("Refused: set route TBD to TAW2 (conflicts with a route already set)"), true)
    );
    t.run(REFUSED_S);
    assert_eq!(t.app.game().unwrap().refused(), None);
```
- add after `automatic_routes_offer_auto_working_on_and_off`:
```rust
/// The ○A button sends exactly what the signal menu's auto-working entry
/// would, and nothing when the menu has none.
#[test]
fn the_auto_button_is_the_menus_auto_working_command() {
    let l = auto_layout();
    let mut v = empty_view();
    assert_eq!(select::auto_toggle(&l, &v, "S1"), None, "no route set from S1");
    assert_eq!(select::describe_auto(&l, &v, "S1"), "Auto-working S1: off");
    v.routes.insert(s("S1-S2"), RouteView { state: RouteState::Locked, auto_working: false });
    assert_eq!(select::auto_toggle(&l, &v, "S1"), Some(PlayerCommand::SetAutoWorking { entrance: s("S1"), on: true }));
    v.routes.insert(s("S1-S2"), RouteView { state: RouteState::Locked, auto_working: true });
    assert_eq!(select::auto_toggle(&l, &v, "S1"), Some(PlayerCommand::SetAutoWorking { entrance: s("S1"), on: false }));
    assert!(select::auto_working(&l, &v, "S1"));
    assert_eq!(select::describe_auto(&l, &v, "S1"), "Auto-working S1: on");
    let mut theirs = l.clone();
    theirs.signals[0].operable = false;
    assert_eq!(select::auto_toggle(&theirs, &v, "S1"), None, "not on the fringe or for a spectator");
}

#[test]
fn clicking_an_auto_button_never_touches_the_selection() {
    let mut t = Table::new("ann", Some("West"));
    t.app.click(&sig("W1"));
    t.app.click(&Target::Auto(s("W1")));
    assert_eq!(t.app.game().unwrap().selected(), Some("W1"));
    assert!(t.h.take_sent().is_empty(), "twobox has no automatic routes: nothing to toggle");
    assert_eq!(t.app.describe(&Target::Auto(s("W1"))), "Auto-working TAW1: off");
    assert!(t.app.menu(&Target::Auto(s("W1"))).is_empty());
}
```

`crates/client-ui/tests/screens.rs`, in `every_character_on_screen_has_a_glyph`: `assert!(shown.contains("Refused") && shown.contains("Cancel route TAW1 to TAA"), "{shown}");`

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-core`
Expected: compile errors — no `client_core::Names`, no `REFUSED_S`, no `Target::Auto`, `command_text` takes one argument.

- [ ] **Step 3: `Names`**

Create `crates/client-core/src/names.rs`:
```rust
//! Display names (realism spec §2, owner decision 11): a signal is shown as
//! `<box><workstation><name>` (`LA9`, `LB72`), without the workstation
//! letter on a single-area layout (`L9`). Display only: everything sent
//! keeps the plain name. Other names (berths, points, track, nodes) are
//! shown as they are.

use std::collections::BTreeMap;

use protocol::{ExitName, Layout};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Names {
    /// Plain signal name → displayed name, for the signals the layout lists.
    signals: BTreeMap<String, String>,
    /// Area → workstation letter; empty on a single-area layout.
    workstations: BTreeMap<String, String>,
}

impl Names {
    pub fn new(l: &Layout) -> Names {
        let single = l.areas.len() <= 1;
        let letter = |area: &str| if single { "" } else { l.workstations.get(area).map_or("", String::as_str) };
        let signals = l.signals.iter().map(|s| (s.name.clone(), format!("{}{}{}", l.box_prefix, letter(&s.area), s.name))).collect();
        let workstations = if single { BTreeMap::new() } else { l.workstations.clone() };
        Names { signals, workstations }
    }

    /// How a signal is shown; a name the layout does not list stays plain.
    pub fn signal(&self, name: &str) -> String {
        self.signals.get(name).cloned().unwrap_or_else(|| name.to_string())
    }

    /// A route's exit: a signal as `signal` shows it, a node as it is.
    pub fn exit(&self, e: &ExitName) -> String {
        match e {
            ExitName::Signal(s) => self.signal(s),
            ExitName::Node(n) => n.clone(),
        }
    }

    /// An area's workstation letter; `None` on a single-area layout or when
    /// the server sent none.
    pub fn workstation(&self, area: &str) -> Option<&str> {
        self.workstations.get(area).map(String::as_str).filter(|l| !l.is_empty())
    }
}
```
In `crates/client-core/src/lib.rs` add `pub mod names;` after `pub mod log;` and `pub use names::Names;` after `pub use input::Target;`.

- [ ] **Step 4: Text, the app and the refusal**

`crates/client-core/src/text.rs`: the module comment gains a second line ``//! Signals are named as the screen shows them (`Names`).``; add `use crate::names::Names;` after the `protocol` import; replace `command_text`'s first three arms and `notice_text`'s first three arms (and both signatures):
```rust
pub fn command_text(c: &PlayerCommand, names: &Names) -> String {
    match c {
        PlayerCommand::SetRoute { entrance, exit } => {
            format!("set route {} to {}", names.signal(entrance), names.exit(exit))
        }
        PlayerCommand::CancelRoute { entrance } => format!("cancel route from {}", names.signal(entrance)),
        PlayerCommand::SetAutoWorking { entrance, on } => {
            format!("auto-working {} at {}", if *on { "on" } else { "off" }, names.signal(entrance))
        }
```
```rust
/// A notice as one log line, and whether it is an alarm.
pub fn notice_text(n: &Notice, names: &Names) -> (String, bool) {
    match n {
        Notice::Rejected { cmd, reason } => {
            (format!("Refused: {} ({})", command_text(cmd, names), rejection_text(*reason)), true)
        }
        Notice::NotYourArea { area } => (format!("Not your area: that is in {area}"), true),
        Notice::Spad { signal, train } => (format!("SPAD: {train} passed {} at danger", names.signal(signal)), true),
```
(`exit_text` stays as it is: it is public and plain.)

`crates/client-core/src/app.rs`:
- `use crate::names::Names;` after `use crate::log::Log;`
- replace `FLASH_S` and its comment with
```rust
/// How long a refused command's entrance signal stays outlined (a steady
/// outline, not a flash: realism spec owner decision 9).
pub const REFUSED_S: f64 = 2.0;
```
- `InGame`: the field `flash` becomes `refused: Option<(String, f64)>`, plus a last field
```rust
    /// Display names for the layout held (rebuilt with every layout).
    pub(crate) names: Names,
```
  `InGame::new` builds it field by field (`refused: None`, `names: Names::default()`), and `flashing` is replaced by
```rust
    /// The entrance of a command just refused, outlined for `REFUSED_S`.
    pub fn refused(&self) -> Option<&str> {
        self.refused.as_ref().map(|(s, _)| s.as_str())
    }

    /// How this layout's signals are shown (plain names before any layout).
    pub fn names(&self) -> &Names {
        &self.names
    }
```
- in `tick`, `g.flash` becomes `g.refused` (both places); in `game_msg`, `notice_text(&Notice::Replaced)` becomes `notice_text(&Notice::Replaced, &g.names)`, `notice_text(n)` becomes `notice_text(n, &g.names)`, and `g.flash = Some((e.to_string(), self.now + FLASH_S));` becomes `g.refused = Some((e.to_string(), self.now + REFUSED_S));`
- in `game_msg`, replace `let reply = g.bot.receive(m);` and the `take_notices` line after it with
```rust
        let is_layout = matches!(m, ServerMsg::Layout(_));
        let reply = g.bot.receive(m);
        g.bot.take_notices();
        if let (true, Some(l)) = (is_layout, g.bot.layout()) {
            g.names = Names::new(l);
        }
```

- [ ] **Step 5: Menus, hover and the ○A target**

`crates/client-core/src/select.rs`: the module comment's last line becomes `//! fringe and spectators get hover text only. Signals are named as the` followed by ``//! screen shows them (`Names`).``; the imports become `use crate::names::Names;` and `use crate::text::pos_text;`. In `signal_menu`, add `let names = Names::new(l);` before `let mut items` and make the label `format!("Cancel route {} to {}", names.signal(signal), names.exit(&r.exit))`. `describe_signal` starts
```rust
pub fn describe_signal(l: &Layout, v: &View, signal: &str) -> String {
    let names = Names::new(l);
    let Some(s) = l.signals.iter().find(|s| s.name == signal) else { return format!("Signal {signal}") };
    let aspect = v.signals.get(signal).map_or("?", |a| aspect_text(*a));
    let mut out = format!("Signal {}{}: {aspect}", names.signal(signal), area_note(l, &s.area));
```
and its route line uses `names.exit(&r.exit)`. Add before `points_menu`:
```rust
/// What the ○A button beside `signal` sends when clicked: exactly the
/// signal menu's auto-working command, if it offers one.
pub fn auto_toggle(l: &Layout, v: &View, signal: &str) -> Option<PlayerCommand> {
    signal_menu(l, v, signal).into_iter().map(|m| m.cmd).find(|c| matches!(c, PlayerCommand::SetAutoWorking { .. }))
}

/// Whether an automatic route from `signal` is auto-working (○A filled).
pub fn auto_working(l: &Layout, v: &View, signal: &str) -> bool {
    l.routes.iter().any(|r| r.automatic && r.entrance == signal && v.routes.get(&r.name).is_some_and(|rv| rv.auto_working))
}

pub fn describe_auto(l: &Layout, v: &View, signal: &str) -> String {
    let state = if auto_working(l, v, signal) { "on" } else { "off" };
    format!("Auto-working {}: {state}", Names::new(l).signal(signal))
}
```
(`Names::new` per call walks the layout's signals — under a hundred on the shipped layouts, once per hover frame.)

`crates/client-core/src/input.rs`: `Target` gains a last variant
```rust
    /// The ○A auto-working button beside this signal.
    Auto(String),
```
`click`'s match gains `Target::Auto(s) => return self.toggle_auto(s),` (before the dead-click arm); add before `escape`:
```rust
    /// The ○A button: the signal menu's auto-working command, if it offers
    /// one; the selection is left as it is.
    fn toggle_auto(&mut self, signal: &str) {
        let cmd = self.game.as_ref().and_then(|g| select::auto_toggle(g.bot.layout()?, g.bot.view()?, signal));
        if let Some(cmd) = cmd {
            self.command(cmd);
        }
    }
```
`menu`'s last arm becomes `Target::Exit(_) | Target::Section(_) | Target::Auto(_) => vec![],` and `describe` gains `Target::Auto(s) => select::describe_auto(l, v, s),`.

- [ ] **Step 6: The two callers outside client-core**

`crates/client-ui/src/screens.rs`: in `diagram_ui`, the `PaintState` gets `flashing: g.refused()` (Task 8 renames the field).
`crates/server/tests/client.rs`: the refusal line is built with the app's names:
```rust
        let shown = app.game().unwrap().names().clone();
        let refused_line = format!("Refused: {}", client_core::text::command_text(&cmd, &shown));
```
(`shown`, because the loop already has a `names` of route names.)

- [ ] **Step 7: Run the tests**

Run: `scripts/cargo test -p signalbox-client-core -p signalbox-client-ui`
Expected: PASS.
Run: `scripts/cargo test -p signalbox-server --features dev-auth --test client` and `scripts/cargo build --workspace --all-targets --locked`
Expected: PASS, no warnings.

- [ ] **Step 8: Commit**

```bash
git add crates/client-core crates/client-ui/src/screens.rs crates/client-ui/tests/screens.rs crates/server/tests/client.rs
git commit -m "feat(client-core): signals shown as box+workstation+number, a steady refusal outline, the auto button target"
```

---

### Task 7: `client-core` — settings, the simplifier model and the headcode enquiry

**Files:**
- Create: `crates/client-core/src/settings.rs` (`Settings`, `AspectMode`, `SettingsStore`, `MemStore`, `SETTINGS_KEY`)
- Create: `crates/client-core/src/simplifier.rs` (`fmt_wtt`, `rows`, `lines`, `lateness`, `Enquiry`)
- Modify: `crates/client-core/src/lib.rs`, `crates/client-core/src/text.rs` (`train_state_text`), `crates/client-core/src/input.rs` (`App::headcode_at`)
- Test: `crates/client-core/tests/settings.rs`, `crates/client-core/tests/simplifier.rs` (new)

**Interfaces:**
- Consumes: Task 4's `Layout.simplifier`; D1's `View.trains`, `TrainRow`, `TrainState`, `Target`.
- Produces:
  - `pub enum client_core::AspectMode { RedGreen, Real }`; `pub struct client_core::Settings { pub aspects: AspectMode, pub enquiry: bool, pub numbers: bool }` (`Clone, Copy, Debug, PartialEq, Eq`, `Default` = red/green, enquiry off, numbers on) with `to_text(&self) -> String` and `from_text(&str) -> Settings`.
  - `pub trait client_core::SettingsStore { fn load(&self) -> Option<String>; fn save(&mut self, text: &str); }`; `pub struct client_core::MemStore` (`Clone, Default`; clones share one slot) with `new()`, `text() -> Option<String>`; `pub const client_core::settings::SETTINGS_KEY: &str = "signalbox.settings"`.
  - `client_core::simplifier::{fmt_wtt(f64) -> String, first_time(&SimplifierRow) -> Option<f64>, rows<'a>(&'a Layout, search: &str) -> Vec<&'a SimplifierRow>, Line { headcode, from, to, place, platform, arr, dep: String }, lines(&SimplifierRow) -> Vec<Line>, lateness(Option<&View>, &str) -> Option<String>, Enquiry<'a> { headcode: String, rows: Vec<&'a SimplifierRow>, train: Option<&'a TrainRow> }, enquiry<'a>(&'a Layout, Option<&'a View>, &str) -> Enquiry<'a>, Enquiry::live_text(&self) -> String}`.
  - `pub fn client_core::text::train_state_text(TrainState) -> &'static str` (`at platform`, `in area`, `approaching`, `due`).
  - `pub fn App::headcode_at(&self, target: &Target) -> Option<String>`: the headcode in a berth, for the enquiry; it sends nothing.

- [ ] **Step 1: Write the failing tests**

Create `crates/client-core/tests/settings.rs`:
```rust
//! Display settings (realism spec §4): defaults, and what a store keeps.

use client_core::{AspectMode, MemStore, Settings, SettingsStore};

#[test]
fn the_defaults_are_the_real_panel() {
    let d = Settings::default();
    assert_eq!((d.aspects, d.enquiry, d.numbers), (AspectMode::RedGreen, false, true));
}

#[test]
fn settings_survive_their_text() {
    let all = [AspectMode::RedGreen, AspectMode::Real];
    for aspects in all {
        for enquiry in [false, true] {
            for numbers in [false, true] {
                let s = Settings { aspects, enquiry, numbers };
                assert_eq!(Settings::from_text(&s.to_text()), s, "{}", s.to_text());
            }
        }
    }
    assert_eq!(Settings { aspects: AspectMode::Real, enquiry: true, numbers: false }.to_text(), "aspects=real\nenquiry=on\nnumbers=off\n");
}

#[test]
fn anything_else_in_the_store_gives_the_defaults() {
    for junk in ["", "garbage", "aspects=purple\nnumbers=maybe", "=\n==\n", "{\"aspects\": \"real\"}", "\u{0}\u{ffff}"] {
        assert_eq!(Settings::from_text(junk), Settings::default(), "{junk:?}");
    }
    let partial = Settings::from_text(" enquiry = on \nwho=knows\n");
    assert_eq!(partial, Settings { enquiry: true, ..Settings::default() }, "good lines count, the rest is ignored");
}

#[test]
fn a_mem_store_is_shared_by_its_clones() {
    let store = MemStore::new();
    let mut writer = store.clone();
    assert_eq!(store.load(), None);
    writer.save("numbers=off\n");
    assert_eq!(store.text().as_deref(), Some("numbers=off\n"));
    assert_eq!(client_core::settings::SETTINGS_KEY, "signalbox.settings");
}
```
Create `crates/client-core/tests/simplifier.rs`:
```rust
//! The simplifier and the headcode enquiry (realism spec §3).

mod common;

use client_core::Target;
use client_core::simplifier::{Line, enquiry, fmt_wtt, lateness, lines, rows};
use common::*;
use protocol::*;

fn call(place: &str, platform: Option<&str>, arr: Option<f64>, dep: Option<f64>, stops: bool) -> SimplifierCall {
    SimplifierCall { place: s(place), platform: platform.map(s), arr, dep, stops }
}

#[test]
fn times_are_hours_and_minutes_with_half_minutes() {
    assert_eq!(fmt_wtt(25_440.0), "07:04");
    assert_eq!(fmt_wtt(25_170.0), "06:59½");
    assert_eq!(fmt_wtt(25_199.9), "06:59½");
    assert_eq!(fmt_wtt(86_400.0 + 30.0), "00:00½", "wraps at midnight");
    for bad in [f64::NAN, -1.0, f64::INFINITY] {
        assert_eq!(fmt_wtt(bad), "00:00");
    }
}

#[test]
fn a_row_is_a_line_per_call_like_the_working_timetable() {
    let r = SimplifierRow {
        headcode: s("1A07"),
        origin: Some(s("BOWJ")),
        destination: Some(s("LIVST")),
        calls: vec![
            call("WSJ", Some("ML_UP"), None, Some(34_170.0), false),
            call("LIVST", Some("12"), Some(34_380.0), Some(34_410.0), true),
        ],
    };
    let line = |h: &str, f: &str, t: &str, p: &str, pf: &str, a: &str, d: &str| Line {
        headcode: s(h),
        from: s(f),
        to: s(t),
        place: s(p),
        platform: s(pf),
        arr: s(a),
        dep: s(d),
    };
    assert_eq!(
        lines(&r),
        [
            line("1A07", "BOWJ", "LIVST", "WSJ", "ML_UP", "pass", "09:29½"),
            line("", "", "", "LIVST", "12", "09:33", "09:33½"),
        ]
    );
    let bare = SimplifierRow { headcode: s("2W03"), origin: None, destination: None, calls: vec![] };
    assert_eq!(lines(&bare), [line("2W03", "", "", "", "", "", "")], "a service with no calls still shows");
}

#[test]
fn search_filters_by_headcode_and_rows_run_in_time_order() {
    let mut t = Table::new("sam", None);
    let order: Vec<&str> = rows(t.layout(), "").iter().map(|r| r.headcode.as_str()).collect();
    assert_eq!(order, ["1E01", "1N02", "2W03", "2W04"]);
    let found: Vec<&str> = rows(t.layout(), " 2w ").iter().map(|r| r.headcode.as_str()).collect();
    assert_eq!(found, ["2W03", "2W04"], "trimmed, any case");
    assert!(rows(t.layout(), "nothing like it").is_empty());
    // A server that sent them out of order is put right.
    t.run(0.1);
    let mut l = t.layout().clone();
    l.simplifier.reverse();
    let order: Vec<&str> = rows(&l, "").iter().map(|r| r.headcode.as_str()).collect();
    assert_eq!(order, ["1E01", "1N02", "2W03", "2W04"]);
}

#[test]
fn a_running_train_shows_its_lateness() {
    let mut t = Table::new("eve", Some("East"));
    assert_eq!(lateness(Some(t.view()), "1E01"), None, "due, not running");
    assert_eq!(lateness(None, "1E01"), None);
    t.run(3.0);
    assert!(t.view().trains.get("1E01").is_some_and(|r| r.state != TrainState::Due), "{:?}", t.view().trains);
    assert_eq!(lateness(Some(t.view()), "1E01").as_deref(), Some("OT"));
    let mut v = t.view().clone();
    v.trains.get_mut("1E01").unwrap().late_s = 180;
    assert_eq!(lateness(Some(&v), "1E01").as_deref(), Some("3L"));
}

#[test]
fn the_enquiry_has_the_rows_and_the_live_state_and_never_routes() {
    let mut t = Table::new("sam", None);
    let e = enquiry(t.layout(), Some(t.view()), "1E01");
    assert_eq!((e.headcode.as_str(), e.rows.len()), ("1E01", 1));
    assert_eq!(e.live_text(), "due", "a spectator lists every train due");
    assert_eq!(enquiry(t.layout(), None, "9Z99").live_text(), "not in your train list");
    t.run(3.0);
    let berth = t.view().berths.iter().find(|(_, h)| *h == "1E01").map(|(b, _)| b.clone()).expect("1E01 is described");
    assert_eq!(t.app.headcode_at(&Target::Berth(berth)).as_deref(), Some("1E01"));
    assert_eq!(t.app.headcode_at(&Target::Berth(s("BD"))), None, "an empty berth");
    assert_eq!(t.app.headcode_at(&Target::Signal(s("C"))), None);
    let e = enquiry(t.layout(), Some(t.view()), "1E01");
    assert!(e.live_text().ends_with(", OT"), "{}", e.live_text());
    assert!(t.h.take_sent().is_empty(), "looking a headcode up sends nothing");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-core --test settings --test simplifier`
Expected: compile errors — no `AspectMode`, `Settings`, `MemStore` in `client_core`, no module `simplifier`, no method `headcode_at`.

- [ ] **Step 3: Settings**

Create `crates/client-core/src/settings.rs`:
```rust
//! The player's display settings (realism spec §4): signal aspects as the
//! real panel shows them (red/green) or as the driver sees them, the
//! headcode enquiry, and signal numbers. Kept per browser through a
//! `SettingsStore` the shell provides, as a few `key=value` lines.

use std::cell::RefCell;
use std::rc::Rc;

/// The key the web shell stores the settings under.
pub const SETTINGS_KEY: &str = "signalbox.settings";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AspectMode {
    /// Red for on, green for any proceed aspect, as on a real IECC.
    RedGreen,
    /// Red, yellow, double yellow, green.
    Real,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub aspects: AspectMode,
    /// Clicking a headcode opens its enquiry window.
    pub enquiry: bool,
    /// Signal numbers beside every signal.
    pub numbers: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { aspects: AspectMode::RedGreen, enquiry: false, numbers: true }
    }
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

impl Settings {
    pub fn to_text(&self) -> String {
        let aspects = match self.aspects {
            AspectMode::RedGreen => "red_green",
            AspectMode::Real => "real",
        };
        format!("aspects={aspects}\nenquiry={}\nnumbers={}\n", on_off(self.enquiry), on_off(self.numbers))
    }

    /// Whatever was stored: unknown keys and bad values give the defaults,
    /// so nothing in the store can break the client.
    pub fn from_text(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            match (key.trim(), value.trim()) {
                ("aspects", "red_green") => s.aspects = AspectMode::RedGreen,
                ("aspects", "real") => s.aspects = AspectMode::Real,
                ("enquiry", "on") => s.enquiry = true,
                ("enquiry", "off") => s.enquiry = false,
                ("numbers", "on") => s.numbers = true,
                ("numbers", "off") => s.numbers = false,
                _ => {}
            }
        }
        s
    }
}

/// Where the settings live between visits: `localStorage` in the browser,
/// a file for the desktop client (D2).
pub trait SettingsStore {
    fn load(&self) -> Option<String>;
    fn save(&mut self, text: &str);
}

/// A store in memory; clones share it, so a test can read what was saved.
#[derive(Clone, Debug, Default)]
pub struct MemStore(Rc<RefCell<Option<String>>>);

impl MemStore {
    pub fn new() -> MemStore {
        MemStore::default()
    }

    pub fn text(&self) -> Option<String> {
        self.0.borrow().clone()
    }
}

impl SettingsStore for MemStore {
    fn load(&self) -> Option<String> {
        self.text()
    }

    fn save(&mut self, text: &str) {
        *self.0.borrow_mut() = Some(text.to_string());
    }
}
```
(`Rc<RefCell<…>>`: the client is single-threaded in the browser, and `SettingsStore` has no `Send` bound.)

- [ ] **Step 4: The simplifier and the enquiry**

Create `crates/client-core/src/simplifier.rs`:
```rust
//! The simplifier (realism spec §3): the area's timetable in running order,
//! shown like the working timetable — `HH:MM` with half minutes as `½`, a
//! platform column, `pass` for passing calls — with live lateness beside a
//! running train. And the headcode enquiry: one headcode's rows and state.

use std::cmp::Ordering;

use protocol::{Layout, SimplifierCall, SimplifierRow, TrainRow, TrainState, View};

use crate::text::train_state_text;

/// `HH:MM`, with `½` for a time 30 s or more past the minute (WTT style);
/// wraps at midnight; nonsense is `00:00`.
pub fn fmt_wtt(s: f64) -> String {
    let t = if s.is_finite() && s >= 0.0 { s.floor() as u64 % 86_400 } else { 0 };
    let half = if t % 60 >= 30 { "½" } else { "" };
    format!("{:02}:{:02}{half}", t / 3600, t / 60 % 60)
}

/// When the row's first listed call is booked (arrival, else departure).
pub fn first_time(r: &SimplifierRow) -> Option<f64> {
    r.calls.iter().find_map(|c| c.arr.or(c.dep))
}

/// Rows whose headcode contains `search` (trimmed, any case), in running
/// order: by first call (untimed last), then headcode, else as sent.
pub fn rows<'a>(l: &'a Layout, search: &str) -> Vec<&'a SimplifierRow> {
    let want = search.trim().to_ascii_uppercase();
    let mut out: Vec<&SimplifierRow> =
        l.simplifier.iter().filter(|r| r.headcode.to_ascii_uppercase().contains(&want)).collect();
    out.sort_by(|a, b| {
        match (first_time(a), first_time(b)) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
        .then_with(|| a.headcode.cmp(&b.headcode))
    });
    out
}

/// One line of the simplifier table: a row's first call carries its
/// headcode, origin and destination; later calls leave them blank.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub headcode: String,
    pub from: String,
    pub to: String,
    pub place: String,
    pub platform: String,
    pub arr: String,
    pub dep: String,
}

fn call_line(c: &SimplifierCall) -> (String, String) {
    let t = |v: Option<f64>| v.map(fmt_wtt).unwrap_or_default();
    if c.stops { (t(c.arr), t(c.dep)) } else { ("pass".to_string(), t(c.dep.or(c.arr))) }
}

pub fn lines(r: &SimplifierRow) -> Vec<Line> {
    if r.calls.is_empty() {
        return vec![Line {
            headcode: r.headcode.clone(),
            from: r.origin.clone().unwrap_or_default(),
            to: r.destination.clone().unwrap_or_default(),
            place: String::new(),
            platform: String::new(),
            arr: String::new(),
            dep: String::new(),
        }];
    }
    r.calls
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let (arr, dep) = call_line(c);
            let head = |s: &Option<String>| if i == 0 { s.clone().unwrap_or_default() } else { String::new() };
            Line {
                headcode: if i == 0 { r.headcode.clone() } else { String::new() },
                from: head(&r.origin),
                to: head(&r.destination),
                place: c.place.clone(),
                platform: c.platform.clone().unwrap_or_default(),
                arr,
                dep,
            }
        })
        .collect()
}

/// A running train's lateness, TRUST style: `OT` on time, `3L` three
/// minutes late; `None` when it is not running (not listed, or due).
pub fn lateness(v: Option<&View>, headcode: &str) -> Option<String> {
    let t = v?.trains.get(headcode)?;
    if t.state == TrainState::Due {
        return None;
    }
    Some(if t.late_s >= 60 { format!("{}L", t.late_s / 60) } else { "OT".to_string() })
}

/// What the enquiry window shows for one headcode.
#[derive(Clone, Debug, PartialEq)]
pub struct Enquiry<'a> {
    pub headcode: String,
    /// Every simplifier row with this headcode (headcodes can repeat).
    pub rows: Vec<&'a SimplifierRow>,
    pub train: Option<&'a TrainRow>,
}

pub fn enquiry<'a>(l: &'a Layout, v: Option<&'a View>, headcode: &str) -> Enquiry<'a> {
    Enquiry {
        headcode: headcode.to_string(),
        rows: l.simplifier.iter().filter(|r| r.headcode == headcode).collect(),
        train: v.and_then(|v| v.trains.get(headcode)),
    }
}

impl Enquiry<'_> {
    /// `in area, 3L`, `due`, or `not in your train list`.
    pub fn live_text(&self) -> String {
        match self.train {
            Some(t) if t.state == TrainState::Due => train_state_text(t.state).to_string(),
            Some(t) => {
                let late = if t.late_s >= 60 { format!("{}L", t.late_s / 60) } else { "OT".to_string() };
                format!("{}, {late}", train_state_text(t.state))
            }
            None => "not in your train list".to_string(),
        }
    }
}
```
In `crates/client-core/src/text.rs`, add `TrainState` to the `protocol` import and, before `proposal_text`:
```rust
/// A train-list state in words.
pub fn train_state_text(s: TrainState) -> &'static str {
    match s {
        TrainState::AtPlatform => "at platform",
        TrainState::InArea => "in area",
        TrainState::Approaching => "approaching",
        TrainState::Due => "due",
    }
}
```
In `crates/client-core/src/input.rs`, add before `pub fn describe`:
```rust
    /// The headcode shown at `target`, if it is a berth holding one: what a
    /// click opens the enquiry for (realism spec §3). Never routes.
    pub fn headcode_at(&self, target: &Target) -> Option<String> {
        let Target::Berth(b) = target else { return None };
        self.game.as_ref()?.bot.view()?.berths.get(b).cloned()
    }
```
In `crates/client-core/src/lib.rs`, add `pub mod settings;` and `pub mod simplifier;` after `pub mod select;`, and `pub use settings::{AspectMode, MemStore, Settings, SettingsStore};` after `pub use names::Names;`.

- [ ] **Step 5: Run the tests**

Run: `scripts/cargo test -p signalbox-client-core` and `scripts/cargo build --workspace --all-targets --locked`
Expected: PASS, no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/client-core
git commit -m "feat(client-core): display settings with a store interface, the simplifier model and the headcode enquiry"
```

---

### Task 8: `client-ui` — the VDU look: palette, thick track with joints, white overlaps with a tick, hollow fringe, hooked-post signals

**Files:**
- Modify: `crates/client-ui/src/scene.rs` (`TrackLine.a_meets/b_meets`, `joint_a/joint_b`; `SignalMark.base`, `SignalMark.routes`; `project`)
- Replace: `crates/client-ui/src/paint.rs` (whole file below)
- Modify: `crates/client-ui/src/hit.rs` (`signal_disc`; a signal is hit at its disc, the foot of its post or its point)
- Modify: `crates/client-ui/src/screens.rs` (a `settings` field; the new `PaintState`)
- Test: `crates/client-ui/tests/paint.rs` (rewritten), `crates/client-ui/tests/scene.rs` (one test added)

**Interfaces:**
- Consumes: Task 6's `Names`, `InGame::refused`, `InGame::names`; Task 7's `Settings`, `AspectMode`; D1's `Scene`, `Camera`, `hit_test`.
- Produces:
  - `TrackLine { .., a_meets: Vec<String>, b_meets: Vec<String> }` (sections of the other visible segments at the line's `from`/`to` node) with `joint_a(&self) -> bool`, `joint_b(&self) -> bool`.
  - `SignalMark { .., base: Pos2, routes: Vec<String> }`; `pub fn scene::project(p: Pos2, a: Pos2, b: Pos2) -> Pos2`.
  - `pub fn hit::signal_disc(cam: &Camera, screen: Rect, s: &SignalMark) -> Pos2`.
  - `paint`: the constants of decision 10 (`BG`, `TRACK_FREE`, `ROUTE`, `OVERLAP` (= `ROUTE`), `OCCUPIED`, `RED`, `YELLOW`, `GREEN`, `HEADCODE`, `AUTO`, `PLATFORM`, `LABEL`, `FRINGE`, `SELECT`, `REFUSED`, `TRACK_UNITS`, `TRACK_MIN_PX`, `TRACK_MAX_PX`, `JOINT_GAP_PX`, `FRINGE_EDGE_PX`, `TICK_EXTRA_PX`, `TICK_W`, `LAMP_R`, `POST_PX`, `HOOK_PX`, `POST_W`, `DASH_PX`, `DASH_GAP_PX`, `NUMBER_UNITS`, `NUMBER_MAX_PX`, `NUMBER_MIN_PX`, `GAP`); `track_w(scale: f32) -> f32`, `number_px(scale: f32) -> Option<f32>`, `signal_lamps(Aspect, AspectMode) -> (Color32, Option<Color32>)`, `left_of(facing: Vec2) -> Vec2`; `PaintState { view, selected, exits, refused: Option<&str>, time, aspects: AspectMode, numbers: bool, names: &Names }`. D1's `dim`, `TRACK_W`, `STUB_PX`, `FLASH`, `BERTH_EMPTY`, `AUTO_ON`, `AUTO_OFF` and `PaintState.flashing` are gone.
  - `UiApp` has a private `settings: Settings` (defaults until Task 10 adds the menu and the store).

This task leaves berths, platforms, labels and the automatic `A` in D1's shapes (with the new colours); Task 9 redraws them.

**Deliberate test changes (reasons):** `crates/client-ui/tests/paint.rs` is rewritten: every D1 assertion there pinned a D1 look this pass replaces by spec — 3 px lines of one width (now `track_w(scale)`), whole lines end to end (now joint gaps), a dimmed fringe (now hollow), an overlap in dim white (now white with a tick), a lamp at the signal's point with a stub (now a disc on a hooked post), a flashing refusal (now a steady outline). The D1 facts that still hold (points' gap and its flash while moving, exit squares, headcodes in monospace, idle signals red without a view) are kept in the new file. The `hit.rs` tests and the existing `scene.rs` tests are unchanged: a signal is still hit at its own point.

- [ ] **Step 1: Write the failing tests**

Replace `crates/client-ui/tests/paint.rs` with:
```rust
//! The drawing (realism spec §2), checked as shapes (no GPU, no fonts):
//! track, joints, overlaps, signals, numbers, and what flashes.

mod common;

use client_core::{AspectMode, Names};
use client_ui::camera::Camera;
use client_ui::hit::signal_disc;
use client_ui::paint::*;
use client_ui::scene::Scene;
use common::*;
use egui::{Align2, Color32, Pos2, Rect, Shape, pos2, vec2};
use protocol::*;

fn screen() -> Rect {
    Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0))
}

/// Line segments of exactly this colour and width.
fn lines_of(d: &Drawing, colour: Color32, width: f32) -> Vec<[Pos2; 2]> {
    d.shapes
        .iter()
        .filter_map(|s| match s {
            Shape::LineSegment { points, stroke } if stroke.color == colour && stroke.width == width => Some(*points),
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

fn close(a: Pos2, b: Pos2) -> bool {
    a.distance(b) < 1e-3
}

struct Rig {
    layout: Layout,
    sc: Scene,
    cam: Camera,
    view: View,
    names: Names,
    aspects: AspectMode,
    numbers: bool,
}

impl Rig {
    fn new(area: Option<&str>) -> Rig {
        Rig::of(layout_for(area), view_for(area))
    }

    fn of(layout: Layout, view: View) -> Rig {
        let sc = Scene::build(&layout).unwrap();
        let cam = Camera::fit(sc.all.unwrap(), screen());
        let names = Names::new(&layout);
        Rig { layout, sc, cam, view, names, aspects: AspectMode::RedGreen, numbers: true }
    }

    fn draw(&self, selected: Option<&str>, exits: &[ExitName], refused: Option<&str>, time: f64) -> Drawing {
        let st = PaintState {
            view: Some(&self.view),
            selected,
            exits,
            refused,
            time,
            aspects: self.aspects,
            numbers: self.numbers,
            names: &self.names,
        };
        draw(&self.sc, &self.cam, screen(), &st)
    }

    fn idle(&self) -> Drawing {
        self.draw(None, &[], None, 0.0)
    }

    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.cam.to_screen(screen(), pos2(x, y))
    }

    fn w(&self) -> f32 {
        track_w(self.cam.scale)
    }

    fn disc(&self, name: &str) -> Pos2 {
        signal_disc(&self.cam, screen(), self.sc.signals.iter().find(|s| s.name == name).unwrap())
    }
}

#[test]
fn the_palette_is_the_specs() {
    assert_eq!(BG, Color32::BLACK);
    assert_eq!(
        [TRACK_FREE, ROUTE, OCCUPIED, HEADCODE, AUTO, PLATFORM, LABEL],
        [
            Color32::from_rgb(0x7D, 0x7D, 0x7D),
            Color32::WHITE,
            Color32::from_rgb(0xE8, 0x14, 0x1C),
            Color32::from_rgb(0x39, 0xE0, 0xFF),
            Color32::from_rgb(0x1D, 0x4F, 0xD8),
            Color32::from_rgb(0xB8, 0x86, 0x0B),
            Color32::from_rgb(0x9A, 0x9A, 0x9A),
        ]
    );
    let v = |occupied, held| SectionView { occupied, held };
    assert_eq!(track_colour(None), TRACK_FREE);
    assert_eq!(track_colour(Some(&v(false, Held::Path))), ROUTE);
    assert_eq!(track_colour(Some(&v(false, Held::Overlap))), ROUTE, "overlaps are white like the route");
    assert_eq!(track_colour(Some(&v(true, Held::Overlap))), OCCUPIED, "occupied wins");
    assert!(blink_on(0.0) && !blink_on(0.3) && blink_on(0.5) && blink_on(-0.6) == blink_on(0.4));
}

#[test]
fn signal_colours_in_both_modes() {
    for a in [Aspect::Yellow, Aspect::DoubleYellow, Aspect::Green] {
        assert_eq!(signal_lamps(a, AspectMode::RedGreen), (GREEN, None), "any proceed aspect is green: {a:?}");
    }
    assert_eq!(signal_lamps(Aspect::Red, AspectMode::RedGreen), (RED, None));
    assert_eq!(signal_lamps(Aspect::DoubleYellow, AspectMode::Real), (YELLOW, Some(YELLOW)));
    assert_eq!(signal_lamps(Aspect::Green, AspectMode::Real), (GREEN, None));
}

#[test]
fn track_is_thick_within_limits_and_numbers_hide_when_small() {
    assert_eq!(track_w(1.0), 9.0, "about three times D1's 3 px");
    assert_eq!((track_w(0.01), track_w(100.0), track_w(f32::NAN)), (TRACK_MIN_PX, TRACK_MAX_PX, TRACK_MIN_PX));
    assert_eq!(number_px(1.0), Some(NUMBER_MAX_PX));
    assert_eq!(number_px(0.5), Some(8.0));
    assert_eq!(number_px(0.4), None, "6.4 px is below the 7 px minimum");
}

/// West: w1 (0,0)–(100,0) in TW1 and w2 (100,0)–(200,0) in TW2 meet at J0;
/// w2 meets the points' section TP at J1; W is a plain end.
#[test]
fn track_circuit_joints_are_gaps() {
    let r = Rig::new(Some("West"));
    let d = r.idle();
    let h = vec2(JOINT_GAP_PX / 2.0, 0.0);
    assert_eq!(
        lines_of(&d, TRACK_FREE, r.w()),
        [[r.at(0.0, 0.0), r.at(100.0, 0.0) - h], [r.at(100.0, 0.0) + h, r.at(200.0, 0.0) - h]],
        "a joint at J0 and at J1, none at the boundary W"
    );
    let w1 = &r.sc.tracks[0];
    assert_eq!((w1.a_meets.clone(), w1.b_meets.clone()), (Vec::<String>::new(), vec![s("TW2")]));
    assert!(!w1.joint_a() && w1.joint_b());
}

#[test]
fn routes_are_white_occupation_red_and_an_overlap_ends_in_a_tick() {
    let mut r = Rig::new(Some("West"));
    r.view.sections.insert(s("TW1"), SectionView { occupied: true, held: Held::Path });
    r.view.sections.insert(s("TW2"), SectionView { occupied: false, held: Held::Overlap });
    let d = r.idle();
    let h = vec2(JOINT_GAP_PX / 2.0, 0.0);
    assert_eq!(lines_of(&d, OCCUPIED, r.w()), [[r.at(0.0, 0.0), r.at(100.0, 0.0) - h]]);
    assert_eq!(lines_of(&d, ROUTE, r.w()), [[r.at(100.0, 0.0) + h, r.at(200.0, 0.0) - h]]);
    let end = r.at(200.0, 0.0) - h;
    let half = vec2(0.0, (r.w() + TICK_EXTRA_PX) / 2.0);
    let ticks = lines_of(&d, ROUTE, TICK_W);
    assert_eq!(ticks.len(), 1, "only the far end: TW1 before it is held");
    assert!(close(ticks[0][0], end - half) && close(ticks[0][1], end + half) || close(ticks[0][0], end + half) && close(ticks[0][1], end - half), "{ticks:?}");
    r.view.sections.insert(s("TW2"), SectionView { occupied: false, held: Held::Path });
    assert!(lines_of(&r.idle(), ROUTE, TICK_W).is_empty(), "a route's own end has no tick");
}

#[test]
fn fringe_track_is_hollow_not_dimmed() {
    let r = Rig::new(Some("West"));
    let d = r.idle();
    let edges = lines_of(&d, TRACK_FREE, FRINGE_EDGE_PX);
    assert_eq!(edges.len(), 6, "P's three legs, two edges each");
    let (c, n) = (r.at(207.5, 0.0), r.at(215.0, 0.0));
    let half = r.w() / 2.0;
    assert!(edges.iter().any(|e| close(e[0], c + vec2(0.0, half)) && close(e[1], n + vec2(0.0, half))), "{edges:?}");
    assert!(edges.iter().any(|e| close(e[0], c - vec2(0.0, half)) && close(e[1], n - vec2(0.0, half))));
    assert_eq!(lines_of(&d, TRACK_FREE, r.w()).len(), 2, "only West's own track is solid");
}

#[test]
fn points_show_the_lying_leg_whole_and_a_gap_in_the_other() {
    let mut r = Rig::new(Some("East"));
    let w = r.w();
    let (c, n, rv) = (r.at(207.5, 0.0), r.at(215.0, 0.0), r.at(215.0, 10.0));
    let legs = lines_of(&r.idle(), TRACK_FREE, w);
    assert!(legs.contains(&[c, n]), "normal lies: whole");
    assert!(legs.contains(&[c + (rv - c) * GAP, rv]), "reverse: from the gap");
    r.view.points.insert(s("P"), PointsView { position: PointsPos::Reverse, moving: true, locked: false });
    let open = lines_of(&r.draw(None, &[], None, 0.0), TRACK_FREE, w);
    assert!(open.contains(&[c, rv]) && open.contains(&[c + (n - c) * GAP, n]));
    assert!(!open.contains(&[c, c + (n - c) * GAP]), "the gap open");
    let shut = lines_of(&r.draw(None, &[], None, 0.3), TRACK_FREE, w);
    assert!(shut.contains(&[c, c + (n - c) * GAP]), "while moving, the gap flashes");
    r.view.points.get_mut("P").unwrap().moving = false;
    assert!(!lines_of(&r.draw(None, &[], None, 0.3), TRACK_FREE, w).contains(&[c, c + (n - c) * GAP]));
}

/// W1 faces right (+x): its post goes up (the left of travel, y grows
/// downwards) and hooks right. W2 faces left: down, and hooks left.
#[test]
fn a_signal_is_a_disc_on_a_hooked_post_left_of_the_line() {
    let r = Rig::new(Some("West"));
    let d = r.idle();
    let w1 = r.sc.signals.iter().find(|s| s.name == "W1").unwrap();
    assert_eq!(w1.base, pos2(100.0, 0.0), "the post starts on W1's own line, below its drawn point");
    let base = r.at(100.0, 0.0);
    let top = base + vec2(0.0, -POST_PX);
    let posts = lines_of(&d, TRACK_FREE, POST_W);
    assert!(posts.contains(&[base, top]) && posts.contains(&[top, top + vec2(HOOK_PX, 0.0)]), "{posts:?}");
    assert!(close(r.disc("W1"), top + vec2(HOOK_PX + LAMP_R, 0.0)));
    let w2_base = r.at(100.0, 0.0);
    assert!(close(r.disc("W2"), w2_base + vec2(0.0, POST_PX) - vec2(HOOK_PX + LAMP_R, 0.0)), "below, facing left");
    assert!(circles(&d).contains(&(r.disc("W1"), LAMP_R, RED, Color32::TRANSPARENT)));
}

#[test]
fn red_green_by_default_real_aspects_as_an_option() {
    let mut r = Rig::new(Some("West"));
    r.view.signals.insert(s("W1"), Aspect::DoubleYellow);
    let lit = |r: &Rig| circles(&r.idle()).into_iter().filter(|c| c.2 != Color32::TRANSPARENT && c.2 != RED).collect::<Vec<_>>();
    assert_eq!(lit(&r), [(r.disc("W1"), LAMP_R, GREEN, Color32::TRANSPARENT)]);
    r.aspects = AspectMode::Real;
    let facing = vec2(1.0, 0.0);
    assert_eq!(
        lit(&r),
        [(r.disc("W1"), LAMP_R, YELLOW, Color32::TRANSPARENT), (r.disc("W1") + facing * (LAMP_R * 2.2), LAMP_R, YELLOW, Color32::TRANSPARENT)]
    );
}

#[test]
fn the_entrance_post_is_white_while_a_route_is_set_from_it() {
    let mut r = Rig::new(Some("West"));
    r.view.routes.insert(s("W1-A"), RouteView { state: RouteState::Locked, auto_working: false });
    let d = r.idle();
    let base = r.at(100.0, 0.0);
    assert!(lines_of(&d, ROUTE, POST_W).contains(&[base, base + vec2(0.0, -POST_PX)]));
    assert!(!lines_of(&d, TRACK_FREE, POST_W).contains(&[base, base + vec2(0.0, -POST_PX)]));
}

#[test]
fn automatic_signals_have_a_dashed_post() {
    let mut l = layout_for(Some("West"));
    l.routes[0].automatic = true;
    let r = Rig::of(l, view_for(Some("West")));
    let base = r.at(100.0, 0.0);
    // W1's post goes up from `base`; W2's goes down from the same point.
    let near_w1 = |p: &[Pos2; 2]| p[0].distance(base) < POST_PX + HOOK_PX + 1.0 && p[0].y <= base.y && p[1].y <= base.y;
    let dashes: Vec<[Pos2; 2]> = lines_of(&r.idle(), TRACK_FREE, POST_W).into_iter().filter(near_w1).collect();
    assert!(dashes.len() > 2, "{dashes:?}");
    assert!(dashes.iter().all(|p| p[0].distance(p[1]) <= DASH_PX + 1e-3), "{dashes:?}");
}

#[test]
fn numbers_sit_beside_the_disc_and_hide_when_small_or_off() {
    let mut r = Rig::new(Some("West"));
    let d = r.idle();
    let t = d.texts.iter().find(|t| t.text == "TAW1").expect("W1 as the screen names it");
    assert_eq!((t.colour, t.monospace, t.anchor, t.size), (LABEL, true, Align2::CENTER_BOTTOM, NUMBER_MAX_PX));
    assert!(close(t.at, r.disc("W1") + vec2(0.0, -(LAMP_R + 2.0))), "above the disc");
    let w2 = d.texts.iter().find(|t| t.text == "TAW2").unwrap();
    assert_eq!(w2.anchor, Align2::CENTER_TOP, "W2's post goes down, its number below");
    r.numbers = false;
    assert!(r.idle().texts.iter().all(|t| !t.text.starts_with("TA")));
    r.numbers = true;
    r.cam.scale = 0.3;
    assert!(r.idle().texts.iter().all(|t| !t.text.starts_with("TA")), "4.8 px is too small to read");
}

#[test]
fn fringe_signals_and_their_numbers_are_grey() {
    let mut r = Rig::new(Some("East"));
    r.view.signals.insert(s("A"), Aspect::Green);
    let d = r.idle();
    assert!(circles(&d).contains(&(r.disc("A"), LAMP_R, FRINGE, Color32::TRANSPARENT)), "{:?}", circles(&d));
    assert_eq!(d.texts.iter().find(|t| t.text == "TAA").unwrap().colour, FRINGE);
    assert_eq!(d.texts.iter().find(|t| t.text == "TBC").unwrap().colour, LABEL);
}

/// Owner decision 9: only points moving, the selected entrance and a route
/// cancelling under approach locking flash.
#[test]
fn only_the_listed_states_flash() {
    let mut r = Rig::new(Some("West"));
    let exits = [ExitName::Signal(s("A"))];
    assert_eq!(r.draw(None, &exits, Some("W2"), 0.0), r.draw(None, &exits, Some("W2"), 0.3), "lit exits and refusals are steady");
    let ring = |d: &Drawing, colour: Color32| circles(d).iter().any(|c| c.0 == r.disc("W1") && c.3 == colour);
    assert!(ring(&r.draw(Some("W1"), &[], None, 0.0), SELECT) && !ring(&r.draw(Some("W1"), &[], None, 0.3), SELECT), "the entrance blinks");
    assert!(circles(&r.draw(None, &exits, None, 0.3)).iter().any(|c| c.0 == r.disc("A") && c.3 == SELECT));
    let refused = r.draw(None, &[], Some("W2"), 0.3);
    assert!(circles(&refused).iter().any(|c| c.0 == r.disc("W2") && c.3 == REFUSED && c.1 == LAMP_R + 6.0));
    r.view.routes.insert(s("W1-A"), RouteView { state: RouteState::Cancelling, auto_working: false });
    let on = circles(&r.draw(None, &[], None, 0.0));
    let off = circles(&r.draw(None, &[], None, 0.3));
    assert!(on.contains(&(r.disc("W1"), LAMP_R, RED, Color32::TRANSPARENT)));
    assert!(off.iter().any(|c| c.0 == r.disc("W1") && c.2 == Color32::TRANSPARENT && c.3 == RED), "unlit half: {off:?}");
}

#[test]
fn exit_markers_and_headcodes_in_the_new_colours() {
    let mut r = Rig::new(Some("West"));
    r.view.berths.insert(s("BA"), s("1E01"));
    let d = r.draw(Some("A"), &[ExitName::Node(s("E"))], None, 0.0);
    let squares: Vec<(Pos2, Color32)> = d
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Rect(rs) if rs.rect.width() == 7.0 => Some((rs.rect.center(), rs.stroke.color)),
            _ => None,
        })
        .collect();
    assert!(squares.contains(&(r.at(0.0, 0.0), TRACK_FREE)) && squares.contains(&(r.at(400.0, 60.0), FRINGE)));
    assert!(squares.contains(&(r.at(400.0, 0.0), SELECT)), "a lit exit is lit in full");
    let t = d.texts.iter().find(|t| t.text == "1E01").unwrap();
    assert_eq!((t.colour, t.monospace), (HEADCODE, true));
}

#[test]
fn no_view_yet_draws_everything_idle() {
    let r = Rig::new(Some("West"));
    let names = Names::new(&r.layout);
    let st = PaintState {
        view: None,
        selected: None,
        exits: &[],
        refused: None,
        time: 0.0,
        aspects: AspectMode::RedGreen,
        numbers: true,
        names: &names,
    };
    let d = draw(&r.sc, &r.cam, screen(), &st);
    assert_eq!(lines_of(&d, TRACK_FREE, r.w()).len(), 2);
    assert_eq!(circles(&d).iter().filter(|c| c.2 == RED).count(), 3, "signals default to red");
}

#[test]
fn a_signal_without_a_facing_is_a_bare_disc_at_its_point() {
    let mut l = layout_for(Some("West"));
    l.geometry.as_mut().unwrap().signals.iter_mut().find(|s| s.signal == "W1").unwrap().facing = None;
    let r = Rig::of(l, view_for(Some("West")));
    let d = r.idle();
    assert!(close(r.disc("W1"), r.at(100.0, -5.0)));
    assert!(circles(&d).contains(&(r.at(100.0, -5.0), LAMP_R, RED, Color32::TRANSPARENT)));
    let base = r.at(100.0, 0.0);
    assert!(!lines_of(&d, TRACK_FREE, POST_W).iter().any(|p| p[0] == base && p[1].y < base.y), "no post");
    assert!(d.texts.iter().any(|t| t.text == "TAW1" && t.anchor == Align2::CENTER_BOTTOM), "its number above it");
}
```
Append to `crates/client-ui/tests/scene.rs`:
```rust
#[test]
fn signals_stand_on_their_line_and_know_their_routes() {
    let sc = Scene::build(&layout_for(Some("West"))).unwrap();
    let a = sc.signals.iter().find(|s| s.name == "A").unwrap();
    assert_eq!((a.at, a.base), (pos2(200.0, -5.0), pos2(200.0, 0.0)), "drawn 5 above w2, its post starts on it");
    assert_eq!(a.routes, [s("A-E"), s("A-N")]);
    let mut l = layout_for(Some("West"));
    l.geometry.as_mut().unwrap().lines.retain(|g| g.segment != "w2");
    let sc = Scene::build(&l).unwrap();
    let a = sc.signals.iter().find(|s| s.name == "A").unwrap();
    assert_eq!(a.base, a.at, "no line of its own drawn: the post starts at the signal");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-ui --test paint --test scene`
Expected: compile errors — no `signal_disc`, `track_w`, `signal_lamps`, no fields `refused`/`aspects`/`numbers`/`names` on `PaintState`, no `base` on `SignalMark`.

- [ ] **Step 3: The scene**

In `crates/client-ui/src/scene.rs`, `TrackLine` gains two fields and methods:
```rust
    /// Sections of the other visible segments meeting this line at `a`
    /// (its segment's `from` node) and at `b`, sorted, without repeats.
    pub a_meets: Vec<String>,
    pub b_meets: Vec<String>,
}

impl TrackLine {
    /// A track-circuit joint at `a`: another section meets the line there.
    pub fn joint_a(&self) -> bool {
        self.a_meets.iter().any(|s| *s != self.section)
    }

    pub fn joint_b(&self) -> bool {
        self.b_meets.iter().any(|s| *s != self.section)
    }
}
```
`SignalMark` gains, after `at`:
```rust
    /// Where its post leaves the track: `at` moved onto its own segment's
    /// drawn line (TS2 signals are on it already), else `at`.
    pub base: Pos2,
    /// Every route from it you can see (its post is white while one is set).
    pub routes: Vec<String>,
```
Add above `fn grow`:
```rust
/// The point of segment a–b nearest to `p`.
pub fn project(p: Pos2, a: Pos2, b: Pos2) -> Pos2 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 == 0.0 {
        return a;
    }
    a + ab * ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
}
```
In `Scene::build`, replace the lines loop (from `let mut sc = Scene::default();` to the end of `for line in &g.lines { … }`) with:
```rust
        // Node → (segment, section) of every visible segment meeting there.
        let mut at_node: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
        for s in &l.segments {
            for n in [s.from.as_str(), s.to.as_str()] {
                at_node.entry(n).or_default().push((s.name.as_str(), s.section.as_str()));
            }
        }
        let meets = |node: &str, segment: &str| -> Vec<String> {
            let set: BTreeSet<&str> =
                at_node.get(node).into_iter().flatten().filter(|(g, _)| *g != segment).map(|(_, s)| *s).collect();
            set.into_iter().map(str::to_string).collect()
        };
        let mut sc = Scene::default();
        for line in &g.lines {
            let (Some(a), Some(b), Some(&(section, from, to))) = (pt(line.x1, line.y1), pt(line.x2, line.y2), seg_of.get(line.segment.as_str()))
            else {
                continue;
            };
            sc.tracks.push(TrackLine {
                segment: line.segment.clone(),
                section: section.to_string(),
                a,
                b,
                fringe: fringe_of.get(section).copied().unwrap_or(true),
                a_meets: meets(from, &line.segment),
                b_meets: meets(to, &line.segment),
            });
        }
        let line_of: BTreeMap<&str, (Pos2, Pos2)> = sc.tracks.iter().map(|t| (t.segment.as_str(), (t.a, t.b))).collect();
```
and in the signals loop compute the base before the `SignalMark` and fill the two new fields:
```rust
            let base = line_of.get(info.segment.as_str()).map_or(at, |&(a, b)| project(at, a, b));
            sc.signals.push(SignalMark {
                name: s.signal.clone(),
                at,
                base,
                routes: l.routes.iter().filter(|r| r.entrance == s.signal).map(|r| r.name.clone()).collect(),
```
(the rest of that literal is unchanged).

- [ ] **Step 4: The drawing**

Replace `crates/client-ui/src/paint.rs` with:
```rust
//! Drawing the diagram as an IECC workstation shows it (realism spec §2):
//! black background, thick grey track broken at every track-circuit joint,
//! white routes and overlaps, red occupation, signals as discs on hooked
//! posts. Colour only ever means state. `draw` is pure (shapes and text in
//! screen pixels, testable without a GPU or fonts); `paint` hands them to an
//! egui `Painter`.

use client_core::{AspectMode, Names};
use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2, vec2};
use protocol::{Aspect, ExitName, Held, PointsPos, RouteState, SectionView, View};

use crate::camera::Camera;
use crate::hit::{berth_rect, signal_disc};
use crate::scene::{PointsMark, Scene, SignalMark, TrackLine};

pub const BG: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
pub const TRACK_FREE: Color32 = Color32::from_rgb(0x7D, 0x7D, 0x7D);
pub const ROUTE: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
/// Overlaps are white like the route (owner decision 3), with an end tick.
pub const OVERLAP: Color32 = ROUTE;
pub const OCCUPIED: Color32 = Color32::from_rgb(0xE8, 0x14, 0x1C);
pub const RED: Color32 = Color32::from_rgb(0xE6, 0x1E, 0x1E);
pub const YELLOW: Color32 = Color32::from_rgb(0xFA, 0xD2, 0x00);
pub const GREEN: Color32 = Color32::from_rgb(0x00, 0xDC, 0x50);
pub const HEADCODE: Color32 = Color32::from_rgb(0x39, 0xE0, 0xFF);
pub const AUTO: Color32 = Color32::from_rgb(0x1D, 0x4F, 0xD8);
pub const PLATFORM: Color32 = Color32::from_rgb(0xB8, 0x86, 0x0B);
pub const LABEL: Color32 = Color32::from_rgb(0x9A, 0x9A, 0x9A);
/// Signals, numbers and headcodes of other areas: grey, not dimmed colours.
pub const FRINGE: Color32 = Color32::from_rgb(0x6E, 0x6E, 0x6E);
pub const SELECT: Color32 = Color32::from_rgb(0x00, 0xC8, 0xFF);
/// The steady outline on a refused command's signal.
pub const REFUSED: Color32 = Color32::from_rgb(0xFF, 0x3C, 0xFF);

/// Track width: this many pixels per layout unit, within the limits.
pub const TRACK_UNITS: f32 = 9.0;
pub const TRACK_MIN_PX: f32 = 4.0;
pub const TRACK_MAX_PX: f32 = 14.0;
/// The gap at a track-circuit joint (half off each touching end).
pub const JOINT_GAP_PX: f32 = 2.0;
/// Fringe track is two lines this wide at the edges of the bar.
pub const FRINGE_EDGE_PX: f32 = 1.0;
/// The end-of-overlap tick: this much longer than the track is wide.
pub const TICK_EXTRA_PX: f32 = 8.0;
pub const TICK_W: f32 = 2.0;
pub const LAMP_R: f32 = 4.0;
/// The post: out from the track to the left of travel, then hooked forward.
pub const POST_PX: f32 = 9.0;
pub const HOOK_PX: f32 = 5.0;
pub const POST_W: f32 = 1.5;
/// Automatic signals' posts are dashed.
pub const DASH_PX: f32 = 3.0;
pub const DASH_GAP_PX: f32 = 2.0;
/// Signal numbers: this many pixels per layout unit, at most the maximum,
/// and not drawn below the minimum.
pub const NUMBER_UNITS: f32 = 16.0;
pub const NUMBER_MAX_PX: f32 = 11.0;
pub const NUMBER_MIN_PX: f32 = 7.0;
/// Where the non-lying leg of points starts, as a fraction of its length.
pub const GAP: f32 = 0.5;

pub fn track_w(scale: f32) -> f32 {
    let w = TRACK_UNITS * scale;
    if w.is_finite() { w.clamp(TRACK_MIN_PX, TRACK_MAX_PX) } else { TRACK_MIN_PX }
}

/// Signal numbers' text size at this zoom; `None` when too small to read.
pub fn number_px(scale: f32) -> Option<f32> {
    let px = (NUMBER_UNITS * scale).min(NUMBER_MAX_PX);
    (px >= NUMBER_MIN_PX).then_some(px)
}

/// A section's colour: occupied red, route or overlap white, else grey.
pub fn track_colour(v: Option<&SectionView>) -> Color32 {
    match v {
        Some(s) if s.occupied => OCCUPIED,
        Some(s) if s.held != Held::Free => ROUTE,
        _ => TRACK_FREE,
    }
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

/// What the signal's lamp shows in a mode: on a real panel red for on and
/// green for any proceed aspect (owner decision 1).
pub fn signal_lamps(a: Aspect, mode: AspectMode) -> (Color32, Option<Color32>) {
    match (mode, a) {
        (AspectMode::Real, _) => lamps(a),
        (AspectMode::RedGreen, Aspect::Red) => (RED, None),
        (AspectMode::RedGreen, _) => (GREEN, None),
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
    /// The signal outlined for a refused command.
    pub refused: Option<&'a str>,
    /// Seconds, for flashing.
    pub time: f64,
    pub aspects: AspectMode,
    /// Signal numbers on.
    pub numbers: bool,
    pub names: &'a Names,
}

/// A bar from `a` to `b`: solid, or for the fringe two thin edge lines.
fn bar(out: &mut Vec<Shape>, a: Pos2, b: Pos2, w: f32, colour: Color32, hollow: bool) {
    if !hollow {
        out.push(Shape::line_segment([a, b], Stroke::new(w, colour)));
        return;
    }
    let d = b - a;
    let n = if d.length() > 0.0 { vec2(-d.y, d.x).normalized() * (w / 2.0) } else { Vec2::ZERO };
    for side in [n, -n] {
        out.push(Shape::line_segment([a + side, b + side], Stroke::new(FRINGE_EDGE_PX, colour)));
    }
}

/// `a` and `b` pulled in by half a joint gap at each end that is a joint.
fn trimmed(t: &TrackLine, a: Pos2, b: Pos2) -> (Pos2, Pos2) {
    let d = b - a;
    if d.length() <= JOINT_GAP_PX * 2.0 {
        return (a, b);
    }
    let step = d.normalized() * (JOINT_GAP_PX / 2.0);
    (if t.joint_a() { a + step } else { a }, if t.joint_b() { b - step } else { b })
}

fn points_shapes(out: &mut Vec<Shape>, p: &PointsMark, to: &dyn Fn(Pos2) -> Pos2, st: &PaintState, colour: Color32, w: f32) {
    let pv = st.view.and_then(|v| v.points.get(&p.name));
    let lying = pv.map_or(PointsPos::Normal, |v| v.position);
    let moving = pv.is_some_and(|v| v.moving);
    let (lie, other) = match lying {
        PointsPos::Normal => (p.normal, p.reverse),
        PointsPos::Reverse => (p.reverse, p.normal),
    };
    let c = to(p.at);
    if let Some(t) = p.toe {
        bar(out, c, to(t), w, colour, p.fringe);
    }
    if let Some(l) = lie {
        bar(out, c, to(l), w, colour, p.fringe);
    }
    if let Some(o) = other {
        let end = to(o);
        let gap_end = c + (end - c) * GAP;
        bar(out, gap_end, end, w, colour, p.fringe);
        // While moving the gap flashes: closed in the dark half of the blink.
        if moving && !blink_on(st.time) {
            bar(out, c, gap_end, w, colour, p.fringe);
        }
    }
}

fn track_shapes(d: &mut Drawing, t: &TrackLine, to: &dyn Fn(Pos2) -> Pos2, st: &PaintState, w: f32) {
    let section = |name: &str| st.view.and_then(|v| v.sections.get(name));
    let held = |name: &str| section(name).is_some_and(|s| s.held != Held::Free);
    let (a, b) = trimmed(t, to(t.a), to(t.b));
    bar(&mut d.shapes, a, b, w, track_colour(section(&t.section)), t.fringe);
    // End of overlap: an end of an overlap section where nothing held goes on.
    if section(&t.section).is_some_and(|s| s.held == Held::Overlap) {
        let n = if (b - a).length() > 0.0 { vec2(-(b - a).y, (b - a).x).normalized() } else { Vec2::ZERO };
        let half = n * ((w + TICK_EXTRA_PX) / 2.0);
        for (end, meets) in [(a, &t.a_meets), (b, &t.b_meets)] {
            if !meets.iter().any(|m| held(m)) {
                d.shapes.push(Shape::line_segment([end - half, end + half], Stroke::new(TICK_W, OVERLAP)));
            }
        }
    }
}

/// Unit vector to the left of travel on screen (y grows downwards).
pub fn left_of(facing: Vec2) -> Vec2 {
    vec2(facing.y, -facing.x)
}

/// Which way text beside a disc hangs, from the side it is on.
fn anchor_towards(v: Vec2) -> Align2 {
    if v.y.abs() >= v.x.abs() {
        if v.y < 0.0 { Align2::CENTER_BOTTOM } else { Align2::CENTER_TOP }
    } else if v.x < 0.0 {
        Align2::RIGHT_CENTER
    } else {
        Align2::LEFT_CENTER
    }
}

fn signal_shapes(d: &mut Drawing, s: &SignalMark, cam: &Camera, screen: Rect, st: &PaintState) {
    let aspect = st.view.and_then(|v| v.signals.get(&s.name)).copied().unwrap_or(Aspect::Red);
    let routes: Vec<RouteState> =
        s.routes.iter().filter_map(|r| st.view.and_then(|v| v.routes.get(r)).map(|rv| rv.state)).collect();
    let disc = signal_disc(cam, screen, s);
    if s.facing != Vec2::ZERO {
        let base = cam.to_screen(screen, s.base);
        let top = base + left_of(s.facing) * POST_PX;
        let hook = top + s.facing * HOOK_PX;
        let colour = if !routes.is_empty() {
            ROUTE
        } else if s.fringe {
            FRINGE
        } else {
            TRACK_FREE
        };
        let stroke = Stroke::new(POST_W, colour);
        if s.auto_routes.is_empty() {
            d.shapes.push(Shape::line_segment([base, top], stroke));
            d.shapes.push(Shape::line_segment([top, hook], stroke));
        } else {
            d.shapes.extend(Shape::dashed_line(&[base, top, hook], stroke, DASH_PX, DASH_GAP_PX));
        }
    }
    let cancelling = routes.contains(&RouteState::Cancelling);
    if s.fringe {
        d.shapes.push(Shape::circle_filled(disc, LAMP_R, FRINGE));
    } else if cancelling && !blink_on(st.time) {
        // Approach locking timing out: the lamp flashes red.
        d.shapes.push(Shape::circle_stroke(disc, LAMP_R, Stroke::new(1.0, RED)));
    } else {
        let (first, second) = signal_lamps(aspect, st.aspects);
        d.shapes.push(Shape::circle_filled(disc, LAMP_R, first));
        if let Some(c) = second {
            d.shapes.push(Shape::circle_filled(disc + s.facing * (LAMP_R * 2.2), LAMP_R, c));
        }
    }
    if st.selected == Some(s.name.as_str()) && blink_on(st.time) {
        d.shapes.push(Shape::circle_stroke(disc, LAMP_R + 3.5, Stroke::new(2.0, SELECT)));
    }
    if st.exits.contains(&ExitName::Signal(s.name.clone())) {
        d.shapes.push(Shape::circle_stroke(disc, LAMP_R + 3.5, Stroke::new(2.0, SELECT)));
    }
    if st.refused == Some(s.name.as_str()) {
        d.shapes.push(Shape::circle_stroke(disc, LAMP_R + 6.0, Stroke::new(2.0, REFUSED)));
    }
    if let (true, Some(size)) = (st.numbers, number_px(cam.scale)) {
        let side = if s.facing == Vec2::ZERO { vec2(0.0, -1.0) } else { left_of(s.facing) };
        d.texts.push(TextItem {
            at: disc + side * (LAMP_R + 2.0),
            anchor: anchor_towards(side),
            text: st.names.signal(&s.name),
            size,
            colour: if s.fringe { FRINGE } else { LABEL },
            monospace: true,
        });
    }
}

pub fn draw(scene: &Scene, cam: &Camera, screen: Rect, st: &PaintState) -> Drawing {
    let to = |p: Pos2| cam.to_screen(screen, p);
    let w = track_w(cam.scale);
    let mut d = Drawing::default();
    let section = |name: &str| st.view.and_then(|v| v.sections.get(name));
    for p in &scene.platforms {
        let r = Rect::from_two_pos(to(p.rect.min), to(p.rect.max));
        d.shapes.push(Shape::rect_filled(r, CornerRadius::ZERO, PLATFORM));
        d.texts.push(TextItem { at: r.center(), anchor: Align2::CENTER_CENTER, text: p.label.clone(), size: 9.0, colour: LABEL, monospace: false });
    }
    for t in &scene.tracks {
        track_shapes(&mut d, t, &to, st, w);
    }
    for p in &scene.points {
        points_shapes(&mut d.shapes, p, &to, st, track_colour(section(&p.section)), w);
    }
    for e in &scene.exits {
        let lit = st.exits.contains(&ExitName::Node(e.node.clone()));
        let r = Rect::from_center_size(to(e.at), vec2(7.0, 7.0));
        let colour = if lit {
            SELECT
        } else if e.fringe {
            FRINGE
        } else {
            TRACK_FREE
        };
        d.shapes.push(Shape::rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.5, colour), StrokeKind::Middle));
    }
    for s in &scene.signals {
        signal_shapes(&mut d, s, cam, screen, st);
        if !s.auto_routes.is_empty() {
            let on = st.view.is_some_and(|v| s.auto_routes.iter().any(|r| v.routes.get(r).is_some_and(|rv| rv.auto_working)));
            d.texts.push(TextItem {
                at: signal_disc(cam, screen, s) + vec2(LAMP_R + 3.0, -(LAMP_R + 3.0)),
                anchor: Align2::LEFT_BOTTOM,
                text: "A".into(),
                size: 9.0,
                colour: if s.fringe { FRINGE } else if on { ROUTE } else { AUTO },
                monospace: true,
            });
        }
    }
    for b in &scene.berths {
        let r = berth_rect(cam, screen, b.at, b.offset_px);
        let outline = if b.fringe { FRINGE } else { TRACK_FREE };
        d.shapes.push(Shape::rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, outline), StrokeKind::Middle));
        if let Some(h) = st.view.and_then(|v| v.berths.get(&b.name)) {
            d.texts.push(TextItem {
                at: r.center(),
                anchor: Align2::CENTER_CENTER,
                text: h.clone(),
                size: 11.0,
                colour: if b.fringe { FRINGE } else { HEADCODE },
                monospace: true,
            });
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

- [ ] **Step 5: Hit-testing and the screen**

In `crates/client-ui/src/hit.rs`: imports become
```rust
use crate::camera::Camera;
use crate::paint::{HOOK_PX, LAMP_R, POST_PX, left_of};
use crate::scene::{Scene, SignalMark, project};
```
add above `berth_rect`:
```rust
/// Where a signal's disc is on screen: out from its base to the left of
/// travel, then along the hook (realism spec §2); at the signal itself when
/// its facing is unknown.
pub fn signal_disc(cam: &Camera, screen: Rect, s: &SignalMark) -> Pos2 {
    if s.facing == egui::Vec2::ZERO {
        return cam.to_screen(screen, s.at);
    }
    cam.to_screen(screen, s.base) + left_of(s.facing) * POST_PX + s.facing * (HOOK_PX + LAMP_R)
}
```
`dist_to_segment`'s body becomes `p.distance(project(p, a, b))`, and the signal lookup in `hit_test` becomes
```rust
    // A signal is its disc, the foot of its post, and its own point.
    let signal_dist =
        |s: &SignalMark| signal_disc(cam, screen, s).distance(p).min(at(s.base).distance(p)).min(at(s.at).distance(p));
    if let Some(s) = nearest(scene.signals.iter().map(|s| (s, signal_dist(s)))) {
```
In `crates/client-ui/src/screens.rs`: import `Settings` (`use client_core::{App, Link, Settings, Target};`), add a last field `settings: Settings,` to `UiApp` (`settings: Settings::default(),` in `new`), and build the paint state in `diagram_ui` as
```rust
        let st = PaintState {
            view: g.view(),
            selected: g.selected(),
            exits: &exits,
            refused: g.refused(),
            time: now,
            aspects: self.settings.aspects,
            numbers: self.settings.numbers,
            names: g.names(),
        };
```

- [ ] **Step 6: Run the tests**

Run: `scripts/cargo test -p signalbox-client-ui`
Expected: PASS (paint, scene, hit, camera, screens).
Run: `scripts/cargo build --workspace --all-targets --locked`
Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/client-ui
git commit -m "feat(client-ui): IECC look for track and signals: joints, white overlaps with a tick, hollow fringe, hooked posts, numbers"
```
---

### Task 9: `client-ui` — cyan headcodes in the track, the ○A button, ochre platforms, capital labels and direction arrows

**Files:**
- Replace: `crates/client-ui/src/scene.rs`, `crates/client-ui/src/paint.rs`, `crates/client-ui/src/hit.rs` (whole files below: they build on Task 8's)
- Modify: `crates/client-ui/Cargo.toml` (dev-dependency `ts2-import`)
- Test: `crates/client-ui/tests/paint.rs`, `crates/client-ui/tests/scene.rs`, `crates/client-ui/tests/hit.rs`, `crates/client-ui/tests/screens.rs` (one point moved), `crates/client-ui/tests/layouts.rs` (new)

**Interfaces:**
- Consumes: Task 4's `LabelGeom.arrow`; Task 6's `Target::Auto`; Task 8's scene, `signal_disc`, palette.
- Produces:
  - `scene::BERTH_BACK_PX: f32 = 24.0`; `BerthMark` for a signal: `at` = the signal's `base`, `offset_px` = `−facing × 24` (facing unknown: TS2's berth position, no offset); for a boundary: `at` = its node, `offset_px` = 24 px into the track ending there (`BOUNDARY_BERTH_OFFSET_PX` when none does).
  - `PlatformMark.label` is the platform number alone; `LabelMark { text (in capitals), at, arrow: Option<Vec2> (unit) }`.
  - `pub struct scene::Run { points: Vec<Pos2>, forward: bool, backward: bool, loose_start: bool, loose_end: bool }`, `Scene.runs: Vec<Run>`; an end is loose where no other visible segment meets its node or it lies on a route exit's marker (TS2 buffer stops sit behind an undrawn 1 m spacer, so the first rule alone misses every platform road — `layouts.rs` pins that Liverpool Street's platforms get their arrows).
  - `hit::auto_button(cam: &Camera, screen: Rect, s: &SignalMark) -> Option<Pos2>`, `hit::AUTO_AHEAD_PX: f32 = 16.0`; `hit_test` returns `Target::Auto(signal)` (clickable on your own signals) within `AUTO_R + 2` px of a button, before signals.
  - `paint::{AUTO_R, HEADCODE_PX, ARROW_PX, ARROW_OFF_PX, ARROW_INSET_PX, ARROW_EVERY_PX, ARROW_MIN_RUN_PX, LABEL_PX}`, `paint::arrow(c: Pos2, dir: Vec2, colour: Color32) -> Shape` (a filled triangle whose first point is its tip), `paint::arrow_stops(total: f32, loose_start: bool, loose_end: bool) -> Vec<f32>`.

**Deliberate test changes (reasons):** `scene.rs`'s first test pinned D1's berth boxes at TS2's berth positions and the boundary berth above its exit, `["West"]` and `["EST 1", "NST 1"]`; now berths are in the track (spec §2), labels are capitals and a platform block carries its number only. `hit.rs` and `screens.rs` found BA at D1's box position (190, −15); they now find it in the track 24 px behind A.

- [ ] **Step 1: Write the failing tests**

`crates/client-ui/Cargo.toml`: add `ts2-import = { path = "../ts2-import" }` to `[dev-dependencies]` after `signalbox-game`.

`crates/client-ui/tests/scene.rs`: the import becomes `use client_ui::scene::{BERTH_BACK_PX, Run, Scene};`; in `an_area_scene_marks_its_fringe_and_what_it_can_work` replace the berths assertion and the labels assertion with
```rust
    let berths: Vec<(&str, egui::Pos2, Vec2)> = sc.berths.iter().map(|b| (b.name.as_str(), b.at, b.offset_px)).collect();
    let back = BERTH_BACK_PX;
    assert_eq!(
        berths,
        [
            ("BW1", pos2(100.0, 0.0), vec2(-back, 0.0)),
            ("BA", pos2(200.0, 0.0), vec2(-back, 0.0)),
            ("BW2", pos2(100.0, 0.0), vec2(back, 0.0)),
            ("BW", pos2(0.0, 0.0), vec2(back, 0.0)),
        ],
        "in the track behind each signal; the boundary berth inside W"
    );
```
```rust
    assert_eq!(sc.labels.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["WEST"], "labels in capitals");
```
in `the_neighbours_signals_are_fringe`:
```rust
    assert_eq!(sc.platforms.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), ["1", "1"], "the platform number");
```
and append:
```rust
/// West sees w1 and w2 joined at J0 (only those two segments meet there),
/// ending at the boundary W and at J1 (where the points' leg pa goes on).
/// W1 and A face along it, W2 against it: a double arrow.
#[test]
fn drawn_lines_chain_into_runs_with_the_directions_of_their_signals() {
    let sc = Scene::build(&layout_for(Some("West"))).unwrap();
    assert_eq!(
        sc.runs,
        [Run {
            points: vec![pos2(0.0, 0.0), pos2(100.0, 0.0), pos2(200.0, 0.0)],
            forward: true,
            backward: true,
            loose_start: true,
            loose_end: false,
        }]
    );
    let sc = Scene::build(&layout_for(None)).unwrap();
    let e = sc.runs.iter().find(|r| r.points[0] == pos2(215.0, 0.0)).expect("e, drawn from J2 to E");
    assert_eq!((e.forward, e.backward, e.loose_start, e.loose_end), (false, true, false, true), "C faces back towards P; E is an end");
    let mut l = layout_for(Some("West"));
    for s in &mut l.geometry.as_mut().unwrap().signals {
        s.facing = None;
    }
    let sc = Scene::build(&l).unwrap();
    assert!(!sc.runs[0].forward && !sc.runs[0].backward, "no facings, no arrows");
}

#[test]
fn a_line_names_arrow_is_kept_as_a_unit_vector() {
    let mut l = layout_for(Some("West"));
    let labels = &mut l.geometry.as_mut().unwrap().labels;
    labels.push(protocol::LabelGeom { text: s("down main"), x: 10.0, y: -12.0, arrow: Some([2.0, 0.0]) });
    labels.push(protocol::LabelGeom { text: s("bad"), x: 10.0, y: -30.0, arrow: Some([f64::NAN, 0.0]) });
    let sc = Scene::build(&l).unwrap();
    let down = sc.labels.iter().find(|l| l.text == "DOWN MAIN").unwrap();
    assert_eq!(down.arrow, Some(vec2(1.0, 0.0)));
    assert_eq!(sc.labels.iter().find(|l| l.text == "BAD").unwrap().arrow, None, "a nonsense arrow is dropped");
}
```

`crates/client-ui/tests/hit.rs`: the import becomes `use client_ui::hit::{AUTO_AHEAD_PX, BERTH_H, HIT_PX, Hit, auto_button, berth_rect, hit_test, signal_disc};`; in `signals_berths_exits_points_and_track` the BA line becomes
```rust
    let ba = sc.berths.iter().find(|b| b.name == "BA").unwrap();
    let ba = berth_rect(&cam, screen, ba.at, ba.offset_px).center();
    assert_eq!(hit_test(&sc, &cam, screen, ba), hit(Target::Berth(s("BA")), true), "in the track behind A");
```
and append:
```rust
#[test]
fn the_auto_button_is_its_own_target() {
    let mut l = layout_for(Some("West"));
    l.routes[0].automatic = true;
    let sc = Scene::build(&l).unwrap();
    let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0));
    let cam = Camera::fit(sc.all.unwrap(), screen);
    let w1 = sc.signals.iter().find(|s| s.name == "W1").unwrap();
    let c = auto_button(&cam, screen, w1).expect("W1 has an automatic route");
    assert_eq!(c, signal_disc(&cam, screen, w1) + vec2(AUTO_AHEAD_PX, 0.0));
    assert_eq!(hit_test(&sc, &cam, screen, c), hit(Target::Auto(s("W1")), true));
    assert_eq!(hit_test(&sc, &cam, screen, signal_disc(&cam, screen, w1)), hit(Target::Signal(s("W1")), true));
    assert_eq!(auto_button(&cam, screen, sc.signals.iter().find(|s| s.name == "A").unwrap()), None);
}
```

`crates/client-ui/tests/screens.rs`, in `right_click_opens_the_menu_for_what_is_under_the_pointer`:
```rust
    // BA is in the track 24 px behind A (its signal at x 200, facing right).
    let ba = r.at(200.0, 0.0) - vec2(client_ui::scene::BERTH_BACK_PX, 0.0);
```

Append to `crates/client-ui/tests/paint.rs`:
```rust
// ---- berths, the auto button, platforms, labels and arrows ----

fn knockouts(d: &Drawing) -> Vec<Rect> {
    d.shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Rect(rs) if rs.fill == BG && rs.rect.width() == client_ui::hit::BERTH_W => Some(rs.rect),
            _ => None,
        })
        .collect()
}

#[test]
fn headcodes_sit_in_the_track_behind_their_signal_and_empty_berths_are_not_drawn() {
    let mut r = Rig::new(Some("West"));
    assert!(knockouts(&r.idle()).is_empty(), "no headcodes, nothing drawn");
    r.view.berths.insert(s("BA"), s("2Z99"));
    let d = r.idle();
    let centre = r.at(200.0, 0.0) - vec2(client_ui::scene::BERTH_BACK_PX, 0.0);
    assert_eq!(knockouts(&d).len(), 1);
    assert!(close(knockouts(&d)[0].center(), centre), "on the track, on A's approach side");
    let t = d.texts.iter().find(|t| t.text == "2Z99").unwrap();
    assert_eq!((t.colour, t.monospace, t.size, t.at), (HEADCODE, true, HEADCODE_PX, knockouts(&d)[0].center()));
}

#[test]
fn a_boundary_berth_sits_inside_its_boundary() {
    let mut r = Rig::new(Some("West"));
    r.view.berths.insert(s("BW"), s("1E01"));
    let d = r.idle();
    assert!(close(knockouts(&d)[0].center(), r.at(0.0, 0.0) + vec2(client_ui::scene::BERTH_BACK_PX, 0.0)));
}

#[test]
fn fringe_headcodes_are_grey() {
    let mut r = Rig::new(Some("East"));
    r.view.berths.insert(s("BA"), s("1E01"));
    r.view.berths.insert(s("BC"), s("2W03"));
    let d = r.idle();
    assert_eq!(d.texts.iter().find(|t| t.text == "1E01").unwrap().colour, FRINGE, "BA is West's");
    assert_eq!(d.texts.iter().find(|t| t.text == "2W03").unwrap().colour, HEADCODE);
}

#[test]
fn automatic_signals_carry_a_blue_auto_button_hollow_off_filled_on() {
    let mut l = layout_for(Some("West"));
    l.routes[0].automatic = true;
    let mut r = Rig::of(l, view_for(Some("West")));
    let c = r.disc("W1") + vec2(client_ui::hit::AUTO_AHEAD_PX, 0.0);
    let d = r.idle();
    assert!(circles(&d).contains(&(c, AUTO_R, Color32::TRANSPARENT, AUTO)), "hollow: {:?}", circles(&d));
    let a = d.texts.iter().find(|t| t.text == "A").unwrap();
    assert_eq!((a.colour, a.anchor, a.at), (AUTO, Align2::LEFT_CENTER, c + vec2(AUTO_R + 2.0, 0.0)));
    r.view.routes.insert(s("W1-A"), RouteView { state: RouteState::Locked, auto_working: true });
    assert!(circles(&r.idle()).contains(&(c, AUTO_R, AUTO, Color32::TRANSPARENT)), "filled while auto-working");
    assert_eq!(r.idle().texts.iter().filter(|t| t.text == "A").count(), 1, "only W1 is automatic");
}

#[test]
fn platforms_are_ochre_blocks_with_their_number() {
    let r = Rig::new(Some("East"));
    let d = r.idle();
    let blocks: Vec<Rect> = d
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Rect(rs) if rs.fill == PLATFORM => Some(rs.rect),
            _ => None,
        })
        .collect();
    assert_eq!(blocks.len(), 2);
    let numbers: Vec<(&str, Color32)> = d.texts.iter().filter(|t| blocks.iter().any(|b| b.center() == t.at)).map(|t| (t.text.as_str(), t.colour)).collect();
    assert_eq!(numbers, [("1", BG), ("1", BG)], "the number in black inside");
}

#[test]
fn labels_are_grey_capitals_and_a_line_name_carries_its_arrow() {
    let mut l = layout_for(Some("West"));
    l.geometry.as_mut().unwrap().labels.push(LabelGeom { text: s("Up Main"), x: 150.0, y: -12.0, arrow: Some([-1.0, 0.0]) });
    let r = Rig::of(l, view_for(Some("West")));
    let d = r.idle();
    let west = d.texts.iter().find(|t| t.text == "WEST").unwrap();
    assert_eq!((west.colour, west.anchor), (LABEL, Align2::LEFT_TOP));
    let up = d.texts.iter().find(|t| t.text == "UP MAIN").unwrap();
    let p = r.at(150.0, -12.0);
    assert_eq!((up.anchor, up.at), (Align2::LEFT_CENTER, p + vec2(3.0, 0.0)), "text right of the arrow, which points left");
    let tip = |s: &Shape| match s {
        Shape::Path(path) if path.fill == LABEL => Some(path.points[0]),
        _ => None,
    };
    assert!(d.shapes.iter().filter_map(tip).any(|t| close(t, p - vec2(ARROW_PX, 0.0))), "the arrow starts at the point and points out");
    assert!(!d.texts.iter().any(|t| t.text.contains(['→', '←', '○', '●'])), "arrows and circles are shapes, never glyphs");
}

#[test]
fn arrows_go_at_loose_ends_and_along_long_runs() {
    assert!(arrow_stops(59.0, true, true).is_empty(), "too short to carry one");
    assert_eq!(arrow_stops(100.0, true, false), [ARROW_INSET_PX]);
    assert_eq!(arrow_stops(100.0, false, false), Vec::<f32>::new(), "a short run between points has none");
    assert_eq!(arrow_stops(1000.0, true, true), [ARROW_INSET_PX, 400.0, 1000.0 - ARROW_INSET_PX]);
    assert_eq!(arrow_stops(900.0, false, false), [400.0], "800 would crowd the end");
    assert!(arrow_stops(f32::NAN, true, true).is_empty());
}

/// West's run: a double arrow 24 px in from W, beside the bar, on the
/// right of the run's forward direction (below it: +x runs right).
#[test]
fn every_running_line_gets_direction_arrows() {
    let r = Rig::new(Some("West"));
    let d = r.idle();
    let tips: Vec<Pos2> = d
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) if p.fill == LABEL => Some(p.points[0]),
            _ => None,
        })
        .collect();
    let c = r.at(0.0, 0.0) + vec2(ARROW_INSET_PX, r.w() / 2.0 + ARROW_OFF_PX);
    let off = ARROW_PX / 2.0 + 1.0 + ARROW_PX / 2.0;
    assert!(tips.len() >= 2 && tips.len() % 2 == 0, "double arrows only: {tips:?}");
    assert!(tips.iter().any(|t| close(*t, c + vec2(off, 0.0))) && tips.iter().any(|t| close(*t, c - vec2(off, 0.0))), "{tips:?}");
    let total = r.at(200.0, 0.0).x - r.at(0.0, 0.0).x;
    assert_eq!(tips.len(), 2 * arrow_stops(total, true, false).len(), "one per stop along the {total} px run");
}
```

Create `crates/client-ui/tests/layouts.rs`:
```rust
//! The shipped layouts, drawn for every box and for a spectator with trains
//! running: nothing panics, every shape is finite, and the new marks appear.

use client_core::{AspectMode, Names};
use client_ui::camera::Camera;
use client_ui::paint::{LABEL, PaintState, draw};
use client_ui::scene::Scene;
use egui::{Rect, Shape, pos2, vec2};
use game::{Game, GameMeta};
use protocol::{ClientMsg, Proposal};
use signalbox_core::world::World;

fn world(name: &str) -> World {
    let dir = env!("CARGO_MANIFEST_DIR");
    let read = |p: String| std::fs::read_to_string(p).unwrap();
    let mut w = ts2_import::convert(&read(format!("{dir}/../ts2-import/tests/data/{name}.json"))).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&read(format!("{dir}/../../layouts/{name}.areas.json"))).unwrap()).unwrap();
    ts2_import::lines::apply(&mut w, &ts2_import::lines::parse(&read(format!("{dir}/../../layouts/{name}.lines.json"))).unwrap()).unwrap();
    World::from_file(w).unwrap()
}

fn finite(s: &Shape) -> bool {
    match s {
        Shape::LineSegment { points, .. } => points.iter().all(|p| p.is_finite()),
        Shape::Circle(c) => c.center.is_finite(),
        Shape::Rect(r) => r.rect.is_finite(),
        Shape::Path(p) => p.points.iter().all(|p| p.is_finite()),
        _ => true,
    }
}

#[test]
fn every_shipped_layout_draws_for_every_box() {
    let screen = Rect::from_min_size(pos2(0.0, 40.0), vec2(950.0, 700.0));
    for name in ["liverpool-st", "drain", "gretz-armainvilliers"] {
        let w = world(name);
        let areas: Vec<Option<String>> = std::iter::once(None).chain(w.net.areas.iter().map(|a| Some(a.name.clone()))).collect();
        let mut g = Game::new(w, GameMeta { layout: name.into(), seed: 1 });
        g.connect("sam");
        g.handle("sam", ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
        // Ten sim minutes at 8x, the robot running every box: trains about.
        for _ in 0..75 {
            g.advance(1.0);
        }
        for area in areas {
            match &area {
                Some(a) => g.handle("sam", ClientMsg::Claim { area: a.clone() }),
                None => g.handle("sam", ClientMsg::Release),
            };
            let (l, v) = (g.layout_of("sam").unwrap(), g.view_of("sam").unwrap());
            let sc = Scene::build(&l).unwrap_or_else(|| panic!("{name} {area:?}: no scene"));
            assert!(!sc.runs.is_empty(), "{name} {area:?}");
            let cam = Camera::fit(sc.fit_bounds().unwrap(), screen);
            let names = Names::new(&l);
            for (time, aspects) in [(0.0, AspectMode::RedGreen), (0.3, AspectMode::Real)] {
                let st = PaintState { view: Some(&v), selected: None, exits: &[], refused: None, time, aspects, numbers: true, names: &names };
                let d = draw(&sc, &cam, screen, &st);
                assert!(d.shapes.iter().all(finite), "{name} {area:?}");
                assert!(d.shapes.iter().any(|s| matches!(s, Shape::Path(p) if p.fill == LABEL)), "{name} {area:?}: direction arrows");
            }
            if name == "liverpool-st" && area.as_deref() == Some("Liverpool Street") {
                // Every platform road ends at a buffer stop (behind TS2's undrawn spacer).
                let loose = sc.runs.iter().filter(|r| (r.loose_start || r.loose_end) && (r.forward || r.backward)).count();
                assert!(loose >= 18, "{loose} runs with an arrowed end");
            }
            if name == "liverpool-st" && area.is_none() {
                assert_eq!(sc.labels.iter().filter(|l| l.arrow.is_some()).count(), 8, "the eight line names");
                assert!(!v.berths.is_empty(), "headcodes to draw");
            }
        }
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-ui`
Expected: compile errors — no `BERTH_BACK_PX`, `Run`, `runs`, `auto_button`, `arrow_stops`, `AUTO_R`, no field `arrow` on `LabelMark`.

- [ ] **Step 3: The scene**

Replace `crates/client-ui/src/scene.rs` with:
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
    /// Sections of the other visible segments meeting this line at `a`
    /// (its segment's `from` node) and at `b`, sorted, without repeats.
    pub a_meets: Vec<String>,
    pub b_meets: Vec<String>,
}

impl TrackLine {
    /// A track-circuit joint at `a`: another section meets the line there.
    pub fn joint_a(&self) -> bool {
        self.a_meets.iter().any(|s| *s != self.section)
    }

    pub fn joint_b(&self) -> bool {
        self.b_meets.iter().any(|s| *s != self.section)
    }
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
    /// Where its post leaves the track: `at` moved onto its own segment's
    /// drawn line (TS2 signals are on it already), else `at`.
    pub base: Pos2,
    /// Every route from it you can see (its post is white while one is set).
    pub routes: Vec<String>,
    /// Unit direction of travel past the signal, or zero when unknown.
    pub facing: Vec2,
    pub fringe: bool,
    pub operable: bool,
    /// Automatic routes starting here (an "A" is drawn, lit while one auto-works).
    pub auto_routes: Vec<String>,
    /// Ends a route you can set, so it takes the click that sets it even on the fringe.
    pub route_exit: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BerthMark {
    pub name: String,
    pub at: Pos2,
    /// Drawn this far from `at` on screen: behind its signal along the
    /// track, or inside the track from its boundary.
    pub offset_px: Vec2,
    pub fringe: bool,
    pub operable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExitMark {
    pub node: String,
    pub at: Pos2,
    /// On no section of your own (dimmed).
    pub fringe: bool,
    /// Ends a route you can set: clickable.
    pub route_exit: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlatformMark {
    pub rect: Rect,
    /// The platform number, drawn inside the block.
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LabelMark {
    /// In capitals, as IECC labels are.
    pub text: String,
    pub at: Pos2,
    /// A line name's direction of travel (unit), drawn as an arrow at `at`.
    pub arrow: Option<Vec2>,
}

/// A stretch of drawn track through plain joints (nodes where exactly two
/// visible segments meet, both drawn), for the direction arrows.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// Line ends in running order.
    pub points: Vec<Pos2>,
    /// Signals on it face along `points` / against it.
    pub forward: bool,
    pub backward: bool,
    /// Its first / last end is where your visible track stops.
    pub loose_start: bool,
    pub loose_end: bool,
}

/// Where a boundary berth is drawn when no track ends at its node.
pub const BOUNDARY_BERTH_OFFSET_PX: Vec2 = vec2(0.0, -18.0);
/// How far (pixels) behind its signal, or inside its boundary, a berth sits.
pub const BERTH_BACK_PX: f32 = 24.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub tracks: Vec<TrackLine>,
    pub points: Vec<PointsMark>,
    pub signals: Vec<SignalMark>,
    pub berths: Vec<BerthMark>,
    pub exits: Vec<ExitMark>,
    pub platforms: Vec<PlatformMark>,
    pub labels: Vec<LabelMark>,
    pub runs: Vec<Run>,
    /// Bounds of your own area's drawing (`None` for a spectator).
    pub own: Option<Rect>,
    /// Bounds of everything drawn.
    pub all: Option<Rect>,
}

/// Coordinates beyond this are nonsense and left out, so bounds, centres
/// and fits stay finite.
pub const MAX_COORD: f64 = 1.0e7;

fn pt(x: f64, y: f64) -> Option<Pos2> {
    (x.abs() <= MAX_COORD && y.abs() <= MAX_COORD).then(|| pos2(x as f32, y as f32))
}

/// The point of segment a–b nearest to `p`.
pub fn project(p: Pos2, a: Pos2, b: Pos2) -> Pos2 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 == 0.0 {
        return a;
    }
    a + ab * ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
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
        let exit_of_yours = |exit: &ExitName| l.routes.iter().any(|r| r.operable && &r.exit == exit);
        // Node → (segment, section) of every visible segment meeting there.
        let mut at_node: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
        for s in &l.segments {
            for n in [s.from.as_str(), s.to.as_str()] {
                at_node.entry(n).or_default().push((s.name.as_str(), s.section.as_str()));
            }
        }
        let meets = |node: &str, segment: &str| -> Vec<String> {
            let set: BTreeSet<&str> =
                at_node.get(node).into_iter().flatten().filter(|(g, _)| *g != segment).map(|(_, s)| *s).collect();
            set.into_iter().map(str::to_string).collect()
        };
        let mut sc = Scene::default();
        for line in &g.lines {
            let (Some(a), Some(b), Some(&(section, from, to))) = (pt(line.x1, line.y1), pt(line.x2, line.y2), seg_of.get(line.segment.as_str()))
            else {
                continue;
            };
            sc.tracks.push(TrackLine {
                segment: line.segment.clone(),
                section: section.to_string(),
                a,
                b,
                fringe: fringe_of.get(section).copied().unwrap_or(true),
                a_meets: meets(from, &line.segment),
                b_meets: meets(to, &line.segment),
            });
        }
        let line_of: BTreeMap<&str, (Pos2, Pos2)> = sc.tracks.iter().map(|t| (t.segment.as_str(), (t.a, t.b))).collect();
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
            let base = line_of.get(info.segment.as_str()).map_or(at, |&(a, b)| project(at, a, b));
            sc.signals.push(SignalMark {
                name: s.signal.clone(),
                at,
                base,
                routes: l.routes.iter().filter(|r| r.entrance == s.signal).map(|r| r.name.clone()).collect(),
                facing: facing.map_or(Vec2::ZERO, Vec2::normalized),
                fringe: other_area(&info.area),
                operable: info.operable,
                auto_routes: l.routes.iter().filter(|r| r.automatic && r.entrance == s.signal).map(|r| r.name.clone()).collect(),
                route_exit: exit_of_yours(&ExitName::Signal(s.signal.clone())),
            });
            let facing = facing.map_or(Vec2::ZERO, Vec2::normalized);
            for b in l.berths.iter().filter(|b| b.signal.as_deref() == Some(s.signal.as_str())) {
                // In the track on the approach side of its signal; where the
                // facing is unknown, at TS2's own berth position.
                let (bat, offset_px) = if facing == Vec2::ZERO {
                    (pt(s.berth_x, s.berth_y), Vec2::ZERO)
                } else {
                    (Some(base), -facing * BERTH_BACK_PX)
                };
                if let Some(bat) = bat {
                    sc.berths.push(BerthMark {
                        name: b.name.clone(),
                        at: bat,
                        offset_px,
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
                // Fringe unless one of your own sections reaches the node
                // (nothing is fringe to a spectator).
                let own = l.area.is_none() || l.segments.iter().any(|s| {
                    (s.from == *n || s.to == *n) && fringe_of.get(s.section.as_str()) == Some(&false)
                });
                sc.exits.push(ExitMark {
                    node: n.to_string(),
                    at,
                    fringe: !own,
                    route_exit: exit_of_yours(&ExitName::Node(n.to_string())),
                });
            }
        }
        // Into the track that ends at a point, if one does.
        let inward = |p: Pos2| {
            sc.tracks.iter().find_map(|t| {
                let d = if t.a.distance(p) < 0.5 {
                    t.b - t.a
                } else if t.b.distance(p) < 0.5 {
                    t.a - t.b
                } else {
                    return None;
                };
                (d.length() > 0.0).then(|| d.normalized())
            })
        };
        let mut boundary_berths = Vec::new();
        for b in &l.berths {
            if let Some(&at) = b.boundary.as_deref().and_then(|n| node_at.get(n)) {
                boundary_berths.push(BerthMark {
                    name: b.name.clone(),
                    at,
                    offset_px: inward(at).map_or(BOUNDARY_BERTH_OFFSET_PX, |d| d * BERTH_BACK_PX),
                    fringe: other_area(&b.area),
                    operable: b.operable,
                });
            }
        }
        sc.berths.extend(boundary_berths);
        for p in &g.platforms {
            if let (Some(a), Some(b)) = (pt(p.x1, p.y1), pt(p.x2, p.y2)) {
                sc.platforms.push(PlatformMark { rect: Rect::from_two_pos(a, b), label: p.platform.clone() });
            }
        }
        for t in &g.labels {
            if let Some(at) = pt(t.x, t.y) {
                let arrow = t.arrow.map(|[x, y]| vec2(x as f32, y as f32)).filter(|v| v.is_finite() && v.length() > 0.0);
                sc.labels.push(LabelMark { text: t.text.to_uppercase(), at, arrow: arrow.map(Vec2::normalized) });
            }
        }
        let signal_segment: BTreeMap<&str, &str> = l.signals.iter().map(|s| (s.name.as_str(), s.segment.as_str())).collect();
        let ends: Vec<Pos2> = sc.exits.iter().map(|e| e.at).collect();
        sc.runs = runs(&sc.tracks, &at_node, &seg_of, &sc.signals, &signal_segment, &ends);
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

/// Chain drawn lines into runs through plain joints, and give each run the
/// directions its signals face. A run's end is loose where your visible
/// track stops: no other segment meets it, or it is a route's exit (a
/// buffer stop or boundary, often behind an undrawn spacer in TS2 data).
fn runs(
    tracks: &[TrackLine],
    at_node: &BTreeMap<&str, Vec<(&str, &str)>>,
    seg_of: &BTreeMap<&str, (&str, &str, &str)>,
    signals: &[SignalMark],
    signal_segment: &BTreeMap<&str, &str>,
    exits: &[Pos2],
) -> Vec<Run> {
    let index: BTreeMap<&str, usize> = tracks.iter().enumerate().map(|(i, t)| (t.segment.as_str(), i)).collect();
    // (from, to) node of each drawn line's segment (every drawn line has one).
    let ends: Vec<(&str, &str)> = tracks.iter().map(|t| seg_of.get(t.segment.as_str()).map_or(("", ""), |e| (e.1, e.2))).collect();
    // The drawn line continuing line `i` through `node`, if the node is a plain joint.
    let next = |node: &str, i: usize| -> Option<usize> {
        let segs = at_node.get(node)?;
        if segs.len() != 2 {
            return None;
        }
        let other = segs.iter().find(|(g, _)| *g != tracks[i].segment)?;
        index.get(other.0).copied()
    };
    let loose = |node: &str| at_node.get(node).is_none_or(|s| s.len() <= 1);
    let mut seen = vec![false; tracks.len()];
    let mut out = Vec::new();
    for start in 0..tracks.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        // (line, reversed): a reversed line is walked from its `to` node.
        let mut chain = std::collections::VecDeque::from([(start, false)]);
        loop {
            let &(i, rev) = chain.back().expect("never empty");
            let exit = if rev { ends[i].0 } else { ends[i].1 };
            match next(exit, i).filter(|&k| !seen[k]) {
                Some(k) => {
                    seen[k] = true;
                    chain.push_back((k, ends[k].0 != exit));
                }
                None => break,
            }
        }
        loop {
            let &(i, rev) = chain.front().expect("never empty");
            let entry = if rev { ends[i].1 } else { ends[i].0 };
            match next(entry, i).filter(|&k| !seen[k]) {
                Some(k) => {
                    seen[k] = true;
                    chain.push_front((k, ends[k].1 != entry));
                }
                None => break,
            }
        }
        let walked = |&(i, rev): &(usize, bool)| if rev { (tracks[i].b, tracks[i].a) } else { (tracks[i].a, tracks[i].b) };
        let mut points = Vec::new();
        for step in &chain {
            let (a, b) = walked(step);
            if points.last() != Some(&a) {
                points.push(a);
            }
            points.push(b);
        }
        let (mut forward, mut backward) = (false, false);
        for s in signals.iter().filter(|s| s.facing != Vec2::ZERO) {
            let Some(step) = signal_segment.get(s.name.as_str()).and_then(|g| chain.iter().find(|(i, _)| tracks[*i].segment == *g)) else {
                continue;
            };
            let (a, b) = walked(step);
            let along = s.facing.dot(b - a);
            forward |= along > 0.0;
            backward |= along < 0.0;
        }
        let (first, last) = (chain.front().expect("never empty"), chain.back().expect("never empty"));
        let start_node = if first.1 { ends[first.0].1 } else { ends[first.0].0 };
        let end_node = if last.1 { ends[last.0].0 } else { ends[last.0].1 };
        let at_exit = |p: Option<&Pos2>| p.is_some_and(|p| exits.iter().any(|e| e.distance(*p) < 1.0));
        let loose_start = loose(start_node) || at_exit(points.first());
        let loose_end = loose(end_node) || at_exit(points.last());
        out.push(Run { points, forward, backward, loose_start, loose_end });
    }
    out
}
```

- [ ] **Step 4: Hit-testing**

Replace `crates/client-ui/src/hit.rs` with:
```rust
//! What is under the pointer: signals first, then berths, exits, points
//! and finally track, each within a fixed distance in pixels.

use client_core::Target;
use egui::{Pos2, Rect, vec2};

use crate::camera::Camera;
use crate::paint::{AUTO_R, HOOK_PX, LAMP_R, POST_PX, left_of};
use crate::scene::{Scene, SignalMark, project};

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

/// Where a signal's disc is on screen: out from its base to the left of
/// travel, then along the hook (realism spec §2); at the signal itself when
/// its facing is unknown.
pub fn signal_disc(cam: &Camera, screen: Rect, s: &SignalMark) -> Pos2 {
    if s.facing == egui::Vec2::ZERO {
        return cam.to_screen(screen, s.at);
    }
    cam.to_screen(screen, s.base) + left_of(s.facing) * POST_PX + s.facing * (HOOK_PX + LAMP_R)
}

/// Where the ○A button of an automatic signal is: `AUTO_AHEAD_PX` ahead of
/// its lamp (past a second yellow); `None` for other signals.
pub fn auto_button(cam: &Camera, screen: Rect, s: &SignalMark) -> Option<Pos2> {
    if s.auto_routes.is_empty() {
        return None;
    }
    let ahead = if s.facing == egui::Vec2::ZERO { vec2(1.0, 0.0) } else { s.facing };
    Some(signal_disc(cam, screen, s) + ahead * AUTO_AHEAD_PX)
}

/// How far ahead of its lamp an automatic signal's ○A button sits.
pub const AUTO_AHEAD_PX: f32 = 16.0;

/// The berth box on screen.
pub fn berth_rect(cam: &Camera, screen: Rect, at: Pos2, offset_px: egui::Vec2) -> Rect {
    Rect::from_center_size(cam.to_screen(screen, at) + offset_px, vec2(BERTH_W, BERTH_H))
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    p.distance(project(p, a, b))
}

fn nearest<'a, T>(items: impl Iterator<Item = (&'a T, f32)>) -> Option<&'a T>
where
    T: 'a,
{
    items.filter(|(_, d)| *d <= HIT_PX).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(t, _)| t)
}

pub fn hit_test(scene: &Scene, cam: &Camera, screen: Rect, p: Pos2) -> Option<Hit> {
    let at = |q: Pos2| cam.to_screen(screen, q);
    // The ○A buttons first: they sit just ahead of their lamps.
    let button = |s: &SignalMark| auto_button(cam, screen, s).map(|c| c.distance(p)).filter(|d| *d <= AUTO_R + 2.0);
    if let Some(s) = scene.signals.iter().filter_map(|s| Some((s, button(s)?))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(s, _)| s) {
        return Some(Hit { target: Target::Auto(s.name.clone()), clickable: s.operable });
    }
    // A signal is its disc, the foot of its post, and its own point.
    let signal_dist =
        |s: &SignalMark| signal_disc(cam, screen, s).distance(p).min(at(s.base).distance(p)).min(at(s.at).distance(p));
    if let Some(s) = nearest(scene.signals.iter().map(|s| (s, signal_dist(s)))) {
        return Some(Hit { target: Target::Signal(s.name.clone()), clickable: s.operable || s.route_exit });
    }
    if let Some(b) = scene.berths.iter().find(|b| berth_rect(cam, screen, b.at, b.offset_px).contains(p)) {
        return Some(Hit { target: Target::Berth(b.name.clone()), clickable: b.operable });
    }
    if let Some(e) = nearest(scene.exits.iter().map(|e| (e, at(e.at).distance(p)))) {
        return Some(Hit { target: Target::Exit(e.node.clone()), clickable: e.route_exit });
    }
    if let Some(pm) = nearest(scene.points.iter().map(|m| (m, at(m.at).distance(p)))) {
        return Some(Hit { target: Target::Points(pm.name.clone()), clickable: pm.operable });
    }
    nearest(scene.tracks.iter().map(|t| (t, dist_to_segment(p, at(t.a), at(t.b)))))
        .map(|t| Hit { target: Target::Section(t.section.clone()), clickable: false })
}
```

- [ ] **Step 5: The drawing**

Replace `crates/client-ui/src/paint.rs` with:
```rust
//! Drawing the diagram as an IECC workstation shows it (realism spec §2):
//! black background, thick grey track broken at every track-circuit joint,
//! white routes and overlaps, red occupation, signals as discs on hooked
//! posts, cyan headcodes in the track, blue ○A buttons, ochre platforms,
//! grey capital labels and direction arrows. Colour only ever means state.
//! `draw` is pure (shapes and text in
//! screen pixels, testable without a GPU or fonts); `paint` hands them to an
//! egui `Painter`.

use client_core::{AspectMode, Names};
use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2, vec2};
use protocol::{Aspect, ExitName, Held, PointsPos, RouteState, SectionView, View};

use crate::camera::Camera;
use crate::hit::{auto_button, berth_rect, signal_disc};
use crate::scene::{PointsMark, Run, Scene, SignalMark, TrackLine};

pub const BG: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
pub const TRACK_FREE: Color32 = Color32::from_rgb(0x7D, 0x7D, 0x7D);
pub const ROUTE: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
/// Overlaps are white like the route (owner decision 3), with an end tick.
pub const OVERLAP: Color32 = ROUTE;
pub const OCCUPIED: Color32 = Color32::from_rgb(0xE8, 0x14, 0x1C);
pub const RED: Color32 = Color32::from_rgb(0xE6, 0x1E, 0x1E);
pub const YELLOW: Color32 = Color32::from_rgb(0xFA, 0xD2, 0x00);
pub const GREEN: Color32 = Color32::from_rgb(0x00, 0xDC, 0x50);
pub const HEADCODE: Color32 = Color32::from_rgb(0x39, 0xE0, 0xFF);
pub const AUTO: Color32 = Color32::from_rgb(0x1D, 0x4F, 0xD8);
pub const PLATFORM: Color32 = Color32::from_rgb(0xB8, 0x86, 0x0B);
pub const LABEL: Color32 = Color32::from_rgb(0x9A, 0x9A, 0x9A);
/// Signals, numbers and headcodes of other areas: grey, not dimmed colours.
pub const FRINGE: Color32 = Color32::from_rgb(0x6E, 0x6E, 0x6E);
pub const SELECT: Color32 = Color32::from_rgb(0x00, 0xC8, 0xFF);
/// The steady outline on a refused command's signal.
pub const REFUSED: Color32 = Color32::from_rgb(0xFF, 0x3C, 0xFF);

/// Track width: this many pixels per layout unit, within the limits.
pub const TRACK_UNITS: f32 = 9.0;
pub const TRACK_MIN_PX: f32 = 4.0;
pub const TRACK_MAX_PX: f32 = 14.0;
/// The gap at a track-circuit joint (half off each touching end).
pub const JOINT_GAP_PX: f32 = 2.0;
/// Fringe track is two lines this wide at the edges of the bar.
pub const FRINGE_EDGE_PX: f32 = 1.0;
/// The end-of-overlap tick: this much longer than the track is wide.
pub const TICK_EXTRA_PX: f32 = 8.0;
pub const TICK_W: f32 = 2.0;
pub const LAMP_R: f32 = 4.0;
/// The post: out from the track to the left of travel, then hooked forward.
pub const POST_PX: f32 = 9.0;
pub const HOOK_PX: f32 = 5.0;
pub const POST_W: f32 = 1.5;
/// Automatic signals' posts are dashed.
pub const DASH_PX: f32 = 3.0;
pub const DASH_GAP_PX: f32 = 2.0;
/// Signal numbers: this many pixels per layout unit, at most the maximum,
/// and not drawn below the minimum.
pub const NUMBER_UNITS: f32 = 16.0;
pub const NUMBER_MAX_PX: f32 = 11.0;
pub const NUMBER_MIN_PX: f32 = 7.0;
/// Where the non-lying leg of points starts, as a fraction of its length.
pub const GAP: f32 = 0.5;
/// The ○A button's circle.
pub const AUTO_R: f32 = 4.0;
/// Headcodes: text size in pixels.
pub const HEADCODE_PX: f32 = 11.0;
/// Direction arrows: a triangle this long, this far off the bar, at loose
/// ends (inset) and every so often along runs long enough to carry one.
pub const ARROW_PX: f32 = 7.0;
pub const ARROW_OFF_PX: f32 = 6.0;
pub const ARROW_INSET_PX: f32 = 24.0;
pub const ARROW_EVERY_PX: f32 = 400.0;
pub const ARROW_MIN_RUN_PX: f32 = 60.0;
/// Labels: text size in pixels.
pub const LABEL_PX: f32 = 11.0;

pub fn track_w(scale: f32) -> f32 {
    let w = TRACK_UNITS * scale;
    if w.is_finite() { w.clamp(TRACK_MIN_PX, TRACK_MAX_PX) } else { TRACK_MIN_PX }
}

/// Signal numbers' text size at this zoom; `None` when too small to read.
pub fn number_px(scale: f32) -> Option<f32> {
    let px = (NUMBER_UNITS * scale).min(NUMBER_MAX_PX);
    (px >= NUMBER_MIN_PX).then_some(px)
}

/// A section's colour: occupied red, route or overlap white, else grey.
pub fn track_colour(v: Option<&SectionView>) -> Color32 {
    match v {
        Some(s) if s.occupied => OCCUPIED,
        Some(s) if s.held != Held::Free => ROUTE,
        _ => TRACK_FREE,
    }
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

/// What the signal's lamp shows in a mode: on a real panel red for on and
/// green for any proceed aspect (owner decision 1).
pub fn signal_lamps(a: Aspect, mode: AspectMode) -> (Color32, Option<Color32>) {
    match (mode, a) {
        (AspectMode::Real, _) => lamps(a),
        (AspectMode::RedGreen, Aspect::Red) => (RED, None),
        (AspectMode::RedGreen, _) => (GREEN, None),
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
    /// The signal outlined for a refused command.
    pub refused: Option<&'a str>,
    /// Seconds, for flashing.
    pub time: f64,
    pub aspects: AspectMode,
    /// Signal numbers on.
    pub numbers: bool,
    pub names: &'a Names,
}

/// A bar from `a` to `b`: solid, or for the fringe two thin edge lines.
fn bar(out: &mut Vec<Shape>, a: Pos2, b: Pos2, w: f32, colour: Color32, hollow: bool) {
    if !hollow {
        out.push(Shape::line_segment([a, b], Stroke::new(w, colour)));
        return;
    }
    let d = b - a;
    let n = if d.length() > 0.0 { vec2(-d.y, d.x).normalized() * (w / 2.0) } else { Vec2::ZERO };
    for side in [n, -n] {
        out.push(Shape::line_segment([a + side, b + side], Stroke::new(FRINGE_EDGE_PX, colour)));
    }
}

/// `a` and `b` pulled in by half a joint gap at each end that is a joint.
fn trimmed(t: &TrackLine, a: Pos2, b: Pos2) -> (Pos2, Pos2) {
    let d = b - a;
    if d.length() <= JOINT_GAP_PX * 2.0 {
        return (a, b);
    }
    let step = d.normalized() * (JOINT_GAP_PX / 2.0);
    (if t.joint_a() { a + step } else { a }, if t.joint_b() { b - step } else { b })
}

fn points_shapes(out: &mut Vec<Shape>, p: &PointsMark, to: &dyn Fn(Pos2) -> Pos2, st: &PaintState, colour: Color32, w: f32) {
    let pv = st.view.and_then(|v| v.points.get(&p.name));
    let lying = pv.map_or(PointsPos::Normal, |v| v.position);
    let moving = pv.is_some_and(|v| v.moving);
    let (lie, other) = match lying {
        PointsPos::Normal => (p.normal, p.reverse),
        PointsPos::Reverse => (p.reverse, p.normal),
    };
    let c = to(p.at);
    if let Some(t) = p.toe {
        bar(out, c, to(t), w, colour, p.fringe);
    }
    if let Some(l) = lie {
        bar(out, c, to(l), w, colour, p.fringe);
    }
    if let Some(o) = other {
        let end = to(o);
        let gap_end = c + (end - c) * GAP;
        bar(out, gap_end, end, w, colour, p.fringe);
        // While moving the gap flashes: closed in the dark half of the blink.
        if moving && !blink_on(st.time) {
            bar(out, c, gap_end, w, colour, p.fringe);
        }
    }
}

fn track_shapes(d: &mut Drawing, t: &TrackLine, to: &dyn Fn(Pos2) -> Pos2, st: &PaintState, w: f32) {
    let section = |name: &str| st.view.and_then(|v| v.sections.get(name));
    let held = |name: &str| section(name).is_some_and(|s| s.held != Held::Free);
    let (a, b) = trimmed(t, to(t.a), to(t.b));
    bar(&mut d.shapes, a, b, w, track_colour(section(&t.section)), t.fringe);
    // End of overlap: an end of an overlap section where nothing held goes on.
    if section(&t.section).is_some_and(|s| s.held == Held::Overlap) {
        let n = if (b - a).length() > 0.0 { vec2(-(b - a).y, (b - a).x).normalized() } else { Vec2::ZERO };
        let half = n * ((w + TICK_EXTRA_PX) / 2.0);
        for (end, meets) in [(a, &t.a_meets), (b, &t.b_meets)] {
            if !meets.iter().any(|m| held(m)) {
                d.shapes.push(Shape::line_segment([end - half, end + half], Stroke::new(TICK_W, OVERLAP)));
            }
        }
    }
}

/// Unit vector to the left of travel on screen (y grows downwards).
pub fn left_of(facing: Vec2) -> Vec2 {
    vec2(facing.y, -facing.x)
}

/// A filled triangle `ARROW_PX` long centred on `c`, pointing along `dir`.
pub fn arrow(c: Pos2, dir: Vec2, colour: Color32) -> Shape {
    let half = ARROW_PX / 2.0;
    let side = vec2(-dir.y, dir.x) * half;
    Shape::convex_polygon(vec![c + dir * half, c - dir * half + side, c - dir * half - side], colour, Stroke::NONE)
}

/// The point `dist` pixels along a polyline, and the direction there.
fn along(points: &[Pos2], dist: f32) -> Option<(Pos2, Vec2)> {
    let mut left = dist;
    for w in points.windows(2) {
        let d = w[1] - w[0];
        let len = d.length();
        if len > 0.0 && left <= len {
            return Some((w[0] + d * (left / len), d / len));
        }
        left -= len;
    }
    None
}

/// Where a run's arrows go (distances along it on screen): inset from each
/// loose end, and every `ARROW_EVERY_PX`, not crowding the end ones.
pub fn arrow_stops(total: f32, loose_start: bool, loose_end: bool) -> Vec<f32> {
    if total < ARROW_MIN_RUN_PX || !total.is_finite() {
        return vec![];
    }
    let mut ends = Vec::new();
    if loose_start {
        ends.push(ARROW_INSET_PX);
    }
    if loose_end {
        ends.push(total - ARROW_INSET_PX);
    }
    let mut stops = ends.clone();
    let mut d = ARROW_EVERY_PX;
    while d <= total - ARROW_EVERY_PX / 2.0 {
        if ends.iter().all(|e| (e - d).abs() >= ARROW_EVERY_PX / 2.0) {
            stops.push(d);
        }
        d += ARROW_EVERY_PX;
    }
    stops.sort_by(f32::total_cmp);
    stops
}

fn run_arrows(out: &mut Vec<Shape>, run: &Run, to: &dyn Fn(Pos2) -> Pos2, w: f32) {
    if !run.forward && !run.backward {
        return;
    }
    let pts: Vec<Pos2> = run.points.iter().map(|&p| to(p)).collect();
    let total: f32 = pts.windows(2).map(|p| p[0].distance(p[1])).sum();
    for stop in arrow_stops(total, run.loose_start, run.loose_end) {
        let Some((p, dir)) = along(&pts, stop) else { continue };
        // Beside the bar, on the right of the run's forward direction.
        let c = p + vec2(-dir.y, dir.x) * (w / 2.0 + ARROW_OFF_PX);
        match (run.forward, run.backward) {
            (true, true) => {
                out.push(arrow(c + dir * (ARROW_PX / 2.0 + 1.0), dir, LABEL));
                out.push(arrow(c - dir * (ARROW_PX / 2.0 + 1.0), -dir, LABEL));
            }
            (true, false) => out.push(arrow(c, dir, LABEL)),
            _ => out.push(arrow(c, -dir, LABEL)),
        }
    }
}

/// Which way text beside a disc hangs, from the side it is on.
fn anchor_towards(v: Vec2) -> Align2 {
    if v.y.abs() >= v.x.abs() {
        if v.y < 0.0 { Align2::CENTER_BOTTOM } else { Align2::CENTER_TOP }
    } else if v.x < 0.0 {
        Align2::RIGHT_CENTER
    } else {
        Align2::LEFT_CENTER
    }
}

fn signal_shapes(d: &mut Drawing, s: &SignalMark, cam: &Camera, screen: Rect, st: &PaintState) {
    let aspect = st.view.and_then(|v| v.signals.get(&s.name)).copied().unwrap_or(Aspect::Red);
    let routes: Vec<RouteState> =
        s.routes.iter().filter_map(|r| st.view.and_then(|v| v.routes.get(r)).map(|rv| rv.state)).collect();
    let disc = signal_disc(cam, screen, s);
    if s.facing != Vec2::ZERO {
        let base = cam.to_screen(screen, s.base);
        let top = base + left_of(s.facing) * POST_PX;
        let hook = top + s.facing * HOOK_PX;
        let colour = if !routes.is_empty() {
            ROUTE
        } else if s.fringe {
            FRINGE
        } else {
            TRACK_FREE
        };
        let stroke = Stroke::new(POST_W, colour);
        if s.auto_routes.is_empty() {
            d.shapes.push(Shape::line_segment([base, top], stroke));
            d.shapes.push(Shape::line_segment([top, hook], stroke));
        } else {
            d.shapes.extend(Shape::dashed_line(&[base, top, hook], stroke, DASH_PX, DASH_GAP_PX));
        }
    }
    let cancelling = routes.contains(&RouteState::Cancelling);
    if s.fringe {
        d.shapes.push(Shape::circle_filled(disc, LAMP_R, FRINGE));
    } else if cancelling && !blink_on(st.time) {
        // Approach locking timing out: the lamp flashes red.
        d.shapes.push(Shape::circle_stroke(disc, LAMP_R, Stroke::new(1.0, RED)));
    } else {
        let (first, second) = signal_lamps(aspect, st.aspects);
        d.shapes.push(Shape::circle_filled(disc, LAMP_R, first));
        if let Some(c) = second {
            d.shapes.push(Shape::circle_filled(disc + s.facing * (LAMP_R * 2.2), LAMP_R, c));
        }
    }
    if st.selected == Some(s.name.as_str()) && blink_on(st.time) {
        d.shapes.push(Shape::circle_stroke(disc, LAMP_R + 3.5, Stroke::new(2.0, SELECT)));
    }
    if st.exits.contains(&ExitName::Signal(s.name.clone())) {
        d.shapes.push(Shape::circle_stroke(disc, LAMP_R + 3.5, Stroke::new(2.0, SELECT)));
    }
    if st.refused == Some(s.name.as_str()) {
        d.shapes.push(Shape::circle_stroke(disc, LAMP_R + 6.0, Stroke::new(2.0, REFUSED)));
    }
    if let (true, Some(size)) = (st.numbers, number_px(cam.scale)) {
        let side = if s.facing == Vec2::ZERO { vec2(0.0, -1.0) } else { left_of(s.facing) };
        d.texts.push(TextItem {
            at: disc + side * (LAMP_R + 2.0),
            anchor: anchor_towards(side),
            text: st.names.signal(&s.name),
            size,
            colour: if s.fringe { FRINGE } else { LABEL },
            monospace: true,
        });
    }
}

pub fn draw(scene: &Scene, cam: &Camera, screen: Rect, st: &PaintState) -> Drawing {
    let to = |p: Pos2| cam.to_screen(screen, p);
    let w = track_w(cam.scale);
    let mut d = Drawing::default();
    let section = |name: &str| st.view.and_then(|v| v.sections.get(name));
    for p in &scene.platforms {
        let r = Rect::from_two_pos(to(p.rect.min), to(p.rect.max));
        d.shapes.push(Shape::rect_filled(r, CornerRadius::ZERO, PLATFORM));
        d.texts.push(TextItem { at: r.center(), anchor: Align2::CENTER_CENTER, text: p.label.clone(), size: 9.0, colour: BG, monospace: false });
    }
    for t in &scene.tracks {
        track_shapes(&mut d, t, &to, st, w);
    }
    for p in &scene.points {
        points_shapes(&mut d.shapes, p, &to, st, track_colour(section(&p.section)), w);
    }
    for r in &scene.runs {
        run_arrows(&mut d.shapes, r, &to, w);
    }
    for e in &scene.exits {
        let lit = st.exits.contains(&ExitName::Node(e.node.clone()));
        let r = Rect::from_center_size(to(e.at), vec2(7.0, 7.0));
        let colour = if lit {
            SELECT
        } else if e.fringe {
            FRINGE
        } else {
            TRACK_FREE
        };
        d.shapes.push(Shape::rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.5, colour), StrokeKind::Middle));
    }
    for s in &scene.signals {
        signal_shapes(&mut d, s, cam, screen, st);
        // The ○A button: hollow while nothing auto-works, filled while it does.
        if let Some(c) = auto_button(cam, screen, s) {
            let on = st.view.is_some_and(|v| s.auto_routes.iter().any(|r| v.routes.get(r).is_some_and(|rv| rv.auto_working)));
            let colour = if s.fringe { FRINGE } else { AUTO };
            d.shapes.push(if on {
                Shape::circle_filled(c, AUTO_R, colour)
            } else {
                Shape::circle_stroke(c, AUTO_R, Stroke::new(1.5, colour))
            });
            let ahead = if s.facing == Vec2::ZERO { vec2(1.0, 0.0) } else { s.facing };
            d.texts.push(TextItem {
                at: c + vec2(if ahead.x < 0.0 { -(AUTO_R + 2.0) } else { AUTO_R + 2.0 }, 0.0),
                anchor: if ahead.x < 0.0 { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER },
                text: "A".into(),
                size: 9.0,
                colour,
                monospace: true,
            });
        }
    }
    // Headcodes in the track, on a black knock-out; an empty berth is not drawn.
    for b in &scene.berths {
        let Some(h) = st.view.and_then(|v| v.berths.get(&b.name)) else { continue };
        let r = berth_rect(cam, screen, b.at, b.offset_px);
        d.shapes.push(Shape::rect_filled(r, CornerRadius::ZERO, BG));
        d.texts.push(TextItem {
            at: r.center(),
            anchor: Align2::CENTER_CENTER,
            text: h.clone(),
            size: HEADCODE_PX,
            colour: if b.fringe { FRINGE } else { HEADCODE },
            monospace: true,
        });
    }
    for l in &scene.labels {
        let at = to(l.at);
        match l.arrow {
            None => d.texts.push(TextItem { at, anchor: Align2::LEFT_TOP, text: l.text.clone(), size: LABEL_PX, colour: LABEL, monospace: false }),
            // A line name: the arrow at the point, pointing out; the text on the other side.
            Some(dir) => {
                d.shapes.push(arrow(at + dir * (ARROW_PX / 2.0), dir, LABEL));
                let (anchor, gap) = if dir.x < 0.0 { (Align2::LEFT_CENTER, 3.0) } else { (Align2::RIGHT_CENTER, -3.0) };
                d.texts.push(TextItem { at: at + vec2(gap, 0.0), anchor, text: l.text.clone(), size: LABEL_PX, colour: LABEL, monospace: false });
            }
        }
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

- [ ] **Step 6: Run the tests**

Run: `scripts/cargo test -p signalbox-client-ui`
Expected: PASS — `layouts` draws all three shipped layouts for every box and a spectator with trains running (about 2 s).
Run: `scripts/cargo build --workspace --all-targets --locked` and `git diff Cargo.lock`
Expected: no warnings; the lock gains exactly one line, `"ts2-import",` in `signalbox-client-ui`'s dependencies.

- [ ] **Step 7: Commit**

```bash
git add crates/client-ui Cargo.lock
git commit -m "feat(client-ui): in-track cyan headcodes, the auto button, ochre platforms, capital labels, direction arrows"
```

---

### Task 10: `client-ui` — the workstation in the top bar, the Settings menu, the simplifier tab and the enquiry window

**Files:**
- Replace: `crates/client-ui/src/screens.rs` (whole file below; it keeps Tasks 1, 3, 6 and 8's changes)
- Modify: `crates/client-ui/tests/common/mod.rs` (`drawn_twobox_with`)
- Test: `crates/client-ui/tests/screens.rs`

**Interfaces:**
- Consumes: Task 6's `Names::workstation`; Task 7's `Settings`, `AspectMode`, `SettingsStore`, `MemStore`, `simplifier::{rows, lines, lateness, enquiry, Line}`, `train_state_text`, `App::headcode_at`.
- Produces:
  - `pub fn UiApp::with_store(core: App, store: Box<dyn SettingsStore>) -> UiApp` (reads the stored settings; every change is saved back at once), `pub fn UiApp::settings(&self) -> Settings`, `pub fn UiApp::enquiry(&self) -> Option<&str>`, `pub enum client_ui::screens::SideTab { Trains, Simplifier }`.
  - Top bar: `<game> · Workstation <letter> · <area> (<you>)` (no `Workstation` part on a single-area layout), `<game> · spectating (<you>)`; a `Settings` menu (`Red/green (panel)` / `Real aspects`, `Headcode enquiry`, `Signal numbers`).
  - Side panel: `TRAINS` | `SIMPLIFIER` tabs above, `ALARMS` always below. Simplifier columns `Train Late From To At Plat Arr Dep`, a `headcode` search box, rows drawn with `ScrollArea::show_rows`, `No booked trains here` / `No headcode matches`.
  - Enquiry: with the setting on, a left click on a berth holding a headcode, or on a headcode in the train list, opens the window `Train <headcode>` (its live state, then per simplifier row `<origin> to <destination>` and `<place> <platform> <arr> <dep>` per call, or `Not in the simplifier for this area`); it closes with its × or when the setting goes off. Nothing else happens on such a click.

**Deliberate test changes (reasons):** the top-bar expectations `g-test · West (ann)` (two tests) become `g-test · Workstation A · West (ann)` (owner decision 11: the top bar names the workstation).

- [ ] **Step 1: Write the failing tests**

`crates/client-ui/tests/common/mod.rs`: replace `drawn_twobox` with
```rust
/// twobox with `twobox-layout.json` as its drawing.
pub fn drawn_twobox() -> World {
    drawn_twobox_with(|_| {})
}

/// `drawn_twobox`, its world JSON changed by `f` first.
pub fn drawn_twobox_with(f: impl FnOnce(&mut Value)) -> World {
    let mut w: Value = serde_json::from_str(&std::fs::read_to_string(format!("{FIXTURES}/twobox.json")).unwrap()).unwrap();
    w["layout"] = serde_json::from_str(&std::fs::read_to_string(format!("{FIXTURES}/twobox-layout.json")).unwrap()).unwrap();
    f(&mut w);
    World::from_json(&w.to_string()).unwrap()
}
```

`crates/client-ui/tests/screens.rs`:
- the import becomes `use client_core::{App, AspectMode, MemHandle, MemStore, MemTransport};`
- `Rig::lobby` and `Rig::in_game` gain store-taking twins:
```rust
    /// Open, in the lobby, the front's lists delivered.
    fn lobby(world: signalbox_core::world::World) -> Rig {
        Rig::lobby_with(world, None)
    }

    /// `lobby`, the settings kept in `store`.
    fn lobby_with(world: signalbox_core::world::World, store: Option<MemStore>) -> Rig {
        let (tr, h) = MemTransport::new();
        let core = App::new(Box::new(tr), 0.0);
        h.open();
        let game = Game::new(world, GameMeta { layout: s("twobox"), seed: 1 });
        let ui = match store {
            Some(st) => UiApp::with_store(core, Box::new(st)),
            None => UiApp::new(core),
        };
        let mut r = Rig { ctx: egui::Context::default(), ui, h, game, t: 0.0, events: vec![], lobby_sent: vec![] };
```
  (the rest of the old `lobby` body follows unchanged), and
```rust
    /// In the game as "ann", holding `area`.
    fn in_game(world: signalbox_core::world::World, area: Option<&str>) -> Rig {
        Rig::in_game_with(world, area, None)
    }

    fn in_game_with(world: signalbox_core::world::World, area: Option<&str>, store: Option<MemStore>) -> Rig {
        let mut r = Rig::lobby_with(world, store);
```
  (the rest of the old `in_game` body follows unchanged)
- in `the_game_screen_shows_bar_trains_alarms_and_the_fitted_diagram` and `a_lost_connection_shows_the_banner`, `"g-test · West (ann)"` becomes `"g-test · Workstation A · West (ann)"`
- in `every_character_on_screen_has_a_glyph`, right after the `let mut shown: String = …` line:
```rust
    // The realism pass: the settings menu, the simplifier (with a half
    // minute) and the enquiry window.
    let half = drawn_twobox_with(|w| w["services"][0]["calls"][0]["arr"] = serde_json::json!("07:04:30"));
    let store = MemStore::new();
    let mut st = store.clone();
    client_core::SettingsStore::save(&mut st, "enquiry=on");
    let mut r2 = Rig::in_game_with(half, Some("East"), Some(store));
    let out = r2.frame();
    click_text(&mut r2, &out, "Settings");
    shown.extend(texts(&r2.frame()).into_iter().map(|(t, _)| t));
    r2.events.push(Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::default() });
    r2.frame();
    let out = r2.frame();
    let right = r2.ui.diagram_rect().unwrap().max.x;
    let at = texts(&out).into_iter().find(|(t, at)| t == "1E01" && at.min.x >= right).unwrap().1.center();
    r2.click(at, PointerButton::Primary);
    let out = r2.frame();
    click_text(&mut r2, &out, "SIMPLIFIER");
    let out = r2.frame();
    shown.extend(texts(&out).into_iter().map(|(t, _)| t));
    assert!(shown.contains("07:04½") && shown.contains("Train 1E01") && shown.contains("Real aspects"), "{shown}");
    assert!(!shown.contains(['→', '←', '○', '●']), "arrows and the auto button are shapes: {shown}");
```
- append:
```rust
// ---- the realism pass: workstation, settings, simplifier, enquiry ----

/// Text drawn right of the diagram: the side panel.
fn side_texts(r: &Rig, out: &FullOutput) -> Vec<String> {
    let right = r.ui.diagram_rect().unwrap().max.x;
    texts(out).into_iter().filter(|(_, at)| at.min.x >= right).map(|(t, _)| t).collect()
}

fn click_text(r: &mut Rig, out: &FullOutput, want: &str) {
    let at = texts(out).into_iter().find(|(t, _)| t == want).unwrap_or_else(|| panic!("no {want:?} in {:?}", texts(out))).1.center();
    r.click(at, PointerButton::Primary);
}

/// Where berth `name` is drawn now.
fn berth_at(r: &Rig, name: &str) -> Pos2 {
    let g = r.ui.core.game().unwrap();
    let sc = client_ui::scene::Scene::build(g.layout().unwrap()).unwrap();
    let b = sc.berths.iter().find(|b| b.name == name).unwrap();
    client_ui::hit::berth_rect(&r.ui.camera().unwrap(), r.ui.diagram_rect().unwrap(), b.at, b.offset_px).center()
}

/// Frames until 1E01 (entering at W at 07:00) is described in a berth.
fn until_1e01_is_shown(r: &mut Rig) -> String {
    for _ in 0..100 {
        r.frame();
        if let Some((b, _)) = r.view().berths.iter().find(|(_, h)| *h == "1E01") {
            return b.clone();
        }
    }
    panic!("1E01 never described: {:?}", r.view().berths);
}

#[test]
fn the_top_bar_names_the_workstation_or_says_spectating() {
    let mut r = Rig::in_game(drawn_twobox(), Some("East"));
    assert!(has_text(&r.frame(), "g-test · Workstation B · East (ann)"));
    let mut r = Rig::in_game(drawn_twobox(), None);
    assert!(has_text(&r.frame(), "g-test · spectating (ann)"));
}

#[test]
fn settings_change_from_the_menu_and_are_kept() {
    let store = MemStore::new();
    let mut r = Rig::in_game_with(drawn_twobox(), Some("West"), Some(store.clone()));
    assert_eq!(r.ui.settings(), client_core::Settings::default());
    for item in ["Real aspects", "Signal numbers"] {
        let out = r.frame();
        click_text(&mut r, &out, "Settings");
        let out = r.frame();
        click_text(&mut r, &out, item);
    }
    assert_eq!((r.ui.settings().aspects, r.ui.settings().numbers), (AspectMode::Real, false));
    assert_eq!(store.text().as_deref(), Some("aspects=real\nenquiry=off\nnumbers=off\n"));
    let again = Rig::in_game_with(drawn_twobox(), Some("West"), Some(store));
    assert_eq!(again.ui.settings().aspects, AspectMode::Real, "a new page reads them back");
}

#[test]
fn the_simplifier_tab_lists_searches_and_shows_lateness() {
    let mut r = Rig::in_game(drawn_twobox(), None);
    let out = r.frame();
    click_text(&mut r, &out, "SIMPLIFIER");
    let out = r.frame();
    let side = side_texts(&r, &out);
    for want in ["Train", "Late", "Plat", "1E01", "1N02", "2W03", "EST", "07:04", "07:05"] {
        assert!(side.iter().any(|t| t == want), "{want} in {side:?}");
    }
    assert!(has_text(&out, "ALARMS"), "the alarms stay in view");
    until_1e01_is_shown(&mut r);
    let out = r.frame();
    let side = side_texts(&r, &out);
    assert!(side.iter().any(|t| t == "OT"), "1E01 is running, on time: {side:?}");
    let out = r.frame();
    click_text(&mut r, &out, "headcode");
    r.events.push(Event::Text(s("1n")));
    r.frame();
    let out = r.frame();
    let side = side_texts(&r, &out);
    assert!(side.iter().any(|t| t == "1N02") && !side.iter().any(|t| t == "1E01"), "{side:?}");
    r.events.push(Event::Text(s("zz")));
    r.frame();
    assert!(has_text(&r.frame(), "No headcode matches"));
}

#[test]
fn a_headcode_click_opens_the_enquiry_only_when_it_is_on() {
    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
    let berth = until_1e01_is_shown(&mut r);
    let w1 = r.at(100.0, -5.0);
    r.click(w1, PointerButton::Primary);
    r.click(berth_at(&r, &berth), PointerButton::Primary);
    assert!(!has_text(&r.frame(), "Train 1E01"), "enquiry off: no window");
    assert_eq!(r.ui.core.game().unwrap().selected(), None, "off, it is a dead click as in D1");

    let store = MemStore::new();
    let mut w = store.clone();
    client_core::SettingsStore::save(&mut w, "enquiry=on");
    let mut r = Rig::in_game_with(drawn_twobox(), Some("West"), Some(store));
    let berth = until_1e01_is_shown(&mut r);
    let w1 = r.at(100.0, -5.0);
    r.click(w1, PointerButton::Primary);
    r.click(berth_at(&r, &berth), PointerButton::Primary);
    let out = r.frame();
    assert!(has_text(&out, "Train 1E01") && has_text(&out, "in area, OT"), "{:?}", texts(&out));
    assert!(has_text(&out, "Not in the simplifier for this area"), "West has no platforms");
    assert_eq!(r.ui.enquiry(), Some("1E01"));
    assert_eq!(r.ui.core.game().unwrap().selected(), Some("W1"), "looking a train up never touches the entrance");
}

#[test]
fn a_headcode_in_the_train_list_opens_the_enquiry() {
    let store = MemStore::new();
    let mut w = store.clone();
    client_core::SettingsStore::save(&mut w, "enquiry=on");
    let mut r = Rig::in_game_with(drawn_twobox(), Some("East"), Some(store));
    let out = r.frame();
    let right = r.ui.diagram_rect().unwrap().max.x;
    let at = texts(&out).into_iter().find(|(t, at)| t == "1E01" && at.min.x >= right).expect("1E01 in the train list").1.center();
    r.click(at, PointerButton::Primary);
    let out = r.frame();
    assert!(has_text(&out, "Train 1E01"), "{:?}", texts(&out));
    assert!(has_text(&out, "EST to EST") && has_text(&out, "EST 1 07:04 07:05"), "East's simplifier row");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-ui --test screens`
Expected: compile errors — no `UiApp::with_store`, `settings`, `enquiry`.

- [ ] **Step 3: The screens**

Replace `crates/client-ui/src/screens.rs` with:
```rust
//! The screens (spec D1 §3, realism spec §3–§4): the lobby, and in a game
//! the top bar (game, workstation, clock, votes, settings, players), the
//! diagram, the train list or the simplifier and the alarms on the right,
//! and the headcode enquiry window. `UiApp::ui` is the whole frame; the
//! shell calls it.

use std::time::Duration;

use client_core::simplifier::{self, Line};
use client_core::text::{fmt_hms, proposal_text, train_state_text, vote_text};
use client_core::trains::train_list;
use client_core::{App, AspectMode, Link, Settings, SettingsStore, Target};
use egui::{Align, Align2, Color32, CornerRadius, FontId, Frame, Key, Layout, PointerButton, Rect, RichText, Sense, Ui, vec2};
use protocol::{GameState, Proposal};

use crate::camera::Camera;
use crate::hit::hit_test;
use crate::paint::{self, BG, PaintState};
use crate::scene::Scene;

/// Alarms and the connection banner.
pub const ALARM: Color32 = Color32::from_rgb(0xFF, 0x5A, 0x5A);
/// How far one wheel "line" (egui points of scroll) zooms.
const ZOOM_PER_POINT: f32 = 1.0 / 200.0;
/// Simplifier columns, in points: headcode, lateness, from, to, at,
/// platform, arrival, departure (wide enough for `BTHNLGR`, `ML_UP` and
/// `05:03½`; the panel scrolls sideways when narrower).
const SIMPLIFIER_COLUMNS: [f32; 8] = [38.0, 26.0, 50.0, 50.0, 56.0, 48.0, 46.0, 46.0];

/// The upper half of the side panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SideTab {
    #[default]
    Trains,
    Simplifier,
}

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
    /// The game whose Delete was pressed and awaits "Yes, delete".
    confirm_delete: Option<String>,
    settings: Settings,
    /// Where the settings are kept between visits (none in most tests).
    store: Option<Box<dyn SettingsStore>>,
    side_tab: SideTab,
    /// The simplifier's headcode search.
    search: String,
    /// The headcode whose enquiry window is open.
    enquiry: Option<String>,
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
            confirm_delete: None,
            settings: Settings::default(),
            store: None,
            side_tab: SideTab::default(),
            search: String::new(),
            enquiry: None,
        }
    }

    /// With the settings `store` holds, saving every change back to it.
    pub fn with_store(core: App, store: Box<dyn SettingsStore>) -> UiApp {
        let mut ui = UiApp::new(core);
        ui.settings = store.load().map_or_else(Settings::default, |t| Settings::from_text(&t));
        ui.store = Some(store);
        ui
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    fn set_settings(&mut self, s: Settings) {
        if s == self.settings {
            return;
        }
        self.settings = s;
        if let Some(store) = self.store.as_mut() {
            store.save(&s.to_text());
        }
        if !s.enquiry {
            self.enquiry = None;
        }
    }

    /// The headcode whose enquiry window is open.
    pub fn enquiry(&self) -> Option<&str> {
        self.enquiry.as_deref()
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
            let mut delete = None;
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
                    ui.horizontal(|ui| {
                        if ui.button(if g.state == GameState::Running { "Join" } else { "Resume" }).clicked() {
                            join = Some(g.id.clone());
                        }
                        // Owner decision 13: the front re-checks all of it.
                        if g.can_delete {
                            if self.confirm_delete.as_deref() == Some(g.id.as_str()) {
                                ui.label(RichText::new("Delete for good?").color(ALARM));
                                if ui.button("Yes, delete").clicked() {
                                    delete = Some(g.id.clone());
                                }
                                if ui.button("Cancel").clicked() {
                                    self.confirm_delete = None;
                                }
                            } else if ui.button("Delete").clicked() {
                                self.confirm_delete = Some(g.id.clone());
                            }
                        }
                    });
                    ui.end_row();
                }
            });
            if let Some(id) = join {
                self.core.join(&id);
            }
            if let Some(id) = delete {
                self.confirm_delete = None;
                self.core.delete_game(&id);
            }
        });
    }

    fn game(&mut self, ui: &mut Ui, now: f64) {
        self.top_bar(ui);
        egui::Panel::right("side").default_size(330.0).show(ui, |ui| self.side(ui));
        egui::CentralPanel::default().frame(Frame::NONE.fill(BG)).show(ui, |ui| self.diagram_ui(ui, now));
        self.enquiry_window(ui);
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        let Some(g) = self.core.game() else { return };
        let title = match g.area() {
            Some(a) => match g.names().workstation(a) {
                Some(ws) => format!("{} · Workstation {ws} · {a} ({})", g.id, g.you),
                None => format!("{} · {a} ({})", g.id, g.you),
            },
            None => format!("{} · spectating ({})", g.id, g.you),
        };
        let view = g.view().cloned();
        let areas: Vec<String> = g.layout().map(|l| l.areas.clone()).unwrap_or_default();
        let holding = g.area().is_some();
        let can_vote = g.can_vote();
        let mut act: Vec<Box<dyn FnOnce(&mut App)>> = Vec::new();
        let mut settings = self.settings;
        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(title).strong());
                if let Some(v) = &view {
                    ui.label(RichText::new(fmt_hms(v.sim_time)).monospace().size(16.0));
                    ui.label(if v.paused { "paused".to_string() } else { format!("{}×", v.speed) });
                    // Only voters get the buttons (owner decision 12).
                    if can_vote {
                        let pause = if v.paused { Proposal::Resume } else { Proposal::Pause };
                        if ui.button(proposal_text(pause)).clicked() {
                            act.push(Box::new(move |a| a.vote(pause)));
                        }
                        for x in [1u8, 2, 4, 8] {
                            if ui.selectable_label(!v.paused && v.speed == x, format!("{x}×")).clicked() {
                                act.push(Box::new(move |a| a.vote(Proposal::Speed { x })));
                            }
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
                ui.menu_button("Settings", |ui| {
                    ui.label(RichText::new("Signal aspects").strong());
                    ui.radio_value(&mut settings.aspects, AspectMode::RedGreen, "Red/green (panel)");
                    ui.radio_value(&mut settings.aspects, AspectMode::Real, "Real aspects");
                    ui.separator();
                    ui.checkbox(&mut settings.enquiry, "Headcode enquiry");
                    ui.checkbox(&mut settings.numbers, "Signal numbers");
                });
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
        self.set_settings(settings);
        for f in act {
            f(&mut self.core);
        }
    }

    /// The upper half: the train list or the simplifier; the lower half:
    /// the alarms, always in view.
    fn side(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.side_tab, SideTab::Trains, RichText::new("TRAINS").strong());
            ui.selectable_value(&mut self.side_tab, SideTab::Simplifier, RichText::new("SIMPLIFIER").strong());
        });
        let half = ui.available_height() * 0.5;
        match self.side_tab {
            SideTab::Trains => self.trains_ui(ui, half),
            SideTab::Simplifier => self.simplifier_ui(ui, half),
        }
        let Some(g) = self.core.game() else { return };
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

    fn trains_ui(&mut self, ui: &mut Ui, height: f32) {
        let Some(g) = self.core.game() else { return };
        let enquiry = self.settings.enquiry;
        let mut open = None;
        egui::ScrollArea::vertical().id_salt("trains").max_height(height).show(ui, |ui| {
            let Some(v) = g.view() else { return };
            egui::Grid::new("train_list").striped(true).show(ui, |ui| {
                for (h, r) in train_list(v) {
                    let code = RichText::new(h).monospace().color(paint::HEADCODE);
                    // With the enquiry on, a headcode opens its window.
                    if enquiry {
                        if ui.add(egui::Label::new(code).sense(Sense::click())).clicked() {
                            open = Some(h.to_string());
                        }
                    } else {
                        ui.label(code);
                    }
                    ui.label(train_state_text(r.state));
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
        if open.is_some() {
            self.enquiry = open;
        }
    }

    /// The simplifier (realism spec §3): the layout's rows in running
    /// order, searched by headcode, drawn only where the scroll shows them.
    fn simplifier_ui(&mut self, ui: &mut Ui, height: f32) {
        ui.add(egui::TextEdit::singleline(&mut self.search).id_salt("simplifier_search").desired_width(120.0).hint_text("headcode"));
        let Some(g) = self.core.game() else { return };
        let Some(l) = g.layout() else { return };
        let v = g.view();
        let lines: Vec<(Line, Option<String>)> = simplifier::rows(l, &self.search)
            .into_iter()
            .flat_map(|r| {
                let late = simplifier::lateness(v, &r.headcode);
                simplifier::lines(r).into_iter().enumerate().map(move |(i, line)| (line, late.clone().filter(|_| i == 0)))
            })
            .collect();
        if lines.is_empty() {
            ui.label(if l.simplifier.is_empty() { "No booked trains here" } else { "No headcode matches" });
            return;
        }
        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
        let header = ["Train", "Late", "From", "To", "At", "Plat", "Arr", "Dep"];
        simplifier_row(ui, row_h, header.map(|h| RichText::new(h).strong()));
        egui::ScrollArea::both().id_salt("simplifier").max_height(height).show_rows(ui, row_h, lines.len(), |ui, range| {
            for (line, late) in &lines[range] {
                let late = late.as_deref().unwrap_or("");
                let cells = [
                    RichText::new(&line.headcode).monospace().color(paint::HEADCODE),
                    RichText::new(late).color(if late == "OT" { paint::LABEL } else { ALARM }),
                    RichText::new(&line.from),
                    RichText::new(&line.to),
                    RichText::new(&line.place),
                    RichText::new(&line.platform),
                    RichText::new(&line.arr),
                    RichText::new(&line.dep),
                ];
                simplifier_row(ui, row_h, cells);
            }
        });
    }

    fn enquiry_window(&mut self, ui: &mut Ui) {
        let Some(h) = self.enquiry.clone() else { return };
        let mut open = true;
        let Some(g) = self.core.game() else { return };
        let (Some(l), v) = (g.layout(), g.view()) else { return };
        let e = simplifier::enquiry(l, v, &h);
        egui::Window::new(format!("Train {h}")).id(egui::Id::new("enquiry")).open(&mut open).resizable(false).show(ui.ctx(), |ui| {
            ui.label(e.live_text());
            if e.rows.is_empty() {
                ui.label("Not in the simplifier for this area");
            }
            for r in &e.rows {
                ui.label(format!("{} to {}", r.origin.as_deref().unwrap_or("?"), r.destination.as_deref().unwrap_or("?")));
                for line in simplifier::lines(r) {
                    ui.label(format!("{} {} {} {}", line.place, line.platform, line.arr, line.dep));
                }
            }
        });
        if !open {
            self.enquiry = None;
        }
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
        // Every click goes on, even one on nothing or on what is not yours:
        // a dead click clears the entrance (`App::click` decides what the
        // rest mean, from the same operability `Hit::clickable` shows).
        let click = resp.clicked().then(|| hit_at(resp.interact_pointer_pos()).map(|h| h.target));
        if resp.secondary_clicked() {
            self.menu_target = hit_at(resp.interact_pointer_pos()).map(|h| h.target);
        }
        let exits = self.core.valid_exits();
        let Some(g) = self.core.game() else { return };
        let st = PaintState {
            view: g.view(),
            selected: g.selected(),
            exits: &exits,
            refused: g.refused(),
            time: now,
            aspects: self.settings.aspects,
            numbers: self.settings.numbers,
            names: g.names(),
        };
        paint::paint(&painter, paint::draw(scene, &cam, rect, &st));
        match click {
            // With the enquiry on, a headcode opens its window and nothing else.
            Some(Some(t)) => match self.core.headcode_at(&t).filter(|_| self.settings.enquiry) {
                Some(h) => self.enquiry = Some(h),
                None => self.core.click(&t),
            },
            Some(None) => self.core.escape(),
            None => {}
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

/// One simplifier line in fixed-width cells.
fn simplifier_row(ui: &mut Ui, row_h: f32, cells: [RichText; 8]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (text, w) in cells.into_iter().zip(SIMPLIFIER_COLUMNS) {
            ui.allocate_ui_with_layout(vec2(w, row_h), Layout::left_to_right(Align::Center), |ui| {
                ui.set_min_width(w);
                ui.add(egui::Label::new(text).truncate());
            });
        }
    });
}
```
Notes for the reviewer: `ui.menu_button` closes its menu when an item inside is clicked (egui's menu behaviour), which is why the test opens it once per change; the simplifier's lateness is the second column so that it is never scrolled out of the 330-point side panel; `show_rows` lays out only the visible lines, so Liverpool Street's 1373 rows cost nothing per frame.

- [ ] **Step 4: Run the tests**

Run: `scripts/cargo test -p signalbox-client-ui`
Expected: PASS.
Run: `scripts/cargo build --workspace --all-targets --locked`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/client-ui
git commit -m "feat(client-ui): workstation in the top bar, Settings menu, simplifier tab and headcode enquiry"
```

---

### Task 11: `client-web` — the settings in `localStorage`

**Files:**
- Create: `crates/client-web/src/store.rs` (`LocalStore`)
- Modify: `crates/client-web/src/lib.rs` (`mod store;`, `UiApp::with_store`), `crates/client-web/Cargo.toml` (web-sys feature `Storage`)

**Interfaces:**
- Consumes: Task 7's `SettingsStore`, `SETTINGS_KEY`; Task 10's `UiApp::with_store`.
- Produces: `client_web::store::LocalStore` (private to the crate): `load` reads `localStorage["signalbox.settings"]`, `save` writes it; no storage (a private window, blocked site data) reads as nothing stored and drops saves, so the defaults apply.

The crate is wasm32-only (natively empty), so there is no native test: the logic it relies on (`Settings::from_text` tolerating anything, `UiApp::with_store` loading and saving) is tested in Tasks 7 and 10, and the Controller's browser check (C2) reloads the page to see a setting kept.

- [ ] **Step 1: The store**

Create `crates/client-web/src/store.rs`:
```rust
//! The display settings in the browser's `localStorage` (realism spec §4):
//! remembered per browser. Without storage (private windows, blocked
//! cookies) the defaults simply apply.

use client_core::SettingsStore;
use client_core::settings::SETTINGS_KEY;

pub struct LocalStore;

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

impl SettingsStore for LocalStore {
    fn load(&self) -> Option<String> {
        storage()?.get_item(SETTINGS_KEY).ok().flatten()
    }

    fn save(&mut self, text: &str) {
        if let Some(s) = storage() {
            // A full or refused store just means the setting is not kept.
            let _ = s.set_item(SETTINGS_KEY, text);
        }
    }
}
```
In `crates/client-web/Cargo.toml`, add `"Storage",` to the web-sys features after `"Response",` (the list stays alphabetical).

- [ ] **Step 2: Use it**

In `crates/client-web/src/lib.rs`: `mod store;` above `mod transport;`, `use crate::store::LocalStore;` above the transport import, and in `run`'s app-creator closure build the UI with the store:
```rust
                let ui = UiApp::with_store(App::new(Box::new(transport), now), Box::new(LocalStore));
                Ok(Box::new(WebApp { ui, sent_to_login: false }))
```

- [ ] **Step 3: Build it**

Run: `scripts/wasm-build`
Expected: ``Finished `web` profile``, no warnings, and `target/web-dist/app/signalbox_web_bg.wasm` listed (about 8 MB, as in D1).
Run: `scripts/cargo build --workspace --all-targets --locked` and `git diff --exit-code Cargo.lock`
Expected: no warnings; exit 0 (web-sys is already locked: a feature flag adds no package).

- [ ] **Step 4: Commit**

```bash
git add crates/client-web
git commit -m "feat(client-web): keep the display settings in localStorage"
```

---

### Task 12: the image converts with line names; docs; the whole-branch check

**Files:**
- Modify: `deploy/Dockerfile` (`--lines`), `deploy/README.md` (the Dockerfile row), `CLAUDE.md`

**Interfaces:**
- Consumes: everything above.
- Produces: a release image whose three layouts carry prefixes, workstation letters, line names and the simplifier; a CLAUDE.md that tells the next session how the realism pass works.

- [ ] **Step 1: The image**

In `deploy/Dockerfile`, the conversion loop's `ts2-import` line gains the lines file:
```dockerfile
      target/release/ts2-import "crates/ts2-import/tests/data/$n.json" -o "/out/layouts/$n.json" \
        --areas "layouts/$n.areas.json" --lines "layouts/$n.lines.json" || exit 1; \
```
In `deploy/README.md`, the `Dockerfile` row of the file table becomes
```markdown
| `Dockerfile` | release image: both binaries, the browser client (`/opt/signalbox/web`, built by the `wasm-tools` and `web` stages) and the converted layouts `liverpool-st`, `drain`, `gretz-armainvilliers` with their areas, box prefixes and line names from `layouts/` (no dev login) |
```

- [ ] **Step 2: CLAUDE.md**

- In the Commands block, the Liverpool Street `ts2-import` line becomes
```bash
scripts/cargo run -p ts2-import -- crates/ts2-import/tests/data/liverpool-st.json -o /w/target/lst.json --areas /w/layouts/liverpool-st.areas.json --lines /w/layouts/liverpool-st.lines.json
```
- In "Multiplayer", replace the first bullet (Areas for converted layouts…) with:
```markdown
- Areas for converted layouts come from `layouts/<name>.areas.json`, applied by
  `ts2-import --areas`: each area floods the section graph from its seeds and
  stops at nodes holding boundary signals (`ts2_import::areas`). The file also
  names the box (`prefix`, 1–3 capitals) and each area's `workstation` letter;
  both go into the world's client-only `layout` JSON (`box_prefix`,
  `workstations`), never into the sim. Optional line names come from
  `layouts/<name>.lines.json` (`ts2-import --lines`): `direction` there is the
  world's `up`/`down` (the direction of the line's signals), not the railway's.
- `game::display` reads those display keys once per game (defaults: the
  title's first letter, A, B, C… in area order) and builds each area's
  simplifier from the timetable; every `Layout` carries them.
- Clock votes (realism owner decision 12): holders vote; while nobody holds
  an area every connected player does (`Game::voters`), re-settled on every
  claim, release, grace expiry, connect and spectator disconnect.
- Deleting games (owner decision 13): lobby `delete_game`, saved or crashed
  games only, by the creator (meta row `creator`, written by the game
  process; save schema still 2) or a `SIGNALBOX_ADMINS` user.
```
- At the end of "Browser client", add:
```markdown
- The look follows `docs/superpowers/specs/2026-10-01-panel-realism-design.md`
  (IECC conventions): signals are shown as `<box><workstation><number>`
  (`client_core::Names`; display only, wire names stay plain); settings
  (aspects red/green or real, headcode enquiry, signal numbers) live behind
  `client_core::SettingsStore` (`localStorage` in `client-web`). Nothing
  flashes except points moving, the selected entrance and a cancelling
  route's lamp. Arrows and the ○A button are shapes: egui's default fonts
  have no arrow glyphs.
```

- [ ] **Step 3: The whole branch**

Run each and read the output:
```bash
scripts/cargo build --workspace --all-targets --locked
scripts/cargo test --workspace --locked
scripts/cargo test -p signalbox-server --features dev-auth --locked
scripts/cargo build -p signalbox-bot --no-default-features --locked
scripts/wasm-build
for n in liverpool-st drain gretz-armainvilliers; do
  scripts/cargo run -q -p ts2-import -- crates/ts2-import/tests/data/$n.json -o /w/target/$n.json \
    --areas /w/layouts/$n.areas.json --lines /w/layouts/$n.lines.json || echo "FAILED $n"
done
git diff main --stat -- Cargo.lock
```
Expected: no warnings anywhere; every test PASS (the slow `--ignored` soaks are not part of this pass: nothing here changes the sim); each conversion prints its `area …` lines and no `FAILED`; the lock differs from `main` by the one `ts2-import` line of Task 9. (All of this was run on the scratch prototype of this plan on 2026-10-01.)

- [ ] **Step 4: Commit**

```bash
git add deploy/Dockerfile deploy/README.md CLAUDE.md
git commit -m "docs: the realism pass in CLAUDE.md; the image converts the layouts with their line names"
```

---

## Controller section (owner-gated infra; never a subagent)

The owner has agreed to redeploy the way D1 was deployed. Do these in order after the branch's final review, and put what was done (and the screenshots) in the branch report.

### C0. The CI cache

Nothing to reseed: the only `Cargo.lock` change is Task 9's path dev-dependency (`ts2-import` in `signalbox-client-ui`), which adds no package. The runner's `SIGNALBOX_REQUIRE_WASM=1` build covers Task 11 (a web-sys feature of an already-cached crate).

### C1. Deploy

As `deploy/README.md` "Build and run", plus the compose file (it now sets `SIGNALBOX_ADMINS: skye`):
```bash
cd /home/skye-fi/projects/signalbox            # at the merged commit
rev=$(git rev-parse --short HEAD)
docker build -f deploy/Dockerfile -t local/signalbox:$rev -t local/signalbox:current .
sudo install -o root -g docker -m 0644 deploy/docker-compose.yml /opt/stack/apps/signalbox/docker-compose.yml
cd /opt/stack/apps/signalbox && sudo docker compose up -d      # the vault must be mounted (oidc.env)
docker logs signalbox 2>&1 | head -3        # "listening on 0.0.0.0:9160", no "no web client"
docker exec signalbox printenv SIGNALBOX_ADMINS                # skye
/home/skye-fi/projects/signalbox/deploy/smoke.sh https://ra.tail3e0c1e.ts.net:50160 303
```
Expected: every smoke line `ok`. Existing saves keep working: they carry no `box_prefix`/`workstations`/`creator`, so they get the default prefixes and only `skye` can delete them. Roll back as the README says (retag the previous `<rev>`).

### C2. Headless browser check of the new look (Playwright on ra, dev-auth build from the working copy)

The release image has no dev login, so this runs a throwaway dev-auth front on `127.0.0.1:19161` (WebGL2 through SwiftShader: `--disable-webgpu`). All files go under `target/realism-check/`.
```bash
cd /home/skye-fi/projects/signalbox
scripts/wasm-build                                            # target/web-dist
scripts/cargo build -p signalbox-server --features dev-auth --bins
mkdir -p target/realism-check/layouts
for n in liverpool-st drain gretz-armainvilliers; do
  scripts/cargo run -q -p ts2-import -- crates/ts2-import/tests/data/$n.json -o /w/target/realism-check/layouts/$n.json \
    --areas /w/layouts/$n.areas.json --lines /w/layouts/$n.lines.json
done
docker run --rm -d --name sbx-realism-check --network host -u "$(id -u):$(id -g)" -v "$PWD:/w" -w /w \
  -e SIGNALBOX_ADDR=127.0.0.1:19161 -e SIGNALBOX_DATA=/w/target/realism-check/data \
  -e SIGNALBOX_LAYOUTS=/w/target/realism-check/layouts -e SIGNALBOX_WEB=/w/target/web-dist \
  -e SIGNALBOX_SESSION_KEY="$(openssl rand -hex 64)" -e SIGNALBOX_ADMINS=ann \
  rust:1.98-slim-bookworm target/debug/signalbox-server
```
Save this as `target/realism-check/check.py` (D1's script plus a `reload` action):
```python
"""Drive the signalbox browser client in headless Chromium.

usage: CHROMIUM_ARGS="..." python3 check.py BASE OUTDIR USER ACTION...
  ACTION: shot:NAME | click:X,Y | rclick:X,Y | wait:MS | key:NAME | reload
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
        elif kind == "reload":
            page.reload()
            page.wait_for_timeout(3000)
        elif kind == "key":
            page.keyboard.press(arg)
            page.wait_for_timeout(200)
        else:
            sys.exit(f"unknown action {a}")
    browser.close()
print("\n".join(logs))
```
and run it with:
```bash
pw() { docker run --rm --network host -e CHROMIUM_ARGS="--disable-webgpu" \
  -v "$PWD/target/realism-check:/out" -w /out mcr.microsoft.com/playwright/python:v1.55.0-noble \
  sh -c "pip install -q playwright==1.55.0 && python3 check.py http://127.0.0.1:19161 /out ann $*"; \
  docker run --rm -v "$PWD/target/realism-check:/out" alpine chown -R "$(id -u):$(id -g)" /out; }
pw shot:1-lobby
```
Everything is one canvas: read the next click's coordinates off each screenshot. Each run is a new browser context (no stored settings) and a new socket; the front keeps the game (paused while nobody is connected), so later runs start with `click:<Join>`, and the bar is laid out differently while paused (`resume` is wider than `pause`). Coordinates from the scratch run at 1280 × 800: layout box (57, 68) then `liverpool-st` (51, 135); Create (435, 68); Join (773, 141); Claim for Bethnal Green as a spectator (398, 33), for Liverpool Street (233, 33); Settings (724, 11) running, (760, 11) paused; its items about 50 right and 45/65/95/115 down; `SIMPLIFIER` (1053, 56). What to see:
1. `click:<box> click:<liverpool-st> click:<Create> wait:5000 shot:2-spectator` — black background, thick grey track broken at joints, white routes the robot set, red occupation, cyan headcodes in the platform roads on black knock-outs, ochre platform blocks with their numbers, blue ○A beside automatic signals, grey capital labels, the eight line names with arrows (they may crowd each other at this zoom), small grey arrows at the ends of the track. No `panicked` or `pageerror` in the console output.
2. `… click:<Claim Liverpool Street> wait:4000 shot:3-box` — the top bar reads `… · Workstation A · Liverpool Street (ann)`; signals as discs on hooked posts left of the line, numbered `LA9`, `LA11` …; fringe track hollow, fringe signals grey (`LB64` …); arrows at the platform ends.
3. `… click:<an entrance> wait:500 shot:4-entrance` then `… click:<a lit exit> wait:4000 shot:5-route` — the entrance ring blinks (catch it lit or not), the route goes white with a tick at the end of its overlap, the entrance's post turns white, the lamp green (red/green mode).
4. `… click:<Settings> wait:500 click:<Real aspects> wait:500 shot:6-real` — the same signal shows its real aspect; then `click:<Settings> click:<Headcode enquiry> reload click:<Join> wait:3000 click:<Settings> shot:7-kept` — both settings survived the reload.
5. `… click:<a cyan headcode> wait:800 shot:8-enquiry` — a `Train <headcode>` window with its live state and calls; the entrance selection (if any) unchanged.
6. `… click:<SIMPLIFIER> wait:800 shot:9-simplifier` — rows in time order, `HH:MM` with `½`, `pass`, `OT`/`nL` beside running trains; the alarms still visible below.

Attach the screenshots to the branch report. Then clean up:
```bash
docker stop sbx-realism-check
rm -rf target/realism-check
```

### C3. The owner's own look

In Chrome/Edge and Firefox on the tailnet: create a Liverpool Street game, claim a box, set a route, try both aspect modes, the enquiry and the simplifier; as the game's creator delete an old saved game from the lobby (the confirm step), and check that a game created by someone else offers no Delete to a non-admin.
