//! Turning rows into what a template renders.
//!
//! ## Column addressing
//!
//! The plan wrote the fragment route as `/b/:board/list/:id`, but day-lists are
//! created lazily, so an empty day has no id to be addressed or invalidated by.
//! Columns are therefore keyed by a **stable** identifier that exists before any
//! row does: a date for day columns, `list-<id>` for custom lists. This keeps
//! one fragment route for both, and means an empty day can carry an SSE trigger
//! in phase 6 exactly like a populated one.

use anyhow::{Context, Result};
use chrono::NaiveDate;
use rusqlite::Connection;
use serde::Serialize;

use crate::calendar;
use crate::i18n::{self, Locale};
use crate::models::{Task, User};
use crate::queries;

/// How much room a column has to render in.
///
/// The week grid gives a column the height of the screen; a month cell has room
/// for a handful of rows and offers to open the rest. This is the *only* thing
/// a column knows about which view it is in (D18) — the markup, and therefore
/// every behaviour attached to it, is identical either way.
pub const COMPACT: &str = "compact";
pub const FULL: &str = "full";

/// Rows a month cell shows before it starts counting the remainder.
const MONTH_CELL_ROWS: usize = 4;

fn limit_for(density: &str) -> Option<usize> {
    (density == COMPACT).then_some(MONTH_CELL_ROWS)
}

/// Density is a property of the *column*, not of the page it appears on.
///
/// Custom lists sit in their own row with room to breathe in both views (D17),
/// so only day columns follow the page. Deciding it here means a custom list
/// re-fetched from the four-week grid — or mutated from it — comes back the
/// right shape without every call site remembering the exception.
pub fn density_for(key: ColumnKey, page: &str) -> &'static str {
    if matches!(key, ColumnKey::Day(_)) && page == COMPACT {
        COMPACT
    } else {
        FULL
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKey {
    Day(NaiveDate),
    List(i64),
}

impl ColumnKey {
    pub fn parse(s: &str) -> Option<Self> {
        s.strip_prefix("list-").map_or_else(
            || calendar::parse(s).map(ColumnKey::Day),
            |id| id.parse().ok().map(ColumnKey::List),
        )
    }

    pub fn as_string(&self) -> String {
        match self {
            Self::Day(d) => calendar::fmt(*d),
            Self::List(id) => format!("list-{id}"),
        }
    }

    /// The list row this column stores into, creating a day-list if this is the
    /// first task to land on that date. `None` when a custom list is gone.
    pub fn resolve_for_write(&self, conn: &Connection, board_id: i64) -> Result<Option<i64>> {
        match self {
            Self::Day(d) => queries::ensure_day_list(conn, board_id, &calendar::fmt(*d))
                .map(Some)
                .with_context(|| format!("creating the day list for {d}")),
            Self::List(id) => Ok(queries::list(conn, *id)?
                .filter(|l| l.board_id == board_id)
                .map(|l| l.id)),
        }
    }

    /// The existing list row, if any. Never creates one — reads must not write.
    pub fn resolve_for_read(&self, conn: &Connection, board_id: i64) -> Result<Option<i64>> {
        match self {
            Self::Day(d) => {
                queries::day_lists_in_range(conn, board_id, &calendar::fmt(*d), &calendar::fmt(*d))
                    .map(|ls| ls.first().map(|l| l.id))
                    .with_context(|| format!("looking up the day list for {d}"))
            }
            Self::List(id) => Ok(queries::list(conn, *id)?
                .filter(|l| l.board_id == board_id)
                .map(|l| l.id)),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TaskView {
    pub id: i64,
    pub title: String,
    pub notes: String,
    pub done: bool,
    pub version: i64,
    pub author_name: String,
    pub colour: String,
}

#[derive(Debug, Serialize)]
pub struct ColumnView {
    pub key: String,
    /// "Mon" for a day, the list name for a custom list.
    pub heading: String,
    /// "3 Sep" for a day, empty for a custom list.
    pub subheading: String,
    pub is_day: bool,
    pub is_today: bool,
    pub list_id: Option<i64>,
    /// Carried into the markup so the column's own refresh URL round-trips it.
    pub density: &'static str,
    pub tasks: Vec<TaskView>,
    /// Tasks the cell had no room for. Always 0 where nothing is limited.
    pub hidden: usize,
}

/// "3 Sep" — but German writes "3. Sep", so the punctuation is part of the
/// translation rather than baked into a format string here.
pub fn day_label(date: chrono::NaiveDate, loc: &Locale) -> String {
    use chrono::Datelike;
    i18n::fill(
        loc.t("{day} {month}"),
        &[
            ("day", &date.day().to_string()),
            ("month", loc.t(calendar::month_abbrev(date))),
        ],
    )
}

pub fn task_view(task: &Task, authors: &[User]) -> TaskView {
    let author = authors.iter().find(|u| u.id == task.author_id);
    TaskView {
        id: task.id,
        title: task.title.clone(),
        notes: task.notes.clone(),
        done: task.done,
        version: task.version,
        author_name: author.map(|u| u.name.clone()).unwrap_or_default(),
        colour: author.map_or_else(|| "#9aa3af".into(), |u| u.colour.clone()),
    }
}

/// Builds one column. `tasks` is pre-filtered to this list.
///
/// A compact column shows a handful of rows and reports the rest as `hidden`,
/// so the template can offer to open the day in full. Deciding that here keeps
/// the templates free of slicing and arithmetic, and lets both grids share one
/// partial unchanged.
pub fn column_view(
    key: ColumnKey,
    list_id: Option<i64>,
    heading: String,
    subheading: String,
    tasks: &[Task],
    authors: &[User],
    density: &'static str,
) -> ColumnView {
    let shown = limit_for(density).map_or(tasks.len(), |n| n.min(tasks.len()));
    ColumnView {
        key: key.as_string(),
        heading,
        subheading,
        is_day: matches!(key, ColumnKey::Day(_)),
        is_today: matches!(key, ColumnKey::Day(d) if d == calendar::today()),
        list_id,
        density,
        tasks: tasks[..shown]
            .iter()
            .map(|t| task_view(t, authors))
            .collect(),
        hidden: tasks.len() - shown,
    }
}

/// Loads a single column by key — the fragment path used by every mutation
/// response and by the polled refresh.
///
/// Reads the move-completed setting from the connection it was handed rather
/// than taking it as an argument. That is not a convenience: the mutex around
/// the single connection is not reentrant, so a caller fetching the setting
/// while already inside `Db::with` deadlocks the process. Taking it here means
/// there is nothing to get wrong.
pub fn load_column(
    conn: &Connection,
    board_id: i64,
    key: ColumnKey,
    density: &'static str,
    loc: &Locale,
) -> Result<ColumnView> {
    let move_completed = queries::get_flag(conn, queries::MOVE_COMPLETED, true)
        .context("reading the move-completed setting")?;
    let authors = queries::users(conn).context("loading task authors")?;
    let list_id = key.resolve_for_read(conn, board_id)?;
    let tasks = match list_id {
        Some(id) => queries::tasks_for_list(conn, id, move_completed)?,
        None => Vec::new(),
    };

    let (heading, subheading) = match key {
        ColumnKey::Day(d) => (
            loc.t(calendar::weekday_label(d)).to_string(),
            day_label(d, loc),
        ),
        ColumnKey::List(id) => (
            queries::list(conn, id)?
                .and_then(|l| l.name)
                .unwrap_or_else(|| loc.t("List").to_string()),
            String::new(),
        ),
    };

    Ok(column_view(
        key, list_id, heading, subheading, &tasks, &authors, density,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_keys_round_trip() {
        for raw in ["2026-09-03", "list-7"] {
            let key = ColumnKey::parse(raw).expect("parses");
            assert_eq!(key.as_string(), raw);
        }
    }

    #[test]
    fn a_key_that_is_neither_a_date_nor_a_list_is_rejected() {
        for raw in ["", "nonsense", "list-", "list-abc", "2026-13-40", "2026-09"] {
            assert!(ColumnKey::parse(raw).is_none(), "{raw} should not parse");
        }
    }

    fn task(id: i64) -> Task {
        Task {
            id,
            list_id: 1,
            title: format!("task {id}"),
            notes: String::new(),
            done: false,
            author_id: 1,
            position: id,
            version: 1,
        }
    }

    fn build(count: i64, density: &'static str) -> ColumnView {
        let tasks: Vec<Task> = (0..count).map(task).collect();
        column_view(
            ColumnKey::List(1),
            Some(1),
            "L".into(),
            String::new(),
            &tasks,
            &[],
            density,
        )
    }

    #[test]
    fn a_full_column_shows_everything() {
        let col = build(9, FULL);
        assert_eq!(col.tasks.len(), 9);
        assert_eq!(col.hidden, 0);
    }

    #[test]
    fn a_compact_cell_shows_the_first_few_and_counts_the_rest() {
        let col = build(9, COMPACT);
        assert_eq!(col.tasks.len(), MONTH_CELL_ROWS);
        assert_eq!(col.hidden, 9 - MONTH_CELL_ROWS);
        assert_eq!(col.tasks[0].title, "task 0", "the visible ones come first");
    }

    #[test]
    fn a_cell_with_room_to_spare_hides_nothing() {
        let col = build(2, COMPACT);
        assert_eq!(col.tasks.len(), 2);
        assert_eq!(col.hidden, 0, "must not underflow");
    }

    #[test]
    fn an_empty_column_is_fine_at_either_density() {
        for density in [FULL, COMPACT] {
            let col = build(0, density);
            assert_eq!(col.tasks.len(), 0);
            assert_eq!(col.hidden, 0);
        }
    }

    /// A custom list is never squeezed into a calendar cell, whichever grid it
    /// is shown in (D17).
    #[test]
    fn custom_lists_are_full_density_on_every_page() {
        for page in [FULL, COMPACT] {
            assert_eq!(density_for(ColumnKey::List(1), page), FULL);
        }
    }

    #[test]
    fn day_columns_follow_the_page() {
        let day = ColumnKey::parse("2026-09-03").unwrap();
        assert_eq!(density_for(day, COMPACT), COMPACT);
        assert_eq!(density_for(day, FULL), FULL);
    }
}
