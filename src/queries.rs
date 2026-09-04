//! All SQL lives here. Functions take `&Connection` so they compose freely
//! inside or outside a transaction (`Transaction` derefs to `Connection`).

use std::collections::HashMap;

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, Result, params};

use crate::models::{Board, List, Task, User};

fn now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

// --------------------------------------------------------- app settings

/// App-wide display settings, stored as strings so adding one later needs no
/// migration of shape — only a new key.
pub const MOVE_COMPLETED: &str = "move_completed_to_bottom";

/// What becomes of an undone task once its day has gone by (D23).
pub const OVERDUE_ACTION: &str = "overdue_action";

/// Leave it on the day it was written for.
pub const OVERDUE_LEAVE: &str = "leave";
/// Bring it forward to today, and again each day it stays undone.
pub const OVERDUE_TODAY: &str = "today";
/// Move it off the calendar into a list, where it stops chasing you.
pub const OVERDUE_LIST: &str = "list";

/// Remembers which list a board sweeps overdue tasks into. Stored per board
/// because lists belong to boards, even though the *choice* of what to do is
/// app-wide.
fn overdue_list_key(board_id: i64) -> String {
    format!("overdue_list:{board_id}")
}

/// The name a swept-into list is created with. Renaming it later is fine — the
/// board remembers the list by id, not by name.
const OVERDUE_LIST_NAME: &str = "Unfinished";

pub fn get_setting(conn: &Connection, key: &str, default: &str) -> Result<String> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.unwrap_or_else(|| default.to_owned()))
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_flag(conn: &Connection, key: &str, default: bool) -> Result<bool> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.map_or(default, |v| v == "1"))
}

pub fn set_flag(conn: &Connection, key: &str, value: bool) -> Result<()> {
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, if value { "1" } else { "0" }],
    )?;
    Ok(())
}

// ---------------------------------------------------------------- users

const USER_COLS: &str = "id, name, colour, theme";

fn map_user(row: &rusqlite::Row<'_>) -> Result<User> {
    Ok(User {
        id: row.get(0)?,
        name: row.get(1)?,
        colour: row.get(2)?,
        theme: row.get(3)?,
    })
}

pub fn users(conn: &Connection) -> Result<Vec<User>> {
    let sql = format!("SELECT {USER_COLS} FROM users WHERE deleted_at IS NULL ORDER BY name");
    conn.prepare(&sql)?.query_map([], map_user)?.collect()
}

pub fn user(conn: &Connection, id: i64) -> Result<Option<User>> {
    let sql = format!("SELECT {USER_COLS} FROM users WHERE id = ?1 AND deleted_at IS NULL");
    conn.query_row(&sql, params![id], map_user).optional()
}

pub fn create_user(conn: &Connection, name: &str, colour: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO users (name, colour, created_at) VALUES (?1, ?2, ?3)",
        params![name, colour, now()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_user(
    conn: &Connection,
    id: i64,
    name: &str,
    colour: &str,
    theme: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE users SET name = ?2, colour = ?3, theme = ?4 WHERE id = ?1",
        params![id, name, colour, theme],
    )?;
    Ok(())
}

/// Soft delete: tasks they authored stay put and keep rendering in their colour.
pub fn delete_user(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE users SET deleted_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;
    Ok(())
}

// --------------------------------------------------------------- boards

fn map_board(row: &rusqlite::Row<'_>) -> Result<Board> {
    Ok(Board {
        id: row.get(0)?,
        name: row.get(1)?,
    })
}

pub fn boards(conn: &Connection) -> Result<Vec<Board>> {
    conn.prepare("SELECT id, name FROM boards WHERE deleted_at IS NULL ORDER BY name")?
        .query_map([], map_board)?
        .collect()
}

pub fn board(conn: &Connection, id: i64) -> Result<Option<Board>> {
    conn.query_row(
        "SELECT id, name FROM boards WHERE id = ?1 AND deleted_at IS NULL",
        params![id],
        map_board,
    )
    .optional()
}

pub fn boards_for_user(conn: &Connection, user_id: i64) -> Result<Vec<Board>> {
    conn.prepare(
        "SELECT b.id, b.name FROM boards b
         JOIN board_members m ON m.board_id = b.id
         WHERE m.user_id = ?1 AND b.deleted_at IS NULL
         ORDER BY b.name",
    )?
    .query_map(params![user_id], map_board)?
    .collect()
}

pub fn create_board(conn: &Connection, name: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO boards (name, created_at) VALUES (?1, ?2)",
        params![name, now()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn rename_board(conn: &Connection, id: i64, name: &str) -> Result<()> {
    conn.execute(
        "UPDATE boards SET name = ?2 WHERE id = ?1",
        params![id, name],
    )?;
    Ok(())
}

pub fn delete_board(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE boards SET deleted_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;
    Ok(())
}

pub fn board_member_ids(conn: &Connection, board_id: i64) -> Result<Vec<i64>> {
    conn.prepare("SELECT user_id FROM board_members WHERE board_id = ?1")?
        .query_map(params![board_id], |r| r.get(0))?
        .collect()
}

/// Replaces the whole member set — the settings form posts every checkbox, so
/// a diff would be more code for the same result.
pub fn set_board_members(conn: &Connection, board_id: i64, user_ids: &[i64]) -> Result<()> {
    conn.execute(
        "DELETE FROM board_members WHERE board_id = ?1",
        params![board_id],
    )?;
    let mut stmt = conn.prepare("INSERT INTO board_members (board_id, user_id) VALUES (?1, ?2)")?;
    for uid in user_ids {
        stmt.execute(params![board_id, uid])?;
    }
    Ok(())
}

pub fn board_member_count(conn: &Connection, board_id: i64) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM board_members WHERE board_id = ?1",
        params![board_id],
        |r| r.get(0),
    )
}

// ----------------------------------------------------------- board views

/// The two grids, spelled the way they appear in a URL so the stored value is
/// also the path segment.
pub const VIEW_WEEK: &str = "w";
pub const VIEW_FOUR_WEEKS: &str = "4w";

/// Which grid this person last read each board in.
///
/// One query for every board, because the board picker needs them all: each
/// entry in it opens its board in the view that board is read in.
pub fn remembered_views(conn: &Connection, user_id: i64) -> Result<HashMap<i64, String>> {
    let mut stmt = conn.prepare("SELECT board_id, view FROM board_views WHERE user_id = ?1")?;
    let rows = stmt.query_map([user_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

pub fn remembered_view(conn: &Connection, user_id: i64, board_id: i64) -> Result<Option<String>> {
    conn.query_row(
        "SELECT view FROM board_views WHERE user_id = ?1 AND board_id = ?2",
        (user_id, board_id),
        |r| r.get(0),
    )
    .optional()
}

pub fn remember_view(conn: &Connection, user_id: i64, board_id: i64, view: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO board_views (user_id, board_id, view) VALUES (?1, ?2, ?3)
         ON CONFLICT (user_id, board_id) DO UPDATE SET view = excluded.view",
        (user_id, board_id, view),
    )?;
    Ok(())
}

// ---------------------------------------------------------------- lists

const LIST_COLS: &str = "id, board_id, name, date, position";

fn map_list(row: &rusqlite::Row<'_>) -> Result<List> {
    Ok(List {
        id: row.get(0)?,
        board_id: row.get(1)?,
        name: row.get(2)?,
        date: row.get(3)?,
        position: row.get(4)?,
    })
}

pub fn list(conn: &Connection, id: i64) -> Result<Option<List>> {
    let sql = format!("SELECT {LIST_COLS} FROM lists WHERE id = ?1 AND deleted_at IS NULL");
    conn.query_row(&sql, params![id], map_list).optional()
}

/// Day-lists that exist in `[from, to]`. Days with no tasks have no row yet and
/// are rendered from the calendar, not from here.
pub fn day_lists_in_range(
    conn: &Connection,
    board_id: i64,
    from: &str,
    to: &str,
) -> Result<Vec<List>> {
    let sql = format!(
        "SELECT {LIST_COLS} FROM lists
         WHERE board_id = ?1 AND deleted_at IS NULL
           AND date IS NOT NULL AND date BETWEEN ?2 AND ?3
         ORDER BY date"
    );
    conn.prepare(&sql)?
        .query_map(params![board_id, from, to], map_list)?
        .collect()
}

pub fn custom_lists(conn: &Connection, board_id: i64) -> Result<Vec<List>> {
    let sql = format!(
        "SELECT {LIST_COLS} FROM lists
         WHERE board_id = ?1 AND deleted_at IS NULL AND date IS NULL
         ORDER BY position, id"
    );
    conn.prepare(&sql)?
        .query_map(params![board_id], map_list)?
        .collect()
}

/// Day-lists are created lazily — the first task to land on a date makes the row.
pub fn ensure_day_list(conn: &Connection, board_id: i64, date: &str) -> Result<i64> {
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM lists
             WHERE board_id = ?1 AND date = ?2 AND deleted_at IS NULL",
            params![board_id, date],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO lists (board_id, name, date, position, created_at)
         VALUES (?1, NULL, ?2, 0, ?3)",
        params![board_id, date, now()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn create_custom_list(conn: &Connection, board_id: i64, name: &str) -> Result<i64> {
    let next: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM lists
         WHERE board_id = ?1 AND date IS NULL AND deleted_at IS NULL",
        params![board_id],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO lists (board_id, name, date, position, created_at)
         VALUES (?1, ?2, NULL, ?3, ?4)",
        params![board_id, name, next, now()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn rename_list(conn: &Connection, id: i64, name: &str) -> Result<()> {
    conn.execute(
        "UPDATE lists SET name = ?2 WHERE id = ?1",
        params![id, name],
    )?;
    Ok(())
}

pub fn delete_list(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE lists SET deleted_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;
    Ok(())
}

// -------------------------------------------------------------- overdue

/// The board's list for swept-up tasks, created the first time it is needed.
///
/// Tracked by id, so renaming it keeps working; if it has been deleted, a fresh
/// one is made rather than resurrecting a tombstone.
pub fn ensure_overdue_list(conn: &Connection, board_id: i64) -> Result<i64> {
    let key = overdue_list_key(board_id);
    if let Ok(id) = get_setting(conn, &key, "")?.parse::<i64>()
        && let Some(l) = list(conn, id)?
        && l.board_id == board_id
        && l.date.is_none()
    {
        return Ok(id);
    }
    let id = create_custom_list(conn, board_id, OVERDUE_LIST_NAME)?;
    set_setting(conn, &key, &id.to_string())?;
    Ok(id)
}

/// Undone tasks sitting on days that have already gone by, oldest first.
pub fn overdue_tasks(conn: &Connection, board_id: i64, today: &str) -> Result<Vec<Task>> {
    let sql = format!(
        "SELECT {} FROM tasks t
         JOIN lists l ON l.id = t.list_id
         WHERE l.board_id = ?1 AND l.deleted_at IS NULL
           AND l.date IS NOT NULL AND l.date < ?2
           AND t.deleted_at IS NULL AND t.done = 0
         ORDER BY l.date, t.position",
        TASK_COLS
            .split(", ")
            .map(|c| format!("t.{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    conn.prepare(&sql)?
        .query_map(params![board_id, today], map_task)?
        .collect()
}

/// Applies the overdue rule to a board. Returns the lists that changed, so the
/// caller can tell other browsers which columns to re-fetch.
///
/// Idempotent by construction: a task moved to today is no longer in the past,
/// and one moved into a list has no date at all, so neither is picked up again
/// by the same sweep. Call inside a transaction.
pub fn sweep_overdue(
    conn: &Connection,
    board_id: i64,
    today: &str,
    action: &str,
) -> Result<Vec<i64>> {
    if action != OVERDUE_TODAY && action != OVERDUE_LIST {
        return Ok(Vec::new());
    }
    let stale = overdue_tasks(conn, board_id, today)?;
    if stale.is_empty() {
        return Ok(Vec::new());
    }

    let dest = if action == OVERDUE_TODAY {
        ensure_day_list(conn, board_id, today)?
    } else {
        ensure_overdue_list(conn, board_id)?
    };

    let mut touched = vec![dest];
    for task in stale {
        if task.list_id == dest {
            continue;
        }
        if !touched.contains(&task.list_id) {
            touched.push(task.list_id);
        }
        // Append, keeping the oldest at the top of the run.
        move_task(conn, task.id, dest, i64::MAX)?;
    }
    Ok(touched)
}

// ---------------------------------------------------------------- tasks

const TASK_COLS: &str = "id, list_id, title, notes, done, author_id, position, version";

fn map_task(row: &rusqlite::Row<'_>) -> Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        list_id: row.get(1)?,
        title: row.get(2)?,
        notes: row.get(3)?,
        done: row.get(4)?,
        author_id: row.get(5)?,
        position: row.get(6)?,
        version: row.get(7)?,
    })
}

/// `move_completed` is a per-user preference, so ordering is decided at read
/// time rather than being baked into stored positions.
pub fn tasks_for_list(conn: &Connection, list_id: i64, move_completed: bool) -> Result<Vec<Task>> {
    let order = if move_completed {
        "done ASC, position ASC, id ASC"
    } else {
        "position ASC, id ASC"
    };
    let sql = format!(
        "SELECT {TASK_COLS} FROM tasks
         WHERE list_id = ?1 AND deleted_at IS NULL
         ORDER BY {order}"
    );
    conn.prepare(&sql)?
        .query_map(params![list_id], map_task)?
        .collect()
}

/// One query for every column on screen, rather than one per column.
pub fn tasks_for_lists(
    conn: &Connection,
    list_ids: &[i64],
    move_completed: bool,
) -> Result<Vec<Task>> {
    if list_ids.is_empty() {
        return Ok(Vec::new());
    }
    let order = if move_completed {
        "done ASC, position ASC, id ASC"
    } else {
        "position ASC, id ASC"
    };
    let placeholders = vec!["?"; list_ids.len()].join(",");
    let sql = format!(
        "SELECT {TASK_COLS} FROM tasks
         WHERE deleted_at IS NULL AND list_id IN ({placeholders})
         ORDER BY list_id, {order}"
    );
    let refs: Vec<&dyn rusqlite::ToSql> =
        list_ids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
    conn.prepare(&sql)?
        .query_map(refs.as_slice(), map_task)?
        .collect()
}

pub fn task(conn: &Connection, id: i64) -> Result<Option<Task>> {
    let sql = format!("SELECT {TASK_COLS} FROM tasks WHERE id = ?1 AND deleted_at IS NULL");
    conn.query_row(&sql, params![id], map_task).optional()
}

/// Appends to the end of the list.
pub fn create_task(conn: &Connection, list_id: i64, title: &str, author_id: i64) -> Result<i64> {
    let next: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM tasks
         WHERE list_id = ?1 AND deleted_at IS NULL",
        params![list_id],
        |r| r.get(0),
    )?;
    let ts = now();
    conn.execute(
        "INSERT INTO tasks (list_id, title, notes, done, author_id, position, version,
                            created_at, updated_at)
         VALUES (?1, ?2, '', 0, ?3, ?4, 1, ?5, ?5)",
        params![list_id, title, author_id, next, ts],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Structural op — totally ordered by the server, so it cannot conflict and
/// deliberately does not take a version.
pub fn toggle_task(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET done = 1 - done, version = version + 1, updated_at = ?2
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id, now()],
    )?;
    Ok(())
}

/// Compare-and-swap (D12). `Ok(false)` means the row moved under the editor and
/// the write was refused — never merged.
pub fn update_task_cas(
    conn: &Connection,
    id: i64,
    title: &str,
    notes: &str,
    version: i64,
) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE tasks SET title = ?2, notes = ?3, version = version + 1, updated_at = ?4
         WHERE id = ?1 AND version = ?5 AND deleted_at IS NULL",
        params![id, title, notes, now(), version],
    )?;
    Ok(changed == 1)
}

/// A task including a deleted one, which is what undo needs to look at.
pub fn task_any(conn: &Connection, id: i64) -> Result<Option<Task>> {
    let sql = format!("SELECT {TASK_COLS} FROM tasks WHERE id = ?1");
    conn.query_row(&sql, params![id], map_task).optional()
}

/// Puts a soft-deleted task back where it was.
///
/// Deleting decremented everything below it while leaving its own position
/// untouched, so re-opening that gap and clearing the tombstone restores the
/// exact order — no "where did it go" surprise. Call inside a transaction.
pub fn restore_task(conn: &Connection, id: i64) -> Result<()> {
    let Some(t) = task_any(conn, id)? else {
        return Ok(());
    };
    conn.execute(
        "UPDATE tasks SET position = position + 1
         WHERE list_id = ?1 AND deleted_at IS NULL AND position >= ?2",
        params![t.list_id, t.position],
    )?;
    conn.execute(
        "UPDATE tasks SET deleted_at = NULL, updated_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;
    Ok(())
}

/// Soft delete, then close the gap it leaves. Without the renumbering, a
/// deleted row's position stays reserved forever and the list stops being a
/// dense 0..n run — which is the invariant `move_task` clamps against.
/// Call inside a transaction.
pub fn delete_task(conn: &Connection, id: i64) -> Result<()> {
    let Some(t) = task(conn, id)? else {
        return Ok(());
    };
    conn.execute(
        "UPDATE tasks SET deleted_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;
    conn.execute(
        "UPDATE tasks SET position = position - 1
         WHERE list_id = ?1 AND deleted_at IS NULL AND position > ?2",
        params![t.list_id, t.position],
    )?;
    Ok(())
}

/// Task ids in a list, in stored order, optionally omitting one.
pub fn task_ids_in_list(conn: &Connection, list_id: i64, exclude: Option<i64>) -> Result<Vec<i64>> {
    let skip = exclude.unwrap_or(-1);
    conn.prepare(
        "SELECT id FROM tasks
         WHERE list_id = ?1 AND deleted_at IS NULL AND id != ?2
         ORDER BY position, id",
    )?
    .query_map(params![list_id, skip], |r| r.get(0))?
    .collect()
}

/// Translates the drag intent "put `moved` immediately after `after`" into the
/// index `move_task` expects.
///
/// A drop cannot send a raw index: with completed tasks sunk to the bottom the
/// order on screen is not the order in the table, so index 2 on screen may be
/// any row underneath. Naming the neighbour is unambiguous either way. `None`
/// means the head of the list.
pub fn position_after(
    conn: &Connection,
    dest_list: i64,
    moved: i64,
    after: Option<i64>,
) -> Result<i64> {
    let Some(after) = after else {
        return Ok(0);
    };
    let ids = task_ids_in_list(conn, dest_list, Some(moved))?;
    let target = ids.iter().position(|id| *id == after).map_or_else(
        // The neighbour is gone (deleted or moved by someone else); appending
        // is closer to the intent than silently landing at the top.
        || ids.len(),
        |i| i + 1,
    );
    Ok(i64::try_from(target).unwrap_or(i64::MAX))
}

/// Moves a task to `position` in `dest_list`, renumbering both the list it left
/// and the list it joined so positions stay dense integers. Call inside a
/// transaction — the caller owns atomicity.
pub fn move_task(conn: &Connection, id: i64, dest_list: i64, position: i64) -> Result<()> {
    let Some(t) = task(conn, id)? else {
        return Ok(());
    };

    // Close the gap in the source list first, so a same-list move sees indices
    // as if the task were already gone.
    conn.execute(
        "UPDATE tasks SET position = position - 1
         WHERE list_id = ?1 AND deleted_at IS NULL AND position > ?2",
        params![t.list_id, t.position],
    )?;

    let len: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks
         WHERE list_id = ?1 AND deleted_at IS NULL AND id != ?2",
        params![dest_list, id],
        |r| r.get(0),
    )?;
    let target = position.clamp(0, len);

    conn.execute(
        "UPDATE tasks SET position = position + 1
         WHERE list_id = ?1 AND deleted_at IS NULL AND position >= ?2 AND id != ?3",
        params![dest_list, target, id],
    )?;
    conn.execute(
        "UPDATE tasks SET list_id = ?2, position = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, dest_list, target, now()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A board with two lists and a known author, in memory.
    fn fixture() -> (Connection, i64, i64, i64) {
        let conn = Connection::open_in_memory().unwrap();
        // Every migration, so tests see the same schema production does.
        conn.execute_batch(include_str!("../migrations/001_init.sql"))
            .unwrap();
        conn.execute_batch(include_str!("../migrations/002_global_settings.sql"))
            .unwrap();
        conn.execute_batch(include_str!("../migrations/003_board_views.sql"))
            .unwrap();
        let author = create_user(&conn, "Tester", "#3563e9").unwrap();
        let board = create_board(&conn, "Board").unwrap();
        let a = create_custom_list(&conn, board, "A").unwrap();
        let b = create_custom_list(&conn, board, "B").unwrap();
        (conn, author, a, b)
    }

    fn titles(conn: &Connection, list: i64) -> Vec<String> {
        tasks_for_list(conn, list, false)
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect()
    }

    /// Positions must stay a dense 0..n run — that is the whole reason for
    /// renumbering rather than using fractional indices.
    fn positions(conn: &Connection, list: i64) -> Vec<i64> {
        tasks_for_list(conn, list, false)
            .unwrap()
            .into_iter()
            .map(|t| t.position)
            .collect()
    }

    fn seed(conn: &Connection, list: i64, author: i64, names: &[&str]) -> Vec<i64> {
        names
            .iter()
            .map(|n| create_task(conn, list, n, author).unwrap())
            .collect()
    }

    #[test]
    fn create_task_appends_to_the_end() {
        let (conn, author, a, _) = fixture();
        seed(&conn, a, author, &["one", "two", "three"]);
        assert_eq!(titles(&conn, a), ["one", "two", "three"]);
        assert_eq!(positions(&conn, a), [0, 1, 2]);
    }

    #[test]
    fn move_within_list_to_head() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three"]);
        move_task(&conn, ids[2], a, 0).unwrap();
        assert_eq!(titles(&conn, a), ["three", "one", "two"]);
        assert_eq!(positions(&conn, a), [0, 1, 2]);
    }

    #[test]
    fn move_within_list_to_tail() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three"]);
        move_task(&conn, ids[0], a, 2).unwrap();
        assert_eq!(titles(&conn, a), ["two", "three", "one"]);
        assert_eq!(positions(&conn, a), [0, 1, 2]);
    }

    #[test]
    fn move_within_list_to_middle() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three", "four"]);
        move_task(&conn, ids[3], a, 1).unwrap();
        assert_eq!(titles(&conn, a), ["one", "four", "two", "three"]);
        assert_eq!(positions(&conn, a), [0, 1, 2, 3]);
    }

    #[test]
    fn moving_a_task_to_where_it_already_is_changes_nothing() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three"]);
        move_task(&conn, ids[1], a, 1).unwrap();
        assert_eq!(titles(&conn, a), ["one", "two", "three"]);
        assert_eq!(positions(&conn, a), [0, 1, 2]);
    }

    #[test]
    fn move_across_lists_closes_the_gap_it_leaves() {
        let (conn, author, a, b) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three"]);
        seed(&conn, b, author, &["x", "y"]);

        move_task(&conn, ids[1], b, 1).unwrap();

        assert_eq!(titles(&conn, a), ["one", "three"]);
        assert_eq!(positions(&conn, a), [0, 1], "source list must stay dense");
        assert_eq!(titles(&conn, b), ["x", "two", "y"]);
        assert_eq!(positions(&conn, b), [0, 1, 2]);
    }

    #[test]
    fn move_into_an_empty_list() {
        let (conn, author, a, b) = fixture();
        let ids = seed(&conn, a, author, &["only"]);
        move_task(&conn, ids[0], b, 0).unwrap();
        assert_eq!(titles(&conn, a), [] as [std::string::String; 0]);
        assert_eq!(titles(&conn, b), ["only"]);
        assert_eq!(positions(&conn, b), [0]);
    }

    #[test]
    fn out_of_range_position_is_clamped_not_rejected() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three"]);
        // A drag can report a stale index; clamping keeps the run dense.
        move_task(&conn, ids[0], a, 99).unwrap();
        assert_eq!(titles(&conn, a), ["two", "three", "one"]);
        assert_eq!(positions(&conn, a), [0, 1, 2]);

        move_task(&conn, ids[0], a, -5).unwrap();
        assert_eq!(titles(&conn, a), ["one", "two", "three"]);
        assert_eq!(positions(&conn, a), [0, 1, 2]);
    }

    #[test]
    fn repeated_moves_never_drift() {
        let (conn, author, a, b) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three", "four"]);
        for (i, id) in ids.iter().enumerate() {
            let target = i64::try_from(i).unwrap();
            move_task(&conn, *id, if i % 2 == 0 { b } else { a }, target).unwrap();
        }
        for list in [a, b] {
            let p = positions(&conn, list);
            let dense: Vec<i64> = (0..i64::try_from(p.len()).unwrap()).collect();
            assert_eq!(p, dense, "list {list} drifted");
        }
    }

    #[test]
    fn soft_deleted_tasks_do_not_hold_a_position() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three"]);
        delete_task(&conn, ids[0]).unwrap();
        move_task(&conn, ids[2], a, 0).unwrap();
        assert_eq!(titles(&conn, a), ["three", "two"]);
        assert_eq!(positions(&conn, a), [0, 1]);
    }

    /// Exercises every query function once against a real schema.
    ///
    /// rusqlite only parses SQL when a statement is prepared, so a typo in a
    /// rarely-hit path — renaming a list, removing a board — would otherwise
    /// stay invisible until someone triggered it in production. This is the
    /// cheap half of what a compile-time-checked query layer would buy, and it
    /// costs no build-time database.
    /// The picker opens each board in the view it is read in, so the write has
    /// to overwrite rather than accumulate, and one person's choice must not
    /// reach another's screen.
    #[test]
    fn a_remembered_view_is_per_person_and_per_board() {
        let (conn, ann, _, _) = fixture();
        let bob = create_user(&conn, "Bob", "#e0533d").unwrap();
        let planning = create_board(&conn, "Planning").unwrap();
        let shopping = create_board(&conn, "Shopping").unwrap();

        remember_view(&conn, ann, planning, VIEW_FOUR_WEEKS).unwrap();
        remember_view(&conn, ann, shopping, VIEW_WEEK).unwrap();
        remember_view(&conn, bob, planning, VIEW_WEEK).unwrap();

        let hers = remembered_views(&conn, ann).unwrap();
        assert_eq!(
            hers.get(&planning).map(String::as_str),
            Some(VIEW_FOUR_WEEKS)
        );
        assert_eq!(hers.get(&shopping).map(String::as_str), Some(VIEW_WEEK));
        assert_eq!(
            remembered_views(&conn, bob)
                .unwrap()
                .get(&planning)
                .map(String::as_str),
            Some(VIEW_WEEK),
            "Bob reads the same board his own way"
        );

        // Switching view replaces the row rather than adding one.
        remember_view(&conn, ann, planning, VIEW_WEEK).unwrap();
        assert_eq!(remembered_views(&conn, ann).unwrap().len(), 2);
        assert_eq!(
            remembered_view(&conn, ann, planning).unwrap().as_deref(),
            Some(VIEW_WEEK)
        );
    }

    #[test]
    fn every_query_prepares_and_runs() {
        let (conn, author, list_a, _list_b) = fixture();

        // users
        let u2 = create_user(&conn, "Second", "#e0533d").unwrap();
        assert_eq!(users(&conn).unwrap().len(), 2);
        assert!(user(&conn, author).unwrap().is_some());
        update_user(&conn, u2, "Renamed", "#2ea36b", "dark").unwrap();
        assert_eq!(user(&conn, u2).unwrap().unwrap().theme, "dark");
        delete_user(&conn, u2).unwrap();
        assert!(
            user(&conn, u2).unwrap().is_none(),
            "soft-deleted users stop resolving"
        );
        assert_eq!(users(&conn).unwrap().len(), 1);

        // boards and membership
        let b = create_board(&conn, "Board Two").unwrap();
        assert!(board(&conn, b).unwrap().is_some());
        assert!(boards(&conn).unwrap().len() >= 2);
        set_board_members(&conn, b, &[author]).unwrap();
        assert_eq!(board_member_ids(&conn, b).unwrap(), vec![author]);
        assert_eq!(board_member_count(&conn, b).unwrap(), 1);
        assert!(
            boards_for_user(&conn, author)
                .unwrap()
                .iter()
                .any(|x| x.id == b)
        );
        rename_board(&conn, b, "Renamed Board").unwrap();
        assert_eq!(board(&conn, b).unwrap().unwrap().name, "Renamed Board");

        // remembered views
        assert!(remembered_view(&conn, author, b).unwrap().is_none());
        remember_view(&conn, author, b, VIEW_FOUR_WEEKS).unwrap();
        assert_eq!(
            remembered_view(&conn, author, b).unwrap().as_deref(),
            Some(VIEW_FOUR_WEEKS)
        );
        assert_eq!(remembered_views(&conn, author).unwrap().len(), 1);

        // lists
        let cl = create_custom_list(&conn, b, "Shopping").unwrap();
        assert!(list(&conn, cl).unwrap().is_some());
        assert_eq!(custom_lists(&conn, b).unwrap().len(), 1);
        rename_list(&conn, cl, "Groceries").unwrap();
        assert_eq!(list(&conn, cl).unwrap().unwrap().name.unwrap(), "Groceries");

        let day = ensure_day_list(&conn, b, "2026-09-03").unwrap();
        assert_eq!(
            day_lists_in_range(&conn, b, "2026-09-01", "2026-09-30")
                .unwrap()
                .len(),
            1
        );
        assert!(
            day_lists_in_range(&conn, b, "2026-10-01", "2026-10-31")
                .unwrap()
                .is_empty()
        );

        // tasks
        let t1 = create_task(&conn, day, "first", author).unwrap();
        let t2 = create_task(&conn, cl, "second", author).unwrap();
        assert!(task(&conn, t1).unwrap().is_some());
        assert_eq!(task_ids_in_list(&conn, day, None).unwrap(), vec![t1]);
        assert_eq!(
            task_ids_in_list(&conn, day, Some(t1)).unwrap(),
            Vec::<i64>::new()
        );
        assert_eq!(tasks_for_list(&conn, cl, false).unwrap().len(), 1);
        assert_eq!(tasks_for_lists(&conn, &[day, cl], true).unwrap().len(), 2);
        assert!(
            tasks_for_lists(&conn, &[], false).unwrap().is_empty(),
            "no ids, no query"
        );

        // flags
        set_flag(&conn, MOVE_COMPLETED, false).unwrap();
        assert!(!get_flag(&conn, MOVE_COMPLETED, true).unwrap());
        set_setting(&conn, OVERDUE_ACTION, OVERDUE_TODAY).unwrap();
        assert_eq!(
            get_setting(&conn, OVERDUE_ACTION, OVERDUE_LIST).unwrap(),
            OVERDUE_TODAY
        );

        // teardown paths, which are the least-travelled SQL in the app
        delete_task(&conn, t2).unwrap();
        delete_list(&conn, cl).unwrap();
        assert_eq!(custom_lists(&conn, b).unwrap().len(), 0);
        delete_board(&conn, b).unwrap();
        assert!(board(&conn, b).unwrap().is_none());

        // the fixture's own list is still intact and queryable
        assert!(tasks_for_list(&conn, list_a, true).unwrap().is_empty());
    }

    // ------------------------------------------------------- overdue

    const TODAY: &str = "2026-09-04";

    /// Two undone tasks on past days, one done task on a past day, and one on
    /// today. Returns their ids in that order.
    fn overdue_fixture(conn: &Connection, board: i64, author: i64) -> [i64; 4] {
        let mon = ensure_day_list(conn, board, "2026-09-01").unwrap();
        let tue = ensure_day_list(conn, board, "2026-09-02").unwrap();
        let today = ensure_day_list(conn, board, TODAY).unwrap();
        let a = create_task(conn, mon, "monday leftover", author).unwrap();
        let b = create_task(conn, tue, "tuesday leftover", author).unwrap();
        let done = create_task(conn, tue, "already done", author).unwrap();
        toggle_task(conn, done).unwrap();
        let c = create_task(conn, today, "for today", author).unwrap();
        [a, b, done, c]
    }

    #[test]
    fn overdue_finds_only_undone_tasks_in_the_past() {
        let (conn, author, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        overdue_fixture(&conn, board, author);

        let stale = overdue_tasks(&conn, board, TODAY).unwrap();
        let titles: Vec<_> = stale.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            ["monday leftover", "tuesday leftover"],
            "today is not overdue, and a ticked task is not chasing anyone"
        );
    }

    #[test]
    fn leaving_them_alone_moves_nothing() {
        let (conn, author, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        let ids = overdue_fixture(&conn, board, author);

        let touched = sweep_overdue(&conn, board, TODAY, OVERDUE_LEAVE).unwrap();
        assert_eq!(touched, Vec::<i64>::new());
        assert_eq!(overdue_tasks(&conn, board, TODAY).unwrap().len(), 2);
        assert!(task(&conn, ids[0]).unwrap().is_some());
    }

    #[test]
    fn moving_to_today_empties_the_past() {
        let (conn, author, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        overdue_fixture(&conn, board, author);

        sweep_overdue(&conn, board, TODAY, OVERDUE_TODAY).unwrap();
        assert_eq!(overdue_tasks(&conn, board, TODAY).unwrap().len(), 0);

        let today = ensure_day_list(&conn, board, TODAY).unwrap();
        let titles: Vec<String> = tasks_for_list(&conn, today, false)
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(
            titles,
            ["for today", "monday leftover", "tuesday leftover"],
            "appended in date order, behind what was already planned"
        );
        assert_eq!(positions(&conn, today), [0, 1, 2]);
    }

    #[test]
    fn moving_to_a_list_takes_them_off_the_calendar() {
        let (conn, author, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        overdue_fixture(&conn, board, author);

        sweep_overdue(&conn, board, TODAY, OVERDUE_LIST).unwrap();
        assert_eq!(overdue_tasks(&conn, board, TODAY).unwrap().len(), 0);

        let dest = ensure_overdue_list(&conn, board).unwrap();
        let titles: Vec<String> = tasks_for_list(&conn, dest, false)
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(titles, ["monday leftover", "tuesday leftover"]);
        assert!(
            list(&conn, dest).unwrap().unwrap().date.is_none(),
            "off the calendar"
        );
    }

    /// The sweep runs on every page load, so running it twice must be the same
    /// as running it once.
    #[test]
    fn sweeping_twice_changes_nothing_the_second_time() {
        for action in [OVERDUE_TODAY, OVERDUE_LIST] {
            let (conn, author, _, _) = fixture();
            let board = create_board(&conn, "B").unwrap();
            overdue_fixture(&conn, board, author);

            sweep_overdue(&conn, board, TODAY, action).unwrap();
            let second = sweep_overdue(&conn, board, TODAY, action).unwrap();
            assert_eq!(second, Vec::<i64>::new(), "{action} was not idempotent");
        }
    }

    #[test]
    fn a_ticked_task_is_left_where_it_was() {
        let (conn, author, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        let ids = overdue_fixture(&conn, board, author);

        sweep_overdue(&conn, board, TODAY, OVERDUE_LIST).unwrap();
        let done = task(&conn, ids[2]).unwrap().unwrap();
        let its_list = list(&conn, done.list_id).unwrap().unwrap();
        assert_eq!(
            its_list.date.as_deref(),
            Some("2026-09-02"),
            "history stays put"
        );
    }

    #[test]
    fn the_sweep_stops_at_the_board_it_was_asked_about() {
        let (conn, author, _, _) = fixture();
        let mine = create_board(&conn, "Mine").unwrap();
        let theirs = create_board(&conn, "Theirs").unwrap();
        overdue_fixture(&conn, mine, author);
        overdue_fixture(&conn, theirs, author);

        sweep_overdue(&conn, mine, TODAY, OVERDUE_LIST).unwrap();
        assert!(overdue_tasks(&conn, mine, TODAY).unwrap().is_empty());
        assert_eq!(
            overdue_tasks(&conn, theirs, TODAY).unwrap().len(),
            2,
            "another board's tasks are not this board's business"
        );
    }

    #[test]
    fn each_board_gets_its_own_unfinished_list_once() {
        let (conn, _, _, _) = fixture();
        let one = create_board(&conn, "One").unwrap();
        let two = create_board(&conn, "Two").unwrap();

        let a = ensure_overdue_list(&conn, one).unwrap();
        assert_eq!(
            a,
            ensure_overdue_list(&conn, one).unwrap(),
            "reused, not remade"
        );
        assert_ne!(a, ensure_overdue_list(&conn, two).unwrap(), "one per board");
        assert_eq!(custom_lists(&conn, one).unwrap().len(), 1);
    }

    #[test]
    fn renaming_the_list_keeps_it_as_the_target() {
        let (conn, _, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        let id = ensure_overdue_list(&conn, board).unwrap();
        rename_list(&conn, id, "Backlog").unwrap();
        assert_eq!(
            ensure_overdue_list(&conn, board).unwrap(),
            id,
            "tracked by id"
        );
    }

    #[test]
    fn deleting_the_list_makes_a_fresh_one() {
        let (conn, _, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        let first = ensure_overdue_list(&conn, board).unwrap();
        delete_list(&conn, first).unwrap();

        let second = ensure_overdue_list(&conn, board).unwrap();
        assert_ne!(second, first, "a tombstone is not resurrected");
        assert!(list(&conn, second).unwrap().is_some());
    }

    #[test]
    fn a_board_with_nothing_overdue_is_untouched() {
        let (conn, author, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        let today = ensure_day_list(&conn, board, TODAY).unwrap();
        create_task(&conn, today, "for today", author).unwrap();

        assert_eq!(
            sweep_overdue(&conn, board, TODAY, OVERDUE_LIST).unwrap(),
            Vec::<i64>::new()
        );
        assert_eq!(
            custom_lists(&conn, board).unwrap().len(),
            0,
            "no list made for nothing"
        );
    }

    // ---------------------------------------------------------- undo

    #[test]
    fn undo_puts_a_task_back_exactly_where_it_was() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three", "four"]);

        delete_task(&conn, ids[1]).unwrap();
        assert_eq!(titles(&conn, a), ["one", "three", "four"]);
        assert_eq!(positions(&conn, a), [0, 1, 2]);

        restore_task(&conn, ids[1]).unwrap();
        assert_eq!(titles(&conn, a), ["one", "two", "three", "four"]);
        assert_eq!(positions(&conn, a), [0, 1, 2, 3]);
    }

    #[test]
    fn undoing_a_deleted_head_or_tail_also_restores_the_order() {
        for index in [0, 2] {
            let (conn, author, a, _) = fixture();
            let ids = seed(&conn, a, author, &["one", "two", "three"]);
            delete_task(&conn, ids[index]).unwrap();
            restore_task(&conn, ids[index]).unwrap();
            assert_eq!(titles(&conn, a), ["one", "two", "three"]);
            assert_eq!(positions(&conn, a), [0, 1, 2]);
        }
    }

    #[test]
    fn undo_survives_other_edits_made_in_between() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three"]);
        delete_task(&conn, ids[0]).unwrap();
        let added = create_task(&conn, a, "four", author).unwrap();

        restore_task(&conn, ids[0]).unwrap();
        assert_eq!(titles(&conn, a), ["one", "two", "three", "four"]);
        assert_eq!(positions(&conn, a), [0, 1, 2, 3]);
        assert!(task(&conn, added).unwrap().is_some());
    }

    #[test]
    fn undoing_something_that_was_never_deleted_is_harmless() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two"]);
        restore_task(&conn, 9999).unwrap(); // no such task
        assert_eq!(titles(&conn, a), ["one", "two"]);
        assert_eq!(positions(&conn, a), [0, 1]);
        assert!(task(&conn, ids[0]).unwrap().is_some());
    }

    #[test]
    fn task_any_sees_through_a_tombstone() {
        let (conn, author, a, _) = fixture();
        let id = seed(&conn, a, author, &["gone"])[0];
        delete_task(&conn, id).unwrap();
        assert!(task(&conn, id).unwrap().is_none());
        assert_eq!(task_any(&conn, id).unwrap().unwrap().title, "gone");
    }

    // ------------------------------------------- drop intent -> position

    #[test]
    fn dropping_at_the_head_means_position_zero() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three"]);
        assert_eq!(position_after(&conn, a, ids[2], None).unwrap(), 0);
    }

    #[test]
    fn dropping_after_a_neighbour_lands_just_past_it() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three", "four"]);

        // Move "four" to sit after "one". Excluding the moved task the list is
        // [one, two, three], so "one" is index 0 and the target is 1.
        let pos = position_after(&conn, a, ids[3], Some(ids[0])).unwrap();
        assert_eq!(pos, 1);
        move_task(&conn, ids[3], a, pos).unwrap();
        assert_eq!(titles(&conn, a), ["one", "four", "two", "three"]);
    }

    #[test]
    fn the_moved_task_is_excluded_when_locating_its_neighbour() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three"]);

        // Dragging "one" (index 0) to sit after "two". With "one" removed,
        // "two" is index 0, so the target is 1 — not 2, which is what using
        // the un-filtered list would have given.
        let pos = position_after(&conn, a, ids[0], Some(ids[1])).unwrap();
        assert_eq!(pos, 1);
        move_task(&conn, ids[0], a, pos).unwrap();
        assert_eq!(titles(&conn, a), ["two", "one", "three"]);
    }

    #[test]
    fn dropping_after_the_last_task_appends() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three"]);
        let pos = position_after(&conn, a, ids[0], Some(ids[2])).unwrap();
        move_task(&conn, ids[0], a, pos).unwrap();
        assert_eq!(titles(&conn, a), ["two", "three", "one"]);
        assert_eq!(positions(&conn, a), [0, 1, 2]);
    }

    #[test]
    fn dropping_into_another_list_after_a_neighbour() {
        let (conn, author, a, b) = fixture();
        let moved = seed(&conn, a, author, &["moved"])[0];
        let there = seed(&conn, b, author, &["x", "y", "z"]);

        let pos = position_after(&conn, b, moved, Some(there[1])).unwrap();
        assert_eq!(pos, 2);
        move_task(&conn, moved, b, pos).unwrap();
        assert_eq!(titles(&conn, b), ["x", "y", "moved", "z"]);
        assert_eq!(titles(&conn, a), [] as [std::string::String; 0]);
    }

    #[test]
    fn dropping_into_an_empty_list_is_position_zero() {
        let (conn, author, a, b) = fixture();
        let moved = seed(&conn, a, author, &["moved"])[0];
        assert_eq!(position_after(&conn, b, moved, None).unwrap(), 0);
    }

    /// Someone else deleted or moved the neighbour between render and drop.
    #[test]
    fn a_vanished_neighbour_appends_rather_than_jumping_to_the_top() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three"]);
        delete_task(&conn, ids[1]).unwrap();

        let pos = position_after(&conn, a, ids[0], Some(ids[1])).unwrap();
        assert_eq!(
            pos, 1,
            "appended to the two survivors, not sent to the head"
        );
    }

    #[test]
    fn a_drop_is_unambiguous_even_when_done_tasks_are_sunk() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three"]);
        toggle_task(&conn, ids[0]).unwrap(); // "one" is done, so it displays last

        // On screen the order is [two, three, one]. Dropping "three" after
        // "two" must mean the row called "two", regardless of where either
        // sits in the table.
        let pos = position_after(&conn, a, ids[2], Some(ids[1])).unwrap();
        move_task(&conn, ids[2], a, pos).unwrap();

        let shown: Vec<String> = tasks_for_list(&conn, a, true)
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(shown, ["two", "three", "one"]);
    }

    // ------------------------------------------------------------ CAS

    #[test]
    fn cas_accepts_a_write_carrying_the_current_version() {
        let (conn, author, a, _) = fixture();
        let id = seed(&conn, a, author, &["draft"])[0];
        let v = task(&conn, id).unwrap().unwrap().version;

        assert!(update_task_cas(&conn, id, "final", "notes", v).unwrap());
        let t = task(&conn, id).unwrap().unwrap();
        assert_eq!(t.title, "final");
        assert_eq!(t.notes, "notes");
        assert_eq!(t.version, v + 1, "an accepted write bumps the version");
    }

    #[test]
    fn cas_refuses_a_stale_write_and_leaves_the_row_untouched() {
        let (conn, author, a, _) = fixture();
        let id = seed(&conn, a, author, &["draft"])[0];
        let stale = task(&conn, id).unwrap().unwrap().version;

        // Someone else saves first.
        assert!(update_task_cas(&conn, id, "theirs", "", stale).unwrap());

        // The second editor replays the version they rendered from.
        assert!(!update_task_cas(&conn, id, "mine", "", stale).unwrap());

        let t = task(&conn, id).unwrap().unwrap();
        assert_eq!(t.title, "theirs", "a refused write must not be merged");
        assert_eq!(
            t.version,
            stale + 1,
            "a refused write must not bump version"
        );
    }

    #[test]
    fn toggling_bumps_the_version_so_a_pending_edit_goes_stale() {
        let (conn, author, a, _) = fixture();
        let id = seed(&conn, a, author, &["thing"])[0];
        let v = task(&conn, id).unwrap().unwrap().version;

        toggle_task(&conn, id).unwrap();
        assert!(task(&conn, id).unwrap().unwrap().done);

        // The editor opened before the toggle is now writing against the past.
        assert!(!update_task_cas(&conn, id, "renamed", "", v).unwrap());
        assert_eq!(task(&conn, id).unwrap().unwrap().title, "thing");
    }

    #[test]
    fn cas_on_a_deleted_task_is_refused() {
        let (conn, author, a, _) = fixture();
        let id = seed(&conn, a, author, &["gone"])[0];
        let v = task(&conn, id).unwrap().unwrap().version;
        delete_task(&conn, id).unwrap();
        assert!(!update_task_cas(&conn, id, "back", "", v).unwrap());
    }

    // ---------------------------------------------------- lazy day lists

    #[test]
    fn day_lists_are_created_once_and_then_reused() {
        let (conn, _, _, _) = fixture();
        let board = create_board(&conn, "Other").unwrap();
        let first = ensure_day_list(&conn, board, "2026-09-03").unwrap();
        let again = ensure_day_list(&conn, board, "2026-09-03").unwrap();
        assert_eq!(first, again, "a second task on a date must reuse the row");

        let other_day = ensure_day_list(&conn, board, "2026-09-04").unwrap();
        assert_ne!(first, other_day);
        assert_eq!(
            day_lists_in_range(&conn, board, "2026-09-01", "2026-09-30")
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn day_lists_are_scoped_per_board() {
        let (conn, _, _, _) = fixture();
        let b1 = create_board(&conn, "One").unwrap();
        let b2 = create_board(&conn, "Two").unwrap();
        assert_ne!(
            ensure_day_list(&conn, b1, "2026-09-03").unwrap(),
            ensure_day_list(&conn, b2, "2026-09-03").unwrap(),
            "the unique index is per board, not global"
        );
    }

    #[test]
    fn move_completed_defaults_on_and_round_trips() {
        let (conn, _, _, _) = fixture();
        assert!(get_flag(&conn, MOVE_COMPLETED, true).unwrap(), "seeded on");

        set_flag(&conn, MOVE_COMPLETED, false).unwrap();
        assert!(!get_flag(&conn, MOVE_COMPLETED, true).unwrap());

        set_flag(&conn, MOVE_COMPLETED, true).unwrap();
        assert!(
            get_flag(&conn, MOVE_COMPLETED, true).unwrap(),
            "upsert, not a duplicate row"
        );
    }

    #[test]
    fn an_unknown_flag_falls_back_to_its_default() {
        let (conn, _, _, _) = fixture();
        assert!(get_flag(&conn, "never_set", true).unwrap());
        assert!(!get_flag(&conn, "never_set", false).unwrap());
    }

    #[test]
    fn move_completed_to_bottom_is_a_read_time_choice() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three"]);
        toggle_task(&conn, ids[0]).unwrap();

        assert_eq!(
            titles(&conn, a),
            ["one", "two", "three"],
            "stored order is untouched"
        );
        let sunk: Vec<String> = tasks_for_list(&conn, a, true)
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(sunk, ["two", "three", "one"]);
    }
}
