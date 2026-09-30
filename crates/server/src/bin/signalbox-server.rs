//! The front (spec §8). Configuration comes from the environment; see
//! `server::config`.

use std::process::ExitCode;

fn main() -> ExitCode {
    let cfg = match server::config::Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("signalbox-server: {e}");
            return ExitCode::from(2);
        }
    };
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a tokio runtime");
    match rt.block_on(server::run(cfg)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("signalbox-server: {e}");
            ExitCode::FAILURE
        }
    }
}
