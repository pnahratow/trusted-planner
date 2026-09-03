//! Task mutations. Every one of them answers with the re-rendered column, so
//! the server's view of ordering always wins over the client's guess.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use axum_extra::extract::Form;
use axum_extra::extract::CookieJar;
use serde::Deserialize;

use crate::routes::{current_user, move_completed, render};
use crate::routes::board::render_column;
use crate::views::{self, ColumnKey};
use crate::{queries, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/b/{board}/task", post(create))
        .route("/task/{id}/toggle", post(toggle))
        .route("/task/{id}/edit", get(edit_form))
        .route("/task/{id}", post(update))
        .route("/task/{id}/delete", post(delete))
        .route("/task/{id}/move", post(move_task))
}

fn bad(msg: &str) -> Response {
    (axum::http::StatusCode::BAD_REQUEST, format!("{msg}\n")).into_response()
}

fn oops(e: rusqlite::Error) -> Response {
    tracing::error!(error = %e, "task mutation failed");
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "database error\n").into_response()
}

/// The column a task currently lives in, for re-rendering after a mutation.
fn column_of(state: &AppState, task_id: i64) -> rusqlite::Result<Option<(i64, ColumnKey)>> {
    state.db.with(|conn| {
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
}

#[derive(Deserialize)]
struct CreateForm {
    /// A date or `list-<id>` — see the column addressing note in `views`.
    key: String,
    title: String,
}

async fn create(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(board_id): Path<i64>,
    Form(f): Form<CreateForm>,
) -> Response {
    let Some(me) = current_user(&state, &jar) else {
        return bad("no identity selected");
    };
    let Some(key) = ColumnKey::parse(&f.key) else {
        return bad("bad column key");
    };
    let title = f.title.trim().to_string();

    if !title.is_empty() {
        // Creating the day-list and the task together: a lazily created list
        // with no task in it would be a row that renders nothing.
        let created = state.db.transaction(|tx| {
            let Some(list_id) = key.resolve_for_write(tx, board_id)? else {
                return Ok(false);
            };
            queries::create_task(tx, list_id, &title, me.id)?;
            Ok(true)
        });
        if let Err(e) = created {
            return oops(e);
        }
    }

    render_column(&state, board_id, key, move_completed(&state))
}

async fn toggle(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    let move_completed = move_completed(&state);
    match column_of(&state, id) {
        Ok(Some((board_id, key))) => {
            if let Err(e) = state.db.with(|conn| queries::toggle_task(conn, id)) {
                return oops(e);
            }
            render_column(&state, board_id, key, move_completed)
        }
        Ok(None) => bad("no such task"),
        Err(e) => oops(e),
    }
}

async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    let move_completed = move_completed(&state);
    match column_of(&state, id) {
        Ok(Some((board_id, key))) => {
            // Two writes (the tombstone and the renumbering) must land together.
            if let Err(e) = state.db.transaction(|tx| queries::delete_task(tx, id)) {
                return oops(e);
            }
            render_column(&state, board_id, key, move_completed)
        }
        Ok(None) => bad("no such task"),
        Err(e) => oops(e),
    }
}

/// Swaps the row for an editor. The version rendered here is the one the save
/// is checked against (D12).
async fn edit_form(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    let loaded = state.db.with(|conn| {
        let Some(t) = queries::task(conn, id)? else {
            return Ok(None);
        };
        let authors = queries::users(conn)?;
        let Some(l) = queries::list(conn, t.list_id)? else {
            return Ok(None);
        };
        let key = match &l.date {
            Some(d) => ColumnKey::Day(crate::calendar::parse(d).unwrap_or_else(crate::calendar::today)),
            None => ColumnKey::List(l.id),
        };
        Ok(Some((views::task_view(&t, &authors), l.board_id, key.as_string())))
    });

    match loaded {
        Ok(Some((task, board_id, col_key))) => render(
            &state,
            "task_edit.html",
            minijinja::context! {
                task => task, board_id => board_id,
                col_key => col_key, conflict => false,
            },
        ),
        Ok(None) => bad("no such task"),
        Err(e) => oops(e),
    }
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
) -> Response {
    let move_completed = move_completed(&state);
    let title = f.title.trim().to_string();
    if title.is_empty() {
        return bad("title cannot be empty");
    }

    let outcome = state.db.with(|conn| {
        let accepted = queries::update_task_cas(conn, id, &title, &f.notes, f.version)?;
        let current = queries::task(conn, id)?;
        let authors = queries::users(conn)?;
        Ok((accepted, current, authors))
    });

    let (accepted, current, authors) = match outcome {
        Ok(v) => v,
        Err(e) => return oops(e),
    };
    let Some(current) = current else {
        return bad("no such task");
    };

    if accepted {
        return match column_of(&state, id) {
            Ok(Some((board_id, key))) => render_column(&state, board_id, key, move_completed),
            Ok(None) => bad("no such task"),
            Err(e) => oops(e),
        };
    }

    let (board_id, col_key) = match column_of(&state, id) {
        Ok(Some((b, k))) => (b, k.as_string()),
        _ => (0, String::new()),
    };
    let mut view = views::task_view(&current, &authors);
    let server_title = view.title.clone();
    let server_notes = view.notes.clone();
    // Put their draft back in the fields; show the server's value alongside.
    view.title = title;
    view.notes = f.notes;

    // The form posts at the column, but a refusal must replace the editor, not
    // the column around it — so retarget the swap onto the row itself.
    let mut resp = render(
        &state,
        "task_edit.html",
        minijinja::context! {
            task => view,
            board_id => board_id,
            col_key => col_key,
            conflict => true,
            server_title => server_title,
            server_notes => server_notes,
        },
    );
    let headers = resp.headers_mut();
    if let Ok(v) = axum::http::HeaderValue::from_str(&format!("#task-{id}")) {
        headers.insert("HX-Retarget", v);
    }
    headers.insert("HX-Reswap", axum::http::HeaderValue::from_static("outerHTML"));
    resp
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

/// Phase 5 drives this from SortableJS; it is server-authoritative already, so
/// the endpoint lands now and the drag layer is purely additive.
async fn move_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(f): Form<MoveForm>,
) -> Response {
    let move_completed = move_completed(&state);
    let Some(dest) = ColumnKey::parse(&f.key) else {
        return bad("bad column key");
    };
    let Ok(Some((board_id, origin))) = column_of(&state, id) else {
        return bad("no such task");
    };

    // Renumbering both lists has to be atomic or a crash mid-move leaves a
    // duplicated or missing position.
    let moved = state.db.transaction(|tx| {
        let Some(dest_list) = dest.resolve_for_write(tx, board_id)? else {
            return Ok(false);
        };
        let position = queries::position_after(tx, dest_list, id, f.after)?;
        queries::move_task(tx, id, dest_list, position)?;
        Ok(true)
    });
    if let Err(e) = moved {
        return oops(e);
    }

    // The source column changed too when the task left it; the client refetches
    // it via the out-of-band header rather than us guessing at swap targets.
    let mut resp = render_column(&state, board_id, dest, move_completed);
    if origin != dest {
        if let Ok(v) = axum::http::HeaderValue::from_str(&origin.as_string()) {
            resp.headers_mut().insert("X-Refresh-Column", v);
        }
    }
    resp
}
