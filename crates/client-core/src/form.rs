//! The lobby's New game form, checked where it is typed (polish spec M15):
//! a seed is a whole number or blank, a start time `HH:MM` or `HH:MM:SS` or
//! blank; anything else is said beside the field, never sent.

/// A typed seed: `Ok(None)` blank (random), `Ok(Some(n))`, or why not.
pub fn seed(typed: &str) -> Result<Option<u64>, &'static str> {
    let t = typed.trim();
    if t.is_empty() {
        return Ok(None);
    }
    t.parse().map(Some).map_err(|_| "a whole number, or blank for random")
}

/// A typed start time: `Ok(None)` blank (the layout's own), `Ok(Some(t))`
/// as typed, or why not. The front checks it again.
pub fn start(typed: &str) -> Result<Option<String>, &'static str> {
    let t = typed.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let parts: Vec<&str> = t.split(':').collect();
    let num = |s: &str, max: u32| s.len() <= 2 && !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && s.parse::<u32>().is_ok_and(|v| v <= max);
    let ok = matches!(parts.as_slice(), [h, m] if num(h, 23) && num(m, 59)) || matches!(parts.as_slice(), [h, m, s] if num(h, 23) && num(m, 59) && num(s, 59));
    if ok { Ok(Some(t.to_string())) } else { Err("HH:MM, or blank for the layout's start") }
}

/// Unix seconds as `2026-10-01 14:05 UTC` (the lobby's Last played).
pub fn utc(unix_s: u64) -> String {
    let (days, secs) = (unix_s / 86_400, unix_s % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", secs / 3600, secs / 60 % 60)
}
