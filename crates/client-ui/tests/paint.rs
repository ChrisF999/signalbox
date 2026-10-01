//! The drawing (realism spec §2), checked as shapes (no GPU, no fonts):
//! track, joints, overlaps, signals, numbers, and what flashes.

mod common;

use client_core::{AspectMode, Names};
use client_ui::camera::Camera;
use client_ui::hit::signal_disc;
use client_ui::paint::*;
use client_ui::scene::Scene;
use common::*;
use egui::{Align2, Color32, Pos2, Rect, Shape, pos2, vec2};
use protocol::*;

fn screen() -> Rect {
    Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0))
}

/// Line segments of exactly this colour and width.
fn lines_of(d: &Drawing, colour: Color32, width: f32) -> Vec<[Pos2; 2]> {
    d.shapes
        .iter()
        .filter_map(|s| match s {
            Shape::LineSegment { points, stroke } if stroke.color == colour && stroke.width == width => Some(*points),
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

fn close(a: Pos2, b: Pos2) -> bool {
    a.distance(b) < 1e-3
}

/// `lines` has a segment from `a` to `b` (to within float error).
fn has(lines: &[[Pos2; 2]], a: Pos2, b: Pos2) -> bool {
    lines.iter().any(|l| close(l[0], a) && close(l[1], b))
}

/// `p` pulled back by half a joint gap towards `from`.
fn short(p: Pos2, from: Pos2) -> Pos2 {
    p - (p - from).normalized() * (JOINT_GAP_PX / 2.0)
}

struct Rig {
    layout: Layout,
    sc: Scene,
    cam: Camera,
    view: View,
    names: Names,
    aspects: AspectMode,
    numbers: bool,
}

impl Rig {
    fn new(area: Option<&str>) -> Rig {
        Rig::of(layout_for(area), view_for(area))
    }

    fn of(layout: Layout, view: View) -> Rig {
        let sc = Scene::build(&layout).unwrap();
        let cam = Camera::fit(sc.all.unwrap(), screen());
        let names = Names::new(&layout);
        Rig { layout, sc, cam, view, names, aspects: AspectMode::RedGreen, numbers: true }
    }

    fn draw(&self, selected: Option<&str>, exits: &[ExitName], refused: Option<&str>, time: f64) -> Drawing {
        let st = PaintState {
            view: Some(&self.view),
            selected,
            exits,
            refused,
            time,
            aspects: self.aspects,
            numbers: self.numbers,
            names: &self.names,
        };
        draw(&self.sc, &self.cam, screen(), &st)
    }

    fn idle(&self) -> Drawing {
        self.draw(None, &[], None, 0.0)
    }

    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.cam.to_screen(screen(), pos2(x, y))
    }

    fn w(&self) -> f32 {
        track_w(self.cam.scale)
    }

    fn disc(&self, name: &str) -> Pos2 {
        signal_disc(&self.cam, screen(), self.sc.signals.iter().find(|s| s.name == name).unwrap())
    }
}

#[test]
fn the_palette_is_the_specs() {
    assert_eq!(BG, Color32::BLACK);
    assert_eq!(
        [TRACK_FREE, ROUTE, OCCUPIED, HEADCODE, AUTO, PLATFORM, LABEL],
        [
            Color32::from_rgb(0x7D, 0x7D, 0x7D),
            Color32::WHITE,
            Color32::from_rgb(0xE8, 0x14, 0x1C),
            Color32::from_rgb(0x39, 0xE0, 0xFF),
            Color32::from_rgb(0x1D, 0x4F, 0xD8),
            Color32::from_rgb(0xB8, 0x86, 0x0B),
            Color32::from_rgb(0x9A, 0x9A, 0x9A),
        ]
    );
    let v = |occupied, held| SectionView { occupied, held };
    assert_eq!(track_colour(None), TRACK_FREE);
    assert_eq!(track_colour(Some(&v(false, Held::Path))), ROUTE);
    assert_eq!(track_colour(Some(&v(false, Held::Overlap))), ROUTE, "overlaps are white like the route");
    assert_eq!(track_colour(Some(&v(true, Held::Overlap))), OCCUPIED, "occupied wins");
    assert!(blink_on(0.0) && !blink_on(0.3) && blink_on(0.5) && blink_on(-0.6) == blink_on(0.4));
    assert_eq!(
        [FRINGE, SELECT, REFUSED, RED, YELLOW, GREEN],
        [
            Color32::from_rgb(0x6E, 0x6E, 0x6E),
            Color32::from_rgb(0x00, 0xC8, 0xFF),
            Color32::from_rgb(0xFF, 0x3C, 0xFF),
            Color32::from_rgb(0xE6, 0x1E, 0x1E),
            Color32::from_rgb(0xFA, 0xD2, 0x00),
            Color32::from_rgb(0x00, 0xDC, 0x50),
        ]
    );
    let all = [Aspect::Red, Aspect::Yellow, Aspect::DoubleYellow, Aspect::Green];
    assert_eq!(
        all.map(|a| signal_lamps(a, AspectMode::RedGreen)),
        [(RED, None), (GREEN, None), (GREEN, None), (GREEN, None)]
    );
    assert_eq!(
        all.map(|a| signal_lamps(a, AspectMode::Real)),
        [(RED, None), (YELLOW, None), (YELLOW, Some(YELLOW)), (GREEN, None)]
    );
}

#[test]
fn signal_colours_in_both_modes() {
    for a in [Aspect::Yellow, Aspect::DoubleYellow, Aspect::Green] {
        assert_eq!(signal_lamps(a, AspectMode::RedGreen), (GREEN, None), "any proceed aspect is green: {a:?}");
    }
    assert_eq!(signal_lamps(Aspect::Red, AspectMode::RedGreen), (RED, None));
    assert_eq!(signal_lamps(Aspect::DoubleYellow, AspectMode::Real), (YELLOW, Some(YELLOW)));
    assert_eq!(signal_lamps(Aspect::Green, AspectMode::Real), (GREEN, None));
}

#[test]
fn track_is_thick_within_limits_and_numbers_hide_when_small() {
    assert_eq!(track_w(1.0), 9.0, "about three times D1's 3 px");
    assert_eq!((track_w(0.01), track_w(100.0), track_w(f32::NAN)), (TRACK_MIN_PX, TRACK_MAX_PX, TRACK_MIN_PX));
    assert_eq!(number_px(1.0), Some(NUMBER_MAX_PX));
    assert_eq!(number_px(0.5), Some(8.0));
    assert_eq!(number_px(0.4), None, "6.4 px is below the 7 px minimum");
}

/// West: w1 (0,0)–(100,0) in TW1 and w2 (100,0)–(200,0) in TW2 meet at J0;
/// w2 meets the points' section TP at J1; W is a plain end.
#[test]
fn track_circuit_joints_are_gaps() {
    let r = Rig::new(Some("West"));
    let d = r.idle();
    let h = vec2(JOINT_GAP_PX / 2.0, 0.0);
    assert_eq!(
        lines_of(&d, TRACK_FREE, r.w()),
        [[r.at(0.0, 0.0), r.at(100.0, 0.0) - h], [r.at(100.0, 0.0) + h, r.at(200.0, 0.0) - h]],
        "a joint at J0 and at J1, none at the boundary W"
    );
    let w1 = &r.sc.tracks[0];
    assert_eq!((w1.a_meets.clone(), w1.b_meets.clone()), (Vec::<String>::new(), vec![s("TW2")]));
    assert!(!w1.joint_a() && w1.joint_b());
    // The points' side of J1: TP's toe leg stops half a gap short of w2.
    // (East's lines are not drawn for West: the other legs meet nothing.)
    let p = &r.sc.points[0];
    assert_eq!((p.toe_meets.clone(), p.normal_meets.clone(), p.reverse_meets.clone()), (vec![s("TW2")], vec![], vec![]));
    let edges = lines_of(&d, TRACK_FREE, FRINGE_EDGE_PX);
    let (c, j1) = (r.at(207.5, 0.0), r.at(200.0, 0.0) + h);
    let half = vec2(0.0, r.w() / 2.0);
    assert!(has(&edges, c + half, j1 + half) && has(&edges, c - half, j1 - half), "{edges:?}");
}

#[test]
fn routes_are_white_occupation_red_and_an_overlap_ends_in_a_tick() {
    let mut r = Rig::new(Some("West"));
    r.view.sections.insert(s("TW1"), SectionView { occupied: true, held: Held::Path });
    r.view.sections.insert(s("TW2"), SectionView { occupied: false, held: Held::Overlap });
    let d = r.idle();
    let h = vec2(JOINT_GAP_PX / 2.0, 0.0);
    assert_eq!(lines_of(&d, OCCUPIED, r.w()), [[r.at(0.0, 0.0), r.at(100.0, 0.0) - h]]);
    assert_eq!(lines_of(&d, ROUTE, r.w()), [[r.at(100.0, 0.0) + h, r.at(200.0, 0.0) - h]]);
    let end = r.at(200.0, 0.0) - h;
    let half = vec2(0.0, (r.w() + TICK_EXTRA_PX) / 2.0);
    let ticks = lines_of(&d, ROUTE, TICK_W);
    assert_eq!(ticks.len(), 1, "only the far end: TW1 before it is held");
    assert!(close(ticks[0][0], end - half) && close(ticks[0][1], end + half) || close(ticks[0][0], end + half) && close(ticks[0][1], end - half), "{ticks:?}");
    r.view.sections.insert(s("TW2"), SectionView { occupied: false, held: Held::Path });
    assert!(lines_of(&r.idle(), ROUTE, TICK_W).is_empty(), "a route's own end has no tick");
}

#[test]
fn fringe_track_is_hollow_not_dimmed() {
    let r = Rig::new(Some("West"));
    let d = r.idle();
    let edges = lines_of(&d, TRACK_FREE, FRINGE_EDGE_PX);
    assert_eq!(edges.len(), 6, "P's three legs, two edges each");
    let (c, n) = (r.at(207.5, 0.0), r.at(215.0, 0.0));
    let half = r.w() / 2.0;
    // East's line e is not drawn for West: no joint at the normal leg's end.
    assert!(has(&edges, c + vec2(0.0, half), n + vec2(0.0, half)), "{edges:?}");
    assert!(has(&edges, c - vec2(0.0, half), n - vec2(0.0, half)));
    assert_eq!(lines_of(&d, TRACK_FREE, r.w()).len(), 2, "only West's own track is solid");
}

#[test]
fn points_show_the_lying_leg_whole_and_a_gap_in_the_other() {
    let mut r = Rig::new(Some("East"));
    let w = r.w();
    let (c, n, rv) = (r.at(207.5, 0.0), r.at(215.0, 0.0), r.at(215.0, 10.0));
    // Both legs end at a joint (TE, TN), so each stops half a gap short.
    let (n_end, rv_end) = (short(n, c), short(rv, c));
    let legs = lines_of(&r.idle(), TRACK_FREE, w);
    assert!(has(&legs, c, n_end), "normal lies: whole up to its joint {legs:?}");
    assert!(has(&legs, c + (rv - c) * GAP, rv_end), "reverse: from the gap");
    r.view.points.insert(s("P"), PointsView { position: PointsPos::Reverse, moving: true, locked: false });
    let open = lines_of(&r.draw(None, &[], None, 0.0), TRACK_FREE, w);
    assert!(has(&open, c, rv_end) && has(&open, c + (n - c) * GAP, n_end));
    assert!(!has(&open, c, c + (n - c) * GAP), "the gap open");
    let shut = lines_of(&r.draw(None, &[], None, 0.3), TRACK_FREE, w);
    assert!(has(&shut, c, c + (n - c) * GAP), "while moving, the gap flashes");
    r.view.points.get_mut("P").unwrap().moving = false;
    assert!(!has(&lines_of(&r.draw(None, &[], None, 0.3), TRACK_FREE, w), c, c + (n - c) * GAP));
}

/// W1 faces right (+x): its post goes up (the left of travel, y grows
/// downwards) and hooks right. W2 faces left: down, and hooks left.
#[test]
fn a_signal_is_a_disc_on_a_hooked_post_left_of_the_line() {
    let r = Rig::new(Some("West"));
    let d = r.idle();
    let w1 = r.sc.signals.iter().find(|s| s.name == "W1").unwrap();
    assert_eq!(w1.base, pos2(100.0, 0.0), "the post starts on W1's own line, below its drawn point");
    let base = r.at(100.0, 0.0);
    let top = base + vec2(0.0, -POST_PX);
    let posts = lines_of(&d, TRACK_FREE, POST_W);
    assert!(posts.contains(&[base, top]) && posts.contains(&[top, top + vec2(HOOK_PX, 0.0)]), "{posts:?}");
    assert!(close(r.disc("W1"), top + vec2(HOOK_PX + LAMP_R, 0.0)));
    let w2_base = r.at(100.0, 0.0);
    assert!(close(r.disc("W2"), w2_base + vec2(0.0, POST_PX) - vec2(HOOK_PX + LAMP_R, 0.0)), "below, facing left");
    assert!(circles(&d).contains(&(r.disc("W1"), LAMP_R, RED, Color32::TRANSPARENT)));
}

#[test]
fn red_green_by_default_real_aspects_as_an_option() {
    let mut r = Rig::new(Some("West"));
    r.view.signals.insert(s("W1"), Aspect::DoubleYellow);
    let lit = |r: &Rig| circles(&r.idle()).into_iter().filter(|c| c.2 != Color32::TRANSPARENT && c.2 != RED).collect::<Vec<_>>();
    assert_eq!(lit(&r), [(r.disc("W1"), LAMP_R, GREEN, Color32::TRANSPARENT)]);
    r.aspects = AspectMode::Real;
    let facing = vec2(1.0, 0.0);
    assert_eq!(
        lit(&r),
        [(r.disc("W1"), LAMP_R, YELLOW, Color32::TRANSPARENT), (r.disc("W1") + facing * (LAMP_R * 2.2), LAMP_R, YELLOW, Color32::TRANSPARENT)]
    );
}

#[test]
fn the_entrance_post_is_white_while_a_route_is_set_from_it() {
    let mut r = Rig::new(Some("West"));
    r.view.routes.insert(s("W1-A"), RouteView { state: RouteState::Locked, auto_working: false });
    let d = r.idle();
    let base = r.at(100.0, 0.0);
    assert!(lines_of(&d, ROUTE, POST_W).contains(&[base, base + vec2(0.0, -POST_PX)]));
    assert!(!lines_of(&d, TRACK_FREE, POST_W).contains(&[base, base + vec2(0.0, -POST_PX)]));
}

#[test]
fn automatic_signals_have_a_dashed_post() {
    let mut l = layout_for(Some("West"));
    l.routes[0].automatic = true;
    let r = Rig::of(l, view_for(Some("West")));
    let base = r.at(100.0, 0.0);
    // W1's post goes up from `base`; W2's goes down from the same point.
    let near_w1 = |p: &[Pos2; 2]| p[0].distance(base) < POST_PX + HOOK_PX + 1.0 && p[0].y <= base.y && p[1].y <= base.y;
    let dashes: Vec<[Pos2; 2]> = lines_of(&r.idle(), TRACK_FREE, POST_W).into_iter().filter(near_w1).collect();
    assert!(dashes.len() > 2, "{dashes:?}");
    assert!(dashes.iter().all(|p| p[0].distance(p[1]) <= DASH_PX + 1e-3), "{dashes:?}");
}

#[test]
fn numbers_sit_beside_the_disc_and_hide_when_small_or_off() {
    let mut r = Rig::new(Some("West"));
    let d = r.idle();
    let t = d.texts.iter().find(|t| t.text == "TAW1").expect("W1 as the screen names it");
    assert_eq!((t.colour, t.monospace, t.anchor, t.size), (LABEL, true, Align2::CENTER_BOTTOM, NUMBER_MAX_PX));
    assert!(close(t.at, r.disc("W1") + vec2(0.0, -(LAMP_R + 2.0))), "above the disc");
    let w2 = d.texts.iter().find(|t| t.text == "TAW2").unwrap();
    assert_eq!(w2.anchor, Align2::CENTER_TOP, "W2's post goes down, its number below");
    r.numbers = false;
    assert!(r.idle().texts.iter().all(|t| !t.text.starts_with("TA")));
    r.numbers = true;
    r.cam.scale = 0.3;
    assert!(r.idle().texts.iter().all(|t| !t.text.starts_with("TA")), "4.8 px is too small to read");
}

#[test]
fn fringe_signals_and_their_numbers_are_grey() {
    let mut r = Rig::new(Some("East"));
    r.view.signals.insert(s("A"), Aspect::Green);
    let d = r.idle();
    assert!(circles(&d).contains(&(r.disc("A"), LAMP_R, FRINGE, Color32::TRANSPARENT)), "{:?}", circles(&d));
    assert_eq!(d.texts.iter().find(|t| t.text == "TAA").unwrap().colour, FRINGE);
    assert_eq!(d.texts.iter().find(|t| t.text == "TBC").unwrap().colour, LABEL);
}

/// Owner decision 9: only points moving, the selected entrance and a route
/// cancelling under approach locking flash.
#[test]
fn only_the_listed_states_flash() {
    let mut r = Rig::new(Some("West"));
    let exits = [ExitName::Signal(s("A"))];
    assert_eq!(r.draw(None, &exits, Some("W2"), 0.0), r.draw(None, &exits, Some("W2"), 0.3), "lit exits and refusals are steady");
    let ring = |d: &Drawing, colour: Color32| circles(d).iter().any(|c| c.0 == r.disc("W1") && c.3 == colour);
    assert!(ring(&r.draw(Some("W1"), &[], None, 0.0), SELECT) && !ring(&r.draw(Some("W1"), &[], None, 0.3), SELECT), "the entrance blinks");
    assert!(circles(&r.draw(None, &exits, None, 0.3)).iter().any(|c| c.0 == r.disc("A") && c.3 == SELECT));
    let refused = r.draw(None, &[], Some("W2"), 0.3);
    assert!(circles(&refused).iter().any(|c| c.0 == r.disc("W2") && c.3 == REFUSED && c.1 == LAMP_R + 6.0));
    r.view.routes.insert(s("W1-A"), RouteView { state: RouteState::Cancelling, auto_working: false });
    let on = circles(&r.draw(None, &[], None, 0.0));
    let off = circles(&r.draw(None, &[], None, 0.3));
    assert!(on.contains(&(r.disc("W1"), LAMP_R, RED, Color32::TRANSPARENT)));
    assert!(off.iter().any(|c| c.0 == r.disc("W1") && c.2 == Color32::TRANSPARENT && c.3 == RED), "unlit half: {off:?}");
}

#[test]
fn exit_markers_and_headcodes_in_the_new_colours() {
    let mut r = Rig::new(Some("West"));
    r.view.berths.insert(s("BA"), s("1E01"));
    let d = r.draw(Some("A"), &[ExitName::Node(s("E"))], None, 0.0);
    let squares: Vec<(Pos2, Color32)> = d
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Rect(rs) if rs.rect.width() == 7.0 => Some((rs.rect.center(), rs.stroke.color)),
            _ => None,
        })
        .collect();
    assert!(squares.contains(&(r.at(0.0, 0.0), TRACK_FREE)) && squares.contains(&(r.at(400.0, 60.0), FRINGE)));
    assert!(squares.contains(&(r.at(400.0, 0.0), SELECT)), "a lit exit is lit in full");
    let t = d.texts.iter().find(|t| t.text == "1E01").unwrap();
    assert_eq!((t.colour, t.monospace), (HEADCODE, true));
}

#[test]
fn no_view_yet_draws_everything_idle() {
    let r = Rig::new(Some("West"));
    let names = Names::new(&r.layout);
    let st = PaintState {
        view: None,
        selected: None,
        exits: &[],
        refused: None,
        time: 0.0,
        aspects: AspectMode::RedGreen,
        numbers: true,
        names: &names,
    };
    let d = draw(&r.sc, &r.cam, screen(), &st);
    assert_eq!(lines_of(&d, TRACK_FREE, r.w()).len(), 2);
    assert_eq!(circles(&d).iter().filter(|c| c.2 == RED).count(), 3, "signals default to red");
}

#[test]
fn a_signal_without_a_facing_is_a_bare_disc_at_its_point() {
    let mut l = layout_for(Some("West"));
    l.geometry.as_mut().unwrap().signals.iter_mut().find(|s| s.signal == "W1").unwrap().facing = None;
    let r = Rig::of(l, view_for(Some("West")));
    let d = r.idle();
    assert!(close(r.disc("W1"), r.at(100.0, -5.0)));
    assert!(circles(&d).contains(&(r.at(100.0, -5.0), LAMP_R, RED, Color32::TRANSPARENT)));
    let base = r.at(100.0, 0.0);
    assert!(!lines_of(&d, TRACK_FREE, POST_W).iter().any(|p| p[0] == base && p[1].y < base.y), "no post");
    assert!(d.texts.iter().any(|t| t.text == "TAW1" && t.anchor == Align2::CENTER_BOTTOM), "its number above it");
}

/// An overlap that runs into points ends at the far end of the lying leg
/// (spec owner decision 3), not at the points' toe where the route meets it.
#[test]
fn an_overlap_through_points_ends_in_a_tick_at_the_lying_leg() {
    let mut r = Rig::new(Some("West"));
    r.view.sections.insert(s("TW2"), SectionView { occupied: false, held: Held::Path });
    r.view.sections.insert(s("TP"), SectionView { occupied: false, held: Held::Overlap });
    // East's lines are not drawn for West, so the legs end where they are drawn.
    let end = r.at(215.0, 0.0);
    let half = vec2(0.0, (r.w() + TICK_EXTRA_PX) / 2.0);
    let ticks = lines_of(&r.idle(), ROUTE, TICK_W);
    assert_eq!(ticks.len(), 1, "the toe meets TW2's route: no tick there {ticks:?}");
    assert!(has(&ticks, end - half, end + half) || has(&ticks, end + half, end - half), "{ticks:?}");
    r.view.points.insert(s("P"), PointsView { position: PointsPos::Reverse, moving: false, locked: true });
    let rv = r.at(215.0, 10.0);
    let ticks = lines_of(&r.idle(), ROUTE, TICK_W);
    assert_eq!(ticks.len(), 1, "{ticks:?}");
    let mid = (ticks[0][0] + ticks[0][1].to_vec2()) / 2.0;
    assert!(close(mid, rv), "reverse lies: the tick moves to its end {ticks:?}");
    // An overlap ending on a plain line still has its tick (TW1 the route).
    r.view.sections.insert(s("TW1"), SectionView { occupied: false, held: Held::Path });
    r.view.sections.insert(s("TP"), SectionView { occupied: false, held: Held::Free });
    r.view.sections.insert(s("TW2"), SectionView { occupied: false, held: Held::Overlap });
    let w2_end = r.at(200.0, 0.0) - vec2(JOINT_GAP_PX / 2.0, 0.0);
    let ticks = lines_of(&r.idle(), ROUTE, TICK_W);
    assert_eq!(ticks.len(), 1, "{ticks:?}");
    let half = vec2(0.0, (r.w() + TICK_EXTRA_PX) / 2.0);
    assert!(has(&ticks, w2_end - half, w2_end + half) || has(&ticks, w2_end + half, w2_end - half), "{ticks:?}");
}

#[test]
fn a_fringe_signals_post_stays_grey_while_a_route_is_set_from_it() {
    let mut r = Rig::new(Some("East"));
    let a = r.sc.signals.iter().find(|s| s.name == "A").unwrap();
    assert!(a.fringe && !a.routes.is_empty(), "{a:?}");
    let route = a.routes[0].clone();
    r.view.routes.insert(route, RouteView { state: RouteState::Locked, auto_working: false });
    let d = r.idle();
    let base = r.at(200.0, 0.0);
    let post = [base, base + vec2(0.0, -POST_PX)];
    assert!(lines_of(&d, FRINGE, POST_W).contains(&post), "{:?}", lines_of(&d, FRINGE, POST_W));
    assert!(!lines_of(&d, ROUTE, POST_W).contains(&post));
}

// ---- berths, the auto button, platforms, labels and arrows ----

fn knockouts(d: &Drawing) -> Vec<Rect> {
    d.shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Rect(rs) if rs.fill == BG && rs.rect.width() == client_ui::hit::BERTH_W => Some(rs.rect),
            _ => None,
        })
        .collect()
}

#[test]
fn headcodes_sit_in_the_track_behind_their_signal_and_empty_berths_are_not_drawn() {
    let mut r = Rig::new(Some("West"));
    assert!(knockouts(&r.idle()).is_empty(), "no headcodes, nothing drawn");
    r.view.berths.insert(s("BA"), s("2Z99"));
    let d = r.idle();
    let centre = r.at(200.0, 0.0) - vec2(client_ui::scene::BERTH_BACK_PX, 0.0);
    assert_eq!(knockouts(&d).len(), 1);
    assert!(close(knockouts(&d)[0].center(), centre), "on the track, on A's approach side");
    let t = d.texts.iter().find(|t| t.text == "2Z99").unwrap();
    assert_eq!((t.colour, t.monospace, t.size, t.at), (HEADCODE, true, HEADCODE_PX, knockouts(&d)[0].center()));
}

#[test]
fn a_boundary_berth_sits_inside_its_boundary() {
    let mut r = Rig::new(Some("West"));
    r.view.berths.insert(s("BW"), s("1E01"));
    let d = r.idle();
    assert!(close(knockouts(&d)[0].center(), r.at(0.0, 0.0) + vec2(client_ui::scene::BERTH_BACK_PX, 0.0)));
}

#[test]
fn fringe_headcodes_are_grey() {
    let mut r = Rig::new(Some("East"));
    r.view.berths.insert(s("BA"), s("1E01"));
    r.view.berths.insert(s("BC"), s("2W03"));
    let d = r.idle();
    assert_eq!(d.texts.iter().find(|t| t.text == "1E01").unwrap().colour, FRINGE, "BA is West's");
    assert_eq!(d.texts.iter().find(|t| t.text == "2W03").unwrap().colour, HEADCODE);
}

#[test]
fn controlled_signals_carry_a_blue_auto_button_hollow_off_filled_on() {
    let mut r = Rig::new(Some("West"));
    let c = r.disc("W1") + vec2(client_ui::hit::AUTO_AHEAD_PX, 0.0);
    let d = r.idle();
    assert!(circles(&d).contains(&(c, AUTO_R, Color32::TRANSPARENT, AUTO)), "hollow: {:?}", circles(&d));
    let a = d.texts.iter().find(|t| t.text == "A" && t.at == c + vec2(AUTO_R + 2.0, 0.0)).unwrap();
    assert_eq!((a.colour, a.anchor), (AUTO, Align2::LEFT_CENTER));
    r.view.routes.insert(s("W1-A"), RouteView { state: RouteState::Locked, auto_working: false });
    assert!(circles(&r.idle()).contains(&(c, AUTO_R, Color32::TRANSPARENT, AUTO)), "hollow while set normally");
    r.view.routes.insert(s("W1-A"), RouteView { state: RouteState::Locked, auto_working: true });
    assert!(circles(&r.idle()).contains(&(c, AUTO_R, AUTO, Color32::TRANSPARENT)), "filled while auto-working");
    r.view.routes.insert(s("W1-A"), RouteView { state: RouteState::Cancelling, auto_working: true });
    assert!(circles(&r.idle()).contains(&(c, AUTO_R, Color32::TRANSPARENT, AUTO)), "hollow once cancelling");
    let colours = |d: &Drawing| d.texts.iter().filter(|t| t.text == "A").map(|t| t.colour).collect::<Vec<_>>();
    assert_eq!(colours(&r.idle()), [AUTO, AUTO, AUTO], "W1, A and W2");
    let east = Rig::new(Some("East"));
    let a = east.disc("A") + vec2(client_ui::hit::AUTO_AHEAD_PX, 0.0);
    assert!(circles(&east.idle()).contains(&(a, AUTO_R, Color32::TRANSPARENT, FRINGE)), "grey on East's fringe");
}

/// A permanently automatic signal keeps its dashed post and gets no ○A.
#[test]
fn automatic_signals_have_no_auto_button() {
    let mut l = layout_for(Some("West"));
    l.routes[0].automatic = true;
    let mut r = Rig::of(l, view_for(Some("West")));
    r.view.routes.insert(s("W1-A"), RouteView { state: RouteState::Locked, auto_working: true });
    let d = r.idle();
    let c = r.disc("W1") + vec2(client_ui::hit::AUTO_AHEAD_PX, 0.0);
    assert!(circles(&d).iter().all(|k| k.0 != c), "no ○A beside W1: {:?}", circles(&d));
    assert_eq!(d.texts.iter().filter(|t| t.text == "A").count(), 2, "only A and W2");
}

/// A spectator sees every controlled signal's ○A, coloured as the rest of
/// its view (nothing is fringe to a spectator); `hit` keeps them unclickable.
#[test]
fn a_spectator_sees_every_auto_button() {
    let d = Rig::new(None).idle();
    assert_eq!(d.texts.iter().filter(|t| t.text == "A").map(|t| t.colour).collect::<Vec<_>>(), [AUTO; 5]);
}

#[test]
fn platforms_are_ochre_blocks_with_their_number() {
    let r = Rig::new(Some("East"));
    let d = r.idle();
    let blocks: Vec<Rect> = d
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Rect(rs) if rs.fill == PLATFORM => Some(rs.rect),
            _ => None,
        })
        .collect();
    assert_eq!(blocks.len(), 2);
    let numbers: Vec<(&str, Color32)> = d.texts.iter().filter(|t| blocks.iter().any(|b| b.center() == t.at)).map(|t| (t.text.as_str(), t.colour)).collect();
    assert_eq!(numbers, [("1", BG), ("1", BG)], "the number in black inside");
}

#[test]
fn labels_are_grey_capitals_and_a_line_name_carries_its_arrow() {
    let mut l = layout_for(Some("West"));
    l.geometry.as_mut().unwrap().labels.push(LabelGeom { text: s("Up Main"), x: 150.0, y: -12.0, arrow: Some([-1.0, 0.0]) });
    let r = Rig::of(l, view_for(Some("West")));
    let d = r.idle();
    let west = d.texts.iter().find(|t| t.text == "WEST").unwrap();
    assert_eq!((west.colour, west.anchor), (LABEL, Align2::LEFT_TOP));
    let up = d.texts.iter().find(|t| t.text == "UP MAIN").unwrap();
    let p = r.at(150.0, -12.0);
    assert_eq!((up.anchor, up.at), (Align2::LEFT_CENTER, p + vec2(3.0, 0.0)), "text right of the arrow, which points left");
    let tip = |s: &Shape| match s {
        Shape::Path(path) if path.fill == LABEL => Some(path.points[0]),
        _ => None,
    };
    assert!(d.shapes.iter().filter_map(tip).any(|t| close(t, p - vec2(ARROW_PX, 0.0))), "the arrow starts at the point and points out");
    assert!(!d.texts.iter().any(|t| t.text.contains(['→', '←', '○', '●'])), "arrows and circles are shapes, never glyphs");
}

#[test]
fn arrows_go_at_loose_ends_and_along_long_runs() {
    assert!(arrow_stops(59.0, true, true).is_empty(), "too short to carry one");
    assert_eq!(arrow_stops(100.0, true, false), [ARROW_INSET_PX]);
    assert_eq!(arrow_stops(100.0, false, false), Vec::<f32>::new(), "a short run between points has none");
    assert_eq!(arrow_stops(1000.0, true, true), [ARROW_INSET_PX, 400.0, 1000.0 - ARROW_INSET_PX]);
    assert_eq!(arrow_stops(900.0, false, false), [400.0], "800 would crowd the end");
    assert!(arrow_stops(f32::NAN, true, true).is_empty());
}

/// West's run: a double arrow 24 px in from W, beside the bar, on the
/// right of the run's forward direction (below it: +x runs right).
#[test]
fn every_running_line_gets_direction_arrows() {
    let r = Rig::new(Some("West"));
    let d = r.idle();
    let tips: Vec<Pos2> = d
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) if p.fill == LABEL => Some(p.points[0]),
            _ => None,
        })
        .collect();
    let c = r.at(0.0, 0.0) + vec2(ARROW_INSET_PX, r.w() / 2.0 + ARROW_OFF_PX);
    let off = ARROW_PX / 2.0 + 1.0 + ARROW_PX / 2.0;
    assert!(tips.len() >= 2 && tips.len() % 2 == 0, "double arrows only: {tips:?}");
    assert!(tips.iter().any(|t| close(*t, c + vec2(off, 0.0))) && tips.iter().any(|t| close(*t, c - vec2(off, 0.0))), "{tips:?}");
    let total = r.at(200.0, 0.0).x - r.at(0.0, 0.0).x;
    assert_eq!(tips.len(), 2 * arrow_stops(total, true, false).len(), "one per stop along the {total} px run");
}

#[test]
fn absurdly_long_runs_have_a_bounded_number_of_arrows() {
    let stops = arrow_stops(1.0e12, true, true);
    assert!(stops.len() <= ARROW_MAX_STOPS + 2, "{} stops", stops.len());
    assert!(stops.windows(2).all(|w| w[0] < w[1]), "every stop moves on");
    let r = Rig::new(Some("West"));
    let mut sc = r.sc.clone();
    sc.runs = vec![client_ui::scene::Run {
        points: vec![pos2(-1.0e7, 0.0), pos2(1.0e7, 0.0)],
        forward: true,
        backward: false,
        loose_start: true,
        loose_end: true,
    }];
    sc.tracks.clear();
    let names = Names::new(&r.layout);
    let st = PaintState { view: None, selected: None, exits: &[], refused: None, time: 0.0, aspects: AspectMode::RedGreen, numbers: true, names: &names };
    let cam = Camera { centre: pos2(0.0, 0.0), scale: client_ui::camera::MAX_SCALE };
    let d = draw(&sc, &cam, screen(), &st);
    let arrows: Vec<Pos2> = d
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) if p.fill == LABEL => Some(p.points[0]),
            _ => None,
        })
        .collect();
    assert!(arrows.iter().all(|p| screen().expand(ARROW_PX * 2.0).contains(*p)), "only arrows on screen are drawn");
}

/// A run whose signals all face against it: the arrow points back along
/// it and sits on the right of that travel (above a line drawn left to right).
#[test]
fn a_backward_runs_arrow_is_on_the_right_of_its_travel() {
    let r = Rig::new(Some("West"));
    let mut sc = r.sc.clone();
    sc.runs = vec![client_ui::scene::Run {
        points: vec![pos2(0.0, 0.0), pos2(200.0, 0.0)],
        forward: false,
        backward: true,
        loose_start: true,
        loose_end: false,
    }];
    let names = Names::new(&r.layout);
    let st = PaintState { view: None, selected: None, exits: &[], refused: None, time: 0.0, aspects: AspectMode::RedGreen, numbers: true, names: &names };
    let d = draw(&sc, &r.cam, screen(), &st);
    let tips: Vec<Pos2> = d
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) if p.fill == LABEL => Some(p.points[0]),
            _ => None,
        })
        .collect();
    let c = r.at(0.0, 0.0) + vec2(ARROW_INSET_PX, -(r.w() / 2.0 + ARROW_OFF_PX));
    let total = r.at(200.0, 0.0).x - r.at(0.0, 0.0).x;
    assert_eq!(tips.len(), arrow_stops(total, true, false).len(), "single arrows: {tips:?}");
    assert!(close(tips[0], c - vec2(ARROW_PX / 2.0, 0.0)), "{tips:?} vs centre {c:?}");
    assert!(tips.iter().all(|t| (t.y - c.y).abs() < 1e-3), "all above the line: {tips:?}");
}

/// W2's disc hangs below the line at J0, over W1's berth behind it: the
/// headcode's knock-out goes down first, so the disc stays whole.
#[test]
fn headcodes_are_drawn_under_signals_and_auto_buttons() {
    let mut r = Rig::new(Some("West"));
    r.view.berths.insert(s("BW1"), s("1A01"));
    let d = r.idle();
    let knockout = d.shapes.iter().position(|s| matches!(s, Shape::Rect(rs) if rs.fill == BG && rs.rect.width() == client_ui::hit::BERTH_W)).unwrap();
    let rect = knockouts(&d)[0];
    let w2 = r.disc("W2");
    assert!(rect.expand(LAMP_R).contains(w2), "the case: W2's disc {w2:?} touches BW1's box {rect:?}");
    let disc = d.shapes.iter().position(|s| matches!(s, Shape::Circle(c) if c.center == w2)).unwrap();
    let auto = d.shapes.iter().position(|s| matches!(s, Shape::Circle(c) if c.radius == AUTO_R && c.stroke.color == AUTO)).unwrap();
    assert!(knockout < disc && knockout < auto, "knock-out {knockout}, disc {disc}, ○A {auto}");
}
