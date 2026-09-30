//! The lobby (spec §3.1), handled by the front, and the frames that carry
//! lobby and game messages over one WebSocket. Every frame is a JSON object
//! tagged by `"type"`; lobby and game tags never collide, so a frame's type
//! alone says which enum it belongs to.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::msg::{ClientMsg, ServerMsg};

/// Client → front.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LobbyMsg {
    ListGames,
    ListLayouts,
    /// Start a new game; `seed` defaults to a random one, `start`
    /// ("HH:MM" or "HH:MM:SS") to the layout's own start time.
    CreateGame {
        layout: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seed: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start: Option<String>,
    },
    /// Join a game; a saved or crashed one is resumed.
    Join { game: String },
    /// Back to the lobby.
    Leave,
    /// Delete a saved or crashed game for good (owner decision 13): its
    /// creator or an admin only. Answered with the new `games` list.
    DeleteGame { game: String },
}

/// Front → client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LobbyReply {
    Games { games: Vec<GameInfo> },
    Layouts { layouts: Vec<LayoutInfo> },
    /// You are in `game` as `you`; its layout and view follow.
    Joined { game: String, you: String },
    Error { code: String, message: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameState {
    Running,
    Saved,
    Crashed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameInfo {
    pub id: String,
    pub layout: String,
    pub state: GameState,
    /// Seconds since midnight: live for a running game, the newest save's
    /// otherwise.
    pub sim_time: f64,
    pub areas: Vec<AreaHolder>,
    /// Connected players, in name order.
    pub players: Vec<String>,
    /// Why a crashed game stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Who created it; `None` for saves from before owner decision 13.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creator: Option<String>,
    /// Whether the user this list was sent to may delete it now.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub can_delete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AreaHolder {
    pub name: String,
    /// `None` = the robot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutInfo {
    pub name: String,
    pub areas: Vec<String>,
}

/// `"type"` tags of `LobbyMsg`.
pub const LOBBY_MSG_TYPES: [&str; 6] = ["list_games", "list_layouts", "create_game", "join", "leave", "delete_game"];
/// `"type"` tags of `ClientMsg`.
pub const CLIENT_MSG_TYPES: [&str; 5] = ["claim", "release", "command", "vote", "resync"];
/// `"type"` tags of `LobbyReply`.
pub const LOBBY_REPLY_TYPES: [&str; 4] = ["games", "layouts", "joined", "error"];
/// `"type"` tags of `ServerMsg`.
pub const SERVER_MSG_TYPES: [&str; 4] = ["layout", "view", "delta", "notice"];

/// Why a text frame could not be read. `code()` is the `error` code the
/// front answers with.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    #[error("not JSON: {0}")]
    BadJson(String),
    #[error("{0}")]
    BadMessage(String),
}

impl FrameError {
    pub fn code(&self) -> &'static str {
        match self {
            FrameError::BadJson(_) => crate::codes::BAD_JSON,
            FrameError::BadMessage(_) => crate::codes::BAD_MESSAGE,
        }
    }
}

/// Anything a client may send over the socket.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ClientFrame {
    Lobby(LobbyMsg),
    Game(ClientMsg),
}

/// Anything the front sends over the socket.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ServerFrame {
    Lobby(LobbyReply),
    Game(ServerMsg),
}

fn type_of(v: &Value) -> Result<&str, FrameError> {
    v.get("type").and_then(Value::as_str).ok_or_else(|| FrameError::BadMessage("a frame needs a string `type`".into()))
}

fn field_error(e: serde_json::Error) -> FrameError {
    FrameError::BadMessage(e.to_string())
}

impl ClientFrame {
    pub fn from_json(text: &str) -> Result<ClientFrame, FrameError> {
        let v: Value = serde_json::from_str(text).map_err(|e| FrameError::BadJson(e.to_string()))?;
        ClientFrame::from_value(v)
    }

    pub fn from_value(v: Value) -> Result<ClientFrame, FrameError> {
        let t = type_of(&v)?;
        if LOBBY_MSG_TYPES.contains(&t) {
            serde_json::from_value(v).map(ClientFrame::Lobby).map_err(field_error)
        } else if CLIENT_MSG_TYPES.contains(&t) {
            serde_json::from_value(v).map(ClientFrame::Game).map_err(field_error)
        } else {
            Err(FrameError::BadMessage(format!("unknown type `{t}`")))
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("frames serialise")
    }
}

impl ServerFrame {
    pub fn from_json(text: &str) -> Result<ServerFrame, FrameError> {
        let v: Value = serde_json::from_str(text).map_err(|e| FrameError::BadJson(e.to_string()))?;
        ServerFrame::from_value(v)
    }

    pub fn from_value(v: Value) -> Result<ServerFrame, FrameError> {
        let t = type_of(&v)?;
        if LOBBY_REPLY_TYPES.contains(&t) {
            serde_json::from_value(v).map(ServerFrame::Lobby).map_err(field_error)
        } else if SERVER_MSG_TYPES.contains(&t) {
            serde_json::from_value(v).map(ServerFrame::Game).map_err(field_error)
        } else {
            Err(FrameError::BadMessage(format!("unknown type `{t}`")))
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("frames serialise")
    }

    /// A lobby `error` frame.
    pub fn error(code: &str, message: impl Into<String>) -> ServerFrame {
        ServerFrame::Lobby(LobbyReply::Error { code: code.to_string(), message: message.into() })
    }
}

impl<'de> Deserialize<'de> for ClientFrame {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        ClientFrame::from_value(Value::deserialize(d)?).map_err(D::Error::custom)
    }
}

impl<'de> Deserialize<'de> for ServerFrame {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        ServerFrame::from_value(Value::deserialize(d)?).map_err(D::Error::custom)
    }
}
