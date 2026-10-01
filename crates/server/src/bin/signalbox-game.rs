//! One game in its own process (spec §2.2). See `server::process`.

use std::process::ExitCode;

use server::process::{Args, USAGE, exit_status};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(&args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("signalbox-game: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a tokio runtime");
    match rt.block_on(server::process::run(args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("signalbox-game: {e}");
            ExitCode::from(exit_status(&e))
        }
    }
}
