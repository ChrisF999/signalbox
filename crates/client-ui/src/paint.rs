//! Drawing the diagram (spec D1 §3.1): an IECC-style VDU on a near-black
//! background. `draw` is pure (shapes and text in screen pixels, testable
//! without a GPU or fonts); `paint` hands them to an egui `Painter`.

use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, vec2};
use protocol::{Aspect, ExitName, Held, PointsPos, SectionView, View};

use crate::camera::Camera;
use crate::hit::berth_rect;
use crate::scene::{PointsMark, Scene};

pub const BG: Color32 = Color32::from_rgb(0x0B, 0x0B, 0x0F);
pub const TRACK_FREE: Color32 = Color32::from_rgb(0x6E, 0x6E, 0x6E);
pub const ROUTE: Color32 = Color32::from_rgb(0xEB, 0xEB, 0xEB);
pub const OVERLAP: Color32 = Color32::from_rgb(0xA0, 0xA0, 0xA0);
pub const OCCUPIED: Color32 = Color32::from_rgb(0xE6, 0x28, 0x28);
pub const RED: Color32 = Color32::from_rgb(0xE6, 0x1E, 0x1E);
pub const YELLOW: Color32 = Color32::from_rgb(0xFA, 0xD2, 0x00);
pub const GREEN: Color32 = Color32::from_rgb(0x00, 0xDC, 0x50);
pub const HEADCODE: Color32 = YELLOW;
pub const BERTH_EMPTY: Color32 = Color32::from_rgb(0x46, 0x46, 0x46);
pub const SELECT: Color32 = Color32::from_rgb(0x00, 0xC8, 0xFF);
pub const FLASH: Color32 = Color32::from_rgb(0xFF, 0x3C, 0xFF);
pub const PLATFORM: Color32 = Color32::from_rgb(0x23, 0x23, 0x4A);
pub const LABEL: Color32 = Color32::from_rgb(0x96, 0x96, 0xAA);
pub const AUTO_ON: Color32 = ROUTE;
pub const AUTO_OFF: Color32 = BERTH_EMPTY;

/// Line widths and sizes in pixels, whatever the zoom.
pub const TRACK_W: f32 = 3.0;
pub const LAMP_R: f32 = 4.0;
pub const STUB_PX: f32 = 9.0;
/// Where the non-lying leg of points starts, as a fraction of its length.
pub const GAP: f32 = 0.5;

/// A section's colour: occupied red, route white, overlap dim white, else grey.
pub fn track_colour(v: Option<&SectionView>) -> Color32 {
    match v {
        Some(s) if s.occupied => OCCUPIED,
        Some(s) if s.held == Held::Path => ROUTE,
        Some(s) if s.held == Held::Overlap => OVERLAP,
        _ => TRACK_FREE,
    }
}

/// Half brightness, for the fringe.
pub fn dim(c: Color32) -> Color32 {
    Color32::from_rgb(c.r() / 2, c.g() / 2, c.b() / 2)
}

fn shade(c: Color32, fringe: bool) -> Color32 {
    if fringe { dim(c) } else { c }
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
    /// The signal flashing for a refusal.
    pub flashing: Option<&'a str>,
    /// Seconds, for flashing.
    pub time: f64,
}

fn points_shapes(out: &mut Vec<Shape>, p: &PointsMark, to: &dyn Fn(Pos2) -> Pos2, st: &PaintState, colour: Color32) {
    let pv = st.view.and_then(|v| v.points.get(&p.name));
    let lying = pv.map_or(PointsPos::Normal, |v| v.position);
    let moving = pv.is_some_and(|v| v.moving);
    let (lie, other) = match lying {
        PointsPos::Normal => (p.normal, p.reverse),
        PointsPos::Reverse => (p.reverse, p.normal),
    };
    let c = to(p.at);
    let stroke = Stroke::new(TRACK_W, colour);
    if let Some(t) = p.toe {
        out.push(Shape::line_segment([c, to(t)], stroke));
    }
    if let Some(l) = lie {
        out.push(Shape::line_segment([c, to(l)], stroke));
    }
    if let Some(o) = other {
        let end = to(o);
        let gap_end = c + (end - c) * GAP;
        out.push(Shape::line_segment([gap_end, end], stroke));
        // While moving the gap flashes: closed in the dark half of the blink.
        if moving && !blink_on(st.time) {
            out.push(Shape::line_segment([c, gap_end], stroke));
        }
    }
}

pub fn draw(scene: &Scene, cam: &Camera, screen: Rect, st: &PaintState) -> Drawing {
    let to = |p: Pos2| cam.to_screen(screen, p);
    let mut d = Drawing::default();
    let section = |name: &str| st.view.and_then(|v| v.sections.get(name));
    for p in &scene.platforms {
        let r = Rect::from_two_pos(to(p.rect.min), to(p.rect.max));
        d.shapes.push(Shape::rect_filled(r, CornerRadius::ZERO, PLATFORM));
        d.texts.push(TextItem { at: r.center(), anchor: Align2::CENTER_CENTER, text: p.label.clone(), size: 9.0, colour: LABEL, monospace: false });
    }
    for t in &scene.tracks {
        let c = shade(track_colour(section(&t.section)), t.fringe);
        d.shapes.push(Shape::line_segment([to(t.a), to(t.b)], Stroke::new(TRACK_W, c)));
    }
    for p in &scene.points {
        let c = shade(track_colour(section(&p.section)), p.fringe);
        points_shapes(&mut d.shapes, p, &to, st, c);
    }
    for e in &scene.exits {
        let lit = st.exits.contains(&ExitName::Node(e.node.clone()));
        let r = Rect::from_center_size(to(e.at), vec2(7.0, 7.0));
        d.shapes.push(Shape::rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.5, if lit { SELECT } else { shade(TRACK_FREE, e.fringe) }), StrokeKind::Middle));
    }
    for s in &scene.signals {
        let at = to(s.at);
        let aspect = st.view.and_then(|v| v.signals.get(&s.name)).copied().unwrap_or(Aspect::Red);
        let (first, second) = lamps(aspect);
        d.shapes.push(Shape::line_segment([at - s.facing * STUB_PX, at], Stroke::new(1.5, shade(ROUTE, s.fringe))));
        d.shapes.push(Shape::circle_filled(at, LAMP_R, shade(first, s.fringe)));
        if let Some(c) = second {
            d.shapes.push(Shape::circle_filled(at + s.facing * (LAMP_R * 2.2), LAMP_R, shade(c, s.fringe)));
        }
        if st.selected == Some(s.name.as_str()) || st.exits.contains(&ExitName::Signal(s.name.clone())) {
            d.shapes.push(Shape::circle_stroke(at, LAMP_R + 3.5, Stroke::new(2.0, SELECT)));
        }
        if st.flashing == Some(s.name.as_str()) && blink_on(st.time) {
            d.shapes.push(Shape::circle_stroke(at, LAMP_R + 6.0, Stroke::new(2.0, FLASH)));
        }
        if !s.auto_routes.is_empty() {
            let on = st.view.is_some_and(|v| s.auto_routes.iter().any(|r| v.routes.get(r).is_some_and(|rv| rv.auto_working)));
            d.texts.push(TextItem {
                at: at + vec2(LAMP_R + 3.0, -(LAMP_R + 3.0)),
                anchor: Align2::LEFT_BOTTOM,
                text: "A".into(),
                size: 9.0,
                colour: shade(if on { AUTO_ON } else { AUTO_OFF }, s.fringe),
                monospace: true,
            });
        }
    }
    for b in &scene.berths {
        let r = berth_rect(cam, screen, b.at, b.offset_px);
        match st.view.and_then(|v| v.berths.get(&b.name)) {
            Some(h) => {
                d.shapes.push(Shape::rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, shade(BERTH_EMPTY, b.fringe)), StrokeKind::Middle));
                d.texts.push(TextItem {
                    at: r.center(),
                    anchor: Align2::CENTER_CENTER,
                    text: h.clone(),
                    size: 11.0,
                    colour: shade(HEADCODE, b.fringe),
                    monospace: true,
                });
            }
            None => d.shapes.push(Shape::rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, dim(shade(BERTH_EMPTY, b.fringe))), StrokeKind::Middle)),
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
