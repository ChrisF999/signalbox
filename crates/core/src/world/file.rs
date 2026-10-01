//! The on-disk world format (JSON). Everything refers to everything else by name.

use serde::{Deserialize, Serialize};

use crate::network::{Dir, PointsPos};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldFile {
    pub schema: u32,
    #[serde(default)]
    pub title: String,
    pub areas: Vec<AreaFile>,
    pub sections: Vec<SectionFile>,
    pub nodes: Vec<NodeFile>,
    pub segments: Vec<SegmentFile>,
    #[serde(default)]
    pub signals: Vec<SignalFile>,
    #[serde(default)]
    pub berths: Vec<BerthFile>,
    #[serde(default)]
    pub platforms: Vec<PlatformFile>,
    #[serde(default)]
    pub routes: Vec<RouteFile>,
    #[serde(default)]
    pub train_types: Vec<TrainTypeFile>,
    #[serde(default)]
    pub services: Vec<ServiceFile>,
    #[serde(default)]
    pub entries: Vec<EntryFile>,
    #[serde(default)]
    pub options: OptionsFile,
    /// Diagram geometry for clients; never read by the simulation.
    #[serde(default)]
    pub layout: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AreaFile {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionFile {
    pub name: String,
    pub area: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeFile {
    pub name: String,
    #[serde(flatten)]
    pub kind: NodeKindFile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeKindFile {
    Joint,
    BufferStop,
    Boundary,
    Points {
        toe: String,
        normal: String,
        reverse: String,
        #[serde(default = "default_swing_s")]
        swing_s: f64,
    },
}

fn default_swing_s() -> f64 {
    5.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentFile {
    pub name: String,
    pub from: String,
    pub to: String,
    pub length_m: f64,
    pub line_speed_kmh: f64,
    #[serde(default)]
    pub gradient: f64,
    pub section: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalFile {
    pub name: String,
    pub area: String,
    pub segment: String,
    pub offset_m: f64,
    pub direction: Dir,
    pub aspects: u8,
    #[serde(default = "default_sighting_m")]
    pub sighting_m: f64,
}

fn default_sighting_m() -> f64 {
    200.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BerthFile {
    pub name: String,
    #[serde(default)]
    pub signal: Option<String>,
    #[serde(default)]
    pub boundary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformFile {
    pub place: String,
    pub platform: String,
    pub segment: String,
    pub from_m: f64,
    pub to_m: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteFile {
    pub entrance: String,
    pub exit: ExitFile,
    pub path: Vec<String>,
    #[serde(default)]
    pub points: Vec<PointsReqFile>,
    #[serde(default)]
    pub overlap: Vec<String>,
    #[serde(default)]
    pub overlap_points: Vec<PointsReqFile>,
    #[serde(default)]
    pub automatic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum ExitFile {
    Signal(String),
    Node(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointsReqFile {
    pub points: String,
    pub position: PointsPos,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainTypeFile {
    pub code: String,
    pub max_speed_kmh: f64,
    pub accel: f64,
    pub service_brake: f64,
    pub emergency_brake: f64,
    pub length_m: f64,
    #[serde(default)]
    pub mass_t: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceFile {
    /// Unique per service: entries, `form` and the describer use it.
    pub headcode: String,
    /// What the panel shows for this service where that differs from its
    /// headcode (a WTT trip `301/1` shows its train number `301`); absent
    /// means the headcode itself, so other worlds are written unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    pub train_type: String,
    #[serde(default)]
    pub calls: Vec<CallFile>,
    #[serde(default)]
    pub end: EndFile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallFile {
    pub place: String,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub arr: Option<String>,
    #[serde(default)]
    pub dep: Option<String>,
    #[serde(default = "yes")]
    pub stop: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EndFile {
    #[default]
    Exit,
    Form {
        service: String,
    },
    Stable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryFile {
    pub service: String,
    /// Enter at this boundary node...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<String>,
    /// ...or appear with the head at this position (exactly one of the two).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<PositionFile>,
    pub time: String,
    #[serde(default)]
    pub speed_kmh: f64,
    /// Offered only when asked for (`Sim::offer_entry`, the tutorial's
    /// `spawn`), never at `time`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub on_demand: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionFile {
    pub segment: String,
    /// Metres from the segment's `from` node.
    pub offset_m: f64,
    pub direction: Dir,
}

/// One band of a delay generator: `weight` (relative, e.g. percent) of the
/// draws fall uniformly in `[lo_s, hi_s]` seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelayBandFile {
    pub lo_s: i32,
    pub hi_s: i32,
    pub weight: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OptionsFile {
    pub start_time: String,
    pub entry_delay_s: [u32; 2],
    pub min_dwell_s: [u32; 2],
    /// Weighted entry-delay bands (TS2's `[lo, hi, percent]`); when present
    /// they replace `entry_delay_s`, which then only describes their span.
    /// A negative delay enters early.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entry_delay_bands: Vec<DelayBandFile>,
    /// Weighted minimum-dwell bands; when present they replace `min_dwell_s`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub min_dwell_bands: Vec<DelayBandFile>,
    pub overlap_release_s: f64,
    pub approach_lock_s: f64,
    pub late_penalty_per_min: i64,
    pub wrong_platform_penalty: i64,
    pub spad_penalty: i64,
    pub collision_penalty: i64,
}

impl Default for OptionsFile {
    fn default() -> Self {
        OptionsFile {
            start_time: "06:00:00".into(),
            entry_delay_s: [0, 0],
            min_dwell_s: [30, 30],
            entry_delay_bands: Vec::new(),
            min_dwell_bands: Vec::new(),
            overlap_release_s: 60.0,
            approach_lock_s: 120.0,
            late_penalty_per_min: 1,
            wrong_platform_penalty: 5,
            spad_penalty: 50,
            collision_penalty: 500,
        }
    }
}
