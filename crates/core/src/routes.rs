//! Route definitions (static interlocking data).

use serde::{Deserialize, Serialize};

use crate::ids::*;
use crate::network::PointsPos;

/// Where a route ends: a signal, or a buffer stop / boundary node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Exit {
    Signal(SignalId),
    Node(NodeId),
}

#[derive(Clone, Debug)]
pub struct RouteDef {
    /// "<entrance>-<exit>", e.g. "S1-E1".
    pub name: String,
    pub entrance: SignalId,
    pub exit: Exit,
    /// Sections from the entrance signal to the exit, in running order.
    pub path: Vec<SectionId>,
    pub points: Vec<(NodeId, PointsPos)>,
    /// Sections beyond the exit signal, in running order.
    pub overlap: Vec<SectionId>,
    pub overlap_points: Vec<(NodeId, PointsPos)>,
    /// Permanently set (an automatic signal).
    pub automatic: bool,
}

impl RouteDef {
    pub fn all_points(&self) -> impl Iterator<Item = &(NodeId, PointsPos)> {
        self.points.iter().chain(self.overlap_points.iter())
    }
}
