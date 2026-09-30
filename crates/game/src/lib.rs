//! The multiplayer game (spec §6): a core `Sim` shared by players who each
//! run one signalling area, with the robot playing the rest. Pure logic; the
//! only I/O is the SQLite save in `save`.

pub mod areas;
pub mod layout;
