# LarpBook

A shared feed for the people on a [LOKI](https://github.com/LOKI-Server/LOKI) server. LarpBook keeps no accounts of its own: names, usernames, bios and profile pictures come from LOKI.

- **Post** plain text, up to 2,000 characters. Web links become clickable; nothing else in a post is markup.
- **Like** a post, or take the like back.
- **Reply** under any post. Each post has its own page with its replies, oldest first.
- **Profiles** show a person's bio, post count and posts. Change your name, bio or picture on LOKI's account page; it shows up the next time you open LarpBook.
- **Delete** your own posts and replies. An app admin can delete anyone's.

Times show in each reader's own time zone, the composer counts down near the limit, and Ctrl+Enter (⌘+Enter) sends. Pages follow the system's light or dark mode and work without JavaScript.

## Requirements

LarpBook runs on LOKI 0.3 or later.

Give people access to the app in LOKI. To let someone moderate, grant them the app's `admin` role; LOKI's owner has it already.

## Build

Install Rust, then from this directory:

```powershell
rustup target add wasm32-wasip2
cargo install cargo-loki --locked
cargo test
cargo loki dev --server PATH_TO_LOKI_SERVER
```

`cargo loki dev` needs a `loki-server` executable, from a LOKI release or your own build. It runs a separate local LOKI and prints a sign-in for it, without touching an existing LOKI server.

`cargo loki package` writes `target/loki/larpbook-0.1.0.loki-app`. Import it in LOKI, grant access, and enable it.

## How it works

`src/lib.rs` routes requests, `src/store.rs` holds all SQL, `src/view.rs` turns rows into what the templates in `templates/` show, and `src/text.rs` cleans posts and turns them into safe HTML. Each visit copies the visitor's LOKI profile into the `authors` table, so their posts can show it to others.

`cargo test` runs the handlers against a real in-memory SQLite, through the SDK's fake host, with foreign keys on as in LOKI.
