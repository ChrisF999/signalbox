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
