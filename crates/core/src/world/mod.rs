//! A validated, name-resolved world.

pub mod file;
mod load;

use crate::network::Network;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LoadError {
    #[error("invalid JSON: {0}")]
    Json(String),
    #[error("unsupported schema version {0} (expected {expected})", expected = file::SCHEMA_VERSION)]
    Schema(u32),
    #[error("duplicate {kind} name `{name}`")]
    Duplicate { kind: &'static str, name: String },
    #[error("{from}: unknown {kind} `{name}`")]
    UnknownRef { kind: &'static str, name: String, from: String },
    #[error("node `{node}`: {problem}")]
    BadNode { node: String, problem: String },
    #[error("section `{0}` has no segments")]
    EmptySection(String),
    #[error("signal `{0}` is off its segment")]
    SignalOffTrack(String),
    #[error("berth `{0}` needs exactly one of signal or boundary")]
    BerthUnanchored(String),
    #[error("route `{route}`: {problem}")]
    BadRoute { route: String, problem: String },
    #[error("{0}")]
    Other(String),
}

use crate::ids::*;
use crate::routes::{Exit, RouteDef};
use crate::timetable::{Entry, Options, Service, TrainType};

#[derive(Debug, Clone)]
pub struct World {
    pub title: String,
    pub net: Network,
    pub routes: Vec<RouteDef>,
    /// Routes indexed by entrance signal.
    pub routes_from: Vec<Vec<RouteId>>,
    pub train_types: Vec<TrainType>,
    pub services: Vec<Service>,
    /// Sorted by time.
    pub entries: Vec<Entry>,
    pub options: Options,
    pub layout: serde_json::Value,
}

impl World {
    pub fn from_json(json: &str) -> Result<World, LoadError> {
        let f: file::WorldFile = serde_json::from_str(json).map_err(|e| LoadError::Json(e.to_string()))?;
        Self::from_file(f)
    }

    pub fn from_file(f: file::WorldFile) -> Result<World, LoadError> {
        load::build(f)
    }

    pub fn find_route(&self, entrance: SignalId, exit: Exit) -> Option<RouteId> {
        self.routes_from.get(entrance.idx())?.iter().copied().find(|r| self.routes[r.idx()].exit == exit)
    }

    pub fn route_by_name(&self, name: &str) -> Option<RouteId> {
        self.routes.iter().position(|r| r.name == name).map(RouteId::from_idx)
    }

    pub fn service(&self, headcode: &str) -> Option<ServiceId> {
        self.services.iter().position(|s| s.headcode == headcode).map(ServiceId::from_idx)
    }
}
