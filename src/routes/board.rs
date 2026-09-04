//! The week grid and the column fragment both views share.

use anyhow::Context;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect};
use axum::routing::get;
use axum_extra::extract::CookieJar;

use crate::calendar;
use crate::error::AppResult;
use crate::i18n::{self, Locale};
use crate::routes::{current_user, locale, render, theme_cycle};
use crate::views::{self, COMPACT, ColumnKey, FULL, density_for};
use crate::{AppState, queries};
use serde::Deserialize;

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

    // Straight into the grid this board is read in, rather than always the
    // week and a toggle away from where you were.
    let view = state
        .db
        .with(|conn| queries::remembered_view(conn, me.id, board.id))
        .with_context(|| format!("loading the remembered view of board {}", board.id))?;
    let monday = calendar::fmt(calendar::monday_of(calendar::today()));
    let path = view.as_deref().unwrap_or(queries::VIEW_WEEK);
    Ok(Redirect::to(&format!("/b/{}/{path}/{monday}", board.id)).into_response())
}

/// Applies the overdue rule before a grid is drawn.
///
/// Lazily, on page load, rather than from a scheduled job: a household app that
/// is idle for days should not need a timer thread and a timezone-aware cron to
/// stay correct, and looking at the board is exactly the moment the answer has
/// to be right. The sweep is idempotent, so doing it on every render is safe
/// and usually finds nothing.
fn sweep_overdue(state: &AppState, board_id: i64, loc: &Locale) -> anyhow::Result<()> {
    let today = calendar::fmt(calendar::today());
    // The list is named when it is first made, in the language in force then.
    // It is an ordinary list afterwards: renaming it is the way to change it,
    // and switching language later does not rename what already exists.
    let list_name = loc.t(queries::OVERDUE_LIST_NAME).to_string();
    let touched = state.db.transaction(|tx| -> anyhow::Result<_> {
        let action = queries::get_setting(tx, queries::OVERDUE_ACTION, queries::OVERDUE_LIST)?;
        Ok(queries::sweep_overdue(
            tx, board_id, &today, &action, &list_name,
        )?)
    })?;

    // Tell other browsers which columns moved under them.
    for list_id in touched {
        if let Some(key) = state.db.with(|conn| -> anyhow::Result<_> {
            Ok(queries::list(conn, list_id)?.and_then(|l| match &l.date {
                Some(d) => calendar::parse(d).map(ColumnKey::Day),
                None => Some(ColumnKey::List(l.id)),
            }))
        })? {
            state.changes.record(board_id, &key);
        }
    }
    Ok(())
}

/// "31 Aug – 6 Sep 2026". The year only on the far end, where it settles which
/// year the whole range is in without repeating itself.
fn range_label(from: chrono::NaiveDate, to: chrono::NaiveDate, loc: &Locale) -> String {
    use chrono::Datelike;
    let end = i18n::fill(
        loc.t("{day} {month} {year}"),
        &[
            ("day", &to.day().to_string()),
            ("month", loc.t(calendar::month_abbrev(to))),
            ("year", &to.year().to_string()),
        ],
    );
    format!("{} – {end}", views::day_label(from, loc))
}

/// The boards to offer, given the one being looked at.
///
/// Yours, plus the one you are standing on if it is not among them — which
/// happens when a link or a switch of identity lands you on someone else's
/// board. Leaving it out would show the picker naming a board you are not on.
fn pickable(
    mut mine: Vec<crate::models::Board>,
    current: &crate::models::Board,
) -> Vec<crate::models::Board> {
    if !mine.iter().any(|b| b.id == current.id) {
        mine.push(current.clone());
    }
    mine
}

/// One entry in the board picker: the board, and the URL that opens it.
#[derive(serde::Serialize)]
struct BoardLink {
    id: i64,
    name: String,
    url: String,
}

/// Board-switch links, each opening its board in the grid that board is read
/// in (per reader), on the period you are looking at now.
///
/// A board this person has not opened yet keeps the view you are in, so
/// switching boards never rearranges the screen unasked; the first toggle
/// there is what settles it. The four-week grid must start on a Monday for its
/// rows to line up, so a link into it snaps, while a week link keeps the exact
/// day you had scrolled to.
fn board_links(
    boards: &[crate::models::Board],
    board_views: &std::collections::HashMap<i64, String>,
    current_view: &str,
    start: chrono::NaiveDate,
) -> Vec<BoardLink> {
    boards
        .iter()
        .map(|b| {
            let view = board_views.get(&b.id).map_or(current_view, String::as_str);
            let url = if view == queries::VIEW_FOUR_WEEKS {
                format!(
                    "/b/{}/{}/{}",
                    b.id,
                    queries::VIEW_FOUR_WEEKS,
                    calendar::fmt(calendar::monday_of(start))
                )
            } else {
                format!(
                    "/b/{}/{}/{}",
                    b.id,
                    queries::VIEW_WEEK,
                    calendar::fmt(start)
                )
            };
            BoardLink {
                id: b.id,
                name: b.name.clone(),
                url,
            }
        })
        .collect()
}

/// Records which grid this person reads this board in, so the picker and `/`
/// can land them back in it.
///
/// Written only when it changes: a page render is otherwise a read, and every
/// page load dirtying the write-ahead log to store the value already there
/// would be a write nobody asked for.
fn remember_view(state: &AppState, me_id: i64, board_id: i64, view: &str, grid: &Grid) {
    if grid.board_views.get(&board_id).map(String::as_str) == Some(view) {
        return;
    }
    if let Err(e) = state
        .db
        .with(|conn| queries::remember_view(conn, me_id, board_id, view))
    {
        // Not worth failing a page render over; the view is a convenience.
        tracing::warn!(error = %e, board_id, view, "could not remember the view");
    }
}

/// Everything a grid page renders, whichever grid it is.
struct Grid {
    board: crate::models::Board,
    users: Vec<crate::models::User>,
    boards: Vec<crate::models::Board>,
    /// This reader's view for each board, for the picker's links.
    board_views: std::collections::HashMap<i64, String>,
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
    me: &crate::models::User,
    board_id: i64,
    dates: &[chrono::NaiveDate],
    label: impl Fn(chrono::NaiveDate) -> (String, String),
    page_density: &str,
) -> anyhow::Result<Option<Grid>> {
    state.db.with(|conn| {
        let move_completed = queries::get_flag(conn, queries::MOVE_COMPLETED, true)?;
        let Some(board) = queries::board(conn, board_id)? else {
            return Ok(None);
        };
        let users = queries::users(conn).context("loading authors")?;
        // Your boards, not everyone's: a board you are not a member of is
        // meant to be out of the way (D7/D8), and a picker listing all of them
        // is the opposite of that.
        let boards = queries::boards_for_user(conn, me.id).context("loading board list")?;
        let board_views =
            queries::remembered_views(conn, me.id).context("loading remembered views")?;

        // Spines only mean something where more than one person writes (D11).
        let show_colour = queries::board_member_count(conn, board_id)? > 1;

        // An empty range renders nothing, the same answer a missing board
        // gets — and it means no date below has to be reached for by index.
        let Some((from, to)) = calendar::ends(dates) else {
            return Ok(None);
        };
        let from = calendar::fmt(from);
        let to = calendar::fmt(to);
        let day_lists =
            queries::day_lists_in_range(conn, board_id, &from, &to).context("loading day lists")?;
        let custom = queries::custom_lists(conn, board_id).context("loading custom lists")?;

        // One query for every column on screen.
        let ids: Vec<i64> = day_lists.iter().chain(&custom).map(|l| l.id).collect();
        let tasks = queries::tasks_for_lists(conn, &ids, move_completed)
            .context("loading tasks for the grid")?;

        let of_list = |list_id: i64| -> Vec<crate::models::Task> {
            tasks
                .iter()
                .filter(|t| t.list_id == list_id)
                .cloned()
                .collect()
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

        Ok(Some(Grid {
            boards: pickable(boards, &board),
            board,
            users,
            board_views,
            days,
            lists,
            show_colour,
        }))
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
    // Deliberately not snapped to a Monday. The arrows step a day at a time,
    // which only means anything if the window can start anywhere; "Today"
    // returns to the tidy Monday-aligned week.
    let Some(start) = calendar::parse(&monday) else {
        return Ok(Redirect::to("/").into_response());
    };

    let loc = locale(&state);
    let dates = calendar::week_of(start);
    sweep_overdue(&state, board_id, &loc).context("applying the overdue rule")?;
    let grid = load_grid(
        &state,
        &me,
        board_id,
        &dates,
        |d| {
            (
                loc.t(calendar::weekday_label(d)).to_string(),
                views::day_label(d, &loc),
            )
        },
        FULL, // a week column has the height of the screen; nothing is hidden
    )
    .with_context(|| format!("loading board {board_id} for the week from {start}"))?;

    let Some(grid) = grid else {
        return Ok(Redirect::to("/").into_response());
    };
    remember_view(&state, me.id, board_id, queries::VIEW_WEEK, &grid);
    let boards = board_links(&grid.boards, &grid.board_views, queries::VIEW_WEEK, start);

    render(
        &state,
        &loc,
        "week.html",
        minijinja::context! {
            theme => me.theme,
            next_theme => theme_cycle(&me.theme).0,
            theme_symbol => theme_cycle(&me.theme).1,
            me => me,
            users => grid.users,
            board => grid.board,
            // column.html is included here and rendered standalone by the
            // fragment route; both must supply the same names.
            board_id => board_id,
            // The version this page reflects; polling resumes from here so a
            // change committed between render and first poll is not missed.
            change_seq => state.changes.current_seq(),
            boards => boards,
            days => grid.days,
            lists => grid.lists,
            show_colour => grid.show_colour,
            // Both grids share one topbar partial, so every link it needs is
            // built here rather than assembled in the template.
            self_url => format!("/b/{board_id}/w/{}", calendar::fmt(start)),
            // One day at a time, so you can slide the window onto whatever
            // stretch you are actually planning.
            prev_url => format!("/b/{board_id}/w/{}", calendar::fmt(calendar::minus_days(start, 1))),
            next_url => format!("/b/{board_id}/w/{}", calendar::fmt(calendar::plus_days(start, 1))),
            today_url => format!("/b/{board_id}/w/{}", calendar::fmt(calendar::monday_of(calendar::today()))),
            // The four-week grid must start on a Monday for its rows to line
            // up, so switching snaps to the Monday of the week you are on.
            switch_url => format!("/b/{board_id}/4w/{}", calendar::fmt(calendar::monday_of(start))),
            switch_label => loc.t("4 weeks"),
            // The week is `start` plus six days by construction, so its ends
            // are known without looking them up in the range.
            range_label => range_label(start, calendar::plus_days(start, 6), &loc),
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

    let loc = locale(&state);
    let dates = calendar::weeks_from(monday, calendar::VIEW_WEEKS);
    sweep_overdue(&state, board_id, &loc).context("applying the overdue rule")?;
    let grid = load_grid(
        &state,
        &me,
        board_id,
        &dates,
        // Day number plus month, always: four weeks can span three months, and
        // a bare number does not say which one you are looking at.
        |d| {
            (
                d.format("%-d").to_string(),
                loc.t(calendar::month_abbrev(d)).to_string(),
            )
        },
        COMPACT,
    )
    .with_context(|| format!("loading board {board_id} for four weeks from {monday}"))?;

    let Some(grid) = grid else {
        return Ok(Redirect::to("/").into_response());
    };
    remember_view(&state, me.id, board_id, queries::VIEW_FOUR_WEEKS, &grid);
    let boards = board_links(
        &grid.boards,
        &grid.board_views,
        queries::VIEW_FOUR_WEEKS,
        monday,
    );

    // The grid is four whole weeks from `monday`, so its last day is a known
    // distance away rather than something to fetch out of the range.
    let last = calendar::minus_days(calendar::plus_weeks(monday, 4), 1);

    render(
        &state,
        &loc,
        "weeks.html",
        minijinja::context! {
            theme => me.theme,
            next_theme => theme_cycle(&me.theme).0,
            theme_symbol => theme_cycle(&me.theme).1,
            me => me,
            users => grid.users,
            board => grid.board,
            board_id => board_id,
            change_seq => state.changes.current_seq(),
            boards => boards,
            cells => grid.days,
            lists => grid.lists,
            show_colour => grid.show_colour,
            // The header row names the seven weekdays, which the first week of
            // the grid supplies in order.
            weekday_names => dates
                .iter()
                .take(7)
                .map(|d| loc.t(calendar::weekday_label(*d)))
                .collect::<Vec<_>>(),
            self_url => format!("/b/{board_id}/4w/{}", calendar::fmt(monday)),
            prev_url => format!("/b/{board_id}/4w/{}", calendar::fmt(calendar::minus_weeks(monday, 1))),
            next_url => format!("/b/{board_id}/4w/{}", calendar::fmt(calendar::plus_weeks(monday, 1))),
            today_url => format!("/b/{board_id}/4w/{}", calendar::fmt(calendar::monday_of(calendar::today()))),
            switch_url => format!("/b/{board_id}/w/{}", calendar::fmt(monday)),
            switch_label => loc.t("Week"),
            range_label => range_label(monday, last, &loc),
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
    let page = if q.density.as_deref() == Some(COMPACT) {
        COMPACT
    } else {
        FULL
    };
    render_column(&state, &locale(&state), board_id, key, page)
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
    let loc = locale(&state);

    let (col, show_colour) = state
        .db
        .with(|conn| -> anyhow::Result<_> {
            let show_colour = queries::board_member_count(conn, board_id)? > 1;
            let col = views::load_column(conn, board_id, key, FULL, &loc)?;
            Ok((col, show_colour))
        })
        .with_context(|| format!("loading the day panel for {date}"))?;

    render(
        &state,
        &loc,
        "day_panel.html",
        minijinja::context! {
            col => col,
            board_id => board_id,
            show_colour => show_colour,
            day_label => i18n::fill(
                loc.t("{weekday} {day} {month}"),
                &[
                    ("weekday", loc.t(calendar::weekday_name(date))),
                    ("day", &chrono::Datelike::day(&date).to_string()),
                    ("month", loc.t(calendar::month_name(date))),
                ],
            ),
        },
    )
}

/// Shared by the fragment route and every mutation handler.
/// `page_density` is the grid the request came from; the column resolves its
/// own from that, so a custom list mutated inside the four-week grid still
/// comes back full size.
pub fn render_column(
    state: &AppState,
    loc: &Locale,
    board_id: i64,
    key: ColumnKey,
    page_density: &str,
) -> AppResult {
    let density = density_for(key, page_density);
    let (col, show_colour) = state
        .db
        .with(|conn| -> anyhow::Result<_> {
            let show_colour = queries::board_member_count(conn, board_id)? > 1;
            let col = views::load_column(conn, board_id, key, density, loc)?;
            Ok((col, show_colour))
        })
        .with_context(|| format!("loading column {} of board {board_id}", key.as_string()))?;

    render(
        state,
        loc,
        "column.html",
        minijinja::context! {
            col => col,
            board_id => board_id,
            show_colour => show_colour,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Board;
    use std::collections::HashMap;

    fn boards() -> Vec<Board> {
        vec![
            Board {
                id: 1,
                name: "Home".into(),
            },
            Board {
                id: 2,
                name: "Planning".into(),
            },
        ]
    }

    fn d(s: &str) -> chrono::NaiveDate {
        calendar::parse(s).unwrap()
    }

    /// 2026-09-03 is a Thursday, so a link into the four-week grid has to snap
    /// back to Monday the 31st while a week link keeps the Thursday.
    /// The picker lists your boards. It also has to name the board you are
    /// actually on, or it would sit there showing a board you are not looking
    /// at — which is what happens the moment a link, or a switch of identity,
    /// puts you somewhere that is not yours.
    #[test]
    fn the_picker_offers_your_boards_and_the_one_you_are_on() {
        let mine = boards();
        let visiting = Board {
            id: 9,
            name: "Someone else's".into(),
        };

        let offered = pickable(mine.clone(), &visiting);
        assert_eq!(
            offered.iter().map(|b| b.id).collect::<Vec<_>>(),
            [1, 2, 9],
            "the board being visited is added, at the end"
        );

        let offered = pickable(mine.clone(), &mine[0]);
        assert_eq!(
            offered.iter().map(|b| b.id).collect::<Vec<_>>(),
            [1, 2],
            "a board of your own is not listed twice"
        );

        assert_eq!(
            pickable(Vec::new(), &visiting).len(),
            1,
            "someone with no boards of their own still sees where they are"
        );
    }

    #[test]
    fn each_board_opens_in_the_view_it_is_read_in() {
        let mut views = HashMap::new();
        views.insert(2, queries::VIEW_FOUR_WEEKS.to_string());

        let links = board_links(&boards(), &views, queries::VIEW_WEEK, d("2026-09-03"));
        assert_eq!(links[1].url, "/b/2/4w/2026-08-31");
        assert_eq!(
            links[0].url, "/b/1/w/2026-09-03",
            "a board with no remembered view keeps the view you are in"
        );
    }

    #[test]
    fn a_board_never_opened_follows_the_page_you_are_on() {
        let links = board_links(
            &boards(),
            &HashMap::new(),
            queries::VIEW_FOUR_WEEKS,
            d("2026-08-31"),
        );
        for link in &links {
            assert!(
                link.url.contains("/4w/"),
                "{} should stay in four weeks",
                link.url
            );
        }
    }

    /// A stored value that is neither of the two spellings must not produce a
    /// URL that routes nowhere.
    #[test]
    fn an_unrecognised_stored_view_falls_back_to_the_week() {
        let mut views = HashMap::new();
        views.insert(1, "month".to_string());
        let links = board_links(&boards(), &views, queries::VIEW_FOUR_WEEKS, d("2026-08-31"));
        assert_eq!(links[0].url, "/b/1/w/2026-08-31");
    }
}
