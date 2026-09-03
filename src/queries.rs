//! All SQL lives here. Functions take `&Connection` so they compose freely
//! inside or outside a transaction (`Transaction` derefs to `Connection`).

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Result};

use crate::models::{Board, List, Task, User};

fn now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

// ---------------------------------------------------------------- users

const USER_COLS: &str = "id, name, colour, theme, move_completed_to_bottom";

fn map_user(row: &rusqlite::Row<'_>) -> Result<User> {
    Ok(User {
        id: row.get(0)?,
        name: row.get(1)?,
        colour: row.get(2)?,
        theme: row.get(3)?,
        move_completed_to_bottom: row.get(4)?,
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
    move_completed: bool,
) -> Result<()> {
    conn.execute(
        "UPDATE users SET name = ?2, colour = ?3, theme = ?4, move_completed_to_bottom = ?5
         WHERE id = ?1",
        params![id, name, colour, theme, move_completed],
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
    conn.execute("UPDATE boards SET name = ?2 WHERE id = ?1", params![id, name])?;
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
    let mut stmt =
        conn.prepare("INSERT INTO board_members (board_id, user_id) VALUES (?1, ?2)")?;
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
    conn.execute("UPDATE lists SET name = ?2 WHERE id = ?1", params![id, name])?;
    Ok(())
}

pub fn delete_list(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE lists SET deleted_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;
    Ok(())
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
        conn.execute_batch(include_str!("../migrations/001_init.sql"))
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
        assert!(titles(&conn, a).is_empty());
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
            move_task(&conn, *id, if i % 2 == 0 { b } else { a }, i as i64).unwrap();
        }
        for list in [a, b] {
            let p = positions(&conn, list);
            assert_eq!(p, (0..p.len() as i64).collect::<Vec<_>>(), "list {list} drifted");
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
        assert_eq!(t.version, stale + 1, "a refused write must not bump version");
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
        assert_eq!(day_lists_in_range(&conn, board, "2026-09-01", "2026-09-30").unwrap().len(), 2);
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
    fn move_completed_to_bottom_is_a_read_time_choice() {
        let (conn, author, a, _) = fixture();
        let ids = seed(&conn, a, author, &["one", "two", "three"]);
        toggle_task(&conn, ids[0]).unwrap();

        assert_eq!(titles(&conn, a), ["one", "two", "three"], "stored order is untouched");
        let sunk: Vec<String> = tasks_for_list(&conn, a, true)
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(sunk, ["two", "three", "one"]);
    }
}
