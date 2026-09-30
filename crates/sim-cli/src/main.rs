//! Headless runner for signalbox worlds.

use std::process::ExitCode;

use signalbox_core::events::{Command, Event};
use signalbox_core::robot;
use signalbox_core::sim::{Sim, TICK_S};
use signalbox_core::time::fmt_hms;
use signalbox_core::world::World;

const USAGE: &str = "usage:
  sim-cli run <world.json> [--seed N] [--hours H] [--robot] [--record LOG.json]
  sim-cli replay <world.json> <LOG.json>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("run") => run(&args[1..]),
        Some("replay") => replay(&args[1..]),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::from(2)
        }
    }
}

fn load(path: &str) -> Result<World, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    World::from_json(&text).map_err(|e| format!("{path}: {e}"))
}

/// The value after `--name`; an error if the flag is there without one.
fn flag<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(None),
        Some(i) => match args.get(i + 1) {
            Some(v) if !v.starts_with("--") => Ok(Some(v.as_str())),
            _ => Err(format!("{name} needs a value")),
        },
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let path = args.first().ok_or(USAGE)?;
    let world = load(path)?;
    let seed: u64 = flag(args, "--seed")?.map_or(Ok(1), str::parse).map_err(|e| format!("--seed: {e}"))?;
    let hours: f64 = flag(args, "--hours")?.map_or(Ok(1.0), str::parse).map_err(|e| format!("--hours: {e}"))?;
    if !hours.is_finite() || hours <= 0.0 {
        return Err("--hours must be a positive, finite number".into());
    }
    let record = flag(args, "--record")?;
    let use_robot = args.iter().any(|a| a == "--robot");
    let mut sim = Sim::new(world, seed);
    let secs = hours * 3600.0;
    let report = if use_robot {
        robot::soak(&mut sim, secs)
    } else {
        let ev = sim.run_for(secs);
        robot::SoakReport {
            spads: ev.iter().filter(|e| matches!(e, Event::SignalPassedAtDanger { .. })).count(),
            collisions: ev.iter().filter(|e| matches!(e, Event::Collision { .. })).count(),
            ..Default::default()
        }
    };
    println!("{} after {} ({} ticks of {TICK_S} s)", sim.world().title, fmt_hms(sim.now_s()), sim.tick());
    println!("{}", serde_json::to_string_pretty(&report).expect("report serialises"));
    println!("state fnv1a {:016x}", sim.state_hash());
    if let Some(out) = record {
        let log = serde_json::json!({"seed": seed, "ticks": sim.tick(), "log": sim.log()});
        std::fs::write(out, serde_json::to_string_pretty(&log).expect("log serialises"))
            .map_err(|e| format!("{out}: {e}"))?;
        println!("recorded {} commands to {out}", sim.log().len());
    }
    Ok(if report.spads == 0 && report.collisions == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

fn replay(args: &[String]) -> Result<ExitCode, String> {
    let (Some(world_path), Some(log_path)) = (args.first(), args.get(1)) else {
        return Err(USAGE.to_string());
    };
    let world = load(world_path)?;
    let text = std::fs::read_to_string(log_path).map_err(|e| format!("{log_path}: {e}"))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{log_path}: {e}"))?;
    let seed = v["seed"].as_u64().ok_or("log: missing seed")?;
    let ticks = v["ticks"].as_u64().ok_or("log: missing ticks")?;
    let log: Vec<(u64, Command)> = serde_json::from_value(v["log"].clone()).map_err(|e| format!("log: {e}"))?;
    let sim = Sim::replay(world, seed, &log, ticks);
    println!("replayed {ticks} ticks; state fnv1a {:016x}", sim.state_hash());
    Ok(ExitCode::SUCCESS)
}
