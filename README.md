# Trusted Planner

A self-hosted week planner for a home network. One container, one SQLite file,
no accounts.

> **The board boundary is soft.** There is no authentication and no permission
> checks: anyone who can reach the app can pick any identity and open any board.
> A "personal" board is out of the way, not protected. Do not store anything
> genuinely sensitive here.

[ARCHITECTURE.md](ARCHITECTURE.md) is the tour of the source: the stack, the
layout, and the order to read it in.

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
| `PLANNER_TZ` (or `TZ`) | `Europe/Berlin` | IANA zone the calendar is read in |

Templates are re-read on every render, so editing anything in `templates/` shows
up on the next page load with no rebuild and no restart.

The timezone matters more than it looks. A container has no local time — it is
UTC unless told otherwise — so a task added at half past eleven at night would
be filed under tomorrow. The default is therefore `Europe/Berlin`, an actual
place rather than a defensible abstention; elsewhere, set `PLANNER_TZ`. An
unknown zone name stops the server at startup instead of quietly meaning UTC,
where the mistake would only show up late in the evening. The zone database is
compiled into the binary, so the image needs no `tzdata`.

## Tests

```sh
cargo test
cargo clippy --all-targets   # pedantic + nursery, warning-free
cargo fmt --check            # stock rustfmt, no config
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

## Days that have passed

A task still unticked when its day is over follows one app-wide rule, set under
Settings → Display:

- **Move it to a list** (the default) — each board grows an `Unfinished` list,
  so the calendar shows what is still planned rather than what went wrong. The
  list is an ordinary custom list: rename it, or delete it and the next sweep
  makes a new one.
- **Move it to today**, and again each day it stays unticked.
- **Leave it** on the day it was written for.

Ticked tasks never move; a finished day stays as a record of itself.

The sweep runs when a board is loaded, not from a timer. A household app that
sits idle for days should not need a scheduler and a timezone-aware cron to be
correct, and opening the board is exactly when the answer has to be right. It
is idempotent, so running it on every page load costs a query that usually
finds nothing.

## Theme

Light, dark, or system, per person — it is about the eyes in front of the
screen, not the shared data. The topbar button cycles the three; Settings has
the same choice spelled out.

## Deploying

The image is a two-stage build: `cargo build --release` in `rust:1-trixie`, and
a `debian:trixie-slim` layer holding the binary, the templates and the static
files. SQLite is compiled into the binary and so is the timezone database, so
nothing is installed at runtime except `curl`, which the `HEALTHCHECK` uses.

```sh
docker build -t trusted-planner:0.1.0 .
docker compose up -d          # reads docker-compose.yml
```

On TrueNAS SCALE: **Apps → Discover Apps → Custom App → Install via YAML**, and
paste `docker-compose.yml`. The image has to exist on the NAS first — either
push it to a registry it can reach, or copy it over:

```sh
docker save trusted-planner:0.1.0 | ssh nas 'docker load'
```

Two things to get right, both in the compose file:

- **The host path** (`/mnt/tank/apps/trusted-planner`) should be a dataset of
  its own, so ZFS snapshots are the backup story and `planner.sqlite3` can be
  copied out of one. Point it somewhere real before installing.
- **Ownership.** TrueNAS runs custom apps as `apps:apps` = **568:568**, and the
  container runs as that by default. The host directory must be owned by it:

  ```sh
  chown -R 568:568 /mnt/tank/apps/trusted-planner
  ```

  Get this wrong and the container exits immediately saying it cannot create
  the data directory — which is the intended behaviour, because the alternative
  is a running app that fails on the first write. Any other UID works as well;
  set `user:` and the directory's owner to the same thing.

Data lives entirely in `/data`. Restarting, upgrading or rebuilding the
container touches nothing in there; `docker compose down` and back up keeps
everything.

`docker stop` sends SIGTERM and waits ten seconds before resorting to SIGKILL.
The server takes milliseconds of that: it stops accepting connections, lets the
requests already in flight finish, and folds SQLite's write-ahead log back into
`planner.sqlite3` so the file on disk is whole. That last part is why a ZFS
snapshot taken after a stop is a complete database rather than one missing its
most recent writes. Ctrl-C locally, or `kill` from outside, is the same
shutdown.

## Status

All nine phases of the plan are done: skeleton, settings, week grid, task CRUD,
drag & drop, live updates, four-week view, polish, packaging.

Recurring tasks are the intended next feature and the schema does not preclude
them, but they are the largest single piece of complexity in the app — an
occurrence as a row or as a projection, editing one versus the series, what
dragging an occurrence means — so v1 deliberately ships without them.
