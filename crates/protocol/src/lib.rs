//! Messages between signalbox clients and a game, and the per-player views
//! they carry (spec §3–§4). No I/O: callers move these as JSON text. Core's
//! small enums are reused as they are; their serde forms are the wire forms.

pub mod diff;
pub mod msg;
pub mod view;

pub use diff::{SeqGap, diff};
pub use msg::{ClientMsg, ExitName, Notice, PlayerCommand, Proposal, ServerMsg};
pub use signalbox_core::aspect::Aspect;
pub use signalbox_core::events::Rejection;
pub use signalbox_core::network::{Dir, PointsPos};
pub use view::*;

/// Codes carried by `Notice::Error`.
pub mod codes {
    pub const BAD_HEADCODE: &str = "bad_headcode";
    pub const BAD_SPEED: &str = "bad_speed";
    pub const NOT_A_HOLDER: &str = "not_a_holder";
    pub const NOT_HOLDING: &str = "not_holding";
    pub const UNKNOWN_AREA: &str = "unknown_area";
    pub const RESERVED_NAME: &str = "reserved_name";
    pub const SAVE_FAILED: &str = "save_failed";
}
