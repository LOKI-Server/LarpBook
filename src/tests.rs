//! Handler tests. The SDK's fake host runs the app's SQL against a real in-memory
//! SQLite, with foreign keys on as in LOKI.
use super::*;
use loki_sdk::{Method, testing};
use rusqlite::types::{Value as Sql, ValueRef};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;

const ADA: &str = "11111111-1111-4111-8111-111111111111";
const GRACE: &str = "22222222-2222-4222-8222-222222222222";
const MISSING: &str = "33333333-3333-4333-8333-333333333333";

thread_local! {
    static DB: RefCell<Option<rusqlite::Connection>> = const { RefCell::new(None) };
    static FORM: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
}

/// Runs the statements in one transaction, as LOKI does.
fn database(input: &str) -> std::result::Result<String, String> {
    let input: Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
    DB.with(|db| {
        let mut db = db.borrow_mut();
        let tx = db.as_mut().unwrap().transaction().map_err(host_error)?;
        let mut results = Vec::new();
        for statement in input["statements"].as_array().unwrap() {
            let params: Vec<Sql> = statement["params"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| match v {
                    Value::Null => Sql::Null,
                    Value::Bool(b) => Sql::Integer(i64::from(*b)),
                    Value::Number(n) => n
                        .as_i64()
                        .map_or(Sql::Real(n.as_f64().unwrap()), Sql::Integer),
                    Value::String(s) => Sql::Text(s.clone()),
                    other => panic!("non-scalar SQL parameter {other}"),
                })
                .collect();
            let mut prepared = tx
                .prepare(statement["sql"].as_str().unwrap())
                .map_err(host_error)?;
            let params = rusqlite::params_from_iter(params);
            if prepared.column_count() == 0 {
                let affected = prepared.execute(params).map_err(host_error)?;
                results.push(json!({"rows":[],"affected":affected}));
                continue;
            }
            let names: Vec<String> = prepared.column_names().map(str::to_string).collect();
            let mut rows = prepared.query(params).map_err(host_error)?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().map_err(host_error)? {
                let mut object = serde_json::Map::new();
                for (i, name) in names.iter().enumerate() {
                    let value = match row.get_ref(i).map_err(host_error)? {
                        ValueRef::Null => Value::Null,
                        ValueRef::Integer(n) => json!(n),
                        ValueRef::Real(n) => json!(n),
                        ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
                        ValueRef::Blob(_) => panic!("blob in a result"),
                    };
                    object.insert(name.clone(), value);
                }
                out.push(Value::Object(object));
            }
            results.push(json!({"rows":out,"affected":0}));
        }
        tx.commit().map_err(host_error)?;
        Ok(Value::Array(results).to_string())
    })
}
fn host_error(error: rusqlite::Error) -> String {
    json!({"loki_error":1,"status":400,"message":error.to_string()}).to_string()
}
fn host(operation: &str, input: &str) -> std::result::Result<String, String> {
    match operation {
        "database" => database(input),
        "form" => Ok(json!({"fields":FORM.with(|f| f.borrow().clone()),"files":[]}).to_string()),
        other => panic!("unexpected host operation {other}"),
    }
}
/// A fresh database with LarpBook's schema, as after LOKI's health check.
fn with_app(test: impl FnOnce()) {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    DB.with(|db| *db.borrow_mut() = Some(connection));
    FORM.with(|f| f.borrow_mut().clear());
    testing::with_host(host, || {
        assert_eq!(get("/health", &ada()).status, 200);
        test();
    });
}
/// Reads or seeds the test database directly.
fn sql<T>(read: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T>) -> T {
    DB.with(|db| read(db.borrow().as_ref().unwrap()).unwrap())
}
fn count(table: &str) -> i64 {
    sql(|db| db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)))
}

fn person(
    id: &str,
    username: &str,
    name: &str,
    bio: &str,
    avatar: bool,
    admin: bool,
) -> loki_sdk::Context {
    let roles: &[&str] = if admin {
        &["access", "admin"]
    } else {
        &["access"]
    };
    serde_json::from_value(json!({
        "app_id": "larpbook",
        "origin": "http://larpbook.localhost:7070",
        "issuer": "http://127.0.0.1:7070",
        "dashboard_url": "http://127.0.0.1:7070",
        "csrf": "token",
        "user": {"id":id,"username":username,"name":name,"email":"hidden@example.com","bio":bio,"avatar":avatar,"roles":roles}
    }))
    .unwrap()
}
fn ada() -> loki_sdk::Context {
    person(ADA, "ada", "Ada Lovelace", "Counts stars", true, false)
}
fn grace() -> loki_sdk::Context {
    person(GRACE, "grace", "Grace Hopper", "", false, false)
}
fn admin() -> loki_sdk::Context {
    person(GRACE, "grace", "Grace Hopper", "", false, true)
}
fn get(path: &str, who: &loki_sdk::Context) -> Response {
    handle(testing::request(Method::Get, path, who.clone())).unwrap()
}
fn post(path: &str, who: &loki_sdk::Context, fields: &[(&str, &str)]) -> Result<Response> {
    FORM.with(|f| {
        *f.borrow_mut() = fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
    });
    handle(testing::request(Method::Post, path, who.clone()))
}
fn html(response: &Response) -> String {
    String::from_utf8(response.body.clone()).unwrap()
}
fn location(response: &Response) -> &str {
    response
        .headers
        .get("location")
        .map(String::as_str)
        .unwrap_or("")
}
/// Posts as `who` and returns the new post's ID.
fn write(who: &loki_sdk::Context, body: &str) -> String {
    let response = post("/posts", who, &[("body", body)]).unwrap();
    assert_eq!(response.status, 303, "{}", html(&response));
    sql(|db| {
        db.query_row(
            "SELECT id FROM posts ORDER BY rowid DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
    })
}

#[test]
fn posts_are_escaped_linked_and_show_the_loki_profile() {
    with_app(|| {
        write(
            &ada(),
            "  hello <script>alert(1)</script> https://example.com/x  ",
        );
        let page = html(&get("/", &ada()));
        assert!(page.contains(
            "hello &lt;script&gt;alert(1)&lt;/script&gt; <a href=\"https://example.com/x\""
        ));
        assert!(!page.contains("<script>alert"));
        assert!(!page.contains("hidden@example.com"));
        assert!(
            page.contains("http://127.0.0.1:7070/avatars/11111111-1111-4111-8111-111111111111")
        );
        assert!(page.contains("@ada"));
        // Bios belong on the profile, not repeated on every post.
        assert!(!page.contains("Counts stars"));
        let profile = html(&get(&format!("/u/{ADA}"), &grace()));
        assert!(profile.contains("Counts stars"));
        assert!(profile.contains("1 post"));
        assert!(!profile.contains("Change them there"));
        assert!(html(&get(&format!("/u/{ADA}"), &ada())).contains("Change them there"));
    });
}

#[test]
fn invalid_posts_are_refused_and_kept_as_drafts() {
    with_app(|| {
        for body in [
            "",
            "   \n  ",
            "hello\u{0001}",
            &"a".repeat(text::MAX_CHARS + 1),
        ] {
            let response = post("/posts", &ada(), &[("body", body)]).unwrap();
            assert_eq!(response.status, 400, "{body:?}");
        }
        let response = post("/posts", &ada(), &[("body", "bad\u{0001}<b>")]).unwrap();
        assert!(html(&response).contains("bad\u{0001}&lt;b&gt;</textarea>"));
        assert_eq!(count("posts"), 0);
    });
}

#[test]
fn likes_toggle_per_person() {
    with_app(|| {
        let id = write(&ada(), "like me");
        let back = format!("/u/{ADA}");
        let liked = post(&format!("/p/{id}/like"), &grace(), &[("back", &back)]).unwrap();
        assert_eq!(location(&liked), format!("{back}#p-{id}"));
        post(&format!("/p/{id}/like"), &ada(), &[]).unwrap();
        assert_eq!(count("likes"), 2);
        let page = html(&get("/", &grace()));
        assert!(page.contains("class=\"like liked\""));
        assert!(page.contains("♥</span> 2"));
        post(&format!("/p/{id}/like"), &grace(), &[]).unwrap();
        assert_eq!(count("likes"), 1);
        assert!(html(&get("/", &grace())).contains("♡</span> 1"));
        let gone = post(&format!("/p/{MISSING}/like"), &grace(), &[]).unwrap_err();
        assert_eq!(gone.status(), 404);
        // An outside address is never a redirect target.
        let away = post(
            &format!("/p/{id}/like"),
            &grace(),
            &[("back", "https://evil.example")],
        )
        .unwrap();
        assert_eq!(location(&away), format!("/#p-{id}"));
    });
}

#[test]
fn replies_form_a_thread_under_the_post() {
    with_app(|| {
        let id = write(&ada(), "question?");
        let reply = post(
            &format!("/p/{id}/replies"),
            &grace(),
            &[("body", "answer <i>")],
        )
        .unwrap();
        assert_eq!(location(&reply), format!("/p/{id}#replies"));
        post(&format!("/p/{id}/replies"), &ada(), &[("body", "thanks")]).unwrap();
        let thread = html(&get(&format!("/p/{id}"), &ada()));
        assert!(thread.contains("2 replies"));
        let (answer, thanks) = (
            thread.find("answer &lt;i&gt;").unwrap(),
            thread.find("thanks").unwrap(),
        );
        assert!(answer < thanks, "replies read oldest first");
        assert!(html(&get("/", &ada())).contains("2 replies"));
        let empty = post(&format!("/p/{id}/replies"), &grace(), &[("body", " ")]).unwrap();
        assert_eq!(empty.status, 400);
        assert!(html(&empty).contains("Write something first."));
        let gone = post(
            &format!("/p/{MISSING}/replies"),
            &grace(),
            &[("body", "hi")],
        )
        .unwrap_err();
        assert_eq!(gone.status(), 404);
    });
}

#[test]
fn people_delete_their_own_posts_and_replies_and_admins_any() {
    with_app(|| {
        let mine = write(&ada(), "from ada");
        let theirs = write(&grace(), "from grace");
        post(
            &format!("/p/{theirs}/replies"),
            &grace(),
            &[("body", "note")],
        )
        .unwrap();
        post(&format!("/p/{theirs}/like"), &ada(), &[]).unwrap();
        let reply: String = sql(|db| db.query_row("SELECT id FROM replies", [], |r| r.get(0)));

        assert_eq!(
            post(&format!("/p/{theirs}/delete"), &ada(), &[])
                .unwrap_err()
                .status(),
            403
        );
        assert_eq!(
            post(&format!("/replies/{reply}/delete"), &ada(), &[])
                .unwrap_err()
                .status(),
            403
        );
        assert_eq!(count("posts"), 2);

        let deleted = post(
            &format!("/p/{mine}/delete"),
            &ada(),
            &[("back", &format!("/p/{mine}"))],
        )
        .unwrap();
        assert_eq!(location(&deleted), "/");
        assert_eq!(count("posts"), 1);

        post(&format!("/p/{theirs}/delete"), &admin(), &[]).unwrap();
        assert_eq!(
            (count("posts"), count("replies"), count("likes")),
            (0, 0, 0)
        );
        // Deleting what is already gone is not an error.
        assert_eq!(
            post(&format!("/p/{theirs}/delete"), &admin(), &[])
                .unwrap()
                .status,
            303
        );
    });
}

#[test]
fn feeds_page_by_time_without_skipping_or_repeating() {
    with_app(|| {
        sql(|db| {
            db.execute(
                "INSERT INTO authors(id,username,display_name,updated) VALUES(?1,'ada','Ada',0)",
                [ADA],
            )?;
            for i in 0..(store::PAGE + 5) {
                // Pairs share a timestamp, so the ID has to break ties.
                db.execute(
                    "INSERT INTO posts(id,author_id,body,created) VALUES(?1,?2,?3,?4)",
                    rusqlite::params![
                        format!("00000000-0000-4000-8000-{i:012}"),
                        ADA,
                        format!("post {i:02}"),
                        (i / 2) as i64
                    ],
                )?;
            }
            Ok(())
        });
        let first = html(&get("/", &ada()));
        assert_eq!(first.matches("class=\"post\"").count(), store::PAGE);
        assert!(first.contains("post 44"));
        let older = first
            .split("<p class=\"more\"><a href=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .replace("&amp;", "&");
        let second = html(&get(&older, &ada()));
        assert_eq!(second.matches("class=\"post\"").count(), 5);
        for i in 0..5 {
            assert!(second.contains(&format!("post {i:02}")), "{i}");
        }
        assert!(!second.contains("Older posts"));
        let bad = testing::request(Method::Get, "/?before_created=x&before_id=y", ada());
        assert_eq!(handle(bad).unwrap_err().status(), 400);
        let profile = html(&get(&format!("/u/{ADA}"), &ada()));
        assert!(profile.contains(&format!("/u/{ADA}?before_created=")));
        assert!(profile.contains("45 posts"));
    });
}

#[test]
fn missing_people_and_posts_are_not_found() {
    with_app(|| {
        let person = get(&format!("/u/{MISSING}"), &grace());
        assert_eq!(person.status, 404);
        assert!(html(&person).contains("has not opened LarpBook"));
        assert_eq!(get("/u/ada", &grace()).status, 404);
        assert_eq!(get(&format!("/p/{MISSING}"), &grace()).status, 404);
        let page = html(&get("/", &grace()));
        assert!(!page.contains("/avatars/"));
        assert!(page.contains("<span>G</span>"));
    });
}

#[test]
fn back_links_stay_inside_larpbook() {
    for (input, expected) in [
        ("/", "/"),
        (&*format!("/u/{ADA}"), &*format!("/u/{ADA}")),
        (&*format!("/p/{ADA}"), &*format!("/p/{ADA}")),
        ("//evil.example", "/"),
        ("/u/../admin", "/"),
        ("https://evil.example", "/"),
    ] {
        assert_eq!(safe_back(input), expected, "{input}");
    }
}
