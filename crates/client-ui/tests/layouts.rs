//! The shipped layouts, drawn for every box and for a spectator with trains
//! running: nothing panics, every shape is finite, each segment is one drawn
//! line, and the new marks appear.

use client_core::{AspectMode, Names};
use client_ui::camera::Camera;
use client_ui::paint::{LABEL, PaintState, draw};
use client_ui::scene::Scene;
use egui::{Rect, Shape, pos2, vec2};
use game::{Game, GameMeta};
use protocol::{ClientMsg, Proposal};
use signalbox_core::world::World;

fn world(name: &str) -> World {
    let dir = env!("CARGO_MANIFEST_DIR");
    let read = |p: String| std::fs::read_to_string(p).unwrap();
    let mut w = ts2_import::convert(&read(format!("{dir}/../ts2-import/tests/data/{name}.json"))).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&read(format!("{dir}/../../layouts/{name}.areas.json"))).unwrap()).unwrap();
    ts2_import::lines::apply(&mut w, &ts2_import::lines::parse(&read(format!("{dir}/../../layouts/{name}.lines.json"))).unwrap()).unwrap();
    World::from_file(w).unwrap()
}

fn finite(s: &Shape) -> bool {
    match s {
        Shape::LineSegment { points, .. } => points.iter().all(|p| p.is_finite()),
        Shape::Circle(c) => c.center.is_finite(),
        Shape::Rect(r) => r.rect.is_finite(),
        Shape::Path(p) => p.points.iter().all(|p| p.is_finite()),
        _ => true,
    }
}

#[test]
fn every_shipped_layout_draws_for_every_box() {
    let screen = Rect::from_min_size(pos2(0.0, 40.0), vec2(950.0, 700.0));
    for name in ["liverpool-st", "drain", "gretz-armainvilliers"] {
        let w = world(name);
        let areas: Vec<Option<String>> = std::iter::once(None).chain(w.net.areas.iter().map(|a| Some(a.name.clone()))).collect();
        let mut g = Game::new(w, GameMeta { layout: name.into(), seed: 1 });
        g.connect("sam");
        g.handle("sam", ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
        // Ten sim minutes at 8x, the robot running every box: trains about.
        for _ in 0..75 {
            g.advance(1.0);
        }
        for area in areas {
            match &area {
                Some(a) => g.handle("sam", ClientMsg::Claim { area: a.clone() }),
                None => g.handle("sam", ClientMsg::Release),
            };
            let (l, v) = (g.layout_of("sam").unwrap(), g.view_of("sam").unwrap());
            let sc = Scene::build(&l).unwrap_or_else(|| panic!("{name} {area:?}: no scene"));
            assert!(!sc.runs.is_empty(), "{name} {area:?}");
            // Runs, signal bases and joints look lines up by segment: one each.
            let segments: std::collections::BTreeSet<&str> = sc.tracks.iter().map(|t| t.segment.as_str()).collect();
            assert_eq!(segments.len(), sc.tracks.len(), "{name} {area:?}: one drawn line per segment");
            let cam = Camera::fit(sc.fit_bounds().unwrap(), screen);
            let names = Names::new(&l);
            for (time, aspects) in [(0.0, AspectMode::RedGreen), (0.3, AspectMode::Real)] {
                let st = PaintState { view: Some(&v), selected: None, exits: &[], refused: None, blocking: None, time, aspects, numbers: true, names: &names, highlight: &[] };
                let d = draw(&sc, &cam, screen, &st);
                assert!(d.shapes.iter().all(finite), "{name} {area:?}");
                assert!(d.texts.iter().all(|t| t.at.is_finite() && t.size.is_finite()), "{name} {area:?}: texts");
                assert!(d.shapes.iter().any(|s| matches!(s, Shape::Path(p) if p.fill == LABEL)), "{name} {area:?}: direction arrows");
            }
            if name == "liverpool-st" && area.as_deref() == Some("Liverpool Street") {
                // Every platform road ends at a buffer stop (behind TS2's undrawn spacer).
                let loose = sc.runs.iter().filter(|r| (r.loose_start || r.loose_end) && (r.forward || r.backward)).count();
                assert!(loose >= 18, "{loose} runs with an arrowed end");
            }
            if name == "liverpool-st" && area.is_none() {
                assert_eq!(sc.labels.iter().filter(|l| l.arrow.is_some()).count(), 8, "the eight line names");
                assert!(!v.berths.is_empty(), "headcodes to draw");
            }
        }
    }
}

/// The four lesson worlds, drawn for their player and for a spectator with
/// their trains running and every highlight of every step: nothing panics,
/// every shape is finite, every highlight names something on screen.
#[test]
fn every_lesson_draws_with_its_highlights() {
    let screen = Rect::from_min_size(pos2(0.0, 40.0), vec2(880.0, 700.0));
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../lessons");
    let mut dirs: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    dirs.sort();
    assert_eq!(dirs.len(), 4);
    for d in dirs {
        let lesson = game::lesson::load_lesson(&d).unwrap();
        let steps = lesson.file.steps.clone();
        let area = lesson.file.area.clone();
        let (mut g, mut r) = game::lesson::start(lesson);
        r.connect(&mut g, "pat");
        g.connect("sam");
        for step in &steps {
            for a in &step.actions {
                if let game::lesson::Action::Spawn { headcode, entry } = a {
                    if let Some(i) = game::lesson::spawn_entry(g.sim().world(), headcode, entry) {
                        let _ = g.offer_entry(i);
                    }
                }
            }
        }
        for _ in 0..30 {
            r.advance(&mut g, 1.0);
        }
        for who in ["pat", "sam"] {
            let (l, v) = (g.layout_of(who).unwrap(), g.view_of(who).unwrap());
            assert_eq!(l.area.as_deref(), (who == "pat").then_some(area.as_str()), "{}", d.display());
            let sc = Scene::build(&l).unwrap_or_else(|| panic!("{}: no scene", d.display()));
            let cam = Camera::fit(sc.fit_bounds().unwrap(), screen);
            let names = Names::new(&l);
            for step in &steps {
                let diagram: Vec<_> = step.highlight.iter().filter(|h| !matches!(h, protocol::Highlight::Ui(u) if !u.starts_with("auto:"))).cloned().collect();
                let st = |highlight| PaintState { view: Some(&v), selected: None, exits: &[], refused: None, blocking: None, time: 0.25, aspects: AspectMode::Real, numbers: true, names: &names, highlight };
                let dr = draw(&sc, &cam, screen, &st(&diagram));
                assert!(dr.shapes.iter().all(finite), "{}", d.display());
                if who == "pat" {
                    // Each highlight on its own: one that resolves to nothing
                    // must not hide behind another's several lines.
                    for h in &diagram {
                        let dr = draw(&sc, &cam, screen, &st(std::slice::from_ref(h)));
                        let lit = dr.shapes.iter().filter(|s| match s {
                            Shape::Circle(c) => c.stroke.color.r() == 0xFF && c.stroke.color.g() == 0x8C,
                            Shape::Rect(r) => r.stroke.color.r() == 0xFF && r.stroke.color.g() == 0x8C,
                            Shape::LineSegment { stroke, .. } => stroke.color.r() == 0xFF && stroke.color.g() == 0x8C,
                            _ => false,
                        }).count();
                        assert!(lit >= 1, "{}: step `{}`: {h:?} is not drawn", d.display(), step.say);
                    }
                }
            }
        }
    }
}

/// Polish spec M12: crossover middles are found (and only those), and one
/// stops counting as unused once an end lies over it.
#[test]
fn crossovers_neither_end_of_which_is_set_are_found() {
    let mut g = Game::new(world("liverpool-st"), GameMeta { layout: "liverpool-st".into(), seed: 1 });
    g.connect("sam");
    let (l, mut v) = (g.layout_of("sam").unwrap(), g.view_of("sam").unwrap());
    let sc = Scene::build(&l).unwrap();
    let unused = client_ui::paint::unused_crossovers(&sc, Some(&v));
    for running in ["T172", "T198", "T200", "T267", "T272"] {
        assert!(!unused.contains(running), "{running} is a running line, not a crossover middle");
    }
    let expected = [
        "T135", "T162", "T168", "T213", "T216", "T227", "T229", "T231", "T233", "T240", "T245", "T251", "T253", "T258", "T260", "T288",
        "T291", "T294",
    ];
    assert_eq!(unused.iter().map(String::as_str).collect::<Vec<_>>(), expected);
    // Nothing flagged has points lying over it, a toe on it, or points in it.
    for s in &unused {
        assert!(sc.points.iter().all(|p| p.section != *s && !p.toe_meets.contains(s) && !p.normal_meets.contains(s)), "{s}");
    }
    let middle = unused.iter().next().unwrap().clone();
    let end = sc.points.iter().find(|p| p.reverse_meets.contains(&middle)).unwrap();
    v.points.get_mut(&end.name).unwrap().position = protocol::PointsPos::Reverse;
    assert!(!client_ui::paint::unused_crossovers(&sc, Some(&v)).contains(&middle), "{} lies over it", end.name);
}

/// A crossover middle is drawn thin, full width once its end is set over it.
#[test]
fn a_crossover_middle_is_drawn_thin() {
    let screen = Rect::from_min_size(pos2(0.0, 40.0), vec2(950.0, 700.0));
    let mut g = Game::new(world("liverpool-st"), GameMeta { layout: "liverpool-st".into(), seed: 1 });
    g.connect("sam");
    let (l, mut v) = (g.layout_of("sam").unwrap(), g.view_of("sam").unwrap());
    let sc = Scene::build(&l).unwrap();
    let cam = Camera::fit(sc.fit_bounds().unwrap(), screen);
    let names = Names::new(&l);
    let w = client_ui::paint::track_w(cam.scale);
    let unused = client_ui::paint::unused_crossovers(&sc, Some(&v));
    let middle = unused.iter().next().unwrap().clone();
    let widths = |v: &protocol::View| {
        let st = PaintState { view: Some(v), selected: None, exits: &[], refused: None, blocking: None, time: 0.0, aspects: AspectMode::RedGreen, numbers: true, names: &names, highlight: &[] };
        let d = draw(&sc, &cam, screen, &st);
        let t = sc.tracks.iter().find(|t| t.section == middle).unwrap();
        let (a, b) = (cam.to_screen(screen, t.a), cam.to_screen(screen, t.b));
        d.shapes
            .iter()
            .filter_map(|s| match s {
                Shape::LineSegment { points, stroke }
                    if stroke.color == client_ui::paint::TRACK_FREE
                        && ((points[0].distance(a) < 1.5 && points[1].distance(b) < 1.5) || (points[0].distance(b) < 1.5 && points[1].distance(a) < 1.5)) =>
                {
                    Some(stroke.width)
                }
                _ => None,
            })
            .fold(f32::MIN, f32::max)
    };
    let thin = widths(&v);
    assert!(thin < w, "drawn thin: {thin} vs {w}");
    let end = sc.points.iter().find(|p| p.reverse_meets.contains(&middle)).unwrap();
    v.points.get_mut(&end.name).unwrap().position = protocol::PointsPos::Reverse;
    assert!((widths(&v) - w).abs() < 0.01, "full width with an end over it");
}

/// Platform roads of the lessons are loops, not crossovers: never thin.
#[test]
fn lesson_platform_roads_are_never_thin() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../lessons");
    for id in ["02-setting-routes", "03-running-trains"] {
        let lesson = game::lesson::load_lesson(&std::path::Path::new(dir).join(id)).unwrap();
        let (mut g, mut r) = game::lesson::start(lesson);
        r.connect(&mut g, "pat");
        let (l, mut v) = (g.layout_of("pat").unwrap(), g.view_of("pat").unwrap());
        let sc = Scene::build(&l).unwrap();
        for reversed in [false, true] {
            if reversed {
                for p in v.points.values_mut() {
                    p.position = protocol::PointsPos::Reverse;
                }
            }
            let unused = client_ui::paint::unused_crossovers(&sc, Some(&v));
            assert!(!unused.contains("TD") && !unused.contains("TC"), "{id} reversed={reversed}: {unused:?}");
        }
    }
}
