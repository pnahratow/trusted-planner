//! The change log clients poll to learn which columns need re-fetching.
//!
//! ## Why polling and not SSE
//!
//! This started as a `broadcast` channel behind an SSE stream, which worked
//! and was pleasantly instant — until five tabs were open. A browser allows
//! only six concurrent connections per origin over HTTP/1.1, and every SSE
//! stream holds one open for its lifetime. At five tabs the sixth connection
//! is the last one, and *every* further request to the app queues forever:
//! adds do nothing, the page appears frozen. Measured, not theorised.
//!
//! HTTP/2 would multiplex this away, but browsers only negotiate it over TLS,
//! and v1 is plain HTTP on the LAN. So the transport is short polls, which
//! hold no connection open, cannot exhaust the pool, and survive any buffering
//! proxy in between. D3 asks for "seconds, not sub-second", which this meets.
//!
//! Each change gets a sequence number, so a client says where it got to and
//! gets back exactly what it missed — reconnects and sleeping laptops need no
//! special handling.

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::views::ColumnKey;

/// Enough history that a tab asleep for hours still resyncs incrementally at
/// household write rates; anything older is answered with a full resync.
const HISTORY: usize = 1000;

#[derive(Clone, Debug)]
struct Entry {
    seq: u64,
    board_id: i64,
    key: String,
    origin: Option<String>,
}

#[derive(Default)]
struct Inner {
    seq: u64,
    entries: VecDeque<Entry>,
}

pub struct ChangeLog {
    inner: Mutex<Inner>,
}

/// What a polling client needs: the columns to re-fetch, and where to resume.
pub struct Changes {
    pub seq: u64,
    pub keys: Vec<String>,
    /// The client asked from further back than we still remember, so it cannot
    /// be brought up to date incrementally.
    pub resync: bool,
}

impl ChangeLog {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Records that a column changed. `origin` is the browser tab responsible,
    /// which already holds the server's answer and is filtered out of its own
    /// poll results.
    pub fn record(&self, board_id: i64, key: &ColumnKey, origin: Option<&str>) {
        let mut inner = self.inner.lock().expect("change log poisoned");
        inner.seq += 1;
        let entry = Entry {
            seq: inner.seq,
            board_id,
            key: key.as_string(),
            origin: origin.map(str::to_owned),
        };
        inner.entries.push_back(entry);
        while inner.entries.len() > HISTORY {
            inner.entries.pop_front();
        }
        drop(inner); // hold the lock no longer than the write itself
    }

    pub fn current_seq(&self) -> u64 {
        self.inner.lock().expect("change log poisoned").seq
    }

    /// Columns on `board_id` that changed after `since`, excluding those this
    /// client caused itself. Keys are de-duplicated: a column touched five
    /// times still only needs fetching once.
    pub fn since(&self, board_id: i64, since: u64, client: Option<&str>) -> Changes {
        let inner = self.inner.lock().expect("change log poisoned");

        let oldest = inner.entries.front().map_or(inner.seq, |e| e.seq);
        // `since > seq` means the server restarted and its counter went
        // backwards; treat that like any other gap.
        let resync = since > inner.seq || (since > 0 && since + 1 < oldest);

        let mut keys: Vec<String> = Vec::new();
        if !resync {
            for e in inner
                .entries
                .iter()
                .filter(|e| e.seq > since && e.board_id == board_id)
            {
                let own = matches!((client, e.origin.as_deref()), (Some(c), Some(o)) if c == o);
                if !own && !keys.contains(&e.key) {
                    keys.push(e.key.clone());
                }
            }
        }

        Changes {
            seq: inner.seq,
            keys,
            resync,
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

    #[test]
    fn a_fresh_client_starting_at_zero_is_not_a_gap() {
        let log = ChangeLog::new();
        let out = log.since(1, 0, None);
        assert!(!out.resync, "an empty log is not a gap");
        assert_eq!(out.seq, 0);
        assert_eq!(out.keys, Vec::<String>::new());
    }

    #[test]
    fn only_the_asked_for_board_comes_back() {
        let log = ChangeLog::new();
        log.record(1, &day("2026-09-03"), None);
        log.record(2, &day("2026-09-04"), None);

        assert_eq!(log.since(1, 0, None).keys, ["2026-09-03"]);
        assert_eq!(log.since(2, 0, None).keys, ["2026-09-04"]);
    }

    #[test]
    fn a_client_only_receives_what_it_has_not_seen() {
        let log = ChangeLog::new();
        log.record(1, &day("2026-09-03"), None);
        let first = log.since(1, 0, None);
        assert_eq!(first.keys.len(), 1);

        // Nothing new since.
        assert_eq!(log.since(1, first.seq, None).keys, Vec::<String>::new());

        log.record(1, &day("2026-09-05"), None);
        assert_eq!(log.since(1, first.seq, None).keys, ["2026-09-05"]);
    }

    #[test]
    fn a_column_touched_repeatedly_is_fetched_once() {
        let log = ChangeLog::new();
        for _ in 0..5 {
            log.record(1, &day("2026-09-03"), None);
        }
        log.record(1, &ColumnKey::List(7), None);
        assert_eq!(log.since(1, 0, None).keys, ["2026-09-03", "list-7"]);
    }

    #[test]
    fn a_tab_does_not_hear_its_own_writes_back() {
        let log = ChangeLog::new();
        log.record(1, &day("2026-09-03"), Some("tab-a"));
        log.record(1, &ColumnKey::List(2), Some("tab-b"));

        assert_eq!(log.since(1, 0, Some("tab-a")).keys, ["list-2"]);
        assert_eq!(log.since(1, 0, Some("tab-b")).keys, ["2026-09-03"]);
        // An anonymous poller still sees everything.
        assert_eq!(log.since(1, 0, None).keys.len(), 2);
    }

    #[test]
    fn falling_further_behind_than_the_history_asks_for_a_resync() {
        let log = ChangeLog::new();
        for _ in 0..(HISTORY + 50) {
            log.record(1, &day("2026-09-03"), None);
        }
        // Sequence 1 has long since been evicted.
        assert!(log.since(1, 1, None).resync);
        // Someone up to date is fine.
        let seq = log.current_seq();
        assert!(!log.since(1, seq, None).resync);
    }

    #[test]
    fn a_sequence_from_the_future_means_the_server_restarted() {
        let log = ChangeLog::new();
        log.record(1, &day("2026-09-03"), None);
        // The client remembers a counter from a previous process lifetime.
        assert!(log.since(1, 9999, None).resync);
    }

    #[test]
    fn history_is_bounded() {
        let log = ChangeLog::new();
        for _ in 0..(HISTORY * 2) {
            log.record(1, &day("2026-09-03"), None);
        }
        let inner = log.inner.lock().unwrap();
        assert_eq!(inner.entries.len(), HISTORY);
    }
}
