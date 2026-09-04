# Working in this repo

Read [ARCHITECTURE.md](ARCHITECTURE.md) before changing anything; it is the map
of the source and the reading order. This file is only the things that bite.

## Commands

```sh
cargo test                   # 70 tests, all must pass
cargo clippy --all-targets   # pedantic + nursery are on: must be warning-free
cargo fmt --check            # stock rustfmt, no config
PLANNER_DATA_DIR=./data cargo run
```

Templates are re-read on every render, so a template change needs no rebuild
and no restart — reload the page.

## The lints are denials, not suggestions

`cargo clippy --all-targets` must be silent. Pedantic and nursery are denied,
and so is every ordinary way of panicking: no `unwrap`, `expect`, `panic!`,
indexing, slicing, `as`, or unchecked arithmetic outside a test. In practice
that means `get(..)`, `checked_*`/`saturating_*`, `try_from`, a `let ... else`,
or a total `match`. If a value really cannot be absent, say so by making it
impossible rather than by asserting it — `calendar::ends` returning `Option`
instead of `dates[0]` is the pattern.

Tests may unwrap, expect, panic and index; `clippy.toml` allows exactly that.

## Failures that are silent

- **Nested `db.with` deadlocks the process.** There is one connection behind a
  non-reentrant mutex. Never call `db.with` or `db.transaction` from inside
  another; pass the `&Connection` down instead.
- **Templates run under strict-undefined.** A name a template uses must be
  supplied by *every* route that renders it — the page route and the fragment
  route both. `column.html` is rendered standalone by `/b/:board/col/:key`, so
  a name added for the week page alone is a live 500 that every server-side
  test still passes. Check both, or check it in a browser.
- **Task positions must stay a dense `0..n` run** inside a list. `delete_task`,
  `move_task` and `restore_task` renumber to keep that true, and all three must
  run inside a transaction.
- **A new migration file does nothing** until it is listed in
  `db.rs::MIGRATIONS`; the SQL is embedded with `include_str!`.
- **A visible string without `| t`** renders fine and leaves English on the
  German page. The English text is the key; add the German to
  `locales/de.json`. `cargo test i18n` checks both directions.
- **Soft delete is everywhere.** A new `SELECT` filters `deleted_at IS NULL`.
  `task_any` is the single deliberate exception, for undo.
- **A mutation answers with the re-rendered column**, never with a fragment the
  client is expected to assemble. If a handler returns something else, it will
  disagree with the polled refresh path sooner or later.

## Decisions that look like bugs

Do not "fix" these; they are chosen, and the reasoning is in the README.

- No authentication, no permission checks, no session store. Identity is a
  cookie holding a user id, picked from a dropdown.
- Deleting never asks for confirmation. It soft-deletes and offers undo.
- Colour means identity and only identity. There is no per-task colour.
- Move-completed-to-bottom and the overdue rule are app-wide; theme is
  per-user. Ordering belongs to the column, not the reader.
- Live updates are short polls, not SSE. An SSE stream per tab exhausts the
  browser's six connections per origin and freezes the app at five tabs.
- No new dependency without a reason that could not be met by twenty lines.

## Language

The interface speaks English or German. Everything else — code, comments,
commit messages, documentation, database contents — stays English, including
the keys in `locales/de.json`, which are the English strings themselves.

## Style

Comments explain **why**, not what — the tension in a decision, or the failure
it prevents. A comment restating the code is worse than none. The same goes for
commit messages: the subject says what changed in plain language, the body says
why it was worth changing.
