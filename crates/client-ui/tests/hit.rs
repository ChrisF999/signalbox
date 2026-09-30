//! What is under the pointer.

mod common;

use client_core::Target;
use client_ui::camera::Camera;
use client_ui::hit::{BERTH_H, HIT_PX, Hit, berth_rect, hit_test};
use client_ui::scene::Scene;
use common::*;
use egui::{Pos2, Rect, pos2, vec2};

fn setup(area: Option<&str>) -> (Scene, Camera, Rect) {
    let sc = Scene::build(&layout_for(area)).unwrap();
    let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0));
    let cam = Camera::fit(sc.all.unwrap(), screen);
    (sc, cam, screen)
}

fn at(cam: &Camera, screen: Rect, x: f32, y: f32) -> Pos2 {
    cam.to_screen(screen, pos2(x, y))
}

fn hit(t: Target, clickable: bool) -> Option<Hit> {
    Some(Hit { target: t, clickable })
}

#[test]
fn signals_berths_exits_points_and_track() {
    let (sc, cam, screen) = setup(Some("West"));
    assert_eq!(hit_test(&sc, &cam, screen, at(&cam, screen, 200.0, -5.0)), hit(Target::Signal(s("A")), true));
    let near = at(&cam, screen, 200.0, -5.0) + vec2(HIT_PX - 1.0, 0.0);
    assert_eq!(hit_test(&sc, &cam, screen, near), hit(Target::Signal(s("A")), true), "within the radius");
    assert_eq!(hit_test(&sc, &cam, screen, at(&cam, screen, 190.0, -15.0)), hit(Target::Berth(s("BA")), true));
    let bw = berth_rect(&cam, screen, pos2(0.0, 0.0), sc.berths[3].offset_px);
    assert_eq!(hit_test(&sc, &cam, screen, bw.center()), hit(Target::Berth(s("BW")), true));
    assert_eq!(hit_test(&sc, &cam, screen, at(&cam, screen, 0.0, 0.0)), hit(Target::Exit(s("W")), true), "under the berth box");
    assert_eq!(hit_test(&sc, &cam, screen, at(&cam, screen, 207.5, 0.0)), hit(Target::Points(s("P")), false), "East's points");
    assert_eq!(hit_test(&sc, &cam, screen, at(&cam, screen, 50.0, 0.0)), hit(Target::Section(s("TW1")), false));
    assert_eq!(hit_test(&sc, &cam, screen, at(&cam, screen, 50.0, 0.0) + vec2(0.0, HIT_PX + 1.0)), None);
    assert_eq!(hit_test(&sc, &cam, screen, pos2(999.0, 599.0)), None);
    assert!(BERTH_H < HIT_PX * 2.0);
}

#[test]
fn the_nearest_signal_wins() {
    let (sc, _, screen) = setup(Some("West"));
    let far = Camera { centre: pos2(100.0, 0.0), scale: 0.1 };
    let p = far.to_screen(screen, pos2(100.0, -5.0)) + vec2(0.0, 0.3);
    assert_eq!(hit_test(&sc, &far, screen, p), hit(Target::Signal(s("W1")), true), "W1 and W2 overlap when zoomed out; W1 is nearer");
}

#[test]
fn a_spectator_can_hover_but_not_work() {
    let (sc, cam, screen) = setup(None);
    assert_eq!(hit_test(&sc, &cam, screen, at(&cam, screen, 215.0, 5.0)), hit(Target::Signal(s("C")), false));
    assert_eq!(hit_test(&sc, &cam, screen, at(&cam, screen, 207.5, 0.0)), hit(Target::Points(s("P")), false));
}
