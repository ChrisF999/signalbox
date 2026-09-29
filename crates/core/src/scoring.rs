//! Penalty points per signalling area, driven only by events.

use serde::{Deserialize, Serialize};

use crate::events::Event;
use crate::ids::{AreaId, PlatformId};
use crate::network::Network;
use crate::world::World;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scores {
    /// Penalty points; higher is worse.
    pub by_area: Vec<i64>,
}

fn platform_area(net: &Network, p: PlatformId) -> AreaId {
    let seg = net.platforms[p.idx()].segment;
    net.sections[net.segments[seg.idx()].section.idx()].area
}

impl Scores {
    pub fn new(net: &Network) -> Self {
        Scores { by_area: vec![0; net.areas.len()] }
    }

    pub fn total(&self) -> i64 {
        self.by_area.iter().sum()
    }

    pub fn apply(&mut self, w: &World, e: &Event) {
        let o = &w.options;
        let net = &w.net;
        let (area, points) = match e {
            Event::TrainArrived { platform, late_s, .. } | Event::TrainPassed { platform, late_s, .. } => {
                (platform_area(net, *platform), (*late_s).max(0) / 60 * o.late_penalty_per_min)
            }
            Event::WrongPlatform { platform, .. } => (platform_area(net, *platform), o.wrong_platform_penalty),
            Event::SignalPassedAtDanger { signal, .. } => (net.signals[signal.idx()].area, o.spad_penalty),
            Event::Collision { section, .. } => (net.sections[section.idx()].area, o.collision_penalty),
            _ => return,
        };
        self.by_area[area.idx()] += points;
    }
}
