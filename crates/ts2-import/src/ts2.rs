//! The TS2 simulation file, as far as the converter needs it. Unknown keys
//! (signal library, message logger, scores...) are ignored.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ts2 {
    pub options: Options,
    pub track_items: BTreeMap<String, Item>,
    #[serde(default)]
    pub routes: BTreeMap<String, Route>,
    #[serde(default)]
    pub services: BTreeMap<String, Service>,
    #[serde(default)]
    pub train_types: BTreeMap<String, TrainType>,
    #[serde(default)]
    pub trains: Vec<Train>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    #[serde(default)]
    pub title: String,
    pub current_time: String,
    /// m/s
    pub default_max_speed: f64,
    /// m
    pub default_signal_visibility: f64,
    /// Integer seconds, or bands `[[lo, hi, percent], ...]`.
    #[serde(default)]
    pub default_delay_at_entry: serde_json::Value,
    #[serde(default)]
    pub default_minimum_stop_time: serde_json::Value,
    #[serde(default)]
    pub late_penalty: i64,
    #[serde(default)]
    pub wrong_platform_penalty: i64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "__type__")]
pub enum Item {
    LineItem(Line),
    InvisibleLinkItem(Line),
    PointsItem(Points),
    SignalItem(Signal),
    EndItem(End),
    PlatformItem(Platform),
    Place(Place),
    TextItem(Text),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub ti_id: String,
    pub previous_ti_id: Option<String>,
    pub next_ti_id: Option<String>,
    /// m
    pub real_length: f64,
    /// m/s; 0 inherits from the place, else the default.
    #[serde(default)]
    pub max_speed: f64,
    #[serde(default)]
    pub place_code: Option<String>,
    #[serde(default)]
    pub track_code: Option<String>,
    #[serde(default)]
    pub conflict_ti_id: Option<String>,
    pub x: f64,
    pub y: f64,
    pub xf: f64,
    pub yf: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Points {
    pub ti_id: String,
    /// The toe (common end).
    pub previous_ti_id: Option<String>,
    /// The normal end.
    pub next_ti_id: Option<String>,
    /// The reverse end.
    pub reverse_ti_id: Option<String>,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Signal {
    pub ti_id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub signal_type: String,
    /// Protects trains running previous → next.
    pub previous_ti_id: Option<String>,
    pub next_ti_id: Option<String>,
    pub x: f64,
    pub y: f64,
    /// Berth position.
    #[serde(default)]
    pub xn: f64,
    #[serde(default)]
    pub yn: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct End {
    pub ti_id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub previous_ti_id: Option<String>,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Platform {
    pub ti_id: String,
    #[serde(default)]
    pub place_code: Option<String>,
    #[serde(default)]
    pub track_code: Option<String>,
    pub x: f64,
    pub y: f64,
    pub xf: f64,
    pub yf: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub ti_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub place_code: Option<String>,
    #[serde(default)]
    pub max_speed: f64,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Text {
    pub ti_id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub begin_signal: String,
    pub end_signal: String,
    /// Points tiId → 0 normal / 1 reverse, for points entered from the toe.
    #[serde(default)]
    pub directions: BTreeMap<String, u8>,
    /// 0 unset, 1 set once at start, 2 persistent.
    #[serde(default)]
    pub initial_state: u8,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub service_code: String,
    pub planned_train_type: String,
    #[serde(default)]
    pub lines: Vec<ServiceLine>,
    #[serde(default)]
    pub post_actions: Vec<Action>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLine {
    pub place_code: String,
    pub track_code: String,
    #[serde(default)]
    pub must_stop: bool,
    /// "HH:MM:SS" or "".
    #[serde(default)]
    pub scheduled_arrival_time: String,
    #[serde(default)]
    pub scheduled_departure_time: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub action_code: String,
    #[serde(default)]
    pub action_param: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainType {
    pub code: String,
    pub length: f64,
    pub max_speed: f64,
    pub std_accel: f64,
    pub std_braking: f64,
    pub emerg_braking: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Train {
    pub train_id: String,
    pub service_code: String,
    pub appear_time: String,
    #[serde(default)]
    pub initial_delay: serde_json::Value,
    #[serde(default)]
    pub initial_speed: f64,
    pub train_head: Head,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Head {
    pub track_item: String,
    /// The item behind the head: fixes the direction of travel.
    #[serde(rename = "previousTI")]
    pub previous_ti: String,
    /// Metres from the `previous_ti` end of `track_item`.
    #[serde(rename = "positionOnTI")]
    pub position_on_ti: f64,
}

/// The ends of a TS2 item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Port {
    Prev,
    Next,
    Rev,
}

impl Port {
    pub fn tag(self) -> &'static str {
        match self {
            Port::Prev => "p",
            Port::Next => "n",
            Port::Rev => "r",
        }
    }
}

impl Item {
    /// The item linked at `port`, if any.
    pub fn link(&self, port: Port) -> Option<&str> {
        match (self, port) {
            (Item::LineItem(l) | Item::InvisibleLinkItem(l), Port::Prev) => l.previous_ti_id.as_deref(),
            (Item::LineItem(l) | Item::InvisibleLinkItem(l), Port::Next) => l.next_ti_id.as_deref(),
            (Item::PointsItem(p), Port::Prev) => p.previous_ti_id.as_deref(),
            (Item::PointsItem(p), Port::Next) => p.next_ti_id.as_deref(),
            (Item::PointsItem(p), Port::Rev) => p.reverse_ti_id.as_deref(),
            (Item::SignalItem(s), Port::Prev) => s.previous_ti_id.as_deref(),
            (Item::SignalItem(s), Port::Next) => s.next_ti_id.as_deref(),
            (Item::EndItem(e), Port::Prev) => e.previous_ti_id.as_deref(),
            _ => None,
        }
    }

    /// Which of this item's ends links to `other`.
    pub fn port_to(&self, other: &str) -> Option<Port> {
        [Port::Prev, Port::Next, Port::Rev].into_iter().find(|&p| self.link(p) == Some(other))
    }
}
