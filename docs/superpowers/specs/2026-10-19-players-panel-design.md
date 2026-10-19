# Players panel: who is in the game, and who works what

Date 2026-10-19. Branch `players-panel`. Scope: small. Born from a local model's
plan ("multiplayer UI"); its ideas were checked against `main` and only the
missing ones are built.

## 1. What `main` already had

| Idea | On `main` | Verdict |
|---|---|---|
| Persistent players panel | Top bar's second row: `Players: West: ann  East: robot` plus Claim buttons. Holders only: no spectators, no count, no connection state, the robot is just the word `robot`. Wraps and shares the row with the vote | Build: a real panel |
| Area ownership on the diagram | None. Other areas' fringe is grey; nothing names an area or its holder | Build: owner strip |
| Spectator banner / overview | A hint in the top bar ("You are watching. Claim an area to signal:") and `spectating` in the title | The hint stays; the overview is the panel's *Watching* line |
| Connection / link status | Own link: the banner (Connecting, Reconnecting, Connection lost, session expired, replaced). Other players' links: nothing | Own link is done. Others' need one protocol field |

The model's code snippets (a `players` field on `InGame`, a lobby `GameInfo.players`
tuple of name and status) do not exist: `GameInfo.players` is names only and lobby-only.

## 2. Overlap with the comms spec and chat plan

`spec-comms` (2026-10-01) and `chat-notes` specify no roster, presence list or players
panel. They do add a CHAT tab to the side panel, a "To" box built from holders, and a Control
seat. Alignment:

- Naming: *seat* is the comms word for "an area or Control". This panel says *area* for what
  exists today; when Control lands it is one more row, `Holder::Player` on a `Seat` instead of an
  area. `Roster::areas` becomes `seats` then (rename only).
- Placement: the panel is a block above the TRAINS / SIMPLIFIER (and later CHAT) tabs, not a
  tab, so chat's unread count and the roster never compete for one tab strip.
- Data: the chat "To" box lists "every other player known from the holders and the lines".
  `View.players` is the authoritative list; the chat plan can read it instead (a one-line
  follow-up there, not done here). Both add a field to `Shared`/`View` beside `holders`
  (`notes` there, `players` here): a trivial merge.

## 3. Protocol: `View.players`

`View.players: BTreeMap<String, bool>` — every player the game knows, `true` = connected.
Holders in their grace period (`GRACE_S`, 120 s) are `false`; a spectator who leaves is
forgotten, as `Game::disconnect` already does. Same list for everyone (it goes in `Shared`).

- Backward compatible: `#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]`. An old
  server omits it; an old client ignores it (no `deny_unknown_fields`). A client with no list
  treats every named holder as present and always lists you.
- Deltas: `Delta.players: BTreeMap<String, Option<bool>>`, sparse like `trains`: `true`/`false`
  = appeared or changed, `null` = forgotten. `protocol::diff` and `View::apply` carry it; a
  seq gap still resyncs the whole list. The 0.2 s flush already diffs per player, so a connect
  or drop reaches everyone on the next flush.
- No save change (presence is not state), no new message, no sim involvement.
- Tests: `crates/game/tests/game.rs` (list, away, grace expiry, reconnect, delta round trip),
  `crates/protocol/tests/{diff,golden}.rs` (sparse delta, absent when empty, pre-panel view
  reads), `crates/server/tests/front.rs::who_is_in_the_game_reaches_every_player` (real sockets).

## 4. UI

**Panel** (`screens.rs::players_ui`): first block of the side panel, above the tabs, never in
a tutorial. `PLAYERS` and a count (`3 players`: connected humans). One line per area, in layout
order: marker, workstation letter, holder, area name (the only part that is cut short in a
narrow panel). Then `Watching:` and the other spectators. A spectator gets `Claim` on each
robot area (moved from the top bar).

- Marker: a filled disc for a player, a hollow one while their connection is down, a small
  hollow square for the robot. Shapes, not glyphs (the bundled font lacks them).
- Colours from the existing palette, nothing new: you `HEADCODE` cyan, other players white,
  away `YELLOW` ("ann away", hover: connection lost, they keep the area up to two minutes),
  robot `FRINGE` grey, labels `LABEL`.
- Top bar: keeps its short `Players:` line only when the panel is hidden (Hide panel) or in a
  tutorial. Otherwise the row holds the vote and the spectator hint; it keeps its height, so a
  vote never moves the diagram (M5). The hint then points at the panel.

**Owner strip** (`paint.rs::owner_marks`): along the top of the diagram, a 1 px rule over each
area's stretch of track (`Scene::areas`, from the layout's sections) with its text above it,
on a black knock-out: `A  Bethnal Green: lucy`, or when that does not fit `A: lucy`, or nothing.
So it respects zoom the way numbers do: a zoomed-out view has no room and the panel says it
all. Overlapping stretches take up to three rows. It keeps clear of the zoom buttons, is
drawn after the labels so `labels::plan` is untouched, and is not drawn in a tutorial or for a
one-area layout. A holder's Fit frames only their own area, so a neighbour's tag shows as they
pan or zoom out; a spectator sees all.

**Setting** (`Settings.owners`, `owners=on|off` in the existing localStorage text, default on):
Settings menu, "Area owners on diagram". The panel has no toggle: Hide panel already is one.

## 5. Left out

A separate spectator banner (the panel's Watching line covers it); a link indicator (the
banner exists and a healthy link needs no mark); a per-area tint (clutter, and it would fight
the track colours, which mean state only); what other players are doing (needs a feed, not
presence; belongs with the comms incident log).
