//! Polish spec §3.4: the shipped layouts, every box and a spectator, at
//! Fit, 2× and 4× in a 1280 × 800 and a 1920 × 1080 window, drawn and
//! placed with egui's own fonts: no text over another, nothing over track,
//! lamps or boxes but tight signal numbers, every own number drawn at
//! 1280 × 800 Fit. Prints the table the owner reads (`--nocapture`).

use client_core::{AspectMode, Names};
use client_ui::camera::Camera;
use client_ui::labels::{self, Audit};
use client_ui::paint::{self, Drawing, PaintState, TextItem, draw};
use client_ui::scene::Scene;
use egui::{Context, RawInput, Rect, Vec2, pos2};
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

/// The diagram's rectangle in a window of each size, as `UiApp` lays it out
/// (measured with the screens rig: top bars 45 pt, side panel 398 pt).
fn windows() -> [(&'static str, Rect); 2] {
    [
        ("1280x800", Rect::from_min_max(pos2(0.0, 45.0), pos2(882.0, 800.0))),
        ("1920x1080", Rect::from_min_max(pos2(0.0, 45.0), pos2(1522.0, 1080.0))),
    ]
}

/// `d` with only the texts whose anchor is on `screen`: what the owner sees.
fn on_screen(mut d: Drawing, screen: Rect) -> Drawing {
    let keep: Vec<usize> = (0..d.texts.len()).filter(|&i| screen.contains(d.texts[i].at)).collect();
    d.movable.retain(|m| keep.contains(&m.text));
    for m in &mut d.movable {
        m.text = keep.iter().position(|&k| k == m.text).expect("kept");
    }
    d.texts = keep.iter().map(|&i| d.texts[i].clone()).collect();
    d
}

struct Row {
    window: &'static str,
    view: String,
    zoom: f32,
    audit: Audit,
    hidden: Vec<String>,
    /// How long `plan` took (debug builds are several times slower).
    ms: f64,
}

#[test]
fn every_view_is_legible_at_every_zoom() {
    let ctx = Context::default();
    let mut o = ctx.run_ui(RawInput::default(), |_| {});
    o.textures_delta.clear();
    let mut measure = |t: &TextItem| -> Vec2 { ctx.fonts_mut(|f| f.layout_no_wrap(t.text.clone(), paint::font(t), t.colour).size()) };
    let mut rows: Vec<Row> = Vec::new();
    for name in ["liverpool-st", "drain", "gretz-armainvilliers"] {
        let w = world(name);
        let areas: Vec<Option<String>> = std::iter::once(None).chain(w.net.areas.iter().map(|a| Some(a.name.clone()))).collect();
        let mut g = Game::new(w, GameMeta { layout: name.into(), seed: 1 });
        g.connect("sam");
        g.handle("sam", ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
        for _ in 0..75 {
            g.advance(1.0);
        }
        for area in areas {
            match &area {
                Some(a) => g.handle("sam", ClientMsg::Claim { area: a.clone() }),
                None => g.handle("sam", ClientMsg::Release),
            };
            let (l, v) = (g.layout_of("sam").unwrap(), g.view_of("sam").unwrap());
            let sc = Scene::build(&l).unwrap();
            let names = Names::new(&l);
            let view = format!("{name} {}", area.as_deref().unwrap_or("spectator"));
            for (window, screen) in windows() {
                let fit = Camera::fit(sc.fit_bounds().unwrap(), screen);
                for zoom in [1.0_f32, 2.0, 4.0] {
                    let cam = Camera { centre: fit.centre, scale: fit.scale * zoom };
                    let st = PaintState {
                        view: Some(&v),
                        selected: None,
                        exits: &[],
                        refused: None,
                        blocking: None,
                        time: 0.0,
                        aspects: AspectMode::RedGreen,
                        numbers: true,
                        names: &names,
                        highlight: &[],
                    };
                    let d = draw(&sc, &cam, screen, &st);
                    let t0 = std::time::Instant::now();
                    let plan = labels::plan(&d, &mut measure);
                    let ms = t0.elapsed().as_secs_f64() * 1000.0;
                    let audit = labels::audit(&on_screen(labels::apply(d, &plan), screen), &mut measure);
                    rows.push(Row { window, view: view.clone(), zoom, audit, hidden: plan.hidden_numbers.clone(), ms });
                }
            }
        }
    }
    println!("{:10} {:38} {:>4} {:>8} {:>7} {:>5} {:>6} {:>7}  shown", "window", "view", "zoom", "overlaps", "covered", "tight", "hidden", "plan ms");
    for r in &rows {
        println!(
            "{:10} {:38} {:>4} {:>8} {:>7} {:>5} {:>6} {:>7.2}  {:?} {:?}",
            r.window, r.view, r.zoom, r.audit.overlaps, r.audit.covered, r.audit.tight, r.hidden.len(), r.ms, r.audit.shown, r.hidden
        );
    }
    for r in &rows {
        assert_eq!(r.audit.overlaps, 0, "{} {} x{}: texts overlap", r.window, r.view, r.zoom);
        assert_eq!(r.audit.covered, 0, "{} {} x{}: texts cover track, lamps or boxes", r.window, r.view, r.zoom);
    }
    let fit_small: Vec<&Row> = rows.iter().filter(|r| r.window == "1280x800" && r.zoom == 1.0).collect();
    for r in fit_small.iter().filter(|r| !r.view.ends_with("spectator")) {
        assert!(r.hidden.is_empty(), "{}: every own number drawn at 1280x800 Fit, not {:?}", r.view, r.hidden);
    }
    let tight: usize = fit_small.iter().map(|r| r.audit.tight).sum();
    assert!(tight <= 4, "{tight} numbers tight against track at 1280x800 Fit (baseline 23)");
    let mut o = ctx.run_ui(RawInput::default(), |_| {});
    o.textures_delta.clear();
}
