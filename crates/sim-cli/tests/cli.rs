use std::process::{Command, Output};

const WORLD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/fixtures/plain_line.json");

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sim-cli")).args(args).output().expect("sim-cli runs")
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn hash_line(o: &Output) -> String {
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    out.lines().find_map(|l| l.split("fnv1a ").nth(1)).expect("a hash is printed").trim().to_string()
}

#[test]
fn rejects_bad_hours_and_flags_without_values() {
    for hours in ["0", "-1", "NaN", "inf"] {
        let o = cli(&["run", WORLD, "--hours", hours]);
        assert_eq!(o.status.code(), Some(2), "--hours {hours}: {}", stderr(&o));
    }
    let o = cli(&["run", WORLD, "--hours"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("--hours needs a value"), "{}", stderr(&o));
    let o = cli(&["run", WORLD, "--seed", "--robot"]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn run_and_replay_print_the_same_state_hash() {
    let dir = std::env::temp_dir().join(format!("signalbox-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("log.json");
    let log = log.to_str().unwrap();
    let run = cli(&["run", WORLD, "--hours", "0.05", "--robot", "--record", log]);
    assert!(run.status.code().is_some_and(|c| c < 2), "{}", stderr(&run));
    let replay = cli(&["replay", WORLD, log]);
    assert_eq!(replay.status.code(), Some(0), "{}", stderr(&replay));
    assert_eq!(hash_line(&run), hash_line(&replay));
    let _ = std::fs::remove_dir_all(&dir);
}
