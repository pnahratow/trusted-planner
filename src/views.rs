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
use crate::models::{Task, User};
use crate::queries;

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
            Self::Day(d) => queries::day_lists_in_range(
                conn,
                board_id,
                &calendar::fmt(*d),
                &calendar::fmt(*d),
            )
            .map(|ls| ls.first().map(|l| l.id))
            .with_context(|| format!("looking up the day list for {d}")),
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
    /// Dimmed in the month grid; always false in a week.
    pub is_outside: bool,
    pub list_id: Option<i64>,
    pub tasks: Vec<TaskView>,
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
pub fn column_view(
    key: ColumnKey,
    list_id: Option<i64>,
    heading: String,
    subheading: String,
    tasks: &[Task],
    authors: &[User],
) -> ColumnView {
    ColumnView {
        key: key.as_string(),
        heading,
        subheading,
        is_day: matches!(key, ColumnKey::Day(_)),
        is_today: matches!(key, ColumnKey::Day(d) if d == calendar::today()),
        is_outside: false,
        list_id,
        tasks: tasks.iter().map(|t| task_view(t, authors)).collect(),
    }
}

/// Loads a single column by key — the fragment path used by every mutation
/// response and, in phase 6, by SSE-triggered refetches.
pub fn load_column(
    conn: &Connection,
    board_id: i64,
    key: ColumnKey,
    move_completed: bool,
) -> Result<ColumnView> {
    let authors = queries::users(conn).context("loading task authors")?;
    let list_id = key.resolve_for_read(conn, board_id)?;
    let tasks = match list_id {
        Some(id) => queries::tasks_for_list(conn, id, move_completed)?,
        None => Vec::new(),
    };

    let (heading, subheading) = match key {
        ColumnKey::Day(d) => (
            calendar::weekday_label(d).to_string(),
            d.format("%-d %b").to_string(),
        ),
        ColumnKey::List(id) => (
            queries::list(conn, id)?
                .and_then(|l| l.name)
                .unwrap_or_else(|| "List".into()),
            String::new(),
        ),
    };

    Ok(column_view(key, list_id, heading, subheading, &tasks, &authors))
}
