//! What a player sees: the static `Layout` of their screen and the dynamic
//! `View`, keyed by element name, plus the `Delta` between two views.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use signalbox_core::aspect::Aspect;
use signalbox_core::network::{Dir, PointsPos};

use crate::msg::{ExitName, Proposal};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub title: String,
    /// The player this layout was built for.
    pub you: String,
    /// The area they hold; `None` for a spectator.
    pub area: Option<String>,
    /// Every area of the layout, in world order.
    pub areas: Vec<String>,
    pub sections: Vec<SectionInfo>,
    pub segments: Vec<SegmentInfo>,
    pub signals: Vec<SignalInfo>,
    pub points: Vec<PointsInfo>,
    pub berths: Vec<BerthInfo>,
    pub platforms: Vec<PlatformInfo>,
    pub routes: Vec<RouteInfo>,
    /// The diagram of the visible part; `None` when the world has none.
    #[serde(default)]
    pub geometry: Option<Geometry>,
    /// The box's signal prefix (`L` for Liverpool Street); may be empty.
    #[serde(default)]
    pub box_prefix: String,
    /// Area → its workstation letter (realism spec §2, owner decision 11).
    #[serde(default)]
    pub workstations: BTreeMap<String, String>,
    /// The timetable for your area (spectators: all of it), in running
    /// order (realism spec §3).
    #[serde(default)]
    pub simplifier: Vec<SimplifierRow>,
}

/// One service in the simplifier: where it runs from and to, and its calls
/// in the area.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimplifierRow {
    pub headcode: String,
    /// The first call's place, the last call's place.
    pub origin: Option<String>,
    pub destination: Option<String>,
    pub calls: Vec<SimplifierCall>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimplifierCall {
    pub place: String,
    pub platform: Option<String>,
    /// Booked times, seconds since midnight.
    pub arr: Option<f64>,
    pub dep: Option<f64>,
    /// `false`: booked to pass.
    pub stops: bool,
}

/// Diagram geometry in the layout's own coordinates (TS2 scene units, y
/// grows downwards), limited to what the player sees.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    pub lines: Vec<LineGeom>,
    pub points: Vec<PointsGeom>,
    pub signals: Vec<SignalGeom>,
    pub platforms: Vec<PlatformGeom>,
    pub labels: Vec<LabelGeom>,
    /// Where route exits at nodes (buffer stops, boundaries) and boundary
    /// berths are drawn.
    pub nodes: Vec<NodeGeom>,
}

/// A segment drawn from (x1, y1) at its `from` node to (x2, y2) at its `to` node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineGeom {
    pub segment: String,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

/// Points at (x, y); each leg ends where the next drawn line starts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointsGeom {
    pub node: String,
    pub x: f64,
    pub y: f64,
    pub toe: Option<[f64; 2]>,
    pub normal: Option<[f64; 2]>,
    pub reverse: Option<[f64; 2]>,
}

/// A signal at (x, y), its berth box at (berth_x, berth_y), and `facing`,
/// the direction a train passing it travels (not normalised).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignalGeom {
    pub signal: String,
    pub x: f64,
    pub y: f64,
    pub berth_x: f64,
    pub berth_y: f64,
    pub facing: Option<[f64; 2]>,
}

/// A platform rectangle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlatformGeom {
    pub place: String,
    pub platform: String,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeGeom {
    pub node: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LabelGeom {
    pub text: String,
    pub x: f64,
    pub y: f64,
    /// A line name's direction of travel: the arrow is drawn at (x, y)
    /// pointing this way, the text on the other side of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrow: Option<[f64; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionInfo {
    pub name: String,
    pub area: String,
    /// Visible but in a neighbouring area.
    pub fringe: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SegmentInfo {
    pub name: String,
    pub from: String,
    pub to: String,
    pub length_m: f64,
    pub section: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignalInfo {
    pub name: String,
    pub area: String,
    pub segment: String,
    pub offset_m: f64,
    pub direction: Dir,
    pub aspects: u8,
    pub operable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointsInfo {
    pub name: String,
    pub section: String,
    pub area: String,
    pub operable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BerthInfo {
    pub name: String,
    pub signal: Option<String>,
    pub boundary: Option<String>,
    pub area: String,
    pub operable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlatformInfo {
    pub place: String,
    pub platform: String,
    pub segment: String,
    pub from_m: f64,
    pub to_m: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteInfo {
    pub name: String,
    pub entrance: String,
    pub exit: ExitName,
    pub automatic: bool,
    pub operable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub seq: u64,
    /// Seconds since midnight.
    pub sim_time: f64,
    pub speed: u8,
    pub paused: bool,
    pub vote: Option<VoteView>,
    /// Area → holding player, or `"robot"`.
    pub holders: BTreeMap<String, String>,
    /// Your area's penalty points; `None` for a spectator.
    pub score: Option<i64>,
    pub signals: BTreeMap<String, Aspect>,
    /// Only routes that are not idle.
    pub routes: BTreeMap<String, RouteView>,
    pub points: BTreeMap<String, PointsView>,
    pub sections: BTreeMap<String, SectionView>,
    /// Only berths holding a headcode.
    pub berths: BTreeMap<String, String>,
    /// Trains this player should know about, by headcode (spec D1 §4.2).
    #[serde(default)]
    pub trains: BTreeMap<String, TrainRow>,
}

/// In the order a train goes through them (the train list sorts by it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainState {
    /// Not on the railway yet.
    Due,
    /// Running, but outside your area.
    Approaching,
    InArea,
    /// Standing at a platform (dwelling).
    AtPlatform,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainRow {
    /// The next call, `None` once the timetable is done.
    pub next_place: Option<String>,
    pub next_platform: Option<String>,
    /// Booked time at the next call (arrival, else departure), seconds since midnight.
    pub booked: Option<f64>,
    /// How late against `booked` right now, in whole minutes, as seconds; never negative.
    pub late_s: i64,
    pub state: TrainState,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoteView {
    pub proposal: Proposal,
    pub agreed: Vec<String>,
    /// Whole seconds of real time before it lapses (rounded up).
    pub expires_in_s: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteState {
    Setting,
    Locked,
    Cancelling,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteView {
    pub state: RouteState,
    pub auto_working: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointsView {
    /// Where the points lie, or are moving to.
    pub position: PointsPos,
    pub moving: bool,
    /// Their section is held by a route.
    pub locked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Held {
    Free,
    Path,
    Overlap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionView {
    pub occupied: bool,
    pub held: Held,
}

/// Changes since view `seq - 1`. Absent = unchanged; `null` = cleared.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    pub seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sim_time: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "double_option")]
    pub vote: Option<Option<VoteView>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "double_option")]
    pub score: Option<Option<i64>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub holders: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub signals: BTreeMap<String, Aspect>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub routes: BTreeMap<String, Option<RouteView>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub points: BTreeMap<String, PointsView>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sections: BTreeMap<String, SectionView>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub berths: BTreeMap<String, Option<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub trains: BTreeMap<String, Option<TrainRow>>,
}

impl Delta {
    /// Nothing but the sequence number.
    pub fn is_empty(&self) -> bool {
        *self == Delta { seq: self.seq, ..Delta::default() }
    }
}

/// `Some(None)` ⇄ `null`, `Some(Some(v))` ⇄ `v`; an absent field is `None`.
mod double_option {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<T: Serialize, S: Serializer>(v: &Option<Option<T>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(inner) => inner.serialize(s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
        Option::<T>::deserialize(d).map(Some)
    }
}
