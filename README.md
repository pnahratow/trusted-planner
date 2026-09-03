# Trusted Planner

A self-hosted week planner for a home network. One container, one SQLite file,
no accounts.

> **The board boundary is soft.** There is no authentication and no permission
> checks: anyone who can reach the app can pick any identity and open any board.
> A "personal" board is out of the way, not protected. Do not store anything
> genuinely sensitive here.

## Running it locally

```sh
PLANNER_DATA_DIR=./data PLANNER_PORT=8080 cargo run
```

Then open <http://localhost:8080>. With an empty database you are sent to
**Settings** to create a user and a board; after that `/` lands on your board
for the current week.

| Variable | Default | Meaning |
|---|---|---|
| `PLANNER_DATA_DIR` | `/data` | Where `planner.sqlite3` lives |
| `PLANNER_PORT` | `8080` | Listen port |
| `PLANNER_TEMPLATE_DIR` | `templates` | Templates, read from disk at render time |
| `PLANNER_STATIC_DIR` | `static` | CSS, JS, vendored libraries |

Templates are re-read on every render, so editing anything in `templates/` shows
up on the next page load with no rebuild and no restart.

## Tests

```sh
cargo test
cargo clippy --all-targets   # pedantic + nursery, warning-free
```

Covers the things that are easy to get subtly wrong — task position renumbering
across moves and deletes, translating a drop onto its neighbour, compare-and-swap
refusal on a stale edit — plus the calendar date-range maths.

`every_query_prepares_and_runs` exercises all 33 query functions once against a
real schema. SQLite only parses SQL when a statement is prepared, so without it a
typo in a rarely-hit path (renaming a list, removing a board) would stay hidden
until someone triggered it in production.

The rendered page is checked in a real browser (headless Chromium over the
DevTools protocol) rather than only with `curl`, because a template that renders
a hole still returns 200 to a request that never looks at the markup.

## Live updates

Other people's edits appear within a few seconds. The client asks
`/changes?board=N&since=<seq>` on a short poll and re-fetches only the columns
that actually changed.

This deliberately is **not** server-sent events, though it was at first. A
browser allows six concurrent connections per origin over HTTP/1.1, and an SSE
stream holds one open for its lifetime — so at five open tabs the sixth
connection is the last one and every further request queues forever, freezing
the app. HTTP/2 would fix it, but browsers only negotiate that over TLS and v1
is plain HTTP on the LAN. Short polls hold nothing open, cannot exhaust the
pool, and survive a buffering proxy. Verified working with ten tabs open.

A column holding an open editor defers its refresh until the editor closes;
re-rendering it would destroy the edit and the version it is checked against.
Typing in the "add task" box does not defer, because the draft and caret are
restored across the swap.

## Views

A week grid, and a four-week grid, over the same data — a day is a list with a
date, so the second view is a different renderer, not a different data model.
Everything below that line is shared: the queries, the column markup, and so
drag & drop, live updates and the compare-and-swap editor all work identically
in both.

Four weeks rather than a calendar month on purpose. A month grid is 35 cells
some months and 42 others, so rows reflow and change height as you page through
it; 4x7 never does. A cell that runs out of room shows "+N more", which opens
that day in full — the same column partial, so it behaves the same inside.

## Deleting

There is no "are you sure". Deleting removes the row and offers **Undo** in a
bar at the bottom for a few seconds; the row is soft-deleted, so undo restores
it to exactly the position it held. A confirmation dialog would interrupt every
deliberate delete to guard against the occasional slip — this way round, only
the slip pays.

## Status

Phases 1–7 of the plan are done: skeleton, settings, week grid, task CRUD,
drag & drop, live updates, four-week view. Polish and packaging follow.
