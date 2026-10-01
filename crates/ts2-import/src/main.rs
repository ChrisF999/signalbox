//! Command-line converter: TS2 simulation → signalbox world.

use std::process::ExitCode;

use ts2_import::{areas, convert, lines, report, wtt};

const USAGE: &str = "usage: ts2-import <input.json> -o <world.json> [--strict] [--areas <areas.json>] [--lines <lines.json>] [--wtt <wtt.bbox.html>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut input, mut output, mut strict, mut areas_path, mut lines_path) = (None, None, false, None, None);
    let mut wtt_path = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                match args.get(i) {
                    Some(o) => output = Some(o.clone()),
                    None => return usage(),
                }
            }
            "--areas" => {
                i += 1;
                match args.get(i) {
                    Some(a) => areas_path = Some(a.clone()),
                    None => return usage(),
                }
            }
            "--lines" => {
                i += 1;
                match args.get(i) {
                    Some(l) => lines_path = Some(l.clone()),
                    None => return usage(),
                }
            }
            "--wtt" => {
                i += 1;
                match args.get(i) {
                    Some(t) => wtt_path = Some(t.clone()),
                    None => return usage(),
                }
            }
            "--strict" => strict = true,
            a if input.is_none() && !a.starts_with('-') => input = Some(a.to_string()),
            _ => return usage(),
        }
        i += 1;
    }
    let (Some(input), Some(output)) = (input, output) else { return usage() };
    let spec = match &areas_path {
        Some(p) => match std::fs::read_to_string(p).map_err(|e| e.to_string()).and_then(|t| areas::parse(&t).map_err(|e| e.to_string())) {
            Ok(s) => Some((p.clone(), s)),
            Err(e) => {
                eprintln!("{p}: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let line_specs = match &lines_path {
        Some(p) => match std::fs::read_to_string(p).map_err(|e| e.to_string()).and_then(|t| lines::parse(&t).map_err(|e| e.to_string())) {
            Ok(l) => Some((p.clone(), l)),
            Err(e) => {
                eprintln!("{p}: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let day = match &wtt_path {
        Some(p) => match read_wtt(p) {
            Ok(d) => Some((p.clone(), d)),
            Err(e) => {
                eprintln!("{p}: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let text = match std::fs::read_to_string(&input) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{input}: {e}");
            return ExitCode::FAILURE;
        }
    };
    match convert(&text) {
        Ok(mut c) => {
            let mut counts = Vec::new();
            if let Some((p, s)) = &spec {
                match areas::apply(&mut c.world, s) {
                    Ok(v) => counts = v,
                    Err(e) => {
                        eprintln!("{p}: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            if let Some((p, l)) = &line_specs {
                if let Err(e) = lines::apply(&mut c.world, l) {
                    eprintln!("{p}: {e}");
                    return ExitCode::FAILURE;
                }
            }
            if let Some((p, (trips, checked))) = &day {
                match wtt::apply(&mut c.world, trips) {
                    Ok(r) => {
                        eprintln!(
                            "{p}: Wednesday, {} trips of {} trains; {} trips at the published running time, {} longer; snapshots {}",
                            checked.trips,
                            checked.trains,
                            checked.running_exact,
                            checked.running_longer,
                            checked.snapshots.iter().map(|(t, n)| format!("{}={n}", &signalbox_core::time::fmt_hms((*t).into())[..5])).collect::<Vec<_>>().join(" ")
                        );
                        eprintln!(
                            "{p}: {} services, {} entries from {}; left out {} empty moves and trains {:?}; roads {:?}",
                            r.services,
                            r.entries,
                            r.start_time,
                            r.dropped_empty.len(),
                            r.dropped_trains,
                            r.road_use
                        );
                        for s in &r.shortened {
                            eprintln!("{p}: {s}");
                        }
                    }
                    Err(e) => {
                        eprintln!("{p}: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            let json = serde_json::to_string_pretty(&c.world).expect("world serialises");
            if let Err(e) = std::fs::write(&output, json) {
                eprintln!("{output}: {e}");
                return ExitCode::FAILURE;
            }
            eprint!("{}", c.report.render());
            eprintln!(
                "wrote {output}: {} sections, {} signals, {} routes, {} services, {} entries",
                c.world.sections.len(),
                c.world.signals.len(),
                c.world.routes.len(),
                c.world.services.len(),
                c.world.entries.len()
            );
            for a in &counts {
                eprintln!("area {}: {} sections, {} signals", a.name, a.sections, a.signals);
            }
            if strict && c.report.count(report::ROUTE_DROPPED) > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS }
        }
        Err(e) => {
            eprintln!("{input}: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The WTT's Wednesday trips, checked against its own figures.
fn read_wtt(path: &str) -> Result<(Vec<wtt::Trip>, wtt::CheckReport), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let all = wtt::parse(&text).map_err(|e| e.to_string())?;
    let day = wtt::on_day(&all, wtt::DAY).map_err(|e| e.to_string())?;
    let checked = wtt::check(&day, &wtt::Checks::waterloo_city()).map_err(|e| e.to_string())?;
    Ok((day, checked))
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}
