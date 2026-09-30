//! Layout coordinates ⇄ screen pixels: fit, zoom about a point, pan.

use egui::{Pos2, Rect, Vec2};

/// Pixels per layout unit, at the least and the most.
pub const MIN_SCALE: f32 = 0.02;
pub const MAX_SCALE: f32 = 50.0;
/// Fit leaves this fraction of the screen around the drawing.
pub const FIT_MARGIN: f32 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// The layout point at the centre of the screen rectangle.
    pub centre: Pos2,
    /// Pixels per layout unit.
    pub scale: f32,
}

fn usable(r: Rect) -> bool {
    r.min.is_finite() && r.max.is_finite() && r.width() >= 1.0 && r.height() >= 1.0
}

impl Camera {
    /// Frame `world` in `screen`, keeping the aspect ratio.
    pub fn fit(world: Rect, screen: Rect) -> Camera {
        if !world.min.is_finite() || !world.max.is_finite() {
            return Camera { centre: Pos2::ZERO, scale: 1.0 };
        }
        let centre = world.center();
        if !usable(screen) {
            return Camera { centre, scale: 1.0 };
        }
        let room = screen.size() * (1.0 - 2.0 * FIT_MARGIN);
        let fits = [room.x / world.width(), room.y / world.height()];
        let scale = fits.into_iter().filter(|s| s.is_finite() && *s > 0.0).fold(f32::INFINITY, f32::min);
        let scale = if scale.is_finite() { scale } else { 1.0 };
        Camera { centre, scale: scale.clamp(MIN_SCALE, MAX_SCALE) }
    }

    pub fn to_screen(&self, screen: Rect, p: Pos2) -> Pos2 {
        screen.center() + (p - self.centre) * self.scale
    }

    pub fn to_world(&self, screen: Rect, p: Pos2) -> Pos2 {
        self.centre + (p - screen.center()) / self.scale
    }

    /// Zoom by `factor` keeping the layout point under `at` where it is.
    pub fn zoom_at(&mut self, screen: Rect, at: Pos2, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 || !at.is_finite() {
            return;
        }
        let before = self.to_world(screen, at);
        self.scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let after = self.to_world(screen, at);
        self.centre += before - after;
    }

    /// Drag the drawing by `delta` pixels.
    pub fn pan(&mut self, delta: Vec2) {
        if delta.is_finite() {
            self.centre -= delta / self.scale;
        }
    }
}
