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

/// Three signals (of `aspects` aspects each) on a 100 km/h line, the last two only 150 m apart:
/// braking from line speed takes about 550 m, far more than 150 m plus the
/// 100 m sighting. S0 and S1 are cleared, S2 stays red. A driver who reads
/// S0's green as "S1 will be green too" runs at line speed, first sees S2's
/// red 100 m away and overruns it. A green only promises a yellow next, so
/// the driver must plan to stop at S2 from S0 on.
fn close_spacing_sim(aspects: u8) -> signalbox_core::sim::Sim {
    use signalbox_core::sim::Sim;
    let seg = |name: &str, from: &str, to: &str, len: u32, sec: &str| {
        json!({"name": name, "from": from, "to": to, "length_m": len, "line_speed_kmh": 100, "section": sec})
    };
    let signal = |name: &str, segment: &str, at: u32| {
        json!({"name": name, "area": "A", "segment": segment, "offset_m": at, "direction": "up", "aspects": aspects, "sighting_m": 100})
    };
    let w = json!({
        "schema": 1, "areas": [{"name": "A"}],
        "sections": [{"name": "TW", "area": "A"}, {"name": "TA", "area": "A"}, {"name": "TB", "area": "A"}, {"name": "TC", "area": "A"}],
        "nodes": [{"name": "W", "kind": "boundary"}, {"name": "J0", "kind": "joint"}, {"name": "J1", "kind": "joint"},
                  {"name": "J2", "kind": "joint"}, {"name": "E", "kind": "boundary"}],
        "segments": [seg("w", "W", "J0", 500, "TW"), seg("a", "J0", "J1", 1000, "TA"), seg("b", "J1", "J2", 150, "TB"), seg("c", "J2", "E", 1000, "TC")],
        "signals": [signal("S0", "w", 500), signal("S1", "a", 1000), signal("S2", "b", 150)],
        "routes": [
            {"entrance": "S0", "exit": {"kind": "signal", "name": "S1"}, "path": ["TA"], "overlap": ["TB"]},
            {"entrance": "S1", "exit": {"kind": "signal", "name": "S2"}, "path": ["TB"], "overlap": ["TC"]},
            {"entrance": "S2", "exit": {"kind": "node", "name": "E"}, "path": ["TC"]},
        ],
        "train_types": [{"code": "EMU", "max_speed_kmh": 120, "accel": 0.8, "service_brake": 0.7, "emergency_brake": 1.2, "length_m": 100}],
        "services": [{"headcode": "1A01", "train_type": "EMU"}],
        "entries": [{"service": "1A01", "boundary": "W", "time": "06:00", "speed_kmh": 100}],
        "options": {"start_time": "06:00", "entry_delay_s": [0, 0]},
    });
    Sim::new(World::from_json(&w.to_string()).unwrap(), 1)
}

#[test]
fn a_green_promises_only_a_yellow_so_close_signals_do_not_catch_the_driver_out() {
    use signalbox_core::events::{Command, Event};
    use signalbox_core::routes::Exit;
    let mut sim = close_spacing_sim(3);
    let (s0, s1, s2) = {
        let w = sim.world();
        (sig(w, "S0"), sig(w, "S1"), sig(w, "S2"))
    };
    sim.submit(Command::SetRoute { entrance: s0, exit: Exit::Signal(s1) });
    sim.submit(Command::SetRoute { entrance: s1, exit: Exit::Signal(s2) });
    let ev = sim.run_for(300.0);
    assert_eq!(count(&ev, |e| matches!(e, Event::SignalPassedAtDanger { .. })), 0);
    let t = &sim.trains()[0];
    assert_eq!((head_segment(&sim, t), t.speed), ("b".to_string(), 0.0), "stands at S2");
}

/// The same line on 2 aspects: S0's green shows before S1's red, so the driver
/// must not read it as "S1 will be yellow" and run on at line speed.
#[test]
fn a_two_aspect_green_promises_nothing_so_the_driver_expects_the_next_red() {
    use signalbox_core::events::{Command, Event};
    use signalbox_core::routes::Exit;
    let mut sim = close_spacing_sim(2);
    let (s0, s1) = {
        let w = sim.world();
        (sig(w, "S0"), sig(w, "S1"))
    };
    sim.submit(Command::SetRoute { entrance: s0, exit: Exit::Signal(s1) });
    let ev = sim.run_for(300.0);
    assert_eq!(count(&ev, |e| matches!(e, Event::SignalPassedAtDanger { .. })), 0);
    let t = &sim.trains()[0];
    assert_eq!((head_segment(&sim, t), t.speed), ("a".to_string(), 0.0), "stands at S1");
}

fn head_segment(sim: &signalbox_core::sim::Sim, t: &Train) -> String {
    sim.world().net.segments[t.head().0.idx()].name.clone()
}
