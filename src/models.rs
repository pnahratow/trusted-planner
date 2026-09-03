//! Row types shared across routes.
//!
//! Dates are naive `YYYY-MM-DD` strings end to end: one server timezone, no
//! timezone maths (see plan, "Assumptions carried into implementation").

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub colour: String,
    pub theme: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Board {
    pub id: i64,
    pub name: String,
}

/// A day column (`date` set) or a custom list (`date` none) — one table, one type.
#[derive(Debug, Clone, Serialize)]
pub struct List {
    pub id: i64,
    pub board_id: i64,
    pub name: Option<String>,
    pub date: Option<String>,
    pub position: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: i64,
    pub list_id: i64,
    pub title: String,
    pub notes: String,
    pub done: bool,
    pub author_id: i64,
    pub position: i64,
    /// Carried into edit forms and checked on write — see the CAS rule (D12).
    pub version: i64,
}
