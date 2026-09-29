//! The driver model: what speed a train aims for, and how it gets there.

use crate::aspect::{Aspect, expected_after};
use crate::ids::SignalId;
use crate::network::{Dir, Network, NodeKind, PointsView};
use crate::timetable::TrainType;
use crate::trains::Train;

/// Trains aim to stop this far short of a red signal, stop mark or buffers.
pub const STOP_MARGIN_M: f64 = 5.0;
/// How far beyond braking distance the driver looks ahead.
const EXTRA_LOOKAHEAD_M: f64 = 500.0;
const G: f64 = 9.81;

enum Mark {
    Signal(SignalId),
    Stop,
}

/// Highest speed that still lets the train stop `d` metres ahead (minus the margin).
fn stop_curve(d: f64, brake: f64) -> f64 {
    let d = d - STOP_MARGIN_M;
    if d <= 0.5 { 0.0 } else { (2.0 * brake * d).sqrt() }
}

/// Highest speed that still lets the train be at `v` when it is `d` metres ahead.
fn speed_curve(d: f64, v: f64, brake: f64) -> f64 {
    (v * v + 2.0 * brake * d.max(0.0)).sqrt()
}

/// The speed the driver aims for this tick.
///
/// Signals within sighting distance are read as they are; beyond it, the
/// driver expects what the last signal promised (a train that has passed no
/// signal yet expects the first one at red). `stop_place` is the place of the
/// next stopping call, if any.
pub fn target_speed(
    t: &Train,
    tt: &TrainType,
    net: &Network,
    pts: &impl PointsView,
    aspects: &[Aspect],
    stop_place: Option<&str>,
) -> f64 {
    let b = tt.service_brake;
    let mut target = tt.max_speed;
    for s in t.segments() {
        target = target.min(net.segments[s.idx()].line_speed);
    }
    let (seg, dir) = t.head();
    let look = t.speed * t.speed / (2.0 * b) + EXTRA_LOOKAHEAD_M;
    let (steps, end) = net.walk_ahead(seg, dir, t.head_m, look, pts);
    let mut prev = t.last_passed_aspect.unwrap_or(Aspect::Yellow);
    // Aspect count of the signal `prev` was read at. The train does not keep
    // the one it last passed, so assume 3: after a green that plans for a
    // yellow next, never less cautious than the signal promised.
    let mut prev_n = 3;
    'walk: for (i, st) in steps.iter().enumerate() {
        let sg = &net.segments[st.seg.idx()];
        if i > 0 {
            target = target.min(speed_curve(st.d_start, sg.line_speed, b));
        }
        let mut marks: Vec<(f64, Mark)> = Vec::new();
        for &s in &net.signals_on[st.seg.idx()] {
            let sig = &net.signals[s.idx()];
            if sig.at.dir != st.dir {
                continue;
            }
            let a = sg.along(sig.at.offset_m, st.dir);
            if (i == 0 && a > st.from_along) || (i > 0 && a >= st.from_along) {
                marks.push((a, Mark::Signal(s)));
            }
        }
        if let Some(place) = stop_place {
            for &p in &net.platforms_on[st.seg.idx()] {
                if net.platforms[p.idx()].place != place {
                    continue;
                }
                let (_, far) = net.platform_along(p, st.dir);
                if far > st.from_along {
                    marks.push((far, Mark::Stop));
                }
            }
        }
        marks.sort_by(|x, y| x.0.total_cmp(&y.0));
        for (a, m) in marks {
            let d = st.d_start + (a - st.from_along);
            match m {
                Mark::Stop => {
                    target = target.min(stop_curve(d, b));
                    break 'walk;
                }
                Mark::Signal(s) => {
                    let seen = if d <= net.signals[s.idx()].sighting_m { aspects[s.idx()] } else { expected_after(prev, prev_n) };
                    if seen == Aspect::Red {
                        target = target.min(stop_curve(d, b));
                        break 'walk;
                    }
                    prev = seen;
                    prev_n = net.signals[s.idx()].aspects;
                }
            }
        }
    }
    if let Some(e) = end {
        if net.nodes[e.node.idx()].kind != NodeKind::Boundary {
            target = target.min(stop_curve(e.d, b));
        }
    }
    target.max(0.0)
}

/// Move the train's speed towards `target` for one tick.
pub fn apply_speed(t: &mut Train, tt: &TrainType, net: &Network, target: f64, dt: f64) {
    if t.emergency {
        t.speed = (t.speed - tt.emergency_brake * dt).max(0.0);
        return;
    }
    let (seg, dir) = t.head();
    let rise = match dir {
        Dir::Up => net.segments[seg.idx()].gradient,
        Dir::Down => -net.segments[seg.idx()].gradient,
    };
    let accel = tt.accel - G * rise / 1000.0;
    if t.speed < target {
        t.speed = (t.speed + accel * dt).min(target).max(0.0);
    } else {
        t.speed = (t.speed - tt.service_brake * dt).max(target);
    }
}
