//! The lobby form's checks (polish spec M15) and the Last played date (M14).

use client_core::form::{seed, start, utc};

#[test]
fn seeds_are_whole_numbers_or_blank() {
    assert_eq!((seed(""), seed(" 42 ")), (Ok(None), Ok(Some(42))));
    assert!(seed("abc").is_err() && seed("-1").is_err() && seed("1.5").is_err());
}

#[test]
fn starts_are_clock_times_or_blank() {
    assert_eq!((start(" "), start("8:00"), start("07:30:15")), (Ok(None), Ok(Some("8:00".into())), Ok(Some("07:30:15".into()))));
    for bad in ["25:00", "7", "07:60", "x:10", "07:30:15:00", "0730"] {
        assert!(start(bad).is_err(), "{bad}");
    }
}

#[test]
fn unix_times_read_as_utc_dates() {
    assert_eq!(utc(0), "1970-01-01 00:00 UTC");
    assert_eq!(utc(1_790_865_900), "2026-10-01 14:45 UTC");
    assert_eq!(utc(951_782_400), "2000-02-29 00:00 UTC");
}
