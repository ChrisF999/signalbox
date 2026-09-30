//! The drawing: colours and marks, checked as shapes (no GPU, no fonts).

mod common;

use client_ui::camera::Camera;
use client_ui::paint::*;
use client_ui::scene::Scene;
use common::*;
use egui::{Color32, Pos2, Rect, Shape, pos2, vec2};
use protocol::*;

fn screen() -> Rect {
    Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0))
}

fn lines_of(d: &Drawing, colour: Color32) -> Vec<[Pos2; 2]> {
    d.shapes
        .iter()
        .filter_map(|s| match s {
            Shape::LineSegment { points, stroke } if stroke.color == colour && stroke.width == TRACK_W => Some(*points),
            _ => None,
        })
        .collect()
}

fn circles(d: &Drawing) -> Vec<(Pos2, f32, Color32, Color32)> {
    d.shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Circle(c) => Some((c.center, c.radius, c.fill, c.stroke.color)),
            _ => None,
        })
        .collect()
}

struct Rig {
    sc: Scene,
    cam: Camera,
    view: View,
}

impl Rig {
    fn new(area: Option<&str>) -> Rig {
        let sc = Scene::build(&layout_for(area)).unwrap();
        let cam = Camera::fit(sc.all.unwrap(), screen());
        Rig { sc, cam, view: view_for(area) }
    }

    fn draw(&self, selected: Option<&str>, exits: &[ExitName], flashing: Option<&str>, time: f64) -> Drawing {
        draw(&self.sc, &self.cam, screen(), &PaintState { view: Some(&self.view), selected, exits, flashing, time })
    }

    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.cam.to_screen(screen(), pos2(x, y))
    }
}

#[test]
fn colours_follow_the_spec() {
    let v = |occupied, held| SectionView { occupied, held };
    assert_eq!(track_colour(None), TRACK_FREE);
    assert_eq!(track_colour(Some(&v(false, Held::Free))), TRACK_FREE);
    assert_eq!(track_colour(Some(&v(false, Held::Path))), ROUTE);
    assert_eq!(track_colour(Some(&v(false, Held::Overlap))), OVERLAP);
    assert_eq!(track_colour(Some(&v(true, Held::Path))), OCCUPIED, "occupied wins");
    assert_eq!(dim(Color32::from_rgb(200, 100, 51)), Color32::from_rgb(100, 50, 25));
    assert_eq!(lamps(Aspect::DoubleYellow), (YELLOW, Some(YELLOW)));
    assert_eq!(lamps(Aspect::Green), (GREEN, None));
    assert_eq!(BG, Color32::from_rgb(0x0B, 0x0B, 0x0F));
    assert!(blink_on(0.0) && !blink_on(0.3) && blink_on(0.5) && blink_on(-0.6) == blink_on(0.4));
}

#[test]
fn track_is_grey_white_along_a_route_red_when_occupied_and_dim_on_the_fringe() {
    let mut r = Rig::new(Some("West"));
    let d = r.draw(None, &[], None, 0.0);
    assert_eq!(lines_of(&d, TRACK_FREE), [[r.at(0.0, 0.0), r.at(100.0, 0.0)], [r.at(100.0, 0.0), r.at(200.0, 0.0)]]);
    assert_eq!(lines_of(&d, dim(TRACK_FREE)).len(), 3, "P's three legs are East's, on West's fringe");
    r.view.sections.insert(s("TW2"), SectionView { occupied: false, held: Held::Path });
    r.view.sections.insert(s("TW1"), SectionView { occupied: true, held: Held::Free });
    let d = r.draw(None, &[], None, 0.0);
    assert_eq!(lines_of(&d, ROUTE), [[r.at(100.0, 0.0), r.at(200.0, 0.0)]]);
    assert_eq!(lines_of(&d, OCCUPIED), [[r.at(0.0, 0.0), r.at(100.0, 0.0)]]);
}

#[test]
fn points_show_the_lying_leg_whole_and_a_gap_in_the_other() {
    let mut r = Rig::new(Some("East"));
    let (c, n, rv) = (r.at(207.5, 0.0), r.at(215.0, 0.0), r.at(215.0, 10.0));
    let d = r.draw(None, &[], None, 0.0);
    let legs = lines_of(&d, TRACK_FREE);
    assert!(legs.contains(&[c, n]), "normal lies: whole");
    assert!(legs.contains(&[c + (rv - c) * GAP, rv]), "reverse: from the gap");
    r.view.points.insert(s("P"), PointsView { position: PointsPos::Reverse, moving: true, locked: false });
    let open = lines_of(&r.draw(None, &[], None, 0.0), TRACK_FREE);
    assert!(open.contains(&[c, rv]) && open.contains(&[c + (n - c) * GAP, n]));
    assert!(!open.contains(&[c, c + (n - c) * GAP]), "the gap open");
    let shut = lines_of(&r.draw(None, &[], None, 0.3), TRACK_FREE);
    assert!(shut.contains(&[c, rv]), "the lying leg stays while moving");
    assert!(shut.contains(&[c + (n - c) * GAP, n]) && shut.contains(&[c, c + (n - c) * GAP]), "while moving, the gap flashes");
    r.view.points.get_mut("P").unwrap().moving = false;
    let still = lines_of(&r.draw(None, &[], None, 0.3), TRACK_FREE);
    assert!(!still.contains(&[c, c + (n - c) * GAP]), "a gap that is not moving stays open");
}

#[test]
fn fringe_exit_markers_are_dim() {
    let r = Rig::new(Some("West"));
    let d = r.draw(None, &[], None, 0.0);
    let square = |at: Pos2| {
        d.shapes
            .iter()
            .find_map(|s| match s {
                Shape::Rect(rs) if rs.rect.width() == 7.0 && rs.rect.center() == at => Some(rs.stroke.color),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(square(r.at(0.0, 0.0)), TRACK_FREE, "W is West's");
    assert_eq!(square(r.at(400.0, 0.0)), dim(TRACK_FREE), "E is East's");
    let lit = r.draw(None, &[ExitName::Node(s("E"))], None, 0.0);
    assert!(lit.shapes.iter().any(|s| matches!(s, Shape::Rect(rs) if rs.rect.width() == 7.0 && rs.stroke.color == SELECT)), "a lit exit is lit in full");
}

#[test]
fn signals_show_their_aspect_and_selection() {
    let mut r = Rig::new(Some("West"));
    r.view.signals.insert(s("W1"), Aspect::DoubleYellow);
    let exits = [ExitName::Signal(s("A"))];
    let d = r.draw(Some("W1"), &exits, None, 0.0);
    let cs = circles(&d);
    let w1 = r.at(100.0, -5.0);
    assert!(cs.contains(&(w1, LAMP_R, YELLOW, Color32::TRANSPARENT)));
    assert!(cs.iter().any(|c| c.0 == w1 + vec2(LAMP_R * 2.2, 0.0) && c.2 == YELLOW), "second yellow lamp along the direction of travel");
    assert!(cs.iter().any(|c| c.0 == w1 && c.3 == SELECT), "the entrance is ringed");
    assert!(cs.iter().any(|c| c.0 == r.at(200.0, -5.0) && c.3 == SELECT), "so is the lit exit");
    assert!(cs.contains(&(r.at(100.0, 5.0), LAMP_R, RED, Color32::TRANSPARENT)), "W2 red");
}

#[test]
fn a_refused_entrance_flashes() {
    let r = Rig::new(Some("West"));
    let flash = |t| circles(&r.draw(None, &[], Some("A"), t)).iter().any(|c| c.3 == FLASH);
    assert!(flash(0.0));
    assert!(!flash(0.3));
}

#[test]
fn berths_show_headcodes_in_yellow_and_exits_light_up() {
    let mut r = Rig::new(Some("West"));
    r.view.berths.insert(s("BA"), s("1E01"));
    let lit = [ExitName::Node(s("E"))];
    let d = r.draw(Some("A"), &lit, None, 0.0);
    let t = d.texts.iter().find(|t| t.text == "1E01").unwrap();
    assert_eq!((t.colour, t.monospace), (HEADCODE, true));
    let squares: Vec<Color32> = d
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Rect(rs) if rs.rect.width() == 7.0 => Some(rs.stroke.color),
            _ => None,
        })
        .collect();
    assert_eq!(squares.iter().filter(|c| **c == SELECT).count(), 1, "E lit, N and W not");
    assert_eq!(squares.len(), 3);
    assert!(d.texts.iter().any(|t| t.text == "West" && t.colour == LABEL));
}

#[test]
fn automatic_signals_carry_an_a_lit_while_auto_working() {
    let mut l = layout_for(Some("West"));
    l.routes[0].automatic = true;
    let sc = Scene::build(&l).unwrap();
    let cam = Camera::fit(sc.all.unwrap(), screen());
    let mut v = view_for(Some("West"));
    let a_colour = |v: &View| {
        let d = draw(&sc, &cam, screen(), &PaintState { view: Some(v), selected: None, exits: &[], flashing: None, time: 0.0 });
        d.texts.iter().find(|t| t.text == "A").map(|t| t.colour)
    };
    assert_eq!(a_colour(&v), Some(AUTO_OFF));
    v.routes.insert(s("W1-A"), RouteView { state: RouteState::Locked, auto_working: true });
    assert_eq!(a_colour(&v), Some(AUTO_ON));
}

#[test]
fn no_view_yet_draws_everything_idle() {
    let r = Rig::new(Some("West"));
    let d = draw(&r.sc, &r.cam, screen(), &PaintState { view: None, selected: None, exits: &[], flashing: None, time: 0.0 });
    assert_eq!(lines_of(&d, TRACK_FREE).len(), 2);
    assert!(circles(&d).iter().filter(|c| c.2 == RED).count() == 3, "signals default to red");
}
