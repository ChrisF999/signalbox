//! Train types, services, entries and simulation options.

use rand::Rng;

use crate::ids::*;
use crate::network::Position;

#[derive(Clone, Debug)]
pub struct TrainType {
    pub code: String,
    /// m/s
    pub max_speed: f64,
    /// m/s²
    pub accel: f64,
    pub service_brake: f64,
    pub emergency_brake: f64,
    pub length_m: f64,
    /// Reserved for realistic traction; unused in v1.
    pub mass_t: f64,
}

#[derive(Clone, Debug)]
pub struct Call {
    pub place: String,
    /// Planned platform, if the timetable names one.
    pub platform: Option<String>,
    /// Seconds since midnight.
    pub arr_s: Option<f64>,
    pub dep_s: Option<f64>,
    /// false = pass without stopping.
    pub stop: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EndAction {
    Exit,
    Form(ServiceId),
    Stable,
}

#[derive(Clone, Debug)]
pub struct Service {
    pub headcode: String,
    /// What the panel shows: the world file's `display`, else the headcode.
    pub display: String,
    pub train_type: TrainTypeId,
    pub calls: Vec<Call>,
    pub end: EndAction,
}

/// Where an entering train appears.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EntryStart {
    Boundary(NodeId),
    /// Head at this position, the rest of the train laid behind it.
    At(Position),
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub service: ServiceId,
    pub start: EntryStart,
    pub time_s: f64,
    /// m/s
    pub speed: f64,
    /// Offered only by `Sim::offer_entry`, never at `time_s`.
    pub on_demand: bool,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub start_s: f64,
    /// Uniform range [min, max] seconds added to each entry time.
    pub entry_delay_s: (u32, u32),
    /// Uniform range [min, max] seconds a train dwells at a stop at least.
    pub min_dwell_s: (u32, u32),
    /// Weighted bands that replace `entry_delay_s` when not empty (a
    /// negative delay enters early).
    pub entry_delay_bands: Vec<DelayBand>,
    /// Weighted bands that replace `min_dwell_s` when not empty (never negative).
    pub min_dwell_bands: Vec<DelayBand>,
    pub overlap_release_s: f64,
    pub approach_lock_s: f64,
    pub late_penalty_per_min: i64,
    pub wrong_platform_penalty: i64,
    pub spad_penalty: i64,
    pub collision_penalty: i64,
}

/// One band of a delay generator: `weight` (relative) of the draws fall
/// uniformly in `[lo_s, hi_s]` seconds. Loaded bands have `lo_s <= hi_s`
/// and a positive total weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayBand {
    pub lo_s: i32,
    pub hi_s: i32,
    pub weight: u32,
}

/// Draw a delay in seconds. Without bands this is one `random_range(lo..=hi)`
/// over `range`, exactly as before bands existed (old worlds replay
/// bit-identically). With bands it is two draws in this order: an integer in
/// `0..total weight` picks the band (bands in their listed order), then one
/// `random_range(lo_s..=hi_s)` inside it. Overlapping bands are allowed.
pub fn draw_delay(bands: &[DelayBand], range: (u32, u32), rng: &mut impl Rng) -> i32 {
    if bands.is_empty() {
        let d = rng.random_range(range.0..=range.1);
        return i32::try_from(d).unwrap_or(i32::MAX);
    }
    let total: u64 = bands.iter().map(|b| u64::from(b.weight)).sum();
    let mut pick = rng.random_range(0..total.max(1));
    let mut chosen = bands[bands.len() - 1];
    for b in bands {
        if pick < u64::from(b.weight) {
            chosen = *b;
            break;
        }
        pick -= u64::from(b.weight);
    }
    rng.random_range(chosen.lo_s..=chosen.hi_s)
}

/// The earliest a band can put an entry before its booked time (seconds, `<= 0`).
pub fn earliest_delay_s(bands: &[DelayBand]) -> i32 {
    bands.iter().map(|b| b.lo_s).min().unwrap_or(0).min(0)
}
