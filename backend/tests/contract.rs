//! The other half of the OpenAPI gate.
//!
//! `openapi.rs`'s own test proves the document and the hardcoded contract agree with
//! each other. Both could still describe an endpoint nobody built — a path list is
//! just text. So this drives every documented path against the **real router** and
//! asserts none of them falls through.
//!
//! The subtlety is that a handler's own 404 ("Nicht gefunden: Buchung") and the
//! router's fallback 404 ("Nicht gefunden: GET /api/v1/tippfehler") are the same
//! status code. Looking only at the status would either pass vacuously or fail on
//! every `/{id}` route, so the fallback is recognised by the message it builds from
//! the method and path — which is exactly what makes it distinguishable.

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;

use finanzen::{AppState, Config, db::Db, openapi};

mod common;

const ORIGIN: &str = "http://localhost:3100";

async fn app() -> Option<(Router, String)> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .ok()?;
    // Databases from earlier runs, dropped before another is made. An hour
    // is longer than any run, so nothing in use is ever a candidate.
    common::reap_stale(&admin, 900).await;

    let name = common::database_name("fin_contract");
    let role = common::role_name("fin_contract", &name);
    sqlx::query(&format!(
        "CREATE ROLE {role} LOGIN PASSWORD 'test' NOSUPERUSER NOBYPASSRLS"
    ))
    .execute(&admin)
    .await
    .expect("create role");
    sqlx::query(&format!("CREATE DATABASE {name} OWNER {role}"))
        .execute(&admin)
        .await
        .expect("create database");
    admin.close().await;

    let mut target = url::Url::parse(&url).expect("url");
    target.set_path(&name);
    target.set_username(&role).ok()?;
    target.set_password(Some("test")).ok()?;

    let mut config = Config::test(target.as_str());
    config.public_url = ORIGIN.to_string();
    let db = finanzen::db::connect(&config)
        .await
        .expect("connect + migrate");
    let _ = db;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(target.as_str())
        .await
        .ok()?;
    let router = finanzen::router(AppState::new(Db::from_pool(pool), config));

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/setup")
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"username":"fabi","displayName":"Fabian","password":"ein-langes-passwort"})
                .to_string(),
        ))
        .unwrap();
    let response = router.clone().oneshot(req).await.expect("setup");
    assert_eq!(response.status(), StatusCode::CREATED);
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(str::to_string)
        .expect("session cookie");
    Some((router, cookie))
}

/// Fills the path template with values the extractors will accept. A miss here is
/// a 400, which is a perfectly good answer for this test — the only answers it
/// rejects are "no such route" and "wrong method".
fn concrete(path: &str) -> String {
    path.replace("{id}", &Uuid::new_v4().to_string())
        .replace("{bookingId}", &Uuid::new_v4().to_string())
        .replace("{year}", "2026")
}

#[tokio::test]
async fn every_documented_path_is_reachable_on_the_real_router() {
    let Some((router, cookie)) = app().await else {
        return; // no TEST_DATABASE_URL, as elsewhere in this suite
    };

    let doc: Value = serde_json::from_str(&openapi::document()).expect("valid document");
    let paths = doc["paths"].as_object().expect("paths");
    assert!(!paths.is_empty(), "the document must describe something");

    let mut checked = 0usize;
    for (template, item) in paths {
        for method in item.as_object().expect("path item").keys() {
            let path = concrete(template);
            let mut req = Request::builder()
                .method(method.to_uppercase().as_str())
                .uri(&path)
                .header(header::ORIGIN, ORIGIN)
                .header(header::COOKIE, &cookie);
            if method != "get" {
                req = req.header(header::CONTENT_TYPE, "application/json");
            }
            let request = req.body(Body::from("{}")).unwrap();
            let response = router.clone().oneshot(request).await.expect("request");
            let status = response.status();
            let bytes = response
                .into_body()
                .collect()
                .await
                .expect("body")
                .to_bytes();
            let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);

            assert_ne!(
                status,
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} {template}: documented but the router accepts a different method"
            );
            // The router fallback builds its message from the method and the path.
            // A handler's own "Nicht gefunden: Buchung" is a different, legitimate
            // answer to a random uuid.
            let fallback = format!("Nicht gefunden: {} {}", method.to_uppercase(), path);
            assert_ne!(
                body["message"].as_str(),
                Some(fallback.as_str()),
                "{method} {template}: documented but no route is mounted there"
            );
            assert_ne!(
                status,
                StatusCode::INTERNAL_SERVER_ERROR,
                "{method} {template}: documented and mounted, but panics on a plain request: {body}"
            );
            checked += 1;
        }
    }
    assert_eq!(
        checked,
        openapi::CONTRACT.len(),
        "every contracted operation must have been driven"
    );
}

/// The committed `openapi.json` must be what the binary prints. CI enforces this
/// with `git diff --exit-code` after regenerating; this catches it locally, before
/// the push, with a message that says what to run.
#[test]
fn the_committed_document_is_up_to_date() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("openapi.json");
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("openapi.json fehlt ({e}) — `cargo run --bin openapi-export > openapi.json`")
    });
    assert_eq!(
        committed,
        openapi::document(),
        "openapi.json ist veraltet — `cargo run --bin openapi-export > openapi.json` und das Ergebnis mitcommitten"
    );
}
