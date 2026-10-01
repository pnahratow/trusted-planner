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

/// Which language the interface is written in. App-wide: a household picks a
/// language once, and the pages where it matters most — the identity picker,
/// an empty settings page — are rendered before anyone has an identity to
/// hang a preference on.
pub const LANGUAGE: &str = "language";

/// Who the app answers as when a browser carries no identity of its own.
///
/// App-wide, like the language and for the same reason: a screen on the wall
/// has nobody to ask, and the pages that would do the asking are exactly the
/// ones nobody is standing in front of.
pub const DEFAULT_USER: &str = "default_user";

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

/// The name a swept-into list is created with, in English, for the caller to
/// translate. Renaming it later is fine — the board remembers the list by id,
/// not by name — which is also why switching language does not rename one that
/// already exists.
pub const OVERDUE_LIST_NAME: &str = "Todo";

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

/// The earliest surviving user.
///
/// By `id`, not `created_at`: two people added in the same second tie on the
/// timestamp, and the rowid never does.
fn first_user(conn: &Connection) -> Result<Option<User>> {
    let sql = format!("SELECT {USER_COLS} FROM users WHERE deleted_at IS NULL ORDER BY id LIMIT 1");
    conn.query_row(&sql, [], map_user).optional()
}

/// Who a browser with no identity of its own is answered as.
///
/// Total by construction rather than by repair. The stored id is only a
/// preference: if it names nobody — never set, or set to someone since removed
/// — the first person created is the answer. So there is no state in which a
/// populated app has no default, nothing to fix up when a user is deleted, and
/// no stale id to sweep. `None` means one thing only: there are no users yet.
pub fn default_user(conn: &Connection) -> Result<Option<User>> {
    let stored = get_setting(conn, DEFAULT_USER, "")?;
    if let Ok(id) = stored.parse::<i64>()
        && let Some(user) = user(conn, id)?
    {
        return Ok(Some(user));
    }
    first_user(conn)
}

/// Pin the default to a particular person.
///
/// Silently ignores an id that names nobody, which keeps the stored value
/// meaningful; a caller that got it wrong would otherwise be invisible until
/// someone wondered why the setting had no effect.
pub fn set_default_user(conn: &Connection, id: i64) -> Result<()> {
    if user(conn, id)?.is_some() {
        set_setting(conn, DEFAULT_USER, &id.to_string())?;
    }
    Ok(())
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

/// Whether this person is on this board's member list. The board picker and
/// the identity switch both ask, because a board you are not a member of is
/// meant to be out of your way (D7/D8) — not out of your reach.
pub fn is_board_member(conn: &Connection, board_id: i64, user_id: i64) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM board_members WHERE board_id = ?1 AND user_id = ?2",
        (board_id, user_id),
        |r| r.get(0),
    )?;
    Ok(count > 0)
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

/// Removes a custom list, moving anything still in it to the board's overdue
/// list first. Call inside a transaction.
///
/// Without the move the tasks keep `deleted_at IS NULL` while the list that
/// reaches them does not: live rows that no screen can select and `restore`
/// refuses, because it looks the list up with `deleted_at IS NULL` too. A
/// container disappearing must not take entries with it silently — and
/// `stranded_tasks` is the alarm for the day this stops being true.
///
/// A dated list is refused outright. Only custom lists are offered for
/// deletion, so a day arriving here is a stale or hand-made request, and
/// emptying a day of its tasks is not what it asked for.
pub fn delete_list(conn: &Connection, id: i64, overdue_list_name: &str) -> Result<()> {
    let Some(list) = list(conn, id)? else {
        return Ok(());
    };
    if list.date.is_some() {
        return Ok(());
    }

    let stays = tasks_for_list(conn, id, false)?;

    // The tombstone goes down *before* the destination is worked out, because
    // the list being removed can be the one the board already sweeps into —
    // and `ensure_overdue_list` finds that one by id. Live, it would answer
    // with this very list and the tasks would be moved into the thing being
    // deleted. Dead, it cannot: every step of that lookup filters on
    // `deleted_at IS NULL`, so it falls through to another list of the same
    // name or makes a fresh one.
    conn.execute(
        "UPDATE lists SET deleted_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;

    if !stays.is_empty() {
        let dest = ensure_overdue_list(conn, list.board_id, overdue_list_name)?;
        for task in stays {
            move_task(conn, task.id, dest, i64::MAX)?;
        }
    }
    Ok(())
}

/// Live tasks that no screen can reach: the list holding them was removed
/// while the board it belongs to is still here.
///
/// This is an invariant, not a feature — it must always be nought.
/// `delete_list` moves tasks out before it removes a list, so a count above
/// nought means something put a task somewhere nothing can show it, which is
/// the failure that would otherwise be noticed by nobody: the entry is still
/// in the database, and the only person who could miss it does not remember it
/// exists.
///
/// A removed *board* is deliberately not counted. The board is the container
/// for everything on it, and removing one is removing its contents — that is
/// an answer, not an accident.
pub fn stranded_tasks(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM tasks t
         JOIN lists l ON l.id = t.list_id
         JOIN boards b ON b.id = l.board_id
         WHERE t.deleted_at IS NULL
           AND l.deleted_at IS NOT NULL
           AND b.deleted_at IS NULL",
        [],
        |r| r.get(0),
    )
}

// -------------------------------------------------------------- overdue

/// The board's list for swept-up tasks, found or made.
///
/// Three steps, in order:
///
/// 1. The list this board already sweeps into, remembered by id — so renaming
///    it keeps working, and a rename cannot make a second one appear.
/// 2. Failing that, a list already called this. List names are not unique, so
///    without this step a board that already has a "Todo" — one you made
///    yourself, or the one from before you deleted the setting — would end up
///    with a second list of the same name and tasks split between them. If you
///    have already named a list this, that is the one you meant.
/// 3. Failing that, a new list.
///
/// A deleted list is not resurrected: its tombstone stays, and step 2 or 3
/// produces a live one.
pub fn ensure_overdue_list(conn: &Connection, board_id: i64, name: &str) -> Result<i64> {
    let key = overdue_list_key(board_id);
    if let Ok(id) = get_setting(conn, &key, "")?.parse::<i64>()
        && let Some(l) = list(conn, id)?
        && l.board_id == board_id
        && l.date.is_none()
    {
        return Ok(id);
    }

    // Case-insensitively, because "todo" and "Todo" are the same intention and
    // two lists a letter apart is exactly the confusion this avoids.
    let existing = custom_lists(conn, board_id)?.into_iter().find(|l| {
        l.name
            .as_deref()
            .is_some_and(|n| n.eq_ignore_ascii_case(name))
    });

    let id = match existing {
        Some(l) => l.id,
        None => create_custom_list(conn, board_id, name)?,
    };
    set_setting(conn, &key, &id.to_string())?;
    Ok(id)
}

/// Undone tasks sitting on days that have already gone by, oldest first.
///
/// Tasks only: an appointment on a day gone by is not overdue, it is over.
pub fn overdue_tasks(conn: &Connection, board_id: i64, today: &str) -> Result<Vec<Task>> {
    let sql = format!(
        "SELECT {} FROM tasks t
         JOIN lists l ON l.id = t.list_id
         WHERE l.board_id = ?1 AND l.deleted_at IS NULL
           AND l.date IS NOT NULL AND l.date < ?2
           AND t.deleted_at IS NULL AND t.done = 0 AND t.kind = 'task'
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
    list_name: &str,
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
        ensure_overdue_list(conn, board_id, list_name)?
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

const TASK_COLS: &str = "id, list_id, title, notes, done, kind, author_id, position, version";

/// Something owed: it carries a checkbox, and the overdue rule applies to it.
pub const KIND_TASK: &str = "task";
/// Something that happens: no checkbox, and the sweep leaves it alone, because
/// a time that has passed is not a thing left undone.
pub const KIND_APPOINTMENT: &str = "appointment";

/// Anything unrecognised is a task, which is the safe reading: it keeps its
/// checkbox and the rule that was written for it.
pub fn kind_or_task(kind: &str) -> &str {
    if kind == KIND_APPOINTMENT {
        KIND_APPOINTMENT
    } else {
        KIND_TASK
    }
}

fn map_task(row: &rusqlite::Row<'_>) -> Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        list_id: row.get(1)?,
        title: row.get(2)?,
        notes: row.get(3)?,
        done: row.get(4)?,
        kind: row.get(5)?,
        author_id: row.get(6)?,
        position: row.get(7)?,
        version: row.get(8)?,
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
    // An unsizing coercion at the return position rather than an `as` cast,
    // which reads the same and cannot be a silent numeric conversion.
    let refs: Vec<&dyn rusqlite::ToSql> = list_ids
        .iter()
        .map(|id| -> &dyn rusqlite::ToSql { id })
        .collect();
    conn.prepare(&sql)?
        .query_map(refs.as_slice(), map_task)?
        .collect()
}

pub fn task(conn: &Connection, id: i64) -> Result<Option<Task>> {
    let sql = format!("SELECT {TASK_COLS} FROM tasks WHERE id = ?1 AND deleted_at IS NULL");
    conn.query_row(&sql, params![id], map_task).optional()
}

/// Appends to the end of the list.
///
/// Always a task. Everything is written as something owed, and the few that
/// turn out to be appointments are switched afterwards in the editor — so the
/// kind is the column default here rather than an argument every caller has to
/// carry.
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
///
/// An appointment has no checkbox to press, so this is a no-op on one rather
/// than an error: the row is answered with the re-rendered column either way,
/// and a stale page that still shows a box cannot tick something that has none.
pub fn toggle_task(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET done = 1 - done, version = version + 1, updated_at = ?2
         WHERE id = ?1 AND deleted_at IS NULL AND kind = 'task'",
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
    kind: &str,
    version: i64,
) -> Result<bool> {
    let kind = kind_or_task(kind);
    let changed = conn.execute(
        // Becoming an appointment unticks it. An appointment has no checkbox,
        // so a ticked one would render as struck-through with no way back —
        // `toggle_task` refuses to touch it, by design.
        "UPDATE tasks
            SET title = ?2, notes = ?3, kind = ?4,
                done = CASE WHEN ?4 = 'appointment' THEN 0 ELSE done END,
                version = version + 1, updated_at = ?5
          WHERE id = ?1 AND version = ?6 AND deleted_at IS NULL",
        params![id, title, notes, kind, now(), version],
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
        |i| i.saturating_add(1),
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
        // The real list, in order, rather than a copy of it: a hand-kept second
        // list of migrations is one a new migration gets left out of, and the
        // tests then pass against a schema production does not have.
        for (_, sql) in crate::db::MIGRATIONS {
            conn.execute_batch(sql).unwrap();
        }
        let author = create_user(&conn, "Tester", "#3563e9").unwrap();
        let board = create_board(&conn, "Board").unwrap();
        let a = create_custom_list(&conn, board, "A").unwrap();
        let b = create_custom_list(&conn, board, "B").unwrap();
        (conn, author, a, b)
    }

    /// A schema with no users and nothing stored: the one state in which there
    /// is legitimately nobody to be.
    fn empty() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        for (_, sql) in crate::db::MIGRATIONS {
            conn.execute_batch(sql).unwrap();
        }
        conn
    }

    /// The claim the rest of the app leans on: a populated app always has a
    /// default, and an empty one never does. If this can be made false, every
    /// page that no longer asks who you are has a hole in it.
    #[test]
    fn a_populated_app_always_has_a_default_user() {
        let conn = empty();
        assert!(
            default_user(&conn).unwrap().is_none(),
            "with no users there is nobody to be"
        );

        let first = create_user(&conn, "First", "#111111").unwrap();
        assert_eq!(
            default_user(&conn).unwrap().map(|u| u.id),
            Some(first),
            "the first person created is the default, with nothing stored"
        );

        // Later arrivals do not take it over.
        let second = create_user(&conn, "Alphabetically-first", "#222222").unwrap();
        assert_eq!(default_user(&conn).unwrap().map(|u| u.id), Some(first));

        set_default_user(&conn, second).unwrap();
        assert_eq!(default_user(&conn).unwrap().map(|u| u.id), Some(second));

        // Removing the chosen one must not leave the app without a default:
        // the stored id now names nobody, so the fallback answers again.
        delete_user(&conn, second).unwrap();
        assert_eq!(
            default_user(&conn).unwrap().map(|u| u.id),
            Some(first),
            "a stored id that names nobody falls back instead of going blank"
        );

        // And the last one going leaves the one empty state, not a dangling id.
        delete_user(&conn, first).unwrap();
        assert!(default_user(&conn).unwrap().is_none());
    }

    /// The fallback is by id, not by name — otherwise renaming somebody would
    /// silently move the default, and the first person created is the one a
    /// household means by "the default".
    #[test]
    fn the_fallback_is_the_first_created_not_the_first_listed() {
        let conn = empty();
        let first = create_user(&conn, "Zoe", "#111111").unwrap();
        create_user(&conn, "Adam", "#222222").unwrap();
        assert_eq!(
            users(&conn).unwrap().first().map(|u| u.name.clone()),
            Some("Adam".to_string()),
            "the list is by name, so Adam leads it"
        );
        assert_eq!(
            default_user(&conn).unwrap().map(|u| u.id),
            Some(first),
            "but the default is Zoe, who was created first"
        );
    }

    /// A stored id is a preference, not a fact, so storing a bad one must not
    /// become a state the reader has to cope with.
    #[test]
    fn a_default_can_only_be_set_to_somebody_who_exists() {
        let conn = empty();
        let real = create_user(&conn, "Real", "#111111").unwrap();
        set_default_user(&conn, real + 999).unwrap();
        assert_eq!(
            get_setting(&conn, DEFAULT_USER, "").unwrap(),
            "",
            "an id naming nobody is not stored"
        );
        assert_eq!(default_user(&conn).unwrap().map(|u| u.id), Some(real));
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
        assert_eq!(titles(&conn, a), Vec::<String>::new());
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

    /// List names are not unique, so the sweep has to look before it creates:
    /// a board that already has a Todo list must not grow a second one with
    /// the same name and the tasks split between them.
    #[test]
    fn a_list_that_is_already_called_this_is_adopted_rather_than_duplicated() {
        let (conn, _, _, _) = fixture();
        let board = create_board(&conn, "Board").unwrap();
        let mine = create_custom_list(&conn, board, "Todo").unwrap();

        assert_eq!(ensure_overdue_list(&conn, board, "Todo").unwrap(), mine);
        assert_eq!(
            custom_lists(&conn, board).unwrap().len(),
            1,
            "no second list of the same name"
        );
    }

    #[test]
    fn adoption_ignores_capitalisation_but_not_the_word() {
        let (conn, _, _, _) = fixture();
        let board = create_board(&conn, "Board").unwrap();
        let shouty = create_custom_list(&conn, board, "TODO").unwrap();
        assert_eq!(ensure_overdue_list(&conn, board, "Todo").unwrap(), shouty);

        let other = create_board(&conn, "Other").unwrap();
        create_custom_list(&conn, other, "Shopping").unwrap();
        let made = ensure_overdue_list(&conn, other, "Todo").unwrap();
        assert_eq!(
            list(&conn, made).unwrap().unwrap().name.unwrap(),
            "Todo",
            "an unrelated list is not adopted"
        );
        assert_eq!(custom_lists(&conn, other).unwrap().len(), 2);
    }

    /// Deleting the list the board sweeps into and having another by the same
    /// name is the awkward case: the tombstone must not come back, and neither
    /// must a duplicate.
    #[test]
    fn a_deleted_target_falls_through_to_the_one_still_standing() {
        let (conn, _, _, _) = fixture();
        let board = create_board(&conn, "Board").unwrap();
        let first = ensure_overdue_list(&conn, board, "Todo").unwrap();
        let second = create_custom_list(&conn, board, "Todo").unwrap();
        delete_list(&conn, first, OVERDUE_LIST_NAME).unwrap();

        assert_eq!(ensure_overdue_list(&conn, board, "Todo").unwrap(), second);
        assert_eq!(custom_lists(&conn, board).unwrap().len(), 1);
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
        assert!(is_board_member(&conn, b, author).unwrap());
        assert!(!is_board_member(&conn, b, u2).unwrap());
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
        delete_list(&conn, cl, OVERDUE_LIST_NAME).unwrap();
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

    /// Everything is written as a task; the editor is the only way to become
    /// an appointment. Returns its id.
    fn make_appointment(conn: &Connection, list: i64, title: &str, author: i64) -> i64 {
        let id = create_task(conn, list, title, author).unwrap();
        let version = task(conn, id).unwrap().unwrap().version;
        assert!(update_task_cas(conn, id, title, "", KIND_APPOINTMENT, version).unwrap());
        id
    }

    /// The fear this is written against: something put on a date a year out,
    /// forgotten completely, and then not there when the day arrives — with
    /// nobody able to notice, because nobody remembers writing it.
    ///
    /// Nothing between here and the screen may quietly drop it: it keeps its
    /// day, it survives every sweep run in the meantime, and the grid that
    /// covers that week still finds it when the week finally comes round.
    #[test]
    fn an_entry_a_year_out_is_still_there_when_the_day_comes() {
        let (conn, author, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        let far = "2027-09-04";
        let day = ensure_day_list(&conn, board, far).unwrap();
        let task_id = create_task(&conn, day, "passport expires", author).unwrap();
        let appt = make_appointment(&conn, day, "wedding", author);

        // A year of opening the board, every day of it, under both settings.
        for action in [OVERDUE_TODAY, OVERDUE_LIST] {
            for today in ["2026-09-04", "2026-12-31", "2027-01-01", "2027-09-03"] {
                sweep_overdue(&conn, board, today, action, OVERDUE_LIST_NAME).unwrap();
            }
        }

        for id in [task_id, appt] {
            assert_eq!(
                task(&conn, id).unwrap().unwrap().list_id,
                day,
                "a year of sweeps moved an entry off the day it was written for"
            );
        }

        // And the week containing it still selects it. The range is compared
        // as text, so a year away is no different from a week away.
        let lists = day_lists_in_range(&conn, board, "2027-08-30", "2027-09-05").unwrap();
        assert!(lists.iter().any(|l| l.id == day), "the day is in its week");
        let titles: Vec<_> = tasks_for_list(&conn, day, true)
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(titles, ["passport expires", "wedding"]);
    }

    /// Removing a list must not take what is in it out of reach. The tasks
    /// stay live either way; the question is whether anything can still show
    /// them, and `stranded_tasks` is the thing that would notice.
    #[test]
    fn removing_a_list_moves_what_is_in_it_rather_than_stranding_it() {
        let (conn, author, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        let shopping = create_custom_list(&conn, board, "Shopping").unwrap();
        seed(&conn, shopping, author, &["milk", "bread"]);

        delete_list(&conn, shopping, OVERDUE_LIST_NAME).unwrap();

        assert_eq!(stranded_tasks(&conn).unwrap(), 0);
        let todo = ensure_overdue_list(&conn, board, OVERDUE_LIST_NAME).unwrap();
        assert_eq!(titles(&conn, todo), ["milk", "bread"]);
    }

    /// The nastiest shape of it: the list being removed is the one the board
    /// sweeps into, so the destination has to be worked out after it is gone
    /// or it would be the list itself.
    #[test]
    fn removing_the_overdue_list_itself_strands_nothing() {
        let (conn, author, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        let todo = ensure_overdue_list(&conn, board, OVERDUE_LIST_NAME).unwrap();
        seed(&conn, todo, author, &["milk"]);

        delete_list(&conn, todo, OVERDUE_LIST_NAME).unwrap();

        assert_eq!(stranded_tasks(&conn).unwrap(), 0);
        let fresh = ensure_overdue_list(&conn, board, OVERDUE_LIST_NAME).unwrap();
        assert_ne!(fresh, todo, "a tombstone is not resurrected");
        assert_eq!(titles(&conn, fresh), ["milk"]);
    }

    /// A day is not offered for deletion anywhere, so one arriving is a stale
    /// or hand-made request — and emptying a day is not what it asked for.
    #[test]
    fn a_day_cannot_be_deleted_as_if_it_were_a_list() {
        let (conn, author, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        let day = ensure_day_list(&conn, board, "2026-09-04").unwrap();
        seed(&conn, day, author, &["dentist"]);

        delete_list(&conn, day, OVERDUE_LIST_NAME).unwrap();

        assert!(list(&conn, day).unwrap().is_some(), "the day is still here");
        assert_eq!(titles(&conn, day), ["dentist"]);
        assert_eq!(stranded_tasks(&conn).unwrap(), 0);
    }

    /// The alarm has to ring, or every test above it passes against a query
    /// that always answers nought. This is the only place that strands a task
    /// on purpose: straight SQL, the way a future bug would do it, going round
    /// `delete_list` exactly as that bug would.
    #[test]
    fn the_alarm_rings_when_something_is_stranded() {
        let (conn, author, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        let list_id = create_custom_list(&conn, board, "Shopping").unwrap();
        seed(&conn, list_id, author, &["milk", "bread"]);
        assert_eq!(stranded_tasks(&conn).unwrap(), 0);

        conn.execute(
            "UPDATE lists SET deleted_at = '2026-10-01T00:00:00Z' WHERE id = ?1",
            params![list_id],
        )
        .unwrap();

        assert_eq!(stranded_tasks(&conn).unwrap(), 2);
    }

    /// Removing a board is removing what is on it — the container is the
    /// answer, not an accident — so it must not set off the alarm.
    #[test]
    fn a_removed_board_is_not_counted_as_stranded() {
        let (conn, author, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        let list_id = create_custom_list(&conn, board, "Shopping").unwrap();
        seed(&conn, list_id, author, &["milk"]);

        delete_board(&conn, board).unwrap();

        assert_eq!(stranded_tasks(&conn).unwrap(), 0);
    }

    /// The distinction the kind exists for: an appointment on a day that has
    /// gone by is not something left undone, it is something that happened.
    #[test]
    fn an_appointment_in_the_past_is_never_overdue() {
        let (conn, author, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        let mon = ensure_day_list(&conn, board, "2026-09-01").unwrap();
        let appt = make_appointment(&conn, mon, "dentist", author);
        create_task(&conn, mon, "call the plumber", author).unwrap();

        let stale = overdue_tasks(&conn, board, TODAY).unwrap();
        let titles: Vec<_> = stale.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["call the plumber"]);

        // And the sweep that runs on every page load leaves it where it is,
        // under every setting, rather than only the query it is built on.
        for action in [OVERDUE_TODAY, OVERDUE_LIST] {
            sweep_overdue(&conn, board, TODAY, action, OVERDUE_LIST_NAME).unwrap();
            assert_eq!(
                task(&conn, appt).unwrap().unwrap().list_id,
                mon,
                "{action} moved an appointment off the day it happened on"
            );
        }
    }

    /// Nothing on screen offers it, but a stale tab and a hand-written POST
    /// both can, and neither may put a tick on something with no box.
    #[test]
    fn an_appointment_cannot_be_ticked() {
        let (conn, author, a, _) = fixture();
        let appt = make_appointment(&conn, a, "dentist", author);
        let before = task(&conn, appt).unwrap().unwrap();

        toggle_task(&conn, appt).unwrap();

        let after = task(&conn, appt).unwrap().unwrap();
        assert!(!after.done);
        assert_eq!(
            after.version, before.version,
            "a refused toggle must not age the row either, or every open editor \
             on it goes stale for nothing"
        );
    }

    /// Anything the editor did not send, or sent wrong, is something owed —
    /// the reading that keeps the checkbox and the overdue rule.
    #[test]
    fn an_unrecognised_kind_is_saved_as_a_task() {
        let (conn, author, a, _) = fixture();
        let id = make_appointment(&conn, a, "dentist", author);
        let version = task(&conn, id).unwrap().unwrap().version;

        assert!(update_task_cas(&conn, id, "dentist", "", "nonsense", version).unwrap());
        assert_eq!(task(&conn, id).unwrap().unwrap().kind, KIND_TASK);
    }

    /// A ticked task that becomes an appointment must lose the tick with it.
    /// An appointment has no checkbox to press, so a ticked one would be drawn
    /// struck through with nothing on screen able to undo it.
    #[test]
    fn becoming_an_appointment_unticks_it() {
        let (conn, author, a, _) = fixture();
        let id = create_task(&conn, a, "dentist", author).unwrap();
        toggle_task(&conn, id).unwrap();
        assert!(task(&conn, id).unwrap().unwrap().done);

        let version = task(&conn, id).unwrap().unwrap().version;
        assert!(update_task_cas(&conn, id, "dentist", "", KIND_APPOINTMENT, version).unwrap());

        let after = task(&conn, id).unwrap().unwrap();
        assert!(!after.done);
        assert_eq!(after.kind, KIND_APPOINTMENT);
    }

    /// Switching kind goes through the same compare-and-swap as the words do,
    /// so it cannot quietly overwrite an edit made while the editor was open.
    #[test]
    fn switching_kind_on_a_stale_version_is_refused() {
        let (conn, author, a, _) = fixture();
        let id = create_task(&conn, a, "dentist", author).unwrap();
        let stale = task(&conn, id).unwrap().unwrap().version;
        toggle_task(&conn, id).unwrap(); // someone ticks it under the editor

        assert!(!update_task_cas(&conn, id, "dentist", "", KIND_APPOINTMENT, stale).unwrap());
        assert_eq!(task(&conn, id).unwrap().unwrap().kind, KIND_TASK);
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

        let touched = sweep_overdue(&conn, board, TODAY, OVERDUE_LEAVE, OVERDUE_LIST_NAME).unwrap();
        assert_eq!(touched, Vec::<i64>::new());
        assert_eq!(overdue_tasks(&conn, board, TODAY).unwrap().len(), 2);
        assert!(task(&conn, ids[0]).unwrap().is_some());
    }

    #[test]
    fn moving_to_today_empties_the_past() {
        let (conn, author, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        overdue_fixture(&conn, board, author);

        sweep_overdue(&conn, board, TODAY, OVERDUE_TODAY, OVERDUE_LIST_NAME).unwrap();
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

        sweep_overdue(&conn, board, TODAY, OVERDUE_LIST, OVERDUE_LIST_NAME).unwrap();
        assert_eq!(overdue_tasks(&conn, board, TODAY).unwrap().len(), 0);

        let dest = ensure_overdue_list(&conn, board, OVERDUE_LIST_NAME).unwrap();
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

            sweep_overdue(&conn, board, TODAY, action, OVERDUE_LIST_NAME).unwrap();
            let second = sweep_overdue(&conn, board, TODAY, action, OVERDUE_LIST_NAME).unwrap();
            assert_eq!(second, Vec::<i64>::new(), "{action} was not idempotent");
        }
    }

    #[test]
    fn a_ticked_task_is_left_where_it_was() {
        let (conn, author, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        let ids = overdue_fixture(&conn, board, author);

        sweep_overdue(&conn, board, TODAY, OVERDUE_LIST, OVERDUE_LIST_NAME).unwrap();
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

        sweep_overdue(&conn, mine, TODAY, OVERDUE_LIST, OVERDUE_LIST_NAME).unwrap();
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

        let a = ensure_overdue_list(&conn, one, OVERDUE_LIST_NAME).unwrap();
        assert_eq!(
            a,
            ensure_overdue_list(&conn, one, OVERDUE_LIST_NAME).unwrap(),
            "reused, not remade"
        );
        assert_ne!(
            a,
            ensure_overdue_list(&conn, two, OVERDUE_LIST_NAME).unwrap(),
            "one per board"
        );
        assert_eq!(custom_lists(&conn, one).unwrap().len(), 1);
    }

    #[test]
    fn renaming_the_list_keeps_it_as_the_target() {
        let (conn, _, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        let id = ensure_overdue_list(&conn, board, OVERDUE_LIST_NAME).unwrap();
        rename_list(&conn, id, "Backlog").unwrap();
        assert_eq!(
            ensure_overdue_list(&conn, board, OVERDUE_LIST_NAME).unwrap(),
            id,
            "tracked by id"
        );
    }

    #[test]
    fn deleting_the_list_makes_a_fresh_one() {
        let (conn, _, _, _) = fixture();
        let board = create_board(&conn, "B").unwrap();
        let first = ensure_overdue_list(&conn, board, OVERDUE_LIST_NAME).unwrap();
        delete_list(&conn, first, OVERDUE_LIST_NAME).unwrap();

        let second = ensure_overdue_list(&conn, board, OVERDUE_LIST_NAME).unwrap();
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
            sweep_overdue(&conn, board, TODAY, OVERDUE_LIST, OVERDUE_LIST_NAME).unwrap(),
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
        assert_eq!(titles(&conn, a), Vec::<String>::new());
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

        assert!(update_task_cas(&conn, id, "final", "notes", KIND_TASK, v).unwrap());
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
        assert!(update_task_cas(&conn, id, "theirs", "", KIND_TASK, stale).unwrap());

        // The second editor replays the version they rendered from.
        assert!(!update_task_cas(&conn, id, "mine", "", KIND_TASK, stale).unwrap());

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
        assert!(!update_task_cas(&conn, id, "renamed", "", KIND_TASK, v).unwrap());
        assert_eq!(task(&conn, id).unwrap().unwrap().title, "thing");
    }

    #[test]
    fn cas_on_a_deleted_task_is_refused() {
        let (conn, author, a, _) = fixture();
        let id = seed(&conn, a, author, &["gone"])[0];
        let v = task(&conn, id).unwrap().unwrap().version;
        delete_task(&conn, id).unwrap();
        assert!(!update_task_cas(&conn, id, "back", "", KIND_TASK, v).unwrap());
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
