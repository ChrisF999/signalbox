//! Points positions and movement.

use serde::{Deserialize, Serialize};

use crate::ids::NodeId;
use crate::network::{Network, NodeKind, PointsPos, PointsView};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PointsState {
    Set(PointsPos),
    Moving { to: PointsPos, remaining_s: f64 },
}

/// State of every points node, indexed by node (`None` for other nodes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointsTable {
    states: Vec<Option<PointsState>>,
}

impl PointsTable {
    /// Number of nodes the table covers (one slot per node).
    pub fn len(&self) -> usize {
        self.states.len()
    }

    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// All points start detected normal.
    pub fn new(net: &Network) -> Self {
        let states = net
            .nodes
            .iter()
            .map(|n| matches!(n.kind, NodeKind::Points { .. }).then_some(PointsState::Set(PointsPos::Normal)))
            .collect();
        PointsTable { states }
    }

    pub fn state(&self, n: NodeId) -> Option<PointsState> {
        self.states.get(n.idx()).copied().flatten()
    }

    /// The detected position, or `None` while moving (or for non-points).
    pub fn detected(&self, n: NodeId) -> Option<PointsPos> {
        match self.state(n) {
            Some(PointsState::Set(p)) => Some(p),
            _ => None,
        }
    }

    /// Start moving towards `to` unless already detected there.
    pub fn start_swing(&mut self, n: NodeId, to: PointsPos, swing_s: f64) {
        if let Some(s) = self.states[n.idx()].as_mut() {
            if *s != PointsState::Set(to) {
                *s = PointsState::Moving { to, remaining_s: swing_s };
            }
        }
    }

    /// Advance movement by `dt`; returns the points that finished moving.
    pub fn tick(&mut self, dt: f64) -> Vec<(NodeId, PointsPos)> {
        let mut done = Vec::new();
        for (i, s) in self.states.iter_mut().enumerate() {
            if let Some(PointsState::Moving { to, remaining_s }) = s {
                *remaining_s -= dt;
                if *remaining_s <= 0.0 {
                    let to = *to;
                    *s = Some(PointsState::Set(to));
                    done.push((NodeId::from_idx(i), to));
                }
            }
        }
        done
    }
}

impl PointsView for PointsTable {
    fn position(&self, node: NodeId) -> Option<PointsPos> {
        self.detected(node)
    }
}
