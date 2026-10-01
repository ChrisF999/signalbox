# signalbox — browser polish pass (D1.2)

Date: 2026-10-01
Status: draft for owner review (every decision marked *proposed* needs the owner's OK)
License: GPL-2.0-or-later
Builds on: `2026-10-01-panel-realism-design.md` (D1.1, deployed) and `2026-09-30-browser-client-design.md` (D1).
Base for the plan: **`main` after the `tutorial` branch merges.** Line references in the plan were taken at
`5eb82f0` and must be re-checked then.

## 1. Goal

Finish the browser client before any desktop (D2) work: a panel that is legible at every zoom, a Drain
timetable that does not stop at 06:43, old Drain saves that show today's names, and a real-browser check
that the WebGL2 fallback works. Plus one small edge the repeat timetable makes urgent (the simplifier
opening at "now"). Nothing here changes the simulation, the wire protocol or the save schema.

The `tutorial` branch is changing protocol, the server front, client-core and client-ui at the same time.
This pass therefore keeps to drawing and layout code, the converter, the game library's resume path and
deploy scripts; its only server change is one game-process argument (§5.3).

## 2. Owner decisions

| # | Topic | Decision | Status |
|---|---|---|---|
| 1 | Clutter | Make the panel legible at every zoom: label collision avoidance / priority culling, ○A hidden below the number threshold, label density by zoom; measure first so the success criterion is a number | owner, 2026-10-01 |
| 2 | Drain timetable | A "repeat timetable" option: deterministic, save-compatible | owner, 2026-10-01 |
| 3 | Old Drain saves | Saves from before the realism pass show `L…` instead of `W…`; decide a migration that keeps determinism | owner, 2026-10-01 |
| 4 | WebGL2 fallback | An automated real-browser check (Chromium via the Playwright image) in a CI-able deploy script | owner, 2026-10-01 |
| P1 | What "legible" means | The measure in §3.1 over 66 renders (11 views × 3 zooms × 2 window sizes); the targets in §3.4 | **proposed — needs owner OK** |
| P2 | Who wins a collision | Priority: headcodes (never moved), own signal numbers, ○A letters, line names, platform numbers, other labels, fringe signal numbers. A lower item moves or is hidden; nothing is ever drawn over another text | **proposed — needs owner OK** |
| P3 | Nothing jumps while trains run | Placement depends only on the layout, the zoom and the settings: every berth's box is kept clear whether or not it holds a headcode, and a second (double-yellow) lamp's spot is kept clear on every signal. Panning never re-places anything | **proposed — needs owner OK** |
| P4 | A signal number that does not fit | Tried at its own spot, six spots round its signal, then nudged by up to one text width or height; if no spot is clear of track, the first clear of text, lamps and boxes is used (it may touch the track: "tight"); otherwise it is hidden (its hover text still names it). An own number is never culled for a label | **proposed — needs owner OK** |
| P5 | ○A at low zoom | Neither drawn nor clickable whenever signal numbers are too small to draw (below 7 px), for players as well as spectators; zoom in to use it. Its letter `A` moves outward (away from the track) and is dropped where it has no room; the button stays | **proposed — needs owner OK** |
| P6 | Line names that collide | Nudged up or down by up to one line height; hidden where no nudge is clear. No per-layout position override this pass | **proposed — needs owner OK** |
| P7 | Where "repeat" lives | Per layout, baked in at conversion (`layouts/<name>.repeat.json`, `ts2-import --repeat`), not a per-game option at creation: no protocol, lobby or front change while the tutorial is in flight. Only Drain gets a file | **proposed — needs owner OK** |
| P8 | Drain's pattern | Every 10 minutes, headcode numbers +2 per repeat, until 14:00 (last trains BW96/WB96; headcodes stay 4 characters); after that the three trains stable as today's last ones do | **proposed — needs owner OK** |
| P9 | Old saves' timetables | Unchanged: a save carries its world and its sim parts are frozen, so an old Drain save still ends at 06:43. New games get the repeat | **proposed — needs owner OK** |
| P10 | Old saves' names | On every resume the game takes the display data (`layout`) from the current layout file when the saved network matches it exactly; in memory only, never written back | **proposed — needs owner OK** |
| P11 | Where the browser check runs | `deploy/browser-check.sh` on ra (Docker + the Playwright image), run by the controller before every deploy and in this pass's final verification; not in Forge CI, whose runner has neither Docker nor internet | **proposed — needs owner OK** |
| P12 | Simplifier start | The simplifier opens scrolled to the first train not yet finished, not to the first train of the day | **proposed — needs owner OK** |
| P13 | Headcode boxes | Every berth's knock-out is as wide as the layout's longest booked headcode (at least today's 34 px): Gretz's 7-character numbers overflowed theirs onto the track and its neighbours | **proposed — needs owner OK** |

## 3. Legibility (owner decision 1)

### 3.1 How it was measured

A scratch headless test (not committed; its committed form is §3.5) converted the three shipped layouts with
their areas and lines files, ran each for ten sim minutes at 8× under the robot so headcodes are about, and drew
every box and a spectator view — 11 views — with `paint::draw` at the camera "Fit" gives, at 2× and at 4× that
zoom, in the diagram rectangle of a 1280 × 800 window (882 × 755 px) and of a 1920 × 1080 window
(1522 × 1035 px). Text rectangles come from egui's own fonts (`Fonts::layout_no_wrap`), so sizes are real, not
estimated. Counted per render:

- **T, overlapping text pairs**: two drawn texts whose rectangles overlap by more than 0.5 px both ways
  (a headcode counts as its 34 × 14 px knock-out box).
- **N, numbers on track**: signal numbers whose rectangle touches a track bar (any line at the track width).
- **L, labels on track**: labels and line names touching a track bar.
- **D, text on lamps**: texts (not headcodes) touching a signal lamp or a ○A circle.
- **A, ○A letters on track**.

### 3.2 Baseline (deployed `5eb82f0`), 1280 × 800, Fit

| View | Scale (px/unit) | T | N (of numbers drawn) | L | D | A |
|---|---|---|---|---|---|---|
| Liverpool St spectator | 0.300 | 74 | 0 (numbers hidden) | 37 | 25 | 47 |
| Liverpool St box A | 0.712 | 4 | **12 of 36** | 2 | 10 | 2 |
| Liverpool St box B | 0.790 | 3 | 3 of 25 | 4 | 1 | 2 |
| Liverpool St box C | 1.011 | 6 | 0 of 26 | 1 | 0 | 10 |
| Drain spectator | 0.592 | 3 | 5 of 15 | 3 | 4 | 4 |
| Drain box A (Bank) | 1.620 | 0 | 0 of 5 | 0 | 0 | 3 |
| Drain box B (Waterloo) | 0.892 | 2 | 3 of 12 | 0 | 1 | 4 |
| Gretz spectator | 0.128 | **322** | 0 (hidden) | 36 | 116 | 49 |
| Gretz box A | 0.326 | 30 | 0 (hidden) | 13 | 22 | 22 |
| Gretz box B | 0.360 | 18 | 0 (hidden) | 7 | 10 | 13 |
| Gretz box C | 0.388 | 19 | 0 (hidden) | 7 | 9 | 6 |
| **Total** | | **481** | **23** | **110** | **198** | **162** |

What the owner saw in the Liverpool Street throat is mostly **N**: 12 of box A's 36 numbers sit across the
next platform road (the column of signals LA11 … LA43 at x ≈ 260 px), because the number is placed beyond the
disc, which is already 9 px off its own track, and the roads there are about 25 px apart. Text-on-text in the
throat is small (4 pairs). At whole-layout zoom the problem is T and D: ○A buttons and their letters (57 on
Liverpool Street, 97 on Gretz) and TS2 labels pile onto each other, since numbers are already hidden there.
At 2× and 4× Fit almost everything is clean except the ○A letters (A), which sit on the track at every zoom.

### 3.3 Design

All of it is client-ui drawing; nothing else changes.

1. **○A follows the numbers (P5).** `hit::auto_button` returns `None` when `number_px(cam.scale)` is `None`,
   so the button is neither drawn nor hit while numbers are hidden. Its `A` is placed outward from the circle
   (left of travel, away from the track) instead of level with it.
2. **A placement pass after `draw` (P2–P4, P6).** `draw` stays pure and keeps emitting every text at its
   preferred spot. It now also records, for each text that may move, its *role* and its alternative spots,
   and the things to keep clear: every track bar and points leg (with its width), every lamp, the second-lamp
   spot, every shown ○A circle, every berth box, every exit square. A new pure function `labels::plan` takes
   the drawing and a text-measuring function and decides, greedily in priority order (ties in drawing order),
   for each text: its first clear spot, or hidden. `labels::apply` moves and drops texts accordingly.
   - A *clear* spot overlaps no text already placed and touches nothing kept clear.
   - Own signal numbers try their own spot beside the disc (today's), then six others — hugging the track
     behind the post, ahead of the lamp (past its ○A if any), one row further out behind and ahead, and the
     two mirror spots on the other side of the track — then nudges of their own spot by up to one text
     width or height. If none is clear, the first that only touches track bars is used ("tight");
     otherwise the number is hidden.
   - Fringe signal numbers, labels and platform numbers take a clear spot or are hidden; labels may be
     nudged by up to half/one text height and width, line names by up to one line height vertically
     (their arrow stays). A platform number must also fit inside its block.
   - Headcodes are never moved or hidden: they are state. Their knock-out boxes (kept clear) are as wide
     as the layout's longest booked headcode (P13).
3. **Static and cached (P3).** `plan` sees no train state (headcodes are not inputs; every berth box is kept
   clear), and no screen edge, so its result depends only on the scene, the scale and the numbers setting.
   `UiApp` caches the decisions under (game, layout generation, scale, numbers on/off) and re-plans only on
   a zoom or a settings change. Texts that a later feature pushes without placement data (for example the
   tutorial's highlights) are drawn as they are.
4. **Measuring text.** `screens.rs` measures with the frame's fonts (`ctx.fonts_mut(… layout_no_wrap …)`);
   tests use either the same egui fonts (legibility test) or a fixed-advance measure (unit tests).

This algorithm was implemented in a scratch copy (not committed) with the plan's tests; the "after" numbers
below are that implementation's, measured exactly as the baseline.

### 3.4 Success criterion (P1)

Asserted by `crates/client-ui/tests/legibility.rs` over all 66 renders (11 views × {Fit, 2×, 4×} ×
{1280 × 800, 1920 × 1080}), after placement:

1. **T = 0** everywhere, ○A letters and headcode boxes included (baseline 481 at 1280 × 800 Fit alone).
2. **L = 0, A = 0, and D = 0** everywhere: no label, line name, platform number or ○A letter touches a track
   bar, lamp, ○A circle or berth box; no signal number touches a lamp, ○A circle, berth box or text.
3. At 1280 × 800 Fit, in every box view, **every own signal number is drawn** (none hidden), and the
   numbers drawn tight against a track bar total **≤ 4** over all 11 views (baseline N = 23; the scratch
   implementation gave 2: Liverpool Street B 1, Drain Waterloo 1).
4. The test prints the per-render table (overlaps, covered, tight, hidden, plan time, texts shown by role)
   so the owner can see what culling costs. Scratch implementation, 1280 × 800 Fit (shown / drawn before):

   | View | Labels | Line names | Platform nos. | ○A letters |
   |---|---|---|---|---|
   | Liverpool St spectator | 20 / 31 | 0 / 8 | 0 / 21 | (○A hidden) |
   | Liverpool St A | 21 / 21 | 0 / 2 | 18 / 19 | 17 / 28 |
   | Liverpool St B | 2 / 5 | 0 / 1 | 2 / 2 | 12 / 16 |
   | Liverpool St C | 7 / 7 | 4 / 5 | 2 / 2 | 3 / 13 |
   | Drain spectator | 3 / 3 | 2 / 2 | 1 / 4 | 5 / 10 |
   | Drain A / B | — / 2 of 2 | 1 of 1 / 0 of 1 | 2 / 2 each | 2 of 3 / 5 of 7 |
   | Gretz spectator, A, B, C | 8 / 41, 12 / 19, 7 / 13, 6 / 11 | — | 0 (blocks a few px high) | (○A hidden) |

   Every ○A button is still drawn and clickable where numbers are; only letters with no room are dropped.
   Everything returns one or two zoom steps in: at 2× Fit no box view hides an own number at 1280 × 800
   (Gretz box A draws 6 tight in its densest throat).

At 1920 × 1080 Fit the implementation hid 5 numbers on the Liverpool Street spectator view and one each in
Gretz boxes B and C; those are reported, not asserted (they read again one zoom step in). Culling is the price
of criterion 1: the owner should look at the printed table and the browser-check screenshots before accepting
P2–P6.

### 3.5 Tests

- `labels` unit tests on hand-made drawings: priority order, a number moving to its second spot, tight versus
  hidden, a label culled by a number, pan invariance (the same choices after the camera moves), headcodes
  untouched, texts without placement data untouched.
- Paint tests: no ○A (shape or hit) below the number threshold; the `A` outward; the keep-clear lists.
- `legibility.rs` (the committed measurement, §3.4), using real egui fonts.
- Screen test: a real frame of Liverpool Street box A at Fit has no text drawn over another (it had 4); the
  plan is the same after a pan and with or without a train in a berth (a `labels` test).

## 4. Repeat timetable (owner decision 2)

### 4.1 The file (P7)

`layouts/<name>.repeat.json`, applied by `ts2-import --repeat` after `--areas` and `--lines`:

```json
{ "schema": 1, "every": "00:10:00", "until": "14:00:00", "headcode_step": 2 }
```

`deny_unknown_fields`; `every` is 1 minute to 12 hours; `until` is before 24:00:00; `headcode_step` ≥ 1.
Only Drain gets one (Liverpool Street runs to 23:58, Gretz to 11:13). The deploy image converts a layout with
`--repeat` when its file exists.

### 4.2 The rule

A service *s ⊕ P* is *s* shifted by `every` = P: the same train type and calls (place, platform, stop) with
every time P later. In the converted timetable:

1. **Roots.** A service with no *s ⊖ P* in the timetable is a root; every service is root ⊕ jP for one root
   and one j ≥ 0. Its headcode must be the root's with `j × headcode_step` added to the trailing number,
   zero-padded to the same width (Drain: BW01 ⊕ 10 min = BW03). Anything else is a hard error naming it.
2. **Copies.** For each root, copies ⊕ (m+1)P, ⊕ (m+2)P … are added after the timetable's last member
   ⊕ mP, while the copy's first timed call is at or before `until`. Copies keep the train type and calls;
   their headcodes follow rule 1. A headcode that would outgrow its width, collide with another, or a time
   past 23:59:59 is a hard error (so `until` must be chosen to fit).
3. **Ends.** A service's end stays as converted if it is a `form`. Otherwise (and for every copy) it is the
   *steady* end: take the latest earlier-or-equal member root ⊕ iP (i ≤ j) that forms some q ⊕ lP; the
   service forms q ⊕ (l + j − i)P if that exists, else it ends as the root's last converted member ends
   (a non-forming end; Drain: `stable`). This is what turns Drain's three tail services (BW08, WB07, WB08
   `stable`) into workings that continue.
4. **Checks.** No service is formed by two others, and no entry's service is formed by another. Output order:
   the converted services in their order (ends rewritten), then copies by repeat number, then root order.
   Same input, byte-identical output.

On Drain (16 services, 3 trains placed at 05:00) this gives 192 services: the same three trains shuttle
Bank–Waterloo–depot every 5 minutes each way until 14:00, then stable. A scratch run of the robot over the
result gave no SPADs, collisions or stuck trains; at 09:00 BW37, BW36 and WB36 were running; by 14:30 all
three had stabled.

### 4.3 Determinism and saves (P9)

The repeat is input to the converter, so a world is still a pure function of its files; the save copies the
world at creation as before, and replay is untouched. Old saves keep their 16-service timetable.

### 4.4 Tests

Converter: the rule on a hand-made timetable (roots, copies, steady ends, `until`, every hard error: bad
schema, unknown key, no trailing number, step disagreeing with the timetable, overflowing width, a collision,
a time past midnight, a doubly-formed service); Drain's file yields 192 services with BW96/WB96 last and every
end formed except the last three; byte-identical output twice. Soak (non-ignored, debug, like
`drain_runs_its_whole_timetable`): Drain with its repeat file runs 3 sim hours under the robot with no SPADs,
collisions, invariant violations or stuck trains, three trains still running, all with repeated headcodes.

## 5. Old saves show `L…` (owner decision 3)

### 5.1 Why

A save copies the world, `layout` included, when the game is created. Drain saves from before the realism
pass have no `box_prefix` in their `layout`, so `game::display::prefixes` falls back to the first letter of
the title, "London Underground Waterloo & City line": `L`. They also lack line names and arrows.

### 5.2 The migration (P10)

`layout` is "never read by the simulation" (CLAUDE.md, world vs state). So on resume, if a current layout
file is given and its **network** equals the saved one — the JSON values of `areas`, `sections`, `nodes`,
`segments`, `signals`, `berths`, `platforms` and `routes`, compared exactly — the saved world's `layout` is
replaced by the current one before the world is loaded; geometry, prefixes, workstation letters, labels and
simplifier are then built from it as for a new game. Otherwise the saved `layout` is kept and the reason is
logged. The replacement is in memory and repeated on every resume (so later display fixes reach old saves
too); the save file is never written for it. Services, entries and options are not compared and never taken
from the current file, so a save with a custom start time still matches, and the timetable stays the save's.

Determinism: the sim never sees `layout`; a test resumes the same save with and without the refresh and
compares state hashes after running on.

### 5.3 Plumbing

`signalbox-game` gains `--current-layout <world.json>`, valid only without `--create`. The front passes it on
every resume of a save whose layout name it still lists. `game::save::refresh_display(saved, current)` does the
comparison; `Game::resume_with_layout(path, current)` uses it (`Game::resume(path)` stays and passes `None`).
The game process logs one line on stderr: `display data from layout <name>` or `kept the saved display data:
<reason>`.

### 5.4 Tests

`refresh_display`: equal networks → the current `layout` and everything else from the save (start time and
services too); a changed section, route or area name → unchanged with a reason; junk current JSON → unchanged.
Game: a Drain save written with a pre-realism world (the current conversion with `box_prefix`,
`workstations` and line labels removed) resumes showing `WA…`/`WB…` signal names in the layout given the
current file (box prefix `W`), `LA…` without it (`L`), identical sim state either way, and nothing written
back to the save. Process args: `--current-layout` parses
on resume and is refused with `--create`. Supervisor: a resume passes the listed layout's path.

## 6. Real-browser renderer check (owner decision 4)

### 6.1 What was found

Probing `mcr.microsoft.com/playwright/python:v1.55.0-noble`'s Chromium on ra (scratch):

| Flags | `navigator.gpu` | WebGPU adapter | WebGL2 | signalbox drew with |
|---|---|---|---|---|
| none | present | **none** | SwiftShader | `Gl` (the fallback) |
| `--disable-webgpu` (used by the realism check) | present | none | SwiftShader | — (the flag changes nothing) |
| `--enable-unsafe-webgpu` | present | SwiftShader | SwiftShader | `BrowserWebGpu` |
| `--disable-webgl` | present | none | none | fallback page shown |

So plain headless Chromium is exactly the common real-world fallback case (WebGPU API present, no adapter),
WebGPU can be exercised with `--enable-unsafe-webgpu`, and the no-renderer page with `--disable-webgl`.
Headless WebGPU canvases screenshot blank, so pixels can only be checked in the WebGL2 case. The client
already logs `signalbox: drawing with <backend>`. Playwright's `page.route_web_socket` can send a
`create_game` on the page's own socket, which puts the client in a game without clicking at coordinates
(tried: the Drain spectator view drew under WebGL2).

### 6.2 The script (P11)

`deploy/browser-check.sh [--no-build]` (plus `deploy/browser-check.py`), from the repository root:

1. Builds `target/web-dist` (`scripts/wasm-build`), the dev-auth front and game process, and converts Drain
   (areas, lines, repeat) into `target/browser-check/layouts/` (`--no-build` skips the builds).
2. Starts a throwaway front `sbx-browser-check` on `127.0.0.1:${SIGNALBOX_CHECK_PORT:-19162}` from the stock
   Rust image (as the realism check did), removed on exit by a trap.
3. Runs Chromium three times in the Playwright image, each logging in through `/auth/dev?user=check`:
   - **webgl2** (no flags; `--enable-unsafe-swiftshader` so a future Chromium keeps software WebGL): asserts
     the page has no WebGPU adapter (else the case is not exercised: fail), the console says `drawing with
     Gl`, the lobby screenshot is not blank, then sends `create_game drain seed 1` on the page's socket and
     asserts the game screenshot has at least 2000 track-grey (#7d7d7d) pixels and at least 20 headcode-cyan
     (#39e0ff) pixels (three trains stand in the platforms at 06:00).
   - **webgpu** (`--enable-unsafe-webgpu`): the console says `drawing with BrowserWebGpu`; screenshots kept,
     not checked.
   - **none** (`--disable-webgl`): the fallback page is visible, says "could not start" and shows the GPU
     explanation.
   In every case: no `panicked`, no `pageerror`. Screenshots and the console go to `target/browser-check/`.
4. Prints one `ok`/`FAIL` line per case, chowns its outputs back to the caller, and exits non-zero on any
   failure — so it is CI-able wherever Docker and the image exist.

`deploy/README.md` gains the step after `smoke.sh`; CLAUDE.md lists the command.

## 7. Simplifier opens at "now" (P12)

With 192 Drain services the simplifier would open at 06:00 hours into a game. `client_core::simplifier::now_line(rows,
now_s)` gives the first line of the first row whose last timed call is at or after `now_s` (rows with no
times count as not finished); the side panel scrolls there when the simplifier tab is opened, the search
changes or the layout changes, and leaves the scroll alone otherwise. Tests: the function on hand-made rows;
a screen test on repeated Drain at 07:00 where BW01 is scrolled out of view.

## 8. Excluded (and why)

- **A per-game repeat option** in the lobby (`CreateGame.repeat`, front and lobby UI): protocol and front work
  that collides with the tutorial; the per-layout file covers Drain. Follow-up if wanted.
- **Repeat files for Liverpool Street and Gretz**: their timetables run to 23:58 and 11:13.
- **Longer Drain headcodes** (BW100+) to run past 14:00: needs a wider simplifier column and berth box; see P8.
- **Per-layout line-name position overrides, Gretz line names**: the nudging in §3.3 covers the crowding seen;
  the French names are still unknown.
- **Filling TS2's 10-unit signal gaps** in drawn track (needs geometry the data does not have).
- **Shrinking headcode knock-outs** at Gretz's whole-layout zoom (headcodes are state and stay full size).
- **Writing the refreshed display data back into old saves**, and any change to an old save's timetable.
- **Running the browser check in Forge CI** (runner: no Docker, no internet) and on Firefox/WebKit.
- Unchanged from the realism spec §6: exact NR fonts and sizes, detail views, ARS, level crossings, train
  graphs, D2 specifics.

## 9. Risks

- **Tutorial merge.** `crates/server/src/process.rs` and `supervisor.rs` (§5.3) and `crates/game/src/game.rs`
  are edited on both branches; the plan's edits there are a few lines each and must be re-applied against the
  merged code. client-ui is not touched by the tutorial at `b5a9657`, but its lesson highlights may land in
  `paint.rs`; texts without placement data are left alone by design.
- **Placement cost** on a zoom step: measured 3–5 ms native release per plan (Gretz spectator, 248 movable
  texts, the worst), about three times that in debug, once per zoom level (panning reuses it); wasm will be
  slower, so a continuous wheel zoom may stutter on a slow machine. The legibility test prints the times.
- **Scratch versus merged code**: the numbers in §3.4 come from the scratch implementation on `5eb82f0`;
  the plan's legibility test is the judge after the tutorial merge, and its thresholds may need the owner's
  eye if anything there shifts the drawing.
- **Chromium drift**: a newer Playwright image may get a WebGPU adapter by default; the webgl2 case then fails
  loudly ("not exercised") instead of passing vacuously.
