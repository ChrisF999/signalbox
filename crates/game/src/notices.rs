//! One-off notices derived from a tick's events (spec §4.4), each tagged
//! with the area it concerns; the game sends them to that area's holder.

use protocol::Notice;
use signalbox_core::events::Event;
use signalbox_core::ids::*;
use signalbox_core::sim::Sim;

use crate::areas::AreaMap;

/// Trains at least this late are reported.
pub const LATE_NOTICE_S: i64 = 60;

/// Notices for one tick's `events`. `before` is the describer's berths at the
/// start of that tick (to see which headcode an emptied berth held).
pub fn area_notices(sim: &Sim, map: &AreaMap, before: &[Option<String>], events: &[Event]) -> Vec<(AreaId, Notice)> {
    let net = &sim.world().net;
    let train_name = |t: TrainId| {
        sim.trains().iter().find(|x| x.id == t).map_or_else(|| format!("train {}", t.0), |x| x.headcode.clone())
    };
    let platform = |p: PlatformId| {
        let pl = &net.platforms[p.idx()];
        let area = net.sections[net.segments[pl.segment.idx()].section.idx()].area;
        (area, pl.place.clone(), pl.platform.clone())
    };
    let mut out = Vec::new();
    for e in events {
        match e {
            Event::SignalPassedAtDanger { signal, train } => out.push((
                map.signal[signal.idx()],
                Notice::Spad { signal: net.signals[signal.idx()].name.clone(), train: train_name(*train) },
            )),
            Event::Collision { section, .. } => out.push((
                net.sections[section.idx()].area,
                Notice::Collision { section: net.sections[section.idx()].name.clone() },
            )),
            Event::TrainArrived { train, platform: p, late_s } | Event::TrainPassed { train, platform: p, late_s }
                if *late_s >= LATE_NOTICE_S =>
            {
                let (area, place, platform) = platform(*p);
                out.push((area, Notice::Late { train: train_name(*train), place, platform, late_s: *late_s }));
            }
            Event::WrongPlatform { train, platform: p, expected } => {
                let (area, place, platform) = platform(*p);
                out.push((
                    area,
                    Notice::WrongPlatform { train: train_name(*train), place, platform, expected: expected.clone() },
                ));
            }
            Event::BerthChanged { berth, headcode: Some(h) } => {
                let to = map.berth[berth.idx()];
                let from = events.iter().find_map(|x| match x {
                    Event::BerthChanged { berth: b, headcode: None }
                        if before.get(b.idx()).and_then(|o| o.as_deref()) == Some(h.as_str())
                            && map.berth[b.idx()] != to =>
                    {
                        Some(map.berth[b.idx()])
                    }
                    _ => None,
                });
                if let Some(from) = from {
                    out.push((to, Notice::Handover { headcode: h.clone(), from_area: net.areas[from.idx()].name.clone() }));
                }
            }
            _ => {}
        }
    }
    out
}
