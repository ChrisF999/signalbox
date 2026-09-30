//! What is under the pointer: signals first, then berths, exits, points
//! and finally track, each within a fixed distance in pixels.

use client_core::Target;
use egui::{Pos2, Rect, vec2};

use crate::camera::Camera;
use crate::scene::Scene;

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

/// The berth box on screen.
pub fn berth_rect(cam: &Camera, screen: Rect, at: Pos2, offset_px: egui::Vec2) -> Rect {
    Rect::from_center_size(cam.to_screen(screen, at) + offset_px, vec2(BERTH_W, BERTH_H))
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 == 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

fn nearest<'a, T>(items: impl Iterator<Item = (&'a T, f32)>) -> Option<&'a T>
where
    T: 'a,
{
    items.filter(|(_, d)| *d <= HIT_PX).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(t, _)| t)
}

pub fn hit_test(scene: &Scene, cam: &Camera, screen: Rect, p: Pos2) -> Option<Hit> {
    let at = |q: Pos2| cam.to_screen(screen, q);
    if let Some(s) = nearest(scene.signals.iter().map(|s| (s, at(s.at).distance(p)))) {
        return Some(Hit { target: Target::Signal(s.name.clone()), clickable: s.operable });
    }
    if let Some(b) = scene.berths.iter().find(|b| berth_rect(cam, screen, b.at, b.offset_px).contains(p)) {
        return Some(Hit { target: Target::Berth(b.name.clone()), clickable: b.operable });
    }
    if let Some(e) = nearest(scene.exits.iter().map(|e| (e, at(e.at).distance(p)))) {
        return Some(Hit { target: Target::Exit(e.node.clone()), clickable: true });
    }
    if let Some(pm) = nearest(scene.points.iter().map(|m| (m, at(m.at).distance(p)))) {
        return Some(Hit { target: Target::Points(pm.name.clone()), clickable: pm.operable });
    }
    nearest(scene.tracks.iter().map(|t| (t, dist_to_segment(p, at(t.a), at(t.b)))))
        .map(|t| Hit { target: Target::Section(t.section.clone()), clickable: false })
}
