//! Date-range maths. A view is a date range plus a layout (D18), and this is
//! the range half — the only place that knows how many days a view holds.

use chrono::{Datelike, Duration, NaiveDate, Weekday};

/// Monday of the week containing `date`. Weeks start Monday (v1, not configurable).
pub fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
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
pub const VIEW_WEEKS: i64 = 4;

/// `weeks` consecutive weeks of dates starting at `monday`.
pub fn weeks_from(monday: NaiveDate, weeks: i64) -> Vec<NaiveDate> {
    (0..weeks * 7).map(|i| monday + Duration::days(i)).collect()
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

pub fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        for row in 0..usize::try_from(VIEW_WEEKS).unwrap() {
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
    fn dates_round_trip_through_their_url_form() {
        for raw in ["2026-01-01", "2026-09-03", "2028-02-29"] {
            assert_eq!(fmt(parse(raw).unwrap()), raw);
        }
    }

    #[test]
    fn nonsense_dates_are_rejected() {
        for raw in ["", "2026-13-01", "2026-02-30", "2028-02-30", "today", "2026-09"] {
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
