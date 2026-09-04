# Reading this codebase

A guided tour for someone who wants to change something and needs to know where
to look first. The README says what the app does and why it behaves the way it
does; this says how it is built and in what order to read it.

Size, so you know what you are in for: about 2,700 lines of Rust plus 1,000
lines of tests (70 of them), 11 templates, one 320-line JavaScript file, one
440-line stylesheet. It is small enough to read in an afternoon, and the tour
below is roughly that afternoon in order.

## What it is, in one paragraph

One process. A browser asks for a URL, a handler runs a few SQL statements
against a SQLite file, fills a template, and sends back HTML. There is no API,
no JSON except one small polling endpoint, no client-side model of the data,
and no build step anywhere. Everything the browser does — adding a task,
editing one, dragging one — is an HTTP request whose response is the new HTML
for the part of the page that changed.

## The stack

| Piece | What it is | Where you meet it |
|---|---|---|
| **Rust** (edition 2024) | the whole server | `src/` |
| **axum** | the web framework: routing, handlers, extractors | every file in `src/routes/` |
| **tokio** | the async runtime axum runs on | `#[tokio::main]` in `main.rs`, and nowhere else |
| **rusqlite** (`bundled`) | SQLite driver; the C library is compiled into the binary | `src/db.rs`, `src/queries.rs` |
| **minijinja** | Jinja2-style templates, read from disk at render time | `src/templates.rs`, `templates/*.html` |
| **htmx** | HTML attributes that make requests and swap the response into the page | `templates/*.html`, vendored in `static/vendor/` |
| **SortableJS** | drag & drop | `static/app.js`, vendored |
| **anyhow** | error type with `.context()` messages | everywhere; `src/error.rs` turns it into a 500 |
| **chrono** / **chrono-tz** | dates and the one timezone | `src/calendar.rs` |
| **serde** | structs to and from forms, query strings and template context | derive attributes on the small `Form` structs |
| **tower-http** | serves `static/`, logs requests | `main.rs` |
| **tracing** | logging | `tracing::info!` / `error!` |

What is deliberately absent, so you do not go looking: no JavaScript framework,
no bundler, no TypeScript, no CSS preprocessor, no ORM, no migration tool, no
connection pool, no session store, no authentication.

### The Rust you need

Reading this needs less Rust than writing Rust usually does. There are no
lifetimes to reason about beyond plain `&` borrows, no unsafe, no macros of our
own, and almost no generics (`Db::with` and `render` are the exceptions, and
both have doc comments explaining themselves). What does appear on nearly every
page:

- `Result<T, E>` and the `?` operator: "if this failed, return the failure".
- `Option<T>` with `let Some(x) = ... else { return ... }`, which is how every
  handler says "that board/task/date does not exist, redirect instead".
- Closures passed to `db.with(|conn| ...)` and `db.transaction(|tx| ...)`.
- `async fn` on handlers. Nothing in them actually awaits anything except the
  framework itself; SQLite calls are synchronous and fast enough that this is
  fine at household scale.

### The htmx you need

These attributes cover nearly all of it. On an element:

- `hx-get` / `hx-post` — the URL to call when this is clicked or submitted.
- `hx-target` — which element the response replaces (`closest .column` means
  "walk up to the enclosing column").
- `hx-swap` — how: `outerHTML` replaces the target itself, `innerHTML` its
  contents.
- `hx-trigger` — what causes the request; here, mostly the custom
  `refresh-column` event that `app.js` fires.
- `hx-preserve` / `hx-swap-oob` — keep this node across a swap, and update a
  second node (the undo bar) out of band.
- `hx-sync` — on the add-task form only: queue rapid submissions instead of
  letting the second cancel the first.

The server always answers with a fragment of HTML, never JSON. If you can read
`templates/task_row.html` you can predict exactly what a click does.

## Four ideas that explain most of the code

**1. A day is a list that has a date.** There is one `lists` table. A row with
`date = '2026-09-04'` is a day column; a row with `date IS NULL` is a custom
list. This halves the query surface and is why the week grid and the custom
lists row share one template. Day rows are created lazily — an empty day has no
database row at all and is drawn from the calendar.

**2. Every column has a key.** A column is addressed by a string that exists
before any row does: `2026-09-04` for a day, `list-7` for a custom list. That
key is the fragment URL (`/b/1/col/2026-09-04`), the DOM id (`col-2026-09-04`),
and the unit of live invalidation. `ColumnKey` in `src/views.rs` is that idea in
code; read it early.

**3. The server's re-render is the answer.** Every mutation handler ends by
returning the whole column, freshly rendered. The client never computes what the
new state should be, so it cannot disagree with the server. Drag & drop moves
the row optimistically for the feel of it, and is then overwritten by the
server's version a moment later.

**4. Density is the only thing a column knows about its page.** `full` for the
week grid, `compact` for a four-week cell. It changes how many rows are rendered
and nothing else — same markup, same behaviour, so dragging and editing work
identically in both grids. See `views::density_for` and the `Density` extractor
in `routes/mod.rs`, which reads it off htmx's `HX-Current-URL` header rather
than threading a hidden field through every form.

## The map

```
src/
  main.rs        config from env, timezone, data dir check, router assembly
  db.rs          opens SQLite, runs embedded migrations, hands out the one connection
  queries.rs     every SQL statement in the app, one function each (+ half the tests)
  models.rs      User, Board, List, Task — plain structs matching table rows
  views.rs       rows -> what a template renders; ColumnKey lives here
  calendar.rs    date maths: Monday-of, N weeks from, formatting, the timezone
  events.rs      the in-memory change log clients poll
  templates.rs   minijinja setup (strict undefined, re-read from disk)
  error.rs       AppError: anything a handler fails at becomes a logged 500
  routes/
    mod.rs       router assembly, the Density extractor, current_user, render()
    board.rs     the two grid pages, the column fragment, the day panel
    task.rs      add, toggle, edit (compare-and-swap), delete, undo, move
    settings.rs  users, boards, membership, custom lists, display settings, identity
    events.rs    GET /changes — the polling endpoint
    health.rs    GET /healthz
templates/       layout + week + weeks + column + task_row + editors + settings
static/
  app.css        one stylesheet; theming via CSS custom properties
  app.js         focus guard, undo timer, drag & drop, the poll loop
  vendor/        htmx and SortableJS, committed (the NAS may be offline)
migrations/      001_init.sql, 002_global_settings.sql — embedded at build time
```

## A reading order

1. **`src/main.rs`** (134 lines). Config, then the shape of the whole program:
   state, router, listener. Ten minutes.
2. **`migrations/001_init.sql`**, then **`src/models.rs`**. Four tables and the
   structs mirroring them. Notice `deleted_at` on almost everything — nothing is
   ever really deleted — and `version` on `tasks`.
3. **`src/views.rs`**, top to bottom. `ColumnKey`, the two `resolve_*` methods
   (one may create a day-list, the other must never write), and `load_column`,
   which is the funnel every fragment goes through.
4. **One page, end to end**: `routes/board.rs::week`. It parses the URL, sweeps
   overdue tasks, calls `load_grid` — the shared chain both grids run through —
   and renders `week.html`. Then read `four_weeks` directly below it and note
   how little differs: the dates, the labels, and the density.
5. **One mutation, end to end**: `routes/task.rs::toggle`. Follow it into
   `mutate_in_place`: locate the task's column, run one statement, record the
   change, re-render the column. Every other mutation is a variation on this.
   Then read `update` for the compare-and-swap path, which is the one place the
   server refuses a write instead of applying it.
6. **`src/events.rs`** and the poll loop at the bottom of `static/app.js`. Fifty
   lines of idea: a counter, and the value it had when each column last changed.
7. **`static/app.js`** in full, once you know what a column is. Focus guard,
   click-away-to-cancel, SortableJS wiring, the deferred refresh of a column
   holding an open editor.
8. **`src/queries.rs`** last, and by need rather than in order. It is long
   because it is one function per statement, and half of it is tests. The parts
   worth reading properly are `position_after`, `move_task`, `delete_task` and
   `restore_task` — the position arithmetic is the subtlest code in the app.

`src/routes/settings.rs` and `templates/settings.html` are self-contained; read
them when you want to change what settings exist.

## Two traces

Opening the week:

```
GET /b/1/w/2026-08-31
  routes/board.rs::week
    current_user            cookie -> User, or redirect to /pick
    sweep_overdue           the D23 rule, in a transaction, idempotent
    load_grid               the lists on screen, then one query for all tasks
      views::column_view    per day and per custom list
    render("week.html")     layout.html -> column.html -> task_row.html
```

Ticking a task off:

```
POST /task/42/toggle          <- hx-post on the checkbox form
  Density                     from the HX-Current-URL header: week or 4-week
  column_of(42)               which column does this task live in
  queries::toggle_task
  changes.record(board, key)  other tabs will see this within ~3s
  render_column               the column's new HTML
<- 200, one <section class="column"> which htmx swaps into place
```

## Where to change things

| You want to | Go to |
|---|---|
| Change what a task row looks like | `templates/task_row.html`, `static/app.css` |
| Add a field to a task | new file in `migrations/` **and** an entry in `db.rs::MIGRATIONS`, then `models.rs`, `queries.rs`, `views.rs`, the templates |
| Add a route | the relevant `src/routes/*.rs`; its `router()` is merged in `routes/mod.rs` |
| Add an app-wide setting | a key constant in `queries.rs`, a control in `templates/settings.html`, a branch in `settings.rs::display_action` |
| Change how many weeks the wide view shows | `calendar::VIEW_WEEKS`, and the grid width in `app.css` |
| Change how many rows a month cell fits | `views::MONTH_CELL_ROWS` |
| Change the polling interval | `POLL_MS` in `static/app.js` |
| Change colours or spacing | the custom properties at the top of `static/app.css` |

## Traps

- **Templates are strict.** A name the context does not supply is an error, not
  an empty string. If you add a name to a template, every handler that renders
  it must pass it — including the fragment routes, which render `column.html`
  standalone.
- **The database mutex is not reentrant.** Calling `db.with(...)` from inside
  another `db.with(...)` deadlocks the process. That is why `views::load_column`
  reads settings from the connection it was handed instead of taking them as an
  argument.
- **Positions must stay a dense `0..n` run** within a list. `delete_task` and
  `move_task` renumber to keep that true, and both must be called inside a
  transaction. `restore_task` re-opens the gap it left.
- **Every query filters `deleted_at IS NULL`** except `task_any`, which undo
  needs. If you write a new query, remember the tombstones.
- **Migrations are embedded with `include_str!`.** A new `.sql` file does
  nothing until it is listed in `db.rs::MIGRATIONS`. They are also build inputs,
  which is why the Dockerfile copies `migrations/` into the build stage.
- **The change log is in memory.** A restart resets the counter; the client
  notices the sequence went backwards and re-fetches everything.
- **`Density` comes from a header**, so a mutation triggered outside htmx (curl,
  say) answers at full density. That is the right default, but it explains why a
  hand-made request looks different from what the browser gets.

## Poking at it

`cargo run` with the environment from the README, then:

```sh
curl -s localhost:8080/healthz
curl -s 'localhost:8080/changes?board=1&since=0'          # the poll endpoint
curl -s localhost:8080/b/1/col/2026-09-04                 # one column's HTML
curl -s -b user_id=1 localhost:8080/b/1/w/2026-08-31      # a whole page
```

Templates are re-read on every render, so editing one and reloading the page is
the fastest loop in the project — no rebuild, no restart. Rust changes need
`cargo run` again.

The *why* behind most decisions is in the README and in the module-level
comments at the top of each file; the commit messages carry the rest.
