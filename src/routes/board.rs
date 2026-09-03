//! The week grid and the column fragment both views share.

use anyhow::Context;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Redirect};
use axum::routing::get;
use axum::Router;
use axum_extra::extract::CookieJar;

use crate::calendar;
use crate::error::AppResult;
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
async fn index(State(state): State<AppState>, jar: CookieJar) -> AppResult {
    let users = state.db.with(queries::users).context("loading users")?;
    if users.is_empty() {
        return Ok(Redirect::to("/settings").into_response());
    }

    let Some(me) = current_user(&state, &jar)? else {
        return Ok(Redirect::to("/pick").into_response());
    };

    let boards = state
        .db
        .with(|conn| queries::boards_for_user(conn, me.id))
        .with_context(|| format!("loading boards for user {}", me.id))?;
    let Some(board) = boards.first() else {
        return Ok(Redirect::to("/settings").into_response());
    };

    let monday = calendar::fmt(calendar::monday_of(calendar::today()));
    Ok(Redirect::to(&format!("/b/{}/w/{monday}", board.id)).into_response())
}

async fn week(
    State(state): State<AppState>,
    jar: CookieJar,
    Path((board_id, monday)): Path<(i64, String)>,
) -> AppResult {
    let Some(me) = current_user(&state, &jar)? else {
        return Ok(Redirect::to("/pick").into_response());
    };
    let Some(monday) = calendar::parse(&monday).map(calendar::monday_of) else {
        return Ok(Redirect::to("/").into_response());
    };

    let dates = calendar::week_of(monday);
    let move_completed = move_completed(&state)?;

    // `None` means the board is gone, which is a redirect. A query that fails
    // is an error and must not be flattened into the same answer.
    let loaded = state
        .db
        .with(|conn| -> anyhow::Result<_> {
            let Some(board) = queries::board(conn, board_id)? else {
                return Ok(None);
            };
            let authors = queries::users(conn).context("loading authors")?;
            let all_boards = queries::boards(conn).context("loading board list")?;

            // Spines only mean something where more than one person writes (D11).
            let show_colour = queries::board_member_count(conn, board_id)? > 1;

            let from = calendar::fmt(dates[0]);
            let to = calendar::fmt(*dates.last().expect("a week has seven days"));
            let day_lists = queries::day_lists_in_range(conn, board_id, &from, &to)
                .context("loading day lists")?;
            let custom = queries::custom_lists(conn, board_id).context("loading custom lists")?;

            // One query for every column on screen.
            let ids: Vec<i64> = day_lists.iter().chain(&custom).map(|l| l.id).collect();
            let tasks = queries::tasks_for_lists(conn, &ids, move_completed)
                .context("loading tasks for the week")?;

            let days = dates
                .iter()
                .map(|d| {
                    let ds = calendar::fmt(*d);
                    let list = day_lists.iter().find(|l| l.date.as_deref() == Some(&ds));
                    let mine: Vec<_> = list
                        .map(|l| {
                            tasks
                                .iter()
                                .filter(|t| t.list_id == l.id)
                                .cloned()
                                .collect()
                        })
                        .unwrap_or_default();
                    views::column_view(
                        ColumnKey::Day(*d),
                        list.map(|l| l.id),
                        calendar::weekday_label(*d).to_string(),
                        d.format("%-d %b").to_string(),
                        &mine,
                        &authors,
                    )
                })
                .collect::<Vec<_>>();

            let lists = custom
                .iter()
                .map(|l| {
                    let mine: Vec<_> =
                        tasks.iter().filter(|t| t.list_id == l.id).cloned().collect();
                    views::column_view(
                        ColumnKey::List(l.id),
                        Some(l.id),
                        l.name.clone().unwrap_or_else(|| "List".into()),
                        String::new(),
                        &mine,
                        &authors,
                    )
                })
                .collect::<Vec<_>>();

            Ok(Some((board, authors, all_boards, days, lists, show_colour)))
        })
        .with_context(|| format!("loading board {board_id} for the week of {monday}"))?;

    let Some((board, users, all_boards, days, lists, show_colour)) = loaded else {
        return Ok(Redirect::to("/").into_response());
    };

    render(
        &state,
        "week.html",
        minijinja::context! {
            theme => me.theme,
            me => me,
            users => users,
            board => board,
            // column.html is included here and rendered standalone by the
            // fragment route; both must supply the same names.
            board_id => board.id,
            // The sequence this page reflects; polling resumes from here so a
            // change committed between render and first poll is not missed.
            change_seq => state.changes.current_seq(),
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
                dates.last().expect("a week has seven days").format("%-d %b %Y")
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
) -> AppResult {
    let Some(key) = ColumnKey::parse(&key) else {
        return Ok((axum::http::StatusCode::BAD_REQUEST, "bad column key\n").into_response());
    };
    render_column(&state, board_id, key, move_completed(&state)?)
}

/// Shared by the fragment route and every mutation handler.
pub fn render_column(
    state: &AppState,
    board_id: i64,
    key: ColumnKey,
    move_completed: bool,
) -> AppResult {
    let (col, show_colour) = state
        .db
        .with(|conn| -> anyhow::Result<_> {
            let show_colour = queries::board_member_count(conn, board_id)? > 1;
            let col = views::load_column(conn, board_id, key, move_completed)?;
            Ok((col, show_colour))
        })
        .with_context(|| format!("loading column {} of board {board_id}", key.as_string()))?;

    render(
        state,
        "column.html",
        minijinja::context! {
            col => col,
            board_id => board_id,
            show_colour => show_colour,
        },
    )
}
