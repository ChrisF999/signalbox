//! Command-line converter: TS2 simulation → signalbox world.

use std::process::ExitCode;

use ts2_import::{convert, report};

const USAGE: &str = "usage: ts2-import <input.json> -o <world.json> [--strict]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut input, mut output, mut strict) = (None, None, false);
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
            "--strict" => strict = true,
            a if input.is_none() && !a.starts_with('-') => input = Some(a.to_string()),
            _ => return usage(),
        }
        i += 1;
    }
    let (Some(input), Some(output)) = (input, output) else { return usage() };
    let text = match std::fs::read_to_string(&input) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{input}: {e}");
            return ExitCode::FAILURE;
        }
    };
    match convert(&text) {
        Ok(c) => {
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
            if strict && c.report.count(report::ROUTE_DROPPED) > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS }
        }
        Err(e) => {
            eprintln!("{input}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}
