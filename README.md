# Trusted Planner

A self-hosted week planner for a home network. One container, one SQLite file,
no accounts.

> **The board boundary is soft.** There is no authentication and no permission
> checks: anyone who can reach the app can pick any identity and open any board.
> A "personal" board is out of the way, not protected. Do not store anything
> genuinely sensitive here.

Out of the way is still worth something, though. The board picker lists the
boards you are a member of, and switching identity lands you on your own board
rather than leaving you on someone else's — so nobody has to scroll past
another person's boards to find theirs. A board you are visiting is named in
the picker while you are on it, because a picker that does not say where you
are is worse than one that shows an extra board.

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
| `PLANNER_LOCALE_DIR` | `locales` | Translation files, one JSON per language |
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

`every_query_prepares_and_runs` exercises, in one pass against a real schema,
the 35 query functions that no other test reaches; the remaining eight — the
overdue sweep, compare-and-swap, moving and undo — have tests of their own.
SQLite only parses SQL when a statement is prepared, so without this a typo in
a rarely-hit path (renaming a list, removing a board) would stay hidden until
someone triggered it in production.

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
it; 4x7 never does.

**A grid page is exactly as tall as the window.** The point of four weeks at
once is seeing them at once, so the page never scrolls: the topbar takes what
it needs, the custom lists take up to a quarter of the screen, and the calendar
divides the rest into four equal rows. On a 1440p screen that is a cell with
room for seven or eight tasks; on a 768px tablet, for two or three, with the
gaps, headings and rows tightened to claw back the difference. A cell that
still runs out of room scrolls its own list, showing half a row at the cut so
you can see there is more. Clicking a cell's date opens that day in full — the
ordinary column partial, so adding, editing and dragging behave identically
inside it.

The cell used to be cut to four tasks by the server, which was too few on a
large screen and too many on a small one. How much fits is a question about the
height of a cell, and only the browser knows that, so the server sends
everything and the layout decides.

Below 760px wide, or 460px tall, four weeks in one screenful stops being dense
and starts being unreadable, so the page goes back to being a document you
scroll. A phone wants the week view.

Each board remembers which of the two you read it in, per person. A board full
of appointments wants four weeks and a shopping board wants none of it, so the
choice belongs to the board rather than to the app — and it is per person
because, unlike the ordering, the view changes nothing about the data: two
people on a laptop and a big monitor are entitled to disagree. The URL is still
the truth while you are on a page, so a link you paste opens the view you
copied; the memory only decides where `/` and the board picker send you.

## Deleting

There is no "are you sure". Deleting removes the row and offers **Undo** in a
bar at the bottom for a few seconds; the row is soft-deleted, so undo restores
it to exactly the position it held. A confirmation dialog would interrupt every
deliberate delete to guard against the occasional slip — this way round, only
the slip pays.

## Days that have passed

A task still unticked when its day is over follows one app-wide rule, set under
Settings → Display:

- **Move it to a list** (the default) — each board grows a `Todo` list, so the
  calendar shows what is still planned rather than what went wrong. It is an
  ordinary custom list: rename it and the sweep follows, because the board
  remembers it by id rather than by name.

  If the board already has a list called `Todo`, that one is used rather than a
  second one being made — list names are not unique, so without that check a
  board could end up with two and the tasks split between them. The match
  ignores capitalisation. Delete the list and the next sweep makes a fresh one;
  a board whose list was created before this was called `Unfinished`, and it
  keeps that name until you rename it, because it is yours.
- **Move it to today**, and again each day it stays unticked.
- **Leave it** on the day it was written for.

Ticked tasks never move; a finished day stays as a record of itself.

The sweep runs when a board is loaded, not from a timer. A household app that
sits idle for days should not need a scheduler and a timezone-aware cron to be
correct, and opening the board is exactly when the answer has to be right. It
is idempotent, so running it on every page load costs a query that usually
finds nothing.

## Language

English or German, chosen once under Settings → Display for everyone. English
is the source language: the English words are written directly in the
templates, and `locales/de.json` maps each of them to its German. A string with
no German next to it renders as English rather than as a broken placeholder, so
a half-finished translation is a mild embarrassment instead of an outage.

Adding a language is a file: copy `de.json` to `fr.json`, translate the right
hand side, add the code to `LANGUAGES` in `src/i18n.rs`. Nothing else knows how
many languages there are.

Dates are translated too, and not only their words — German writes "3. Sep"
where English writes "3 Sep", so the punctuation lives in the translation as
`"{day} {month}": "{day}. {month}"` rather than in a format string in the code.

Three tests keep this honest, because every failure mode here is silent: one
that every string the app shows has German, one that the file has nothing left
in it that the app no longer says, and one that no template shows a string it
forgot to mark for translation at all.

Code, comments, commit messages, this README and everything in the database
stay in English. The only exception is a list the app creates for you — the
`Todo` list is named in whatever language was in force when it first appeared,
because it is an ordinary list from then on and yours to rename. German calls
it `Todo` as well; change the right hand side in `de.json` if you would rather
it said something else.

## Settings

There is no Save button. Renaming a person, picking a colour, ticking an option
— the change *is* the instruction, and it takes effect when you make it, on
blur for a text field and immediately for anything else. Adding and removing
still have buttons, because those are not adjustments to something already
there.

Saving does not move the page. Every form posts through htmx, so a save is a
request rather than a navigation — a redirect back to `/settings` would land
you at the top of it, which is unbearable when a checkbox saves itself. The
server answers with nothing at all when the page already shows the result, and
asks for a reload only when the page would genuinely look different: a row
added or removed, or the theme or language, which repaint everything. A reload
keeps your scroll position; a navigation cannot.

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
docker build -t trusted-planner:0.3.1 .
docker compose up -d          # reads docker-compose.yml
```

On TrueNAS SCALE: **Apps → Discover Apps → Custom App → Install via YAML**, and
paste `docker-compose.yml`. The image has to exist on the NAS first — either
push it to a registry it can reach, or copy it over:

```sh
docker save trusted-planner:0.3.1 | ssh nas 'docker load'
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

## Licence

GNU Affero General Public License v3.0 or later — see [LICENSE](LICENSE).
Copyleft on purpose: a household planner someone improves should stay something
the next household can have. Affero rather than plain GPL because this is a
thing you run for other people to use over a network, and that is the case the
ordinary GPL does not reach — run a modified copy for others and the source
goes with it.

htmx and SortableJS are committed under `static/vendor/` so the app can be
served by a machine with no internet connection. They are unmodified builds
under their own licences, reproduced in
[static/vendor/NOTICE](static/vendor/NOTICE) — htmx under Zero-Clause BSD,
SortableJS under MIT.

The binary carries more than its own code: SQLite is compiled into it and 136
crates are linked in. [THIRD-PARTY.md](THIRD-PARTY.md) lists them with their
licences, and says how to regenerate the list after a dependency changes.

## Status

All nine phases of the plan are done: skeleton, settings, week grid, task CRUD,
drag & drop, live updates, four-week view, polish, packaging.

Recurring tasks are the intended next feature and the schema does not preclude
them, but they are the largest single piece of complexity in the app — an
occurrence as a row or as a projection, editing one versus the series, what
dragging an occurrence means — so v1 deliberately ships without them.
