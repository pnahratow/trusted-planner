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
```

Covers the two things that are easy to get subtly wrong — task position
renumbering across moves and deletes, and compare-and-swap refusal on a stale
edit — plus the calendar date-range maths.

## Status

Phases 1–4 of the plan are done: skeleton, settings, week grid, task CRUD.
Drag & drop, live updates, month view, polish and packaging follow.
