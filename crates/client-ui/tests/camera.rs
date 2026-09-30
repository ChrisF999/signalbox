//! Fit, zoom and pan.

use client_ui::camera::{Camera, MAX_SCALE, MIN_SCALE};
use egui::{Pos2, Rect, pos2, vec2};

fn screen() -> Rect {
    Rect::from_min_size(pos2(0.0, 40.0), vec2(1000.0, 600.0))
}

fn close(a: Pos2, b: Pos2) -> bool {
    a.distance(b) < 1e-3
}

#[test]
fn fit_centres_the_drawing_and_keeps_a_margin() {
    let world = Rect::from_min_max(pos2(0.0, -5.0), pos2(200.0, 5.0));
    let cam = Camera::fit(world, screen());
    assert_eq!(cam.centre, pos2(100.0, 0.0));
    assert_eq!(cam.scale, 4.5, "900 px of room across 200 units");
    assert!(close(cam.to_screen(screen(), pos2(0.0, 0.0)), pos2(50.0, 340.0)));
    let tall = Camera::fit(Rect::from_min_max(pos2(0.0, 0.0), pos2(10.0, 540.0)), screen());
    assert_eq!(tall.scale, 1.0, "height decides");
}

#[test]
fn screen_and_world_round_trip() {
    let mut cam = Camera::fit(Rect::from_min_max(pos2(-50.0, 10.0), pos2(3000.0, 700.0)), screen());
    cam.pan(vec2(33.0, -7.0));
    for p in [pos2(0.0, 0.0), pos2(1234.5, 99.0), pos2(-50.0, 700.0)] {
        assert!(close(cam.to_world(screen(), cam.to_screen(screen(), p)), p));
    }
}

#[test]
fn zoom_keeps_the_point_under_the_pointer() {
    let mut cam = Camera { centre: pos2(100.0, 0.0), scale: 2.0 };
    let at = pos2(700.0, 100.0);
    let before = cam.to_world(screen(), at);
    cam.zoom_at(screen(), at, 1.25);
    assert_eq!(cam.scale, 2.5);
    assert!(close(cam.to_world(screen(), at), before));
}

#[test]
fn zoom_is_clamped() {
    let mut cam = Camera { centre: pos2(0.0, 0.0), scale: 1.0 };
    for _ in 0..100 {
        cam.zoom_at(screen(), pos2(500.0, 340.0), 1.5);
    }
    assert_eq!(cam.scale, MAX_SCALE);
    for _ in 0..100 {
        cam.zoom_at(screen(), pos2(500.0, 340.0), 0.5);
    }
    assert_eq!(cam.scale, MIN_SCALE);
    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        cam.zoom_at(screen(), pos2(1.0, 1.0), bad);
    }
    assert_eq!(cam.scale, MIN_SCALE);
    assert!(cam.centre.is_finite());
}

#[test]
fn fit_survives_degenerate_bounds() {
    let point = Rect::from_min_max(pos2(5.0, 5.0), pos2(5.0, 5.0));
    assert_eq!(Camera::fit(point, screen()), Camera { centre: pos2(5.0, 5.0), scale: 1.0 });
    let flat = Rect::from_min_max(pos2(0.0, 7.0), pos2(90_000.0, 7.0));
    let cam = Camera::fit(flat, screen());
    assert!(cam.scale.is_finite() && cam.scale >= MIN_SCALE, "{cam:?}");
    assert_eq!(cam.scale, MIN_SCALE, "90 000 units would need 0.01 px each");
    let nothing = Rect::from_min_size(pos2(0.0, 0.0), vec2(0.0, 0.0));
    assert_eq!(Camera::fit(Rect::from_min_max(pos2(0.0, 0.0), pos2(10.0, 10.0)), nothing).scale, 1.0);
    assert_eq!(Camera::fit(Rect::NOTHING, screen()).scale, 1.0);
    let odd = Rect::from_min_max(pos2(f32::NAN, 0.0), pos2(1.0, 1.0));
    assert_eq!(Camera::fit(odd, screen()), Camera { centre: Pos2::ZERO, scale: 1.0 });
}
