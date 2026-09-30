//! The multiplayer game (spec §6): a core `Sim` shared by players who each
//! run one signalling area, with the robot playing the rest. Pure logic; the
//! only I/O is the SQLite save in `save`.

pub mod areas;
pub mod clock;
pub mod display;
pub mod game;
pub mod geometry;
pub mod layout;
pub mod names;
pub mod notices;
pub mod save;
pub mod view;

pub use game::{GRACE_S, Game, GameError, GameMeta, GameStats, GameStatus, MAX_TICKS_PER_ADVANCE, Out, ROBOT, SNAPSHOT_EVERY_S};
