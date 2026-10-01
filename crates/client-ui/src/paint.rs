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
