//! Everything LarpBook keeps in the app's SQLite database.
use crate::text::unix_now;
use loki_sdk::{Database, Result, Statement, User, params};
use serde::Deserialize;
use serde_json::{Value, json};

/// Posts per page.
pub const PAGE: usize = 40;

const SCHEMA: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS authors(id TEXT PRIMARY KEY, username TEXT NOT NULL, display_name TEXT NOT NULL, bio TEXT NOT NULL DEFAULT '', avatar INTEGER NOT NULL DEFAULT 0, updated INTEGER NOT NULL)",
    "CREATE TABLE IF NOT EXISTS posts(id TEXT PRIMARY KEY, author_id TEXT NOT NULL, body TEXT NOT NULL, created INTEGER NOT NULL)",
    "CREATE INDEX IF NOT EXISTS posts_created ON posts(created DESC, id DESC)",
    "CREATE INDEX IF NOT EXISTS posts_author ON posts(author_id, created DESC, id DESC)",
    "CREATE TABLE IF NOT EXISTS replies(id TEXT PRIMARY KEY, post_id TEXT NOT NULL REFERENCES posts(id) ON DELETE CASCADE, author_id TEXT NOT NULL, body TEXT NOT NULL, created INTEGER NOT NULL)",
    "CREATE INDEX IF NOT EXISTS replies_post ON replies(post_id, created)",
    "CREATE TABLE IF NOT EXISTS likes(post_id TEXT NOT NULL REFERENCES posts(id) ON DELETE CASCADE, user_id TEXT NOT NULL, created INTEGER NOT NULL, PRIMARY KEY(post_id, user_id))",
];

pub fn create_schema() -> Result<()> {
    let statements = SCHEMA
        .iter()
        .map(|sql| Statement::new(*sql, params![]))
        .collect::<Result<Vec<_>>>()?;
    Database.transaction(&statements)?;
    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
pub struct AuthorRow {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub bio: String,
    pub avatar: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PostRow {
    pub id: String,
    pub author_id: String,
    pub body: String,
    pub created: i64,
    pub username: String,
    pub display_name: String,
    pub avatar: i64,
    pub replies: i64,
    pub likes: i64,
    /// Whether the person viewing likes it.
    pub liked: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ReplyRow {
    pub id: String,
    pub author_id: String,
    pub body: String,
    pub created: i64,
    pub username: String,
    pub display_name: String,
    pub avatar: i64,
}

/// Which posts to list.
#[derive(Default)]
pub struct Filter<'a> {
    pub author: Option<&'a str>,
    pub post: Option<&'a str>,
    /// Only posts older than this `(created, id)`.
    pub before: Option<(i64, &'a str)>,
}

/// LarpBook keeps no accounts. It copies each person's LOKI profile when they
/// visit, so their posts can show it to others.
pub fn remember(user: &User) -> Result<()> {
    Database.execute(
        "INSERT INTO authors(id,username,display_name,bio,avatar,updated) VALUES(?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET username=excluded.username, display_name=excluded.display_name, bio=excluded.bio, avatar=excluded.avatar, updated=excluded.updated",
        params![user.id, user.username, user.name, user.bio, i64::from(user.avatar), unix_now()],
    )?;
    Ok(())
}

pub fn author(id: &str) -> Result<Option<AuthorRow>> {
    Database.fetch_optional(
        "SELECT id, username, display_name, bio, avatar FROM authors WHERE id=?",
        params![id],
    )
}

pub fn post_count(author_id: &str) -> Result<i64> {
    #[derive(Deserialize)]
    struct Count {
        count: i64,
    }
    Ok(Database
        .fetch_one::<Count>(
            "SELECT count(*) AS count FROM posts WHERE author_id=?",
            params![author_id],
        )?
        .count)
}

/// Newest first, a page at a time, with each post's counts for `viewer`. Also
/// says whether older posts follow.
pub fn posts(viewer: &str, filter: Filter) -> Result<(Vec<PostRow>, bool)> {
    let mut sql = String::from(
        "SELECT p.id AS id, p.author_id AS author_id, p.body AS body, p.created AS created, \
         a.username AS username, a.display_name AS display_name, a.avatar AS avatar, \
         (SELECT count(*) FROM replies r WHERE r.post_id = p.id) AS replies, \
         (SELECT count(*) FROM likes l WHERE l.post_id = p.id) AS likes, \
         EXISTS(SELECT 1 FROM likes l WHERE l.post_id = p.id AND l.user_id = ?) AS liked \
         FROM posts p JOIN authors a ON a.id = p.author_id WHERE 1",
    );
    let mut values: Vec<Value> = vec![json!(viewer)];
    if let Some(author) = filter.author {
        sql.push_str(" AND p.author_id = ?");
        values.push(json!(author));
    }
    if let Some(post) = filter.post {
        sql.push_str(" AND p.id = ?");
        values.push(json!(post));
    }
    if let Some((created, id)) = filter.before {
        sql.push_str(" AND (p.created < ? OR (p.created = ? AND p.id < ?))");
        values.extend([json!(created), json!(created), json!(id)]);
    }
    // One extra row tells whether there is another page.
    sql.push_str(&format!(
        " ORDER BY p.created DESC, p.id DESC LIMIT {}",
        PAGE + 1
    ));
    let mut rows: Vec<PostRow> = Database.fetch_all(&sql, values)?;
    let more = rows.len() > PAGE;
    rows.truncate(PAGE);
    Ok((rows, more))
}

pub fn post(viewer: &str, id: &str) -> Result<Option<PostRow>> {
    let filter = Filter {
        post: Some(id),
        ..Filter::default()
    };
    Ok(posts(viewer, filter)?.0.pop())
}

/// Oldest first, so a conversation reads top to bottom. Replies in the same
/// second keep the order they were written in.
pub fn replies(post_id: &str) -> Result<Vec<ReplyRow>> {
    Database.fetch_all(
        "SELECT r.id AS id, r.author_id AS author_id, r.body AS body, r.created AS created, \
         a.username AS username, a.display_name AS display_name, a.avatar AS avatar \
         FROM replies r JOIN authors a ON a.id = r.author_id \
         WHERE r.post_id = ? ORDER BY r.created, r.rowid LIMIT 500",
        params![post_id],
    )
}

pub fn create_post(author_id: &str, body: &str) -> Result<String> {
    let id = loki_sdk::random_id();
    Database.execute(
        "INSERT INTO posts(id,author_id,body,created) VALUES(?,?,?,?)",
        params![id, author_id, body, unix_now()],
    )?;
    Ok(id)
}

/// Who wrote a post or reply, if it exists.
pub fn owner(table: Owned, id: &str) -> Result<Option<String>> {
    #[derive(Deserialize)]
    struct Owner {
        author_id: String,
    }
    let sql = match table {
        Owned::Post => "SELECT author_id FROM posts WHERE id=?",
        Owned::Reply => "SELECT author_id FROM replies WHERE id=?",
    };
    Ok(Database
        .fetch_optional::<Owner>(sql, params![id])?
        .map(|o| o.author_id))
}
pub enum Owned {
    Post,
    Reply,
}

/// Removes a post with its replies and likes.
pub fn delete_post(id: &str) -> Result<()> {
    Database.transaction(&[
        Statement::new("DELETE FROM likes WHERE post_id=?", params![id])?,
        Statement::new("DELETE FROM replies WHERE post_id=?", params![id])?,
        Statement::new("DELETE FROM posts WHERE id=?", params![id])?,
    ])?;
    Ok(())
}

/// Likes a post, or unlikes it if `user_id` already did. False when there is no
/// such post.
pub fn toggle_like(post_id: &str, user_id: &str) -> Result<bool> {
    if Database.execute(
        "DELETE FROM likes WHERE post_id=? AND user_id=?",
        params![post_id, user_id],
    )? > 0
    {
        return Ok(true);
    }
    let added = Database.execute(
        "INSERT OR IGNORE INTO likes(post_id,user_id,created) SELECT ?,?,? WHERE EXISTS(SELECT 1 FROM posts WHERE id=?)",
        params![post_id, user_id, unix_now(), post_id],
    )?;
    Ok(added > 0)
}

/// False when there is no such post.
pub fn create_reply(post_id: &str, author_id: &str, body: &str) -> Result<bool> {
    let added = Database.execute(
        "INSERT INTO replies(id,post_id,author_id,body,created) SELECT ?,?,?,?,? WHERE EXISTS(SELECT 1 FROM posts WHERE id=?)",
        params![loki_sdk::random_id(), post_id, author_id, body, unix_now(), post_id],
    )?;
    Ok(added > 0)
}

pub fn delete_reply(id: &str) -> Result<()> {
    Database.execute("DELETE FROM replies WHERE id=?", params![id])?;
    Ok(())
}
