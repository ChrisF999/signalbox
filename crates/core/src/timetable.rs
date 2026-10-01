//! Train types, services, entries and simulation options.

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
    pub overlap_release_s: f64,
    pub approach_lock_s: f64,
    pub late_penalty_per_min: i64,
    pub wrong_platform_penalty: i64,
    pub spad_penalty: i64,
    pub collision_penalty: i64,
}
