//! Train types, services, trains and options.

use std::collections::BTreeSet;

use signalbox_core::network::Dir;
use signalbox_core::time::{fmt_hms, parse_hms};
use signalbox_core::world::file::*;

use crate::graph::Graph;
use crate::report::{self, Report};
use crate::ts2::{Item, Ts2};

pub struct Timetable {
    pub train_types: Vec<TrainTypeFile>,
    pub services: Vec<ServiceFile>,
    pub entries: Vec<EntryFile>,
    pub options: OptionsFile,
}

/// EndItems that some train enters from (they must be boundaries).
pub fn entry_ends(ts2: &Ts2) -> BTreeSet<String> {
    ts2.trains
        .iter()
        .filter(|t| matches!(ts2.track_items.get(&t.train_head.previous_ti), Some(Item::EndItem(_))))
        .map(|t| t.train_head.previous_ti.clone())
        .collect()
}

pub fn build(ts2: &Ts2, g: &Graph, report: &mut Report) -> Timetable {
    let mut train_types: Vec<TrainTypeFile> = Vec::new();
    for t in ts2.train_types.values() {
        let numbers = [t.length, t.max_speed * 3.6, t.std_accel, t.std_braking, t.emerg_braking];
        if numbers.iter().any(|v| !(v.is_finite() && *v > 0.0)) {
            report.warn(report::TRAIN_TYPE_SKIPPED, format!("{}: lengths, speeds and rates must be positive", t.code));
        } else if train_types.iter().any(|x| x.code == t.code) {
            report.warn(report::TRAIN_TYPE_SKIPPED, format!("{}: duplicate code", t.code));
        } else {
            train_types.push(TrainTypeFile {
                code: t.code.clone(),
                max_speed_kmh: t.max_speed * 3.6,
                accel: t.std_accel,
                service_brake: t.std_braking,
                emergency_brake: t.emerg_braking,
                length_m: t.length,
                mass_t: 0.0,
            });
        }
    }

    // A service without a usable train type is dropped, and so is everything that runs it.
    let kept: BTreeSet<&str> = ts2
        .services
        .values()
        .filter(|s| {
            let ok = train_types.iter().any(|t| t.code == s.planned_train_type);
            if !ok {
                report.warn(
                    report::SERVICE_SKIPPED,
                    format!("{}: unknown or unusable train type {}", s.service_code, s.planned_train_type),
                );
            }
            ok
        })
        .map(|s| s.service_code.as_str())
        .collect();

    let tracks: BTreeSet<(&str, &str)> =
        g.platforms.iter().map(|p| (p.place.as_str(), p.platform.as_str())).collect();
    let mut services = Vec::new();
    for s in ts2.services.values().filter(|s| kept.contains(s.service_code.as_str())) {
        let mut calls = Vec::new();
        for l in &s.lines {
            if !tracks.contains(&(l.place_code.as_str(), l.track_code.as_str())) {
                report.warn(
                    report::CALL_DROPPED,
                    format!("{}: no track {} at {}", s.service_code, l.track_code, l.place_code),
                );
                continue;
            }
            let time = |x: &str| (!x.is_empty()).then(|| x.to_string());
            if [&l.scheduled_arrival_time, &l.scheduled_departure_time]
                .iter()
                .any(|x| !x.is_empty() && parse_hms(x).is_none())
            {
                report.warn(
                    report::CALL_DROPPED,
                    format!("{}: bad time at {} {}", s.service_code, l.place_code, l.track_code),
                );
                continue;
            }
            let (mut arr, mut dep) = (time(&l.scheduled_arrival_time), time(&l.scheduled_departure_time));
            if !l.must_stop {
                if dep.is_none() {
                    dep = arr.take();
                } else if arr == dep {
                    arr = None;
                }
            }
            calls.push(CallFile {
                place: l.place_code.clone(),
                platform: Some(l.track_code.clone()),
                arr,
                dep,
                stop: l.must_stop,
            });
        }
        for a in &s.post_actions {
            if a.action_code != "SET_SERVICE" && a.action_code != "REVERSE" {
                report.warn(report::ACTION, format!("{}: action {} is not supported", s.service_code, a.action_code));
            }
        }
        let next = s.post_actions.iter().find(|a| a.action_code == "SET_SERVICE").and_then(|a| a.action_param.clone());
        let reverse = s.post_actions.iter().any(|a| a.action_code == "REVERSE");
        let mut end = match (next, reverse) {
            (Some(service), _) if !kept.contains(service.as_str()) => {
                report.warn(
                    report::ACTION,
                    format!("{}: next service {service} is missing or skipped; it stables", s.service_code),
                );
                EndFile::Stable
            }
            (Some(service), rev) => {
                if !rev {
                    report.warn(
                        report::FORM_NO_REVERSE,
                        format!("{} becomes {service} without reversing; signalbox always reverses", s.service_code),
                    );
                }
                EndFile::Form { service }
            }
            (None, true) => EndFile::Stable,
            (None, false) if !g.boundaries.is_empty() => EndFile::Exit,
            (None, false) => {
                report.warn(report::EXIT_NO_BOUNDARY, format!("{} has nowhere to exit; it stables", s.service_code));
                EndFile::Stable
            }
        };
        if !matches!(end, EndFile::Exit) && !calls.iter().any(|c| c.stop) {
            report.warn(report::END_NO_STOP, format!("{} has no stopping call left; it exits", s.service_code));
            end = EndFile::Exit;
        }
        services.push(ServiceFile {
            headcode: s.service_code.clone(),
            train_type: s.planned_train_type.clone(),
            calls,
            end,
        });
    }

    let mut entries = Vec::new();
    for t in &ts2.trains {
        if !kept.contains(t.service_code.as_str()) {
            report.warn(report::TRAIN_SKIPPED, format!("train {}: unknown or skipped service {}", t.train_id, t.service_code));
            continue;
        }
        let delay = match t.initial_delay.as_i64() {
            Some(d) => d,
            None => {
                if !t.initial_delay.is_null() {
                    report.warn(report::DELAY, format!("train {}: delay bands ignored", t.train_id));
                }
                0
            }
        };
        let speed_kmh = t.initial_speed * 3.6;
        if !(speed_kmh.is_finite() && speed_kmh >= 0.0) {
            report.warn(report::TRAIN_SKIPPED, format!("train {}: bad initial speed {}", t.train_id, t.initial_speed));
            continue;
        }
        let Some(appear) = parse_hms(&t.appear_time) else {
            report.warn(report::TRAIN_SKIPPED, format!("train {}: bad appear time {}", t.train_id, t.appear_time));
            continue;
        };
        let h = &t.train_head;
        let (boundary, at) = if let Some(node) = g.end_nodes.get(&h.previous_ti) {
            if !g.boundaries.contains(node) {
                report.warn(report::TRAIN_SKIPPED, format!("train {}: starts at buffer stop {node}", t.train_id));
                continue;
            }
            (Some(node.clone()), None)
        } else if let (Some(seg), Some(Item::LineItem(l) | Item::InvisibleLinkItem(l))) =
            (g.line_segments.get(&h.track_item), ts2.track_items.get(&h.track_item))
        {
            let pos = h.position_on_ti.clamp(0.0, l.real_length);
            let (direction, offset_m) = if l.previous_ti_id.as_deref() == Some(h.previous_ti.as_str()) {
                (Dir::Up, pos)
            } else {
                (Dir::Down, l.real_length - pos)
            };
            (None, Some(PositionFile { segment: seg.clone(), offset_m, direction }))
        } else {
            report.warn(report::TRAIN_SKIPPED, format!("train {}: head is not on a kept line", t.train_id));
            continue;
        };
        let start = i64::from(appear).saturating_add(delay).max(0);
        if start >= 48 * 3600 {
            report.warn(report::TRAIN_SKIPPED, format!("train {}: enters at or after 48:00:00", t.train_id));
            continue;
        }
        entries.push(EntryFile {
            service: t.service_code.clone(),
            boundary,
            at,
            time: fmt_hms(start as f64),
            speed_kmh,
            on_demand: false,
        });
    }

    let o = &ts2.options;
    report.warn(
        report::OPTIONS_IGNORED,
        "timeFactor, trackCircuitBased, warningSpeed, wrongDestinationPenalty, currentScore and clientToken have no equivalent",
    );
    let options = OptionsFile {
        start_time: o.current_time.clone(),
        entry_delay_s: delay_range(&o.default_delay_at_entry, "entry delay", report),
        min_dwell_s: delay_range(&o.default_minimum_stop_time, "minimum stop time", report),
        late_penalty_per_min: o.late_penalty,
        wrong_platform_penalty: o.wrong_platform_penalty,
        ..OptionsFile::default()
    };
    Timetable { train_types, services, entries, options }
}

/// A TS2 delay generator (seconds, or `[lo, hi, percent]` bands) as `[min, max]`.
fn delay_range(v: &serde_json::Value, what: &str, report: &mut Report) -> [u32; 2] {
    let clamp = |x: f64| x.max(0.0).round() as u32;
    if let Some(n) = v.as_f64() {
        return [clamp(n), clamp(n)];
    }
    if let Some(bands) = v.as_array() {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for b in bands.iter().filter_map(|b| b.as_array()) {
            if let (Some(l), Some(h)) = (b.first().and_then(|x| x.as_f64()), b.get(1).and_then(|x| x.as_f64())) {
                lo = lo.min(l);
                hi = hi.max(h);
            }
        }
        if lo.is_finite() && hi.is_finite() {
            if bands.len() > 1 {
                report.warn(report::DELAY, format!("{what}: {} bands merged into [{lo}, {hi}] s", bands.len()));
            }
            let (l, h) = (clamp(lo), clamp(hi));
            return [l, h.max(l)];
        }
    }
    [0, 0]
}
