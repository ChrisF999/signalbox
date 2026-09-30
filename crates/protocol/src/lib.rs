//! Messages between signalbox clients and a game, and the per-player views
//! they carry (spec §3–§4). No I/O: callers move these as JSON text. Core's
//! small enums are reused as they are; their serde forms are the wire forms.

pub mod diff;
pub mod lobby;
pub mod msg;
pub mod view;

pub use diff::{SeqGap, diff};
pub use lobby::{AreaHolder, ClientFrame, FrameError, GameInfo, GameState, LayoutInfo, LobbyMsg, LobbyReply, ServerFrame};
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
    // Lobby errors, sent by the front as `{"type": "error", ...}`.
    pub const BAD_JSON: &str = "bad_json";
    pub const BAD_MESSAGE: &str = "bad_message";
    pub const UNKNOWN_GAME: &str = "unknown_game";
    pub const UNKNOWN_LAYOUT: &str = "unknown_layout";
    pub const BAD_START: &str = "bad_start";
    pub const NOT_IN_GAME: &str = "not_in_game";
    pub const TOO_MANY_GAMES: &str = "too_many_games";
    pub const GAME_STOPPED: &str = "game_stopped";
}
