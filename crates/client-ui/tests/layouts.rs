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
                let st = PaintState { view: Some(&v), selected: None, exits: &[], refused: None, time, aspects, numbers: true, names: &names };
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
