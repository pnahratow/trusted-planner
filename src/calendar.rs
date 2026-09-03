//! Date-range maths. A view is a date range plus a layout (D18), and this is
//! the range half — the only place that knows how many days a view holds.

use chrono::{Datelike, Duration, NaiveDate, Weekday};

/// Monday of the week containing `date`. Weeks start Monday (v1, not configurable).
pub fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

/// The 7 dates of the week starting at `monday`.
pub fn week_of(monday: NaiveDate) -> Vec<NaiveDate> {
    (0..7).map(|i| monday + Duration::days(i)).collect()
}

/// The padded calendar grid for a month: whole weeks, Monday-first, including
/// the leading and trailing days of the adjacent months. 35 or 42 dates.
#[allow(dead_code)] // wired up by the month view in phase 7; already under test
pub fn month_of(year: i32, month: u32) -> Option<Vec<NaiveDate>> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let start = monday_of(first);

    // Last day of the month, then round its week up to the following Monday.
    let next_month = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)?
    };
    let last = next_month - Duration::days(1);
    let end = monday_of(last) + Duration::days(7);

    let len = (end - start).num_days();
    Some((0..len).map(|i| start + Duration::days(i)).collect())
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

    #[test]
    fn month_grid_is_whole_weeks_starting_monday() {
        let grid = month_of(2026, 9).unwrap();
        assert_eq!(grid.len() % 7, 0);
        assert_eq!(grid[0].weekday(), Weekday::Mon);
        assert!(grid.contains(&d("2026-09-01")));
        assert!(grid.contains(&d("2026-09-30")));
    }

    #[test]
    fn month_starting_sunday_needs_leading_padding() {
        // 2026-11-01 is a Sunday, so the grid opens on 2026-10-26.
        let grid = month_of(2026, 11).unwrap();
        assert_eq!(grid[0], d("2026-10-26"));
        assert!(grid.contains(&d("2026-11-30")));
    }

    #[test]
    fn february_of_a_common_year() {
        // 2026 is not a leap year: the month must end at the 28th, and the
        // grid must still pad out to whole weeks.
        let grid = month_of(2026, 2).unwrap();
        assert!(grid.contains(&d("2026-02-28")));
        assert!(grid.contains(&d("2026-03-01")));
        assert_eq!(grid.iter().filter(|x| x.month() == 2).count(), 28);
        assert_eq!(grid.len() % 7, 0);
    }

    #[test]
    fn february_of_a_leap_year_has_29_days() {
        let grid = month_of(2028, 2).unwrap();
        assert!(grid.contains(&d("2028-02-29")));
        assert_eq!(grid.iter().filter(|x| x.month() == 2).count(), 29);
    }

    #[test]
    fn month_grid_covers_every_day_of_the_month_exactly_once() {
        for (y, m) in [(2026, 1), (2026, 2), (2026, 8), (2026, 12), (2028, 2)] {
            let grid = month_of(y, m).unwrap();
            let own: Vec<_> = grid.iter().filter(|x| x.year() == y && x.month() == m).collect();
            assert_eq!(own.first().unwrap().day(), 1, "{y}-{m} misses the 1st");
            let mut sorted = own.clone();
            sorted.sort();
            assert_eq!(own, sorted, "{y}-{m} out of order");
        }
    }

    #[test]
    fn some_months_need_six_rows() {
        // 2026-08-01 is a Saturday in a 31-day month: 42 cells, not 35.
        assert_eq!(month_of(2026, 8).unwrap().len(), 42);
        assert_eq!(month_of(2026, 9).unwrap().len(), 35);
    }

    #[test]
    fn december_rolls_into_the_next_year() {
        let grid = month_of(2026, 12).unwrap();
        assert!(grid.contains(&d("2026-12-31")));
        assert_eq!(grid.len() % 7, 0);
    }
}
