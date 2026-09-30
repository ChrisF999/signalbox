# signalbox — panel realism pass (D1.1)

Date: 2026-10-01
Status: draft for review
License: GPL-2.0-or-later
Builds on: `2026-09-30-browser-client-design.md` (D1). Replaces its §3.1 drawing rules where they differ.

## 1. Goal

Make the signaller's screen look and behave as close as practical to a real UK IECC / Westcad
workstation, with two deliberate, switchable concessions for playability. Research and sources:
`docs/superpowers/research/2026-10-01-uk-vdu-conventions.md` (RAIB report photos of the Thames Valley IECC and an
Invensys workstation; SimSig's documented IECC style; NR/RSSB standards are paywalled, so fonts and
exact sizes stay approximate). The comparison the owner chose from:
https://claude.ai/artifact/Kt6TDwNEWY8ecY68dnrKF9

### Owner decisions (2026-10-01)

| # | Topic | Decision |
|---|---|---|
| 1 | Signal colours | Toggle: red (on) / green (off) as on real IECC, or real aspects (R, Y, YY, G). Default: red/green. The real aspect shows in hover text in both modes |
| 2 | Track | Thick bars with small gaps at every track-circuit (section) joint; points show the lying leg by a gap in the other leg; locked points ends white |
| 3 | Overlaps | White like the route, with an end-of-overlap tick |
| 4 | Signals | Disc on a hooked post, on the correct side of the line, facing the direction of travel; the entrance signal's post turns white while a route is set from it; automatic signals get a distinct (dashed) post; signal numbers always drawn beside every signal |
| 5 | Train describer | Cyan headcodes drawn inside the track in the section before their signal; empty berths invisible; instant step |
| 6 | Auto-working and labels | Blue ○A button by each automatic signal (hollow off, filled on, clickable = the existing auto-working menu command); uppercase grey labels, with a direction arrow where the layout gives one; ochre platform blocks with the platform number inside |
| 7 | Fringe | Track outside the player's area drawn hollow (outline only), not dimmed; fringe signals, numbers and headcodes grey rather than dimmed colours |
| 8 | Booked platforms | A simplifier panel (always available); click a headcode for its schedule as a toggle. Default: off |
| 10 | Direction arrows | Automatic arrows on every running line from signal facing, plus optional hand-authored line names (`UP MAIN`, `DOWN MAIN`) per layout |
| 11 | Signal prefixes | Both: a box prefix per layout and a workstation letter per area, e.g. `LA121`; single-area layouts omit the workstation letter |
| 9 | Flashing | Only for transitional or abnormal states: points moving / not detected, the selected entrance, a cancelled route whose approach locking is timing out. Rejected-command feedback becomes a steady alarm line plus a brief outline on the signal, not a flash |

## 2. Drawing (client-ui)

- Background pure black. Colour only ever means state.
- Idle track `#7d7d7d`, route and overlap `#ffffff`, occupied `#e8141c`, headcode `#39e0ff`, auto button
  `#1d4fd8`, platform `#b8860b`, labels `#9a9a9a` (sampled from the research sources; tuned by eye).
- Track thickness about 3× today's, scaled with zoom within limits; joint gaps at every section
  boundary, drawn by shortening each section's line ends.
- End-of-overlap tick: a short perpendicular white stroke at the overlap's far end.
- Sectional route release shows naturally: sections go white → red → grey as the view changes.
- Signal symbol: post from the track to the disc, hooked towards the direction of travel, disc on the
  side of the line the signal stands (left of the direction of travel). Signal numbers in grey mono
  beside the disc; hidden only below a minimum on-screen text size (about 7 px).
- Headcodes: cyan mono text on a black knock-out drawn over the track in the berth's section; nothing
  drawn for an empty berth. Fringe headcodes grey.
- Fringe track: outlined, not filled. Fringe elements never clickable except a fringe signal or exit
  that ends one of the player's routes (D1 §3 exception, unchanged).
- Direction arrows (owner decision 10): every running line gets small grey direction-of-travel arrows,
  derived from the facing of the signals along it (a line whose signals face both ways, e.g. a single line,
  gets a double arrow), placed at the ends of the player's visible track and at intervals along long
  stretches. Line names are optional and hand-authored per layout (§2.1).
- Signal prefixes (owner decision 11, "both"): each layout has a box prefix, and each player area is a
  workstation with its own letter. A signal is displayed as `<box><workstation><number>` (e.g. `LA121`,
  `LB72`, `LC101` on Liverpool Street); a single-area layout omits the workstation letter (`L121`).
  Display only: wire names, commands, saves and the areas file's boundary/seed names keep the plain TS2
  names. Hover text, menus, alarms, the simplifier and the enquiry window all use the displayed form. The
  top bar shows the workstation, e.g. `Workstation B · Bethnal Green`.

### 2.1 Per-layout display data

- Prefixes live in the layout's areas file (`layouts/<name>.areas.json`, applied by `ts2-import
  --areas`): an optional top-level `"prefix"` (the box, 1–3 capital letters) and an optional per-area
  `"workstation"` (one capital letter). Defaults: the box prefix is the first letter of the layout's title;
  workstation letters are A, B, C… in file order. ts2-import writes them into the world (areas carry their
  workstation letter; the world carries the box prefix), and the game sends them in the `Layout`
  (`Layout.box_prefix`, `AreaInfo.workstation`, `#[serde(default)]`).
- Line names come from an optional `layouts/<name>.lines.json`, applied by `ts2-import --lines`: a list of
  `{ "name": "UP MAIN", "direction": "up"|"down", "through": [signal or section names] }`; the converter
  writes each as a label with an arrow at the named stretch's ends in the world's `layout` labels. Unknown
  names are hard errors, like the areas file. The owner's three layouts get hand-written line files
  (Liverpool Street, Drain, Gretz), using the TS2 data's own hints (e.g. `SL_DN` / `SL_UP` platform names);
  where the real line names are not known, those lines get arrows only.
- Defaults chosen for the shipped layouts (editable in the areas files): Liverpool Street `L` (A Liverpool
  Street, B Bethnal Green, C Hackney & Bow), Drain `W` (A Bank, B Waterloo), Gretz `G` (A Gretz, B Tournan
  & Marles, C Mortcerf & Coulommiers).

## 3. Simplifier and headcode enquiry

- The server adds the timetable for the player's area to the `Layout` (static per game and area): for
  every service that calls at or passes a platform in the player's area (spectators: every service),
  its headcode, origin, destination, and each call in the area (place, platform, arrival, departure,
  stops or passes). Built once per game and area, like the geometry. Protocol: `Layout.simplifier:
  Vec<SimplifierRow>`, `#[serde(default)]` so older clients still read.
- Simplifier panel: a tab beside Trains and Alarms, listing rows in time order of first call in the
  area, styled like the working timetable (`HH:MM` for times, half-minutes as `½`, platform column,
  `pass` for passing calls). A search box filters by headcode. Live lateness from `View.trains` is shown
  beside the row when the train is running.
- Headcode enquiry (toggle, default off): clicking a headcode on the diagram or in the train list opens
  a small window with that train's simplifier row plus its live state and lateness. Clicking a headcode
  never selects or routes anything.

## 4. Settings

- A Settings menu in the top bar with: Signal aspects (red/green · real), Headcode enquiry (off · on),
  Signal numbers (on · off, default on). Remembered per browser (eframe storage: localStorage on the
  web, a file natively for D2).

## 5. Testing

- Headless paint tests for each rule above (joint gaps, overlap tick, disc and post side and hook
  direction, white entrance post, dashed automatic post, numbers placement and hiding, cyan in-track
  headcodes and invisible empty berths, hollow fringe, ochre platforms, auto button states, flashing
  only in the listed states, both signal-colour modes).
- Protocol golden JSON for `simplifier`; game builds it for area and spectator; a Layout without it
  still reads.
- Screen tests: simplifier tab, search, enquiry window only when the toggle is on, settings persist.
- Glyph check still covers every string on screen (½, ○, arrows).

## 6. Out of scope

Exact NR fonts and sizes (paywalled standards); detail views for track-circuit IDs; ARS; level
crossings (no layout has them yet); train graphs; D2 desktop specifics beyond reusing these settings.
