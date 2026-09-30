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
- Direction arrows: TS2 labels carry no direction, so arrows appear only where a label's text already
  contains one; no arrows are invented.
- Signal numbers are the layout's own names (TS2 names), with no invented box prefix.

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
