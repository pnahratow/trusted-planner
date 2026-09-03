//! The week grid and the column fragment both views share.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;
use axum_extra::extract::CookieJar;

use crate::calendar;
use crate::routes::{current_user, move_completed, render};
use crate::views::{self, ColumnKey};
use crate::{queries, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/b/{board}/w/{monday}", get(week))
        .route("/b/{board}/col/{key}", get(column_fragment))
}

/// Sends you somewhere sensible: settings if the app is empty, the picker if
/// this browser has no identity, otherwise your first board this week.
async fn index(State(state): State<AppState>, jar: CookieJar) -> Response {
    let users = state.db.with(|conn| queries::users(conn)).unwrap_or_default();
    if users.is_empty() {
        return Redirect::to("/settings").into_response();
    }

    let Some(me) = current_user(&state, &jar) else {
        return Redirect::to("/pick").into_response();
    };

    let boards = state
        .db
        .with(|conn| queries::boards_for_user(conn, me.id))
        .unwrap_or_default();
    let Some(board) = boards.first() else {
        return Redirect::to("/settings").into_response();
    };

    let monday = calendar::fmt(calendar::monday_of(calendar::today()));
    Redirect::to(&format!("/b/{}/w/{}", board.id, monday)).into_response()
}

async fn week(
    State(state): State<AppState>,
    jar: CookieJar,
    Path((board_id, monday)): Path<(i64, String)>,
) -> Response {
    let Some(me) = current_user(&state, &jar) else {
        return Redirect::to("/pick").into_response();
    };
    let Some(monday) = calendar::parse(&monday).map(calendar::monday_of) else {
        return Redirect::to("/").into_response();
    };

    let dates = calendar::week_of(monday);
    let move_completed = move_completed(&state);

    let loaded = state.db.with(|conn| {
        let Some(board) = queries::board(conn, board_id)? else {
            return Ok(None);
        };
        let authors = queries::users(conn)?;
        let all_boards = queries::boards(conn)?;

        // Spines only mean something where more than one person writes (D11).
        let show_colour = queries::board_member_count(conn, board_id)? > 1;

        let from = calendar::fmt(dates[0]);
        let to = calendar::fmt(*dates.last().unwrap());
        let day_lists = queries::day_lists_in_range(conn, board_id, &from, &to)?;
        let custom = queries::custom_lists(conn, board_id)?;

        // One query for every column on screen.
        let ids: Vec<i64> = day_lists.iter().chain(&custom).map(|l| l.id).collect();
        let tasks = queries::tasks_for_lists(conn, &ids, move_completed)?;

        let mut days = Vec::new();
        for d in &dates {
            let ds = calendar::fmt(*d);
            let list = day_lists.iter().find(|l| l.date.as_deref() == Some(&ds));
            let mine: Vec<_> = list
                .map(|l| tasks.iter().filter(|t| t.list_id == l.id).cloned().collect())
                .unwrap_or_default();
            days.push(views::column_view(
                ColumnKey::Day(*d),
                list.map(|l| l.id),
                calendar::weekday_label(*d).to_string(),
                d.format("%-d %b").to_string(),
                &mine,
                &authors,
            ));
        }

        let mut lists = Vec::new();
        for l in &custom {
            let mine: Vec<_> = tasks.iter().filter(|t| t.list_id == l.id).cloned().collect();
            lists.push(views::column_view(
                ColumnKey::List(l.id),
                Some(l.id),
                l.name.clone().unwrap_or_else(|| "List".into()),
                String::new(),
                &mine,
                &authors,
            ));
        }

        Ok(Some((board, authors, all_boards, days, lists, show_colour)))
    });

    let Ok(Some((board, users, all_boards, days, lists, show_colour))) = loaded else {
        return Redirect::to("/").into_response();
    };

    render(
        &state,
        "week.html",
        minijinja::context! {
            theme => me.theme.clone(),
            me => me,
            users => users,
            board => board,
            // column.html is included here and rendered standalone by the
            // fragment route; both must supply the same names.
            board_id => board.id,
            boards => all_boards,
            days => days,
            lists => lists,
            show_colour => show_colour,
            monday => calendar::fmt(monday),
            prev_week => calendar::fmt(monday - chrono::Duration::days(7)),
            next_week => calendar::fmt(monday + chrono::Duration::days(7)),
            this_week => calendar::fmt(calendar::monday_of(calendar::today())),
            range_label => format!(
                "{} – {}",
                dates[0].format("%-d %b"),
                dates.last().unwrap().format("%-d %b %Y")
            ),
        },
    )
}

/// One column, re-rendered. Every mutation returns this, and in phase 6 the SSE
/// trigger refetches exactly this — one code path, so push and fetch can never
/// disagree.
async fn column_fragment(
    State(state): State<AppState>,
    Path((board_id, key)): Path<(i64, String)>,
) -> Response {
    let Some(key) = ColumnKey::parse(&key) else {
        return (axum::http::StatusCode::BAD_REQUEST, "bad column key\n").into_response();
    };
    render_column(&state, board_id, key, move_completed(&state))
}

/// Shared by the fragment route and every mutation handler.
pub fn render_column(
    state: &AppState,
    board_id: i64,
    key: ColumnKey,
    move_completed: bool,
) -> Response {
    let loaded = state.db.with(|conn| {
        let show_colour = queries::board_member_count(conn, board_id)? > 1;
        let col = views::load_column(conn, board_id, key, move_completed)?;
        Ok((col, show_colour))
    });

    match loaded {
        Ok((col, show_colour)) => render(
            state,
            "column.html",
            minijinja::context! {
                col => col,
                board_id => board_id,
                show_colour => show_colour,
            },
        ),
        Err(e) => {
            tracing::error!(error = %e, "column load failed");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "database error\n").into_response()
        }
    }
}
