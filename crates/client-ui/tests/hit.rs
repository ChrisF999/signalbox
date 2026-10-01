//! What is under the pointer.

mod common;

use client_core::Target;
use client_ui::camera::Camera;
use client_ui::hit::{AUTO_AHEAD_PX, BERTH_H, HIT_PX, Hit, auto_button, berth_rect, hit_test, signal_disc};
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
fn a_click_in_the_widened_part_of_a_berth_box_hits_the_berth() {
    use client_ui::hit::{BERTH_W, berth_box, berth_width};
    let mut l = layout_for(Some("West"));
    l.simplifier.push(protocol::SimplifierRow { headcode: "W118400".into(), origin: None, destination: None, calls: vec![] });
    let sc = Scene::build(&l).unwrap();
    let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0));
    let cam = Camera::fit(sc.all.unwrap(), screen);
    let b = sc.berths.iter().find(|b| b.name == "BA").unwrap();
    assert!(b.width_px > BERTH_W && b.width_px == berth_width(7));
    let r = berth_box(&cam, screen, b);
    let p = pos2(r.left() + 1.0, r.center().y);
    assert!(p.x < r.center().x - BERTH_W / 2.0, "outside a BERTH_W box");
    assert_eq!(hit_test(&sc, None, &cam, screen, p), hit(Target::Berth(s("BA")), true));
}

#[test]
fn signals_berths_exits_points_and_track() {
    let (sc, cam, screen) = setup(Some("West"));
    assert_eq!(hit_test(&sc, None, &cam, screen, at(&cam, screen, 200.0, -5.0)), hit(Target::Signal(s("A")), true));
    let near = at(&cam, screen, 200.0, -5.0) + vec2(HIT_PX - 1.0, 0.0);
    assert_eq!(hit_test(&sc, None, &cam, screen, near), hit(Target::Signal(s("A")), true), "within the radius");
    let ba = sc.berths.iter().find(|b| b.name == "BA").unwrap();
    let ba = berth_rect(&cam, screen, ba.at, ba.offset_px).center();
    assert_eq!(hit_test(&sc, None, &cam, screen, ba), hit(Target::Berth(s("BA")), true), "in the track behind A");
    let bw = berth_rect(&cam, screen, pos2(0.0, 0.0), sc.berths[3].offset_px);
    assert_eq!(hit_test(&sc, None, &cam, screen, bw.center()), hit(Target::Berth(s("BW")), true));
    assert_eq!(hit_test(&sc, None, &cam, screen, at(&cam, screen, 0.0, 0.0)), hit(Target::Exit(s("W")), true), "under the berth box");
    assert_eq!(hit_test(&sc, None, &cam, screen, at(&cam, screen, 207.5, 0.0)), hit(Target::Points(s("P")), false), "East's points");
    assert_eq!(hit_test(&sc, None, &cam, screen, at(&cam, screen, 50.0, 0.0)), hit(Target::Section(s("TW1")), false));
    assert_eq!(hit_test(&sc, None, &cam, screen, at(&cam, screen, 50.0, 0.0) + vec2(0.0, HIT_PX + 1.0)), None);
    assert_eq!(hit_test(&sc, None, &cam, screen, pos2(999.0, 599.0)), None);
    assert!(BERTH_H < HIT_PX * 2.0);
    let w1 = sc.signals.iter().find(|s| s.name == "W1").unwrap();
    let disc = signal_disc(&cam, screen, w1);
    assert!(disc.distance(at(&cam, screen, 100.0, -5.0)) > HIT_PX, "the disc is away from W1's own point");
    assert_eq!(hit_test(&sc, None, &cam, screen, disc), hit(Target::Signal(s("W1")), true), "W1's disc");
}

#[test]
fn the_nearest_signal_wins() {
    let (sc, _, screen) = setup(Some("West"));
    let far = Camera { centre: pos2(100.0, 0.0), scale: 0.1 };
    let p = far.to_screen(screen, pos2(100.0, -5.0)) + vec2(0.0, 0.3);
    assert_eq!(hit_test(&sc, None, &far, screen, p), hit(Target::Signal(s("W1")), true), "W1 and W2 overlap when zoomed out; W1 is nearer");
}

#[test]
fn a_spectator_can_hover_but_not_work() {
    let (sc, cam, screen) = setup(None);
    assert_eq!(hit_test(&sc, None, &cam, screen, at(&cam, screen, 215.0, 5.0)), hit(Target::Signal(s("C")), false));
    assert_eq!(hit_test(&sc, None, &cam, screen, at(&cam, screen, 207.5, 0.0)), hit(Target::Points(s("P")), false));
}

#[test]
fn the_exit_of_a_route_you_can_set_is_clickable_even_on_the_fringe() {
    let (sc, cam, screen) = setup(Some("East"));
    assert_eq!(hit_test(&sc, None, &cam, screen, at(&cam, screen, 100.0, 5.0)), hit(Target::Signal(s("W2")), true), "W2 ends C-W2 and D-W2");
    assert_eq!(hit_test(&sc, None, &cam, screen, at(&cam, screen, 200.0, -5.0)), hit(Target::Signal(s("A")), false), "A ends none of East's routes");
    let (sc, cam, screen) = setup(Some("West"));
    assert_eq!(hit_test(&sc, None, &cam, screen, at(&cam, screen, 400.0, 0.0)), hit(Target::Exit(s("E")), true), "E: West's A-E, on the fringe");
}

#[test]
fn a_spectator_cannot_click_an_exit_marker() {
    let (sc, cam, screen) = setup(None);
    let w = sc.exits.iter().find(|e| e.node == "W").unwrap().at;
    assert_eq!(hit_test(&sc, None, &cam, screen, at(&cam, screen, w.x, w.y)), hit(Target::Exit(s("W")), false));
}

#[test]
fn the_auto_button_is_its_own_target() {
    let (sc, cam, screen) = setup(Some("West"));
    let w1 = sc.signals.iter().find(|s| s.name == "W1").unwrap();
    let c = auto_button(&cam, screen, w1).expect("W1 is a controlled signal with a route");
    // Zoomed in past the track's widest, the button sits further out with
    // the bigger glyphs (polish spec M11).
    assert_eq!(c, signal_disc(&cam, screen, w1) + vec2(AUTO_AHEAD_PX * client_ui::paint::glyph(cam.scale), 0.0));
    assert_eq!(hit_test(&sc, None, &cam, screen, c), hit(Target::Auto(s("W1")), true));
    assert_eq!(hit_test(&sc, None, &cam, screen, signal_disc(&cam, screen, w1)), hit(Target::Signal(s("W1")), true));
    let mut l = layout_for(Some("West"));
    l.routes[0].automatic = true;
    let sc = Scene::build(&l).unwrap();
    let w1 = sc.signals.iter().find(|s| s.name == "W1").unwrap();
    assert_eq!(auto_button(&cam, screen, w1), None, "W1 is permanently automatic");
    assert_ne!(hit_test(&sc, None, &cam, screen, c).map(|h| h.target), Some(Target::Auto(s("W1"))));
}

/// C turned to face away from P, about 20 px past it: its berth, in the
/// track behind it, lies over the points.
fn c_past_the_points() -> (Scene, Camera, Rect) {
    let mut l = layout_for(Some("East"));
    l.geometry.as_mut().unwrap().signals.iter_mut().find(|s| s.signal == "C").unwrap().facing = Some([1.0, 0.0]);
    let sc = Scene::build(&l).unwrap();
    let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0));
    let cam = Camera::fit(sc.all.unwrap(), screen);
    (sc, cam, screen)
}

#[test]
fn an_empty_berth_never_hides_points_or_an_exit() {
    let (sc, cam, screen) = c_past_the_points();
    let p = at(&cam, screen, 207.5, 0.0);
    let past = at(&cam, screen, 215.0, 0.0).x - p.x;
    assert!((15.0..25.0).contains(&past), "C's base is {past} px past P");
    let bc = sc.berths.iter().find(|b| b.name == "BC").unwrap();
    assert!(berth_rect(&cam, screen, bc.at, bc.offset_px).contains(p), "BC's box covers P");
    assert_eq!(hit_test(&sc, None, &cam, screen, p), hit(Target::Points(s("P")), true), "BC is empty: the points win");
    let e = at(&cam, screen, 400.0, 0.0);
    let mut moved = sc.clone();
    moved.berths.iter_mut().find(|b| b.name == "BC").unwrap().at = pos2(400.0, 0.0);
    moved.berths.iter_mut().find(|b| b.name == "BC").unwrap().offset_px = vec2(-3.0, 0.0);
    assert_eq!(hit_test(&moved, None, &cam, screen, e), hit(Target::Exit(s("E")), false), "and the exit wins too");
    let mut v = view_for(Some("East"));
    v.berths.insert(s("BC"), s("2W03"));
    assert_eq!(hit_test(&sc, Some(&v), &cam, screen, p), hit(Target::Berth(s("BC")), true), "a headcode shown there wins");
    // An empty berth with nothing else near still answers: BA in `signals_berths_exits_points_and_track`.
}

/// No ○A on the fringe, so nothing to hit there; a spectator's is hover only.
#[test]
fn a_fringe_signal_has_no_auto_button_and_a_spectators_is_not_clickable() {
    let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0));
    let sc = Scene::build(&layout_for(None)).unwrap();
    let cam = Camera::fit(sc.all.unwrap(), screen);
    let a = sc.signals.iter().find(|s| s.name == "A").unwrap();
    let c = auto_button(&cam, screen, a).expect("a spectator sees A's ○A");
    assert_eq!(hit_test(&sc, None, &cam, screen, c), hit(Target::Auto(s("A")), false));
    let east = Scene::build(&layout_for(Some("East"))).unwrap();
    let a = east.signals.iter().find(|s| s.name == "A").unwrap();
    assert_eq!(auto_button(&cam, screen, a), None, "A is on East's fringe");
    assert!(east.signals.iter().all(|s| auto_button(&cam, screen, s).is_none_or(|b| b.distance(c) > 10.0)));
    assert_ne!(hit_test(&east, None, &cam, screen, c).map(|h| h.target), Some(Target::Auto(s("A"))));
}
