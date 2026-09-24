//! HTTP-level tests against the real router and a real Postgres.
//!
//! These pin the behaviours that are easy to break and expensive to get wrong:
//! category resolution precedence, retroactive recategorisation, netting on the
//! wire, and the CSRF/auth gates.
//!
//! Needs TEST_DATABASE_URL pointing at a superuser connection; each test builds its
//! own database owned by a fresh NOSUPERUSER role, because RLS is inert for a
//! superuser and every assertion here would then pass vacuously.

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

use finanzen::{AppState, Config, db::Db};

mod common;

const ORIGIN: &str = "http://localhost:3100";

struct TestApp {
    router: Router,
    cookie: Option<String>,
    /// A per-test APP_DATA_DIR, so the receipt tests can assert that nothing was
    /// written outside the tenant's own subtree.
    data_dir: std::path::PathBuf,
}

impl TestApp {
    async fn new() -> Option<Self> {
        let url = std::env::var("TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .ok()?;

        // Databases from earlier runs, dropped before another is made. An hour
        // is longer than any run, so nothing in use is ever a candidate.
        common::reap_stale(&admin, 900).await;

        let name = common::database_name("fin_api");
        let role = common::role_name("fin_api", &name);
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
        let data_dir = std::env::temp_dir().join(format!("finanzen-test-{}", Uuid::new_v4()));
        config.data_dir = data_dir.clone();
        // The app owns its tables so it can migrate; FORCE ROW LEVEL SECURITY means
        // policies still apply to it.
        let db = finanzen::db::connect(&config)
            .await
            .expect("connect + migrate");
        let _ = db;

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(target.as_str())
            .await
            .ok()?;
        let state = AppState::new(Db::from_pool(pool), config);
        Some(Self {
            router: finanzen::router(state),
            cookie: None,
            data_dir,
        })
    }

    /// Sends as a specific session, so one test can drive two tenants.
    async fn send_as(
        &self,
        cookie: Option<&str>,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(format!("/api/v1{path}"))
            .header(header::ORIGIN, ORIGIN);
        if let Some(c) = cookie {
            req = req.header(header::COOKIE, c);
        }
        let req = match body {
            Some(v) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(v.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let response = self.router.clone().oneshot(req).await.expect("request");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }

    /// A GET whose body is not JSON — the CSV, the PDF and a receipt.
    async fn get_raw(&self, path: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let req = Request::builder()
            .method("GET")
            .uri(format!("/api/v1{path}"))
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, self.cookie.clone().expect("session"))
            .body(Body::empty())
            .unwrap();
        let response = self.router.clone().oneshot(req).await.expect("request");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, headers, bytes.to_vec())
    }

    /// Multipart upload of arbitrary bytes, so a receipt test can choose the
    /// filename and the declared content type — which is the whole attack surface.
    async fn upload_bytes(
        &self,
        path: &str,
        filename: &str,
        content_type: &str,
        bytes: &[u8],
    ) -> (StatusCode, Value) {
        let boundary = "----finanzen-test-boundary";
        let mut body = Vec::new();
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
                 filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1{path}"))
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, self.cookie.clone().expect("session"))
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        let response = self.router.clone().oneshot(req).await.expect("upload");
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// Creates a second account and returns its session cookie. The round-trip test
    /// needs a genuinely separate tenant, not a second view of the same rows.
    async fn create_second_user(&self, username: &str) -> String {
        let (status, _) = self
            .send(
                "POST",
                "/admin/users",
                Some(json!({
                    "username": username,
                    "displayName": username,
                    "password": "ein-anderes-langes-passwort"
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "zweiter Benutzer");

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header(header::ORIGIN, ORIGIN)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"username": username, "password": "ein-anderes-langes-passwort"})
                    .to_string(),
            ))
            .unwrap();
        let response = self.router.clone().oneshot(req).await.expect("login");
        assert_eq!(response.status(), StatusCode::OK);
        response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(str::to_string)
            .expect("session cookie")
    }

    async fn send(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(format!("/api/v1{path}"))
            .header(header::ORIGIN, ORIGIN);
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, c);
        }
        let req = match body {
            Some(v) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(v.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let response = self.router.clone().oneshot(req).await.expect("request");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }

    async fn setup_admin(&mut self) {
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
        let response = self.router.clone().oneshot(req).await.expect("setup");
        assert_eq!(response.status(), StatusCode::CREATED);
        self.cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(str::to_string);
        assert!(self.cookie.is_some(), "setup must return a session cookie");
    }

    /// Uploads a workbook as multipart and returns the import id.
    async fn upload_workbook(&self, path: &str) -> String {
        let bytes = std::fs::read(path).expect("workbook");
        let boundary = "----finanzen-test-boundary";
        let name = std::path::Path::new(path)
            .file_name()
            .unwrap()
            .to_string_lossy();
        let mut body = Vec::new();
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
                 filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(&bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/imports")
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, self.cookie.clone().expect("session"))
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        let response = self.router.clone().oneshot(req).await.expect("upload");
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        assert!(status.is_success(), "upload failed: {status} {value}");
        value["id"].as_str().expect("import id").to_string()
    }

    async fn category_id(&self, name: &str) -> Uuid {
        let (_, cats) = self.send("GET", "/categories", None).await;
        cats.as_array()
            .expect("categories")
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("category {name} missing"))["id"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .expect("uuid")
    }
}

macro_rules! app {
    () => {
        match TestApp::new().await {
            Some(a) => a,
            None => {
                eprintln!("SKIP: TEST_DATABASE_URL not set");
                return;
            }
        }
    };
}

#[tokio::test]
async fn health_is_public_but_data_is_not() {
    let app = app!();
    let (status, body) = app.send("GET", "/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");

    for path in [
        "/bookings",
        "/categories",
        "/dashboard?year=2026",
        "/auth/me",
    ] {
        let (status, _) = app.send("GET", path, None).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{path} must require a session"
        );
    }
}

#[tokio::test]
async fn setup_creates_an_admin_seeded_with_the_taxonomy_and_cannot_run_twice() {
    let mut app = app!();
    let (_, status) = app.send("GET", "/auth/setup-status", None).await;
    assert_eq!(status["setupRequired"], true);
    assert_eq!(
        status["registrationOpen"], false,
        "Registrierung ist standardmäßig zu"
    );

    app.setup_admin().await;

    let (_, me) = app.send("GET", "/auth/me", None).await;
    assert_eq!(me["username"], "fabi");
    assert_eq!(me["isAdmin"], true);

    let (_, cats) = app.send("GET", "/categories", None).await;
    assert_eq!(cats.as_array().unwrap().len(), 32, "32 Kategorien");
    let (_, types) = app.send("GET", "/category-types", None).await;
    assert_eq!(types.as_array().unwrap().len(), 5, "5 Typen");

    let (status, _) = app
        .send(
            "POST",
            "/auth/setup",
            Some(json!({"username":"x","displayName":"X","password":"another-long-pass"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_mutating_request_from_a_foreign_origin_is_refused() {
    let mut app = app!();
    app.setup_admin().await;

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/bookings")
        .header(header::ORIGIN, "https://evil.example")
        .header(header::COOKIE, app.cookie.clone().unwrap())
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"year":2026,"month":1,"kind":"expense","amountCents":1250,"comment":"tanken"})
                .to_string(),
        ))
        .unwrap();
    let response = app.router.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// The flatmate case, end to end on the wire: a category's figure is expenses minus
/// income of the same category, and both gross legs stay visible.
#[tokio::test]
async fn netting_is_visible_on_the_wire() {
    let mut app = app!();
    app.setup_admin().await;
    let miete = app.category_id("Miete").await;

    app.send(
        "POST",
        "/years",
        Some(json!({"year":2026,"openingBalanceCents":4_000_000})),
    )
    .await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment":"Miete","categoryId":miete})),
    )
    .await;

    for (kind, amount) in [("expense", 110_000), ("income", 55_000)] {
        let (status, _) = app
            .send(
                "POST",
                "/bookings",
                Some(json!({"year":2026,"month":1,"kind":kind,
                            "amountCents":amount,"comment":"Miete"})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let (_, dash) = app.send("GET", "/dashboard?year=2026", None).await;
    assert_eq!(dash["incomeCents"], 55_000);
    assert_eq!(dash["expenseCents"], 110_000);
    assert_eq!(dash["balanceCents"], -55_000);
    assert_eq!(dash["openingBalanceCents"], 4_000_000);
    assert_eq!(dash["closingBalanceCents"], 4_000_000 - 55_000);

    let (_, analysis) = app
        .send("GET", "/analysis/categories?year=2026", None)
        .await;
    let row = analysis["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["categoryName"] == "Miete")
        .expect("Miete row");
    assert_eq!(row["netCents"], 55_000, "netto = 1100 aus - 550 ein");
    assert_eq!(
        row["expenseCents"], 110_000,
        "Bruttoschenkel bleiben sichtbar"
    );
    assert_eq!(row["incomeCents"], 55_000);
}

/// A transfer moves the balance but must not appear in any consumption figure, and
/// must not be counted as "uncategorised" — it legitimately has no category.
#[tokio::test]
async fn transfers_are_excluded_from_consumption_and_from_the_uncategorised_badge() {
    let mut app = app!();
    app.setup_admin().await;

    app.send(
        "POST",
        "/bookings",
        Some(json!({"year":2026,"month":1,"kind":"transfer",
                    "amountCents":50_000,"comment":"abgehoben"})),
    )
    .await;

    let (_, dash) = app.send("GET", "/dashboard?year=2026", None).await;
    assert_eq!(
        dash["balanceCents"], 0,
        "eine Umbuchung verschiebt den Saldo nicht"
    );
    assert_eq!(
        dash["uncategorizedCount"], 0,
        "eine Umbuchung ohne Kategorie ist normal, kein offener Posten"
    );

    let (_, months) = app.send("GET", "/overview/months?year=2026", None).await;
    let jan = &months["months"][0];
    assert_eq!(jan["fixedCostsNetCents"], 0);
    assert_eq!(jan["variableCostsNetCents"], 0);
    assert_eq!(jan["uncategorizedCount"], 0);
}

/// Resolution precedence, and the retroactivity that makes the rule table worth
/// maintaining at all.
#[tokio::test]
async fn category_resolution_precedence_and_retroactivity() {
    let mut app = app!();
    app.setup_admin().await;
    let lebensmittel = app.category_id("Lebensmittel").await;
    let miete = app.category_id("Miete").await;

    // No rule yet: the booking must be visibly flagged, never bucketed.
    let (_, booking) = app
        .send(
            "POST",
            "/bookings",
            Some(json!({"year":2026,"month":1,"kind":"expense",
                        "amountCents":4_235,"comment":"Kaufland"})),
        )
        .await;
    assert_eq!(booking["categoryName"], Value::Null);
    assert_eq!(booking["categorySource"], "unresolved");
    let id = booking["id"].as_str().unwrap().to_string();

    let (_, dash) = app.send("GET", "/dashboard?year=2026", None).await;
    assert_eq!(
        dash["uncategorizedCount"], 1,
        "muss sichtbar gezählt werden"
    );

    // Adding a rule fixes history, not just future entries.
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment":"Kaufland","categoryId":lebensmittel})),
    )
    .await;
    let (_, after) = app.send("GET", &format!("/bookings/{id}"), None).await;
    assert_eq!(after["categoryName"], "Lebensmittel");
    assert_eq!(after["categorySource"], "rule");

    // A manual override wins, and survives a re-apply of every rule.
    app.send(
        "PUT",
        &format!("/bookings/{id}"),
        Some(
            json!({"year":2026,"month":1,"kind":"expense","amountCents":4_235,
                    "comment":"Kaufland","categoryId":miete}),
        ),
    )
    .await;
    let (_, applied) = app.send("POST", "/rules/apply", Some(json!({}))).await;
    assert_eq!(applied["recategorized"], 0);
    let (_, still) = app.send("GET", &format!("/bookings/{id}"), None).await;
    assert_eq!(
        still["categoryName"], "Miete",
        "manuelle Zuordnung ist unantastbar"
    );
    assert_eq!(still["categorySource"], "manual");

    // Clearing the override hands the booking back to the rule table.
    app.send(
        "PUT",
        &format!("/bookings/{id}"),
        Some(
            json!({"year":2026,"month":1,"kind":"expense","amountCents":4_235,
                    "comment":"Kaufland","clearCategoryOverride":true}),
        ),
    )
    .await;
    let (_, cleared) = app.send("GET", &format!("/bookings/{id}"), None).await;
    assert_eq!(cleared["categoryName"], "Lebensmittel");
    assert_eq!(cleared["categorySource"], "rule");

    // Deleting the rule returns the booking to unresolved rather than leaving a
    // stale category behind.
    let (_, rules) = app.send("GET", "/rules", None).await;
    let rule_id = rules[0]["id"].as_str().unwrap().to_string();
    let (deleted, _) = app.send("DELETE", &format!("/rules/{rule_id}"), None).await;
    assert_eq!(
        deleted,
        StatusCode::NO_CONTENT,
        "eine Regel muss loeschbar sein, auch wenn Buchungen sie verwenden"
    );
    let (_, orphaned) = app.send("GET", &format!("/bookings/{id}"), None).await;
    assert_eq!(orphaned["categorySource"], "unresolved");
}

#[tokio::test]
async fn a_duplicate_rule_key_is_a_conflict_regardless_of_case() {
    let mut app = app!();
    app.setup_admin().await;
    let abos = app.category_id("Abos & Streaming").await;

    let (first, _) = app
        .send(
            "POST",
            "/rules",
            Some(json!({"comment":"Spotify","categoryId":abos})),
        )
        .await;
    assert_eq!(first, StatusCode::CREATED);

    let (second, body) = app
        .send(
            "POST",
            "/rules",
            Some(json!({"comment":"  spotify  ","categoryId":abos})),
        )
        .await;
    assert_eq!(second, StatusCode::CONFLICT);
    assert_eq!(body["code"], "conflict");
}

#[tokio::test]
async fn invalid_bookings_are_rejected_with_a_useful_status() {
    let mut app = app!();
    app.setup_admin().await;

    let cases = [
        json!({"year":2026,"month":1,"kind":"expense","amountCents":0,"comment":"x"}),
        json!({"year":2026,"month":1,"kind":"expense","amountCents":-5,"comment":"x"}),
        json!({"year":2026,"month":13,"kind":"expense","amountCents":100,"comment":"x"}),
        json!({"year":2026,"month":1,"kind":"expense","amountCents":100,"comment":"   "}),
    ];
    for body in cases {
        let (status, response) = app.send("POST", "/bookings", Some(body.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(response["code"], "validation_error");
    }
}

#[tokio::test]
async fn filters_narrow_and_report_their_own_sums() {
    let mut app = app!();
    app.setup_admin().await;
    let lebensmittel = app.category_id("Lebensmittel").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment":"Kaufland","categoryId":lebensmittel})),
    )
    .await;

    for (month, amount, comment) in [
        (1, 1_000, "Kaufland"),
        (2, 2_000, "Kaufland"),
        (2, 3_000, "unbekannt"),
    ] {
        app.send(
            "POST",
            "/bookings",
            Some(json!({"year":2026,"month":month,"kind":"expense",
                        "amountCents":amount,"comment":comment})),
        )
        .await;
    }

    let (_, all) = app.send("GET", "/bookings?year=2026", None).await;
    assert_eq!(all["total"], 3);
    assert_eq!(all["sumExpenseCents"], 6_000);
    assert_eq!(all["uncategorizedCount"], 1);

    let (_, feb) = app.send("GET", "/bookings?year=2026&month=2", None).await;
    assert_eq!(feb["total"], 2);
    assert_eq!(feb["sumExpenseCents"], 5_000);

    let (_, search) = app
        .send("GET", "/bookings?year=2026&search=kaufland", None)
        .await;
    assert_eq!(search["total"], 2, "Suche ist case-insensitiv");

    let (_, uncat) = app
        .send("GET", "/bookings?year=2026&uncategorized=true", None)
        .await;
    assert_eq!(uncat["total"], 1);
    assert_eq!(uncat["items"][0]["comment"], "unbekannt");
}

// ------------------------------------------------------------------ importer

/// Imports the real workbooks through the HTTP API and asserts the acceptance
/// numbers come out of the database, not just out of the pure engine.
///
/// The figures the real workbooks must produce, read from the frozen fixture the
/// golden suite uses. They are a real household's totals, so they live in
/// `tests/fixtures/expected.json`, which `.gitignore` covers — see `golden.rs`.
fn expected(path: &str) -> i64 {
    static EXPECTED: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    let doc = EXPECTED.get_or_init(|| {
        let raw = std::fs::read_to_string("tests/fixtures/expected.json")
            .expect("tests/fixtures/expected.json — see golden.rs");
        serde_json::from_str(&raw).expect("expected fixture")
    });
    let mut node = doc;
    for key in path.split('.') {
        node = node
            .get(key)
            .unwrap_or_else(|| panic!("expected.json has no `{path}` (missing `{key}`)"));
    }
    node.as_i64()
        .unwrap_or_else(|| panic!("`{path}` is not an integer"))
}

/// The expected net of one 2026 category, by name.
fn expected_category(name: &str) -> i64 {
    static EXPECTED: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    let doc = EXPECTED.get_or_init(|| {
        let raw = std::fs::read_to_string("tests/fixtures/expected.json")
            .expect("tests/fixtures/expected.json — see golden.rs");
        serde_json::from_str(&raw).expect("expected fixture")
    });
    doc["y2026"]["categoryNets"]
        .as_array()
        .expect("categoryNets")
        .iter()
        .find(|r| r[0].as_str() == Some(name))
        .unwrap_or_else(|| panic!("expected.json has no category `{name}`"))[1]
        .as_i64()
        .expect("integer")
}

/// Skips when the workbooks are absent, because they carry personal financial data
/// and are deliberately not in the repository. `expected.json` is absent for the
/// same reason, so it gates the test too.
#[tokio::test]
async fn importing_the_real_workbooks_reproduces_the_acceptance_numbers() {
    let (Ok(_), Ok(_), Ok(_)) = (
        std::fs::metadata("../konten_2026_auswertung.xlsx"),
        std::fs::metadata("../konten.ods"),
        std::fs::metadata("tests/fixtures/expected.json"),
    ) else {
        eprintln!("SKIP: source workbooks or expected.json not present");
        return;
    };
    let mut app = app!();
    app.setup_admin().await;

    // Load the workbook's own rule table, with the one correction: the pattern
    // below is filed under Haustier there and belongs to Sport.
    let taxonomy = {
        let bytes = std::fs::read("../konten_2026_auswertung.xlsx").unwrap();
        finanzen::sheets::read_xlsx_taxonomy(&bytes).unwrap()
    };
    let (_, cats) = app.send("GET", "/categories", None).await;
    let ids: std::collections::BTreeMap<String, String> = cats
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["name"].as_str().unwrap().to_string(),
                c["id"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    for (pattern, category) in &taxonomy.rules {
        let category = match pattern.as_str() {
            "mapet" | "mapet guthaben" => "Sport",
            _ => category.as_str(),
        };
        if let Some(id) = ids.get(category) {
            app.send(
                "POST",
                "/rules",
                Some(json!({"comment": pattern, "categoryId": id})),
            )
            .await;
        }
    }

    let import_id = app.upload_workbook("../konten_2026_auswertung.xlsx").await;
    let (status, committed) = app
        .send(
            "POST",
            &format!("/imports/{import_id}/commit"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(committed["inserted"], 474, "474 Buchungen");
    assert_eq!(
        committed["uncategorizedRemaining"], 0,
        "nichts ohne Kategorie"
    );

    app.send(
        "PUT",
        "/years/2026",
        Some(json!({"year":2026,"openingBalanceCents":expected("y2026.openingCents")})),
    )
    .await;

    let (_, d) = app.send("GET", "/dashboard?year=2026", None).await;
    assert_eq!(d["incomeCents"], expected("y2026.incomeCents"), "Einnahmen");
    assert_eq!(
        d["expenseCents"],
        expected("y2026.expenseCents"),
        "Ausgaben"
    );
    assert_eq!(d["balanceCents"], expected("y2026.saldoCents"), "Bilanz");
    assert_eq!(
        d["closingBalanceCents"],
        expected("y2026.closingCents"),
        "Bilanz gesamt"
    );
    assert_eq!(d["taxRelevantCount"], expected("y2026.taxCount"));
    assert_eq!(d["uncategorizedCount"], 0);
    assert_eq!(d["monthsWithData"], expected("y2026.monthsWithData"));
    assert_eq!(
        d["averageExpensePerMonthCents"],
        expected("y2026.averageExpensePerMonth")
    );
    assert_eq!(
        d["fixedCostsPerMonthCents"],
        expected("y2026.fixedCostsPerMonth")
    );

    // The rows carrying a "Kategorie manuell" override must land in Dienstreisen,
    // not in the category their comment's rule would choose. Getting this wrong
    // moves their net into Reisen & Urlaub and is invisible in the totals.
    let (_, analysis) = app
        .send("GET", "/analysis/categories?year=2026", None)
        .await;
    let net: std::collections::BTreeMap<&str, i64> = analysis["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["categoryName"].as_str().unwrap(),
                r["netCents"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        net["Dienstreisen"],
        expected_category("Dienstreisen"),
        "manuelle Zuordnung wurde angewandt"
    );
    assert_eq!(net["Reisen & Urlaub"], expected_category("Reisen & Urlaub"));
    assert_eq!(net["Miete"], expected_category("Miete"));
    assert_eq!(net["Sport"], expected_category("Sport"));
    assert_eq!(net["Sparen & Anlage"], expected_category("Sparen & Anlage"));

    // June's variable costs are negative because of a reimbursement — the single
    // best regression test for netting surviving the whole stack.
    let (_, months) = app.send("GET", "/overview/months?year=2026", None).await;
    assert_eq!(
        months["months"][5]["variableCostsNetCents"],
        expected("y2026.juneVariableNetCents")
    );
    assert_eq!(
        months["months"][8]["cumulativeCents"],
        expected("y2026.saldoCents")
    );

    // The legacy sheet: month-only rows recovered from saldo markers, the three
    // blocks that disagree with their own marker recorded rather than adjusted.
    let legacy_id = app.upload_workbook("../konten.ods").await;
    let (_, preview) = app
        .send("GET", &format!("/imports/{legacy_id}"), None)
        .await;
    assert_eq!(
        preview["counts"]["dataRows"],
        expected("legacy.bookingCount")
    );
    assert_eq!(preview["counts"]["transfer"], 4, "nur die Kontoumbuchungen");
    assert_eq!(
        preview["markerTotalCents"],
        expected("legacy.markerTotalCents"),
        "Marker = Vortrag"
    );
    assert_eq!(preview["rowTotalCents"], expected("legacy.rowTotalCents"));
    assert_eq!(preview["blocks"].as_array().unwrap().len(), 31);
    let mismatched: Vec<_> = preview["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|b| !b["deltaCents"].is_null())
        .map(|b| {
            (
                b["year"].as_i64(),
                b["month"].as_i64(),
                b["deltaCents"].as_i64(),
            )
        })
        .collect();
    assert_eq!(
        mismatched,
        vec![
            (Some(2023), Some(9), Some(-16_000)),
            (Some(2023), Some(10), Some(-16_000)),
            (Some(2024), Some(6), Some(350)),
        ]
    );
    assert!(
        !preview["warnings"].as_array().unwrap().is_empty(),
        "die Abweichung muss sichtbar gemeldet werden"
    );

    // The review queue is keyed by distinct comment, not by row: ~238 decisions
    // instead of ~362, which is what makes it finishable.
    let (_, review) = app
        .send(
            "GET",
            &format!("/imports/{legacy_id}/review?limit=1000"),
            None,
        )
        .await;
    let items = review.as_array().unwrap();
    assert!(
        (230..=245).contains(&items.len()),
        "Prüfliste hat {} Einträge",
        items.len()
    );
    // Sorted by frequency, so the work that clears the most rows comes first.
    let counts: Vec<i64> = items
        .iter()
        .map(|i| i["rowCount"].as_i64().unwrap())
        .collect();
    assert!(
        counts.windows(2).all(|w| w[0] >= w[1]),
        "nach Häufigkeit sortiert"
    );
}

/// Re-uploading the same file must not create a second job, and committing twice
/// must not double-post.
#[tokio::test]
async fn import_is_idempotent() {
    let Ok(_) = std::fs::metadata("../konten_2026_auswertung.xlsx") else {
        eprintln!("SKIP: source workbook not present");
        return;
    };
    let mut app = app!();
    app.setup_admin().await;

    let first = app.upload_workbook("../konten_2026_auswertung.xlsx").await;
    let second = app.upload_workbook("../konten_2026_auswertung.xlsx").await;
    assert_eq!(
        first, second,
        "eine identische Datei ergibt denselben Import"
    );

    app.send("POST", &format!("/imports/{first}/commit"), Some(json!({})))
        .await;
    let (status, _) = app
        .send("POST", &format!("/imports/{first}/commit"), Some(json!({})))
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "ein zweiter Commit muss abgelehnt werden"
    );

    let (_, bookings) = app
        .send("GET", "/bookings?year=2026&pageSize=1", None)
        .await;
    assert_eq!(bookings["total"], 474, "keine Doppelbuchungen");
}

/// An unknown API path must answer a JSON 404. Without an explicit fallback on the
/// nested router it falls through to the SPA and returns 200 text/html, which turns
/// a client bug into a silent rendering oddity.
#[tokio::test]
async fn an_unknown_api_path_is_a_json_404() {
    let app = app!();
    let (status, body) = app.send("GET", "/does-not-exist", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");

    let (status, body) = app.send("POST", "/bookings/not-a-uuid/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
}

// -------------------------------------------------------------------- funds

/// A fund measured against what the ledger actually did.
///
/// The point of the feature is that an annual bill is not a surprise, so the
/// interesting assertion is not the accrual — that is unit-tested in `funds.rs` —
/// but that Soll and Ist come from two independent places and are put side by side:
/// the accrual from the stated annual amount, the spending from the bookings in the
/// fund's category.
#[tokio::test]
async fn a_fund_compares_its_accrual_with_what_the_category_actually_cost() {
    let mut app = app!();
    app.setup_admin().await;
    let versicherungen = app.category_id("Versicherungen").await;

    // Kfz-Versicherung: the real shape — one booking, in Juli, for the whole year.
    app.send(
        "POST",
        "/bookings",
        Some(
            json!({"year":2026,"month":7,"kind":"expense","amountCents":30_700,
                    "comment":"Kfz Versicherung","categoryId":versicherungen}),
        ),
    )
    .await;

    let (status, fund) = app
        .send(
            "POST",
            "/funds",
            Some(
                json!({"name":"Kfz-Versicherung","categoryId":versicherungen,
                        "annualCents":30_700,"dueMonth":7}),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{fund}");
    assert_eq!(fund["dueMonthName"], "Juli");

    // Halfway through the year: half accrued, the bill counted for the whole year.
    let (status, june) = app
        .send("GET", "/funds/status?year=2026&month=6", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{june}");
    let row = &june["funds"][0];
    assert_eq!(row["monthlyAccrualCents"], 2_558);
    assert_eq!(row["accruedByMonthCents"], 15_350);
    assert_eq!(row["spentCents"], 30_700);
    // The bill is bigger than what has been put aside by June — which is the
    // warning the feature exists to give.
    assert_eq!(row["overUnderCents"], 15_350 - 30_700);
    assert_eq!(row["duePassed"], false);

    // By December the accrual has caught up exactly, to the cent.
    let (_, december) = app
        .send("GET", "/funds/status?year=2026&month=12", None)
        .await;
    let row = &december["funds"][0];
    assert_eq!(row["accruedByMonthCents"], 30_700);
    assert_eq!(row["overUnderCents"], 0);
    assert_eq!(row["duePassed"], true);
    // Nothing is left to come: the bill has been paid.
    assert_eq!(december["owedToTheFutureCents"], 0);
}

/// A fund books nothing. It is an expectation; creating one must not move a single
/// figure in the ledger, or it would double-count the spending it anticipates.
#[tokio::test]
async fn creating_a_fund_changes_no_figure_in_the_ledger() {
    let mut app = app!();
    app.setup_admin().await;
    let nebenkosten = app.category_id("Nebenkosten").await;
    app.send(
        "POST",
        "/bookings",
        Some(
            json!({"year":2026,"month":8,"kind":"expense","amountCents":144_000,
                    "comment":"Nebenkosten 2025","categoryId":nebenkosten}),
        ),
    )
    .await;

    let (_, before) = app.send("GET", "/dashboard?year=2026", None).await;
    app.send(
        "POST",
        "/funds",
        Some(json!({"name":"Nebenkosten","categoryId":nebenkosten,
                    "annualCents":144_000,"dueMonth":8})),
    )
    .await;
    let (_, after) = app.send("GET", "/dashboard?year=2026", None).await;

    assert_eq!(before["expenseCents"], after["expenseCents"]);
    assert_eq!(before["balanceCents"], after["balanceCents"]);
    assert_eq!(before["bookingCount"], after["bookingCount"]);
}

/// Suggested, never created — and only where the history actually argues for it.
#[tokio::test]
async fn suggestions_are_the_lumps_and_not_the_habits() {
    let mut app = app!();
    app.setup_admin().await;
    let versicherungen = app.category_id("Versicherungen").await;
    let lebensmittel = app.category_id("Lebensmittel").await;

    // A lump: one month, one big amount.
    app.send(
        "POST",
        "/bookings",
        Some(
            json!({"year":2026,"month":7,"kind":"expense","amountCents":30_700,
                    "comment":"Kfz Versicherung","categoryId":versicherungen}),
        ),
    )
    .await;
    // A habit: comparable money, spread across the year. Accruing for that would be
    // bookkeeping for its own sake.
    for month in 1..=6 {
        app.send(
            "POST",
            "/bookings",
            Some(
                json!({"year":2026,"month":month,"kind":"expense","amountCents":5_000,
                        "comment":format!("Supermarkt {month}"),"categoryId":lebensmittel}),
            ),
        )
        .await;
    }

    let (status, suggestions) = app.send("GET", "/funds/suggestions?year=2026", None).await;
    assert_eq!(status, StatusCode::OK, "{suggestions}");
    let names: Vec<&str> = suggestions
        .as_array()
        .expect("suggestions")
        .iter()
        .map(|s| s["categoryName"].as_str().unwrap_or_default())
        .collect();
    assert!(names.contains(&"Versicherungen"), "{names:?}");
    assert!(!names.contains(&"Lebensmittel"), "{names:?}");

    let kfz = &suggestions[0];
    assert_eq!(kfz["annualCents"], 30_700);
    assert_eq!(kfz["dueMonth"], 7);
    assert_eq!(kfz["monthsWithSpending"], 1);

    // ...and once a fund exists for that category, suggesting it again is noise.
    app.send(
        "POST",
        "/funds",
        Some(json!({"name":"Kfz","categoryId":versicherungen,
                    "annualCents":30_700,"dueMonth":7})),
    )
    .await;
    let (_, again) = app.send("GET", "/funds/suggestions?year=2026", None).await;
    assert_eq!(again.as_array().expect("suggestions").len(), 0);
}

// ----------------------------------------------------------------- recurring

/// Creates a template and returns its id.
async fn make_template(app: &TestApp, body: Value) -> String {
    let (status, created) = app.send("POST", "/recurring", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "Vorlage: {created}");
    created["id"].as_str().expect("template id").to_string()
}

/// A template with no override still books into a category — the rule table's —
/// and the listing has to say so rather than showing an empty cell.
///
/// Leaving the override off is the RECOMMENDED way to write a template, because a
/// rule change then still reaches future bookings. A list that renders that as
/// "uncategorised" argues for the opposite.
#[tokio::test]
async fn a_template_without_an_override_shows_the_category_its_rule_gives_it() {
    let mut app = app!();
    app.setup_admin().await;
    let sport = app.category_id("Sport").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment": "Mafit", "categoryId": sport})),
    )
    .await;

    make_template(
        &app,
        json!({"name":"Sport","comment":"Mafit","kind":"expense","amountCents":2900,
               "activeFrom":{"year":2026,"month":9}}),
    )
    .await;

    let (status, list) = app.send("GET", "/recurring", None).await;
    assert_eq!(status, StatusCode::OK);
    let row = &list[0];
    assert!(row["categoryId"].is_null(), "no override was asked for");
    assert_eq!(row["categoryName"], "Sport");
    // ...and it is marked as coming from the rule, because a rule change moves it.
    assert_eq!(row["categoryFromRule"], true);
}

/// Materialising twice must create nothing the second time.
///
/// This is the property that decides whether the "alle buchen" button is safe to
/// press. It is guaranteed by the partial unique index, not by the handler — which is
/// why the assertion is made through the API rather than by unit-testing a guard.
#[tokio::test]
async fn materialising_a_month_twice_creates_nothing_the_second_time() {
    let mut app = app!();
    app.setup_admin().await;
    let miete = app.category_id("Miete").await;

    make_template(
        &app,
        json!({
            "name": "Miete", "comment": "Miete", "kind": "expense",
            "amountCents": 110000, "categoryId": miete, "dayOfMonth": 1,
            "activeFrom": {"year": 2026, "month": 1}
        }),
    )
    .await;
    make_template(
        &app,
        json!({
            "name": "Spotify", "comment": "Spotify", "kind": "expense",
            "amountCents": 300, "activeFrom": {"year": 2026, "month": 1}
        }),
    )
    .await;

    let (status, first) = app
        .send(
            "POST",
            "/recurring/materialize",
            Some(json!({"year": 2026, "month": 3})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["created"], 2);
    assert_eq!(first["skipped"], 0);

    let (_, second) = app
        .send(
            "POST",
            "/recurring/materialize",
            Some(json!({"year": 2026, "month": 3})),
        )
        .await;
    assert_eq!(second["created"], 0, "ein zweiter Lauf legt nichts an");
    assert_eq!(second["skipped"], 2);
    for item in second["items"].as_array().unwrap() {
        assert_eq!(item["skippedReason"], "alreadyBooked");
    }

    let (_, bookings) = app.send("GET", "/bookings?year=2026", None).await;
    assert_eq!(bookings["total"], 2, "keine Doppelbuchungen");

    // A different month is a different period, so it is not a duplicate.
    let (_, april) = app
        .send(
            "POST",
            "/recurring/materialize",
            Some(json!({"year": 2026, "month": 4})),
        )
        .await;
    assert_eq!(april["created"], 2);
}

/// A dry run reports exactly what a real run would do, and writes nothing.
#[tokio::test]
async fn a_dry_run_reports_the_same_counts_and_writes_nothing() {
    let mut app = app!();
    app.setup_admin().await;
    make_template(
        &app,
        json!({
            "name": "Internet", "comment": "Internet", "kind": "expense",
            "amountCents": 4500, "activeFrom": {"year": 2026, "month": 1}
        }),
    )
    .await;

    let (_, dry) = app
        .send(
            "POST",
            "/recurring/materialize",
            Some(json!({"year": 2026, "month": 5, "dryRun": true})),
        )
        .await;
    assert_eq!(dry["created"], 1);
    assert_eq!(dry["dryRun"], true);
    assert!(dry["items"][0]["bookingId"].is_null());

    let (_, bookings) = app
        .send("GET", "/bookings?year=2026&status=all", None)
        .await;
    assert_eq!(bookings["total"], 0, "ein Probelauf bucht nichts");

    let (_, wet) = app
        .send(
            "POST",
            "/recurring/materialize",
            Some(json!({"year": 2026, "month": 5})),
        )
        .await;
    assert_eq!(wet["created"], dry["created"]);
}

/// A quarterly template is due in four months of the year and in no others, and an
/// annual one in exactly one. Getting this wrong is invisible until a quarter is
/// double-booked or silently skipped.
#[tokio::test]
async fn a_quarterly_template_is_due_only_in_the_right_months() {
    let mut app = app!();
    app.setup_admin().await;

    make_template(
        &app,
        json!({
            "name": "Versicherung", "comment": "Versicherung", "kind": "expense",
            "amountCents": 32000, "intervalMonths": 3,
            "anchor": {"year": 2026, "month": 2},
            "activeFrom": {"year": 2026, "month": 1}
        }),
    )
    .await;
    make_template(
        &app,
        json!({
            "name": "Domain", "comment": "Domain", "kind": "expense",
            "amountCents": 1500, "intervalMonths": 12,
            "anchor": {"year": 2026, "month": 9},
            "activeFrom": {"year": 2026, "month": 1}
        }),
    )
    .await;

    let mut due_quarterly = Vec::new();
    let mut due_annual = Vec::new();
    for month in 1..=12u8 {
        let (status, list) = app
            .send("GET", &format!("/recurring?year=2026&month={month}"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{list}");
        for t in list.as_array().unwrap() {
            if t["dueInPeriod"] == json!(true) {
                if t["name"] == "Versicherung" {
                    due_quarterly.push(month);
                } else {
                    due_annual.push(month);
                }
            }
        }
    }
    assert_eq!(due_quarterly, vec![2, 5, 8, 11]);
    assert_eq!(due_annual, vec![9]);

    // And the materialiser agrees with the listing.
    let (_, march) = app
        .send(
            "POST",
            "/recurring/materialize",
            Some(json!({"year": 2026, "month": 3})),
        )
        .await;
    assert_eq!(march["created"], 0, "im März ist nichts fällig");
    let (_, may) = app
        .send(
            "POST",
            "/recurring/materialize",
            Some(json!({"year": 2026, "month": 5})),
        )
        .await;
    assert_eq!(may["created"], 1);
    assert_eq!(may["items"][0]["templateName"], "Versicherung");
}

/// The whole reason `amount_is_estimate` exists: a membership billed a different
/// amount in different months. Materialising it as a confirmed figure would put a
/// wrong number into the year's total and nothing would ever flag it.
#[tokio::test]
async fn an_estimate_materialises_as_a_draft_that_moves_no_total_until_confirmed() {
    let mut app = app!();
    app.setup_admin().await;
    let sport = app.category_id("Sport").await;

    app.send(
        "POST",
        "/bookings",
        Some(json!({
            "year": 2026, "month": 2, "kind": "expense",
            "amountCents": 50000, "comment": "Anker"
        })),
    )
    .await;
    let (_, before) = app.send("GET", "/dashboard?year=2026", None).await;
    assert_eq!(before["expenseCents"], 50000);

    make_template(
        &app,
        json!({
            "name": "Mafit", "comment": "Mafit", "kind": "expense",
            "amountCents": 2900, "amountIsEstimate": true, "categoryId": sport,
            "activeFrom": {"year": 2026, "month": 1}
        }),
    )
    .await;

    let (_, run) = app
        .send(
            "POST",
            "/recurring/materialize",
            Some(json!({"year": 2026, "month": 2})),
        )
        .await;
    assert_eq!(run["created"], 1);
    assert_eq!(run["drafts"], 1);
    assert_eq!(run["items"][0]["status"], "draft");
    let draft_id = run["items"][0]["bookingId"].as_str().unwrap().to_string();

    // Nothing moved. Not the totals, not the category, not the month.
    let (_, after) = app.send("GET", "/dashboard?year=2026", None).await;
    assert_eq!(after["expenseCents"], 50000, "ein Entwurf zählt nicht mit");
    assert_eq!(after["balanceCents"], before["balanceCents"]);
    let (_, cats) = app
        .send("GET", "/analysis/categories?year=2026", None)
        .await;
    assert!(
        !cats["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["categoryName"] == "Sport"),
        "Sport darf vor der Bestätigung nirgends auftauchen"
    );
    // ... and it is not in the ordinary listing either.
    let (_, listed) = app.send("GET", "/bookings?year=2026", None).await;
    assert_eq!(listed["total"], 1);
    let (_, drafts) = app
        .send("GET", "/bookings?year=2026&status=draft", None)
        .await;
    assert_eq!(drafts["total"], 1, "aber die Prüfliste findet ihn");

    // Confirming with the month's real amount is the whole workflow.
    let (status, confirmed) = app
        .send(
            "POST",
            &format!("/bookings/{draft_id}/confirm"),
            Some(json!({"amountCents": 3450})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    assert_eq!(confirmed["status"], "confirmed");
    assert_eq!(confirmed["amountCents"], 3450);

    let (_, final_state) = app.send("GET", "/dashboard?year=2026", None).await;
    assert_eq!(final_state["expenseCents"], 53450);

    // Confirming again is harmless.
    let (status, _) = app
        .send("POST", &format!("/bookings/{draft_id}/confirm"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, unchanged) = app.send("GET", "/dashboard?year=2026", None).await;
    assert_eq!(unchanged["expenseCents"], 53450);
}

/// A template with no explicit category resolves through the rule table at
/// materialisation time, so a rule fixed today reaches next month's booking.
#[tokio::test]
async fn a_template_without_a_category_still_goes_through_the_rule_table() {
    let mut app = app!();
    app.setup_admin().await;
    let strom = app.category_id("Strom").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment": "Stromabschlag", "categoryId": strom})),
    )
    .await;

    make_template(
        &app,
        json!({
            "name": "Strom", "comment": "Stromabschlag", "kind": "expense",
            "amountCents": 8200, "activeFrom": {"year": 2026, "month": 1}
        }),
    )
    .await;
    app.send(
        "POST",
        "/recurring/materialize",
        Some(json!({"year": 2026, "month": 6})),
    )
    .await;

    let (_, bookings) = app.send("GET", "/bookings?year=2026", None).await;
    let booking = &bookings["items"][0];
    assert_eq!(booking["categoryName"], "Strom");
    assert_eq!(booking["categorySource"], "rule");
    assert_eq!(booking["origin"], "recurring");
    assert_eq!(booking["bookedOn"], "2026-06-01");
}

/// A template whose day does not exist in a given month must clamp, not explode: a
/// single badly-configured template must not fail the whole "alle buchen" run.
#[tokio::test]
async fn a_template_day_past_the_end_of_the_month_is_clamped() {
    let mut app = app!();
    app.setup_admin().await;
    make_template(
        &app,
        json!({
            "name": "Rate", "comment": "Rate", "kind": "expense",
            "amountCents": 5000, "dayOfMonth": 31,
            "activeFrom": {"year": 2026, "month": 1}
        }),
    )
    .await;
    let (status, run) = app
        .send(
            "POST",
            "/recurring/materialize",
            Some(json!({"year": 2026, "month": 2})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{run}");
    let (_, bookings) = app.send("GET", "/bookings?year=2026", None).await;
    assert_eq!(bookings["items"][0]["bookedOn"], "2026-02-28");
}

/// Deleting a template must not delete the money it already booked.
#[tokio::test]
async fn deleting_a_template_leaves_its_bookings_alone() {
    let mut app = app!();
    app.setup_admin().await;
    let id = make_template(
        &app,
        json!({
            "name": "Miete", "comment": "Miete", "kind": "expense",
            "amountCents": 110000, "activeFrom": {"year": 2026, "month": 1}
        }),
    )
    .await;
    app.send(
        "POST",
        "/recurring/materialize",
        Some(json!({"year": 2026, "month": 1})),
    )
    .await;

    let (status, _) = app.send("DELETE", &format!("/recurring/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, dashboard) = app.send("GET", "/dashboard?year=2026", None).await;
    assert_eq!(dashboard["expenseCents"], 110000, "die Buchung bleibt");
}

// ------------------------------------------------------------------ receipts

const ONE_PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

async fn make_booking(app: &TestApp, comment: &str, amount: i64, tax: bool) -> String {
    let (status, booking) = app
        .send(
            "POST",
            "/bookings",
            Some(json!({
                "year": 2026, "month": 4, "kind": "expense",
                "amountCents": amount, "comment": comment, "taxRelevant": tax
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{booking}");
    booking["id"].as_str().expect("booking id").to_string()
}

/// The filename a phone or a browser sends is attacker-controlled. It is stored as
/// metadata and is never a path component — so a name full of `../` produces a file
/// in exactly the same place an ordinary name does.
#[tokio::test]
async fn a_receipt_filename_containing_dot_dot_cannot_escape_its_directory() {
    let mut app = app!();
    app.setup_admin().await;
    let booking = make_booking(&app, "Kaufland", 1907, true).await;

    let (status, receipt) = app
        .upload_bytes(
            &format!("/bookings/{booking}/receipt"),
            "../../../../etc/passwd",
            "image/png",
            ONE_PIXEL_PNG,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{receipt}");
    assert_eq!(
        receipt["filename"], "passwd",
        "der Pfadanteil wird verworfen"
    );

    // Exactly one file, and it lives under the tenant's own uuid-named directory.
    let root = app.data_dir.join("receipts");
    let mut found: Vec<std::path::PathBuf> = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                stack.push(entry.path());
            } else {
                found.push(entry.path());
            }
        }
    }
    assert_eq!(found.len(), 1, "genau eine Datei: {found:?}");
    let path = &found[0];
    assert!(
        path.starts_with(&root),
        "die Datei liegt unter {root:?}, nicht {path:?}"
    );
    assert_eq!(
        path.extension().and_then(|e| e.to_str()),
        Some("png"),
        "die Endung kommt aus dem Content-Type, nicht aus dem Dateinamen"
    );
    assert!(
        !path.to_string_lossy().contains("passwd"),
        "der Dateiname taucht im Pfad nicht auf"
    );
    // Nothing was written next to the data directory either.
    assert!(!app.data_dir.join("etc").exists());

    // The bytes come back unchanged, under the sanitised name.
    let (status, headers, bytes) = app.get_raw(&format!("/bookings/{booking}/receipt")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, ONE_PIXEL_PNG);
    let disposition = headers
        .get(header::CONTENT_DISPOSITION)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(disposition.contains("attachment"));
    assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");

    // The tax report sees it.
    let (_, tax) = app.send("GET", "/tax?year=2026", None).await;
    assert_eq!(tax["receiptsPresent"], 1);
    assert_eq!(tax["entries"][0]["hasReceipt"], true);

    let (status, _) = app
        .send("DELETE", &format!("/bookings/{booking}/receipt"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!path.exists(), "die Datei wird mitgelöscht");
    let (status, _, _) = app.get_raw(&format!("/bookings/{booking}/receipt")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Only images and PDFs. Anything else is refused before a byte reaches the disk.
#[tokio::test]
async fn a_receipt_that_is_not_an_image_or_a_pdf_is_refused() {
    let mut app = app!();
    app.setup_admin().await;
    let booking = make_booking(&app, "Kaufland", 1907, true).await;

    for (name, content_type) in [
        ("beleg.html", "text/html"),
        ("beleg.sh", "application/x-sh"),
        ("beleg.pdf", "application/octet-stream"),
        ("beleg.png", ""),
    ] {
        let (status, body) = app
            .upload_bytes(
                &format!("/bookings/{booking}/receipt"),
                name,
                content_type,
                b"<script>alert(1)</script>",
            )
            .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{content_type} muss abgelehnt werden: {body}"
        );
    }
    assert!(
        !app.data_dir.join("receipts").exists()
            || std::fs::read_dir(app.data_dir.join("receipts"))
                .map(|mut d| d.next().is_none())
                .unwrap_or(true),
        "eine abgelehnte Datei landet nicht auf der Platte"
    );

    // A receipt for a booking that does not exist is a 404, never a 403: a 403 would
    // confirm the id exists in somebody else's account.
    let (status, _) = app
        .upload_bytes(
            &format!("/bookings/{}/receipt", Uuid::new_v4()),
            "beleg.png",
            "image/png",
            ONE_PIXEL_PNG,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// `receipts_dedupe` is UNIQUE (user_id, sha256), so the same bytes exist once per
/// user. The documented consequence: re-uploading to the same booking is a no-op, and
/// attaching the same file to a second booking is refused with the first booking
/// named — rather than silently moving the receipt off the booking it belongs to.
#[tokio::test]
async fn the_same_file_twice_is_a_no_op_once_and_a_conflict_on_another_booking() {
    let mut app = app!();
    app.setup_admin().await;
    let first = make_booking(&app, "Kaufland", 1907, true).await;
    let second = make_booking(&app, "Edeka", 2210, true).await;

    let (status, one) = app
        .upload_bytes(
            &format!("/bookings/{first}/receipt"),
            "beleg.png",
            "image/png",
            ONE_PIXEL_PNG,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, again) = app
        .upload_bytes(
            &format!("/bookings/{first}/receipt"),
            "beleg.png",
            "image/png",
            ONE_PIXEL_PNG,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "derselbe Beleg, dieselbe Buchung");
    assert_eq!(again["id"], one["id"], "kein zweiter Datensatz");

    let (status, conflict) = app
        .upload_bytes(
            &format!("/bookings/{second}/receipt"),
            "beleg.png",
            "image/png",
            ONE_PIXEL_PNG,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        conflict["message"].as_str().unwrap().contains("Kaufland"),
        "die Meldung nennt die andere Buchung: {conflict}"
    );
}

// ------------------------------------------------------------------- exports

/// German Excel needs the BOM and the semicolon, and a German filename needs
/// RFC 5987. All three are silent failures: without the BOM every umlaut is
/// mojibake, without the semicolon every amount splits in half at its decimal comma,
/// without `filename*` the file lands as `export.csv`.
#[tokio::test]
async fn the_tax_csv_is_readable_by_a_german_excel() {
    let mut app = app!();
    app.setup_admin().await;
    let booking = make_booking(&app, "Uni Gebühren", 39000, true).await;
    app.upload_bytes(
        &format!("/bookings/{booking}/receipt"),
        "beleg.pdf",
        "application/pdf",
        b"%PDF-1.4 fake",
    )
    .await;
    make_booking(&app, "Spotify", 2994, true).await;

    let (status, headers, bytes) = app.get_raw("/tax/export.csv?year=2026").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "UTF-8 BOM");
    let text = String::from_utf8(bytes[3..].to_vec()).expect("utf-8");

    assert!(text.starts_with("Nr.;Monat;"), "Semikolon-getrennt: {text}");
    assert!(text.contains("390,00"), "de-DE formatiert: {text}");
    assert!(!text.contains("39000"), "keine Cents in der CSV: {text}");
    assert!(text.contains("Uni Gebühren"), "Umlaute unverfälscht");
    assert!(text.contains(";ja"), "die Belegspalte");
    assert!(text.contains("Summe"), "eine Summenzeile");
    // 390,00 + 29,94
    assert!(text.contains("419,94"), "die Summe stimmt: {text}");

    let disposition = headers
        .get(header::CONTENT_DISPOSITION)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(disposition.starts_with("attachment;"));
    assert!(disposition.contains("filename*=UTF-8''Steuer_2026.csv"));
    assert_eq!(
        headers.get(header::CONTENT_TYPE).unwrap(),
        "text/csv; charset=utf-8"
    );
}

/// The PDF only has to be correct and printable. What is asserted is that it is a
/// real PDF, that it carries the data, and that its German filename survives — the
/// layout is a judgement call, the bytes are not.
#[tokio::test]
async fn the_tax_pdf_is_a_real_pdf_with_a_german_filename() {
    let mut app = app!();
    app.setup_admin().await;
    // More rows than fit on one page, so pagination is exercised rather than assumed.
    for i in 0..60 {
        make_booking(&app, &format!("Beleg {i}"), 1000 + i, true).await;
    }

    let (status, headers, bytes) = app.get_raw("/tax/export.pdf?year=2026").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&bytes[..5], b"%PDF-", "ein echtes PDF");
    assert!(bytes.len() > 1500, "nicht leer: {} Bytes", bytes.len());
    let tail = String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(1024)..]);
    assert!(tail.contains("%%EOF"), "vollständig geschrieben");

    assert_eq!(
        headers.get(header::CONTENT_TYPE).unwrap(),
        "application/pdf"
    );
    let disposition = headers
        .get(header::CONTENT_DISPOSITION)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        disposition.contains("filename*=UTF-8''Beleg%C3%BCbersicht_2026.pdf"),
        "{disposition}"
    );
    // The ASCII fallback must still be a usable name.
    assert!(disposition.contains("filename=\"Belegubersicht_2026.pdf\""));

    // An empty year still produces a valid document rather than a 500.
    let (status, _, bytes) = app.get_raw("/tax/export.pdf?year=1999").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&bytes[..5], b"%PDF-");
}

/// Cents on the JSON side, de-DE on the CSV side. The exception is deliberate and
/// this is the test that would catch someone "fixing" it in either direction.
#[tokio::test]
async fn the_json_export_keeps_cents_and_the_csv_export_does_not() {
    let mut app = app!();
    app.setup_admin().await;
    make_booking(&app, "Miete", 110000, false).await;

    let (_, _, bytes) = app.get_raw("/exports/bookings.json?year=2026").await;
    let doc: Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(doc["formatVersion"], 1);
    assert_eq!(doc["bookings"][0]["amountCents"], 110000);

    let (_, headers, bytes) = app.get_raw("/exports/bookings.csv?year=2026").await;
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF]);
    let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
    assert!(text.contains("1.100,00"), "{text}");
    assert!(!text.contains("110000"), "{text}");
    assert!(
        headers
            .get(header::CONTENT_DISPOSITION)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("Buchungen_2026.csv")
    );
}

/// Strips every identifier so two accounts holding the same ledger compare equal.
/// Ids are per-user by design — two users' "Miete" are different rows — so comparing
/// them would only prove that uuids differ.
fn strip_ids(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| {
                    !matches!(
                        k.as_str(),
                        "id" | "categoryId" | "bookingId" | "templateId" | "exportedAt"
                    )
                })
                .map(|(k, v)| (k.clone(), strip_ids(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(strip_ids).collect()),
        other => other.clone(),
    }
}

/// The strongest single test available: a full account is exported as JSON, restored
/// into a genuinely separate tenant, and every report is compared field by field.
///
/// It exercises the schema, the netting rule, the categorisation state machine, the
/// engine and both export paths at once — and it is the test that fails if a future
/// column is added to `bookings` and forgotten in the export.
#[tokio::test]
async fn the_export_round_trip_preserves_every_figure() {
    let mut app = app!();
    app.setup_admin().await;

    // A deliberately awkward ledger: netting on both sides of one category, a
    // transfer that must not touch consumption, a manual override that must survive
    // as an override, a tax-relevant row, a draft that must stay a draft, and a
    // recurring template.
    let miete = app.category_id("Miete").await;
    let dienstreisen = app.category_id("Dienstreisen").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment": "Miete", "categoryId": miete})),
    )
    .await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment": "to ING", "kindOverride": "transfer"})),
    )
    .await;
    app.send(
        "POST",
        "/years",
        Some(json!({"year": 2026, "openingBalanceCents": 4000000})),
    )
    .await;

    for (month, kind, amount, comment, tax) in [
        (1u8, "expense", 110000i64, "Miete", false),
        (1, "income", 55000, "Miete", false),
        (1, "income", 300000, "Gehalt", false),
        (2, "expense", 4250, "Lebensmittel", false),
        (2, "expense", 39000, "Uni Gebühren", true),
        (3, "transfer", 100000, "to ING", false),
        (3, "income", 600000, "Freelancing", true),
    ] {
        let (status, body) = app
            .send(
                "POST",
                "/bookings",
                Some(json!({
                    "year": 2026, "month": month, "kind": kind,
                    "amountCents": amount, "comment": comment, "taxRelevant": tax
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    // A manual override: the rule table says nothing about this comment, the user does.
    let (_, override_booking) = app
        .send(
            "POST",
            "/bookings",
            Some(json!({
                "year": 2026, "month": 4, "kind": "expense", "amountCents": 20000,
                "comment": "Hotel Wien", "categoryId": dienstreisen
            })),
        )
        .await;
    assert_eq!(override_booking["categorySource"], "manual");

    make_template(
        &app,
        json!({
            "name": "Mafit", "comment": "Mafit", "kind": "expense",
            "amountCents": 2900, "amountIsEstimate": true,
            "intervalMonths": 1, "activeFrom": {"year": 2026, "month": 1}
        }),
    )
    .await;
    let (_, run) = app
        .send(
            "POST",
            "/recurring/materialize",
            Some(json!({"year": 2026, "month": 5})),
        )
        .await;
    assert_eq!(run["drafts"], 1, "der Entwurf gehört mit in den Export");

    let (status, _, bytes) = app.get_raw("/exports/bookings.json").await;
    assert_eq!(status, StatusCode::OK);
    let document: Value = serde_json::from_slice(&bytes).expect("export json");
    assert_eq!(document["bookings"].as_array().unwrap().len(), 9);
    assert_eq!(document["recurringTemplates"].as_array().unwrap().len(), 1);

    // ---- into a fresh, genuinely separate tenant ----
    let other = app.create_second_user("zweitkonto").await;
    let (status, empty) = app
        .send_as(Some(&other), "GET", "/dashboard?year=2026", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty["bookingCount"], 0, "das Zielkonto ist leer");

    let (status, restored) = app
        .send_as(Some(&other), "POST", "/exports/restore", Some(document))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{restored}");
    assert_eq!(restored["bookingsCreated"], 9);
    assert_eq!(restored["templatesCreated"], 1);
    assert_eq!(
        restored["ruleLinksDowngraded"], 0,
        "jede Regel wurde wiedergefunden"
    );

    // ---- and every report agrees, field by field ----
    for path in [
        "/dashboard?year=2026",
        "/overview/months?year=2026",
        "/analysis/categories?year=2026",
        "/tax?year=2026",
        "/years",
    ] {
        let (_, mine) = app.send("GET", path, None).await;
        let (_, theirs) = app.send_as(Some(&other), "GET", path, None).await;
        assert_eq!(
            strip_ids(&mine),
            strip_ids(&theirs),
            "{path} weicht nach dem Round-Trip ab"
        );
    }

    // The properties that would be silently wrong if only the totals matched.
    let (_, theirs) = app
        .send_as(Some(&other), "GET", "/dashboard?year=2026", None)
        .await;
    assert_eq!(
        theirs["openingBalanceCents"], 4000000,
        "der Vortrag reist mit"
    );
    assert_eq!(theirs["balanceCents"], 781750, "955.000 ein − 173.250 aus");
    assert_eq!(theirs["bookingCount"], 8, "der Entwurf zählt nicht mit");
    assert_eq!(theirs["taxRelevantCount"], 2);

    let (_, their_bookings) = app
        .send_as(
            Some(&other),
            "GET",
            "/bookings?year=2026&status=draft",
            None,
        )
        .await;
    assert_eq!(their_bookings["total"], 1, "ein Entwurf bleibt ein Entwurf");

    let (_, their_list) = app
        .send_as(
            Some(&other),
            "GET",
            "/bookings?year=2026&search=Hotel",
            None,
        )
        .await;
    assert_eq!(
        their_list["items"][0]["categorySource"], "manual",
        "eine manuelle Zuordnung bleibt manuell"
    );
    assert_eq!(their_list["items"][0]["categoryName"], "Dienstreisen");

    let (_, their_transfer) = app
        .send_as(
            Some(&other),
            "GET",
            "/bookings?year=2026&kind=transfer",
            None,
        )
        .await;
    assert_eq!(their_transfer["total"], 1);
    assert_eq!(
        their_transfer["sumNetCents"], 0,
        "eine Umbuchung nettet zu null"
    );

    // Restoring over an account that already has bookings is refused rather than
    // merged: merge semantics for two ledgers that both claim to be true would be an
    // invention, and inventing one quietly is how money goes missing.
    let (_, _, bytes) = app.get_raw("/exports/bookings.json").await;
    let again: Value = serde_json::from_slice(&bytes).unwrap();
    let (status, _) = app
        .send_as(Some(&other), "POST", "/exports/restore", Some(again))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

/// A second tenant must not see the first tenant's templates or receipts, and must
/// not be able to reach them by id. 404, not 403 — a 403 confirms the id exists.
#[tokio::test]
async fn recurring_templates_and_receipts_are_tenant_scoped() {
    let mut app = app!();
    app.setup_admin().await;
    let booking = make_booking(&app, "Kaufland", 1907, true).await;
    app.upload_bytes(
        &format!("/bookings/{booking}/receipt"),
        "beleg.png",
        "image/png",
        ONE_PIXEL_PNG,
    )
    .await;
    let template = make_template(
        &app,
        json!({
            "name": "Miete", "comment": "Miete", "kind": "expense",
            "amountCents": 110000, "activeFrom": {"year": 2026, "month": 1}
        }),
    )
    .await;

    let other = app.create_second_user("fremder").await;
    let (_, list) = app.send_as(Some(&other), "GET", "/recurring", None).await;
    assert_eq!(list.as_array().unwrap().len(), 0);

    for (method, path) in [
        ("PUT", format!("/recurring/{template}")),
        ("DELETE", format!("/recurring/{template}")),
        ("DELETE", format!("/bookings/{booking}/receipt")),
        ("GET", format!("/bookings/{booking}/receipt")),
    ] {
        let body = (method == "PUT").then(|| {
            json!({
                "name": "Gekapert", "comment": "Gekapert", "kind": "expense",
                "amountCents": 1, "activeFrom": {"year": 2026, "month": 1}
            })
        });
        let (status, _) = app.send_as(Some(&other), method, &path, body).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {path} muss 404 sein, nicht 403"
        );
    }

    // And the first tenant's data is untouched.
    let (_, mine) = app.send("GET", "/recurring", None).await;
    assert_eq!(mine[0]["name"], "Miete");
}

/// The same round trip, against the real 2026 workbook, asserting the acceptance
/// numbers come back out the other side.
///
/// The synthetic round-trip above proves the mechanism; this one proves it on the
/// real ledger, with its rule table, its manual overrides and the netting they
/// depend on. Skips when the workbook is absent — it carries personal financial
/// data and is deliberately not in the repository, and neither is `expected.json`.
#[tokio::test]
async fn the_round_trip_reproduces_the_2026_acceptance_numbers() {
    let (Ok(_), Ok(_)) = (
        std::fs::metadata("../konten_2026_auswertung.xlsx"),
        std::fs::metadata("tests/fixtures/expected.json"),
    ) else {
        eprintln!("SKIP: source workbook or expected.json not present");
        return;
    };
    let mut app = app!();
    app.setup_admin().await;

    let taxonomy = {
        let bytes = std::fs::read("../konten_2026_auswertung.xlsx").unwrap();
        finanzen::sheets::read_xlsx_taxonomy(&bytes).unwrap()
    };
    let (_, cats) = app.send("GET", "/categories", None).await;
    let ids: std::collections::BTreeMap<String, String> = cats
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["name"].as_str().unwrap().to_string(),
                c["id"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    for (pattern, category) in &taxonomy.rules {
        // The one correction applied at import: Sport, not the workbook's Haustier.
        let category = match pattern.as_str() {
            "mapet" | "mapet guthaben" => "Sport",
            _ => category.as_str(),
        };
        if let Some(id) = ids.get(category) {
            app.send(
                "POST",
                "/rules",
                Some(json!({"comment": pattern, "categoryId": id})),
            )
            .await;
        }
    }

    let import_id = app.upload_workbook("../konten_2026_auswertung.xlsx").await;
    app.send(
        "POST",
        &format!("/imports/{import_id}/commit"),
        Some(json!({})),
    )
    .await;
    app.send(
        "PUT",
        "/years/2026",
        Some(json!({"year": 2026, "openingBalanceCents": expected("y2026.openingCents")})),
    )
    .await;

    let (_, mine) = app.send("GET", "/dashboard?year=2026", None).await;
    assert_eq!(
        mine["balanceCents"],
        expected("y2026.saldoCents"),
        "Bilanz 2026"
    );
    assert_eq!(
        mine["closingBalanceCents"],
        expected("y2026.closingCents"),
        "Bilanz gesamt"
    );

    let (_, _, bytes) = app.get_raw("/exports/bookings.json").await;
    let document: Value = serde_json::from_slice(&bytes).expect("export json");
    assert_eq!(
        document["bookings"].as_array().unwrap().len() as i64,
        expected("y2026.bookingCount")
    );

    let other = app.create_second_user("wiederhergestellt").await;
    let (status, restored) = app
        .send_as(Some(&other), "POST", "/exports/restore", Some(document))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{restored}");
    assert_eq!(restored["bookingsCreated"], expected("y2026.bookingCount"));
    assert_eq!(restored["ruleLinksDowngraded"], 0);

    let (_, theirs) = app
        .send_as(Some(&other), "GET", "/dashboard?year=2026", None)
        .await;
    assert_eq!(
        theirs["incomeCents"],
        expected("y2026.incomeCents"),
        "Einnahmen"
    );
    assert_eq!(
        theirs["expenseCents"],
        expected("y2026.expenseCents"),
        "Ausgaben"
    );
    assert_eq!(
        theirs["balanceCents"],
        expected("y2026.saldoCents"),
        "Bilanz"
    );
    assert_eq!(
        theirs["openingBalanceCents"],
        expected("y2026.openingCents"),
        "Vortrag"
    );
    assert_eq!(
        theirs["closingBalanceCents"],
        expected("y2026.closingCents"),
        "Bilanz gesamt"
    );
    assert_eq!(theirs["taxRelevantCount"], expected("y2026.taxCount"));
    assert_eq!(theirs["uncategorizedCount"], 0);

    for path in [
        "/dashboard?year=2026",
        "/overview/months?year=2026",
        "/analysis/categories?year=2026",
        "/tax?year=2026",
    ] {
        let (_, mine) = app.send("GET", path, None).await;
        let (_, theirs) = app.send_as(Some(&other), "GET", path, None).await;
        assert_eq!(
            strip_ids(&mine),
            strip_ids(&theirs),
            "{path} weicht nach dem Round-Trip ab"
        );
    }

    // Juni's negative variable-cost figure is the single best regression test for
    // netting; it has to survive the export too.
    let (_, months) = app
        .send_as(Some(&other), "GET", "/overview/months?year=2026", None)
        .await;
    let juni = months["months"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["month"] == 6)
        .expect("Juni");
    assert_eq!(
        juni["variableCostsNetCents"],
        expected("y2026.juneVariableNetCents")
    );
}

/// A ledger is read from the end: the bookings someone is most likely to be fixing
/// are the ones they just made. Ascending order also means the default page shows
/// January while the user is looking for last week.
#[tokio::test]
async fn bookings_are_newest_first_unless_asked_otherwise() {
    let mut app = app!();
    app.setup_admin().await;

    for (month, comment) in [
        (1, "Januar-Buchung"),
        (6, "Juni-Buchung"),
        (9, "September-Buchung"),
    ] {
        app.send(
            "POST",
            "/bookings",
            Some(json!({"year":2026,"month":month,"kind":"expense",
                        "amountCents":1000,"comment":comment})),
        )
        .await;
    }

    let (_, newest) = app.send("GET", "/bookings?year=2026", None).await;
    let order: Vec<&str> = newest["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["comment"].as_str().unwrap())
        .collect();
    assert_eq!(
        order,
        vec!["September-Buchung", "Juni-Buchung", "Januar-Buchung"],
        "die Standardsortierung muss die neueste Buchung zuerst zeigen"
    );

    let (_, oldest) = app
        .send("GET", "/bookings?year=2026&direction=asc", None)
        .await;
    let order: Vec<&str> = oldest["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["comment"].as_str().unwrap())
        .collect();
    assert_eq!(
        order,
        vec!["Januar-Buchung", "Juni-Buchung", "September-Buchung"]
    );
}

/// Paging must partition the rows: every booking appears on exactly one page, and
/// none appears on two. A reversed first column with un-reversed tiebreakers is the
/// classic way to break this without it being visible on page one.
#[tokio::test]
async fn paging_partitions_the_rows_in_both_directions() {
    let mut app = app!();
    app.setup_admin().await;

    // Same month and same amount, so only the tiebreakers separate them.
    for i in 0..7 {
        app.send(
            "POST",
            "/bookings",
            Some(json!({"year":2026,"month":3,"kind":"expense",
                        "amountCents":500,"comment":format!("Buchung {i}")})),
        )
        .await;
    }

    for direction in ["desc", "asc"] {
        let mut seen: Vec<String> = Vec::new();
        for page in 0..3 {
            let (_, body) = app
                .send(
                    "GET",
                    &format!("/bookings?year=2026&pageSize=3&page={page}&direction={direction}"),
                    None,
                )
                .await;
            for item in body["items"].as_array().unwrap() {
                seen.push(item["id"].as_str().unwrap().to_string());
            }
        }
        let unique: std::collections::BTreeSet<&String> = seen.iter().collect();
        assert_eq!(
            seen.len(),
            7,
            "{direction}: alle sieben Buchungen über drei Seiten"
        );
        assert_eq!(
            unique.len(),
            7,
            "{direction}: keine Buchung auf zwei Seiten"
        );
    }
}

// ------------------------------------------------------------------- search

/// "What have I ever paid this merchant" — one request, every year.
///
/// The listing is year-scoped on purpose, which turns this question into four page
/// loads and a mental addition. The per-year summary is the actual answer here, so
/// it is what the assertions are about; the rows are only the evidence.
#[tokio::test]
async fn a_search_spans_every_year_and_sums_each_one() {
    let mut app = app!();
    app.setup_admin().await;

    for (year, month, cents, comment) in [
        (2024, 3, 4_210, "Hofladen Brinkmann"),
        (2025, 7, 1_999, "hofladen brinkmann"),
        (2025, 11, 3_000, "Hofladen Brinkmann Berlin"),
        (2026, 2, 2_500, "Hofladen Brinkmann"),
        // A refund from the same merchant: income, so the year nets lower.
        (2026, 4, 500, "Hofladen Brinkmann"),
        // Must not match.
        (2026, 5, 9_999, "Kaufland"),
    ] {
        let kind = if cents == 500 { "income" } else { "expense" };
        app.send(
            "POST",
            "/bookings",
            Some(json!({"year":year,"month":month,"kind":kind,
                        "amountCents":cents,"comment":comment})),
        )
        .await;
    }

    let (status, out) = app
        .send("GET", "/bookings/search?q=hofladen%20brinkmann", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{out}");

    // Five rows across three years — and the unrelated merchant is not one of them.
    assert_eq!(out["total"], 5);
    assert_eq!(out["query"], "hofladen brinkmann");

    let years = out["byYear"].as_array().expect("byYear");
    assert_eq!(years.len(), 3, "three years matched: {years:?}");
    // Newest first, so the most recent year is the one you read without scrolling.
    assert_eq!(years[0]["year"], 2026);
    assert_eq!(years[0]["bookingCount"], 2);
    assert_eq!(years[0]["expenseCents"], 2_500);
    assert_eq!(years[0]["incomeCents"], 500);
    // Stored convention: expenses minus income of the same rows.
    assert_eq!(years[0]["netCents"], 2_000);

    assert_eq!(years[1]["year"], 2025);
    assert_eq!(years[1]["bookingCount"], 2);
    assert_eq!(years[1]["netCents"], 1_999 + 3_000);

    assert_eq!(years[2]["year"], 2024);
    assert_eq!(years[2]["netCents"], 4_210);

    // The grand total is the sum of the years, which is the only reason to print
    // both on one screen.
    let year_net: i64 = years.iter().map(|y| y["netCents"].as_i64().unwrap()).sum();
    assert_eq!(out["sumNetCents"].as_i64().unwrap(), year_net);
    assert_eq!(out["sumExpenseCents"], 4_210 + 1_999 + 3_000 + 2_500);
    assert_eq!(out["sumIncomeCents"], 500);

    // Spellings: `Hofladen Brinkmann` and `hofladen brinkmann` are ONE merchant,
    // folded by the stored match_key and reported under the spelling used most
    // recently. The suffixed one is a different key and stays its own row.
    let comments = out["comments"].as_array().expect("comments");
    assert_eq!(comments.len(), 2, "{comments:?}");
    assert_eq!(comments[0]["comment"], "Hofladen Brinkmann");
    assert_eq!(comments[0]["bookingCount"], 4);
    assert_eq!(comments[1]["comment"], "Hofladen Brinkmann Berlin");
}

/// Case folding is the schema's, not the query's: `match_key` is a generated
/// `lower(btrim(comment))`, and searching by any casing or with stray spaces finds
/// the same rows.
#[tokio::test]
async fn a_search_folds_case_and_whitespace_like_the_rule_table() {
    let mut app = app!();
    app.setup_admin().await;
    for comment in ["Kaufland", "KAUFLAND", "  kaufland  "] {
        app.send(
            "POST",
            "/bookings",
            Some(json!({"year":2026,"month":1,"kind":"expense",
                        "amountCents":1_000,"comment":comment})),
        )
        .await;
    }

    // Percent-encoded, because the point is that the SERVER trims, not the client.
    for needle in ["kaufland", "KaUfLaNd", "%20%20Kaufland%20"] {
        let (status, out) = app
            .send("GET", &format!("/bookings/search?q={needle}"), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(out["total"], 3, "needle {needle:?}");
    }

    // A fragment matches, because a person searching types part of a name where a
    // rule states a whole key.
    let (_, out) = app.send("GET", "/bookings/search?q=aufl", None).await;
    assert_eq!(out["total"], 3);

    // Nothing asked, nothing claimed: an empty query must not become "everything".
    let (status, empty) = app.send("GET", "/bookings/search?q=%20%20", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty["total"], 0);
    assert_eq!(empty["sumNetCents"], 0);
    assert!(empty["byYear"].as_array().expect("byYear").is_empty());
}

/// The search can be narrowed to one category, and the summary narrows with it.
#[tokio::test]
async fn a_search_can_be_confined_to_one_category() {
    let mut app = app!();
    app.setup_admin().await;
    let food = app.category_id("Lebensmittel").await;
    let out_eating = app.category_id("Essen auswärts").await;

    app.send(
        "POST",
        "/bookings",
        Some(
            json!({"year":2026,"month":1,"kind":"expense","amountCents":2_000,
                    "comment":"Markt","categoryId":food}),
        ),
    )
    .await;
    app.send(
        "POST",
        "/bookings",
        Some(
            json!({"year":2025,"month":1,"kind":"expense","amountCents":3_000,
                    "comment":"Markt Imbiss","categoryId":out_eating}),
        ),
    )
    .await;

    let (_, all) = app.send("GET", "/bookings/search?q=markt", None).await;
    assert_eq!(all["total"], 2);

    let (_, only) = app
        .send(
            "GET",
            &format!("/bookings/search?q=markt&categoryId={food}"),
            None,
        )
        .await;
    assert_eq!(only["total"], 1);
    assert_eq!(only["byYear"].as_array().expect("byYear").len(), 1);
    assert_eq!(only["byYear"][0]["year"], 2026);
    assert_eq!(only["sumNetCents"], 2_000);
}

/// The year-scoped listing is untouched by any of the above: omitting `year` has
/// always spanned every year, and passing one still confines the answer to it.
#[tokio::test]
async fn the_listing_still_scopes_to_a_year_and_still_spans_all_of_them_without_one() {
    let mut app = app!();
    app.setup_admin().await;
    for year in [2024, 2025, 2026] {
        app.send(
            "POST",
            "/bookings",
            Some(json!({"year":year,"month":6,"kind":"expense",
                        "amountCents":1_000,"comment":"Strom"})),
        )
        .await;
    }

    let (status, scoped) = app.send("GET", "/bookings?year=2025", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(scoped["total"], 1);
    assert_eq!(scoped["sumExpenseCents"], 1_000);

    let (status, all) = app.send("GET", "/bookings", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(all["total"], 3);
    assert_eq!(all["sumExpenseCents"], 3_000);
}

/// Twelve months for one comment — "how much do I spend on tanken, and is it
/// getting worse". The spreadsheet's Filter tab answered this and it is the
/// question a household asks more often than "what did the category cost".
#[tokio::test]
async fn a_comment_can_be_charted_across_the_year() {
    let mut app = app!();
    app.setup_admin().await;
    let auto = app.category_id("Auto & Parken").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment":"tanken","categoryId":auto})),
    )
    .await;

    for (month, cents) in [(1, 6_500), (1, 5_500), (3, 7_200), (9, 8_100)] {
        app.send(
            "POST",
            "/bookings",
            Some(json!({"year":2026,"month":month,"kind":"expense",
                        "amountCents":cents,"comment":"tanken"})),
        )
        .await;
    }
    // A different comment in the same category must not bleed into the series.
    app.send(
        "POST",
        "/bookings",
        Some(json!({"year":2026,"month":1,"kind":"expense",
                    "amountCents":2_500,"comment":"Parkhaus"})),
    )
    .await;

    let (status, s) = app
        .send("GET", "/analysis/series?year=2026&comment=tanken", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(s["mode"], "comment");
    assert_eq!(s["subject"], "tanken");
    assert_eq!(s["bookingCount"], 4, "Parkhaus gehört nicht dazu");
    assert_eq!(s["expenseCents"], 27_300);

    let months = s["months"].as_array().unwrap();
    assert_eq!(months.len(), 12, "immer zwölf, auch die leeren");
    assert_eq!(months[0]["netCents"], 12_000, "Januar: beide Tankfüllungen");
    assert_eq!(months[1]["netCents"], 0, "Februar ist leer, nicht abwesend");
    assert_eq!(months[8]["netCents"], 8_100);

    // Averaged over the months that carry a booking for THIS comment, not over
    // twelve and not over the year's nine — a summer-only expense should not be
    // made to look small by the months it never occurs in.
    assert_eq!(s["monthsWithData"], 3);
    assert_eq!(s["averagePerActiveMonthCents"], 9_100);
}

/// The same endpoint, asked about a category, and the mutual exclusivity that
/// keeps the two questions from being confused.
#[tokio::test]
async fn the_series_takes_a_category_or_a_comment_but_not_both() {
    let mut app = app!();
    app.setup_admin().await;
    let auto = app.category_id("Auto & Parken").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment":"tanken","categoryId":auto})),
    )
    .await;
    app.send(
        "POST",
        "/bookings",
        Some(json!({"year":2026,"month":5,"kind":"expense",
                    "amountCents":6_000,"comment":"tanken"})),
    )
    .await;

    let (_, by_category) = app
        .send(
            "GET",
            &format!("/analysis/series?year=2026&categoryId={auto}"),
            None,
        )
        .await;
    assert_eq!(by_category["mode"], "category");
    // The name is looked up, not echoed, so a rename answers with the current one.
    assert_eq!(by_category["subject"], "Auto & Parken");
    assert_eq!(by_category["months"][4]["netCents"], 6_000);

    for query in [
        "/analysis/series?year=2026",
        "/analysis/series?year=2026&comment=tanken&categoryId=00000000-0000-0000-0000-000000000000",
    ] {
        let (status, _) = app.send("GET", query, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
    }
}

/// Matching is case-insensitive on the trimmed comment, exactly as the rule table
/// matches — otherwise "Tanken" and "tanken" would chart as two different things.
#[tokio::test]
async fn a_comment_series_folds_case_the_way_the_rules_do() {
    let mut app = app!();
    app.setup_admin().await;
    for comment in ["tanken", "Tanken", "  tanken  "] {
        app.send(
            "POST",
            "/bookings",
            Some(json!({"year":2026,"month":2,"kind":"expense",
                        "amountCents":5_000,"comment":comment})),
        )
        .await;
    }

    let (_, s) = app
        .send("GET", "/analysis/series?year=2026&comment=TANKEN", None)
        .await;
    assert_eq!(
        s["bookingCount"], 3,
        "alle drei Schreibweisen sind dasselbe"
    );
    assert_eq!(s["months"][1]["netCents"], 15_000);
}

// ------------------------------------------------------------- year on year

/// The trap this endpoint exists to avoid.
///
/// A part year against a full one is not a comparison, it is a subtraction of the
/// missing months. 2026 holds nine months in the real ledger and 2025 holds twelve,
/// so the raw totals would report a 25 % improvement caused entirely by the calendar.
#[tokio::test]
async fn a_part_year_is_never_compared_with_a_full_one() {
    let mut app = app!();
    app.setup_admin().await;
    let miete = app.category_id("Miete").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment":"Miete","categoryId":miete})),
    )
    .await;

    // Last year: twelve months at 100,00 each.
    for month in 1..=12 {
        app.send(
            "POST",
            "/bookings",
            Some(json!({"year":2025,"month":month,"kind":"expense",
                        "amountCents":10_000,"comment":"Miete"})),
        )
        .await;
    }
    // This year: three months at the very same 100,00.
    for month in 1..=3 {
        app.send(
            "POST",
            "/bookings",
            Some(json!({"year":2026,"month":month,"kind":"expense",
                        "amountCents":10_000,"comment":"Miete"})),
        )
        .await;
    }

    let (status, c) = app.send("GET", "/analysis/compare?year=2026", None).await;
    assert_eq!(status, StatusCode::OK, "{c}");
    assert_eq!(c["previousYear"], 2025);
    assert_eq!(c["previousYearHasData"], true);

    // The raw figures are reported honestly and are NOT a comparison.
    assert_eq!(c["current"]["expenseCents"], 30_000);
    assert_eq!(c["previous"]["expenseCents"], 120_000);
    assert_eq!(c["current"]["monthsWithData"], 3);
    assert_eq!(c["previous"]["monthsWithData"], 12);
    assert_eq!(
        c["fullyComparable"], false,
        "drei Monate gegen zwölf ist kein Vergleich"
    );

    // Restricted to the months both years have, nothing changed at all — which is
    // the truth about this household, and the opposite of what the raw totals say.
    assert_eq!(c["comparableMonths"].as_array().unwrap().len(), 3);
    assert_eq!(c["current"]["comparableExpenseCents"], 30_000);
    assert_eq!(c["previous"]["comparableExpenseCents"], 30_000);

    let row = c["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["categoryName"] == "Miete")
        .expect("Miete");
    assert_eq!(row["netCents"], 30_000);
    assert_eq!(row["previousNetCents"], 120_000);
    assert_eq!(row["deltaCents"], -90_000, "roh: neun Monate fehlen");
    assert_eq!(
        row["comparableDeltaCents"], 0,
        "gemeinsame Monate: unverändert"
    );
    assert_eq!(row["comparableDeltaRatio"], 0.0);
}

/// A category that did not exist last year has no percentage, and says so.
#[tokio::test]
async fn a_new_category_reports_no_ratio_rather_than_infinity() {
    let mut app = app!();
    app.setup_admin().await;
    let miete = app.category_id("Miete").await;
    let abos = app.category_id("Abos & Streaming").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment":"Miete","categoryId":miete})),
    )
    .await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment":"Spotify","categoryId":abos})),
    )
    .await;
    for (year, comment) in [(2025, "Miete"), (2026, "Miete"), (2026, "Spotify")] {
        app.send(
            "POST",
            "/bookings",
            Some(json!({"year":year,"month":1,"kind":"expense",
                        "amountCents":5_000,"comment":comment})),
        )
        .await;
    }

    let (status, c) = app.send("GET", "/analysis/compare?year=2026", None).await;
    assert_eq!(status, StatusCode::OK);
    let rows = c["rows"].as_array().unwrap();
    let spotify = rows
        .iter()
        .find(|r| r["categoryName"] == "Abos & Streaming")
        .expect("Spotify-Kategorie");
    assert_eq!(spotify["previousNetCents"], 0);
    assert!(spotify["deltaRatio"].is_null(), "kein Prozent gegen nichts");
    assert_eq!(spotify["isNew"], true);
    assert_eq!(spotify["isGone"], false);
}

/// An income category compares in the direction money moves, not in the direction
/// its stored sign points.
#[tokio::test]
async fn earning_more_reads_as_more_money_not_as_a_bigger_cost() {
    let mut app = app!();
    app.setup_admin().await;
    let gehalt = app.category_id("Gehalt").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment":"Gehalt","categoryId":gehalt})),
    )
    .await;
    app.send(
        "POST",
        "/bookings",
        Some(json!({"year":2025,"month":1,"kind":"income",
                    "amountCents":250_000,"comment":"Gehalt"})),
    )
    .await;
    app.send(
        "POST",
        "/bookings",
        Some(json!({"year":2026,"month":1,"kind":"income",
                    "amountCents":275_000,"comment":"Gehalt"})),
    )
    .await;

    let (_, c) = app.send("GET", "/analysis/compare?year=2026", None).await;
    let row = c["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["categoryName"] == "Gehalt")
        .expect("Gehalt");
    // Stored convention: income is negative, so earning 250,00 more is -25.000.
    assert_eq!(row["netCents"], -275_000);
    assert_eq!(row["previousNetCents"], -250_000);
    assert_eq!(row["deltaCents"], -25_000);
    assert_eq!(row["deltaRatio"], -0.1);
}

/// Twelve months ending in February means eleven of them are last year.
#[tokio::test]
async fn the_trailing_window_crosses_the_year_boundary() {
    let mut app = app!();
    app.setup_admin().await;
    let miete = app.category_id("Miete").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment":"Miete","categoryId":miete})),
    )
    .await;
    // One booking in every month of 2025, and two in Januar and Februar 2026.
    for month in 1..=12 {
        app.send(
            "POST",
            "/bookings",
            Some(json!({"year":2025,"month":month,"kind":"expense",
                        "amountCents":1_000,"comment":"Miete"})),
        )
        .await;
    }
    for month in 1..=2 {
        app.send(
            "POST",
            "/bookings",
            Some(json!({"year":2026,"month":month,"kind":"expense",
                        "amountCents":2_000,"comment":"Miete"})),
        )
        .await;
    }

    let (status, w) = app
        .send("GET", "/analysis/trailing?year=2026&month=2", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{w}");
    assert_eq!(w["fromYear"], 2025);
    assert_eq!(w["fromMonth"], 3, "März 2025 bis Februar 2026");

    let months = w["months"].as_array().unwrap();
    assert_eq!(months.len(), 12, "immer zwölf, auch über die Jahresgrenze");
    assert_eq!(months[0]["year"], 2025);
    assert_eq!(months[0]["month"], 3);
    assert_eq!(months[11]["year"], 2026);
    assert_eq!(months[11]["month"], 2);

    // Ten months of 2025 (März–Dezember) plus two of 2026.
    assert_eq!(w["expenseCents"], 10 * 1_000 + 2 * 2_000);
    assert_eq!(w["bookingCount"], 12);
    assert_eq!(w["monthsWithData"], 12);

    // The category's twelve values are in WINDOW order, not calendar order.
    let row = &w["rows"].as_array().unwrap()[0];
    assert_eq!(row["categoryName"], "Miete");
    assert_eq!(row["monthlyNetCents"][0], 1_000, "März 2025");
    assert_eq!(row["monthlyNetCents"][11], 2_000, "Februar 2026");
}

/// An empty month inside the window is present and empty, never dropped.
#[tokio::test]
async fn a_gap_inside_the_trailing_window_stays_a_gap() {
    let mut app = app!();
    app.setup_admin().await;
    let miete = app.category_id("Miete").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment":"Miete","categoryId":miete})),
    )
    .await;
    for (year, month) in [(2025, 12), (2026, 2)] {
        app.send(
            "POST",
            "/bookings",
            Some(json!({"year":year,"month":month,"kind":"expense",
                        "amountCents":1_000,"comment":"Miete"})),
        )
        .await;
    }

    let (_, w) = app
        .send("GET", "/analysis/trailing?year=2026&month=2", None)
        .await;
    let months = w["months"].as_array().unwrap();
    assert_eq!(months.len(), 12);
    // Januar 2026 sits between them with nothing in it.
    let januar = months
        .iter()
        .find(|m| m["year"] == 2026 && m["month"] == 1)
        .unwrap();
    assert_eq!(januar["bookingCount"], 0);
    assert_eq!(januar["netCents"], 0);
    assert_eq!(w["monthsWithData"], 2);

    let (status, _) = app
        .send("GET", "/analysis/trailing?year=2026&month=13", None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ------------------------------------------------------- forecast and anomalies

/// Books one expense into one month, with a rule so it lands in a real category.
async fn book(app: &TestApp, year: i32, month: u8, comment: &str, cents: i64) {
    let (status, body) = app
        .send(
            "POST",
            "/bookings",
            Some(json!({"year": year, "month": month, "kind": "expense",
                        "amountCents": cents, "comment": comment})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// THE test for the projection: one settlement month must not move it.
///
/// Five ordinary Lebensmittel months of ~80 € and one of 1.479 €. A mean would
/// project ~313 € a month for the rest of the year and put the closing balance
/// thousands of euros wrong; the median projects the 80 € the month actually tends
/// to cost. This is the entire reason the engine says "median" on the wire.
#[tokio::test]
async fn one_outlier_month_barely_moves_the_forecast() {
    let mut app = app!();
    app.setup_admin().await;
    let food = app.category_id("Lebensmittel").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment": "Supermarkt", "categoryId": food})),
    )
    .await;

    for (month, cents) in [
        (1u8, 8_000i64),
        (2, 8_500),
        (3, 7_900),
        (4, 8_200),
        (5, 8_100),
        (6, 144_000),
    ] {
        book(&app, 2026, month, "Supermarkt", cents).await;
    }

    let (status, f) = app.send("GET", "/analysis/forecast?year=2026", None).await;
    assert_eq!(status, StatusCode::OK, "{f}");
    assert_eq!(f["method"], "median");
    assert_eq!(f["actualThroughMonth"], 6);
    assert_eq!(f["projectedFromMonth"], 7);

    // Juli..Dezember are projected at the median of the six months in the window,
    // not at the mean those same months would give (31.433 cents).
    let months = f["months"].as_array().expect("months");
    let july = &months[6];
    assert_eq!(july["isProjected"], true);
    assert!(
        july["netCents"].as_i64().expect("net") < 12_000,
        "the outlier leaked into the projection: {july}"
    );

    // ...and the months that happened are not projections.
    assert_eq!(months[5]["isProjected"], false);
    assert_eq!(months[5]["netCents"], 144_000);
    assert_eq!(months[5]["bookingCount"], 1);

    // The actual saldo is the six months that happened, as a balance delta.
    assert_eq!(
        f["actualBalanceCents"],
        -(8_000 + 8_500 + 7_900 + 8_200 + 8_100 + 144_000)
    );
}

/// A template due in a remaining month is counted once, and never twice.
///
/// The trap this guards is adding the template to the category's median, which
/// would project a rent nobody pays.
#[tokio::test]
async fn a_due_template_is_counted_once_in_each_remaining_month() {
    let mut app = app!();
    app.setup_admin().await;
    let rent = app.category_id("Miete").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment": "Miete", "categoryId": rent})),
    )
    .await;

    // Three months of rent actually booked, then a template for the rest.
    for month in 1..=3u8 {
        book(&app, 2026, month, "Miete", 120_000).await;
    }
    make_template(
        &app,
        json!({"name":"Miete","comment":"Miete","kind":"expense","amountCents":120_000,
               "activeFrom":{"year":2026,"month":1}}),
    )
    .await;

    let (status, f) = app.send("GET", "/analysis/forecast?year=2026", None).await;
    assert_eq!(status, StatusCode::OK, "{f}");

    let months = f["months"].as_array().expect("months");
    let april = &months[3];
    assert_eq!(april["isProjected"], true);
    // 1.200,00 — not 2.400,00, which is what adding the template to the median
    // would produce.
    assert_eq!(april["netCents"], 120_000);
    assert_eq!(april["fixedCents"], 120_000);
    assert_eq!(april["variableCents"], 0);
    // Nine remaining months, one template due in each.
    assert_eq!(f["dueTemplateCount"], 9);
}

/// A category with two months of history has no median worth reporting.
#[tokio::test]
async fn a_category_with_too_little_history_produces_no_anomaly() {
    let mut app = app!();
    app.setup_admin().await;
    let food = app.category_id("Lebensmittel").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment": "Supermarkt", "categoryId": food})),
    )
    .await;

    // Two quiet months, then one that is wildly different.
    book(&app, 2026, 4, "Supermarkt", 8_000).await;
    book(&app, 2026, 5, "Supermarkt", 8_000).await;
    book(&app, 2026, 6, "Supermarkt", 90_000).await;

    let (status, a) = app
        .send("GET", "/analysis/anomalies?year=2026&month=6", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{a}");
    assert_eq!(
        a["items"].as_array().expect("items").len(),
        0,
        "two data points have no middle: {a}"
    );

    // A third month of history makes the same spike reportable.
    book(&app, 2026, 3, "Supermarkt", 8_200).await;
    let (_, a) = app
        .send("GET", "/analysis/anomalies?year=2026&month=6", None)
        .await;
    let items = a["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "{a}");
    assert_eq!(items[0]["categoryName"], "Lebensmittel");
    assert_eq!(items[0]["direction"], "above");
    assert_eq!(items[0]["currentCents"], 90_000);
    assert_eq!(items[0]["medianCents"], 8_000);
    assert_eq!(items[0]["monthsOfHistory"], 3);
}

/// Saving is a decision, not overspending, and an empty list is the normal case.
#[tokio::test]
async fn savings_are_never_an_anomaly_and_a_quiet_month_reports_nothing() {
    let mut app = app!();
    app.setup_admin().await;
    let savings = app.category_id("Sparen & Anlage").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment": "ETF", "categoryId": savings})),
    )
    .await;

    for month in 1..=5u8 {
        book(&app, 2026, month, "ETF", 30_000).await;
    }
    // A deliberate extra payment into savings: large, and nobody's business.
    book(&app, 2026, 6, "ETF", 300_000).await;

    let (status, a) = app
        .send("GET", "/analysis/anomalies?year=2026&month=6", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{a}");
    assert_eq!(a["items"].as_array().expect("items").len(), 0, "{a}");
    // The thresholds are on the wire, so the UI can say why something is listed.
    assert_eq!(a["minDeltaCents"], 2_000);

    let (status, _) = app
        .send("GET", "/analysis/anomalies?year=2026&month=13", None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// A booking made today is dated today, not the first of the month.
///
/// The day is not cosmetic: it is what a KitchenOwl push files the expense under,
/// and defaulting to the 1st put a pushed booking two weeks up a date-sorted list,
/// where its author went looking at the top and concluded the push had failed. It
/// also decides what a bank CSV can be reconciled against.
#[tokio::test]
async fn a_booking_created_today_carries_today() {
    let mut app = app!();
    app.setup_admin().await;
    let today = chrono::Utc::now().date_naive();

    let (status, created) = app
        .send(
            "POST",
            "/bookings",
            Some(json!({
                "year": today.format("%Y").to_string().parse::<i32>().unwrap(),
                "month": today.format("%m").to_string().parse::<u8>().unwrap(),
                "kind": "expense", "amountCents": 7356, "comment": "Kaufland"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["bookedOn"], today.to_string());

    // A month that is not this one has no defensible day, so it keeps the 1st.
    let (_, other) = app
        .send(
            "POST",
            "/bookings",
            Some(json!({"year": 2026, "month": 1, "kind": "expense",
                        "amountCents": 500, "comment": "Kaufland"})),
        )
        .await;
    assert_eq!(other["bookedOn"], "2026-01-01");

    // And an explicit day always wins.
    let (_, exact) = app
        .send(
            "POST",
            "/bookings",
            Some(json!({"year": 2026, "month": 1, "bookedOn": "2026-01-17",
                        "kind": "expense", "amountCents": 500, "comment": "Kaufland"})),
        )
        .await;
    assert_eq!(exact["bookedOn"], "2026-01-17");
}

/// A bank statement, end to end: upload, review, commit — and the two things that
/// make this import different from a workbook.
///
/// Nothing is booked by uploading. A statement line is evidence of a payment, not
/// a decision about what it was: the comment a bank gives you is a card
/// terminal's name for a shop. So every line waits for a person, and the commit
/// takes only the accepted ones.
#[tokio::test]
async fn a_bank_statement_is_reviewed_line_by_line_before_anything_is_booked() {
    let mut app = app!();
    app.setup_admin().await;

    // Something already in the ledger for the statement to collide with: the same
    // amount, two days before the bank got round to booking it — the edge of the
    // window on purpose, because that is the case the window exists for.
    let (status, _) = app
        .send(
            "POST",
            "/bookings",
            Some(
                json!({"year": 2026, "month": 9, "kind": "expense", "amountCents": 6430,
                        "comment": "tanken", "bookedOn": "2026-09-16"}),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let statement = "Umsatzanzeige;Datei erstellt am: 10.03.2026 08:00\n\n\
         IBAN;DE00 0000 0000 0000 0000 00\nKontoname;Girokonto\nBank;ING\n\
         Saldo;512,40;EUR\n\n\
         Buchung;Wertstellungsdatum;Auftraggeber/Empfänger;Buchungstext;Verwendungszweck;\
         Saldo;Währung;Betrag;Währung\n\
         18.09.2026;18.09.2026;VISA TANKSTELLE NORD;Lastschrift;NR XXXX KAUFUMSATZ;512,40;EUR;\
         -64,30;EUR\n\
         17.09.2026;17.09.2026;MUSTER GMBH;Gutschrift;Lohn September;766,17;EUR;3186,00;EUR\n";
    let (status, preview) = app
        .upload_bytes("/imports", "auszug.csv", "text/csv", statement.as_bytes())
        .await;
    assert!(status.is_success(), "upload failed: {preview}");
    let import_id = preview["id"].as_str().expect("import id").to_string();
    assert_eq!(preview["source"].as_str(), Some("csv_ing"));

    // Uploading booked nothing at all.
    let (_, bookings) = app.send("GET", "/bookings?year=2026", None).await;
    assert_eq!(bookings["total"].as_i64(), Some(1));

    let (_, rows) = app
        .send("GET", &format!("/imports/{import_id}/statement"), None)
        .await;
    assert_eq!(rows["total"].as_i64(), Some(2));
    assert_eq!(rows["pending"].as_i64(), Some(2));
    // The line the ledger may already hold is flagged, and only flagged: a bank
    // really does charge the same amount at the same shop twice in a week.
    assert_eq!(rows["duplicates"].as_i64(), Some(1));

    let items = rows["items"].as_array().expect("items").clone();
    let flagged = items
        .iter()
        .find(|r| r["duplicateBookingId"].is_string())
        .expect("the duplicate");
    assert_eq!(flagged["duplicateComment"].as_str(), Some("tanken"));
    // The suggested comment is the payee without the card-network shouting.
    assert_eq!(flagged["comment"].as_str(), Some("Tankstelle Nord"));
    let salary = items
        .iter()
        .find(|r| r["kind"] == "income")
        .expect("the credit");
    // The SIGN decided that, not the word `Gutschrift`.
    assert_eq!(salary["amountCents"].as_i64(), Some(318_600));

    // Set the duplicate aside, and review the credit by hand into a category.
    let (status, bulk) = app
        .send(
            "POST",
            &format!("/imports/{import_id}/statement/bulk"),
            Some(json!({"scope": "duplicates", "decision": "rejected"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bulk["affected"].as_i64(), Some(1));

    let gehalt = app.category_id("Gehalt").await;
    let (status, _) = app
        .send(
            "PATCH",
            &format!(
                "/imports/{import_id}/statement/{}",
                salary["id"].as_str().unwrap()
            ),
            Some(json!({"comment": "Gehalt September", "categoryId": gehalt,
                        "decision": "accepted", "createRule": true})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, result) = app
        .send("POST", &format!("/imports/{import_id}/commit"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["inserted"].as_i64(), Some(1));

    // One booking, carrying the statement's own date and the bank's own words.
    let (_, bookings) = app.send("GET", "/bookings?year=2026", None).await;
    let booked = bookings["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|b| b["comment"] == "Gehalt September")
        .expect("the booked credit");
    assert_eq!(booked["bookedOn"].as_str(), Some("2026-09-17"));
    assert_eq!(booked["counterparty"].as_str(), Some("MUSTER GMBH"));
    assert_eq!(booked["purpose"].as_str(), Some("Lohn September"));
    assert_eq!(booked["categoryName"].as_str(), Some("Gehalt"));
    // ...and the review left a rule behind, so the same payer is never typed twice.
    let (_, rules) = app.send("GET", "/rules", None).await;
    assert!(
        rules
            .as_array()
            .expect("rules")
            .iter()
            .any(|r| r["comment"] == "Gehalt September"),
        "the accepted line should have written its rule"
    );
}

// ------------------------------------------------------------- review fixes

/// A password refused at setup used to be refused AFTER the account was written:
/// the first admin existed with no credential, setup would not run a second time,
/// and the instance was locked until someone edited the database by hand.
#[tokio::test]
async fn a_short_password_at_setup_leaves_nothing_behind() {
    let mut app = app!();
    let (status, _) = app
        .send(
            "POST",
            "/auth/setup",
            Some(json!({"username": "fabi", "displayName": "Fabian", "password": "kurz"})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Nothing was written, so setup is still available…
    let (_, setup) = app.send("GET", "/auth/setup-status", None).await;
    assert_eq!(setup["setupRequired"], true);
    // …and a second attempt with a proper password simply works.
    app.setup_admin().await;
}

/// The same trap on the admin's "create user": every retry of a refused password
/// then failed with "name already taken".
#[tokio::test]
async fn a_short_password_for_a_new_user_can_be_retried() {
    let mut app = app!();
    app.setup_admin().await;

    let (status, _) = app
        .send(
            "POST",
            "/admin/users",
            Some(json!({"username": "zweite", "displayName": "Zweite", "password": "kurz"})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, body) = app
        .send(
            "POST",
            "/admin/users",
            Some(json!({"username": "zweite", "displayName": "Zweite",
                        "password": "ein-langes-passwort"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// axum caps a body at 2 MB by default, so the configured 25 MB upload limit was
/// never reached: a 3 MB receipt photo died mid-upload as "could not parse
/// multipart". A file over 2 MB must at least reach the handler — which then
/// refuses this one for what it IS, not for its size.
#[tokio::test]
async fn an_upload_over_two_megabytes_reaches_the_handler() {
    let mut app = app!();
    app.setup_admin().await;

    let bytes = vec![b'x'; 3 * 1024 * 1024];
    let (status, body) = app
        .upload_bytes("/imports", "gross.csv", "text/csv", &bytes)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let message = body["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("Kopfzeile"),
        "a 3 MB file should be read and refused as a statement, got: {message}"
    );
}

/// A statement with a header and the given lines, in ING's shape.
fn statement(lines: &[&str]) -> Vec<u8> {
    let mut s = String::from(
        "Umsatzanzeige;Datei erstellt am: 10.03.2026 08:00\n\nIBAN;DE00 0000 0000 0000 0000 00\n\
         Bank;ING\n\nBuchung;Wertstellungsdatum;Auftraggeber/Empfänger;Buchungstext;\
         Verwendungszweck;Saldo;Währung;Betrag;Währung\n",
    );
    for line in lines {
        s.push_str(line);
        s.push('\n');
    }
    s.into_bytes()
}

impl TestApp {
    /// Uploads a statement and returns (import id, staged rows).
    async fn stage_statement(&self, bytes: &[u8]) -> (String, Vec<Value>) {
        let (status, preview) = self
            .upload_bytes("/imports", "auszug.csv", "text/csv", bytes)
            .await;
        assert!(status.is_success(), "upload failed: {preview}");
        let id = preview["id"].as_str().expect("import id").to_string();
        let (_, rows) = self
            .send("GET", &format!("/imports/{id}/statement"), None)
            .await;
        (id, rows["items"].as_array().expect("items").clone())
    }

    async fn review(&self, import: &str, row: &Value, body: Value) -> Value {
        let (status, view) = self
            .send(
                "PATCH",
                &format!(
                    "/imports/{import}/statement/{}",
                    row["id"].as_str().unwrap()
                ),
                Some(body),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{view}");
        view
    }
}

/// Accept re-sends the comment, and a rename re-asks the rule table — so every
/// Accept used to write the rule's category over the one the user had just
/// picked. What a person picks is changed by nothing but another pick.
#[tokio::test]
async fn accepting_a_statement_line_keeps_the_category_the_user_picked() {
    let mut app = app!();
    app.setup_admin().await;
    let auto = app.category_id("Auto & Parken").await;
    let urlaub = app.category_id("Reisen & Urlaub").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment": "Tankstelle Nord", "categoryId": auto})),
    )
    .await;

    let (import, rows) = app
        .stage_statement(&statement(&[
            "17.09.2026;17.09.2026;VISA TANKSTELLE NORD;Lastschrift;Kauf;700,00;EUR;-64,30;EUR",
        ]))
        .await;
    let line = &rows[0];
    assert_eq!(line["categoryId"].as_str(), Some(auto.to_string().as_str()));

    app.review(&import, line, json!({"categoryId": urlaub}))
        .await;
    let view = app
        .review(
            &import,
            line,
            json!({"comment": "Tankstelle Nord", "decision": "accepted"}),
        )
        .await;
    assert_eq!(
        view["categoryId"].as_str(),
        Some(urlaub.to_string().as_str())
    );
    assert_eq!(view["categorySource"].as_str(), Some("manual"));

    app.send("POST", &format!("/imports/{import}/commit"), None)
        .await;
    let (_, bookings) = app.send("GET", "/bookings?year=2026", None).await;
    assert_eq!(
        bookings["items"][0]["categoryName"].as_str(),
        Some("Reisen & Urlaub")
    );
}

/// …and the feature that caused it still works: renaming a line to a comment a
/// rule knows picks that rule's category by itself.
#[tokio::test]
async fn renaming_a_statement_line_still_applies_the_rule() {
    let mut app = app!();
    app.setup_admin().await;
    let auto = app.category_id("Auto & Parken").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment": "tanken", "categoryId": auto})),
    )
    .await;

    let (import, rows) = app
        .stage_statement(&statement(&[
            "17.09.2026;17.09.2026;VISA IRGENDWAS;Lastschrift;Kauf;700,00;EUR;-64,30;EUR",
        ]))
        .await;
    let view = app
        .review(&import, &rows[0], json!({"comment": "tanken"}))
        .await;
    assert_eq!(view["categoryId"].as_str(), Some(auto.to_string().as_str()));
    assert_eq!(view["categorySource"].as_str(), Some("rule"));
}

/// "No category" sent `categoryId: null`, which means "leave it alone", so a wrong
/// guess snapped straight back and was booked anyway.
#[tokio::test]
async fn no_category_on_a_statement_line_books_it_without_one() {
    let mut app = app!();
    app.setup_admin().await;
    let auto = app.category_id("Auto & Parken").await;
    app.send(
        "POST",
        "/rules",
        Some(json!({"comment": "Tankstelle Nord", "categoryId": auto})),
    )
    .await;

    let (import, rows) = app
        .stage_statement(&statement(&[
            "17.09.2026;17.09.2026;VISA TANKSTELLE NORD;Lastschrift;Kauf;700,00;EUR;-64,30;EUR",
        ]))
        .await;
    let view = app
        .review(&import, &rows[0], json!({"clearCategory": true}))
        .await;
    assert!(view["categoryId"].is_null(), "{view}");
    app.review(&import, &rows[0], json!({"decision": "accepted"}))
        .await;

    app.send("POST", &format!("/imports/{import}/commit"), None)
        .await;
    let (_, bookings) = app.send("GET", "/bookings?year=2026", None).await;
    // …even though a rule matches its comment: the user said none.
    assert!(bookings["items"][0]["categoryId"].is_null(), "{bookings}");
}

/// Two identical charges both pointed at one hand-entered booking, so setting the
/// duplicates aside rejected the genuine second charge too.
#[tokio::test]
async fn one_existing_booking_is_the_duplicate_of_at_most_one_line() {
    let mut app = app!();
    app.setup_admin().await;
    app.send(
        "POST",
        "/bookings",
        Some(
            json!({"year": 2026, "month": 9, "kind": "expense", "amountCents": 6430,
                    "comment": "tanken", "bookedOn": "2026-09-16"}),
        ),
    )
    .await;

    let (_, rows) = app
        .stage_statement(&statement(&[
            "18.09.2026;18.09.2026;VISA TANKSTELLE;Lastschrift;Kauf;635,70;EUR;-64,30;EUR",
            "17.09.2026;17.09.2026;VISA TANKSTELLE;Lastschrift;Kauf;700,00;EUR;-64,30;EUR",
        ]))
        .await;
    let flagged = rows
        .iter()
        .filter(|r| r["duplicateBookingId"].is_string())
        .count();
    assert_eq!(flagged, 1, "{rows:?}");
}

/// A statement without a running balance gave two identical same-day charges the
/// same fingerprint, and the commit dropped the second as a duplicate of itself.
#[tokio::test]
async fn two_identical_charges_without_a_balance_are_both_booked() {
    let mut app = app!();
    app.setup_admin().await;
    let (import, rows) = app
        .stage_statement(&statement(&[
            "17.09.2026;17.09.2026;BAECKEREI;Lastschrift;Kauf;;EUR;-3,90;EUR",
            "17.09.2026;17.09.2026;BAECKEREI;Lastschrift;Kauf;;EUR;-3,90;EUR",
        ]))
        .await;
    for row in &rows {
        app.review(&import, row, json!({"decision": "accepted"}))
            .await;
    }
    let (_, result) = app
        .send("POST", &format!("/imports/{import}/commit"), None)
        .await;
    assert_eq!(result["inserted"].as_i64(), Some(2), "{result}");
}

/// A guard, not a reproduction: a workbook's suggestions are only confirmed by the
/// review queue, so an unreviewed one must never be booked as the user's choice.
/// Workbook rows keep their suggestions in the queue today, so this holds; the
/// commit query now says so explicitly, and this keeps it that way should a
/// suggestion ever be staged on the row itself.
#[tokio::test]
async fn an_unreviewed_workbook_suggestion_is_not_booked_as_a_choice() {
    let mut app = app!();
    app.setup_admin().await;
    let id = app
        .upload_workbook("tests/fixtures/monthly_shape.xlsx")
        .await;
    let (status, _) = app
        .send("POST", &format!("/imports/{id}/commit"), None)
        .await;
    assert_eq!(status, StatusCode::OK);

    for year in [2014, 2015] {
        let (_, bookings) = app
            .send("GET", &format!("/bookings?year={year}"), None)
            .await;
        for b in bookings["items"].as_array().expect("items") {
            assert_ne!(
                b["categorySource"].as_str(),
                Some("manual"),
                "booked as the user's choice without a review: {b}"
            );
        }
    }
}
