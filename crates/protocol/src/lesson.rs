//! Tutorial lessons on the wire (tutorial spec §2–§4): the lobby's list,
//! the step a player is on, and what to highlight.

use serde::{Deserialize, Serialize};

use crate::msg::ExitName;

/// One lesson the lobby offers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LessonInfo {
    /// Its directory name, e.g. `01-reading-the-panel`: what `start_lesson`
    /// names and what the client's ticks remember.
    pub id: String,
    pub title: String,
    pub steps: u32,
}

/// Something the lesson points at: an element of the diagram, or a control
/// of the screen (`ui`: `settings`, `simplifier`, `trains`, `clock`, or
/// `auto:<signal>` for a ○A button).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Highlight {
    Signal(String),
    Exit(ExitName),
    Points(String),
    Berth(String),
    Section(String),
    Platform { place: String, platform: String },
    Ui(String),
}

/// The lesson's state for its player: sent on join, on resync and on every
/// change of step or alert.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LessonView {
    /// The lesson's id (`LessonInfo::id`).
    pub lesson: String,
    pub title: String,
    /// The step showing, from 0; `count` once the lesson is done.
    pub index: u32,
    pub count: u32,
    /// What the step says (empty once done).
    pub say: String,
    #[serde(default)]
    pub highlight: Vec<Highlight>,
    /// The step waits for the player to press Next.
    pub needs_next: bool,
    pub done: bool,
    /// Said after a SPAD or a collision: the step can be restarted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alert: Option<String>,
    /// The step's task is done and it waits for Next, so the player sees
    /// what happened (polish spec H5); `after` says what to look at.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub completed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}
