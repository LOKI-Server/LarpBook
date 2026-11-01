//! What the templates show, built from LOKI's user and the stored rows.
use crate::store::{AuthorRow, PostRow, ReplyRow};
use crate::text::{format_time, linkify};
use loki_sdk::User;

pub struct Person {
    pub id: String,
    pub name: String,
    pub username: String,
    pub bio: String,
    /// Shown while the picture loads, or when there is none.
    pub initial: String,
    /// Empty without a LOKI profile picture.
    pub avatar: String,
}
impl Person {
    pub fn from_user(user: &User, dashboard: &str) -> Self {
        Self::new(
            &user.id,
            &user.name,
            &user.username,
            &user.bio,
            user.avatar,
            dashboard,
        )
    }
    pub fn from_author(author: &AuthorRow, dashboard: &str) -> Self {
        let avatar = author.avatar != 0;
        Self::new(
            &author.id,
            &author.display_name,
            &author.username,
            &author.bio,
            avatar,
            dashboard,
        )
    }
    fn new(id: &str, name: &str, username: &str, bio: &str, avatar: bool, dashboard: &str) -> Self {
        let name = name.trim();
        let name = if name.is_empty() { username } else { name };
        let dashboard = dashboard.trim_end_matches('/');
        Self {
            id: id.into(),
            name: name.into(),
            username: username.into(),
            bio: bio.into(),
            initial: name
                .chars()
                .next()
                .map(|c| c.to_uppercase().collect())
                .unwrap_or_else(|| "?".into()),
            avatar: if avatar && !dashboard.is_empty() {
                format!("{dashboard}/avatars/{id}")
            } else {
                String::new()
            },
        }
    }
}

/// When something was written, for `<time>`.
pub struct When {
    pub label: String,
    pub iso: String,
}
impl When {
    fn new(unix: i64) -> Self {
        let (label, iso) = format_time(unix);
        Self { label, iso }
    }
}

pub struct Card {
    pub id: String,
    pub author: Person,
    /// Escaped text with clickable links.
    pub html: String,
    pub when: When,
    pub can_delete: bool,
    pub likes: i64,
    pub liked: bool,
    pub replies: i64,
}
impl Card {
    pub fn new(row: &PostRow, viewer: &User, dashboard: &str) -> Self {
        Self {
            id: row.id.clone(),
            author: Person::new(
                &row.author_id,
                &row.display_name,
                &row.username,
                "",
                row.avatar != 0,
                dashboard,
            ),
            html: linkify(&row.body),
            when: When::new(row.created),
            can_delete: row.author_id == viewer.id || viewer.has_role("admin"),
            likes: row.likes,
            liked: row.liked != 0,
            replies: row.replies,
        }
    }
}

pub struct ReplyCard {
    pub id: String,
    pub author: Person,
    pub html: String,
    pub when: When,
    pub can_delete: bool,
}
impl ReplyCard {
    pub fn new(row: &ReplyRow, viewer: &User, dashboard: &str) -> Self {
        Self {
            id: row.id.clone(),
            author: Person::new(
                &row.author_id,
                &row.display_name,
                &row.username,
                "",
                row.avatar != 0,
                dashboard,
            ),
            html: linkify(&row.body),
            when: When::new(row.created),
            can_delete: row.author_id == viewer.id || viewer.has_role("admin"),
        }
    }
}

/// What every page has: who is signed in, and where actions return to.
pub struct Chrome {
    pub viewer: Person,
    pub account_url: String,
    pub csrf: String,
    /// This page's path, so forms can come back to it.
    pub here: String,
}

#[derive(askama::Template)]
#[template(path = "feed.html")]
pub struct FeedPage {
    pub chrome: Chrome,
    pub draft: String,
    pub error: String,
    pub posts: Vec<Card>,
    pub older: String,
}

#[derive(askama::Template)]
#[template(path = "post.html")]
pub struct PostPage {
    pub chrome: Chrome,
    pub post: Card,
    pub replies: Vec<ReplyCard>,
    pub draft: String,
    pub error: String,
}

#[derive(askama::Template)]
#[template(path = "profile.html")]
pub struct ProfilePage {
    pub chrome: Chrome,
    pub subject: Person,
    pub post_count: i64,
    pub posts: Vec<Card>,
    pub older: String,
}

#[derive(askama::Template)]
#[template(path = "missing.html")]
pub struct MissingPage {
    pub chrome: Chrome,
    pub message: String,
}
