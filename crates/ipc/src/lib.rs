//! Front ⇄ game messages over a Unix stream socket (spec §9). Each frame is
//! a big-endian `u32` length followed by that many bytes of JSON. Both ends
//! run tokio; the codec works on any `AsyncRead`/`AsyncWrite`.

use std::collections::BTreeMap;

use protocol::{ClientMsg, Preparing, ServerMsg};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Largest frame either side sends or accepts. A spectator's layout of the
/// biggest shipped world is well under this.
pub const MAX_FRAME: usize = 4 << 20;

/// Front → game.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToGame {
    Connect { player: String },
    Disconnect { player: String },
    Client { player: String, msg: ClientMsg },
    /// Save and exit.
    Shutdown,
}

/// Game → front.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FromGame {
    ToPlayer { player: String, msg: ServerMsg },
    /// For the lobby, at most once a second.
    Status(StatusMsg),
    /// A snapshot at `tick` is on disk.
    Saved { tick: u64 },
    /// Something the front should log (save failures, for one).
    Log { level: LogLevel, message: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatusMsg {
    /// Seconds since midnight.
    pub sim_time: f64,
    pub tick: u64,
    pub paused: bool,
    pub speed: u8,
    /// Area → holder (`None` = the robot).
    pub holders: BTreeMap<String, Option<String>>,
    pub players: Vec<PlayerStatus>,
    pub counters: Counters,
    /// The game is still being prepared (timetables spec §3.4): nobody
    /// plays until a status without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preparing: Option<Preparing>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerStatus {
    pub name: String,
    pub connected: bool,
}

/// Running totals from the game, passed through the front untouched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counters {
    pub spads: u64,
    pub collisions: u64,
    pub invariant_violations: u64,
    pub player_commands: u64,
    pub robot_commands: u64,
    /// Milliseconds of wall time spent writing the save.
    pub save_busy_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("frame of {0} bytes is over the {MAX_FRAME} byte limit")]
    TooLarge(usize),
    #[error("the stream ended inside a frame")]
    Truncated,
    #[error("bad frame: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Write one frame and flush it.
pub async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(w: &mut W, msg: &T) -> Result<(), IpcError> {
    let body = serde_json::to_vec(msg)?;
    if body.len() > MAX_FRAME {
        return Err(IpcError::TooLarge(body.len()));
    }
    let mut buf = Vec::with_capacity(4 + body.len());
    buf.extend_from_slice(&(body.len() as u32).to_be_bytes());
    buf.extend_from_slice(&body);
    w.write_all(&buf).await?;
    w.flush().await?;
    Ok(())
}

/// Read one frame: `Ok(None)` at a clean end of stream (between frames).
/// The length is checked before anything is allocated. Not cancel-safe:
/// call it from a task of its own, never as a `select!` branch.
pub async fn read_frame<R: AsyncRead + Unpin, T: DeserializeOwned>(r: &mut R) -> Result<Option<T>, IpcError> {
    let mut len = [0u8; 4];
    let mut got = 0;
    while got < len.len() {
        let n = r.read(&mut len[got..]).await?;
        if n == 0 {
            return if got == 0 { Ok(None) } else { Err(IpcError::Truncated) };
        }
        got += n;
    }
    let n = u32::from_be_bytes(len) as usize;
    if n > MAX_FRAME {
        return Err(IpcError::TooLarge(n));
    }
    let mut body = vec![0u8; n];
    r.read_exact(&mut body).await.map_err(|e| match e.kind() {
        std::io::ErrorKind::UnexpectedEof => IpcError::Truncated,
        _ => IpcError::Io(e),
    })?;
    Ok(Some(serde_json::from_slice(&body)?))
}
