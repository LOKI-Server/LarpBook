//! LarpBook: a shared text feed for the people on a LOKI server. Accounts, names,
//! bios and pictures all come from LOKI.
mod store;
mod text;
mod view;

use loki_sdk::{Error, Request, Response, Result, Router};
use store::{Filter, Owned};
use text::{clean_body, is_id};
use view::{Card, Chrome, FeedPage, MissingPage, Person, PostPage, ProfilePage, ReplyCard};

fn handle(request: Request) -> Result<Response> {
    Router::new()
        .get("/health", health)
        .get("/", feed)
        .post("/posts", create_post)
        .get("/p/{id}", post_page)
        .post("/p/{id}/like", like)
        .post("/p/{id}/replies", reply)
        .post("/p/{id}/delete", delete_post)
        .post("/replies/{id}/delete", delete_reply)
        .get("/u/{id}", profile)
        .get("/static/style.css", |_| loki_sdk::asset("static/style.css"))
        .get("/static/app.js", |_| loki_sdk::asset("static/app.js"))
        .handle(request)
}
loki_sdk::export!(handle);

fn health(_: Request) -> Result<Response> {
    store::create_schema()?;
    Ok(Response::text("ready"))
}

fn feed(request: Request) -> Result<Response> {
    show_feed(&request, "", "")
}

fn create_post(request: Request) -> Result<Response> {
    let user = signed_in(&request)?;
    let draft = request.form()?.field("body").unwrap_or("").to_string();
    match clean_body(&draft) {
        Ok(body) => {
            store::create_post(&user.id, &body)?;
            Response::redirect("/")
        }
        Err(message) => show_feed(&request, &message, &draft),
    }
}

fn post_page(request: Request) -> Result<Response> {
    show_post(&request, request.param("id")?, "", "")
}

fn like(request: Request) -> Result<Response> {
    let user = signed_in(&request)?;
    let id = post_id(&request)?;
    let back = back(&request)?;
    if !store::toggle_like(&id, &user.id)? {
        return Err(Error::not_found("That post was deleted."));
    }
    Response::redirect(&format!("{back}#p-{id}"))
}

fn reply(request: Request) -> Result<Response> {
    let user = signed_in(&request)?;
    let id = post_id(&request)?;
    let draft = request.form()?.field("body").unwrap_or("").to_string();
    match clean_body(&draft) {
        Ok(body) => {
            if !store::create_reply(&id, &user.id, &body)? {
                return Err(Error::not_found("That post was deleted."));
            }
            Response::redirect(&format!("/p/{id}#replies"))
        }
        Err(message) => show_post(&request, &id, &message, &draft),
    }
}

fn delete_post(request: Request) -> Result<Response> {
    let user = signed_in(&request)?;
    let id = post_id(&request)?;
    let back = back(&request)?;
    if let Some(author) = store::owner(Owned::Post, &id)? {
        if author != user.id && !user.has_role("admin") {
            return Err(Error::forbidden());
        }
        store::delete_post(&id)?;
    }
    // Its own page is gone with it.
    Response::redirect(if back.starts_with("/p/") { "/" } else { &back })
}

fn delete_reply(request: Request) -> Result<Response> {
    let user = signed_in(&request)?;
    let id = request.param("id")?.to_string();
    let back = back(&request)?;
    if is_id(&id)
        && let Some(author) = store::owner(Owned::Reply, &id)?
    {
        if author != user.id && !user.has_role("admin") {
            return Err(Error::forbidden());
        }
        store::delete_reply(&id)?;
    }
    Response::redirect(&format!("{back}#replies"))
}

fn profile(request: Request) -> Result<Response> {
    let user = signed_in(&request)?;
    let id = request.param("id")?.to_string();
    let dashboard = dashboard(&request);
    let chrome = chrome(&request, format!("/u/{id}"));
    let Some(author) = is_id(&id)
        .then(|| store::author(&id))
        .transpose()?
        .flatten()
    else {
        return missing(chrome, "This person has not opened LarpBook yet.");
    };
    let before = cursor(&request)?;
    let filter = Filter {
        author: Some(&id),
        before: before.as_ref().map(|(c, i)| (*c, i.as_str())),
        ..Filter::default()
    };
    let (rows, more) = store::posts(&user.id, filter)?;
    let older = older(&chrome.here, &rows, more);
    Response::render(ProfilePage {
        subject: Person::from_author(&author, dashboard),
        post_count: store::post_count(&id)?,
        posts: rows.iter().map(|r| Card::new(r, user, dashboard)).collect(),
        older,
        chrome,
    })
}

fn show_feed(request: &Request, error: &str, draft: &str) -> Result<Response> {
    let user = signed_in(request)?;
    let dashboard = dashboard(request);
    let before = cursor(request)?;
    let filter = Filter {
        before: before.as_ref().map(|(c, i)| (*c, i.as_str())),
        ..Filter::default()
    };
    let (rows, more) = store::posts(&user.id, filter)?;
    let chrome = chrome(request, "/".into());
    let page = Response::render(FeedPage {
        older: older(&chrome.here, &rows, more),
        posts: rows.iter().map(|r| Card::new(r, user, dashboard)).collect(),
        draft: draft.into(),
        error: error.into(),
        chrome,
    })?;
    Ok(if error.is_empty() {
        page
    } else {
        page.with_status(400)
    })
}

fn show_post(request: &Request, id: &str, error: &str, draft: &str) -> Result<Response> {
    let user = signed_in(request)?;
    let dashboard = dashboard(request);
    let chrome = chrome(request, format!("/p/{id}"));
    let Some(row) = is_id(id)
        .then(|| store::post(&user.id, id))
        .transpose()?
        .flatten()
    else {
        return missing(chrome, "This post was deleted, or never existed.");
    };
    let page = Response::render(PostPage {
        post: Card::new(&row, user, dashboard),
        replies: store::replies(id)?
            .iter()
            .map(|r| ReplyCard::new(r, user, dashboard))
            .collect(),
        draft: draft.into(),
        error: error.into(),
        chrome,
    })?;
    Ok(if error.is_empty() {
        page
    } else {
        page.with_status(400)
    })
}

fn missing(chrome: Chrome, message: &str) -> Result<Response> {
    Ok(Response::render(MissingPage {
        chrome,
        message: message.into(),
    })?
    .with_status(404))
}

/// The signed-in person, with their LOKI profile copied for others to see.
fn signed_in(request: &Request) -> Result<&loki_sdk::User> {
    let user = request.context().require_user()?;
    store::remember(user)?;
    Ok(user)
}

fn dashboard(request: &Request) -> &str {
    request.context().dashboard_url.as_str()
}

fn chrome(request: &Request, here: String) -> Chrome {
    let context = request.context();
    Chrome {
        viewer: context
            .user
            .as_ref()
            .map(|u| Person::from_user(u, &context.dashboard_url))
            .expect("signed in"),
        account_url: format!("{}/account", context.dashboard_url.trim_end_matches('/')),
        csrf: context.csrf_token().into(),
        here,
    }
}

fn post_id(request: &Request) -> Result<String> {
    let id = request.param("id")?;
    if is_id(id) {
        Ok(id.into())
    } else {
        Err(Error::not_found("Post not found."))
    }
}

/// Where a form came from, if it is one of LarpBook's own pages.
fn back(request: &Request) -> Result<String> {
    Ok(safe_back(request.form()?.field("back").unwrap_or("/")))
}
fn safe_back(value: &str) -> String {
    let page = ["/u/", "/p/"]
        .iter()
        .any(|prefix| value.strip_prefix(prefix).is_some_and(is_id));
    if page { value.into() } else { "/".into() }
}

/// The `before_created`/`before_id` page cursor, if any.
fn cursor(request: &Request) -> Result<Option<(i64, String)>> {
    match (request.query("before_created"), request.query("before_id")) {
        (None, None) => Ok(None),
        (Some(created), Some(id)) if is_id(&id) => created
            .parse()
            .map(|created| Some((created, id)))
            .map_err(|_| Error::bad("That page link is not valid.")),
        _ => Err(Error::bad("That page link is not valid.")),
    }
}

fn older(here: &str, rows: &[store::PostRow], more: bool) -> String {
    match rows.last() {
        Some(last) if more => format!(
            "{here}?before_created={}&before_id={}",
            last.created, last.id
        ),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests;
