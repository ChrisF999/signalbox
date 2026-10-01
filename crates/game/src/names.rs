//! Player commands carry names; the sim wants ids.

use protocol::{ExitName, PlayerCommand};
use signalbox_core::events::Command;
use signalbox_core::routes::Exit;
use signalbox_core::world::World;

use crate::layout::exit_name;

/// The core command a named command means; `None` if a name is unknown.
pub fn resolve(w: &World, cmd: &PlayerCommand) -> Option<Command> {
    let net = &w.net;
    Some(match cmd {
        PlayerCommand::SetRoute { entrance, exit } => Command::SetRoute {
            entrance: net.signal(entrance)?,
            exit: match exit {
                ExitName::Signal(s) => Exit::Signal(net.signal(s)?),
                ExitName::Node(n) => Exit::Node(net.node(n)?),
            },
        },
        PlayerCommand::CancelRoute { entrance } => Command::CancelRoute { entrance: net.signal(entrance)? },
        PlayerCommand::SetAutoWorking { entrance, on } => {
            Command::SetAutoWorking { entrance: net.signal(entrance)?, on: *on }
        }
        PlayerCommand::SwingPoints { points, to } => Command::SwingPoints { points: net.node(points)?, to: *to },
        PlayerCommand::Interpose { berth, headcode } => {
            Command::Interpose { berth: net.berth(berth)?, headcode: headcode.clone() }
        }
        PlayerCommand::CancelBerth { berth } => Command::CancelBerth { berth: net.berth(berth)? },
    })
}

/// The named form of a command with valid ids.
pub fn to_player_command(w: &World, cmd: &Command) -> PlayerCommand {
    let net = &w.net;
    let signal = |s: signalbox_core::ids::SignalId| net.signals[s.idx()].name.clone();
    let berth = |b: signalbox_core::ids::BerthId| net.berths[b.idx()].name.clone();
    match cmd {
        Command::SetRoute { entrance, exit } => {
            PlayerCommand::SetRoute { entrance: signal(*entrance), exit: exit_name(w, *exit) }
        }
        Command::CancelRoute { entrance } => PlayerCommand::CancelRoute { entrance: signal(*entrance) },
        Command::SetAutoWorking { entrance, on } => PlayerCommand::SetAutoWorking { entrance: signal(*entrance), on: *on },
        Command::SwingPoints { points, to } => {
            PlayerCommand::SwingPoints { points: net.nodes[points.idx()].name.clone(), to: *to }
        }
        Command::Interpose { berth: b, headcode } => PlayerCommand::Interpose { berth: berth(*b), headcode: headcode.clone() },
        Command::CancelBerth { berth: b } => PlayerCommand::CancelBerth { berth: berth(*b) },
    }
}

/// 1 to 10 ASCII letters or digits.
pub fn valid_headcode(h: &str) -> bool {
    (1..=10).contains(&h.len()) && h.bytes().all(|b| b.is_ascii_alphanumeric())
}
