//! Task mutations. Every one of them answers with the re-rendered column, so
//! the server's view of ordering always wins over the client's guess.

use anyhow::Context;
use axum::Router;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum_extra::extract::Form;
use serde::Deserialize;

use crate::error::AppResult;
use crate::routes::board::render_column;
use crate::routes::{Density, current_user, locale, render};
use crate::views::{self, ColumnKey};
use crate::{AppState, queries};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/b/{board}/task", post(create))
        .route("/task/{id}/toggle", post(toggle))
        .route("/task/{id}/edit", get(edit_form))
        .route("/task/{id}/row", get(row_fragment))
        .route("/task/{id}", post(update))
        .route("/task/{id}/delete", post(delete))
        .route("/task/{id}/restore", post(restore))
        .route("/task/{id}/move", post(move_task))
}

/// A malformed or stale request from the client — distinct from a 500, which
/// means we broke.
fn bad(msg: &str) -> Response {
    (axum::http::StatusCode::BAD_REQUEST, format!("{msg}\n")).into_response()
}

/// The column a task currently lives in, for re-rendering after a mutation.
fn column_of(state: &AppState, task_id: i64) -> anyhow::Result<Option<(i64, ColumnKey)>> {
    state
        .db
        .with(|conn| -> anyhow::Result<_> {
            let Some(t) = queries::task(conn, task_id)? else {
                return Ok(None);
            };
            let Some(l) = queries::list(conn, t.list_id)? else {
                return Ok(None);
            };
            let key = match &l.date {
                Some(d) => match crate::calendar::parse(d) {
                    Some(d) => ColumnKey::Day(d),
                    None => return Ok(None),
                },
                None => ColumnKey::List(l.id),
            };
            Ok(Some((l.board_id, key)))
        })
        .with_context(|| format!("locating the column of task {task_id}"))
}

#[derive(Deserialize)]
struct CreateForm {
    /// A date or `list-<id>` — see the column addressing note in `views`.
    key: String,
    title: String,
}

async fn create(
    State(state): State<AppState>,
    jar: axum_extra::extract::CookieJar,
    Density(density): Density,
    Path(board_id): Path<i64>,
    Form(f): Form<CreateForm>,
) -> AppResult {
    let Some(me) = current_user(&state, &jar)? else {
        return Ok(bad("no identity selected"));
    };
    let Some(key) = ColumnKey::parse(&f.key) else {
        return Ok(bad("bad column key"));
    };
    let title = f.title.trim().to_string();

    if !title.is_empty() {
        // Creating the day-list and the task together: a lazily created list
        // with no task in it would be a row that renders nothing.
        state
            .db
            .transaction(|tx| -> anyhow::Result<_> {
                let Some(list_id) = key.resolve_for_write(tx, board_id)? else {
                    return Ok(());
                };
                queries::create_task(tx, list_id, &title, me.id)?;
                Ok(())
            })
            .with_context(|| format!("adding a task to column {}", key.as_string()))?;
        state.changes.record(board_id, &key);
    }

    render_column(&state, &locale(&state), board_id, key, density)
}

/// The three single-task mutations differ only in the statement they run, so
/// they share the locate / mutate / re-render shape.
fn mutate_in_place(
    state: &AppState,
    id: i64,
    what: &'static str,
    density: &str,
    run: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<()>,
) -> AppResult {
    let Some((board_id, key)) = column_of(state, id)? else {
        return Ok(bad("no such task"));
    };
    state
        .db
        .with(run)
        .with_context(|| format!("{what} task {id}"))?;
    state.changes.record(board_id, &key);
    render_column(state, &locale(state), board_id, key, density)
}

async fn toggle(
    State(state): State<AppState>,
    Density(density): Density,
    Path(id): Path<i64>,
) -> AppResult {
    mutate_in_place(&state, id, "toggling", density, move |conn| {
        queries::toggle_task(conn, id)
    })
}

/// Deleting does not ask first. A confirmation dialog interrupts the common
/// case (you meant it) to guard against the rare one, and the rare one is
/// already covered: the row is soft-deleted, so putting it back is exact.
async fn delete(
    State(state): State<AppState>,
    Density(density): Density,
    Path(id): Path<i64>,
) -> AppResult {
    let Some((board_id, key)) = column_of(&state, id)? else {
        return Ok(bad("no such task"));
    };
    let title = state
        .db
        .with(|conn| queries::task(conn, id))
        .with_context(|| format!("reading task {id} before deleting it"))?
        .map(|t| t.title)
        .unwrap_or_default();

    // Two writes (the tombstone and the renumbering) must land together.
    state
        .db
        .transaction(|tx| queries::delete_task(tx, id))
        .with_context(|| format!("deleting task {id}"))?;
    state.changes.record(board_id, &key);

    render_column_with_undo(&state, board_id, key, density, Some((id, title)))
}

async fn restore(
    State(state): State<AppState>,
    Density(density): Density,
    Path(id): Path<i64>,
) -> AppResult {
    let restored = state
        .db
        .transaction(|tx| -> anyhow::Result<_> {
            let Some(task) = queries::task_any(tx, id)? else {
                return Ok(None);
            };
            let Some(list) = queries::list(tx, task.list_id)? else {
                return Ok(None);
            };
            queries::restore_task(tx, id)?;
            let key = match &list.date {
                Some(d) => crate::calendar::parse(d).map(ColumnKey::Day),
                None => Some(ColumnKey::List(list.id)),
            };
            Ok(key.map(|k| (list.board_id, k)))
        })
        .with_context(|| format!("restoring task {id}"))?;

    let Some((board_id, key)) = restored else {
        return Ok(bad("no such task"));
    };
    state.changes.record(board_id, &key);

    // `None` re-renders the undo bar empty, which dismisses it.
    render_column_with_undo(&state, board_id, key, density, None)
}

/// The column, plus an out-of-band update to the undo bar.
///
/// Only delete and restore use this. An ordinary column refresh must not touch
/// the bar, or a background poll landing a second after a delete would clear
/// the undo before it could be used.
fn render_column_with_undo(
    state: &AppState,
    board_id: i64,
    key: ColumnKey,
    page_density: &str,
    undo: Option<(i64, String)>,
) -> AppResult {
    let loc = locale(state);
    let density = crate::views::density_for(key, page_density);
    let (col, show_colour) = state
        .db
        .with(|conn| -> anyhow::Result<_> {
            let show_colour = queries::board_member_count(conn, board_id)? > 1;
            let col = views::load_column(conn, board_id, key, density, &loc)?;
            Ok((col, show_colour))
        })
        .with_context(|| format!("re-rendering column {}", key.as_string()))?;

    render(
        state,
        &loc,
        "column_undo.html",
        minijinja::context! {
            col => col,
            board_id => board_id,
            show_colour => show_colour,
            undo_id => undo.as_ref().map(|(id, _)| *id),
            undo_title => undo.map(|(_, title)| title),
        },
    )
}

/// Swaps the row for an editor. The version rendered here is the one the save
/// is checked against (D12).
async fn edit_form(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult {
    let loaded = state
        .db
        .with(|conn| -> anyhow::Result<_> {
            let Some(t) = queries::task(conn, id)? else {
                return Ok(None);
            };
            let authors = queries::users(conn)?;
            let Some(l) = queries::list(conn, t.list_id)? else {
                return Ok(None);
            };
            let key = match &l.date {
                Some(d) => {
                    ColumnKey::Day(crate::calendar::parse(d).unwrap_or_else(crate::calendar::today))
                }
                None => ColumnKey::List(l.id),
            };
            Ok(Some((
                views::task_view(&t, &authors),
                l.board_id,
                key.as_string(),
            )))
        })
        .with_context(|| format!("loading the editor for task {id}"))?;

    let Some((task, board_id, col_key)) = loaded else {
        return Ok(bad("no such task"));
    };
    render(
        &state,
        &locale(&state),
        "task_edit.html",
        minijinja::context! {
            task => task, board_id => board_id,
            col_key => col_key, conflict => false,
        },
    )
}

/// One task row, as it looks when not being edited. Cancelling an edit swaps
/// this back over the editor — a row-sized answer rather than re-rendering the
/// whole column, so a second editor open in the same column survives it.
async fn row_fragment(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult {
    let loaded = state
        .db
        .with(|conn| -> anyhow::Result<_> {
            let Some(t) = queries::task(conn, id)? else {
                return Ok(None);
            };
            let Some(l) = queries::list(conn, t.list_id)? else {
                return Ok(None);
            };
            let authors = queries::users(conn)?;
            let show_colour = queries::board_member_count(conn, l.board_id)? > 1;
            Ok(Some((views::task_view(&t, &authors), show_colour)))
        })
        .with_context(|| format!("loading the row for task {id}"))?;

    let Some((task, show_colour)) = loaded else {
        // Deleted under the editor: answer with nothing so the row disappears
        // rather than leaving a stuck editor behind.
        return Ok(axum::response::Html(String::new()).into_response());
    };
    render(
        &state,
        &locale(&state),
        "task_row.html",
        minijinja::context! { task => task, show_colour => show_colour },
    )
}

#[derive(Deserialize)]
struct UpdateForm {
    title: String,
    #[serde(default)]
    notes: String,
    /// The version the form was rendered from.
    version: i64,
}

/// Compare-and-swap. A refused write re-renders the editor holding *both* the
/// current server value and the rejected draft — nothing typed is destroyed,
/// and there is no merge dialog (D12).
async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(f): Form<UpdateForm>,
) -> AppResult {
    let title = f.title.trim().to_string();
    if title.is_empty() {
        return Ok(bad("title cannot be empty"));
    }

    let (accepted, current, authors) = state
        .db
        .with(|conn| -> anyhow::Result<_> {
            let accepted = queries::update_task_cas(conn, id, &title, &f.notes, f.version)?;
            let current = queries::task(conn, id)?;
            let authors = queries::users(conn)?;
            Ok((accepted, current, authors))
        })
        .with_context(|| format!("saving task {id}"))?;

    let Some(current) = current else {
        return Ok(bad("no such task"));
    };

    let mut view = views::task_view(&current, &authors);
    let show_colour = state
        .db
        .with(|conn| -> anyhow::Result<_> {
            let Some(list) = queries::list(conn, current.list_id)? else {
                return Ok(false);
            };
            Ok(queries::board_member_count(conn, list.board_id)? > 1)
        })
        .with_context(|| format!("loading the board of task {id}"))?;

    if accepted {
        if let Some((board_id, key)) = column_of(&state, id)? {
            state.changes.record(board_id, &key);
        }
        // Editing a title or note cannot reorder the column, so the row is the
        // whole answer.
        return render(
            &state,
            &locale(&state),
            "task_row.html",
            minijinja::context! { task => view, show_colour => show_colour },
        );
    }

    let server_title = view.title.clone();
    let server_notes = view.notes.clone();
    // Put their draft back in the fields; show the server's value alongside.
    view.title = title;
    view.notes = f.notes;

    render(
        &state,
        &locale(&state),
        "task_edit.html",
        minijinja::context! {
            task => view,
            conflict => true,
            server_title => server_title,
            server_notes => server_notes,
        },
    )
}

#[derive(Deserialize)]
struct MoveForm {
    /// Destination column, addressed the same way everywhere.
    key: String,
    /// The task this one was dropped after; absent means the head of the list.
    /// Naming the neighbour rather than sending an index keeps the drop
    /// unambiguous when the display order differs from the stored order.
    #[serde(default)]
    after: Option<i64>,
}

async fn move_task(
    State(state): State<AppState>,
    Density(density): Density,
    Path(id): Path<i64>,
    Form(f): Form<MoveForm>,
) -> AppResult {
    let Some(dest) = ColumnKey::parse(&f.key) else {
        return Ok(bad("bad column key"));
    };
    let Some((board_id, origin_col)) = column_of(&state, id)? else {
        return Ok(bad("no such task"));
    };

    // Renumbering both lists has to be atomic or a crash mid-move leaves a
    // duplicated or missing position.
    state
        .db
        .transaction(|tx| -> anyhow::Result<_> {
            let Some(dest_list) = dest.resolve_for_write(tx, board_id)? else {
                return Ok(());
            };
            let position = queries::position_after(tx, dest_list, id, f.after)?;
            queries::move_task(tx, id, dest_list, position)?;
            Ok(())
        })
        .with_context(|| format!("moving task {id} into column {}", dest.as_string()))?;

    // Both ends of the move changed, so both are invalidated.
    state.changes.record(board_id, &dest);
    if origin_col != dest {
        state.changes.record(board_id, &origin_col);
    }

    // The source column changed too when the task left it; the client refetches
    // it via the out-of-band header rather than us guessing at swap targets.
    let mut resp = render_column(&state, &locale(&state), board_id, dest, density)?;
    if origin_col != dest
        && let Ok(v) = axum::http::HeaderValue::from_str(&origin_col.as_string())
    {
        resp.headers_mut().insert("X-Refresh-Column", v);
    }
    Ok(resp)
}
