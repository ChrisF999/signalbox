mod common;

use common::*;
use serde_json::json;
use signalbox_core::aspect::Aspect::{self, *};
use signalbox_core::driver::{apply_speed, target_speed};
use signalbox_core::ids::*;
use signalbox_core::network::*;
use signalbox_core::trains::Train;
use signalbox_core::world::World;

struct Pts;

impl PointsView for Pts {
    fn position(&self, _: NodeId) -> Option<PointsPos> {
        Some(PointsPos::Normal)
    }
}

fn train(w: &World, segment: &str, head_m: f64, speed: f64, last: Option<Aspect>) -> Train {
    let mut t = Train::new(TrainId(0), ServiceId(0), "X", TrainTypeId(0), 100.0, seg(w, segment), Dir::Up, speed);
    t.head_m = head_m;
    t.last_passed_aspect = last;
    t
}

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} != {b}");
}

const LINE: f64 = 100.0 / 3.6;

#[test]
fn cruises_at_line_speed_when_all_is_green() {
    let w = world("plain_line");
    let t = train(&w, "a", 100.0, 20.0, Some(Green));
    approx(target_speed(&t, &w.train_types[0], &w.net, &Pts, &[Green, Green], None), LINE);
}

#[test]
fn brakes_for_a_red_it_can_see() {
    let w = world("plain_line");
    let t = train(&w, "a", 900.0, 10.0, Some(Green));
    approx(target_speed(&t, &w.train_types[0], &w.net, &Pts, &[Red, Red], None), (2.0 * 0.7 * 95.0f64).sqrt());
}

#[test]
fn unseen_signal_is_expected_red_after_a_yellow() {
    let w = world("plain_line");
    let tt = &w.train_types[0];
    let after_yellow = train(&w, "a", 600.0, 10.0, Some(Yellow));
    approx(target_speed(&after_yellow, tt, &w.net, &Pts, &[Green, Green], None), (2.0 * 0.7 * 395.0f64).sqrt());
    let after_green = train(&w, "a", 600.0, 10.0, Some(Green));
    approx(target_speed(&after_green, tt, &w.net, &Pts, &[Red, Red], None), LINE);
}

#[test]
fn new_train_expects_first_signal_at_red() {
    let w = world("plain_line");
    let t = train(&w, "a", 600.0, 10.0, None);
    approx(target_speed(&t, &w.train_types[0], &w.net, &Pts, &[Green, Green], None), (2.0 * 0.7 * 395.0f64).sqrt());
}

#[test]
fn yellow_means_stop_at_the_next_signal() {
    let w = world("plain_line");
    let mut tt = w.train_types[0].clone();
    tt.service_brake = 0.1;
    let t = train(&w, "a", 950.0, 10.0, Some(Green));
    approx(target_speed(&t, &tt, &w.net, &Pts, &[Yellow, Green], None), (2.0 * 0.1 * 1045.0f64).sqrt());
}

#[test]
fn stops_at_the_platform_it_calls_at() {
    let w = load_with("terminus", |v| v["platforms"][0]["to_m"] = json!(150)).unwrap();
    let tt = &w.train_types[0];
    let t = train(&w, "p1", 100.0, 5.0, Some(Yellow));
    approx(target_speed(&t, tt, &w.net, &Pts, &[Red, Red, Red], Some("TRM")), (2.0 * 0.7 * 45.0f64).sqrt());
    approx(target_speed(&t, tt, &w.net, &Pts, &[Red, Red, Red], None), 30.0 / 3.6);
}

#[test]
fn stops_short_of_buffers_but_runs_off_at_a_boundary() {
    let w = world("terminus");
    let t = train(&w, "p1", 240.0, 1.0, Some(Yellow));
    approx(target_speed(&t, &w.train_types[0], &w.net, &Pts, &[Red, Red, Red], None), (2.0 * 0.7 * 5.0f64).sqrt());
    let p = world("plain_line");
    let t = train(&p, "c", 950.0, 20.0, Some(Green));
    approx(target_speed(&t, &p.train_types[0], &p.net, &Pts, &[Green, Green], None), LINE);
}

#[test]
fn lowest_limit_under_the_whole_train_applies() {
    let w = world("terminus");
    let mut t = train(&w, "in", 1500.0, 10.0, Some(Green));
    t.advance(&w.net, &Pts, 10.0);
    approx(target_speed(&t, &w.train_types[0], &w.net, &Pts, &[Green, Red, Red], None), 40.0 / 3.6);
}

#[test]
fn speed_update_accelerates_brakes_and_never_goes_negative() {
    let w = world("plain_line");
    let tt = &w.train_types[0];
    let mut t = train(&w, "a", 0.0, 0.0, None);
    apply_speed(&mut t, tt, &w.net, 10.0, 0.1);
    approx(t.speed, 0.08);
    t.speed = 10.0;
    apply_speed(&mut t, tt, &w.net, 0.0, 0.1);
    approx(t.speed, 9.93);
    t.emergency = true;
    apply_speed(&mut t, tt, &w.net, 20.0, 0.1);
    approx(t.speed, 9.81);
    t.speed = 0.05;
    apply_speed(&mut t, tt, &w.net, 0.0, 0.1);
    assert_eq!(t.speed, 0.0);
}
