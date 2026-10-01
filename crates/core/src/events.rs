//! Commands players send, and events the simulation reports.

use serde::{Deserialize, Serialize};

use crate::aspect::Aspect;
use crate::ids::*;
use crate::network::PointsPos;
use crate::routes::Exit;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    SetRoute { entrance: SignalId, exit: Exit },
    CancelRoute { entrance: SignalId },
    SetAutoWorking { entrance: SignalId, on: bool },
    SwingPoints { points: NodeId, to: PointsPos },
    Interpose { berth: BerthId, headcode: String },
    CancelBerth { berth: BerthId },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rejection {
    UnknownId,
    NoSuchRoute,
    AlreadySet,
    ConflictingRoute,
    PointsLocked,
    PointsOccupied,
    RouteNotSet,
    RouteIsAutomatic,
    NotPoints,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    CommandRejected { cmd: Command, reason: Rejection },
    PointsMoving { points: NodeId, to: PointsPos },
    PointsMoved { points: NodeId, to: PointsPos },
    RouteSetting { route: RouteId },
    RouteLocked { route: RouteId },
    RouteCancelled { route: RouteId, approach_locked: bool },
    RouteReleased { route: RouteId },
    OverlapReleased { route: RouteId },
    AutoWorking { route: RouteId, on: bool },
    SignalAspect { signal: SignalId, aspect: Aspect },
    TrainEntered { train: TrainId, headcode: String },
    TrainArrived { train: TrainId, platform: PlatformId, late_s: i64 },
    TrainDeparted { train: TrainId, platform: PlatformId },
    TrainPassed { train: TrainId, platform: PlatformId, late_s: i64 },
    WrongPlatform { train: TrainId, platform: PlatformId, expected: String },
    TrainFormed { train: TrainId, headcode: String },
    TrainStabled { train: TrainId },
    TrainExited { train: TrainId, boundary: NodeId },
    /// A train's head passed a signal in its direction, whatever it showed
    /// (followed by `SignalPassedAtDanger` when that was red).
    SignalPassed { signal: SignalId, train: TrainId },
    SignalPassedAtDanger { signal: SignalId, train: TrainId },
    Collision { train: TrainId, other: TrainId, section: SectionId },
    BerthChanged { berth: BerthId, headcode: Option<String> },
    InvariantViolated { what: String },
}
