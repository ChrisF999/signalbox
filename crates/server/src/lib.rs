//! signalbox's server side: the game process (`process`, run by the
//! `signalbox-game` binary) and the front's parts: the layouts it offers,
//! per-client outbound queues and the supervisor of game processes.

pub mod layouts;
pub mod outbox;
pub mod process;
pub mod supervisor;
