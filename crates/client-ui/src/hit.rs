//! What is under the pointer: ○A buttons and signals first, then berths
//! holding a headcode, exits, points, empty berths and finally track, each
//! within a fixed distance in pixels. An empty berth draws nothing, so it
//! never hides what is drawn under it.

use client_core::Target;
use egui::{Pos2, Rect, vec2};
use protocol::View;

use crate::camera::Camera;
use crate::paint::{AUTO_R, HOOK_PX, LAMP_R, POST_PX, glyph, left_of, number_px};
use crate::scene::{BerthMark, Scene, SignalMark, project};

/// How near (pixels) the pointer must be to a signal, exit, points or track.
pub const HIT_PX: f32 = 8.0;
/// A berth box, in pixels.
pub const BERTH_W: f32 = 34.0;
pub const BERTH_H: f32 = 14.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub target: Target,
    /// Yours to work: clicks and menus apply. Otherwise hover only.
    pub clickable: bool,
}

/// Where a signal's disc is on screen: out from its base to the left of
/// travel, then along the hook (realism spec §2); at the signal itself when
/// its facing is unknown.
pub fn signal_disc(cam: &Camera, screen: Rect, s: &SignalMark) -> Pos2 {
    if s.facing == egui::Vec2::ZERO {
        return cam.to_screen(screen, s.at);
    }
    let g = glyph(cam.scale);
    cam.to_screen(screen, s.base) + left_of(s.facing) * (POST_PX * g) + s.facing * ((HOOK_PX + LAMP_R) * g)
}

/// Where a controlled signal's ○A button is: `AUTO_AHEAD_PX` ahead of its
/// lamp (past a second yellow); `None` for signals without one, and for
/// every signal while numbers are too small to draw (polish spec P5).
pub fn auto_button(cam: &Camera, screen: Rect, s: &SignalMark) -> Option<Pos2> {
    if !s.auto_button || number_px(cam.scale).is_none() {
        return None;
    }
    let ahead = if s.facing == egui::Vec2::ZERO { vec2(1.0, 0.0) } else { s.facing };
    Some(signal_disc(cam, screen, s) + ahead * (AUTO_AHEAD_PX * glyph(cam.scale)))
}

/// How far ahead of its lamp a signal's ○A button sits.
pub const AUTO_AHEAD_PX: f32 = 16.0;

/// One headcode character at `paint::HEADCODE_PX` in egui's monospace font,
/// and the knock-out's margin round the text.
pub const HEADCODE_CHAR_PX: f32 = 6.7;
pub const BERTH_PAD_PX: f32 = 6.0;

/// A berth box wide enough for headcodes of `chars` characters, never
/// narrower than `BERTH_W` (Gretz's are up to 8 characters long). The scene
/// sizes by this area's simplifier and the world's display headcodes; the raw
/// headcode of a non-calling service outside both can still overflow.
pub fn berth_width(chars: usize) -> f32 {
    BERTH_W.max(chars as f32 * HEADCODE_CHAR_PX + BERTH_PAD_PX)
}

/// A berth's box on screen, as wide as the layout's longest headcode.
pub fn berth_box(cam: &Camera, screen: Rect, b: &BerthMark) -> Rect {
    Rect::from_center_size(cam.to_screen(screen, b.at) + b.offset_px, vec2(b.width_px, BERTH_H))
}

/// A `BERTH_W` box at `at` moved by `offset_px`, on screen.
pub fn berth_rect(cam: &Camera, screen: Rect, at: Pos2, offset_px: egui::Vec2) -> Rect {
    Rect::from_center_size(cam.to_screen(screen, at) + offset_px, vec2(BERTH_W, BERTH_H))
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    p.distance(project(p, a, b))
}

fn nearest<'a, T>(items: impl Iterator<Item = (&'a T, f32)>) -> Option<&'a T>
where
    T: 'a,
{
    items.filter(|(_, d)| *d <= HIT_PX).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(t, _)| t)
}

/// `view` says which berths hold a headcode (none without one).
pub fn hit_test(scene: &Scene, view: Option<&View>, cam: &Camera, screen: Rect, p: Pos2) -> Option<Hit> {
    let at = |q: Pos2| cam.to_screen(screen, q);
    // The ○A buttons first: they sit just ahead of their lamps.
    let button = |s: &SignalMark| auto_button(cam, screen, s).map(|c| c.distance(p)).filter(|d| *d <= AUTO_R * glyph(cam.scale) + 2.0);
    if let Some(s) = scene.signals.iter().filter_map(|s| Some((s, button(s)?))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(s, _)| s) {
        return Some(Hit { target: Target::Auto(s.name.clone()), clickable: s.operable });
    }
    // A signal is its disc, the foot of its post, and its own point.
    let signal_dist =
        |s: &SignalMark| signal_disc(cam, screen, s).distance(p).min(at(s.base).distance(p)).min(at(s.at).distance(p));
    if let Some(s) = nearest(scene.signals.iter().map(|s| (s, signal_dist(s)))) {
        return Some(Hit { target: Target::Signal(s.name.clone()), clickable: s.operable || s.route_exit });
    }
    let filled = |name: &str| view.is_some_and(|v| v.berths.contains_key(name));
    let berth = |want_filled: bool| {
        scene
            .berths
            .iter()
            .find(|b| filled(&b.name) == want_filled && berth_box(cam, screen, b).contains(p))
            .map(|b| Hit { target: Target::Berth(b.name.clone()), clickable: b.operable })
    };
    if let Some(h) = berth(true) {
        return Some(h);
    }
    if let Some(e) = nearest(scene.exits.iter().map(|e| (e, at(e.at).distance(p)))) {
        return Some(Hit { target: Target::Exit(e.node.clone()), clickable: e.route_exit });
    }
    if let Some(pm) = nearest(scene.points.iter().map(|m| (m, at(m.at).distance(p)))) {
        return Some(Hit { target: Target::Points(pm.name.clone()), clickable: pm.operable });
    }
    // Still hit-testable, so hover and the interpose menu reach it.
    if let Some(h) = berth(false) {
        return Some(h);
    }
    nearest(scene.tracks.iter().map(|t| (t, dist_to_segment(p, at(t.a), at(t.b)))))
        .map(|t| Hit { target: Target::Section(t.section.clone()), clickable: false })
}
