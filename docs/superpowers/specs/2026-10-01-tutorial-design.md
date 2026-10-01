# signalbox — interactive tutorial (D1.2)

Date: 2026-10-01
Status: approved (owner, 2026-10-01); implemented on branch tutorial
License: GPL-2.0-or-later
Builds on: C (server/protocol), D1 (browser client), D1.1 (panel realism)

## 1. Goal

Teach new players the basics of UK panel signalling inside the game itself: a Tutorial list in the
lobby offering four interactive lessons, each a private single-player game on a small purpose-built
layout, guided one step at a time with highlights, advancing when the game sees the player has done
what was asked.

### Owner decisions (2026-10-01)

| Topic | Decision |
|---|---|
| Format | Interactive lessons in private tutorial games (not an overlay on real games, not a static page) |
| Lessons | 1 Reading the panel · 2 Setting & cancelling routes · 3 Running trains · 4 Junctions, auto-working & handovers |
| Authoring | Lessons are data files (a small world + a step script); the game process runs the script |

### Success criteria

1. A new player can complete all four lessons in the browser without outside help.
2. Every lesson is played through automatically in CI and still completes after later changes.
3. Tutorial games are private, never listed, never joinable by others, and cleaned up on leave.
4. A broken lesson file never crashes the server: it is left out of the list with a logged reason.

## 2. Lesson files

`lessons/<nn>-<slug>/` (repo root, copied into the image next to the layouts):

- `world.json` — a hand-built world (core world format) with one or two areas, layout geometry, and
  whatever timetable the lesson needs (often none). Display data (box prefix, workstations, line names)
  in its `layout` JSON like converted worlds.
- `lesson.json`:

```json
{ "schema": 1, "title": "Setting & cancelling routes", "area": "Westbox", "start": "08:00:00",
  "steps": [
    { "say": "This is the entrance signal WA1. Click it.", "highlight": [{"signal": "W1"}],
      "wait_for": {"selected": {"signal": "W1"}} },
    { "say": "Now click WA3 to set the route.", "highlight": [{"signal": "W3"}],
      "wait_for": {"route_set": {"entrance": "W1", "exit": {"kind": "signal", "name": "W3"}}} },
    { "say": "…", "do": [{"spawn": {"headcode": "1A01", "entry": "E"}}, {"run": {}}],
      "wait_for": {"train_at": {"headcode": "1A01", "place": "WST", "platform": "2"}} }
  ] }
```

Steps may also carry an optional `solution` — the player commands that complete the step (e.g.
`[{"set_route": {...}}]`), used only by the CI play-through when it can't be derived from `wait_for`.

Highlights: `signal`, `exit` (signal or node), `points`, `berth`, `section`, `platform`, or `ui`
(`settings`, `simplifier`, `trains`, `clock`, `auto:<signal>`).

Conditions (`wait_for`): `continue` (player presses Next); `selected {signal}`;
`route_set {entrance, exit}`; `route_cancelled {entrance}`; `points {name, position}`;
`auto_working {signal, on}`; `train_at {headcode, place, platform}`; `train_passed {headcode, signal}`;
`train_left_area {headcode}`; `berth {name, headcode}`; `rejected {}` (the player's last command was
refused by the interlocking); `clock {paused}`; `tab {simplifier}` (client-reported UI state);
`all [conditions]`.

Actions (`do`, run when the step starts, in order): `spawn {headcode, entry}` (a lesson-defined entry in
the world), `pause`, `run`, `speed {x}`, `set_route {entrance, exit}` and `cancel_route {entrance}` (the
lesson demonstrating), `interpose {berth, headcode}`.

Validation at front startup (and in a CI test over every shipped lesson): schema, every name resolves
against the lesson's world, every condition/action is well formed, the area exists, every step can
complete (no `selected` on a signal the player can't work, etc.). A failing lesson is omitted from the
Tutorial list with one logged line naming the lesson and the reason.

## 3. Running a lesson

- Lobby: a Tutorial section listing the valid lessons (title, step count), ticked when completed (ticks
  kept in the browser via the existing settings store mechanism, key `signalbox.lessons`).
- `start_lesson {lesson}` (new lobby message) creates a private tutorial game: the front spawns the game
  process with `--lesson <dir>` instead of a layout. The game is visible and joinable only by its
  creator, never appears in `games`, is not counted against `MAX_LIVE_GAMES` but against its own cap
  (`MAX_TUTORIALS = 8`), saves nothing durable (its save file is deleted when the game ends) and exits
  when its player leaves (no 10-minute empty wait).
- The player is placed as holder of the lesson's area on join; any other area is robot-run. Penalty
  scores are hidden for tutorial games.
- The game process runs the script: at each step start it records a snapshot (core snapshot/restore),
  runs the step's actions, then evaluates the condition after every tick and every handled command
  (client-reported UI conditions arrive as a new client message `lesson_ui {tab}` / `lesson_next`).
  When met, it advances. Messages to the client: `lesson {index, count, title, say, highlight,
  needs_next, done}` (new `ServerMsg`), sent on join/resync and on every step change.
- Restart step: `lesson_restart_step` restores the step's snapshot and re-runs its actions. Restart lesson:
  `lesson_restart` restarts from step 0. A SPAD or collision during a lesson sends a friendly lesson
  notice offering Restart step.
- Off-script play is allowed: the current step simply keeps waiting.

## 4. Client

- Lesson panel (replacing the side panel's tabs while in a tutorial, Trains/Alarms still reachable below):
  title, "Step n of m", the `say` text, Next (only when `needs_next`), Restart step, Restart lesson,
  Leave. On `done`: a completion message and "Back to tutorials", and the lesson is ticked.
- Highlights: a pulsing outline (a calm flash is allowed here — it is UI, not panel state) around the
  highlighted element on the diagram or around the named UI control.
- `tab {simplifier}` and similar UI conditions are reported by the client when the player does them.

## 5. The four lessons (content)

1. **Reading the panel** (~8 steps): the screen layout; hover a signal, a berth, a platform; track
   colours; a train appears and its track turns red; open Settings and switch to real aspects and back.
2. **Setting & cancelling routes** (~10): entrance then exit; why only some exits light up; cancel a route;
   try a conflicting route (refused — explained); swing points by hand; set a route over reversed points.
3. **Running trains** (~10): a train is due (train list and simplifier); route it into its booked
   platform; watch sectional release behind it; route it out; lateness.
4. **Junctions, auto-working & handovers** (~12): conflicting moves at a junction; a plain-line signal
   with ○A on while two trains follow; hand a train over to the robot's box; the clock and voting.

Text is plain English, short sentences, UK terms explained the first time they appear (entrance, exit,
overlap, SPAD, headcode, berth, simplifier, auto-working).

## 6. Testing

- Lesson format: parse/validate tests per condition and action; validation failures (unknown names,
  bad area) produce the logged omission, not a crash.
- Game: conditions evaluated correctly against sim state; actions; Restart step restores the snapshot;
  privacy (another user can't join, not listed, deleted on leave); separate tutorial cap.
- Every shipped lesson is played through in CI by a scripted player that performs each step's expected
  action (derived from its `wait_for`, plus an explicit `solution` hint where needed) and must reach
  `done` within a sim-time limit.
- Client: lesson panel, Next, highlights drawn, ticks persisted, UI conditions reported.
- After deploy: the owner plays all four in the browser.

## 7. Out of scope

Voice-over or animation; an in-game lesson editor; translations; scores or certificates; tutorials on the
desktop client beyond what D2 inherits from the shared UI.
