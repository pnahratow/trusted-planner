//! Users, boards, membership, custom lists, and the identity picker.

use anyhow::Context;
use axum::extract::State;
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::Router;
// serde_urlencoded (axum::Form) cannot deserialise repeated keys into a Vec,
// which is exactly what a set of membership checkboxes posts.
use axum_extra::extract::Form;
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use serde::Deserialize;

use crate::error::AppResult;
use crate::routes::{current_user, render, IDENTITY_COOKIE};
use crate::{queries, AppState};

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
    "#3563e9", "#e0533d", "#2ea36b", "#b552d6", "#d99a1f", "#0e9bb5", "#d64f8a", "#5b6470",
];

async fn page(State(state): State<AppState>, jar: CookieJar) -> AppResult {
    let me = current_user(&state, &jar)?;

    let (users, boards, move_completed, overdue_action) = state.db.with(|conn| -> anyhow::Result<_> {
        let users = queries::users(conn)?;
        let boards = queries::boards(conn)?;
        let move_completed = queries::get_flag(conn, queries::MOVE_COMPLETED, true)?;
        let overdue_action =
            queries::get_setting(conn, queries::OVERDUE_ACTION, queries::OVERDUE_LIST)?;
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
        Ok((users, board_rows, move_completed, overdue_action))
    }).context("loading the settings page")?;

    let _theme = me.as_ref().map_or_else(|| "system".to_string(), |u| u.theme.clone());
    render(
        &state,
        "settings.html",
        minijinja::context! {
            users => users,
            boards => boards,
            palette => PALETTE,
            move_completed => move_completed,
            overdue_action => overdue_action,
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

async fn user_action(State(state): State<AppState>, Form(f): Form<UserForm>) -> AppResult {
    let name = f.name.trim().to_string();
    state.db.with(|conn| -> anyhow::Result<_> {
        match f.action.as_str() {
            "create" if !name.is_empty() => {
                // Cycle the palette so consecutive users never collide.
                let n = queries::users(conn)?.len();
                let colour = if f.colour.is_empty() {
                    PALETTE[n % PALETTE.len()].to_string()
                } else {
                    f.colour.clone()
                };
                queries::create_user(conn, &name, &colour)?;
            }
            "update" => {
                if let Some(id) = f.id
                    && !name.is_empty() {
                        queries::update_user(
                            conn,
                            id,
                            &name,
                            &f.colour,
                            f.theme.as_deref().unwrap_or("system"),
                        )?;
                    }
            }
            "delete" => {
                if let Some(id) = f.id {
                    queries::delete_user(conn, id)?;
                }
            }
            _ => {}
        }
        Ok(())
    }).with_context(|| format!("user action {:?}", f.action))?;

    Ok(Redirect::to("/settings").into_response())
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

async fn board_action(State(state): State<AppState>, Form(f): Form<BoardForm>) -> AppResult {
    let name = f.name.trim().to_string();
    state.db.transaction(|tx| -> anyhow::Result<_> {
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
                }
            }
            "delete" => {
                if let Some(id) = f.id {
                    queries::delete_board(tx, id)?;
                }
            }
            _ => {}
        }
        Ok(())
    }).with_context(|| format!("board action {:?}", f.action))?;

    Ok(Redirect::to("/settings").into_response())
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

async fn list_action(State(state): State<AppState>, Form(f): Form<ListForm>) -> AppResult {
    let name = f.name.trim().to_string();
    state.db.with(|conn| -> anyhow::Result<_> {
        match f.action.as_str() {
            "create" => {
                if let Some(board_id) = f.board_id
                    && !name.is_empty() {
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
                    queries::delete_list(conn, id)?;
                }
            }
            _ => {}
        }
        Ok(())
    }).with_context(|| format!("list action {:?}", f.action))?;

    Ok(Redirect::to("/settings").into_response())
}

#[derive(Deserialize)]
struct DisplayForm {
    /// Absent when the checkbox is unticked — HTML forms omit rather than send false.
    #[serde(default)]
    move_completed_to_bottom: Option<String>,
    #[serde(default)]
    overdue_action: Option<String>,
}

/// App-wide display settings. Deliberately not per-user: two people looking at
/// the same shared column should not see it in two different orders.
async fn display_action(State(state): State<AppState>, Form(f): Form<DisplayForm>) -> AppResult {
    let on = f.move_completed_to_bottom.is_some();
    let overdue = match f.overdue_action.as_deref() {
        Some(queries::OVERDUE_LEAVE) => queries::OVERDUE_LEAVE,
        Some(queries::OVERDUE_TODAY) => queries::OVERDUE_TODAY,
        _ => queries::OVERDUE_LIST,
    };
    state
        .db
        .with(|conn| -> anyhow::Result<_> {
            queries::set_flag(conn, queries::MOVE_COMPLETED, on)?;
            queries::set_setting(conn, queries::OVERDUE_ACTION, overdue)?;
            Ok(())
        })
        .context("saving the display settings")?;
    Ok(Redirect::to("/settings").into_response())
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
        return Ok(Redirect::to("/pick").into_response());
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
    let next = if f.next.starts_with('/') { f.next } else { "/".into() };
    Ok(Redirect::to(&next).into_response())
}

#[derive(Deserialize)]
struct WhoamiForm {
    user_id: i64,
    #[serde(default)]
    next: Option<String>,
}

async fn whoami(jar: CookieJar, Form(f): Form<WhoamiForm>) -> impl IntoResponse {
    let mut cookie = Cookie::new(IDENTITY_COOKIE, f.user_id.to_string());
    cookie.set_path("/");
    cookie.set_same_site(SameSite::Lax);
    // Long-lived on purpose: picking your name should stick across restarts.
    cookie.set_max_age(time::Duration::days(365));

    let next = f.next.filter(|n| n.starts_with('/')).unwrap_or_else(|| "/".into());
    (jar.add(cookie), Redirect::to(&next))
}

/// Shown when nobody has claimed an identity in this browser yet.
async fn pick_page(State(state): State<AppState>) -> AppResult {
    let users = state
        .db
        .with(queries::users)
        .context("loading users for the picker")?;
    render(&state, "pick_user.html", minijinja::context! { users => users })
}
