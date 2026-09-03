//! The week grid and the column fragment both views share.

use anyhow::Context;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect};
use axum::routing::get;
use axum::Router;
use axum_extra::extract::CookieJar;

use crate::calendar;
use chrono::Datelike;
use crate::error::AppResult;
use crate::routes::{current_user, move_completed, render};
use crate::views::{self, density_for, ColumnKey, COMPACT, FULL};
use serde::Deserialize;
use crate::{queries, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/b/{board}/w/{monday}", get(week))
        .route("/b/{board}/4w/{monday}", get(four_weeks))
        .route("/b/{board}/col/{key}", get(column_fragment))
        .route("/b/{board}/day/{date}", get(day_panel))
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

/// Everything a grid page renders, whichever grid it is.
struct Grid {
    board: crate::models::Board,
    users: Vec<crate::models::User>,
    boards: Vec<crate::models::Board>,
    days: Vec<views::ColumnView>,
    lists: Vec<views::ColumnView>,
    show_colour: bool,
}

/// Loads a grid over an arbitrary run of dates.
///
/// This is the "one renderer chain" of D18: the week and month pages differ
/// only in which dates they ask for, how each day is labelled, and how many
/// rows a cell has room for. Everything below this line — the queries, the
/// column building, the markup, and therefore drag & drop, live updates and
/// the compare-and-swap editor — is shared verbatim.
///
/// `Ok(None)` means the board does not exist, which is a redirect rather than
/// an error.
fn load_grid(
    state: &AppState,
    board_id: i64,
    dates: &[chrono::NaiveDate],
    label: impl Fn(chrono::NaiveDate) -> (String, String),
    page_density: &str,
    move_completed: bool,
) -> anyhow::Result<Option<Grid>> {
    state.db.with(|conn| {
        let Some(board) = queries::board(conn, board_id)? else {
            return Ok(None);
        };
        let users = queries::users(conn).context("loading authors")?;
        let boards = queries::boards(conn).context("loading board list")?;

        // Spines only mean something where more than one person writes (D11).
        let show_colour = queries::board_member_count(conn, board_id)? > 1;

        let from = calendar::fmt(dates[0]);
        let to = calendar::fmt(*dates.last().expect("a grid always spans some dates"));
        let day_lists =
            queries::day_lists_in_range(conn, board_id, &from, &to).context("loading day lists")?;
        let custom = queries::custom_lists(conn, board_id).context("loading custom lists")?;

        // One query for every column on screen.
        let ids: Vec<i64> = day_lists.iter().chain(&custom).map(|l| l.id).collect();
        let tasks = queries::tasks_for_lists(conn, &ids, move_completed)
            .context("loading tasks for the grid")?;

        let of_list = |list_id: i64| -> Vec<crate::models::Task> {
            tasks.iter().filter(|t| t.list_id == list_id).cloned().collect()
        };

        let days = dates
            .iter()
            .map(|d| {
                let ds = calendar::fmt(*d);
                let list = day_lists.iter().find(|l| l.date.as_deref() == Some(&ds));
                let mine = list.map(|l| of_list(l.id)).unwrap_or_default();
                let (heading, subheading) = label(*d);
                let key = ColumnKey::Day(*d);
                views::column_view(
                    key,
                    list.map(|l| l.id),
                    heading,
                    subheading,
                    &mine,
                    &users,
                    density_for(key, page_density),
                )
            })
            .collect();

        let lists = custom
            .iter()
            .map(|l| {
                let key = ColumnKey::List(l.id);
                views::column_view(
                    key,
                    Some(l.id),
                    l.name.clone().unwrap_or_else(|| "List".into()),
                    String::new(),
                    &of_list(l.id),
                    &users,
                    // Always full: a custom list is never a calendar cell.
                    density_for(key, page_density),
                )
            })
            .collect();

        Ok(Some(Grid { board, users, boards, days, lists, show_colour }))
    })
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
    let grid = load_grid(
        &state,
        board_id,
        &dates,
        |d| {
            (
                calendar::weekday_label(d).to_string(),
                d.format("%-d %b").to_string(),
            )
        },
        FULL, // a week column has the height of the screen; nothing is hidden
        move_completed(&state)?,
    )
    .with_context(|| format!("loading board {board_id} for the week of {monday}"))?;

    let Some(grid) = grid else {
        return Ok(Redirect::to("/").into_response());
    };

    render(
        &state,
        "week.html",
        minijinja::context! {
            theme => me.theme,
            me => me,
            users => grid.users,
            board => grid.board,
            // column.html is included here and rendered standalone by the
            // fragment route; both must supply the same names.
            board_id => board_id,
            // The version this page reflects; polling resumes from here so a
            // change committed between render and first poll is not missed.
            change_seq => state.changes.current_seq(),
            boards => grid.boards,
            days => grid.days,
            lists => grid.lists,
            show_colour => grid.show_colour,
            // Both grids share one topbar partial, so every link it needs is
            // built here rather than assembled in the template.
            self_url => format!("/b/{board_id}/w/{}", calendar::fmt(monday)),
            prev_url => format!("/b/{board_id}/w/{}", calendar::fmt(monday - chrono::Duration::days(7))),
            next_url => format!("/b/{board_id}/w/{}", calendar::fmt(monday + chrono::Duration::days(7))),
            today_url => format!("/b/{board_id}/w/{}", calendar::fmt(calendar::monday_of(calendar::today()))),
            switch_url => format!("/b/{board_id}/4w/{}", calendar::fmt(monday)),
            switch_label => "4 weeks",
            board_path => format!("/w/{}", calendar::fmt(monday)),
            range_label => format!(
                "{} – {}",
                dates[0].format("%-d %b"),
                dates.last().expect("a week has seven days").format("%-d %b %Y")
            ),
        },
    )
}

/// The four-week grid. Same columns, same behaviour — only the range of dates
/// and how much of each cell fits are different (D18).
///
/// Four weeks rather than a calendar month so the grid is always 4x7: a month
/// view is 35 cells some months and 42 others, which makes rows change height
/// as you page through it.
async fn four_weeks(
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

    let dates = calendar::weeks_from(monday, calendar::VIEW_WEEKS);
    let grid = load_grid(
        &state,
        board_id,
        &dates,
        // A cell is headed by its day number. The month is added on the 1st,
        // which is the only place four weeks of numbers would be ambiguous.
        |d| {
            let month = if d.day() == 1 {
                d.format("%b").to_string()
            } else {
                String::new()
            };
            (d.format("%-d").to_string(), month)
        },
        COMPACT,
        move_completed(&state)?,
    )
    .with_context(|| format!("loading board {board_id} for four weeks from {monday}"))?;

    let Some(grid) = grid else {
        return Ok(Redirect::to("/").into_response());
    };

    let span = chrono::Duration::days(calendar::VIEW_WEEKS * 7);
    let last = *dates.last().expect("four weeks is never empty");

    render(
        &state,
        "weeks.html",
        minijinja::context! {
            theme => me.theme,
            me => me,
            users => grid.users,
            board => grid.board,
            board_id => board_id,
            change_seq => state.changes.current_seq(),
            boards => grid.boards,
            cells => grid.days,
            lists => grid.lists,
            show_colour => grid.show_colour,
            weekday_names => ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
            self_url => format!("/b/{board_id}/4w/{}", calendar::fmt(monday)),
            prev_url => format!("/b/{board_id}/4w/{}", calendar::fmt(monday - span)),
            next_url => format!("/b/{board_id}/4w/{}", calendar::fmt(monday + span)),
            today_url => format!("/b/{board_id}/4w/{}", calendar::fmt(calendar::monday_of(calendar::today()))),
            switch_url => format!("/b/{board_id}/w/{}", calendar::fmt(monday)),
            switch_label => "Week",
            board_path => format!("/4w/{}", calendar::fmt(monday)),
            range_label => format!(
                "{} – {}",
                monday.format("%-d %b"),
                last.format("%-d %b %Y")
            ),
        },
    )
}

#[derive(Deserialize)]
pub struct DensityQuery {
    /// Carried on the column's own refresh URL, so a cell re-fetched from the
    /// month grid comes back sized for a month cell.
    #[serde(default)]
    density: Option<String>,
}

async fn column_fragment(
    State(state): State<AppState>,
    Path((board_id, key)): Path<(i64, String)>,
    Query(q): Query<DensityQuery>,
) -> AppResult {
    let Some(key) = ColumnKey::parse(&key) else {
        return Ok((axum::http::StatusCode::BAD_REQUEST, "bad column key\n").into_response());
    };
    let page = if q.density.as_deref() == Some(COMPACT) { COMPACT } else { FULL };
    render_column(&state, board_id, key, move_completed(&state)?, page)
}

/// One day in full, opened from a month cell's "+N more". Deliberately the
/// same column markup at full density, so everything in it — adding, editing,
/// dragging — behaves exactly as it does in the week grid.
async fn day_panel(
    State(state): State<AppState>,
    Path((board_id, date)): Path<(i64, String)>,
) -> AppResult {
    let Some(date) = calendar::parse(&date) else {
        return Ok((axum::http::StatusCode::BAD_REQUEST, "bad date\n").into_response());
    };
    let key = ColumnKey::Day(date);

    // Read the setting *before* taking the connection: `move_completed` opens
    // its own `db.with`, and the mutex guarding the single connection is not
    // reentrant, so calling it inside the closure deadlocks the whole process.
    let move_completed = move_completed(&state)?;

    let (col, show_colour) = state
        .db
        .with(|conn| -> anyhow::Result<_> {
            let show_colour = queries::board_member_count(conn, board_id)? > 1;
            let col = views::load_column(conn, board_id, key, move_completed, FULL)?;
            Ok((col, show_colour))
        })
        .with_context(|| format!("loading the day panel for {date}"))?;

    render(
        &state,
        "day_panel.html",
        minijinja::context! {
            col => col,
            board_id => board_id,
            show_colour => show_colour,
            day_label => date.format("%A %-d %B").to_string(),
        },
    )
}

/// Shared by the fragment route and every mutation handler.
/// `page_density` is the grid the request came from; the column resolves its
/// own from that, so a custom list mutated inside the four-week grid still
/// comes back full size.
pub fn render_column(
    state: &AppState,
    board_id: i64,
    key: ColumnKey,
    move_completed: bool,
    page_density: &str,
) -> AppResult {
    let density = density_for(key, page_density);
    let (col, show_colour) = state
        .db
        .with(|conn| -> anyhow::Result<_> {
            let show_colour = queries::board_member_count(conn, board_id)? > 1;
            let col = views::load_column(conn, board_id, key, move_completed, density)?;
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
