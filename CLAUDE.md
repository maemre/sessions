# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build            # compile
cargo run              # run the server (binds to 127.0.0.1:8080)
cargo clippy --no-deps # lint
```

The server creates `bookmarks.db` (SQLite) on first run and runs `schema.sql` to initialize tables.

Don't use `cargo test` yet, we don't have automated testing set up.

## Architecture

All application code lives in `src/main.rs`. The entry point connects to SQLite, applies `schema.sql`, builds `AppState`, and starts an Axum server on port 8080.

**Request flow:** Axum routes → handler extracts `State<AppState>` → calls a `_impl` helper that talks to SQLite via `sqlx` → renders a MiniJinja template → returns `Response`.

**`AppState`** holds two things shared across all handlers:
- `store: SqlitePool` — the SQLite connection pool
- `templates: Arc<Environment<'static>>` — MiniJinja template environment (immutable after setup)

**Templates** are loaded at startup from `templates/` via `include_str!` macros in `build_templates()`, using Jinja2 syntax with `base.html` inheritance.

**Database schema** (`schema.sql`): three tables — `bookmark` (id, url, title), `tag` (id, name), and `bookmark_tag` join table. Tags are stored normalized; the application assembles `Bookmark { tags: Vec<String> }` by joining across all three tables.

**Route ordering matters:** `/bookmarks/new` must be registered before `/bookmarks/{id}` so the literal string "new" isn't captured as an id. This is documented in `build_router`.

**Tag creation on bookmark insert** (`create_bookmark_impl`) runs inside a single transaction: insert the bookmark, upsert tags with `INSERT OR IGNORE`, then bulk-insert `bookmark_tag` rows using a dynamically built `SELECT … WHERE name IN (…)` query.
