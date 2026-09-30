//! Client ⇄ game messages. Everything is named, never numbered.

use serde::{Deserialize, Serialize};
use signalbox_core::events::Rejection;
use signalbox_core::network::PointsPos;

use crate::view::{Delta, Layout, View};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Take a free area.
    Claim { area: String },
    /// Give your area back to the robot.
    Release,
    Command { cmd: PlayerCommand },
    /// Propose, or agree to, a clock change.
    Vote { proposal: Proposal },
    /// Ask for the layout and a full view.
    Resync,
}

/// Core `Command` with names in place of ids.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum PlayerCommand {
    SetRoute { entrance: String, exit: ExitName },
    CancelRoute { entrance: String },
    SetAutoWorking { entrance: String, on: bool },
    SwingPoints { points: String, to: PointsPos },
    Interpose { berth: String, headcode: String },
    CancelBerth { berth: String },
}

/// Where a route ends: a signal, or a buffer stop / boundary node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum ExitName {
    Signal(String),
    Node(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Proposal {
    Pause,
    Resume,
    /// `x` sim ticks per 0.1 s of real time; 1, 2, 4 or 8.
    Speed { x: u8 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Layout(Layout),
    View(View),
    Delta(Delta),
    Notice(Notice),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Notice {
    /// The command was refused (unknown name, or by the interlocking).
    Rejected { cmd: PlayerCommand, reason: Rejection },
    /// The command's subject lies in `area`, which is not yours.
    NotYourArea { area: String },
    Spad { signal: String, train: String },
    Collision { section: String },
    Late { train: String, place: String, platform: String, late_s: i64 },
    WrongPlatform { train: String, place: String, platform: String, expected: String },
    /// A berth in your area was filled by a step from `from_area`'s berth.
    Handover { headcode: String, from_area: String },
    AreaTaken { area: String, holder: String },
    Replaced,
    GameCrashed,
    Error { code: String, message: String },
}
