//! Placing the diagram's texts (polish spec §3.3), on hand-made drawings
//! with a fixed-advance measure: 6 px a character, 10 px high.

use client_ui::labels::*;
use client_ui::paint::{Drawing, LABEL, TextItem};
use egui::{Align2, Pos2, Rect, Vec2, pos2, vec2};

fn measure(t: &TextItem) -> Vec2 {
    vec2(t.text.chars().count() as f32 * 6.0, 10.0)
}

fn text(s: &str, at: Pos2, anchor: Align2) -> TextItem {
    TextItem { at, anchor, text: s.to_string(), size: 10.0, colour: LABEL, monospace: true }
}

/// `d` with `t` added as a movable text of `role`.
fn push(d: &mut Drawing, t: TextItem, role: Role, alts: Vec<(Pos2, Align2)>) {
    d.movable.push(Movable { text: d.texts.len(), role, alts, within: None });
    d.texts.push(t);
}

/// A horizontal track bar along y = 100, 6 px wide.
fn track() -> Drawing {
    Drawing { keep: KeepClear { bars: vec![(pos2(0.0, 100.0), pos2(400.0, 100.0), 6.0)], ..KeepClear::default() }, ..Drawing::default() }
}

fn plan_of(d: &Drawing) -> Plan {
    plan(d, &mut measure)
}

#[test]
fn a_number_on_the_track_moves_to_its_first_clear_spot() {
    let mut d = track();
    // Its own spot (y 89..99) touches the bar (97..103); the first other
    // spot does too; the second is clear.
    let alts = vec![(pos2(50.0, 101.0), Align2::CENTER_BOTTOM), (pos2(50.0, 90.0), Align2::CENTER_BOTTOM)];
    push(&mut d, text("LA11", pos2(50.0, 99.0), Align2::CENTER_BOTTOM), Role::Number, alts);
    let p = plan_of(&d);
    assert_eq!(p.spots, vec![Some((vec2(0.0, -9.0), Align2::CENTER_BOTTOM))]);
    assert_eq!((p.tight, p.hidden_numbers.len()), (0, 0));
    let placed = apply(d, &p);
    assert_eq!(placed.texts[0].at, pos2(50.0, 90.0));
}

#[test]
fn a_number_with_no_clear_spot_is_tight_and_one_with_no_free_spot_is_hidden() {
    let mut d = track();
    d.keep.bars[0].2 = 40.0; // 80..120: no spot or nudge of a 10 px text escapes it
    push(&mut d, text("LA11", pos2(50.0, 100.0), Align2::CENTER_CENTER), Role::Number, vec![]);
    let p = plan_of(&d);
    assert_eq!(p.spots, vec![Some((Vec2::ZERO, Align2::CENTER_CENTER))], "on the track, but readable");
    assert_eq!(p.tight, 1);
    // A lamp under every spot: hidden, and named.
    d.keep.rounds = (0..40).map(|i| (pos2(i as f32 * 5.0, 100.0), 30.0)).collect();
    let p = plan_of(&d);
    assert_eq!(p.spots, vec![None]);
    assert_eq!(p.hidden_numbers, vec!["LA11".to_string()]);
}

#[test]
fn labels_and_fringe_numbers_are_hidden_rather_than_drawn_on_track() {
    let mut d = track();
    d.keep.bars[0].2 = 80.0; // a bar 80 px wide: no nudge escapes it
    push(&mut d, text("BANK", pos2(50.0, 100.0), Align2::LEFT_TOP), Role::Label, vec![]);
    push(&mut d, text("LB72", pos2(150.0, 100.0), Align2::CENTER_CENTER), Role::FringeNumber, vec![]);
    let p = plan_of(&d);
    assert_eq!(p.spots, vec![None, None]);
    assert_eq!(p.tight, 0, "only your own numbers may touch track");
    assert!(p.hidden_numbers.is_empty(), "fringe numbers are not counted as yours");
}

#[test]
fn a_number_wins_its_spot_from_a_label_drawn_first() {
    let mut d = Drawing::default();
    push(&mut d, text("BETHNAL GREEN", pos2(100.0, 50.0), Align2::CENTER_CENTER), Role::Label, vec![]);
    push(&mut d, text("LB72", pos2(100.0, 50.0), Align2::CENTER_CENTER), Role::Number, vec![]);
    let p = plan_of(&d);
    assert_eq!(p.spots[1], Some((Vec2::ZERO, Align2::CENTER_CENTER)), "the number stays");
    assert_eq!(p.spots[0], Some((vec2(0.0, -10.0), Align2::CENTER_CENTER)), "the label moves up a line");
}

#[test]
fn a_line_name_moves_only_up_or_down() {
    let mut d = Drawing::default();
    push(&mut d, text("LB72", pos2(100.0, 50.0), Align2::CENTER_CENTER), Role::Number, vec![]);
    push(&mut d, text("UP MAIN", pos2(100.0, 50.0), Align2::RIGHT_CENTER), Role::LineName, vec![]);
    let p = plan_of(&d);
    let (off, _) = p.spots[1].unwrap();
    assert_eq!(off.x, 0.0);
    assert!(off.y.abs() >= 5.0);
}

#[test]
fn a_platform_number_must_fit_its_block() {
    let mut d = Drawing::default();
    d.movable.push(Movable { text: 0, role: Role::Platform, alts: vec![], within: Some(Rect::from_center_size(pos2(50.0, 50.0), vec2(4.0, 4.0))) });
    d.texts.push(text("12", pos2(50.0, 50.0), Align2::CENTER_CENTER));
    assert_eq!(plan_of(&d).spots, vec![None], "a 12 × 10 text in a 4 px block");
    d.movable[0].within = Some(Rect::from_center_size(pos2(50.0, 50.0), vec2(40.0, 14.0)));
    assert_eq!(plan_of(&d).spots, vec![Some((Vec2::ZERO, Align2::CENTER_CENTER))]);
}

#[test]
fn texts_without_placement_data_are_left_alone_and_ignored() {
    let mut d = Drawing::default();
    d.texts.push(text("1A01", pos2(100.0, 50.0), Align2::CENTER_CENTER)); // a headcode
    push(&mut d, text("LB72", pos2(100.0, 50.0), Align2::CENTER_CENTER), Role::Number, vec![]);
    let p = plan_of(&d);
    assert_eq!(p.spots, vec![Some((Vec2::ZERO, Align2::CENTER_CENTER))], "berth boxes stand in for headcodes");
    let placed = apply(d, &p);
    assert_eq!(placed.texts[0].text, "1A01");
    assert_eq!(placed.texts[0].at, pos2(100.0, 50.0));
}

#[test]
fn apply_drops_hidden_texts_and_points_the_rest_at_their_new_index() {
    let mut d = track();
    d.keep.bars[0].2 = 80.0;
    d.texts.push(text("1A01", pos2(10.0, 10.0), Align2::CENTER_CENTER));
    push(&mut d, text("BANK", pos2(50.0, 100.0), Align2::LEFT_TOP), Role::Label, vec![]);
    push(&mut d, text("LB72", pos2(150.0, 20.0), Align2::CENTER_CENTER), Role::Number, vec![]);
    let p = plan_of(&d);
    let placed = apply(d.clone(), &p);
    assert_eq!(placed.texts.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["1A01", "LB72"]);
    assert_eq!(placed.movable.len(), 1);
    assert_eq!((placed.movable[0].text, placed.movable[0].role), (1, Role::Number));
    assert!(placed.movable[0].alts.is_empty());
    // A plan made for another drawing changes nothing.
    assert_eq!(apply(d.clone(), &Plan::default()), d);
}

/// Review focus 1: nonsense coordinates and sizes stay cheap and finite.
#[test]
fn nonsense_geometry_stays_cheap_and_finite() {
    let mut d = Drawing::default();
    d.keep.bars.push((pos2(-1.0e9, 0.0), pos2(1.0e9, 0.0), 6.0));
    d.keep.bars.push((pos2(f32::NAN, 0.0), pos2(5.0, f32::INFINITY), 6.0));
    d.keep.rounds.push((pos2(f32::NAN, f32::NAN), 4.0));
    push(&mut d, text("LA11", pos2(50.0, 50.0), Align2::CENTER_CENTER), Role::Number, vec![(pos2(f32::NAN, 1.0), Align2::LEFT_TOP)]);
    push(&mut d, text("FAR", pos2(4.0e8, -3.0e8), Align2::LEFT_TOP), Role::Label, vec![]);
    let t = std::time::Instant::now();
    let p = plan_of(&d);
    assert!(t.elapsed() < std::time::Duration::from_secs(1), "{:?}", t.elapsed());
    assert_eq!(p.spots[0], Some((Vec2::ZERO, Align2::CENTER_CENTER)));
    assert!(p.spots.iter().flatten().all(|(o, _)| o.is_finite()));
    // A text that cannot be measured is hidden, never drawn somewhere odd.
    let p = plan(&d, &mut |_| vec2(f32::NAN, 10.0));
    assert_eq!(p.spots, vec![None, None]);
}

#[test]
fn the_anchor_for_a_direction() {
    assert_eq!(corner(vec2(-1.0, -1.0)), Align2::RIGHT_BOTTOM, "up and left");
    assert_eq!(corner(vec2(1.0, 0.0)), Align2::LEFT_CENTER);
    assert_eq!(corner(vec2(0.0, 1.0)), Align2::CENTER_TOP);
}

#[test]
fn audit_counts_overlaps_covered_texts_and_tight_numbers() {
    let mut d = track();
    push(&mut d, text("BANK", pos2(50.0, 100.0), Align2::CENTER_CENTER), Role::Label, vec![]);
    push(&mut d, text("LA11", pos2(150.0, 100.0), Align2::CENTER_CENTER), Role::Number, vec![]);
    push(&mut d, text("LA13", pos2(152.0, 100.0), Align2::CENTER_CENTER), Role::Number, vec![]);
    let a = audit(&d, &mut measure);
    assert_eq!((a.overlaps, a.covered, a.tight), (1, 1, 2));
    assert_eq!(a.shown.get("Number"), Some(&2));
}
