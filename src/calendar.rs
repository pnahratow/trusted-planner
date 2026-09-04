//! Date-range maths. A view is a date range plus a layout (D18), and this is
//! the range half — the only place that knows how many days a view holds.

use std::sync::OnceLock;

use anyhow::{Result, bail};
use chrono::{Datelike, Days, NaiveDate, Weekday};
use chrono_tz::Tz;

/// Monday of the week containing `date`. Weeks start Monday (v1, not configurable).
///
/// Checked arithmetic throughout this module, and the fallback is always the
/// date we were given: the only inputs that cannot step are within a week of
/// the ends of the representable calendar, roughly 262000 BC and AD, and a
/// planner that is wrong there but never panics is the right trade.
pub fn monday_of(date: NaiveDate) -> NaiveDate {
    let back = u64::from(date.weekday().num_days_from_monday());
    date.checked_sub_days(Days::new(back)).unwrap_or(date)
}

/// `n` days on from `date`, or `date` itself at the end of the calendar.
pub fn plus_days(date: NaiveDate, n: u64) -> NaiveDate {
    date.checked_add_days(Days::new(n)).unwrap_or(date)
}

/// `n` days back from `date`, or `date` itself at the start of the calendar.
pub fn minus_days(date: NaiveDate, n: u64) -> NaiveDate {
    date.checked_sub_days(Days::new(n)).unwrap_or(date)
}

/// A week on, and a week back — how the four-week grid pages, because its rows
/// only line up while it starts on a Monday.
pub fn plus_weeks(date: NaiveDate, n: u64) -> NaiveDate {
    plus_days(date, n.saturating_mul(7))
}

pub fn minus_weeks(date: NaiveDate, n: u64) -> NaiveDate {
    minus_days(date, n.saturating_mul(7))
}

/// The first and last date of a range. `None` for an empty one, which the
/// callers treat as nothing to render — the alternative is an index that the
/// caller has to be trusted not to get wrong.
pub fn ends(dates: &[NaiveDate]) -> Option<(NaiveDate, NaiveDate)> {
    Some((*dates.first()?, *dates.last()?))
}

/// The 7 dates of the week starting at `monday`.
pub fn week_of(monday: NaiveDate) -> Vec<NaiveDate> {
    weeks_from(monday, 1)
}

/// How many weeks the multi-week grid shows.
///
/// Fixed at four rather than "a calendar month" on purpose: a month grid is 35
/// cells some months and 42 others, so the layout reflows as you page through
/// it and rows change height. Four weeks is always 4x7, every cell the same
/// size, every row aligned.
pub const VIEW_WEEKS: usize = 4;

/// `weeks` consecutive weeks of dates starting at `monday`.
pub fn weeks_from(monday: NaiveDate, weeks: usize) -> Vec<NaiveDate> {
    std::iter::successors(Some(monday), NaiveDate::succ_opt)
        .take(weeks.saturating_mul(7))
        .collect()
}

/// `YYYY-MM-DD`, the only date format that crosses the wire or hits the database.
pub fn fmt(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

pub fn parse(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// Short weekday label for a column header.
pub fn weekday_label(date: NaiveDate) -> &'static str {
    match date.weekday() {
        Weekday::Mon => "Mon",
        Weekday::Tue => "Tue",
        Weekday::Wed => "Wed",
        Weekday::Thu => "Thu",
        Weekday::Fri => "Fri",
        Weekday::Sat => "Sat",
        Weekday::Sun => "Sun",
    }
}

/// The zone "today" is answered in, fixed once at startup.
///
/// A container has no local time — it is UTC unless told otherwise — so a task
/// added at half past eleven at night would land on tomorrow's column for a
/// household east of Greenwich. The zone database is compiled into the binary,
/// so this works without `tzdata` in the image.
static ZONE: OnceLock<Tz> = OnceLock::new();

/// Where this app is. Set `PLANNER_TZ` to move it.
///
/// A default that is a real place beats one that is merely defensible: the
/// household this is written for is in Berlin, so an unconfigured container is
/// right for them rather than eight hours of every day wrong.
const DEFAULT_ZONE: Tz = chrono_tz::Europe::Berlin;

/// Interpret `name` as an IANA zone (`Europe/Berlin`, `UTC`).
fn zone(name: &str) -> Result<Tz> {
    match name.trim().parse() {
        Ok(tz) => Ok(tz),
        Err(_) => bail!("unknown timezone {name:?} (expected an IANA name like Europe/Berlin)"),
    }
}

/// Pin the zone every date in the app is read in. Called once, from startup.
pub fn set_timezone(name: &str) -> Result<()> {
    let _ = ZONE.set(zone(name)?);
    Ok(())
}

/// The zone in force — configured, or [`DEFAULT_ZONE`].
pub fn timezone() -> Tz {
    ZONE.get().copied().unwrap_or(DEFAULT_ZONE)
}

/// Full weekday name, for the day panel's heading.
pub fn weekday_name(date: NaiveDate) -> &'static str {
    match date.weekday() {
        Weekday::Mon => "Monday",
        Weekday::Tue => "Tuesday",
        Weekday::Wed => "Wednesday",
        Weekday::Thu => "Thursday",
        Weekday::Fri => "Friday",
        Weekday::Sat => "Saturday",
        Weekday::Sun => "Sunday",
    }
}

/// Short and full month labels.
///
/// Spelled out here rather than taken from `chrono`'s `%b`, because these are
/// keys the translation file has to be able to name — and there are exactly
/// twelve of them, forever. A match rather than an array, so there is no index
/// to be out of range and no arm that cannot be reached.
fn month_labels(date: NaiveDate) -> (&'static str, &'static str) {
    match date.month() {
        1 => ("Jan", "January"),
        2 => ("Feb", "February"),
        3 => ("Mar", "March"),
        4 => ("Apr", "April"),
        5 => ("May", "May"),
        6 => ("Jun", "June"),
        7 => ("Jul", "July"),
        8 => ("Aug", "August"),
        9 => ("Sep", "September"),
        10 => ("Oct", "October"),
        11 => ("Nov", "November"),
        _ => ("Dec", "December"),
    }
}

/// Short month label for a column subheading.
pub fn month_abbrev(date: NaiveDate) -> &'static str {
    month_labels(date).0
}

/// Full month name, for the day panel's heading.
pub fn month_name(date: NaiveDate) -> &'static str {
    month_labels(date).1
}

/// Every label a date can render as. The translation test walks this, so a
/// thirteenth month cannot appear untranslated.
#[cfg(test)]
pub fn all_date_words() -> Vec<&'static str> {
    let mut words = Vec::new();
    for month in 1..=12 {
        let first = NaiveDate::from_ymd_opt(2026, month, 1).expect("the first of a real month");
        let (abbrev, name) = month_labels(first);
        words.push(abbrev);
        words.push(name);
    }
    // Any Monday will do; a week from it names all seven days.
    let monday = NaiveDate::from_ymd_opt(2026, 8, 31).expect("a real Monday");
    for date in weeks_from(monday, 1) {
        words.push(weekday_label(date));
        words.push(weekday_name(date));
    }
    words
}

pub fn today() -> NaiveDate {
    chrono::Utc::now().with_timezone(&timezone()).date_naive()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn d(s: &str) -> NaiveDate {
        parse(s).unwrap()
    }

    #[test]
    fn monday_of_is_idempotent_and_snaps_backwards() {
        // 2026-09-03 is a Thursday.
        assert_eq!(monday_of(d("2026-09-03")), d("2026-08-31"));
        assert_eq!(monday_of(d("2026-08-31")), d("2026-08-31"));
        // Sunday belongs to the week that started six days earlier.
        assert_eq!(monday_of(d("2026-09-06")), d("2026-08-31"));
    }

    #[test]
    fn week_spans_a_year_boundary() {
        let week = week_of(monday_of(d("2027-01-01")));
        assert_eq!(week.len(), 7);
        assert_eq!(week[0], d("2026-12-28"));
        assert_eq!(week[6], d("2027-01-03"));
    }

    /// The whole reason for four weeks instead of a calendar month: a month
    /// grid is 35 cells some months and 42 others, so rows change height as you
    /// page through it. This one never does.
    #[test]
    fn a_four_week_grid_is_always_the_same_shape() {
        for start in ["2026-08-31", "2026-11-30", "2027-01-25", "2028-02-28"] {
            let grid = weeks_from(d(start), VIEW_WEEKS);
            assert_eq!(grid.len(), 28, "{start} should give 4 x 7");
            assert_eq!(grid[0], d(start));
            assert_eq!(grid[0].weekday(), Weekday::Mon);
            assert_eq!(grid[27], d(start) + Duration::days(27));
        }
    }

    #[test]
    fn a_four_week_grid_is_contiguous_and_crosses_years() {
        let grid = weeks_from(d("2026-12-28"), VIEW_WEEKS);
        assert_eq!(grid.len(), 28);
        assert_eq!(grid[6], d("2027-01-03"), "runs straight through new year");
        for pair in grid.windows(2) {
            assert_eq!(pair[1] - pair[0], Duration::days(1), "no gaps");
        }
    }

    #[test]
    fn a_four_week_grid_covers_a_leap_day() {
        let grid = weeks_from(d("2028-02-07"), VIEW_WEEKS);
        assert!(grid.contains(&d("2028-02-29")));
    }

    #[test]
    fn every_row_of_the_grid_starts_on_a_monday() {
        let grid = weeks_from(d("2026-08-31"), VIEW_WEEKS);
        for row in 0..VIEW_WEEKS {
            assert_eq!(grid[row * 7].weekday(), Weekday::Mon);
            assert_eq!(grid[row * 7 + 6].weekday(), Weekday::Sun);
        }
    }

    #[test]
    fn week_of_is_the_first_week_of_the_grid() {
        let monday = d("2026-08-31");
        assert_eq!(week_of(monday), weeks_from(monday, 1));
        assert_eq!(week_of(monday), weeks_from(monday, VIEW_WEEKS)[..7]);
    }

    #[test]
    fn month_labels_line_up_with_the_month() {
        // Every month of a year, and every day of one, so a mislabelled arm
        // cannot hide behind the first of the month.
        let mut seen = Vec::new();
        let mut date = d("2026-01-01");
        while date.year() == 2026 {
            let (abbrev, name) = month_labels(date);
            assert!(
                name.starts_with(abbrev),
                "{name} should start with {abbrev}"
            );
            if !seen.contains(&(abbrev, name)) {
                seen.push((abbrev, name));
            }
            date = plus_days(date, 1);
        }
        assert_eq!(seen.len(), 12, "twelve distinct months, in order");
        assert_eq!(seen.first(), Some(&("Jan", "January")));
        assert_eq!(seen.last(), Some(&("Dec", "December")));
        assert_eq!(month_abbrev(d("2026-09-04")), "Sep");
        assert_eq!(month_name(d("2026-12-31")), "December");
    }

    #[test]
    fn stepping_stops_at_the_ends_of_the_calendar_rather_than_panicking() {
        let last = NaiveDate::MAX;
        assert_eq!(plus_days(last, 1), last);
        assert_eq!(minus_days(NaiveDate::MIN, 1), NaiveDate::MIN);
        assert_eq!(monday_of(NaiveDate::MIN), NaiveDate::MIN);
        // And an ordinary date still steps.
        assert_eq!(plus_days(d("2026-09-04"), 1), d("2026-09-05"));
        assert_eq!(minus_days(d("2026-09-04"), 4), d("2026-08-31"));
    }

    #[test]
    fn weekday_labels_agree_with_their_full_names() {
        let monday = d("2026-08-31");
        assert_eq!(weekday_label(monday), "Mon");
        assert_eq!(weekday_name(monday), "Monday");
        assert_eq!(weekday_name(monday + Duration::days(6)), "Sunday");
    }

    /// 12 abbreviations + 12 names + 7 short days + 7 long days.
    #[test]
    fn every_date_word_is_listed_for_the_translators() {
        let words = all_date_words();
        assert_eq!(words.len(), 38);
        for expected in ["Jan", "December", "Mon", "Sunday"] {
            assert!(words.contains(&expected), "{expected} should be listed");
        }
    }

    #[test]
    fn dates_round_trip_through_their_url_form() {
        for raw in ["2026-01-01", "2026-09-03", "2028-02-29"] {
            assert_eq!(fmt(parse(raw).unwrap()), raw);
        }
    }

    /// The unconfigured case is the one that ships, so it is worth asserting
    /// rather than assuming: no `PLANNER_TZ` must still mean Berlin, not UTC
    /// and not whatever the host happens to think.
    #[test]
    fn an_unconfigured_app_keeps_berlin_time() {
        assert_eq!(timezone(), chrono_tz::Europe::Berlin);
        assert_eq!(
            today(),
            chrono::Utc::now().with_timezone(&timezone()).date_naive()
        );
    }

    #[test]
    fn timezone_names_are_checked_before_they_are_stored() {
        assert!(zone("Europe/Berlin").is_ok());
        assert!(zone("UTC").is_ok());
        // Whitespace from a compose file's quoting should not be fatal.
        assert!(zone(" Europe/Berlin ").is_ok());
        // A misconfigured zone has to be loud: silently falling back to UTC
        // would put tasks on the wrong day only late in the evening.
        for raw in ["", "Europe/Berlim", "CEST", "+02:00"] {
            assert!(zone(raw).is_err(), "{raw} should not be accepted");
        }
    }

    #[test]
    fn nonsense_dates_are_rejected() {
        for raw in [
            "",
            "2026-13-01",
            "2026-02-30",
            "2028-02-30",
            "today",
            "2026-09",
        ] {
            assert!(parse(raw).is_none(), "{raw} should not parse");
        }
    }

    /// Unpadded numbers are accepted, which is harmless: a hand-typed URL still
    /// works and `fmt` only ever emits the padded form, so nothing downstream
    /// sees two spellings of the same day.
    #[test]
    fn an_unpadded_date_is_accepted_and_normalised() {
        assert_eq!(parse("2026-9-3"), parse("2026-09-03"));
        assert_eq!(fmt(parse("2026-9-3").unwrap()), "2026-09-03");
    }
}
