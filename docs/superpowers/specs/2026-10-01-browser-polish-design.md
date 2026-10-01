# signalbox — browser polish pass (D1.2)

Date: 2026-10-01
Status: approved by the owner 2026-10-01 (P1–P6, P9–P22; P7–P8 withdrawn; P18 amended, P22 conditional). Amended 2026-10-01: the
Drain timetable is the real Waterloo & City WTT (§4), replacing the repeat timetable (P7, P8 withdrawn).
License: GPL-2.0-or-later
Builds on: `2026-10-01-panel-realism-design.md` (D1.1, deployed) and `2026-09-30-browser-client-design.md` (D1).
Base for the plan: **`main` after the `tutorial` branch merges** (`0c0ea67`). Line references in the plan were taken
at `5eb82f0` and must be re-checked; the WTT tasks (4a–4c) and Task 5's screen test were written and run on
`0c0ea67`.

## 1. Goal

Finish the browser client before any desktop (D2) work: a panel that is legible at every zoom, Drain running
the real Waterloo & City timetable for a whole day (when the owner supplies the WTT), old Drain saves that show
today's names, and a real-browser check that the WebGL2 fallback works. Plus one small edge the long timetable
makes urgent (the simplifier opening at "now"). Nothing here changes the simulation, the wire protocol or the
save schema; one rule of the robot signaller changes (P22, §4.6).

The `tutorial` branch is changing protocol, the server front, client-core and client-ui at the same time.
This pass therefore keeps to drawing and layout code, the converter, the robot's standing rule, the game
library's resume path and deploy scripts; its only server change is one game-process argument (§5.3).

## 2. Owner decisions

| # | Topic | Decision | Status |
|---|---|---|---|
| 1 | Clutter | Make the panel legible at every zoom: label collision avoidance / priority culling, ○A hidden below the number threshold, label density by zoom; measure first so the success criterion is a number | owner, 2026-10-01 |
| 2 | Drain timetable | ~~A "repeat timetable" option~~ → amended: the real LU Waterloo & City WTT No. 7 (9 Oct 2017), Mondays–Fridays only, one representative weekday; licence option (c): only the reader/converter is committed, never the PDF or anything made from it, the owner supplies the PDF locally and the image still gets the real timetable, CI tests a synthetic fixture; headcodes are the real train numbers (+ trip where a unique code is needed); replaces P7/P8, keeps P9 (§4.1) | owner, 2026-10-01 (amended the same day) |
| 3 | Old Drain saves | Saves from before the realism pass show `L…` instead of `W…`; decide a migration that keeps determinism | owner, 2026-10-01 |
| 4 | WebGL2 fallback | An automated real-browser check (Chromium via the Playwright image) in a CI-able deploy script | owner, 2026-10-01 |
| P1 | What "legible" means | The measure in §3.1 over 66 renders (11 views × 3 zooms × 2 window sizes); the targets in §3.4 | **owner approved 2026-10-01** |
| P2 | Who wins a collision | Priority: headcodes (never moved), own signal numbers, ○A letters, line names, platform numbers, other labels, fringe signal numbers. A lower item moves or is hidden; nothing is ever drawn over another text | **owner approved 2026-10-01** |
| P3 | Nothing jumps while trains run | Placement depends only on the layout, the zoom and the settings: every berth's box is kept clear whether or not it holds a headcode, and a second (double-yellow) lamp's spot is kept clear on every signal. Panning never re-places anything | **owner approved 2026-10-01** |
| P4 | A signal number that does not fit | Tried at its own spot, six spots round its signal, then nudged by up to one text width or height; if no spot is clear of track, the first clear of text, lamps and boxes is used (it may touch the track: "tight"); otherwise it is hidden (its hover text still names it). An own number is never culled for a label | **owner approved 2026-10-01** |
| P5 | ○A at low zoom | Neither drawn nor clickable whenever signal numbers are too small to draw (below 7 px), for players as well as spectators; zoom in to use it. Its letter `A` moves outward (away from the track) and is dropped where it has no room; the button stays | **owner approved 2026-10-01** |
| P6 | Line names that collide | Nudged up or down by up to one line height; hidden where no nudge is clear. No per-layout position override this pass | **owner approved 2026-10-01** |
| P7 | ~~Where "repeat" lives~~ | Withdrawn: replaced by owner decision 2 as amended (§4) | withdrawn |
| P8 | ~~Drain's pattern~~ | Withdrawn: replaced by owner decision 2 as amended (§4) | withdrawn |
| P9 | Old saves' timetables | Unchanged: a save carries its world and its sim parts are frozen, so an old Drain save still ends at 06:43. New games get the WTT (when the image has it) | owner, 2026-10-01 (kept with decision 2 amended) |
| P10 | Old saves' names | On every resume the game takes the display data (`layout`) from the current layout file when the saved network matches it exactly; in memory only, never written back | **owner approved 2026-10-01** |
| P11 | Where the browser check runs | `deploy/browser-check.sh` on ra (Docker + the Playwright image), run by the controller before every deploy and in this pass's final verification; not in Forge CI, whose runner has neither Docker nor internet | **owner approved 2026-10-01** |
| P12 | Simplifier start | The simplifier opens scrolled to the first train not yet finished, not to the first train of the day | **owner approved 2026-10-01** |
| P13 | Headcode boxes | Every berth's knock-out is as wide as the layout's longest booked headcode (at least today's 34 px): Gretz's 7-character numbers overflowed theirs onto the track and its neighbours (and the WTT's `202/163` would) | **owner approved 2026-10-01** |
| P14 | Reading the WTT | From `pdftotext -bbox` (every word with its box), not `-layout` text, whose 34 stacked fractions break their rows; anything that does not fit is an error naming the page (§4.2) | **owner approved 2026-10-01** |
| P15 | The weekday | Wednesday: the `TThX` variants without Monday's weekend moves (`MO`) or the Monday/Friday late depot run (`MFO`); 585 trips (§4.4) | **owner approved 2026-10-01** |
| P16 | Checks | Running times, train workings, trains in service and service intervals checked before converting; a failure stops the conversion (and the image build). Trains in service at 21:00 checked as **4**, not the printed 3 (the WTT's own workings give 4) (§4.3) | **owner approved 2026-10-01** |
| P17 | Places and what is left out | Waterloo Siding and Depot both map to Drain's roads 5/6/7, booked by the importer; the 10 empty moves and the empty-only trains 206 and 207 are left out; 203 stables at Bank 7 at 23:19¾ instead of running 23:22 to the depot; the last train's call in platform 26 is a timed pass (§4.4) | **owner approved 2026-10-01** |
| P18 | Headcode format | Amended by the owner: each trip keeps a unique internal service code (`<train>/<trip>`, e.g. `201/7`), but the panel, berths, train list, simplifier and enquiry **display only the train number** (`201`), as LU train describers do (§4.4) | **owner approved 2026-10-01 (amended)** |
| P19 | Start and entries | The game starts at 05:40; 203 stands at Bank 8 from the start; depot trains appear in their road 10 minutes before they leave (§4.4) | **owner approved 2026-10-01** |
| P20 | Dwell | The WTT world's minimum dwell is 20–30 s (TS2 Drain: 20–120 s) (§4.4) | **owner approved 2026-10-01** |
| P21 | The PDF and the image | `external/wtt/` (git-ignored but for a README); a `wtt` Docker stage runs `pdftotext -bbox` when exactly one PDF is there, else Drain keeps its TS2 timetable; the layout keeps the name `drain`; the image stays private (§4.5) | **owner approved 2026-10-01** |
| P22 | Robot standing rule (core) | Conditional (owner): the robot-fixes branch (whole-itinerary robot planning, weighted entry delays) lands first; then re-run the whole-day WTT soak. Add this rule only if the Drain still gridlocks, and only if Liverpool Street's 3 h soak does not get worse (longest fringe wait, entries, lateness) (§4.6) | **owner approved 2026-10-01 (conditional)** |

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

## 4. The Waterloo & City Working Timetable on Drain (owner decision 2, amended)

The repeat timetable this section first proposed (P7, P8: `layouts/drain.repeat.json`) is withdrawn. Drain gets
the real London Underground Waterloo & City line timetable instead.

### 4.1 What the owner decided (owner decided, 2026-10-01)

- **Source.** TfL's Working Timetable No. 7, Waterloo & City line, 9 October 2017 until further notice. Only the
  Mondays to Fridays service; the Saturday pages are not read. Where a trip runs on some weekdays only (`MO`,
  `TThO`, `TThX`, `MTX`, `MFO`, `WO`), one representative weekday is used throughout (which one: P15).
- **Licence, option (c).** Only the code that reads and converts the WTT is committed to the public repository.
  The PDF and everything made from it (its text, the parsed timetable, the converted world) are never committed.
  The owner keeps the PDF locally; the release image still gets the real timetable (how: P21). CI tests the reader
  on a small hand-made synthetic fixture in the same textual format, not copied from TfL.
- **Headcodes.** The real train numbers (201–207), as LU train describers show them; train number plus trip where
  the game needs a unique code per trip (P18).
- **Old saves (P9, kept).** A save carries its world, so an old Drain save keeps the timetable it was created with.

### 4.2 Reading the PDF: `pdftotext -bbox`, not `-layout` (P14)

The importer reads `pdftotext -bbox` output: XHTML listing every word with its box. In the `-layout` extract the
train-service tables cannot be read reliably by character position: the WTT prints fractions of a minute as small
trailing digits (`07 3614` is 07:36¼, `12` = ½, `34` = ¾), and the 34 fractions printed as a numerator stacked on a
denominator (pages 6–15) each break their row over several lines with the columns shifted (`06 39 4` with the `3`
on the next line). With boxes every cell is where it is printed:

- A train-service page is one with `MONDAYS TO FRIDAYS` and exactly one of `WESTBOUND`/`EASTBOUND` (and no
  `SATURDAYS`). Row labels sit left of x = 100 pt (`Train No.`, `Trip No.`, `Crew`, `Notes`, `Platform`, `BANK`,
  `arr.`, `dep.`, `Waterloo Siding`, `Waterloo Depot`, `To form`, `By`); each `Train No.` row starts a block.
- Cells (right of x = 128 pt) belong to the row whose label is within 4 pt vertically, else they are an extra notes
  line (`Start`, `MTX`, `Shed Rd`…), and to the column whose train number is centred within 10 pt.
- A time is an `HH` word and an `MM` word 2–5 pt apart, plus a fraction word abutting the minutes (`12`, `14`,
  `34`) or two stacked one-digit words under 4 pt high (1/4, 1/2, 3/4). `23z57` is 23:57 with the train-wash mark.
  Times before 04:00 are after midnight (24:xx). `Stop` in `To form` ends a working; `Pfm 25`/`Pfm 26` in the
  arrival row is a move that starts standing in that Waterloo platform.
- Anything that does not fit (an hour without minutes, a cell in no column, two times in one cell, a `To form`
  that is neither a time nor `Stop`, a note that is neither a day code nor `Start`/`Ety`/`YW`/`Shed`/`Rd`/`RR`)
  is an error naming the page, so a damaged or different PDF stops the build instead of shipping a wrong timetable.

Poppler 22.12 (Debian bookworm, what the image uses) and 25.03 (ra) give byte-identical output for the WTT.

### 4.3 Checks against the WTT's own figures (P16)

Before converting, the importer checks the chosen day's trips against what the WTT says about itself (page 2),
and refuses the file if any check fails:

- **Running times.** Every Bank–Waterloo run takes at least the published time for its Bank platform (westbound
  from platform 7 3½ min, from 8 4 min; eastbound to 7 4¼ min, to 8 4 min). On the real WTT: 515 runs exactly, 61
  given ¼–1 min more (mostly the evening peak's westbound runs from platform 7, at 3¾), none less.
- **Train workings.** Each train's trips chain: every `To form` time is the next trip's start, at the place the
  trip ended, in the other direction; a `Stop` is followed only by a trip marked `Start`. Real WTT: 574 links, all
  consistent.
- **Trains in service** (the "snapshots"): 06:00 1, 09:00 5, 12:00 3, 15:00 3, 18:00 5, 21:00 **4**, 24:00 2. The
  printed table says 3 at 21:00, but the WTT's own workings give 4 (page 5: 201 finishes at 21:37, so 201, 202,
  203 and 205 all run at 21:00), and so does every trip; the table is taken to predate the revision that lengthened
  the evening peak to 19:45. The check uses 4 (**owner approved 2026-10-01**, P16).
- **Service intervals.** The mean interval between Bank departures: morning peak (07:30–09:30) 2¾ min, midday
  (11:00–15:30) 5, evening peak (16:30–19:45) 2¾, 19:45–21:30 3½, 21:30–23:30 6, 23:30–close 10, each within
  6 s. Real WTT: 165, 300, 165, 207, 363 and 600 s. The peaks' windows are chosen by the importer (the WTT does
  not print them).

### 4.4 Into Drain (P15, P17–P20)

**The day (P15): Wednesday.** On the Monday-to-Friday pages the day codes are `TThO`/`TThX` (31 pairs: which Bank
platform the early and late trips use around the train stabled there overnight), `MO` (3: Monday-only empty moves
recovering from weekend stabling), `MTX`, `MFO` and `WO` (1 each). Tuesday and Thursday take the `TThO` variants;
Monday, Wednesday and Friday the `TThX` ones. Wednesday has none of Monday's weekend moves and none of the
Monday/Friday late-night depot run (`MFO`); it starts from Tuesday night's stabling (Bank platform 8, Waterloo
platform 26) and is the plain midweek day. 585 trips by 7 trains (Monday 587, Tuesday 584, Thursday and Friday
585); 575 carry passengers, on trains 201–205.

**Places (P17).** Drain (converted from TS2) has Bank platforms 7 and 8, Waterloo arrival platform 26 and
departure platform 25, and three dead-end roads behind them with platforms `DPT 5`, `6`, `7`, entered from 26
(signal 75) and left for 25 (signals 51, 61, 76). Two more roads (signals 31 and 41) are reached only from 25 and
have no platform, so no service can be booked into them.

| WTT | Drain |
|---|---|
| BANK, platform 7 / 8 | `BNK` 7 / 8, as booked |
| WATERLOO arr./dep. (westbound) | `WTL` 26 |
| WATERLOO arr./dep. (eastbound) | `WTL` 25 |
| Waterloo Siding (stepping back, reversing) | a `DPT` road 5, 6 or 7, chosen by the importer |
| Waterloo Depot, Shed Road, train wash | a `DPT` road too: the train waits there until its next trip |
| 5, 6, 7 Road (overnight) | not used as such: the day starts and ends as below |

A westbound trip is `BNK` (dep) → `WTL 26` (arr, dep) → a road (arr), and forms the next trip; an eastbound trip
is the road (dep) → `WTL 25` (arr, dep) → `BNK` (arr) and forms the next. A depot visit between two trips (204
10:36–16:15, 205 09:41–16:06, the 23:57¾ wash) is one long dwell in a road. The importer books roads from the WTT
times, giving each stay the road that has been free longest (60 s apart at least), so a late train is least
likely to find its road still taken. Real WTT: 290 road stays, 164/73/53 on roads 5/6/7.

**What the track cannot take, and what happens to it (P17).**

| WTT | Why not | In the game |
|---|---|---|
| 10 empty moves (`Ety`): 206 platform 26 → depot 05:35 and its 10:45 shunt to Shed Road; 207 depot → Bank 23:17 to stable; 201 21:32 and 202 00:29 via platform 25 to Shed Road; 204's 00:11 tripcock-test run | Shed Road and the depot roads are not on Drain's panel; seven trains do not fit in three roads | Left out. Trains 206 and 207 run only empty moves on a Wednesday and are left out entirely; 201, 202 and 204 end their day in a road |
| 203's last trip, Bank 23:22 → depot 23:30 | With 204, 201 and 203 stabled in the roads from 23:30, the last two trains (202, 205) would have no road to reverse in until 00:30 | 203 instead stables at Bank platform 7 after its 23:19¾ arrival (booked 8; 7 is free for the rest of the night, where 207 would stable). One passenger trip (23:22) is lost |
| 205 stabling in platform 26 at 00:30¾ (`WO`) | A booked stop there would wait for signal 75 to clear before the train could stable | Its last call at `WTL 26` is a timed pass: it runs in, stops at 75 and stables |
| Stepping back, crew running numbers and reliefs | Crew, not trains | Not represented (the timings already include them) |
| The radio alarm test (`YW`) | | Its 5 extra minutes are already in the times |

Converted: 574 services, 5 entries, every road stay booked.

**Headcodes (P18).** `<train>/<trip>`, e.g. `201/7`, `202/163`: the train number first, as the describer shows,
and the trip so every service is unique (the world loader requires it). 5–7 characters; P13 sizes Drain's berth
boxes to fit them. Typing `201/` in the simplifier search lists one train's day.

**Start and entries (P19).** The game starts at 05:40 (10 minutes before the first train leaves the depot, on the
5 minutes). Train 203, stabled at Bank platform 8 overnight, is there from the start. Trains starting from the
depot appear in their road 10 minutes before they leave (202 05:43, 204 05:55, 201 06:01, 205 06:30), so the depot
holds no more trains than the roads can. Entries cannot be placed in platform 26 (the automatic route 74–75
always owns its section), which is another reason 206's 05:35 move is left out.

**Dwell (P20).** The WTT turns trains round in 1–1½ minutes (stepping back). The world's minimum dwell becomes
20–30 s (TS2's Drain has 20–120 s, which makes those turnarounds impossible: with it the WTT ran up to 1¾ hours
late and two trains were stuck by the evening).

**Same layout, same name.** The converted world is still `drain` (the owner's "into Drain"). Its network and
`layout` are unchanged, so P10's comparison still matches old Drain saves; only services, entries and options
differ, which P10 never takes from the current file.

### 4.5 Where the PDF lives and how the image gets it (P21)

- `external/wtt/` is in the repository with only a `README.md` (what to put there, the PDF's sha256, the private-
  image rule) and a `.gitignore` that ignores everything else, so the owner's PDF and any text made from it can sit
  in a checkout without being committed.
- `deploy/Dockerfile` gains a `wtt` stage (`debian:bookworm-slim` + `poppler-utils`) that copies `external/wtt/`
  and, if it holds exactly one PDF, prints its sha256 and runs `pdftotext -bbox` (two PDFs fail the build; none:
  one log line, "Drain keeps its TS2 timetable"). The build stage converts Drain with `--wtt` when that text
  exists. A WTT that fails the checks fails the build.
- The image then holds a timetable made from TfL's document. It stays on ra (`local/signalbox:<rev>`) and is never
  pushed to a public registry; `deploy/README.md` and `external/wtt/README.md` say so.
- `ts2-import --wtt <wtt.bbox.html>` applies after `--areas` and `--lines` and prints what it did (trips, checks,
  services, what was left out, the road use, the 203 change).

### 4.6 The robot rule the WTT needs (P22, a core change)

**Finding.** With today's robot the WTT gridlocks at 06:52, as the morning peak builds. Drain's westbound line is
three 700 m blocks (73, 74, 75), and the last one also holds platform 26. The robot gives a train its routes only
when it can set them all the way to its next stop, or to a signal where it would stand clear of track that routes
from other signals use. Signal 73 never qualifies: its block is entered by routes from both Bank platforms (72 and
82). So a train leaves Bank only once the train ahead has left platform 26 for a road: about 5 minutes apart
(the run, the minute's stand in 26 and the move into a road), against the WTT's 2¾. Trains queue at Bank, and five trains with booked platforms and roads lock each other in a
circle (Bank 7 → platform 26 → road 5 → platform 25's overlap → the eastbound line → Bank 7).

**Rule.** A train may also stand at an **automatic** signal when every section it would stand on is plain line (no
points) and every route over those sections ends at that signal. It then blocks nothing that could go anywhere
else: it waits in a block section, as on any plain line. (`robot::may_stand`; the existing rule is unchanged
otherwise.) Drain's trains now wait at 73 while platform 26 clears.

Measured (3 sim hours from each layout's start under the robot alone, as `sim-cli run --robot`; scratch runs).
The core, converter and game tests, the Liverpool Street soak and the bot soak all pass with it:

| | Liverpool Street | Gretz | Drain, real WTT (19 h) |
|---|---|---|---|
| Today's robot | 71 entered, 58 exited, no stuck, longest fringe wait 686 s | as below | gridlock 06:52, 2 stuck |
| With P22 | 76 entered, 60 exited, no stuck, longest fringe wait 997 s | identical state hash | clean, see §4.7 |

Two further robot changes were tried and are **not** proposed: falling back to another platform at the same place
when the booked one is taken, and serving trains in timetable order. Without P22 they avoid the gridlock but leave
the peaks up to 56 minutes late; with P22 they bring the worst stop to 46 s late, at the cost of 146 platform
changes a day against the WTT's bookings. They would make the robot sturdier when players delay trains (§9).

### 4.7 Prototype results (scratch copy of `main` 0c0ea67, not committed)

The owner's PDF (sha256 `7709d5b5…2475b`), read and converted as above, run from 05:40 to 01:00 under the robot
with P22, seeds 1, 2, 3, 7 and 42 (they differ only in dwell times; the results are the same to the second):

- 0 SPADs, 0 collisions, 0 invariant violations, 0 stuck trains, 0 wrong platforms; all five trains stabled by
  01:00 (201, 202, 204 in roads, 203 at Bank 7, 205 in platform 26).
- 1,721 timed stops: 137 (8 %) more than a minute late, the worst +103 s (204/73 at 19:45:42); 1,147 timed
  departures, the worst +109 s. All the late running is in the peaks (06:45–10:00 and 16:00–19:50, up to
  1½ minutes, from trains waiting at 73 for platform 26); midday and evening run to time or early.
- Entries: 203 at Bank 8 from 05:40; 202, 204, 201, 205 appear in roads 5, 6, 7, 5 as listed in §4.4.

### 4.8 Determinism and saves (P9)

The WTT is input to the converter like the areas and lines files: the world is a pure function of its files
(converted twice, byte-identical), the save copies it at creation, and replay is untouched. Old saves keep their
16-service timetable; P10 still refreshes their display data (the network is unchanged). P22 changes only which
routes the robot asks for from now on; a resumed save replays its logged commands as before.

### 4.9 Tests

- CI (`crates/ts2-import/tests/wtt.rs`, on `tests/data/wtt-synthetic.bbox.html`, written by the committed
  `wtt-synthetic.py`: fictional trains 301–303, both directions, a contents page and a Saturday page that must be
  skipped, every notation above): every column read with its times, fractions, stacked fractions, wash mark,
  `Stop` and `Pfm`; day codes pick a weekday and an unknown one is an error; the checks pass, and fail for a wrong
  snapshot, a run under the published time and a broken `To form`; the day goes into Drain (services, calls,
  roads, entries, the platform-26 pass, byte-identical twice); the converted day runs an hour under the robot with
  no incident; a non-timetable and a garbled time are refused. CLI: `--wtt` refuses the synthetic WTT (it fails the
  Waterloo & City checks) and writes nothing; `--wtt` needs a value.
- CI (`tests/soak.rs`): on Drain, a train leaves Bank for signal 73 while platform 26 is occupied (fails with
  today's robot).
- Owner-run (`tests/wtt_day.rs`, `#[ignore]`, skipped without `external/wtt/wtt.bbox.html`): the real WTT, the
  whole day, five seeds: no SPADs, collisions or stuck trains, all stabled by 01:00, no stop or departure more
  than 3 minutes late.

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
   (areas, lines; its committed TS2 timetable, never the WTT) into `target/browser-check/layouts/` (`--no-build`
   skips the builds).
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

With the WTT's 574 Drain services the simplifier would still open at 05:40, hours into a game. `client_core::simplifier::now_line(rows,
now_s)` gives the first line of the first row whose last timed call is at or after `now_s` (rows with no
times count as not finished); the side panel scrolls there when the simplifier tab is opened, the search
changes or the layout changes, and leaves the scroll alone otherwise. Tests: the function on hand-made rows;
a screen test on Drain's TS2 timetable at 06:30 where BW01 is scrolled out of view (the WTT is not in CI).

## 8. Excluded (and why)

- **The repeat timetable** (P7, P8): withdrawn for the real WTT.
- **The other weekdays and Saturday** as choices (a per-game or per-layout weekday): one representative weekday
  (P15) this pass; the reader already selects by day, so a later choice is a small change.
- **Drain's missing pieces of the real Waterloo**: the depot roads, Shed Road and the train wash as places of their
  own, and the 10 empty moves that use them (P17); they need track the TS2 layout does not have.
- **Re-signalling Drain** (shorter blocks, platform 26 its own section) to carry the peaks without the late running
  §4.7 shows: fictional signalling and a different network (P10 would no longer match old saves).
- **Robot platform fallback and timetable-order fairness** (§4.6): measured, not proposed.
- **Crew working** (running numbers, reliefs, stepping back) and the radio alarm test as events.
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
- **The WTT's margins are thin on Drain.** Even with P22 the peaks run up to 1¾ minutes late under the robot,
  and they are sensitive to dwell (P20): trains held longer — by players, or the robot holding a road a late train
  is booked into — can still lock five trains in a circle in a robot-held area (seen in scratch runs with long
  dwells). Players can always break it by using the other platform or road. The two robot changes in §4.6 that
  avoid it are the follow-up if it bites.
- **P22 changes the robot for every layout.** The soaks pass and Gretz is unchanged, but Liverpool Street's
  longest fringe wait over 3 hours rose from 686 to 997 s (while more trains got through: 76 entered, 60 exited,
  against 71 and 58).
- **The WTT reader fits WTT No. 7.** Another WTT (a later number, another line) may lay its pages out differently;
  the checks and the strict reader stop the build rather than guess. Its constants (label and data columns,
  tolerances, the W&C checks) are in one place (`ts2_import::wtt`).
- **Licence.** The image built with the PDF holds a timetable made from TfL's document: it must stay on ra and
  never be pushed to a public registry (`deploy/README.md`, `external/wtt/README.md`). The repository has only
  the reader, a synthetic fixture and its generator.
- **Headcode width on Drain.** `202/163` is 7 characters, as wide as Gretz's numbers; the legibility measurement
  (§3.4) runs on the committed TS2 timetable (4 characters) in CI, so the deployed Drain's wider berth boxes are not
  measured there. The browser check's screenshots (TS2 timetable) do not show them either.
