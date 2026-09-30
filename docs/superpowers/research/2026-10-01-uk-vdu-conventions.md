# UK signalling VDU conventions (IECC / IECC Scalable / Westcad) — research notes

Researched 2026-10-01 for the signalbox signaller-screen look. Read-only research; no code changed.

## How much to trust each source

- **Primary visual evidence (best):** RAIB report 28/2012 (Ufton AHB). It has colour photos of the real **IECC** VDUs at Thames Valley Signalling Centre (Reading), 2011, plus RAIB's symbol key (Fig. 6). RAIB report 11/2013 (Lindridge Farm) has a reproduction of the **Invensys (Westcad-family) workstation view** at East Midlands Control Centre, Derby (Fig. 8/9). I sampled the colours in the figures below from the PDFs.
  - Ufton: https://assets.publishing.service.gov.uk/media/547c8fd8e5274a428d000151/R282012_121220_Ufton.pdf (pp. 11, 12, 26, 27)
  - Lindridge Farm: https://assets.publishing.service.gov.uk/media/547c8fcbe5274a428d000149/R112013_130729_Lindridge_Farm.pdf (pp. 11, 14, 15)
- **Standards (not openly readable):** RSSB requires a login for the full text of every relevant Railway Group Standard, so I could only read catalogue metadata. What they cover:
  - BR1878 Iss B (1987) / Iss C (1992): *Operating Specification for the use of VDUs for Railway Signalling Control and Indication Purposes*. Withdrawn 1999. https://www.rssb.co.uk/standards-catalogue/CatalogueItem/BR1878-Iss-C
  - GK/RT0025 Iss 1 (1997) / Iss 2 (2003): *Signalling Control and Display Systems*. Iss 2 withdrawn 2011. Iss 2 partly replaced GK/RT0005, *Safety Related Colours for Signalling Application*. https://www.rssb.co.uk/standards-catalogue/CatalogueItem/GKRT0025-Iss-1 , https://www.rssb.co.uk/standards-catalogue/CatalogueItem/GKRT0005-Iss-2
  - NR/SP/SIG/10067 (RT/E/S/10067): VDU-based Signalling Control System spec. It requires the signaller's interface to conform to GK/RT0025.
  - NR/SP/SIG/17504: *IECC Operating Specification for Signalling Control and Indications Purposes*.
  - NR/SP/SIG/10024: *Signallers Operating Guide for the Use of the IECC Signalling Workstation*. (Titles from GlobalSpec search listings; full text paywalled/403.)
  - "GK/RT0045" and "NR/L2/SIG/10158" did not turn up as VDU-presentation standards. NR/L2/SIG/11201 is the *Signalling Design Handbook* (plan symbols), not VDU presentation.
- **SimSig (good proxy, with caveats):** The SimSig wiki says its symbols are "in most cases identical to those used in real life" (https://www.simsig.co.uk/Wiki/Show?page=usertrack:ssrun:signalling_display). Didcot Railway Centre uses SimSig to show "an exact copy" of the Didcot IECC workstation screens (http://www.gwsbristol.org/signalling_centre_signalcentre.html). SimSig also says it "has tried to keep to the spirit of the IECC displays while adapting for a scrollable layout" (https://www.simsig.co.uk/Article/Details/169). SimSig's own icon PNGs were sampled for exact RGB values.
- **Panel-era background:** Swindon Panel reference (https://reference.swindonpanel.org.uk/index.php/The_Panel_Itself) and Rail Engineer, "Evolution of signalling control", D. Bickell, 21 May 2013 (https://www.railengineer.co.uk/evolution-of-signalling-control/). VDU conventions were designed to "replicate all the functions of the NX panel" (Rail Engineer), so the panel conventions carry over.

**System differences in short:** IECC (Classic and Scalable) sets the reference look: black background, grey track, white route, red occupation. The Invensys/Westcad-family view seen at EMCC uses a **dark slate-grey background** (sampled #494650) with light-grey track (#dbdada). Other symbols look similar, but I found far less Westcad evidence. A SimSig forum post and an RMweb summary say Westcad and GE's MCS "have the look and feel of an IECC" (RMweb Collingwood IECC thread, via search snippet: https://rmweb.co.uk/forums/topic/147816-collingwood-iecc-creating-a-prototypical-iecc-for-a-model-railway — the page itself returned 403). I found nothing substantive on Hitachi/Thales or Siemens Controlguide ROC-era screen colours. Treat those as "IECC-like, unverified".

---

## 1. Background and track colours

| State | Convention | Evidence |
|---|---|---|
| Background | **Black** on IECC. **Dark grey** on Westcad-family (EMCC sample #494650). A SimSig forum poster speculates black/dark grey is chosen because it is easiest on the eye over long periods (speculation, not a source). | Ufton photos (IECC); Lindridge Fig. 8 (Westcad-family); SimSig forum 142193 https://www.simsig.co.uk/forum/PostView/142194 |
| Unoccupied, no route | **Solid mid/light grey bar.** SimSig uses #888888, 6 px thick at 1:1. Real IECC photos show light grey. Westcad EMCC uses #dbdada. Track-circuit joints are small **gaps (2 px in SimSig)** in the bar. | SimSig track.png; Ufton Fig. 11–13; SimSig option "Show track circuit breaks" https://www.simsig.co.uk/Wiki/Show?page=usertrack:ssrun:func:f3:optionsdisplay |
| Non-track-circuited, or another box's track | Thin **hollow / double-line** track (outline only). Fringe tracks with approach berths are drawn this way on real IECC (e.g. `═1C76═` at the Reading fringe). | SimSig track-nonetc; Ufton Fig. 13 |
| Route set | **White** from just past the entrance signal to just past the exit signal. SimSig **also lights the overlap white**, with a short vertical "end of overlap" bar. The overlap reverts to grey when it times out after the train has stood at the exit signal. The route-set signal's **stem also turns white**. | SimSig routing_trains https://www.simsig.co.uk/Wiki/Show?page=usertrack:ssrun:routing_trains ; Rail Engineer ("line of white 'route' lights") ; Swindon panel |
| Occupied (TC or axle counter) | **Red** (SimSig #ff0000; on real IECC CRT/LCD photos it looks orange-red). The berth's headcode is drawn **inside** the red bar. | RAIB Ufton Fig. 6 ("Track occupied by train" = red); SimSig |
| Sectional route release | The white route turns red as the train occupies each section. With train-operated route release (TORR), each section **goes back to grey** as the train clears it, so a white-red-grey wave follows the train. Without TORR the route stays white behind the train until the signaller cancels it. | Rail Engineer ("white route lights change to red with the progress of the train"); SimSig TORR images https://www.simsig.co.uk/Wiki/Show?page=usertrack:ssrun:routing_trains |
| Track circuit failure / disturbed | **Shown exactly like an occupation (red), with no distinction.** SimSig developers say the real display "does not (and cannot) distinguish between them". The signaller's usual workaround is to interpose e.g. `TCF` in the berth. | SimSig forum #52143 / #52058 https://www.simsig.co.uk/Forum/PostView/52052 ; SimSig wiki signalling_display note |
| Possession / isolation | IECC puts **cyan/blue "highlighting" bands above and below** the grey track (RAIB Fig. 6). SimSig uses coloured stripes for possession, traction isolation and blocked-to-electric. | RAIB Ufton Fig. 6 and para 25 |
| Locked points (route or flank locked) | SimSig: the **point ends go white** ("Show locked points"). On real panels a separate red "locked" lamp is lit at the point switch. | SimSig optionsdisplay & operating_points; Swindon panel |

**Where sources disagree:** RAIB Fig. 6 labels a **white** bar "Track not occupied by train", yet the same report's photos show unoccupied, unrouted track as grey. Almost certainly the white sample is a route-set section (or RAIB simplified it). Every other source uses grey for idle track and white for routed track.

## 2. Signals

- **Symbol:** a small filled **circle (the lamp)** on an **L-shaped post/stem** drawn from the lamp back to the track. The post sits on the side of the track the signal is on (above or below the line). The lamp points in the direction of travel, with the post trailing behind it. SimSig uses four orientations (right/left × above/below). Real IECC photos show the same lamp-plus-hooked-post glyph. RAIB Fig. 6 shows a red disc with a grey L post above a grey track with a joint gap.
  - Sources: SimSig signalling_display; RAIB Ufton Fig. 6/11/13.
- **Colours: on/off, not the actual aspect (standard practice).** On NX panels and on IECC the controlled-signal indication is **red = on, green = any proceed aspect** (Y, YY, flashing or G), with **white for a subsidiary or shunt proceed**.
  - Rail Engineer: "the actual light is green for main running signals although a signal may be showing yellow or double yellow. White is used for a shunt/call-on proceed aspect."
  - Swindon panel: "green when the signal is displaying any 'off' colour".
  - SimSig forum (jc92 / kbarber): "only show as green (off) or red (on) despite what they will show to the driver". The history differs by region: SR and BR(S) lever frames repeated full aspects, LMR used an "OFF" stencil, and from the NX panel era green-for-off became BR standard. https://www.simsig.co.uk/Forum/PostView/26686
  - **SimSig deviation:** by default SimSig shows real aspects (Y/YY/G on the lamp). Its **"Panel signals"** option gives the prototypical red/green view, and calling-on/shunt shows as red with a small white quadrant.
- **Route-set stem:** SimSig turns the post **white** while a route is set from the signal. It calls this "your most reliable indication" when cancelling. The RAIB Fig. 6 post is grey (no route).
- **Automatic signals:** drawn with a **different stem**, a T/dash shape instead of an L. Real IECC photos show UW39/DW39 with a `-T-` style stem and a pale lamp.
  - In older boxes the signaller cannot see the aspect or lamp state of automatics. SimSig can show automatics with **grey heads / no aspect** ("Show automatic signal aspects" off). SimSig staff say that in real life "you can't see lamp indications on auto's in older PSBs".
  - The Swindon panel gives autos with replacement switches a white and a red lamp.
  - Sources: SimSig optionsdisplay; forum #112288 https://www.simsig.co.uk/Forum/PostView/113068 ; Swindon panel.
- **Signals controlled by another box:** grey lamp (SimSig sigext).
- **Auto-working buttons:** a **blue `A` beside the signal head with a small circle.** A hollow circle means auto off; a filled circle means auto on (SimSig #00a0ff). On real IECC the "○A" / "A○" in blue sits right next to many controlled signals (Ufton Fig. 11/12).
  - Emergency replacement on automatics: a red `E` (no proof of red) or `R` (interlocking proves red), hollow or filled the same way.
  - Western Region E10K interlockings used turn-switch AUTO/MANUAL buttons that can cover several signals.
  - Sources: https://www.simsig.co.uk/Wiki/Show?page=usertrack:ssrun:signal_auto_working ; .../signal_emergency_replacement
- **Shunt / position-light signals:** a **quarter-circle** head, not a disc. Grey when on (SimSig), white when off.
- **Main signal with a subsidiary (calling-on / PL):** a grey quarter-circle under the lamp that goes white when the subsidiary clears.
- **Exit and route-type buttons:** triangles. **Grey** = exit to a bay, siding or another view. **White** = shunt exit. **Red** = call-on exit or cross-boundary entrance. **Yellow** = warner (reduced-overlap) exit. **Blue arrow or circle** = via button.
  - Sources: SimSig signalling_display; https://www.simsig.co.uk/Wiki/Show?page=usertrack:ssrun:types_of_routesetting
- **Repeaters:** `R` / `RR` next to them; banner repeaters have their own glyph. Numbering is the stop signal's number plus R, RR or BR (railsigns.uk https://railsigns.uk/append/sigid1.html).
- **Signal numbering:** an alphabetic **prefix (1–3 letters) identifying the controlling box or panel, plus a number**, e.g. TR808, NS25, CE493 (railsigns.uk).
  - **Placement:** on real IECC *detailed views* the number sits right next to the signal, above or below it (Ufton Fig. 13: "804", "806", "895").
  - Workstations have *overviews* (limited or no labels) and *detailed views* (signal, track and points IDs). On overview screens the signaller "will have restricted signalling controls". SimSig mostly omits labels to save space.
  - Sources: RAIB Ufton glossary "IECC overview and detailed VDU displays"; SimSig forum #59171 https://www.simsig.co.uk/Forum/PostView/59211 ; Rail Engineer "RIF to ROC" (upper row overviews, lower row detailed views) https://www.railengineer.co.uk/?p=26174
- **Approach locking / time release:** the **signal lamp flashes red** on the screen while approach locking times out after a route is cancelled in front of an approaching train (the lineside signal is steady red). The route stays white until the timer expires. Typical timers: 2 min main, 30 s shunt.
  - Sources: https://www.simsig.co.uk/Wiki/Show?page=usertrack:glossary:approach_locking ; routing_trains
- **Lamp out / signal failure:** SimSig shows a **hollow (unlit) lamp**, which also covers approach-lit signals when dark. Real IECC shows controlled-signal filament failures; for automatics in older installations the signaller sees nothing. Shunt aspects are often not proved, so a shunt signal may show white on the panel while it is actually failed.
  - Sources: SimSig signalling_display (sigfail), signal_failures page, forum #112288
- **Reminder (collar):** real IECC draws **a blue/cyan box around the signal head** (RAIB Ufton para 24 and Fig. 6, which shows a cyan rectangle behind the lamp). SimSig uses a cyan background (cyan+magenta for a traction-isolation reminder).

## 3. Points

- **Drawing:** a diagonal leg joins the main track. The **leg the points lie in is drawn continuous** and the other leg is cut back by a small gap near the switch.
  - SimSig option "Show point positions": "more recent UK standards" always show the lie, while "original UK standards" show it only when locked; otherwise both legs merge.
  - Normal is not always drawn straight; IECC has a command to show the normal lie of every point.
  - Sources: SimSig signalling_display & optionsdisplay; forum #109939 https://www.simsig.co.uk/Forum/PostView/109857
- **Locked:** point ends are highlighted **white** (route or flank locked).
- **Keyed (individually swung):** SimSig puts a **blue line** on the point end. A reminder collar adds a **yellow surround**; older SimSig sims used the yellow surround alone.
- **Moving / out of correspondence / detection lost:** the **point ends flash** until detected. Continuous flashing means a failed point.
  - On NX panels the white track lights either side of the points "flash alternately".
  - Sources: https://www.simsig.co.uk/Wiki/Show?page=usertrack:ssrun:operating_points ; Swindon panel
- **Numbering:** a plain number (often the same prefix scheme), e.g. 975/977 for a crossover pair. Detailed IECC views show point numbers, and hand-crank or N/F indicators beside them (Ufton Fig. 11: "975/977 HANDCRANK", "N F" lamp stacks).
- Sprung points are marked `$` in SimSig. Catch and trap points are drawn as a stub leg.

## 4. Train describer berths

- **In-track berths (IECC):** the four-character description is written **inside the track bar in the berth track section, on the approach side of its signal**.
  - When occupied it appears as coloured text in a gap in the red occupation bar, e.g. `▬2K27▬`, `▬2X40▬` (Ufton Fig. 12/13).
  - **Empty berths are not drawn**. SimSig hides them; real IECC shows empty "last sent" berths as four small grey blocks.
  - Sources: Ufton photos; SimSig train_describer https://www.simsig.co.uk/Wiki/Show?page=usertrack:ssrun:train_describer ; openraildata-style description via search snippet.
- **Text colour:**
  - **ARS-controlled trains show cyan text**, trains not under ARS show **pink/magenta**, and trains on a special ARS timing pattern show **orange** (SimSig "Train describer mode", which matches the Ufton photo: `2K27` cyan, `7C29` / `2X40` pinkish).
  - Non-ARS areas are all cyan in SimSig.
  - An optional SimSig "delay" mode shows black text on green / yellow / red / blue / grey backgrounds by punctuality. It is a SimSig feature; I don't know if it exists in the real system.
- **Fringe / approach berths:** berths on the **hollow track beyond the boundary** mirror the neighbouring box's berths, e.g. `═1C76═`. They cannot be interposed.
  - IECC labels them **APPR**, and uses **LAST / LSNT** for last-sent berths.
  - Real IECC also has a **"TRAIN APPROACHING" text list**: headcodes in a stack with source berth IDs underneath (Ufton Fig. 13: `1M42 2M44 4O11 / 0348 DW37 0346 …`).
  - Sources: SimSig glossary approach_berth / last_sent_berth; Ufton Fig. 13
- **Stepping:** when the train occupies the track ahead of the signal, the description vanishes from the rear berth and appears in the next main signal's berth at the same moment. No animation.
- **Interpose / cancel:** real systems use keyboard (and trackerball) commands. The new text simply replaces the old one.
- **Special descriptions:**
  - An undescribed train that steps gets `****` (Southern: `*X**`).
  - Signallers type free text such as `TCF`, `BLOK`, `POSS`.
  - Headcode format is digit, letter, digit, digit (class, destination/route, number). `0` means light loco.
  - Sources: SimSig train_describer; special_headcodes page
- **Platforms with several trains:** stacked or multiple berths: "last in" at the buffers and "first out" at the signal, or a LIFO stack.

## 5. Route-setting interaction

- **Input:** a trackerball (IECC: "a big yellow one mounted in the desk"; Westcad UK similar). IECC Scalable offers a mouse and zoom/pan across the whole area.
  - The signaller puts the cursor on the **entrance signal**, presses **SET**, then does the same on the **exit signal or button**.
  - Every function also has a **keyboard** equivalent, e.g. `S45 S78 <SET>` or route codes `R45BS`, because two independent control methods are required.
  - Sources: Rail Engineer 2013; SimSig forum #46116/#46130/#46131 https://www.simsig.co.uk/Forum/PostView/46162 ; RMweb/Collingwood snippet on IECC Scalable
- **Entrance selected / "route setting":** SimSig shows **a flashing cursor in the track next to the entrance signal** until the exit is chosen.
  - NX-panel precedent: the entrance button light flashes white, then goes steady when the exit is pressed and the route is free (search snippet summarising panel practice; Swindon panel: route lights "commence lighting up... starting at the entrance and moving along to the exit").
  - SimSig abandons commands left incomplete for more than 30 s.
  - I found **no primary description of the exact IECC entrance highlight**. Treat "flashing entrance marker" as the SimSig/panel convention.
- **Route set:** the route turns white and the signal stem turns white. The lamp goes green when the interlocking clears the signal, which may be later (approach control, TC override, level crossing lowering). Points in the route flash while moving, then show steady.
- **Route cancelled:** the route reverts to grey and the stem to grey. Under approach locking the lamp flashes red and the route stays white until time release, then goes grey.
- **Via buttons, warner and shunt exits:** see the §2 triangles.
- **Rejected routes:** the UK does not stack routes. A request that cannot set is refused with a message; it is not queued.

## 6. Other standard elements

- **Labels and fonts:** place, line and platform names are **uppercase, in a grey bitmap/serif-ish fixed-width font**.
  - Line names carry direction arrows: `← DOWN WESTBURY`, `UP WESTBURY →`, `← UP PLAT →` (Ufton).
  - SimSig uses the same style: an 8 px-pitch bitmap font, ~10 px cap height, grey #888888 (sampled from letchstn.png).
  - White text marks highlighted or boundary labels, e.g. `COLTHROP ▶|◀ TOWNEY` for interlocking or area boundaries.
- **Platforms:** **filled ochre/orange rectangles with the platform number in black** (SimSig #ff8000; IECC photos show a tan/ochre fill).
- **Track circuit IDs:** two-letter or alphanumeric labels (`WA`, `WB`, `AC`, `QP`) in grey beside the track on **detailed views only**.
- **Level crossings:** a **yellow/ochre bar across the track** (SimSig: orange blocks either side of the track).
  - Labelled `UFTON AHB`, `DRAKES NO 2 UWC`, `CCTV`, `MCB`, `UWC`, `MWL`.
  - Status is shown as text or lamps: RAI/WKG/FAI (or R/W/F); controls LWR, CLR, RAI, AUT.
  - Failure alarms appear as **red text next to the crossing** (`BARRIERS FAILED`, `POWER ON`).
  - At the Westcad-family EMCC: orange tick marks on the track for UWCs, a yellow dot for "BARRIERS RAISED", red hollow circles for "POWER OFF" / "BARRIERS FAILED".
  - Sources: Ufton Fig. 11/12; Lindridge Fig. 8; SimSig lcs page https://www.simsig.co.uk/Wiki/Show?page=usertrack:ssterms:lcs
- **Magenta items:** on IECC these are magenta labels with a dot indicator (e.g. `LTHLEUP` / `LTHLEDN`). Probably lamp or telephone status indications; I could not confirm what they mean.
- **Boundaries with neighbouring areas:** the track continues as **hollow/double-line** into the neighbour's area and carries approach berths. Signals controlled by the neighbour are grey. Boundaries are marked `▶|◀` with both names, or `TO READING →` style labels. Continuations to another screen use letters, e.g. `A` ... `A`, or `2A` on paged views.
- **Alarms:** **real IECC has a dedicated Alarm VDU** ("signalling alarms and other messages") beside the four control VDUs. Local alarms also appear as red text on the diagram (Ufton Fig. 5, 11). SimSig uses a scrolling Messages window.
- **Command bar and clock:** IECC views have a **bottom row of command buttons** (ochre/yellow boxes: `REM`, `ISO`, `SCN`, `PTN`, `UW1…UW8`, `SIL`, …) and a **clock at bottom-left in green (HH:MM:SS)**. SimSig puts its Reminder button and clock in a similar control strip.
- **Screen layout:** a typical IECC workstation has 4 control VDUs (views selectable, detailed or overview), 2 overhead non-controllable overviews, an alarm VDU and a CSR/phone touch screen (Ufton Fig. 5). ROCs have an upper row of permanent overviews and a lower row of detailed control views (Rail Engineer "RIF to ROC").

## 7. Fonts, line widths, grid, sizes

- **Grid:** IECC Classic diagrams are built on a **character-cell / tile grid** (fixed-pitch). SimSig reproduces this with a bitmap font and tile-based "draw data". A SimSig developer mentions "tile boundaries" and small labels as things SimSig can't reproduce exactly (forum #142185/#142186). IECC Scalable moved to "a more dynamic, vector-drawn screen diagram" (gwsbristol) with zoom.
- **SimSig pixel metrics at 1:1** (sampled from its PNGs; not measured from a real IECC screen):
  - track bar **6 px** thick
  - TC joint gap **2 px**
  - signal lamp ≈ **8 px** diameter, post 2 px
  - font ≈ **8 px** character pitch, ≈ 10 px cap height, uppercase, serifed bitmap
  - diagonal (points) legs ≈ 2–4 px
  - palette (SimSig core): black #000000, grey #888888, light grey #c0c0c0, white #ffffff, red #ff0000, green #00ff00, yellow #ffff00, orange #ff8000, cyan #00ffff, pink #ffa0ff, blue #00a0ff, magenta #ff00ff/#ff0080
  - SimSig originally had ~12 fixed colours, now 17 named plus 10 custom (forum 142184 and search snippet).
- **Westcad-family EMCC sample:** bg #494650, track #dbdada, orange #ff7f00, labels white in a small sans bitmap font.
- **Text sizes:** TD text is the same character height as labels, sized to fit in the track gap. Label text is uppercase only. I found no public numeric spec; BR1878 / GK/RT0025 presumably define minimum character sizes, but their text is paywalled.

## 8. The simplifier (signal-box timetable sheet)

**What it is:** a box-produced **extract of the Working Timetable (WTT)**. It covers only the timing and regulating points that matter to one workstation or panel. Trains are listed **in the order they reach a key point**, one sheet per direction or line group (e.g. "WS1 Down", "WS1 Up"). Notes are added for immediate reference. It is local and informal, not a national standard format, so layout varies by box.
- SimSig forum (jc92, 2025): "In real life TRUST gives the same information to a signaller, but for large stations and/or junctions the boxes often produce a simplifier which gives the info in a table along with additional useful notes for immediate reference." https://www.simsig.co.uk/Forum/PostView/159925
- Firefly (2013): signallers "9 times out of 10 ... would probably just use the simplifier, it tells them the times that trains are supposed to be at certain places and it gives them the running order". A lever-frame signaller (Colourlight) says simplifiers are a signalling-centre thing and uses TRUST instead. https://www.simsig.co.uk/Forum/PostView/45753
- Firefly (2014): "They also have **paper copies** of the simplifier, therefore they just look at the simplifier and immediately know which way to route a train (no clicking and looking at train lists)." It also notes paper notes on panels, e.g. loop lengths. https://www.simsig.co.uk/Forum/PostView/59409
- **A worked example written by a working signaller** (Edinburgh 2006 simplifier notes, Word doc; author metadata "Network Rail"; "based ... on those that I use at work"): https://www.simsig.co.uk/Media/Wiki/%2Fsimulations%2Fedinburgh%2Fedinburgh_2006_simplifier_notes.doc . Its structure:
  - one sheet per workstation per direction
  - **columns = the workstation's regulating points only** (e.g. Grantshouse → Dunbar → Drem → Monktonhall Jn), deliberately not every station
  - rows = trains in the order they reach the interface point
  - **fringe/approach timings from the neighbouring workstation in coloured italics** (the colour matches that workstation's sheet), so they serve as train-approach notice
  - **highlighted times for calls** (e.g. yellow = stops at Dunbar; plain = passing) and for **booked loop waits** (gold / tan per loop)
  - **route abbreviations written under a time** where a junction diverges (e.g. "NB" = North Berwick branch, "Shotts", "DRT", "Cem't"; lines such as "NLL", "SLW")
  - a **Notes column**: stopping pattern, next working (forms), "follows X from here"
  - days-run handled by "return to the top of the simplifier on the correct day"
- **Underlying WTT format** (the source a simplifier is cut from). Network Rail-format WTT, TfL FOI copy: https://foi.tfl.gov.uk/FOI-1702-1920/Overground%204.pdf
  - Trains are **columns** and locations are **rows**.
  - The header block per train is: `Train ID` (headcode), `Departs` (origin time), `From` / `To` (TIPLOC-style codes, e.g. `CLPHMJ1`, `WCROYDN`), `Timing Type` (traction/timing load), `Days Run` (`[SO]`, `[SX]`, `[MX]`…).
  - Times are 24 h `HH.MM` with **½-minute** precision. `HH/MM` = **passing** time; `Arr`/`Dep` rows at calling points; letter suffixes such as `a`–`h` for "arrives n min earlier" and `p`/`q`/`r` for public-time differences; `*` = stops for other trains ahead only; `OP` = operating stop; `RM` = reversing.
  - Interleaved **`plt`** rows give the platform and **`mgn`** (margin) rows carry allowances: **`[n]` engineering, `(n)` pathing, `<n>` performance**. Line codes include `SL`, `RVL`, `AL`.
  - Day codes: "O" = only, "X" = excepted, "normally between square brackets e.g. [SX]".
- **Paper vs electronic:** the WTT itself is published electronically by Network Rail (https://www.networkrail.co.uk/running-the-railway/the-timetable/working-timetable/). Simplifiers are printed sheets, spreadsheet-produced in the Edinburgh example, kept at the workstation. **Where it sits:** on the desk or in a clipboard beside the trackerball and keyboard. RAIB workstation photos show paper documents on the desk (Lindridge Farm Fig. 6). It is not part of the VDU diagram.
- **SimSig proxy:** the **Simplifier window (F8)** lists every train arriving, departing or passing a chosen location (and optionally platform) from a given time, with time, platform, activities, and current delay for trains in the next 2 h. It sorts by time, or by platform then time. https://www.simsig.co.uk/Wiki/Show?page=usertrack:ssrun:func:f8

## 9. Train enquiry (how a signaller looks up where a train is booked to go)

- **Real systems, in order of how often signallers use them:**
  1. **The simplifier** (above).
  2. **TRUST.** National train running system, auto-fed from TD berth steps via SMART with berth offsets. A signaller can "search for a specific train to see any delays along the previous passing points" and "bring up a passing point and see a list of all trains passing that location along with details of any delay". TRUST takes its schedules from TSI. Sources: forum #45750/#45751 https://www.simsig.co.uk/Forum/PostView/45753 ; TRUST/SMART description via search snippet of railforums "TRUST help" thread.
  3. **ARS** on IECC. ARS holds each train's timetable, keyed on the TD description (changing the description changes the timetable ARS applies). It colours TDs **cyan = ARS will route, magenta/pink = not ARS / manually routed**. SimSig calls this "opposite to what you might expect, but ... authentic". It also shows per-train routing status. https://www.simsig.co.uk/Wiki/Show?page=usertrack:ars ; .../func:f3:arsdispopts
  4. **Traffic-management and info tools** at ROCs: **train graphs** (time–distance) for projecting disruption (Hitachi Tranista TM for Thameslink; Resonate **Luminate** on the GWML since 2018) and **Acumen Platform Docker**. Platform Docker takes signalling data and shows "predicted train locations; actual train locations; status of trains (including timeliness...)" with "map, layout and timetable displays" and "a visual indication of lateness". It updates as trains pass signals. https://blog.bham.ac.uk/bcrre/2020/07/06/automating-data-into-useful-information-to-support-decision-making/ ; Modern Railways TM article (search snippet) https://www.modernrailways.com/article/traffic-management-technology-process
  - These all run on **separate screens** from the signalling VDUs. EMCC workstations have "two additional VDUs for displaying running information for trains outside the workstation's area of control" (RAIB Lindridge para 22, Fig. 6). There is no evidence that clicking a berth on a real IECC pops up a timetable. That is a SimSig convenience; real lookups are by headcode in TRUST, TM or ARS enquiry screens.
- **Query by headcode:** real systems key on the **4-character description** (TD, ARS, TRUST lookups), and TRUST also keys on location. Headcode reuse is common, so the full schedule identity (UID plus date) disambiguates. In SimSig, extended descriptions (e.g. `0K011`) and a separate UID are kept for this reason. https://www.simsig.co.uk/Wiki/Show?page=usertrack:ssrun:train_describer ; .../func:f2
- **SimSig windows, as a proxy for what a lookup shows:**
  - **Show Timetable** (click a berth): a list of timing points with columns **Location, Arr, Dep/Pass, Path, Plat(form), Line, Eng(ineering) allowance, Path(ing) allowance, Pos(ition), Activities**, plus **Last Reported Status** (minutes early/late at the last timing point).
    - Times use `12:34` (stop), `12/34` (pass), `12d34` (set-down), `12t34`, `12*34`, `12r34`.
    - **Path = line in from, Line = line out to**. `REV` = reverses here. `RVL` = reversible line.
    - https://www.simsig.co.uk/Wiki/Show?page=usertrack:ssrun:func:f4:wttshowtt ; .../usertrack:ttuse:timing_codes
  - **Train List (F2):** ID, UID, Timetable, Dir, Status, Current/Prev location, Length, Power, Description, Workstation.
  - **Location Line Up (F9):** a "TRUST-like summary" per location: Headcode, UID, Arrive/`PASS`, Depart, LastReport, Delay (`5L`/`2E`/`OT`), Path, Platform, Line, Origin, Destination, Activities (next working, joins/divides), Stock. https://www.simsig.co.uk/Wiki/Show?page=usertrack:ssrun:lineups
  - **ARS delay mode** colours the TD background by punctuality: blue early, green ≤2 min late, yellow 2–5, red ≥5, grey unknown.

---

## Top 10 conventions for a faithful look

1. **Black background** (dark slate grey if going for Westcad), and no decorative colour anywhere. Colour always means a state.
2. **Idle track is a thick grey bar broken by small gaps at every track-circuit joint.** Non-controlled or fringe track is hollow/double-line.
3. **Route set = white**, from the entrance signal to the exit signal *and through the overlap* (end-of-overlap tick), with the entrance signal's **stem turning white**.
4. **Occupied = red**, which a failed track circuit also looks like. The route clears section by section behind the train (white → red → grey).
5. **Signals are a small disc on a hooked post, on the correct side of the line, facing the direction of travel.** Controlled signals show **red (on) / green (any off)**, automatics have a distinct stem, shunts are quarter-circles, and a subsidiary quadrant goes white when cleared.
6. **Headcodes live inside the track in the berth section on the approach side of the signal, as cyan text (pink if not ARS).** Empty berths are invisible. Stepping is an instant jump berth to berth. Fringe berths sit on the hollow boundary track.
7. **Flashing is reserved for transitional or abnormal states:** points out of correspondence, signal lamp flashing red during approach-locking time release, requested slots and releases, the entrance selection cursor.
8. **Points:** show the lie by a gap in the unused leg. Point ends go white when locked and blue when keyed, and flash while moving or when detection is lost.
9. **Uppercase grey bitmap labels with direction arrows** (`← DOWN MAIN`, `UP FAST →`). Platforms are ochre filled blocks with the number in them. Level crossings are a yellow/ochre bar with a text status that goes red on failure.
10. **Blue `○A` auto buttons beside signals** (hollow = off, filled = on). Red `E`/`R` replacement buttons. Reminders are a cyan/blue box around the signal. A bottom command bar and HH:MM:SS clock.

(Timetable side, for completeness: where a train is booked to go comes from a **simplifier**, off the diagram. It is a per-workstation sheet of trains in running order against regulating-point columns, with platform/line notes, allowances in `[ ]` `( )` `< >`, `HH/MM` passing times, and days-run `[SX]`/`[SO]`. Live running comes from TRUST/TM screens beside the signalling VDUs, not from the track diagram.)

## Where a game can sensibly deviate

- **Show real aspects (R/Y/YY/G, flashing yellows)** as a toggle, as SimSig does by default. Offer "Panel signals" (red/green only) as a realism mode. Real VDUs show only on/off because that's all the signaller needs.
- **Show automatic signal aspects and lamp-out states.** They are invisible on many real installations, but useful for learning.
- **Distinguish a track-circuit failure from a train.** The real system can't, so keep it off by default or as an assist.
- **Labels (signal, points and TC IDs) on demand.** Real systems put them only on detailed views. A hover or hotkey overlay is a good substitute for switching views.
- **A continuous scrolling/zoomable canvas instead of fixed paged views.** IECC Scalable already allows zoom, so this is defensible.
- **Delay-coloured TD backgrounds and ARS colour coding** (SimSig-style) as optional overlays.
- **Colour-blind options:** shape cues (SimSig players already rely on the white route stem rather than lamp colour), or a hue remap. Real red/green is not accessible.
- **Click a berth to see the train's schedule** (SimSig's Show Timetable). Not prototypical for IECC, where lookups are by headcode in TRUST/ARS/TM on separate screens, but it is the single biggest usability win. Keep it in a separate panel or window styled like a WTT/TRUST listing, not an overlay on the diagram.
- **An in-game simplifier panel** (SimSig F8-style) generated from the timetable. Ideally make it printable or dockable to mimic the paper sheet: per-workstation, per-direction, running order, regulating-point columns, call/loop highlighting, Notes column (next working, platform, line).
- **Location line-up / TRUST-like list** with delay (`5L`/`OT`/`2E`), and optional **delay-coloured TD backgrounds**.
- **Anti-aliased vector rendering** instead of strict bitmap tiles. Keep the proportions (thick bars, small gaps, small lamps, uppercase fixed-pitch text) and it will still read as IECC.

## Open gaps (not resolved from public sources)

- The exact IECC/Westcad entrance-selected highlight and the "route requested but not yet set" indication. Only SimSig and NX-panel practice are documented.
- Whether real IECC draws the overlap white. SimSig does, and SimSig devs discuss "overlap markers" matching "panel photos", but no primary image confirms the colour of an overlap section.
- Westcad / Controlguide and Hitachi/Thales ROC signal glyphs and TD styling. The only public image found (EMCC, Lindridge Fig. 8) shows no signals cleared and no berths.
- Numeric font and size requirements from BR1878 / GK/RT0025 / NR/SP/SIG/10067 / 17504 (need RSSB/NR standards access).
