//! Drawing the diagram as an IECC workstation shows it (realism spec §2):
//! black background, thick grey track broken at every track-circuit joint,
//! white routes and overlaps, red occupation, signals as discs on hooked
//! posts, cyan headcodes in the track, blue ○A buttons, ochre platforms,
//! grey capital labels and direction arrows. Colour only ever means state.
//! `draw` is pure (shapes and text in screen pixels, testable without a GPU
//! or fonts); `paint` hands them to an egui `Painter`.

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
/// At most this many arrows along a run between its end ones.
pub const ARROW_MAX_STOPS: usize = 1024;
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

/// The end-of-overlap tick across a bar's `end`, perpendicular to `dir`.
fn tick(out: &mut Vec<Shape>, end: Pos2, dir: Vec2, w: f32) {
    let n = if dir.length() > 0.0 { vec2(-dir.y, dir.x).normalized() } else { Vec2::ZERO };
    let half = n * ((w + TICK_EXTRA_PX) / 2.0);
    out.push(Shape::line_segment([end - half, end + half], Stroke::new(TICK_W, OVERLAP)));
}

/// A leg's far end, pulled in by half a joint gap where another section meets it.
fn leg_end(p: &PointsMark, c: Pos2, end: Pos2, meets: &[String]) -> Pos2 {
    let d = end - c;
    if !meets.iter().any(|m| *m != p.section) || d.length() <= JOINT_GAP_PX * 2.0 {
        return end;
    }
    end - d.normalized() * (JOINT_GAP_PX / 2.0)
}

fn points_shapes(out: &mut Vec<Shape>, p: &PointsMark, to: &dyn Fn(Pos2) -> Pos2, st: &PaintState, colour: Color32, w: f32) {
    let section = |name: &str| st.view.and_then(|v| v.sections.get(name));
    let held = |name: &str| section(name).is_some_and(|s| s.held != Held::Free);
    let overlap = section(&p.section).is_some_and(|s| s.held == Held::Overlap);
    let pv = st.view.and_then(|v| v.points.get(&p.name));
    let lying = pv.map_or(PointsPos::Normal, |v| v.position);
    let moving = pv.is_some_and(|v| v.moving);
    let (lie, other) = match lying {
        PointsPos::Normal => ((p.normal, &p.normal_meets), (p.reverse, &p.reverse_meets)),
        PointsPos::Reverse => ((p.reverse, &p.reverse_meets), (p.normal, &p.normal_meets)),
    };
    let c = to(p.at);
    // The toe and the lying leg carry the route; an overlap held here ends
    // in a tick at the far end of each that nothing held goes on from.
    for (leg, meets) in [(p.toe, &p.toe_meets), lie] {
        let Some(l) = leg else { continue };
        let end = leg_end(p, c, to(l), meets);
        bar(out, c, end, w, colour, p.fringe);
        if overlap && !meets.iter().any(|m| held(m)) {
            tick(out, end, end - c, w);
        }
    }
    if let (Some(o), meets) = other {
        let far = to(o);
        let end = leg_end(p, c, far, meets);
        let gap_end = c + (far - c) * GAP;
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
        for (end, meets) in [(a, &t.a_meets), (b, &t.b_meets)] {
            if !meets.iter().any(|m| held(m)) {
                tick(&mut d.shapes, end, b - a, w);
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
/// loose end, and every `ARROW_EVERY_PX`, not crowding the end ones; at most
/// `ARROW_MAX_STOPS` of those, so nonsense coordinates stay cheap.
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
    // Counted, not summed: a sum of f32 steps stops moving on huge runs.
    for k in 1..=ARROW_MAX_STOPS {
        let d = k as f32 * ARROW_EVERY_PX;
        if d > total - ARROW_EVERY_PX / 2.0 {
            break;
        }
        if ends.iter().all(|e| (e - d).abs() >= ARROW_EVERY_PX / 2.0) {
            stops.push(d);
        }
    }
    stops.sort_by(f32::total_cmp);
    stops
}

fn run_arrows(out: &mut Vec<Shape>, run: &Run, to: &dyn Fn(Pos2) -> Pos2, w: f32, screen: Rect) {
    if !run.forward && !run.backward {
        return;
    }
    let pts: Vec<Pos2> = run.points.iter().map(|&p| to(p)).collect();
    let total: f32 = pts.windows(2).map(|p| p[0].distance(p[1])).sum();
    for stop in arrow_stops(total, run.loose_start, run.loose_end) {
        let Some((p, dir)) = along(&pts, stop) else { continue };
        if !screen.expand(ARROW_PX * 2.0).contains(p) {
            continue;
        }
        // Beside the bar, on the right of travel: of the run's forward
        // direction for a double arrow or a forward one, of its backward
        // direction for a backward one.
        let right = vec2(-dir.y, dir.x) * (w / 2.0 + ARROW_OFF_PX);
        let c = if run.forward { p + right } else { p - right };
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
        // Fringe signals are grey whatever is set from them.
        let colour = if s.fringe {
            FRINGE
        } else if !routes.is_empty() {
            ROUTE
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
        run_arrows(&mut d.shapes, r, &to, w, screen);
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
    // Headcodes in the track, on a black knock-out; an empty berth is not
    // drawn. Before the signals, so a disc or ○A beside one stays whole.
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
    for s in &scene.signals {
        signal_shapes(&mut d, s, cam, screen, st);
        // The ○A button: filled while the route set from the signal is
        // auto-working (live, not cancelling), hollow otherwise.
        if let Some(c) = auto_button(cam, screen, s) {
            let on = st.view.is_some_and(|v| {
                s.routes.iter().filter_map(|r| v.routes.get(r)).any(|rv| rv.auto_working && rv.state != RouteState::Cancelling)
            });
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
