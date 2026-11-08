//! The civil calendar and the UTC clock, as the C++ reads them through
//! std::chrono: `year_month_day(floor<days>(system_clock::now()))` - the date a
//! plot's frame prints and a preflight checks against - and
//! `std::format("{:%FT%TZ}", floor<seconds>(system_clock::now()))` - when a
//! survey job was made.
//!
//! The calendar is the proleptic Gregorian one std::chrono uses, and the date
//! of a count of days is Howard Hinnant's civil_from_days ("chrono-Compatible
//! Low-Level Date Algorithms"), the algorithm `year_month_day` is specified
//! by; days_from_civil is its inverse. A time before 1970 is floored, as
//! `std::chrono::floor` floors it: 1.5 s before the epoch is the second
//! 23:59:58 of 1969-12-31, not 23:59:59.

use std::time::{SystemTime, UNIX_EPOCH};

/// The seconds of a civil day: UTC, as the system clock counts it, has no
/// leap seconds.
pub const SECONDS_PER_DAY: i64 = 86_400;

/// A date of the proleptic Gregorian calendar: std::chrono::year_month_day.
/// `month` is 1 to 12 and `day` 1 to the month's length for every date the
/// functions here make.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct YearMonthDay {
    pub year: i64,
    pub month: u32,
    pub day: u32,
}

impl YearMonthDay {
    pub const fn new(year: i64, month: u32, day: u32) -> Self {
        Self { year, month, day }
    }
}

/// The date `days` after 1970-01-01 (before it, for a negative count).
pub fn civil_from_days(days: i64) -> YearMonthDay {
    // Days counted from 0000-03-01, so that a leap day is the last day of
    // its (shifted) year; an era is 400 years, 146,097 days.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * march_month + 2) / 5 + 1;
    let month = if march_month < 10 {
        march_month + 3
    } else {
        march_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    YearMonthDay {
        year,
        month: month as u32,
        day: day as u32,
    }
}

/// The days from 1970-01-01 to `date`: civil_from_days's inverse.
pub fn days_from_civil(date: YearMonthDay) -> i64 {
    let month = i64::from(date.month);
    let year = date.year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year =
        (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(date.day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// `time` as whole seconds since 1970-01-01T00:00:00Z, floored towards the
/// past as `floor<seconds>` floors it.
pub fn unix_seconds(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => {
            let before = before.duration();
            -i64::try_from(before.as_secs()).unwrap_or(i64::MAX)
                - i64::from(before.subsec_nanos() > 0)
        }
    }
}

/// The UTC date `time` falls on: `year_month_day(floor<days>(time))`.
pub fn date_of(time: SystemTime) -> YearMonthDay {
    civil_from_days(unix_seconds(time).div_euclid(SECONDS_PER_DAY))
}

/// Today's date in UTC, from the system clock.
pub fn today_utc() -> YearMonthDay {
    date_of(SystemTime::now())
}

/// `time` as the C++'s `std::format("{:%FT%TZ}", floor<seconds>(time))`:
/// "2026-11-08T09:41:07Z", UTC to the second. `%Y` writes at least four
/// digits; a year before 0 is written as a minus and four digits ("-0001"),
/// a form not compared with libstdc++'s, since no clock reaches such a year.
pub fn utc_timestamp(time: SystemTime) -> String {
    let seconds = unix_seconds(time);
    let date = civil_from_days(seconds.div_euclid(SECONDS_PER_DAY));
    let of_day = seconds.rem_euclid(SECONDS_PER_DAY);
    let year = if date.year < 0 {
        format!("-{:04}", date.year.unsigned_abs())
    } else {
        format!("{:04}", date.year)
    };
    format!(
        "{year}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        date.month,
        date.day,
        of_day / 3_600,
        of_day / 60 % 60,
        of_day % 60
    )
}

/// The system clock now, as utc_timestamp writes a time.
pub fn now_utc() -> String {
    utc_timestamp(SystemTime::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // Every expected value is a published one: the Unix epoch is day 0;
    // 2000-01-01 is 946,684,800 s, day 10,957; 2024-01-01 is 1,704,067,200 s,
    // day 19,723, so 2024-02-29 is 19,723 + 31 + 28 = 19,782; 0000-03-01 is
    // day -719,468 (Hinnant's offset); 1,700,000,000 s is
    // 2023-11-14T22:13:20Z.

    #[test]
    fn the_epoch_and_published_days_are_the_dates_they_name() {
        assert_eq!(civil_from_days(0), YearMonthDay::new(1970, 1, 1));
        assert_eq!(civil_from_days(-1), YearMonthDay::new(1969, 12, 31));
        assert_eq!(civil_from_days(10_957), YearMonthDay::new(2000, 1, 1));
        assert_eq!(civil_from_days(19_782), YearMonthDay::new(2024, 2, 29));
        assert_eq!(civil_from_days(19_783), YearMonthDay::new(2024, 3, 1));
        assert_eq!(civil_from_days(-719_468), YearMonthDay::new(0, 3, 1));
    }

    #[test]
    fn days_from_civil_inverts_civil_from_days_across_eras() {
        assert_eq!(days_from_civil(YearMonthDay::new(1970, 1, 1)), 0);
        assert_eq!(days_from_civil(YearMonthDay::new(2024, 2, 29)), 19_782);
        assert_eq!(days_from_civil(YearMonthDay::new(0, 3, 1)), -719_468);
        // Two eras either side of the epoch, every day: a leap day, a
        // century that is not a leap year and one that is all crossed.
        for days in -146_097 * 2..146_097 * 2 {
            assert_eq!(days_from_civil(civil_from_days(days)), days, "day {days}");
        }
    }

    #[test]
    fn a_timestamp_is_utc_to_the_second_floored_towards_the_past() {
        let at = |seconds: u64| UNIX_EPOCH + Duration::from_secs(seconds);
        assert_eq!(utc_timestamp(at(0)), "1970-01-01T00:00:00Z");
        assert_eq!(utc_timestamp(at(1_700_000_000)), "2023-11-14T22:13:20Z");
        assert_eq!(
            utc_timestamp(UNIX_EPOCH + Duration::from_millis(1_700_000_000_999)),
            "2023-11-14T22:13:20Z"
        );
        assert_eq!(
            utc_timestamp(UNIX_EPOCH - Duration::from_secs(1)),
            "1969-12-31T23:59:59Z"
        );
        assert_eq!(
            utc_timestamp(UNIX_EPOCH - Duration::from_millis(1_500)),
            "1969-12-31T23:59:58Z"
        );
        assert_eq!(date_of(at(946_684_800)), YearMonthDay::new(2000, 1, 1));
        assert_eq!(date_of(at(946_684_799)), YearMonthDay::new(1999, 12, 31));
    }

    #[test]
    fn the_clock_gives_a_date_and_a_timestamp_of_the_same_day() {
        // Read twice, so a midnight between the reads could part them: the
        // later read is the one compared, and a day is all either can move.
        let stamp = now_utc();
        let today = today_utc();
        assert_eq!(stamp.len(), 20, "{stamp}");
        assert!(stamp.ends_with('Z'), "{stamp}");
        let date = format!("{:04}-{:02}-{:02}", today.year, today.month, today.day);
        let yesterday = civil_from_days(days_from_civil(today) - 1);
        let before = format!(
            "{:04}-{:02}-{:02}",
            yesterday.year, yesterday.month, yesterday.day
        );
        assert!(
            stamp.starts_with(&date) || stamp.starts_with(&before),
            "{stamp} against {date}"
        );
    }
}
