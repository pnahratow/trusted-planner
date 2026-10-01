//! Users, boards, membership, custom lists, and the identity picker.

use anyhow::Context;
use axum::Router;
use axum::extract::State;
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
// serde_urlencoded (axum::Form) cannot deserialise repeated keys into a Vec,
// which is exactly what a set of membership checkboxes posts.
use axum_extra::extract::CookieJar;
use axum_extra::extract::Form;
use axum_extra::extract::cookie::{Cookie, SameSite};
use serde::Deserialize;

use crate::error::AppResult;
use crate::routes::{IDENTITY_COOKIE, current_user, locale, render, saved};
use crate::{AppState, queries};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/settings", get(page))
        .route("/settings/user", post(user_action))
        .route("/settings/board", post(board_action))
        .route("/settings/list", post(list_action))
        .route("/settings/display", post(display_action))
        .route("/settings/theme", post(theme_action))
        .route("/whoami", post(whoami))
        .route("/pick", get(pick_page))
}

/// A small fixed palette: colour means identity (D11), and picking from a
/// palette keeps identities visually distinct without a colour-picker widget.
const PALETTE: &[&str] = &[
    FIRST_COLOUR,
    "#e0533d",
    "#2ea36b",
    "#b552d6",
    "#d99a1f",
    "#0e9bb5",
    "#d64f8a",
    "#5b6470",
];

/// Named so the fallback below can reach it without indexing the palette.
const FIRST_COLOUR: &str = "#3563e9";

async fn page(State(state): State<AppState>, jar: CookieJar) -> AppResult {
    let me = current_user(&state, &jar)?;

    let pinned = crate::routes::has_own_identity(&state, &jar)?;

    let (users, boards, move_completed, overdue_action, language, stranded, default_user) = state
        .db
        .with(|conn| -> anyhow::Result<_> {
            let users = queries::users(conn)?;
            // Resolved rather than read back raw, so the page marks whoever
            // would actually answer — including when the stored id names
            // somebody who has since been removed.
            let default_user = queries::default_user(conn)?.map(|u| u.id);
            // Always nought unless something is wrong; see `stranded_tasks`.
            let stranded = queries::stranded_tasks(conn)?;
            let boards = queries::boards(conn)?;
            let move_completed = queries::get_flag(conn, queries::MOVE_COMPLETED, true)?;
            let overdue_action =
                queries::get_setting(conn, queries::OVERDUE_ACTION, queries::OVERDUE_LIST)?;
            let language = queries::get_setting(conn, queries::LANGUAGE, crate::i18n::DEFAULT)?;
            let mut board_rows = Vec::new();
            for b in &boards {
                let members = queries::board_member_ids(conn, b.id)?;
                let lists = queries::custom_lists(conn, b.id)?;
                board_rows.push(minijinja::context! {
                    id => b.id,
                    name => b.name.clone(),
                    members => members,
                    lists => lists,
                });
            }
            Ok((
                users,
                board_rows,
                move_completed,
                overdue_action,
                language,
                stranded,
                default_user,
            ))
        })
        .context("loading the settings page")?;

    let _theme = me
        .as_ref()
        .map_or_else(|| "system".to_string(), |u| u.theme.clone());
    render(
        &state,
        &locale(&state),
        "settings.html",
        minijinja::context! {
            users => users,
            boards => boards,
            palette => PALETTE,
            move_completed => move_completed,
            overdue_action => overdue_action,
            language => language,
            languages => crate::i18n::LANGUAGES,
            stranded => stranded,
            default_user => default_user,
            pinned => pinned,
            me => me,
            theme => me.as_ref().map_or_else(|| "system".into(), |u| u.theme.clone()),
        },
    )
}

#[derive(Deserialize)]
struct UserForm {
    action: String,
    #[serde(default)]
    id: Option<i64>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    colour: String,
    #[serde(default)]
    theme: Option<String>,
}

async fn user_action(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Form(f): Form<UserForm>,
) -> AppResult {
    let name = f.name.trim().to_string();
    let repaint = state
        .db
        .with(|conn| -> anyhow::Result<_> {
            match f.action.as_str() {
                "create" if !name.is_empty() => {
                    // Cycle the palette so consecutive users never collide.
                    let n = queries::users(conn)?.len();
                    // `cycle().nth()` rather than `PALETTE[n % len]`: the
                    // remainder needs a non-zero length and the index needs to
                    // be in range, neither of which the compiler can know.
                    let colour = if f.colour.is_empty() {
                        PALETTE
                            .iter()
                            .cycle()
                            .nth(n)
                            .copied()
                            .unwrap_or(FIRST_COLOUR)
                    } else {
                        &f.colour
                    };
                    queries::create_user(conn, &name, colour)?;
                }
                "update" => {
                    if let Some(id) = f.id
                        && !name.is_empty()
                    {
                        let theme = f.theme.as_deref().unwrap_or("system");
                        // A new theme repaints every page, so the page has to
                        // be told to come back for it.
                        let was = queries::user(conn, id)?.map(|u| u.theme);
                        queries::update_user(conn, id, &name, &f.colour, theme)?;
                        return Ok(was.as_deref() != Some(theme));
                    }
                }
                "delete" => {
                    if let Some(id) = f.id {
                        queries::delete_user(conn, id)?;
                    }
                }
                // Here rather than in `display_action` because the control is
                // its own form: a form that does not contain the
                // move-completed checkbox would post it absent, and absent
                // means off.
                "default" => {
                    if let Some(id) = f.id {
                        queries::set_default_user(conn, id)?;
                    }
                }
                _ => {}
            }
            // Adding or removing a person changes what the page lists.
            Ok(true)
        })
        .with_context(|| format!("user action {:?}", f.action))?;

    Ok(saved(&headers, repaint))
}

#[derive(Deserialize)]
struct BoardForm {
    action: String,
    #[serde(default)]
    id: Option<i64>,
    #[serde(default)]
    name: String,
    /// Repeated `member` fields; axum's form extractor collects them into a Vec.
    #[serde(default)]
    member: Vec<i64>,
}

async fn board_action(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Form(f): Form<BoardForm>,
) -> AppResult {
    let name = f.name.trim().to_string();
    let repaint = state
        .db
        .transaction(|tx| -> anyhow::Result<_> {
            match f.action.as_str() {
                "create" if !name.is_empty() => {
                    let id = queries::create_board(tx, &name)?;
                    queries::set_board_members(tx, id, &f.member)?;
                }
                "update" => {
                    if let Some(id) = f.id {
                        if !name.is_empty() {
                            queries::rename_board(tx, id, &name)?;
                        }
                        queries::set_board_members(tx, id, &f.member)?;
                        // The name and the ticks are already on the screen.
                        return Ok(false);
                    }
                }
                "delete" => {
                    if let Some(id) = f.id {
                        queries::delete_board(tx, id)?;
                    }
                }
                _ => {}
            }
            Ok(true)
        })
        .with_context(|| format!("board action {:?}", f.action))?;

    Ok(saved(&headers, repaint))
}

#[derive(Deserialize)]
struct ListForm {
    action: String,
    #[serde(default)]
    board_id: Option<i64>,
    #[serde(default)]
    id: Option<i64>,
    #[serde(default)]
    name: String,
}

async fn list_action(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Form(f): Form<ListForm>,
) -> AppResult {
    let name = f.name.trim().to_string();
    // Removing a list moves whatever is still in it to the board's overdue
    // list, which it may have to create — named in the language in force now,
    // the same way the sweep names it.
    let overdue_name = crate::routes::locale(&state)
        .t(queries::OVERDUE_LIST_NAME)
        .to_string();
    state
        .db
        // A transaction, not a plain `with`: removing a list renumbers the
        // positions of everything it moves out, and that must not be half done.
        .transaction(|conn| -> anyhow::Result<_> {
            match f.action.as_str() {
                "create" => {
                    if let Some(board_id) = f.board_id
                        && !name.is_empty()
                    {
                        queries::create_custom_list(conn, board_id, &name)?;
                    }
                }
                "rename" => {
                    if let (Some(id), false) = (f.id, name.is_empty()) {
                        queries::rename_list(conn, id, &name)?;
                    }
                }
                "delete" => {
                    if let Some(id) = f.id {
                        queries::delete_list(conn, id, &overdue_name)?;
                    }
                }
                _ => {}
            }
            Ok(())
        })
        .with_context(|| format!("list action {:?}", f.action))?;

    // Adding, renaming or deleting a list changes the chips on the page.
    Ok(saved(&headers, true))
}

#[derive(Deserialize)]
struct DisplayForm {
    /// Absent when the checkbox is unticked — HTML forms omit rather than send false.
    #[serde(default)]
    move_completed_to_bottom: Option<String>,
    #[serde(default)]
    overdue_action: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

/// App-wide display settings. Deliberately not per-user: two people looking at
/// the same shared column should not see it in two different orders.
async fn display_action(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Form(f): Form<DisplayForm>,
) -> AppResult {
    let on = f.move_completed_to_bottom.is_some();
    let overdue = match f.overdue_action.as_deref() {
        Some(queries::OVERDUE_LEAVE) => queries::OVERDUE_LEAVE,
        Some(queries::OVERDUE_TODAY) => queries::OVERDUE_TODAY,
        _ => queries::OVERDUE_LIST,
    };
    // A language code becomes a file name, so only the ones on offer are
    // stored; anything else stays as it was.
    let language = f
        .language
        .as_deref()
        .filter(|code| crate::i18n::is_known(code))
        .map(str::to_string);
    let repaint = state
        .db
        .with(|conn| -> anyhow::Result<_> {
            queries::set_flag(conn, queries::MOVE_COMPLETED, on)?;
            queries::set_setting(conn, queries::OVERDUE_ACTION, overdue)?;
            let mut repaint = false;
            if let Some(language) = &language {
                // Every word on the page is about to change.
                repaint = queries::get_setting(conn, queries::LANGUAGE, crate::i18n::DEFAULT)?
                    != *language;
                queries::set_setting(conn, queries::LANGUAGE, language)?;
            }
            Ok(repaint)
        })
        .context("saving the display settings")?;
    Ok(saved(&headers, repaint))
}

#[derive(Deserialize)]
struct ThemeForm {
    theme: String,
    next: String,
}

/// The topbar's quick toggle. Theme is per-user, unlike ordering (D20): it is
/// about the eyes in front of the screen, not about the shared data.
async fn theme_action(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(f): Form<ThemeForm>,
) -> AppResult {
    let Some(me) = current_user(&state, &jar)? else {
        return Ok(Redirect::to("/settings").into_response());
    };
    let theme = match f.theme.as_str() {
        "light" | "dark" => f.theme.as_str(),
        _ => "system",
    };
    state
        .db
        .with(|conn| queries::update_user(conn, me.id, &me.name, &me.colour, theme))
        .context("saving the theme")?;

    // Only ever back to a page of ours.
    let next = if f.next.starts_with('/') {
        f.next
    } else {
        "/".into()
    };
    Ok(Redirect::to(&next).into_response())
}

#[derive(Deserialize)]
struct WhoamiForm {
    user_id: i64,
    #[serde(default)]
    next: Option<String>,
    /// The board the picker was on, so we can tell whether it is one of the
    /// new identity's.
    #[serde(default)]
    board: Option<i64>,
}

async fn whoami(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(f): Form<WhoamiForm>,
) -> AppResult {
    let mut cookie = Cookie::new(IDENTITY_COOKIE, f.user_id.to_string());
    cookie.set_path("/");
    cookie.set_same_site(SameSite::Lax);
    // Long-lived on purpose: picking your name should stick across restarts.
    cookie.set_max_age(time::Duration::days(365));

    // Becoming someone else puts you in their world. Staying on a board they
    // are not a member of would leave them looking at another person's week
    // with it named in their own picker; `/` lands them on their own board, in
    // the view they read it in. The boundary is still soft — the URL works if
    // they type it — but switching identity does not carry them across it.
    let theirs = match f.board {
        Some(board_id) => state
            .db
            .with(|conn| queries::is_board_member(conn, board_id, f.user_id))
            .with_context(|| format!("checking membership of board {board_id}"))?,
        None => true,
    };

    let next = f
        .next
        .filter(|n| theirs && n.starts_with('/'))
        .unwrap_or_else(|| "/".into());

    Ok((jar.add(cookie), Redirect::to(&next)).into_response())
}

/// Shown when nobody has claimed an identity in this browser yet.
async fn pick_page(State(state): State<AppState>) -> AppResult {
    let users = state
        .db
        .with(queries::users)
        .context("loading users for the picker")?;
    render(
        &state,
        &locale(&state),
        "pick_user.html",
        minijinja::context! { users => users },
    )
}
