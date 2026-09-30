//! `Greedy`: a signaller that sees only what its client sees (the layout
//! and view the game sent), for bots playing over the network.
//!
//! Honest label (C2 decision 12): it is a transport exerciser, not a good
//! signaller. For each signal it may operate that shows red with a headcode
//! waiting in its berth and no route set from it, it tries that signal's
//! routes in layout order, one attempt per signal per `RETRY_S` sim seconds,
//! at most `MAX_PER_DECISION` commands per decision. The interlocking keeps
//! it safe; trains may still wait longer than under the robot.

use std::collections::BTreeMap;

use protocol::{Aspect, Layout, PlayerCommand, View};

/// Sim seconds before the same signal is tried again.
pub const RETRY_S: f64 = 30.0;
/// Commands one decision sends at most.
pub const MAX_PER_DECISION: usize = 2;

#[derive(Clone, Debug, Default)]
pub struct Greedy {
    /// Signal → sim time of its last attempt.
    tried: BTreeMap<String, f64>,
    /// Signal → how many attempts so far (picks the next route).
    attempts: BTreeMap<String, usize>,
}

impl Greedy {
    pub fn new() -> Greedy {
        Greedy::default()
    }

    /// The commands to send for this layout and view, in berth order.
    pub fn decide(&mut self, layout: &Layout, view: &View) -> Vec<PlayerCommand> {
        let now = view.sim_time;
        let mut out = Vec::new();
        for berth in layout.berths.iter().filter(|b| b.operable) {
            if out.len() >= MAX_PER_DECISION {
                break;
            }
            let Some(signal) = &berth.signal else { continue };
            if !view.berths.contains_key(&berth.name) || view.signals.get(signal) != Some(&Aspect::Red) {
                continue;
            }
            let routes: Vec<_> = layout.routes.iter().filter(|r| r.operable && &r.entrance == signal).collect();
            if routes.is_empty() || routes.iter().any(|r| view.routes.contains_key(&r.name)) {
                continue;
            }
            if self.tried.get(signal).is_some_and(|t| now - t < RETRY_S) {
                continue;
            }
            let n = self.attempts.entry(signal.clone()).or_insert(0);
            let route = routes[*n % routes.len()];
            *n += 1;
            self.tried.insert(signal.clone(), now);
            out.push(PlayerCommand::SetRoute { entrance: route.entrance.clone(), exit: route.exit.clone() });
        }
        out
    }
}
