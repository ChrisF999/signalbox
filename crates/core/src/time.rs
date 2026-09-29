//! Clock times in world files ("HH:MM" or "HH:MM:SS").

/// Parse "HH:MM" or "HH:MM:SS" into seconds since midnight. Hours may go past
/// 24 (up to 47) for services that run after midnight.
pub fn parse_hms(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let mut total = 0u32;
    for (i, p) in parts.iter().enumerate() {
        let v: u32 = p.parse().ok()?;
        let limit = if i == 0 { 48 } else { 60 };
        if v >= limit {
            return None;
        }
        total = total * 60 + v;
    }
    if parts.len() == 2 {
        total *= 60;
    }
    Some(total)
}

/// Format seconds since midnight as "HH:MM:SS" (fractions truncated).
pub fn fmt_hms(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hours_and_minutes() {
        assert_eq!(parse_hms("06:05"), Some(6 * 3600 + 5 * 60));
    }

    #[test]
    fn parses_seconds() {
        assert_eq!(parse_hms("06:05:30"), Some(6 * 3600 + 5 * 60 + 30));
    }

    #[test]
    fn allows_times_past_midnight() {
        assert_eq!(parse_hms("25:00"), Some(25 * 3600));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_hms("6"), None);
        assert_eq!(parse_hms("06:60"), None);
        assert_eq!(parse_hms("aa:bb"), None);
        assert_eq!(parse_hms("06:00:00:00"), None);
    }

    #[test]
    fn formats() {
        assert_eq!(fmt_hms(6.0 * 3600.0 + 5.0 * 60.0 + 30.4), "06:05:30");
    }
}
