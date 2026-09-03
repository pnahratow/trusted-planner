//! What clients poll to learn which columns to re-fetch.
//!
//! ## Why polling
//!
//! This was server-sent events first, and it worked — until five tabs were
//! open. A browser allows six concurrent connections per origin over HTTP/1.1
//! and an SSE stream holds one open for its lifetime, so at five tabs every
//! further request queues forever and the app appears frozen. HTTP/2 would fix
//! it, but browsers only negotiate that over TLS and v1 is plain HTTP on the
//! LAN. Short polls hold nothing open. The app is idle most of the time and has
//! a handful of users, so the traffic is irrelevant either way.
//!
//! ## The whole model
//!
//! A counter that increments on every write, and the value it had when each
//! column last changed. A client says which value it last saw; it gets back the
//! columns that have changed since, and the current value to quote next time.
//!
//! That is all. There is no event history to size, evict or fall off the end
//! of: one entry per column, holding only its latest version, is enough to
//! answer any client no matter how far behind it is.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::views::ColumnKey;

#[derive(Default)]
struct Inner {
    /// Increments on every recorded change. Also the version stamped on the
    /// column that changed, which is what makes "since" comparisons work.
    seq: u64,
    /// `(board, column key) -> the seq at which it last changed`.
    columns: HashMap<(i64, String), u64>,
}

pub struct ChangeLog {
    inner: Mutex<Inner>,
}

/// The columns a client needs to re-fetch, and where to resume from.
pub struct Changes {
    pub seq: u64,
    pub keys: Vec<String>,
}

impl ChangeLog {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
        }
    }

    pub fn record(&self, board_id: i64, key: &ColumnKey) {
        let mut inner = self.inner.lock().expect("change log poisoned");
        inner.seq += 1;
        let seq = inner.seq;
        inner.columns.insert((board_id, key.as_string()), seq);
        drop(inner);
    }

    /// The version a freshly rendered page reflects.
    pub fn current_seq(&self) -> u64 {
        self.inner.lock().expect("change log poisoned").seq
    }

    /// Columns on this board that changed after `since`.
    ///
    /// A client arbitrarily far behind is served correctly, because each column
    /// carries its latest version rather than a place in a queue.
    pub fn since(&self, board_id: i64, since: u64) -> Changes {
        let inner = self.inner.lock().expect("change log poisoned");
        let keys = inner
            .columns
            .iter()
            .filter(|((board, _), version)| *board == board_id && **version > since)
            .map(|((_, key), _)| key.clone())
            .collect();
        Changes {
            seq: inner.seq,
            keys,
        }
    }
}

impl Default for ChangeLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn day(s: &str) -> ColumnKey {
        ColumnKey::Day(NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap())
    }

    fn sorted(mut keys: Vec<String>) -> Vec<String> {
        keys.sort();
        keys
    }

    #[test]
    fn a_fresh_client_on_an_untouched_board_has_nothing_to_do() {
        let log = ChangeLog::new();
        let out = log.since(1, 0);
        assert_eq!(out.seq, 0);
        assert_eq!(out.keys, Vec::<String>::new());
    }

    #[test]
    fn only_the_asked_for_board_comes_back() {
        let log = ChangeLog::new();
        log.record(1, &day("2026-09-03"));
        log.record(2, &day("2026-09-04"));

        assert_eq!(log.since(1, 0).keys, ["2026-09-03"]);
        assert_eq!(log.since(2, 0).keys, ["2026-09-04"]);
    }

    #[test]
    fn a_client_only_hears_about_what_it_has_not_seen() {
        let log = ChangeLog::new();
        log.record(1, &day("2026-09-03"));
        let caught_up = log.since(1, 0).seq;
        assert_eq!(log.since(1, caught_up).keys, Vec::<String>::new());

        log.record(1, &day("2026-09-05"));
        assert_eq!(log.since(1, caught_up).keys, ["2026-09-05"]);
    }

    #[test]
    fn a_column_touched_repeatedly_is_still_fetched_once() {
        let log = ChangeLog::new();
        for _ in 0..5 {
            log.record(1, &day("2026-09-03"));
        }
        log.record(1, &ColumnKey::List(7));
        assert_eq!(
            sorted(log.since(1, 0).keys),
            ["2026-09-03".to_string(), "list-7".to_string()]
        );
    }

    /// The reason a column stores its latest version rather than taking a place
    /// in a queue: no amount of falling behind can lose an update.
    #[test]
    fn a_client_arbitrarily_far_behind_is_still_answered_correctly() {
        let log = ChangeLog::new();
        for _ in 0..10_000 {
            log.record(1, &day("2026-09-03"));
        }
        log.record(1, &ColumnKey::List(1));

        let out = log.since(1, 0);
        assert_eq!(
            sorted(out.keys),
            ["2026-09-03".to_string(), "list-1".to_string()],
            "each column reported once, however long ago the client last looked"
        );
    }

    #[test]
    fn one_entry_per_column_however_many_writes() {
        let log = ChangeLog::new();
        for _ in 0..1_000 {
            log.record(1, &day("2026-09-03"));
        }
        let inner = log.inner.lock().unwrap();
        assert_eq!(inner.columns.len(), 1, "nothing accumulates per write");
        assert_eq!(inner.seq, 1_000);
    }
}
