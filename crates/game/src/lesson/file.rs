//! The lesson file, `lessons/<id>/lesson.json` (tutorial spec §2): a title,
//! the player's area, a start time and the steps. Every object refuses
//! unknown fields, so a misspelt key is an error at load, not a step that
//! never ends.

use protocol::{ExitName, Highlight, PlayerCommand, PointsPos};
use serde::{Deserialize, Serialize};

pub const LESSON_SCHEMA: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LessonFile {
    pub schema: u32,
    pub title: String,
    /// The area the player holds; any other is the robot's.
    pub area: String,
    /// The sim's start time, "HH:MM" or "HH:MM:SS".
    pub start: String,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub say: String,
    #[serde(default)]
    pub highlight: Vec<Highlight>,
    /// Run in order when the step starts (and again on Restart step).
    #[serde(default, rename = "do")]
    pub actions: Vec<Action>,
    pub wait_for: Condition,
    /// What a player does to complete the step, for the CI play-through
    /// when `wait_for` alone does not say (`rejected`, say).
    #[serde(default)]
    pub solution: Vec<Move>,
}

/// What a step waits for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    /// The player presses Next.
    Continue {},
    /// The player has chosen this signal as an entrance (client-reported).
    Selected { signal: String },
    /// The route is set and locked.
    RouteSet { entrance: String, exit: ExitName },
    /// A route from the signal has been cancelled and none is set now.
    RouteCancelled { entrance: String },
    /// The points lie (detected) this way.
    Points { name: String, position: PointsPos },
    /// `on`: a route set from the signal is auto-working; `off`: none is.
    AutoWorking { signal: String, on: bool },
    /// The train stands at the platform for its booked stop.
    TrainAt { headcode: String, place: String, platform: String },
    /// The train has passed the signal.
    TrainPassed { headcode: String, signal: String },
    /// The train has been in the lesson's area and is not now.
    TrainLeftArea { headcode: String },
    /// The berth shows the headcode.
    Berth { name: String, headcode: String },
    /// A command of the player's was refused in this step.
    Rejected {},
    Clock { paused: bool },
    /// The side panel shows this tab (`trains`, `simplifier`; client-reported).
    Tab(String),
    /// Every one of these.
    All(Vec<Condition>),
}

/// What the lesson does when a step starts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Offer the world's on-demand entry for `headcode` at boundary `entry`.
    Spawn { headcode: String, entry: String },
    Pause {},
    Run {},
    Speed { x: u8 },
    /// The lesson demonstrating: these act whoever holds the area.
    SetRoute { entrance: String, exit: ExitName },
    CancelRoute { entrance: String },
    Interpose { berth: String, headcode: String },
}

/// A player command in a step's `solution`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Move {
    SetRoute { entrance: String, exit: ExitName },
    CancelRoute { entrance: String },
    SetAutoWorking { entrance: String, on: bool },
    SwingPoints { points: String, to: PointsPos },
    Interpose { berth: String, headcode: String },
}

impl Move {
    pub fn command(&self) -> PlayerCommand {
        match self.clone() {
            Move::SetRoute { entrance, exit } => PlayerCommand::SetRoute { entrance, exit },
            Move::CancelRoute { entrance } => PlayerCommand::CancelRoute { entrance },
            Move::SetAutoWorking { entrance, on } => PlayerCommand::SetAutoWorking { entrance, on },
            Move::SwingPoints { points, to } => PlayerCommand::SwingPoints { points, to },
            Move::Interpose { berth, headcode } => PlayerCommand::Interpose { berth, headcode },
        }
    }
}

impl Condition {
    /// Whether the player must press Next for it (it is, or includes, `continue`).
    pub fn needs_next(&self) -> bool {
        match self {
            Condition::Continue {} => true,
            Condition::All(v) => v.iter().any(Condition::needs_next),
            _ => false,
        }
    }

    /// It and every condition inside it, depth first.
    pub fn leaves(&self) -> Vec<&Condition> {
        match self {
            Condition::All(v) => v.iter().flat_map(Condition::leaves).collect(),
            c => vec![c],
        }
    }
}
