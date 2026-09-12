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

const ORIGIN: &str = "http://localhost:3100";

struct TestApp {
    router: Router,
    cookie: Option<String>,
}

impl TestApp {
    async fn new() -> Option<Self> {
        let url = std::env::var("TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .ok()?;

        let name = format!("fin_api_{}", Uuid::new_v4().simple());
        let role = format!("fin_api_role_{}", Uuid::new_v4().simple());
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
        })
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
/// Skips when the workbooks are absent, because they carry personal financial data
/// and are deliberately not in the repository.
#[tokio::test]
async fn importing_the_real_workbooks_reproduces_the_acceptance_numbers() {
    let (Ok(_), Ok(_)) = (
        std::fs::metadata("../konten_2026_auswertung.xlsx"),
        std::fs::metadata("../konten.ods"),
    ) else {
        eprintln!("SKIP: source workbooks not present");
        return;
    };
    let mut app = app!();
    app.setup_admin().await;

    // Load the workbook's own rule table, with the one correction: Mafit is the gym,
    // so it belongs to Sport rather than the workbook's Haustier.
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
        Some(json!({"year":2026,"openingBalanceCents":4_000_000})),
    )
    .await;

    let (_, d) = app.send("GET", "/dashboard?year=2026", None).await;
    assert_eq!(d["incomeCents"], 3_600_000, "Einnahmen 36.000,00");
    assert_eq!(d["expenseCents"], 2_700_000, "Ausgaben 27.000,00");
    assert_eq!(d["balanceCents"], 900_000, "Bilanz 9.000,00");
    assert_eq!(
        d["closingBalanceCents"], 4_900_000,
        "Bilanz gesamt 49.000,00"
    );
    assert_eq!(d["taxRelevantCount"], 20);
    assert_eq!(d["uncategorizedCount"], 0);
    assert_eq!(d["monthsWithData"], 9);
    assert_eq!(d["averageExpensePerMonthCents"], 300_000);
    assert_eq!(d["fixedCostsPerMonthCents"], 90_000);

    // The three rows carrying a "Kategorie manuell" override must land in
    // Dienstreisen, not in the category their comment's rule would choose. Getting
    // this wrong moves 860,00 into Reisen & Urlaub and is invisible in the totals.
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
        net["Dienstreisen"], 5_500,
        "manuelle Zuordnung wurde angewandt"
    );
    assert_eq!(net["Reisen & Urlaub"], 240_000);
    assert_eq!(net["Miete"], 480_000);
    assert_eq!(net["Sport"], 46_000);
    assert_eq!(net["Sparen & Anlage"], 648_000);

    // June's variable costs are negative because of a reimbursement — the single
    // best regression test for netting surviving the whole stack.
    let (_, months) = app.send("GET", "/overview/months?year=2026", None).await;
    assert_eq!(months["months"][5]["variableCostsNetCents"], -30_000);
    assert_eq!(months["months"][8]["cumulativeCents"], 900_000);

    // The legacy sheet: month-only rows recovered from saldo markers, the three
    // blocks that disagree with their own marker recorded rather than adjusted.
    let legacy_id = app.upload_workbook("../konten.ods").await;
    let (_, preview) = app
        .send("GET", &format!("/imports/{legacy_id}"), None)
        .await;
    assert_eq!(preview["counts"]["dataRows"], 1404);
    assert_eq!(preview["counts"]["transfer"], 4, "nur die Kontoumbuchungen");
    assert_eq!(preview["markerTotalCents"], 4_000_000, "Marker = Vortrag");
    assert_eq!(preview["rowTotalCents"], 4_485_541);
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
