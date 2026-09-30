//! One SQLite database per game (spec §7): the world, meta, the newest
//! snapshots and every command ever submitted. Only this module does I/O,
//! and only it reads the wall clock (for timestamps the sim never sees).

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use signalbox_core::events::Command;
use signalbox_core::sim::{Sim, SimState};
use signalbox_core::world::World;
use signalbox_core::world::file::WorldFile;

use crate::game::{GameMeta, ROBOT};

pub const SAVE_SCHEMA: u32 = 2;
/// Snapshots kept; older ones are deleted.
pub const KEEP_SNAPSHOTS: i64 = 3;

const SCHEMA_SQL: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE world (id INTEGER PRIMARY KEY CHECK (id = 1), json TEXT NOT NULL);
CREATE TABLE snapshots (tick INTEGER PRIMARY KEY, saved_at TEXT NOT NULL, state TEXT NOT NULL,
                        last_seq INTEGER NOT NULL);
CREATE TABLE commands (seq INTEGER PRIMARY KEY, tick INTEGER NOT NULL,
                       player TEXT NOT NULL, area TEXT NOT NULL, command TEXT NOT NULL);
";

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("sqlite: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("{0}")]
    Bad(String),
}

/// One row of the command log.
#[derive(Clone, Debug, PartialEq)]
pub struct Logged {
    pub seq: i64,
    pub tick: u64,
    pub player: String,
    pub area: String,
    pub command: Command,
}

/// What a resume needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    pub meta: GameMeta,
    pub world_json: String,
    /// The newest snapshot.
    pub snapshot: SimState,
    /// The highest `commands.seq` when the snapshot was written (0 if none):
    /// rows up to it are in the snapshot already, rows after it are replayed.
    pub last_seq: i64,
    /// Commands logged at or after the snapshot's tick, or after `last_seq`,
    /// in `seq` order.
    pub commands: Vec<Logged>,
}

pub struct SaveDb {
    conn: Connection,
}

fn now_text() -> String {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0).to_string()
}

fn wal(conn: &Connection) -> Result<(), SaveError> {
    let _mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    Ok(())
}

impl SaveDb {
    /// A new save file; `path` must not exist yet.
    pub fn create(path: &Path, meta: &GameMeta, world_json: &str) -> Result<SaveDb, SaveError> {
        if path.exists() {
            return Err(SaveError::Bad(format!("{} already exists", path.display())));
        }
        let file: WorldFile = serde_json::from_str(world_json).map_err(|e| SaveError::Bad(format!("world: {e}")))?;
        let conn = Connection::open(path)?;
        wal(&conn)?;
        let areas: Vec<&str> = file.areas.iter().map(|a| a.name.as_str()).collect();
        let areas = serde_json::to_string(&areas).expect("names serialise");
        let now = now_text();
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(SCHEMA_SQL)?;
        for (key, value) in [
            ("schema", SAVE_SCHEMA.to_string()),
            ("layout", meta.layout.clone()),
            ("seed", meta.seed.to_string()),
            ("created", now.clone()),
            ("last_played", now),
            ("areas", areas),
            ("start", file.options.start_time.clone()),
        ] {
            tx.execute("INSERT INTO meta (key, value) VALUES (?1, ?2)", params![key, value])?;
        }
        tx.execute("INSERT INTO world (id, json) VALUES (1, ?1)", params![world_json])?;
        tx.commit()?;
        Ok(SaveDb { conn })
    }

    /// An existing save file.
    pub fn open(path: &Path) -> Result<SaveDb, SaveError> {
        if !path.exists() {
            return Err(SaveError::Bad(format!("no save file {}", path.display())));
        }
        let conn = Connection::open(path)?;
        wal(&conn)?;
        Ok(SaveDb { conn })
    }

    pub fn append_command(&self, tick: u64, player: &str, area: &str, cmd: &Command) -> Result<(), SaveError> {
        let json = serde_json::to_string(cmd).expect("commands serialise");
        self.conn.execute(
            "INSERT INTO commands (tick, player, area, command) VALUES (?1, ?2, ?3, ?4)",
            params![tick as i64, player, area, json],
        )?;
        Ok(())
    }

    pub fn write_snapshot(&self, state: &SimState) -> Result<(), SaveError> {
        let json = serde_json::to_string(state).expect("state serialises");
        let now = now_text();
        let tx = self.conn.unchecked_transaction()?;
        let last_seq: i64 = tx.query_row("SELECT COALESCE(MAX(seq), 0) FROM commands", [], |r| r.get(0))?;
        tx.execute(
            "INSERT OR REPLACE INTO snapshots (tick, saved_at, state, last_seq) VALUES (?1, ?2, ?3, ?4)",
            params![state.tick as i64, now, json, last_seq],
        )?;
        tx.execute(
            "DELETE FROM snapshots WHERE tick NOT IN (SELECT tick FROM snapshots ORDER BY tick DESC LIMIT ?1)",
            params![KEEP_SNAPSHOTS],
        )?;
        tx.execute("UPDATE meta SET value = ?1 WHERE key = 'last_played'", params![now])?;
        tx.commit()?;
        Ok(())
    }

    pub fn load(&self) -> Result<Saved, SaveError> {
        let meta_value = |key: &str| -> Result<String, SaveError> {
            self.conn
                .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| r.get(0))
                .optional()?
                .ok_or_else(|| SaveError::Bad(format!("meta `{key}` is missing")))
        };
        let schema = meta_value("schema")?;
        if schema != SAVE_SCHEMA.to_string() {
            return Err(SaveError::Bad(format!("unsupported save schema {schema}")));
        }
        let seed = meta_value("seed")?.parse::<u64>().map_err(|e| SaveError::Bad(format!("meta seed: {e}")))?;
        let meta = GameMeta { layout: meta_value("layout")?, seed };
        let world_json: String = self.conn.query_row("SELECT json FROM world WHERE id = 1", [], |r| r.get(0))?;
        let newest: Option<(String, i64)> = self
            .conn
            .query_row("SELECT state, last_seq FROM snapshots ORDER BY tick DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let (state, last_seq) = newest.ok_or_else(|| SaveError::Bad("no snapshot".into()))?;
        let snapshot: SimState = serde_json::from_str(&state).map_err(|e| SaveError::Bad(format!("snapshot: {e}")))?;
        let mut st =
            self.conn.prepare("SELECT seq, tick, player, area, command FROM commands WHERE tick >= ?1 OR seq > ?2 ORDER BY seq")?;
        let rows = st.query_map(params![snapshot.tick as i64, last_seq], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?))
        })?;
        let mut commands = Vec::new();
        for row in rows {
            let (seq, tick, player, area, json) = row?;
            let command: Command = serde_json::from_str(&json).map_err(|e| SaveError::Bad(format!("command {seq}: {e}")))?;
            commands.push(Logged { seq, tick: tick as u64, player, area, command });
        }
        Ok(Saved { meta, world_json, snapshot, last_seq, commands })
    }
}

/// Rebuild a saved game's sim (spec §7.3): restore the snapshot, then
/// replay exactly the commands logged after it (`seq > last_seq`) tick by
/// tick, stopping at the last logged tick with that tick's commands queued.
/// Commands at or under `last_seq` are already in the snapshot (its queue
/// holds those at its tick). Returns the sim and whether the robot's
/// commands are among those logged at the tick it stops on (so it must not
/// run again there); when nothing is replayed that includes the robot
/// commands the snapshot already held, so a game saved again straight after
/// a resume on the robot's tick still says so.
pub fn resume_sim(world: World, snapshot: SimState, last_seq: i64, commands: &[Logged]) -> Result<(Sim, bool), String> {
    let start = snapshot.tick;
    let mut sim = Sim::restore(world, snapshot)?;
    let cmds: Vec<&Logged> = commands.iter().filter(|c| c.seq > last_seq).collect();
    if cmds.first().is_some_and(|c| c.tick < start) || cmds.windows(2).any(|p| p[1].tick < p[0].tick) {
        return Err("the command log goes back in time".into());
    }
    if let Some(last) = cmds.last().map(|c| c.tick) {
        let mut i = 0;
        loop {
            while i < cmds.len() && cmds[i].tick == sim.tick() {
                sim.submit(cmds[i].command.clone());
                i += 1;
            }
            if sim.tick() >= last {
                break;
            }
            sim.step();
        }
    }
    let end = sim.tick();
    let robot_ran = commands.iter().any(|c| c.tick == end && c.player == ROBOT);
    Ok((sim, robot_ran))
}
