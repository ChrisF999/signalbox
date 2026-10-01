//! Convert TS2 simulations into signalbox worlds.

pub mod areas;
pub mod graph;
pub mod layout;
pub mod lines;
pub mod report;
pub mod routes;
pub mod timetable;
pub mod ts2;
pub mod wtt;

use signalbox_core::world::file::{AreaFile, SCHEMA_VERSION, WorldFile};
use signalbox_core::world::{LoadError, World};

pub struct Conversion {
    pub world: WorldFile,
    pub report: report::Report,
}

#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("not a TS2 simulation: {0}")]
    Parse(String),
    #[error("track graph: {0}")]
    Graph(String),
    #[error("converted world is invalid (converter bug): {0}")]
    Invalid(LoadError),
}

/// Convert a TS2 simulation (JSON text) into a signalbox world file.
pub fn convert(json: &str) -> Result<Conversion, ConvertError> {
    let ts2: ts2::Ts2 = serde_json::from_str(json).map_err(|e| ConvertError::Parse(e.to_string()))?;
    let mut report = report::Report::default();
    let ends = timetable::entry_ends(&ts2);
    let g = graph::build(&ts2, &ends, &mut report).map_err(ConvertError::Graph)?;
    let tt = timetable::build(&ts2, &g, &mut report);
    let mut file = WorldFile {
        schema: SCHEMA_VERSION,
        title: ts2.options.title.clone(),
        areas: vec![AreaFile { name: g.area.clone() }],
        sections: g.sections.clone(),
        nodes: g.nodes.clone(),
        segments: g.segments.clone(),
        signals: g.signals.clone(),
        berths: g.berths.clone(),
        platforms: g.platforms.clone(),
        routes: vec![],
        train_types: tt.train_types,
        services: tt.services,
        entries: tt.entries,
        options: tt.options,
        layout: layout::build(&ts2, &g),
    };
    let world = World::from_file(file.clone()).map_err(ConvertError::Invalid)?;
    file.routes = routes::build(&ts2, &g, &world, &mut report);
    let file = routes::finish(file, &mut report)?;
    Ok(Conversion { world: file, report })
}
