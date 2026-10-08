//! KitchenOwl integration tests, against the real router and a **mock KitchenOwl**.
//!
//! The live instance is the user's household and is shared with another person, so
//! it was probed read-only and no write was ever made against it. Every write path
//! here runs against the mock below, whose responses reproduce the shapes observed
//! live — including the ones that break naive clients: a `date`-descending page
//! order rather than id-descending, `category`/`category_id` absent together on a
//! third of the corpus, float amounts carrying IEEE-754 artifacts, integer split
//! weights, and a plain-text `Request invalid` error body served as `text/html`.
//!
//! **On the fixtures.** `tests/fixtures/kitchenowl/*.json` are ANONYMISED, not
//! recorded verbatim. The real corpus is a shared household: the expense names and
//! descriptions are another person's spending, which is their personal data and not
//! the user's to commit. The fixtures therefore keep every *structural* property
//! that matters — key presence and absence, ordering, float artifacts, weights, the
//! id gap left by an upstream deletion — with invented merchant names and amounts,
//! except for the handful of artifact values already written down in the project's
//! own design notes. The alternative, gitignoring them like the workbook fixtures,
//! would leave CI unable to exercise any of this.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Query, State},
    http::{Request, StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

use finanzen::{AppState, Config, db::Db};

mod common;

const ORIGIN: &str = "http://localhost:3100";

// --------------------------------------------------------- the mock instance

#[derive(Default)]
struct MockState {
    expenses: Vec<Value>,
    next_id: i64,
    /// POSTs answered with a 504 *after* creating the expense — the ambiguous
    /// timeout that reconcile-before-post exists for.
    swallow_posts: bool,
    /// Every endpoint refuses, as an instance that has gone down does.
    offline: bool,
    post_count: usize,
    /// The last body received, verbatim, so a test can assert the REQUEST spelling
    /// rather than only what the mock chose to echo back.
    last_post_body: Option<Value>,
    page_requests: Vec<Option<i64>>,
    /// How many single-expense replaces arrived, and the last body verbatim — the
    /// tagging queue's whole contract is "one field changes and nothing else does".
    update_count: usize,
    last_update_body: Option<Value>,
    /// Every replace is refused with the live instance's own `400 Request invalid`.
    refuse_updates: bool,
    /// Flips the sign of every member's `expense_balance`, so a test can put the
    /// household in debt to the user instead of the other way round. The settlement
    /// books a different KIND in that direction, which is the whole point.
    owed_to_me: bool,
}

#[derive(Clone)]
struct Mock(Arc<Mutex<MockState>>);

struct MockServer {
    url: String,
    state: Mock,
}

fn expense(
    id: i64,
    name: &str,
    amount: f64,
    date: i64,
    category: Option<(i64, &str)>,
    paid_by: i64,
    weights: &[(i64, i64)],
) -> Value {
    let mut v = json!({
        "id": id, "name": name, "description": "", "amount": amount, "date": date,
        "category_id": category.map(|c| c.0), "household_id": 1, "photo": null,
        "paid_by_id": paid_by,
        "paid_for": weights.iter().map(|(u, f)| json!({
            "user_id": u, "factor": f, "expense_id": id,
            "created_at": date, "updated_at": date
        })).collect::<Vec<_>>(),
        "exclude_from_statistics": false,
        "created_at": date, "updated_at": date,
    });
    // The nested object is present exactly when the id is — and absent, not null,
    // otherwise. 173 of 464 live expenses look like this.
    if let Some((cid, cname)) = category {
        v["category"] = json!({
            "id": cid, "name": cname, "color": null, "budget": null,
            "household_id": 1, "created_at": date, "updated_at": date
        });
    }
    v
}

/// `2026-05-03T12:00` Berlin and friends, as epoch milliseconds.
/// Epoch milliseconds for a date, which the monthly analysis needs in order to
/// spread expenses over a year rather than over one week in May.
fn on(year: i32, month: u32, day: u32) -> i64 {
    chrono::NaiveDate::from_ymd_opt(year, month, day)
        .expect("valid date")
        .and_hms_opt(12, 0, 0)
        .expect("valid time")
        .and_utc()
        .timestamp_millis()
}

fn ms(day: i64) -> i64 {
    // 2026-05-01T12:00:00+02:00
    1777629600000 + day * 86_400_000
}

fn offline() -> axum::response::Response {
    (StatusCode::SERVICE_UNAVAILABLE, "Service Unavailable").into_response()
}

async fn mock_household(State(mock): State<Mock>) -> axum::response::Response {
    let (offline_now, owed_to_me) = {
        let m = mock.0.lock().expect("mock lock");
        (m.offline, m.owed_to_me)
    };
    if offline_now {
        return offline();
    }
    let sign = if owed_to_me { -1.0 } else { 1.0 };
    Json(json!([{
        "id": 1, "name": "Beispielhaushalt", "expenses_feature": true,
        "member": [
            {"id": 1, "name": "Fabi", "username": "fabi",
             "expense_balance": sign * -149.16999999999217, "owner": true, "admin": false},
            {"id": 2, "name": "Ada", "username": "ada",
             "expense_balance": sign * 149.16999999999217, "owner": false, "admin": true},
        ]
    }]))
    .into_response()
}

async fn mock_user(State(mock): State<Mock>) -> axum::response::Response {
    if mock.0.lock().expect("mock lock").offline {
        return offline();
    }
    Json(json!({"id": 1, "name": "Fabi"})).into_response()
}

async fn mock_categories(State(mock): State<Mock>) -> axum::response::Response {
    if mock.0.lock().expect("mock lock").offline {
        return offline();
    }
    Json(json!(MOCK_CATEGORIES)).into_response()
}

/// The household's category list. Four of the live instance's seven, which is
/// enough for the tagging queue to resolve a precedent, an override and a refusal.
const MOCK_CATEGORIES: [(i64, &str); 4] = [
    (1, "Wocheneinkauf"),
    (2, "Essen gehen"),
    (3, "Haushalt"),
    (7, "Hobbies"),
];

/// A `GET` returns the category as a NESTED OBJECT beside `category_id`; a write
/// body carries it as a bare int. The mock resolves it the same way the instance
/// does, so a re-fetch after a write looks like a real re-fetch.
fn nested_category(id: Option<i64>) -> Value {
    match id.and_then(|id| MOCK_CATEGORIES.iter().find(|(cid, _)| *cid == id)) {
        Some((id, name)) => json!({
            "id": id, "name": name, "color": null, "budget": null, "household_id": 1,
        }),
        None => Value::Null,
    }
}

#[derive(serde::Deserialize)]
struct PageQuery {
    #[serde(rename = "startAfterId")]
    start_after_id: Option<i64>,
    /// Anything else is refused exactly as the live instance refuses it.
    #[serde(flatten)]
    rest: std::collections::BTreeMap<String, String>,
}

async fn mock_expenses(
    State(mock): State<Mock>,
    Query(q): Query<PageQuery>,
) -> axum::response::Response {
    if !q.rest.is_empty() {
        // Plain text, served as text/html, with a 400. Verified live for
        // ?page= / ?limit= / ?offset=.
        return (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            "Request invalid",
        )
            .into_response();
    }
    let mut state = mock.0.lock().expect("mock lock");
    if state.offline {
        return offline();
    }
    state.page_requests.push(q.start_after_id);
    // Date descending — NOT id descending. This is the ordering observed live.
    let mut ordered = state.expenses.clone();
    ordered.sort_by_key(|e| std::cmp::Reverse(e["date"].as_i64().unwrap_or(0)));
    let start = match q.start_after_id {
        None => 0,
        Some(cursor) => match ordered
            .iter()
            .position(|e| e["id"].as_i64() == Some(cursor))
        {
            Some(i) => i + 1,
            None => ordered.len(),
        },
    };
    let page: Vec<Value> = ordered.into_iter().skip(start).take(30).collect();
    Json(page).into_response()
}

async fn mock_create(
    State(mock): State<Mock>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let mut state = mock.0.lock().expect("mock lock");
    if state.offline {
        return offline();
    }
    state.post_count += 1;
    state.last_post_body = Some(body.clone());
    let id = state.next_id;
    state.next_id += 1;
    // The mock translates the REQUEST spelling into the RESPONSE spelling, because
    // that is what the real instance does and the asymmetry is the whole trap:
    //   request   paid_by {id} · paid_for [{id, factor}] · category  <int>
    //   response  paid_by_id   · paid_for [{user_id, …}] · category_id
    // Verified live. A mock that echoed the request verbatim would let a wrong
    // request shape pass every test and still fail against KitchenOwl.
    let created = json!({
        "id": id,
        "name": body["name"],
        "description": body["description"],
        "amount": body["amount"],
        "date": body["date"],
        "category_id": body["category"],
        "paid_by_id": body["paid_by"]["id"],
        "paid_for": body["paid_for"].as_array().map(|shares| {
            shares.iter().map(|s| json!({
                "user_id": s["id"], "factor": s["factor"], "expense_id": id,
            })).collect::<Vec<_>>()
        }).unwrap_or_default(),
        "exclude_from_statistics": false,
        "household_id": 1,
    });
    // The real create response omits `paid_for` entirely, which is why the push
    // re-fetches instead of trusting what it gets back. Keep the stored row full
    // and hand the caller the thinner object.
    let create_response = {
        let mut thin = created.clone();
        thin.as_object_mut().expect("object").remove("paid_for");
        thin
    };
    state.expenses.push(created.clone());
    let created = create_response;
    if state.swallow_posts {
        // The expense EXISTS but the caller never learns its id. Retrying blindly
        // is what would double-post.
        return (StatusCode::GATEWAY_TIMEOUT, "upstream timeout").into_response();
    }
    Json(created).into_response()
}

/// `GET /api/expense/{id}` — the single-expense read. Note the path: there is no
/// `/api/household/{hid}/expense/{id}` on the real instance, and asking for one
/// answers 404.
async fn mock_expense(
    State(mock): State<Mock>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> axum::response::Response {
    let state = mock.0.lock().expect("mock lock");
    if state.offline {
        return offline();
    }
    match state.expenses.iter().find(|e| e["id"].as_i64() == Some(id)) {
        Some(e) => Json(e.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, "Requested resource not found").into_response(),
    }
}

/// `POST /api/expense/{id}` — the single-expense REPLACE.
///
/// It replaces rather than patches, exactly as the real one does, so a body that
/// forgets a field loses it here too and the test notices. The request spelling is
/// translated to the response spelling on the way in, for the same reason
/// `mock_create` does it: a mock that echoed the request would let a wrong shape
/// pass every test and still fail against KitchenOwl.
async fn mock_update(
    State(mock): State<Mock>,
    axum::extract::Path(id): axum::extract::Path<i64>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let mut state = mock.0.lock().expect("mock lock");
    if state.offline {
        return offline();
    }
    if state.refuse_updates {
        // What the live instance answers for a body it dislikes: 400, plain text,
        // not JSON.
        return (StatusCode::BAD_REQUEST, "Request invalid").into_response();
    }
    let Some(index) = state
        .expenses
        .iter()
        .position(|e| e["id"].as_i64() == Some(id))
    else {
        return (StatusCode::NOT_FOUND, "Requested resource not found").into_response();
    };
    state.update_count += 1;
    state.last_update_body = Some(body.clone());

    let mut replaced = json!({
        "id": id,
        "name": body["name"],
        "description": body["description"],
        "amount": body["amount"],
        "date": body["date"],
        "category_id": body["category"],
        "paid_by_id": body["paid_by"]["id"],
        "paid_for": body["paid_for"].as_array().map(|shares| {
            shares.iter().map(|s| json!({
                "user_id": s["id"], "factor": s["factor"], "expense_id": id,
            })).collect::<Vec<_>>()
        }).unwrap_or_default(),
        "exclude_from_statistics": body["exclude_from_statistics"],
        "household_id": 1,
    });
    // A real read carries the nested object too, and the mirror takes the category
    // NAME from it. A mock that returned only the id would leave the mirror holding
    // a nameless category and make the caller look broken.
    let nested = nested_category(body["category"].as_i64());
    if !nested.is_null() {
        replaced["category"] = nested;
    }
    state.expenses[index] = replaced.clone();
    Json(replaced).into_response()
}

impl MockServer {
    async fn start() -> Self {
        let state = Mock(Arc::new(Mutex::new(MockState {
            next_id: 1000,
            ..Default::default()
        })));
        let app = Router::new()
            .route("/api/household", get(mock_household))
            .route("/api/user", get(mock_user))
            .route(
                "/api/household/{id}/expense",
                get(mock_expenses).post(mock_create),
            )
            .route(
                "/api/household/{id}/expense/categories",
                get(mock_categories),
            )
            .route("/api/expense/{id}", get(mock_expense).post(mock_update))
            .with_state(state.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock");
        let addr: SocketAddr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            url: format!("http://{addr}"),
            state,
        }
    }

    fn seed(&self, expenses: Vec<Value>) {
        self.inner().expenses = expenses;
    }

    /// How many single-expense replaces reached the instance.
    fn update_count(&self) -> usize {
        self.inner().update_count
    }

    /// The last replace body verbatim, so a test can assert the REQUEST spelling
    /// and that nothing but the category moved.
    fn last_update_body(&self) -> Option<Value> {
        self.inner().last_update_body.clone()
    }

    /// KitchenOwl refuses every category change from here on.
    fn refuse_updates(&self) {
        self.inner().refuse_updates = true;
    }

    /// The household owes the user, rather than the other way round.
    fn owe_the_user(&self) {
        self.inner().owed_to_me = true;
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, MockState> {
        self.state.0.lock().expect("mock lock")
    }
}

// ------------------------------------------------------------- the app under test

struct TestApp {
    router: Router,
    cookie: Option<String>,
    /// The per-test database, so a test can open a connection carrying NO tenant
    /// context — which is exactly the situation the background loops run in.
    db_url: String,
}

impl TestApp {
    async fn new(kitchenowl_url: Option<String>) -> Option<Self> {
        Self::build(kitchenowl_url, 40).await
    }

    /// `max_pages` is the KITCHENOWL_MAX_PULL_PAGES cap, which one test needs to be
    /// able to exhaust without seeding a thousand expenses.
    async fn build(kitchenowl_url: Option<String>, max_pages: u32) -> Option<Self> {
        let url = std::env::var("TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .ok()?;
        // Databases from earlier runs, dropped before another is made. An hour
        // is longer than any run, so nothing in use is ever a candidate.
        common::reap_stale(&admin, 900).await;

        let name = common::database_name("fin_ko");
        let role = common::role_name("fin_ko", &name);
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
        config.kitchenowl_url = kitchenowl_url;
        config.kitchenowl_token = Some("mock-token".into());
        config.kitchenowl_household_id = Some(1);
        // Short, so the reconcile-before-post branch is reachable without waiting.
        config.kitchenowl_http_timeout = std::time::Duration::from_secs(2);
        config.kitchenowl_max_pull_pages = max_pages;
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
        let mut app = Self {
            router: finanzen::router(state),
            cookie: None,
            db_url: target.to_string(),
        };
        app.setup_admin().await;
        Some(app)
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
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
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
    }

    async fn booking(&self, comment: &str, amount: i64, day: u32) -> String {
        let (status, body) = self
            .send(
                "POST",
                "/bookings",
                Some(json!({
                    "year": 2026, "month": 5, "bookedOn": format!("2026-05-{day:02}"),
                    "kind": "expense", "amountCents": amount, "comment": comment
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["id"].as_str().expect("booking id").to_string()
    }

    async fn sync(&self) -> (StatusCode, Value) {
        self.send("POST", "/kitchenowl/sync", None).await
    }

    /// A pool with no `app.user_id` set, like the background loops hold.
    async fn tenantless_pool(&self) -> sqlx::PgPool {
        PgPoolOptions::new()
            .max_connections(1)
            .connect(&self.db_url)
            .await
            .expect("tenantless pool")
    }
}

macro_rules! app {
    ($mock:expr) => {
        match TestApp::new($mock).await {
            Some(app) => app,
            None => return, // no TEST_DATABASE_URL: the suite skips, as elsewhere
        }
    };
}

// ------------------------------------------------------------------- the tests

#[tokio::test]
async fn a_pull_mirrors_the_household_and_writes_no_booking() {
    let mock = MockServer::start().await;
    mock.seed(vec![
        expense(
            1,
            "Supermarkt",
            19.07,
            ms(2),
            Some((1, "Wocheneinkauf")),
            2,
            &[(1, 1), (2, 1)],
        ),
        expense(2, "Kiosk", 7.90, ms(3), None, 1, &[(1, 1)]),
        expense(
            3,
            "Pizzeria",
            13.80,
            ms(4),
            Some((2, "Essen gehen")),
            1,
            &[(1, 1), (2, 1)],
        ),
    ]);
    let app = app!(Some(mock.url.clone()));

    let (status, body) = app.sync().await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["expenses"]["createdCount"], 3);

    let (status, page) = app.send("GET", "/kitchenowl/expenses", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 3);
    // Both figures, always: 19,07 + 7,90 + 13,80 full, and the user's slice of it.
    assert_eq!(page["sumAmountCents"], 4077);
    assert_eq!(page["sumOwnShareCents"], 954 + 790 + 690);

    // THE invariant: a pull writes nothing to the personal ledger.
    let (_, bookings) = app
        .send("GET", "/bookings?year=2026&status=all", None)
        .await;
    assert_eq!(bookings["total"], 0, "a pull must never write a booking");
    let (_, dashboard) = app.send("GET", "/dashboard?year=2026", None).await;
    assert_eq!(dashboard["expenseCents"], 0);
    assert_eq!(dashboard["bookingCount"], 0);
}

#[tokio::test]
async fn re_syncing_an_unchanged_expense_is_a_no_op() {
    let mock = MockServer::start().await;
    mock.seed(vec![expense(
        1,
        "Supermarkt",
        19.07,
        ms(2),
        Some((1, "Wocheneinkauf")),
        2,
        &[(1, 1), (2, 1)],
    )]);
    let app = app!(Some(mock.url.clone()));

    let (_, first) = app.sync().await;
    assert_eq!(first["expenses"]["createdCount"], 1);

    let (_, second) = app.sync().await;
    assert_eq!(second["expenses"]["createdCount"], 0, "no new mirror row");
    assert_eq!(
        second["expenses"]["updatedCount"], 0,
        "an unchanged expense must not touch a row — that is what remote_hash is for"
    );

    // And exactly one draft, not one per sync.
    let (_, drafts) = app
        .send("GET", "/kitchenowl/drafts?status=open", None)
        .await;
    assert_eq!(drafts["total"], 1);

    // A genuine change upstream, however, is picked up.
    {
        let mut state = mock.inner();
        state.expenses[0]["amount"] = json!(19.57);
    }
    let (_, third) = app.sync().await;
    assert_eq!(third["expenses"]["updatedCount"], 1);
    let (_, page) = app.send("GET", "/kitchenowl/expenses", None).await;
    assert_eq!(page["items"][0]["amountCents"], 1957);
    assert_eq!(page["total"], 1, "an update must not duplicate the row");
}

#[tokio::test]
async fn integer_weights_produce_shares_that_sum_to_the_total_exactly() {
    let mock = MockServer::start().await;
    mock.seed(vec![
        // An odd number of cents split evenly: somebody must get the extra cent.
        expense(1, "Imbiss", 13.81, ms(2), None, 1, &[(1, 1), (2, 1)]),
        // Weights above one, as observed live (12 : 7).
        expense(2, "Umzug", 19.00, ms(3), None, 1, &[(1, 12), (2, 7)]),
        // A three-way split of 10,00 — the canonical lost-cent case.
        expense(
            3,
            "Geschenk",
            10.00,
            ms(4),
            None,
            1,
            &[(1, 1), (2, 1), (3, 1)],
        ),
    ]);
    let app = app!(Some(mock.url.clone()));
    app.sync().await;

    let (_, page) = app.send("GET", "/kitchenowl/expenses", None).await;
    for item in page["items"].as_array().expect("items") {
        let total: i64 = item["amountCents"].as_i64().unwrap();
        let sum: i64 = item["paidFor"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["shareCents"].as_i64().unwrap())
            .sum();
        assert_eq!(sum, total, "shares must sum to the amount exactly: {item}");
    }
    let by_id = |ext: i64| {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["externalId"] == ext)
            .cloned()
            .unwrap()
    };
    assert_eq!(by_id(1)["ownShareCents"], 691, "13,81 / 2 rounds up for me");
    assert_eq!(by_id(2)["ownShareCents"], 1200, "12 of 19 weights on 19,00");
    assert_eq!(by_id(3)["ownShareCents"], 334);
}

#[tokio::test]
async fn a_full_amount_match_is_suggested_as_a_link_and_never_as_a_booking() {
    let mock = MockServer::start().await;
    mock.seed(vec![expense(
        1,
        "Wocheneinkauf",
        19.07,
        ms(2),
        Some((1, "Wocheneinkauf")),
        2,
        &[(1, 1), (2, 1)],
    )]);
    let app = app!(Some(mock.url.clone()));
    // The user records the FULL amount, which is the whole reason auto-booking
    // would double-post: 63 of 211 overlapping expenses look exactly like this.
    let booking_id = app.booking("Kaufland", 1907, 3).await;
    app.sync().await;

    let (status, drafts) = app.send("GET", "/kitchenowl/drafts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(drafts["likelyCount"], 1);
    let draft = &drafts["items"][0];
    assert_eq!(draft["status"], "likely_duplicate");
    assert_eq!(draft["suggestedAction"], "link", "never 'create'");
    assert_eq!(draft["candidates"][0]["bookingId"], booking_id);
    assert_eq!(draft["candidates"][0]["basis"], "fullAmount");

    // Confirming the link creates NO booking and moves no figure.
    let draft_id = draft["id"].as_str().unwrap();
    let (status, linked) = app
        .send(
            "POST",
            &format!("/kitchenowl/drafts/{draft_id}/link"),
            Some(json!({"bookingId": booking_id})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    assert_eq!(linked["status"], "confirmed");

    let (_, bookings) = app
        .send("GET", "/bookings?year=2026&status=all", None)
        .await;
    assert_eq!(
        bookings["total"], 1,
        "linking must not create a second booking"
    );
    assert_eq!(bookings["sumExpenseCents"], 1907);
    assert_eq!(bookings["items"][0]["externalSource"], "kitchenowl");
    assert_eq!(bookings["items"][0]["externalId"], "1");

    // A later pull of the same expense is a structural no-op.
    let (_, again) = app.sync().await;
    assert_eq!(again["expenses"]["createdCount"], 0);
    assert_eq!(again["expenses"]["updatedCount"], 0);

    // And the link is reversible, in both directions, without touching a figure.
    let (_, page) = app.send("GET", "/kitchenowl/expenses", None).await;
    let ko_id = page["items"][0]["id"].as_str().unwrap();
    let (status, _) = app
        .send(
            "DELETE",
            &format!("/kitchenowl/expenses/{ko_id}/link"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, bookings) = app
        .send("GET", "/bookings?year=2026&status=all", None)
        .await;
    assert_eq!(bookings["total"], 1);
    assert_eq!(bookings["sumExpenseCents"], 1907);
    assert_eq!(bookings["items"][0]["externalSource"], Value::Null);
}

#[tokio::test]
async fn an_own_share_match_alone_is_not_pre_selected() {
    let mock = MockServer::start().await;
    mock.seed(vec![expense(
        1,
        "Wocheneinkauf",
        19.08,
        ms(2),
        None,
        2,
        &[(1, 1), (2, 1)],
    )]);
    let app = app!(Some(mock.url.clone()));
    // Half of 19,08, recorded under a completely unrelated comment. Plausible, and
    // exactly the kind of coincidence that must not attach itself silently.
    app.booking("Zug Berlin", 954, 3).await;
    app.sync().await;

    let (_, drafts) = app.send("GET", "/kitchenowl/drafts", None).await;
    assert_eq!(drafts["likelyCount"], 0);
    let draft = &drafts["items"][0];
    assert_eq!(draft["suggestedAction"], "none");
    assert_eq!(draft["status"], "possible_duplicate");
    assert_eq!(draft["candidates"][0]["basis"], "ownShare");
}

#[tokio::test]
async fn the_cursor_follows_date_order_and_pages_the_whole_household() {
    let mock = MockServer::start().await;
    let mut seeded: Vec<Value> = (0..70)
        .map(|i| {
            expense(
                100 + i,
                "Supermarkt",
                10.0 + i as f64,
                ms(i),
                None,
                1,
                &[(1, 1), (2, 1)],
            )
        })
        .collect();
    // The back-dated expense: the highest id, dated oldest. An id high-water-mark
    // would never see it, and a min(id) cursor would stall on it.
    seeded.push(expense(
        999,
        "Nachgetragen",
        5.0,
        ms(-5),
        None,
        1,
        &[(1, 1)],
    ));
    mock.seed(seeded);
    let app = app!(Some(mock.url.clone()));

    let (status, body) = app.sync().await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["expenses"]["createdCount"], 71);
    assert_eq!(body["expenses"]["status"], "success");

    let (_, page) = app
        .send("GET", "/kitchenowl/expenses?pageSize=200", None)
        .await;
    assert_eq!(page["total"], 71);
    assert!(
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["externalId"] == 999),
        "the back-dated expense must be mirrored"
    );
    // Two full pages of 30, an eleven-row page, and one more request that comes
    // back empty. The short page is deliberately NOT treated as the end: the page
    // size is not part of KitchenOwl's contract, and guessing it would silently
    // truncate the mirror the day it changes. One extra request per scan is the
    // price. No cursor repeats, which is what would make the scan crawl or spin.
    let requests = mock.inner().page_requests.clone();
    assert_eq!(requests.len(), 4, "{requests:?}");
    assert_eq!(requests[0], None);
    let mut seen = requests.clone();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), requests.len(), "a cursor must never repeat");
}

#[tokio::test]
async fn an_expense_deleted_upstream_is_archived_and_keeps_its_link() {
    let mock = MockServer::start().await;
    mock.seed(vec![
        expense(1, "Supermarkt", 19.07, ms(2), None, 2, &[(1, 1), (2, 1)]),
        expense(2, "Kiosk", 7.90, ms(3), None, 1, &[(1, 1)]),
    ]);
    let app = app!(Some(mock.url.clone()));
    app.sync().await;

    mock.inner().expenses.retain(|e| e["id"] != json!(2));
    let (_, body) = app.sync().await;
    assert_eq!(body["expenses"]["archivedCount"], 1);

    let (_, page) = app.send("GET", "/kitchenowl/expenses", None).await;
    assert_eq!(
        page["total"], 1,
        "an archived expense leaves the default view"
    );
    let (_, all) = app
        .send("GET", "/kitchenowl/expenses?includeArchived=true", None)
        .await;
    assert_eq!(all["total"], 2, "but it is not destroyed");
}

#[tokio::test]
async fn a_push_is_accepted_while_kitchenowl_is_down_and_retried_later() {
    let mock = MockServer::start().await;
    let app = app!(Some(mock.url.clone()));
    // Metadata cached while the instance was up, which is the realistic outage:
    // the dialogue still knows who is in the household.
    app.sync().await;
    mock.inner().offline = true;

    let booking_id = app.booking("Kaufland", 1907, 3).await;

    let (status, intent) = app
        .send("POST", &format!("/bookings/{booking_id}/kitchenowl"), None)
        .await;
    // 202 even though the far end is unreachable: the intent is durable, the HTTP
    // call is opportunistic.
    assert_eq!(status, StatusCode::ACCEPTED, "{intent}");
    assert_eq!(intent["state"], "queued");
    assert_eq!(
        intent["amountCents"], 1907,
        "the booking's amount is untouched"
    );
    assert!(intent["marker"].as_str().unwrap().starts_with("#fin:"));

    // Queueing twice replaces the intent rather than creating a second one.
    let (status, _) = app
        .send("POST", &format!("/bookings/{booking_id}/kitchenowl"), None)
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (_, list) = app.send("GET", "/kitchenowl/push", None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);

    // And the rest of the app is entirely unaffected by the outage.
    let (status, dashboard) = app.send("GET", "/dashboard?year=2026", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(dashboard["expenseCents"], 1907);
    let (status, _) = app.send("GET", "/bookings?year=2026", None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, summary) = app.send("GET", "/kitchenowl/summary", None).await;
    assert_eq!(status, StatusCode::OK, "the widget must not block on HTTP");
    assert_eq!(summary["configured"], true);
    // A manual sync surfaces the outage as a 502 rather than pretending to succeed.
    let (status, _) = app.sync().await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    let (status, st) = app.send("GET", "/kitchenowl/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(st["reachable"], false);
    assert!(
        st["lastExpenseRun"]["error"].is_string(),
        "the failure is visible"
    );
}

#[tokio::test]
async fn a_retry_after_a_timeout_adopts_the_existing_expense_instead_of_double_posting() {
    let mock = MockServer::start().await;
    let app = app!(Some(mock.url.clone()));
    app.sync().await; // populate members and categories for the dialogue

    let booking_id = app.booking("Kaufland", 1907, 3).await;
    mock.inner().swallow_posts = true;

    let (status, _) = app
        .send(
            "POST",
            &format!("/bookings/{booking_id}/kitchenowl"),
            Some(json!({"koCategoryId": 1, "paidById": 1,
                        "paidFor": [{"memberId": 1, "factor": 1}, {"memberId": 2, "factor": 1}]})),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    // The spawned attempt posts, the expense is created, the answer is lost.
    wait_for(&app, |v| {
        v["state"] == "failed" || v["state"] == "abandoned"
    })
    .await;
    assert_eq!(mock.inner().post_count, 1);
    assert_eq!(
        mock.inner().expenses.len(),
        1,
        "the expense exists upstream"
    );

    // The far end recovers. The retry must NOT post again.
    mock.inner().swallow_posts = false;
    let (status, _) = app
        .send(
            "POST",
            &format!("/kitchenowl/push/{booking_id}/retry"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let intent = wait_for(&app, |v| v["state"] == "pushed").await;

    assert_eq!(
        mock.inner().post_count,
        1,
        "reconcile-before-post must adopt the existing expense, not create a second"
    );
    assert_eq!(mock.inner().expenses.len(), 1);
    assert_eq!(intent["externalId"], 1000);

    // And the booking now carries the link.
    let (_, booking) = app
        .send("GET", &format!("/bookings/{booking_id}"), None)
        .await;
    assert_eq!(booking["externalSource"], "kitchenowl");
    assert_eq!(booking["externalId"], "1000");
}

#[tokio::test]
async fn a_reconcile_scan_that_runs_out_of_pages_refuses_to_post_again() {
    // The scan walks by date, so an old push sits deep in the list. If the page cap
    // is reached before the target date, the question "did the last attempt land?"
    // is unanswered — and posting anyway is exactly the duplicate this mechanism
    // exists to prevent. It must fail loudly instead.
    let mock = MockServer::start().await;
    let Some(app) = TestApp::build(Some(mock.url.clone()), 2).await else {
        return;
    };
    app.sync().await;

    // More recent expenses than the two-page cap can walk, all newer than the
    // booking being pushed, so the scan gives up before reaching its date.
    let filler: Vec<Value> = (0..100)
        .map(|i| {
            expense(
                i + 1,
                "Supermarkt",
                10.0,
                ms(30 + i),
                None,
                1,
                &[(1, 1), (2, 1)],
            )
        })
        .collect();
    mock.seed(filler);

    let booking_id = app.booking("Kaufland", 1907, 3).await;
    mock.inner().swallow_posts = true;
    app.send("POST", &format!("/bookings/{booking_id}/kitchenowl"), None)
        .await;
    wait_for(&app, |v| {
        v["state"] == "failed" || v["state"] == "abandoned"
    })
    .await;
    let after_first = mock.inner().post_count;
    assert_eq!(after_first, 1);

    mock.inner().swallow_posts = false;
    app.send(
        "POST",
        &format!("/kitchenowl/push/{booking_id}/retry"),
        None,
    )
    .await;
    let intent = wait_for(&app, |v| {
        v["state"] == "failed" || v["state"] == "abandoned"
    })
    .await;

    assert_eq!(
        mock.inner().post_count,
        after_first,
        "an unanswered reconcile must not fall through to a second post"
    );
    assert!(
        intent["lastError"]
            .as_str()
            .unwrap_or_default()
            .contains("Abgleich vor dem Senden"),
        "the reason must be on the row: {intent}"
    );
}

#[tokio::test]
async fn a_push_carries_the_full_amount_the_chosen_split_and_the_kitchenowl_category() {
    let mock = MockServer::start().await;
    let app = app!(Some(mock.url.clone()));
    app.sync().await;
    let booking_id = app.booking("Umzugskisten", 1900, 3).await;

    let (status, _) = app
        .send(
            "POST",
            &format!("/bookings/{booking_id}/kitchenowl"),
            Some(json!({"name": "Umzug", "koCategoryId": 2, "paidById": 1,
                        "paidFor": [{"memberId": 1, "factor": 12},
                                    {"memberId": 2, "factor": 7}]})),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    wait_for(&app, |v| v["state"] == "pushed").await;

    let posted = mock.inner().expenses[0].clone();
    assert_eq!(posted["name"], "Umzug");
    assert_eq!(
        posted["amount"],
        json!(19.0),
        "the full amount, not the share"
    );
    assert_eq!(posted["category_id"], 2);
    assert_eq!(posted["paid_by_id"], 1);
    assert_eq!(posted["paid_for"][0]["factor"], 12);
    assert_eq!(posted["paid_for"][1]["factor"], 7);

    // And the body as it went over the wire, in the request spelling the live
    // instance actually accepts — the first attempt at this used the response
    // spelling and was rejected with a plain-text 400.
    let sent = mock
        .inner()
        .last_post_body
        .clone()
        .expect("a body was posted");
    assert_eq!(sent["category"], 2, "bare int, not category_id");
    assert_eq!(sent["paid_by"]["id"], 1, "object keyed id, not paid_by_id");
    assert_eq!(sent["paid_for"][0]["id"], 1, "id, not user_id");
    assert_eq!(sent["paid_for"][0]["factor"], 12);
    assert!(
        posted["description"].as_str().unwrap().starts_with("#fin:"),
        "the marker is what makes a retry safe: {posted}"
    );
}

#[tokio::test]
async fn a_transfer_and_an_already_linked_booking_are_refused() {
    let mock = MockServer::start().await;
    let app = app!(Some(mock.url.clone()));
    app.sync().await;

    let (_, transfer) = app
        .send(
            "POST",
            "/bookings",
            Some(json!({"year": 2026, "month": 5, "bookedOn": "2026-05-03",
                        "kind": "transfer", "amountCents": 5000, "comment": "to ING"})),
        )
        .await;
    let transfer_id = transfer["id"].as_str().unwrap();
    let (status, body) = app
        .send("POST", &format!("/bookings/{transfer_id}/kitchenowl"), None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let booking_id = app.booking("Kaufland", 1907, 3).await;
    app.send("POST", &format!("/bookings/{booking_id}/kitchenowl"), None)
        .await;
    wait_for(&app, |v| v["state"] == "pushed").await;
    let (status, _) = app
        .send("POST", &format!("/bookings/{booking_id}/kitchenowl"), None)
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a booking already in KitchenOwl must not be pushed twice"
    );
}

#[tokio::test]
async fn metadata_is_served_stale_rather_than_withheld() {
    let mock = MockServer::start().await;
    let app = app!(Some(mock.url.clone()));

    // Before any sync: no members, but the endpoint answers and says why.
    let (status, empty) = app.send("GET", "/kitchenowl/metadata", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty["stale"], true);
    assert!(empty["warning"].is_string());

    app.sync().await;
    let (status, fresh) = app.send("GET", "/kitchenowl/metadata", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fresh["stale"], false);
    assert_eq!(fresh["warning"], Value::Null);
    assert_eq!(fresh["members"].as_array().unwrap().len(), 2);
    assert_eq!(
        fresh["categories"].as_array().unwrap().len(),
        MOCK_CATEGORIES.len()
    );
    let me: Vec<&Value> = fresh["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["isMe"] == true)
        .collect();
    assert_eq!(me.len(), 1);
    assert_eq!(
        me[0]["balanceCents"], -14917,
        "the float artifact rounds exactly"
    );
}

#[tokio::test]
async fn the_two_ledgers_are_never_summed() {
    // The widget reports the CURRENT month, so both ledgers are dated there.
    let today = chrono::Utc::now().date_naive();
    let now_ms = today
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();
    let mock = MockServer::start().await;
    mock.seed(vec![expense(
        1,
        "Supermarkt",
        19.07,
        now_ms,
        None,
        2,
        &[(1, 1), (2, 1)],
    )]);
    let app = app!(Some(mock.url.clone()));
    let (status, created) = app
        .send(
            "POST",
            "/bookings",
            Some(json!({
                "year": today.format("%Y").to_string().parse::<i32>().unwrap(),
                "month": today.format("%m").to_string().parse::<u8>().unwrap(),
                "bookedOn": today.to_string(),
                "kind": "expense", "amountCents": 1907, "comment": "Kaufland"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    app.sync().await;

    // The personal ledger reports its own figure and only its own.
    let year = today.format("%Y").to_string();
    let (_, dashboard) = app
        .send("GET", &format!("/dashboard?year={year}"), None)
        .await;
    assert_eq!(dashboard["expenseCents"], 1907);
    assert_eq!(dashboard["bookingCount"], 1);

    // The KitchenOwl summary reports its own, labelled, and never adds the two.
    let (_, summary) = app.send("GET", "/kitchenowl/summary", None).await;
    assert_eq!(summary["monthAmountCents"], 1907);
    assert_eq!(summary["monthOwnShareCents"], 954);
    assert_eq!(summary["monthCount"], 1);
    // No field anywhere carries 1907 + 1907 or 1907 + 954.
    let text = summary.to_string();
    assert!(!text.contains("3814") && !text.contains("2861"), "{text}");
}

#[tokio::test]
async fn kitchenowl_is_tenant_scoped_like_everything_else() {
    let mock = MockServer::start().await;
    mock.seed(vec![expense(
        1,
        "Supermarkt",
        19.07,
        ms(2),
        None,
        2,
        &[(1, 1), (2, 1)],
    )]);
    let app = app!(Some(mock.url.clone()));
    app.sync().await;

    let (status, _) = app
        .send(
            "POST",
            "/admin/users",
            Some(json!({"username": "ada", "displayName": "Ada",
                        "password": "ein-anderes-langes-passwort"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"username": "ada", "password": "ein-anderes-langes-passwort"}).to_string(),
        ))
        .unwrap();
    let response = app.router.clone().oneshot(req).await.expect("login");
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(str::to_string)
        .expect("cookie");

    let other = TestApp {
        router: app.router.clone(),
        cookie: Some(cookie),
        db_url: app.db_url.clone(),
    };
    let (status, page) = other.send("GET", "/kitchenowl/expenses", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        page["total"], 0,
        "the mirror is one account's, not the server's"
    );
    let (_, st) = other.send("GET", "/kitchenowl/status", None).await;
    assert_eq!(st["enabled"], false, "opting in is per account");
    assert_eq!(st["configured"], true);
}

/// Polls the push intent until `predicate` holds. The attempt is spawned, so the
/// alternative is a sleep long enough to be flaky in CI and slow everywhere else.
async fn wait_for(app: &TestApp, predicate: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..100 {
        let (_, list) = app.send("GET", "/kitchenowl/push", None).await;
        if let Some(first) = list.as_array().and_then(|a| a.first())
            && predicate(first)
        {
            return first.clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let (_, list) = app.send("GET", "/kitchenowl/push", None).await;
    panic!("push intent never reached the expected state: {list}");
}

/// Re-verifies the fixture shapes against the real instance. **GET only.**
///
/// Ignored by default and skipped unless `KITCHENOWL_URL` and `KITCHENOWL_TOKEN` are
/// set, exactly like the workbook re-parse test: CI must never depend on somebody's
/// household being reachable, and nothing in this function may ever issue a POST,
/// PUT or DELETE. Run it by hand after a KitchenOwl upgrade:
///
/// ```text
/// set -a; . ../.env; set +a; cargo test --test kitchenowl -- --ignored --nocapture
/// ```
#[tokio::test]
#[ignore = "hits the live KitchenOwl instance; read-only, run by hand"]
async fn the_live_instance_still_has_the_shapes_the_fixtures_claim() {
    let (Ok(url), Ok(token)) = (
        std::env::var("KITCHENOWL_URL"),
        std::env::var("KITCHENOWL_TOKEN"),
    ) else {
        eprintln!("KITCHENOWL_URL/KITCHENOWL_TOKEN not set — skipping");
        return;
    };
    let url = url.trim_end_matches('/').to_string();
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("client");
    let get = |path: String| {
        let http = http.clone();
        let token = token.clone();
        let url = url.clone();
        async move {
            let response = http
                .get(format!("{url}{path}"))
                .bearer_auth(token)
                .send()
                .await
                .expect("request");
            assert!(
                response.status().is_success(),
                "{path}: {}",
                response.status()
            );
            response.bytes().await.expect("body").to_vec()
        }
    };

    let households =
        finanzen::kitchenowl::wire::parse_households(&get("/api/household".into()).await)
            .expect("households");
    let household = households.first().expect("at least one household");
    assert!(household.expenses_feature, "expenses must be enabled");
    assert!(
        household.member.iter().all(|m| m.expense_balance.is_some()),
        "expense_balance is the only balance source there is"
    );

    let me = finanzen::kitchenowl::wire::parse_user(&get("/api/user".into()).await)
        .expect("user")
        .id;
    assert!(
        household.member.iter().any(|m| m.id == me),
        "the token's own user must be a member, since that is how `is_me` is decided"
    );

    let page = finanzen::kitchenowl::wire::parse_expenses(
        &get(format!("/api/household/{}/expense", household.id)).await,
    )
    .expect("expenses");
    assert!(!page.is_empty());
    let dates: Vec<i64> = page.iter().map(|e| e.date).collect();
    let mut sorted = dates.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(dates, sorted, "pages are ordered by DATE descending");
    for raw in &page {
        let mirror = finanzen::kitchenowl::wire::to_mirror(raw, me).expect("mirrors");
        assert_eq!(
            mirror.shares.iter().map(|s| s.share_cents).sum::<i64>(),
            mirror.amount_cents
        );
    }

    let categories = finanzen::kitchenowl::wire::parse_categories(
        &get(format!(
            "/api/household/{}/expense/categories",
            household.id
        ))
        .await,
    )
    .expect("categories");
    assert!(!categories.is_empty());

    // The error body is plain text served as text/html, not JSON. Everything here
    // must survive that; a GET with a rejected parameter is the cheapest way to see
    // it, and it changes nothing on the instance.
    let response = http
        .get(format!(
            "{url}/api/household/{}/expense?limit=5",
            household.id
        ))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 400);
    let body = response.text().await.expect("body");
    assert!(
        serde_json::from_str::<serde_json::Value>(&body).is_err(),
        "the error body is not JSON: {body:?}"
    );
}

// ------------------------------------------------------- the household's analysis

/// The same questions the personal analysis answers, asked of the mirror.
///
/// Every figure is a pair — what the household spent and the user's share — and
/// the two are never added. That is the single most likely mistake in this
/// integration, so it is asserted on every level of the response.
#[tokio::test]
async fn the_household_ledger_analyses_its_own_year() {
    let mock = MockServer::start().await;
    let mut excluded = expense(
        9,
        "Korrektur",
        99.00,
        on(2026, 2, 2),
        Some((1, "Wocheneinkauf")),
        1,
        &[(1, 1), (2, 1)],
    );
    // KitchenOwl's own statistics skip these, so ours must too.
    excluded["exclude_from_statistics"] = json!(true);

    mock.seed(vec![
        expense(
            1,
            "Kaufland",
            20.00,
            on(2026, 1, 10),
            Some((1, "Wocheneinkauf")),
            1,
            &[(1, 1), (2, 1)],
        ),
        expense(
            2,
            "Kaufland",
            30.00,
            on(2026, 3, 5),
            Some((1, "Wocheneinkauf")),
            2,
            &[(1, 1), (2, 1)],
        ),
        expense(
            3,
            "Kino",
            22.50,
            on(2026, 3, 20),
            Some((2, "Ausflug")),
            1,
            &[(1, 1), (2, 1)],
        ),
        // No category at all — a third of the live corpus looks like this.
        expense(4, "Kiosk", 10.00, on(2026, 4, 1), None, 1, &[(1, 1)]),
        excluded,
    ]);
    let app = app!(Some(mock.url.clone()));
    let (status, _) = app.sync().await;
    assert_eq!(status, StatusCode::OK);

    let (status, a) = app
        .send("GET", "/kitchenowl/analysis/categories?year=2026", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{a}");

    // 20 + 30 + 22,50 + 10 — and NOT the 99,00 KitchenOwl excludes.
    assert_eq!(a["totalAmountCents"], 8250);
    assert_eq!(a["excludedCount"], 1);
    // The user's slice: half of the three shared ones, all of the solo one.
    assert_eq!(a["totalOwnShareCents"], 1000 + 1500 + 1125 + 1000);
    assert_eq!(a["expenseCount"], 4);
    // Januar, März, April — never twelve, and never the months of the excluded row.
    assert_eq!(a["monthsWithData"], 3);
    assert_eq!(a["uncategorizedCount"], 1);
    assert_eq!(a["years"][0], 2026);

    let rows = a["rows"].as_array().expect("rows");
    let top = &rows[0];
    assert_eq!(top["koCategoryName"], "Wocheneinkauf");
    assert_eq!(top["amountCents"], 5000);
    assert_eq!(top["ownShareCents"], 2500);
    assert_eq!(top["expenseCount"], 2);
    // Twelve slots, the two months that carry something and no others.
    assert_eq!(top["monthlyAmountCents"][0], 2000);
    assert_eq!(
        top["monthlyAmountCents"][1], 0,
        "the excluded Februar row must not appear"
    );
    assert_eq!(top["monthlyAmountCents"][2], 3000);
    assert_eq!(top["monthlyOwnShareCents"][2], 1500);
    // Averaged over the months the household was active, not over twelve.
    assert_eq!(top["averagePerMonthCents"], 1667);

    // Who paid is a question only a shared ledger has.
    let payers = a["paidBy"].as_array().expect("paidBy");
    let fabi = payers.iter().find(|p| p["name"] == "Fabi").expect("Fabi");
    assert_eq!(fabi["amountCents"], 2000 + 2250 + 1000);
    let ada = payers.iter().find(|p| p["name"] == "Ada").expect("Ada");
    assert_eq!(ada["amountCents"], 3000);

    // One recurring purchase across the year, the "how much Kaufland" question.
    let (status, s) = app
        .send(
            "GET",
            "/kitchenowl/analysis/series?year=2026&name=kaufland",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{s}");
    assert_eq!(s["subject"], "kaufland");
    assert_eq!(s["amountCents"], 5000);
    assert_eq!(s["ownShareCents"], 2500);
    assert_eq!(s["monthsWithData"], 2);
    assert_eq!(s["averagePerActiveMonthCents"], 2500);
    assert_eq!(s["averageOwnSharePerActiveMonthCents"], 1250);
    assert_eq!(s["months"].as_array().expect("months").len(), 12);
    assert_eq!(s["months"][0]["amountCents"], 2000);
    assert_eq!(s["months"][2]["amountCents"], 3000);

    // The uncategorised third of the corpus is reachable, not a dead end.
    let (status, u) = app
        .send(
            "GET",
            "/kitchenowl/analysis/series?year=2026&uncategorized=true",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{u}");
    assert_eq!(u["amountCents"], 1000);

    // ...and asking for two subjects at once is a 400, not a silent choice.
    let (status, _) = app
        .send(
            "GET",
            "/kitchenowl/analysis/series?year=2026&name=kaufland&uncategorized=true",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, subjects) = app
        .send(
            "GET",
            "/kitchenowl/analysis/series/subjects?year=2026",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    // Only names seen more than once are worth a chart.
    assert_eq!(subjects.as_array().expect("subjects").len(), 1);
    assert_eq!(subjects[0]["name"], "Kaufland");
    assert_eq!(subjects[0]["expenseCount"], 2);

    // THE invariant, again: analysing the mirror writes nothing to the ledger.
    let (_, bookings) = app
        .send("GET", "/bookings?year=2026&status=all", None)
        .await;
    assert_eq!(bookings["total"], 0);
}

/// The background sync ran for nobody.
///
/// `ko_sync_state` carries forced row-level security, and the loops hold no request
/// and therefore no `app.user_id`. A tenant-scoped read in that state returns an
/// EMPTY LIST rather than an error, so every automatic pull iterated zero users and
/// reported nothing, indefinitely — while the manual button, which runs inside a
/// request, worked perfectly. This asserts both halves of that: the list the loop
/// reads is populated, and the table it used to read is genuinely invisible from
/// there, so the exemption is doing real work.
#[tokio::test]
async fn the_sync_loop_can_see_who_opted_in() {
    let mock = MockServer::start().await;
    mock.seed(vec![expense(
        1,
        "Supermarkt",
        19.07,
        ms(2),
        Some((1, "Wocheneinkauf")),
        2,
        &[(1, 1), (2, 1)],
    )]);
    let app = app!(Some(mock.url.clone()));
    let (status, _) = app.sync().await;
    assert_eq!(status, StatusCode::OK);

    let pool = app.tenantless_pool().await;
    let users = finanzen::kitchenowl::mirror::participating_users(&pool)
        .await
        .expect("participants");
    assert_eq!(
        users.len(),
        1,
        "the loops iterate this list; empty means every automatic sync quietly does nothing"
    );

    let visible: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM ko_sync_state")
        .fetch_one(&pool)
        .await
        .expect("count sync state");
    assert_eq!(
        visible, 0,
        "ko_sync_state must stay invisible without a tenant — that is why the registry exists"
    );
}

// --------------------------------------------------------------- settling up

/// Settling up is the ONE path where a KitchenOwl figure causes a personal
/// booking, so it is the one that has to be hardest to get wrong.
///
/// Four things, each with a plausible wrong answer: the balance keeps KitchenOwl's
/// sign (negative is the user owing), the booking is an EXPENSE that moves the
/// balance by its full amount, pressing twice books once, and nothing at all is sent
/// to KitchenOwl — a settlement is recorded locally and appears over there only as
/// the balance changing.
///
/// The kind is the one this module got wrong at first. A `transfer` has `net_cents`
/// 0 by generated column, so it moves neither the balance nor any category — right
/// for money between the user's own accounts, wrong for money handed to a flatmate,
/// which is gone. The user's own ledger settles it: thirteen `Ausgleich` bookings
/// over three years, nine expenses and four income, no transfers.
#[tokio::test]
async fn settling_up_books_one_expense_and_writes_nothing_to_kitchenowl() {
    let mock = MockServer::start().await;
    mock.seed(vec![expense(
        1,
        "Supermarkt",
        19.07,
        ms(2),
        Some((1, "Wocheneinkauf")),
        2,
        &[(1, 1), (2, 1)],
    )]);
    let app = app!(Some(mock.url.clone()));
    let (status, _) = app.sync().await;
    assert_eq!(status, StatusCode::OK);

    let posts_before = mock.inner().post_count;

    let (status, view) = app.send("GET", "/kitchenowl/settlement", None).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    // The mock household reports -149,16999… for the user. Rounded at the adapter
    // boundary, and passed through with its sign intact.
    assert_eq!(view["balanceCents"], -14917);
    assert_eq!(view["direction"], "i_owe", "negative means the user owes");
    assert_eq!(view["amountCents"], 14917, "what changes hands is positive");
    assert_eq!(view["alreadySettled"], false);
    // Stated before the button is pressed, because it changes the year's figures.
    assert_eq!(
        view["kind"], "expense",
        "owing the household is money about to leave: {view}"
    );
    assert!(
        view["suggestedComment"]
            .as_str()
            .expect("comment")
            .starts_with("Ausgleich "),
        "the spreadsheet's spelling: {view}"
    );

    // The period comes from the response rather than from the wall clock, so this
    // test does not start failing on 1 January.
    let year = view["period"]["year"].as_i64().expect("year");

    let (status, first) = app.send("POST", "/kitchenowl/settlement", None).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert_eq!(first["booking"]["kind"], "expense");
    assert_eq!(first["booking"]["amountCents"], 14917);
    // Structural, not conventional: `net_cents` is a generated column, and for an
    // expense it carries the full amount. A transfer would sit here at 0 and the
    // money would have left the account without the ledger noticing.
    assert_eq!(first["booking"]["netCents"], 14917);
    assert_eq!(first["alreadySettled"], true);
    assert_eq!(
        first["settledBalanceCents"], -14917,
        "the balance it was based on is kept, because the live one moves on"
    );

    // Pressing the button twice is a double tap, not an error.
    let (status, second) = app.send("POST", "/kitchenowl/settlement", None).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["booking"]["id"], first["booking"]["id"]);

    let (_, page) = app
        .send("GET", &format!("/bookings?year={year}&status=all"), None)
        .await;
    assert_eq!(page["total"], 1, "one settlement, not two: {page}");

    // The whole point of the correction: the money is gone, so the year says so.
    let (_, analysis) = app
        .send("GET", &format!("/analysis/categories?year={year}"), None)
        .await;
    assert_eq!(
        analysis["totalNetCents"], 14917,
        "the settlement is a cost of the year, not an invisible movement: {analysis}"
    );
    assert_eq!(
        analysis["excludedTransferCount"], 0,
        "nothing here is a transfer any more"
    );

    let (_, dashboard) = app
        .send("GET", &format!("/dashboard?year={year}"), None)
        .await;
    assert_eq!(
        dashboard["balanceCents"], -14917,
        "paying the household lowers the balance by exactly what was paid: {dashboard}"
    );

    // THE invariant: the live instance is untouched.
    assert_eq!(
        mock.inner().post_count,
        posts_before,
        "a settlement is recorded locally; KitchenOwl learns of it as a balance"
    );
}

/// The other direction, which is not a mirror image: it books the opposite KIND.
///
/// When the household owes the user, the settlement is money ARRIVING, so it is
/// income and the balance goes up. Under the old transfer modelling both directions
/// produced the same invisible zero, which is how a bug like this hides.
#[tokio::test]
async fn being_owed_books_income_and_raises_the_balance() {
    let mock = MockServer::start().await;
    mock.owe_the_user();
    mock.seed(vec![expense(
        1,
        "Supermarkt",
        19.07,
        ms(2),
        Some((1, "Wocheneinkauf")),
        1,
        &[(1, 1), (2, 1)],
    )]);
    let app = app!(Some(mock.url.clone()));
    let (status, _) = app.sync().await;
    assert_eq!(status, StatusCode::OK);

    let (_, view) = app.send("GET", "/kitchenowl/settlement", None).await;
    assert_eq!(view["balanceCents"], 14917, "positive: the household owes");
    assert_eq!(view["direction"], "household_owes_me");
    assert_eq!(view["kind"], "income", "money about to arrive: {view}");

    let year = view["period"]["year"].as_i64().expect("year");
    let (status, booked) = app.send("POST", "/kitchenowl/settlement", None).await;
    assert_eq!(status, StatusCode::CREATED, "{booked}");
    assert_eq!(booked["booking"]["kind"], "income");
    // Income is stored negative by the netting rule, which is what makes the
    // balance — `-sum(net)` — go UP by the amount received.
    assert_eq!(booked["booking"]["netCents"], -14917);

    let (_, dashboard) = app
        .send("GET", &format!("/dashboard?year={year}"), None)
        .await;
    assert_eq!(
        dashboard["balanceCents"], 14917,
        "being paid back raises the balance: {dashboard}"
    );
}

/// Where a settlement lands, and what happens when that category is absent.
///
/// The user's own twelve settlements sit in `Haushaltsausgleich`, assigned by hand
/// — the comment ends in a month name, and rules match the whole comment, so no
/// rule could have done it. A fresh instance has the 32 seeded categories and not
/// that one, so the endpoint says outright that it is falling back instead of
/// quietly inventing taxonomy.
#[tokio::test]
async fn a_settlement_lands_in_haushaltsausgleich_when_it_exists() {
    let mock = MockServer::start().await;
    mock.seed(vec![expense(
        1,
        "Supermarkt",
        19.07,
        ms(2),
        Some((1, "Wocheneinkauf")),
        2,
        &[(1, 1), (2, 1)],
    )]);
    let app = app!(Some(mock.url.clone()));
    app.sync().await;

    // Nothing named that yet: the fallback is announced, not hidden.
    let (_, before) = app.send("GET", "/kitchenowl/settlement", None).await;
    assert_eq!(before["categoryIsFallback"], true, "{before}");
    assert!(before["categoryId"].is_null());

    let (status, created) = app
        .send(
            "POST",
            "/categories",
            Some(json!({"name": "Haushaltsausgleich", "typeCode": "variabel"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let (_, after) = app.send("GET", "/kitchenowl/settlement", None).await;
    assert_eq!(after["categoryIsFallback"], false);
    assert_eq!(after["categoryName"], "Haushaltsausgleich");

    let (_, booked) = app.send("POST", "/kitchenowl/settlement", None).await;
    assert_eq!(booked["booking"]["categoryName"], "Haushaltsausgleich");
    assert_eq!(
        booked["booking"]["categorySource"], "manual",
        "an override, so a later rule change cannot move three years of settlements"
    );
}

/// A settlement booking carries an `external_source`, and the delete guard used to
/// read that as "linked to a KitchenOwl expense — remove the link first". There is
/// no link to remove, so the booking would have been undeletable behind an error
/// naming a step that does not exist.
#[tokio::test]
async fn a_settlement_booking_can_be_deleted_again() {
    let mock = MockServer::start().await;
    mock.seed(vec![expense(
        1,
        "Supermarkt",
        19.07,
        ms(2),
        Some((1, "Wocheneinkauf")),
        2,
        &[(1, 1), (2, 1)],
    )]);
    let app = app!(Some(mock.url.clone()));
    app.sync().await;

    let (_, booked) = app.send("POST", "/kitchenowl/settlement", None).await;
    let id = booked["booking"]["id"].as_str().expect("booking id");

    let (status, body) = app.send("DELETE", &format!("/bookings/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    // ...and the month is offered again afterwards, because the period is free.
    let (_, view) = app.send("GET", "/kitchenowl/settlement", None).await;
    assert_eq!(view["alreadySettled"], false);
}

// ------------------------------------------------- the household, year on year

/// Two years in the mirror, with a deliberately uneven shape.
///
/// 2025 carries February and June; 2026 carries February only. That makes June the
/// months-only-one-year-has, which is exactly the case the comparison has to refuse
/// to read as a saving.
fn two_years() -> Vec<serde_json::Value> {
    vec![
        // 2025: Wocheneinkauf in Februar and Juni, Ausflug in Februar.
        expense(
            1,
            "Kaufland",
            40.00,
            on(2025, 2, 10),
            Some((1, "Wocheneinkauf")),
            1,
            &[(1, 1), (2, 1)],
        ),
        expense(
            2,
            "Kaufland",
            60.00,
            on(2025, 6, 10),
            Some((1, "Wocheneinkauf")),
            1,
            &[(1, 1), (2, 1)],
        ),
        expense(
            3,
            "Kino",
            30.00,
            on(2025, 2, 20),
            Some((2, "Ausflug")),
            2,
            &[(1, 1), (2, 1)],
        ),
        // 2026: Wocheneinkauf in Februar only, and a category that is new this year.
        expense(
            4,
            "Kaufland",
            50.00,
            on(2026, 2, 10),
            Some((1, "Wocheneinkauf")),
            1,
            &[(1, 1), (2, 1)],
        ),
        expense(
            5,
            "Baumarkt",
            20.00,
            on(2026, 2, 12),
            Some((3, "Haushalt")),
            2,
            &[(1, 1), (2, 1)],
        ),
    ]
}

/// The household's year against the one before it — and the trap that a part year
/// against a full one is not a comparison.
#[tokio::test]
async fn the_household_year_is_compared_only_over_the_months_both_years_hold() {
    let mock = MockServer::start().await;
    mock.seed(two_years());
    let app = app!(Some(mock.url.clone()));
    let (status, _) = app.sync().await;
    assert_eq!(status, StatusCode::OK);

    let (status, c) = app
        .send("GET", "/kitchenowl/analysis/compare?year=2026", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{c}");
    assert_eq!(c["previousYear"], 2025);
    assert_eq!(c["previousYearHasData"], true);

    // 2026 has one month, 2025 has two, and only Februar is shared.
    assert_eq!(c["fullyComparable"], false);
    assert_eq!(c["comparableMonths"], serde_json::json!([2]));
    assert_eq!(c["current"]["monthsWithData"], 1);
    assert_eq!(c["previous"]["monthsWithData"], 2);
    assert_eq!(c["current"]["lastMonthWithData"], 2);

    // Raw: 70,00 against 130,00 — which would read as a 46 % saving caused entirely
    // by June not having happened yet.
    assert_eq!(c["current"]["amountCents"], 7000);
    assert_eq!(c["previous"]["amountCents"], 13000);
    // Restricted to Februar, the honest pair: 70,00 against 70,00.
    assert_eq!(c["current"]["comparableAmountCents"], 7000);
    assert_eq!(c["previous"]["comparableAmountCents"], 7000);
    // Both figures, always: the user's half of each.
    assert_eq!(c["current"]["ownShareCents"], 3500);
    assert_eq!(c["previous"]["ownShareCents"], 6500);
    assert_eq!(c["current"]["comparableOwnShareCents"], 3500);
    assert_eq!(c["previous"]["comparableOwnShareCents"], 3500);

    let rows = c["rows"].as_array().expect("rows");
    let find = |name: &str| {
        rows.iter()
            .find(|r| r["koCategoryName"] == name)
            .unwrap_or_else(|| panic!("row {name} missing"))
    };

    // Wocheneinkauf: 50,00 this year against 100,00 raw, but 40,00 in the shared
    // month — so the raw delta says −50,00 and the honest one says +10,00. Getting
    // this backwards is the whole reason the restricted figures exist.
    let wocheneinkauf = find("Wocheneinkauf");
    assert_eq!(wocheneinkauf["amountCents"], 5000);
    assert_eq!(wocheneinkauf["previousAmountCents"], 10000);
    assert_eq!(wocheneinkauf["deltaAmountCents"], -5000);
    assert_eq!(wocheneinkauf["comparableAmountCents"], 5000);
    assert_eq!(wocheneinkauf["comparablePreviousAmountCents"], 4000);
    assert_eq!(wocheneinkauf["comparableDeltaAmountCents"], 1000);
    // The share moves with it and is reported separately, never folded in.
    assert_eq!(wocheneinkauf["ownShareCents"], 2500);
    assert_eq!(wocheneinkauf["comparableDeltaOwnShareCents"], 500);
    assert_eq!(wocheneinkauf["monthlyAmountCents"][1], 5000);
    assert_eq!(wocheneinkauf["previousMonthlyAmountCents"][5], 6000);

    // A category that did not exist last year is new, not infinitely more expensive.
    let haushalt = find("Haushalt");
    assert_eq!(haushalt["isNew"], true);
    assert_eq!(haushalt["previousAmountCents"], 0);
    assert!(haushalt["deltaRatio"].is_null());

    // ...and one that has stopped is marked rather than dropped.
    let ausflug = find("Ausflug");
    assert_eq!(ausflug["isGone"], true);
    assert_eq!(ausflug["amountCents"], 0);
    assert_eq!(ausflug["previousAmountCents"], 3000);

    // Who paid, both years. Ada paid the Kino in 2025 and the Baumarkt in 2026.
    let payers = c["paidBy"].as_array().expect("paidBy");
    let ada = payers
        .iter()
        .find(|p| p["name"] == "Ada")
        .expect("Ada paid something");
    assert_eq!(ada["amountCents"], 2000);
    assert_eq!(ada["previousAmountCents"], 3000);
    assert_eq!(ada["deltaCents"], -1000);

    // THE invariant of this whole module: comparing the mirror writes no booking.
    let (_, bookings) = app
        .send("GET", "/bookings?year=2026&status=all", None)
        .await;
    assert_eq!(bookings["total"], 0);
}

/// The rolling window ignores the calendar, which is the only reason it exists.
#[tokio::test]
async fn the_household_trailing_window_crosses_the_year_boundary() {
    let mock = MockServer::start().await;
    mock.seed(two_years());
    let app = app!(Some(mock.url.clone()));
    app.sync().await;

    // Twelve months ending Februar 2026 start in März 2025 — so Juni 2025 is inside
    // the window and Februar 2025 has just fallen out of it.
    let (status, w) = app
        .send(
            "GET",
            "/kitchenowl/analysis/trailing?year=2026&month=2",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{w}");
    assert_eq!(w["fromYear"], 2025);
    assert_eq!(w["fromMonth"], 3);
    assert_eq!(w["months"].as_array().expect("months").len(), 12);

    // Juni 2025 (60,00) plus Februar 2026 (50,00 + 20,00). Februar 2025 is excluded
    // by the window, which is the assertion that matters: a calendar year would
    // have included it or dropped Juni.
    assert_eq!(w["amountCents"], 6000 + 5000 + 2000);
    assert_eq!(w["ownShareCents"], 3000 + 2500 + 1000);
    assert_eq!(w["monthsWithData"], 2);

    let months = w["months"].as_array().expect("months");
    assert_eq!(months[0]["year"], 2025);
    assert_eq!(months[0]["month"], 3);
    // Position 3 in the window is Juni 2025; position 11 is Februar 2026.
    assert_eq!(months[3]["month"], 6);
    assert_eq!(months[3]["amountCents"], 6000);
    assert_eq!(months[11]["year"], 2026);
    assert_eq!(months[11]["month"], 2);
    assert_eq!(months[11]["amountCents"], 7000);

    // Categories are bucketed by WINDOW position, not by calendar month.
    let rows = w["rows"].as_array().expect("rows");
    let wocheneinkauf = rows
        .iter()
        .find(|r| r["koCategoryName"] == "Wocheneinkauf")
        .expect("Wocheneinkauf");
    assert_eq!(wocheneinkauf["monthlyAmountCents"][3], 6000);
    assert_eq!(wocheneinkauf["monthlyAmountCents"][11], 5000);
    // Averaged over the months the household was active in the window, not twelve.
    assert_eq!(wocheneinkauf["averagePerMonthCents"], 5500);

    // A month outside 1..12 is a 400, not a window that silently slides.
    let (status, _) = app
        .send(
            "GET",
            "/kitchenowl/analysis/trailing?year=2026&month=13",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ------------------------------------------------ filing the untagged expenses

/// Five Kaufland receipts, two of them already filed, plus a Hornbach pair and a
/// name nothing knows anything about.
fn tagging_corpus() -> Vec<Value> {
    vec![
        // Already filed: this is the precedent the queue reasons from.
        expense(
            1,
            "Kaufland",
            20.00,
            on(2026, 1, 10),
            Some((1, "Wocheneinkauf")),
            1,
            &[(1, 1), (2, 1)],
        ),
        expense(
            2,
            "kaufland ",
            30.00,
            on(2026, 2, 10),
            Some((1, "Wocheneinkauf")),
            1,
            &[(1, 1), (2, 1)],
        ),
        // Untagged, and deliberately spelled three ways.
        expense(
            3,
            "Kaufland",
            10.00,
            on(2026, 3, 10),
            None,
            1,
            &[(1, 1), (2, 1)],
        ),
        expense(
            4,
            "kaufland",
            12.00,
            on(2026, 4, 10),
            None,
            2,
            &[(1, 1), (2, 1)],
        ),
        expense(
            5,
            "Kaufland ",
            8.00,
            on(2026, 5, 10),
            None,
            1,
            &[(1, 1), (2, 1)],
        ),
        // The user's standing correction says Haushalt; the history says Hobbies.
        expense(
            6,
            "Hornbach",
            25.00,
            on(2026, 6, 10),
            None,
            1,
            &[(1, 1), (2, 1)],
        ),
        expense(
            7,
            "Hornbach",
            15.00,
            on(2026, 6, 20),
            Some((7, "Hobbies")),
            1,
            &[(1, 1)],
        ),
        // Nothing knows this one.
        expense(
            8,
            "Padefke",
            9.00,
            on(2026, 7, 10),
            None,
            1,
            &[(1, 1), (2, 1)],
        ),
    ]
}

/// The queue is a list of DECISIONS, not of rows.
///
/// Sixteen Kaufland receipts are one judgement about Kaufland, so the grouping is
/// by folded name and the count is what sorts it. Every suggestion carries its own
/// evidence, because a preselected dropdown with no explanation is how one wrong
/// guess becomes thirty-nine wrong expenses.
#[tokio::test]
async fn the_untagged_queue_groups_by_name_and_says_where_each_suggestion_came_from() {
    let mock = MockServer::start().await;
    mock.seed(tagging_corpus());
    let app = app!(Some(mock.url.clone()));
    let (status, _) = app.sync().await;
    assert_eq!(status, StatusCode::OK);

    let (status, groups) = app.send("GET", "/kitchenowl/untagged", None).await;
    assert_eq!(status, StatusCode::OK, "{groups}");
    let groups = groups.as_array().expect("groups").clone();
    // Three untagged names, not six untagged expenses.
    assert_eq!(groups.len(), 3);

    // Most frequent first.
    let kaufland = &groups[0];
    assert_eq!(kaufland["matchKey"], "kaufland");
    assert_eq!(kaufland["expenseCount"], 3);
    // Reported under the most recent spelling. The mirror trims on ingest, so the
    // queue shows the tidy form; the body sent back to KitchenOwl is built from
    // KitchenOwl's own copy and keeps the original spacing. Two different jobs.
    assert_eq!(kaufland["name"], "Kaufland");
    // Both figures, never added: 30,00 household, half of it the user's.
    assert_eq!(kaufland["amountCents"], 3000);
    assert_eq!(kaufland["ownShareCents"], 1500);
    assert_eq!(kaufland["firstDate"], "2026-03-10");
    assert_eq!(kaufland["lastDate"], "2026-05-10");
    // The household filed this name twice already. That is the user's own past
    // decision, and it outranks everything except an explicit correction.
    assert_eq!(kaufland["suggestion"]["source"], "precedent");
    assert_eq!(kaufland["suggestion"]["koCategoryName"], "Wocheneinkauf");
    assert_eq!(kaufland["suggestion"]["timesSeen"], 2);

    let hornbach = groups
        .iter()
        .find(|g| g["matchKey"] == "hornbach")
        .expect("Hornbach");
    // History says Hobbies. The user says Haushalt. The user wins, and the label
    // says which of the two this is.
    assert_eq!(hornbach["suggestion"]["source"], "override");
    assert_eq!(hornbach["suggestion"]["koCategoryName"], "Haushalt");

    let padefke = groups
        .iter()
        .find(|g| g["matchKey"] == "padefke")
        .expect("Padefke");
    // No evidence, so no suggestion. Never a guess.
    assert!(padefke["suggestion"].is_null());
}

/// The write itself: one field changes, everything else goes back untouched.
///
/// `POST /api/expense/{id}` REPLACES the expense on a ledger shared with another
/// person, so this asserts the body field by field against what was there before.
#[tokio::test]
async fn tagging_sends_the_whole_expense_back_with_only_the_category_different() {
    let mock = MockServer::start().await;
    mock.seed(tagging_corpus());
    let app = app!(Some(mock.url.clone()));
    app.sync().await;

    let (status, result) = app
        .send(
            "POST",
            "/kitchenowl/untagged/apply",
            Some(json!({"name": "Kaufland", "koCategoryId": 1})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["requested"], 3);
    assert_eq!(result["tagged"], 3);
    assert_eq!(result["failed"], 0);
    assert_eq!(result["koCategoryName"], "Wocheneinkauf");

    // The REQUEST spelling, which is not the response spelling. Getting this wrong
    // is a 400 from the real instance, and a mock that echoed its own output would
    // have let it pass.
    let body = mock.last_update_body().expect("an update body");
    assert_eq!(body["category"], 1, "category is a bare int, not an object");
    assert!(
        body["paid_by"]["id"].is_i64(),
        "paid_by is an object keyed id"
    );
    assert_eq!(
        body["paid_for"][0]["id"], 1,
        "paid_for entries are keyed id"
    );
    assert!(
        body["paid_for"][0]["user_id"].is_null(),
        "user_id is the RESPONSE spelling and must not appear in a request"
    );

    // Everything that was not the category came back verbatim. The amount in
    // particular did not travel through cents and back, and the name kept its
    // trailing space — it belongs to the other member as much as to this one.
    assert_eq!(body["name"], "Kaufland ");
    assert_eq!(body["amount"], 8.0);
    assert_eq!(body["date"], on(2026, 5, 10));
    assert_eq!(body["exclude_from_statistics"], false);

    // And the mirror now agrees with KitchenOwl rather than with the request.
    let (_, groups) = app.send("GET", "/kitchenowl/untagged", None).await;
    let keys: Vec<String> = groups
        .as_array()
        .expect("groups")
        .iter()
        .map(|g| g["matchKey"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(
        !keys.iter().any(|k| k == "kaufland"),
        "still untagged: {keys:?}"
    );

    let (_, page) = app
        .send("GET", "/kitchenowl/expenses?search=Kaufland", None)
        .await;
    for item in page["items"].as_array().expect("items") {
        assert_eq!(item["koCategoryName"], "Wocheneinkauf");
    }
}

/// Applying the same category twice costs one round trip's worth of nothing.
#[tokio::test]
async fn tagging_the_same_name_twice_writes_once() {
    let mock = MockServer::start().await;
    mock.seed(tagging_corpus());
    let app = app!(Some(mock.url.clone()));
    app.sync().await;

    let body = json!({"name": "Kaufland", "koCategoryId": 1});
    let (_, first) = app
        .send("POST", "/kitchenowl/untagged/apply", Some(body.clone()))
        .await;
    assert_eq!(first["tagged"], 3);
    let after_first = mock.update_count();

    let (_, second) = app
        .send("POST", "/kitchenowl/untagged/apply", Some(body))
        .await;
    // Nothing is left untagged under that name, so there is nothing to request.
    assert_eq!(second["requested"], 0);
    assert_eq!(second["tagged"], 0);
    assert_eq!(
        mock.update_count(),
        after_first,
        "a second apply must not re-post anything"
    );
}

/// A refusal leaves the mirror telling the truth.
///
/// The alternative — marking the row tagged because the request was sent — would
/// be a lie that survives every later sync, and it would hide the expense from the
/// very queue that exists to fix it.
#[tokio::test]
async fn a_refused_change_leaves_the_expense_untagged_and_says_why() {
    let mock = MockServer::start().await;
    mock.seed(tagging_corpus());
    let app = app!(Some(mock.url.clone()));
    app.sync().await;
    mock.refuse_updates();

    let (status, result) = app
        .send(
            "POST",
            "/kitchenowl/untagged/apply",
            Some(json!({"name": "Kaufland", "koCategoryId": 1})),
        )
        .await;
    // The batch reports; the failures are per expense. It stops at the FIRST one:
    // a refusal here means a bad category, an expired token, or an instance that is
    // down, and none of those get better by asking two more times. What is left is
    // reported as owed, and the caller tries again once the cause is fixed.
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["requested"], 3);
    assert_eq!(result["tagged"], 0);
    assert_eq!(result["failed"], 1);
    assert_eq!(result["remaining"], 2);
    let failures = result["failures"].as_array().expect("failures");
    assert_eq!(failures.len(), 1);
    // Carrying what the instance actually said, not a generic message.
    assert!(
        failures[0]["error"]
            .as_str()
            .unwrap_or_default()
            .contains("Request invalid"),
        "{:?}",
        failures[0]["error"]
    );

    // Still in the queue, still untagged.
    let (_, groups) = app.send("GET", "/kitchenowl/untagged", None).await;
    let kaufland = groups
        .as_array()
        .expect("groups")
        .iter()
        .find(|g| g["matchKey"] == "kaufland")
        .expect("still untagged")
        .clone();
    assert_eq!(kaufland["expenseCount"], 3);
}

/// An unknown category is refused before anything is sent.
///
/// KitchenOwl would accept an id it does not have and file the expense under
/// nothing visible, which looks like success and reads as a disappearance.
#[tokio::test]
async fn an_unknown_category_is_refused_before_the_first_request() {
    let mock = MockServer::start().await;
    mock.seed(tagging_corpus());
    let app = app!(Some(mock.url.clone()));
    app.sync().await;

    let (status, _) = app
        .send(
            "POST",
            "/kitchenowl/untagged/apply",
            Some(json!({"name": "Kaufland", "koCategoryId": 987})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(mock.update_count(), 0);

    // Nor can it be talked into "tag everything" by omitting the target.
    let (status, _) = app
        .send(
            "POST",
            "/kitchenowl/untagged/apply",
            Some(json!({"koCategoryId": 1})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(mock.update_count(), 0);
}

/// A suggestion is a statement about TWO ledgers, and it was only ever recomputed
/// when one of them moved.
///
/// Drafts are scored when their expense is pulled and re-scored only when that
/// expense changes upstream. Import the bookings afterwards — which is the normal
/// order, since the mirror syncs on boot — and every draft keeps the empty
/// candidate list it was born with. That is not hypothetical: 459 real drafts were
/// written seven minutes before the real bookings arrived, and 457 of them still
/// said "no match" against a ledger holding 165 exact amount matches.
#[tokio::test]
async fn a_rescan_finds_the_matches_that_appeared_after_the_drafts_did() {
    let mock = MockServer::start().await;
    mock.seed(vec![expense(
        1,
        "Supermarkt",
        19.07,
        ms(2),
        Some((1, "Wocheneinkauf")),
        1,
        &[(1, 1), (2, 1)],
    )]);
    let app = app!(Some(mock.url.clone()));

    // The mirror first, with nothing to match against.
    let (status, _) = app.sync().await;
    assert_eq!(status, StatusCode::OK);
    let (_, before) = app.send("GET", "/kitchenowl/drafts", None).await;
    assert_eq!(
        before["items"][0]["candidates"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(before["items"][0]["status"], "open");

    // The booking arrives afterwards, at the full amount and in the same month.
    app.booking("Supermarkt", 1907, 3).await;

    // Nothing has asked the question again, so the draft still says no match.
    let (_, stale) = app.send("GET", "/kitchenowl/drafts", None).await;
    assert_eq!(
        stale["items"][0]["candidates"].as_array().map(Vec::len),
        Some(0)
    );

    let (status, out) = app.send("POST", "/kitchenowl/drafts/rescan", None).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    assert_eq!(out["scanned"], 1);
    assert_eq!(out["withCandidates"], 1);
    assert_eq!(out["likelyDuplicates"], 1);

    let (_, after) = app.send("GET", "/kitchenowl/drafts", None).await;
    let candidates = after["items"][0]["candidates"]
        .as_array()
        .expect("candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0]["amountCents"], 1907);
    assert_eq!(candidates[0]["basis"], "fullAmount");
    // ...and the draft is now flagged, which is what puts it in front of the user.
    assert_eq!(after["items"][0]["status"], "likely_duplicate");
}

/// The push dialogue opened with the category empty, so a booking pushed as
/// "Kaufland" arrived uncategorised though the household had filed every earlier
/// Kaufland under Wocheneinkauf. The suggestion is the tagging queue's own:
/// whatever the household filed the SAME name under, most often — case and
/// padding aside — and nothing at all for a name it has never seen.
#[tokio::test]
async fn a_push_is_offered_the_category_the_household_already_uses_for_that_name() {
    let mock = MockServer::start().await;
    mock.seed(vec![
        expense(
            1,
            "Supermarkt",
            19.07,
            ms(2),
            Some((1, "Wocheneinkauf")),
            2,
            &[(1, 1), (2, 1)],
        ),
        expense(
            2,
            "supermarkt ",
            23.10,
            ms(3),
            Some((1, "Wocheneinkauf")),
            1,
            &[(1, 1), (2, 1)],
        ),
        expense(
            3,
            "Supermarkt",
            8.40,
            ms(4),
            Some((2, "Essen gehen")),
            1,
            &[(1, 1), (2, 1)],
        ),
        expense(4, "Supermarkt", 5.00, ms(5), None, 1, &[(1, 1), (2, 1)]),
    ]);
    let app = app!(Some(mock.url.clone()));
    let (status, body) = app.sync().await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, suggestion) = app
        .send(
            "GET",
            "/kitchenowl/category-suggestion?name=SUPERMARKT",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    // Two of three filed ones say Wocheneinkauf; the uncategorised one says nothing.
    assert_eq!(suggestion["koCategoryName"].as_str(), Some("Wocheneinkauf"));
    assert_eq!(suggestion["source"].as_str(), Some("precedent"));
    assert_eq!(suggestion["timesSeen"].as_i64(), Some(2));

    let (_, unknown) = app
        .send(
            "GET",
            "/kitchenowl/category-suggestion?name=Nie%20gesehen",
            None,
        )
        .await;
    assert!(unknown.is_null(), "never a guess: {unknown}");
}

impl TestApp {
    /// Uploads an ING statement with the given lines and returns
    /// (import id, staged rows).
    async fn stage_statement(&self, lines: &[&str]) -> (String, Vec<Value>) {
        let mut csv = String::from(
            "Umsatzanzeige;Datei erstellt am: 10.06.2026 08:00\n\nIBAN;DE00 0000 0000 0000 0000 00\n\
             Bank;ING\n\nBuchung;Wertstellungsdatum;Auftraggeber/Empfänger;Buchungstext;\
             Verwendungszweck;Saldo;Währung;Betrag;Währung\n",
        );
        for line in lines {
            csv.push_str(line);
            csv.push('\n');
        }
        let boundary = "----finanzen-test-boundary";
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"auszug.csv\"\r\nContent-Type: text/csv\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(csv.as_bytes());
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
        assert!(response.status().is_success(), "upload failed");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let preview: Value = serde_json::from_slice(&bytes).expect("preview");
        let id = preview["id"].as_str().expect("import id").to_string();
        let (_, rows) = self
            .send("GET", &format!("/imports/{id}/statement"), None)
            .await;
        (id, rows["items"].as_array().expect("items").clone())
    }
}

/// A pushed booking is linked before its expense is ever mirrored. The pull that
/// mirrors it used to open a fresh link suggestion anyway — offering the very
/// booking it was pushed from — and nothing ever closed it.
#[tokio::test]
async fn a_pushed_booking_is_not_offered_for_linking_after_the_next_pull() {
    let mock = MockServer::start().await;
    let app = app!(Some(mock.url.clone()));
    app.sync().await;
    let booking_id = app.booking("Kaufland", 1907, 3).await;

    let (status, _) = app
        .send("POST", &format!("/bookings/{booking_id}/kitchenowl"), None)
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    wait_for(&app, |v| v["state"] == "pushed").await;

    let (status, _) = app.sync().await;
    assert_eq!(status, StatusCode::OK);

    let (_, page) = app.send("GET", "/kitchenowl/expenses", None).await;
    assert_eq!(page["items"][0]["linkedBookingId"], booking_id);
    let (_, drafts) = app.send("GET", "/kitchenowl/drafts", None).await;
    assert_eq!(drafts["total"], 0, "nothing left to link: {drafts}");

    // And a rescan, which re-asks every open suggestion, does not reopen it.
    app.send("POST", "/kitchenowl/drafts/rescan", None).await;
    let (_, drafts) = app.send("GET", "/kitchenowl/drafts", None).await;
    assert_eq!(drafts["total"], 0, "{drafts}");
}

/// A statement line can go to KitchenOwl with the push dialogue's own choices. It
/// waits on the line until the import is committed, and only a line that is booked
/// is sent — linked to its booking from the start.
#[tokio::test]
async fn a_statement_line_is_sent_to_kitchenowl_when_it_is_booked() {
    let mock = MockServer::start().await;
    let app = app!(Some(mock.url.clone()));
    app.sync().await;

    let (import, rows) = app
        .stage_statement(&[
            "03.06.2026;03.06.2026;KAUFLAND SAGT DANKE;Lastschrift;Kauf;700,00;EUR;-38,14;EUR",
            "04.06.2026;04.06.2026;BAECKEREI;Lastschrift;Kauf;661,86;EUR;-4,20;EUR",
        ])
        .await;
    let kaufland = rows
        .iter()
        .find(|r| r["amountCents"] == 3814)
        .expect("kaufland");
    let baecker = rows
        .iter()
        .find(|r| r["amountCents"] == 420)
        .expect("baecker");
    let row_path = |r: &Value| format!("/imports/{import}/statement/{}", r["id"].as_str().unwrap());

    // An unknown member is refused while the dialogue is still open.
    let (status, body) = app
        .send(
            "PATCH",
            &row_path(kaufland),
            Some(json!({"koPush": {"paidById": 99}})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let choices = json!({"name": "Wocheneinkauf", "koCategoryId": 2, "paidById": 1,
                         "paidFor": [{"memberId": 1, "factor": 1}, {"memberId": 2, "factor": 1}]});
    let (status, view) = app
        .send(
            "PATCH",
            &row_path(kaufland),
            Some(json!({"comment": "Kaufland", "decision": "accepted", "koPush": choices})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["koPush"]["name"], "Wocheneinkauf");
    assert_eq!(view["koPush"]["paidFor"][1]["memberId"], 2);
    assert_eq!(
        mock.inner().post_count,
        0,
        "nothing is sent before the commit"
    );

    // A line that is staged and then rejected sends nothing.
    app.send(
        "PATCH",
        &row_path(baecker),
        Some(json!({"koPush": {}, "decision": "rejected"})),
    )
    .await;

    let (status, result) = app
        .send("POST", &format!("/imports/{import}/commit"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["inserted"], 1);
    assert_eq!(result["koQueued"], 1);

    let pushed = wait_for(&app, |v| v["state"] == "pushed").await;
    assert_eq!(pushed["amountCents"], 3814, "the full amount travels");
    assert_eq!(pushed["date"], "2026-06-03", "the statement's own date");
    assert_eq!(mock.inner().post_count, 1);
    let posted = mock.inner().expenses[0].clone();
    assert_eq!(posted["name"], "Wocheneinkauf");
    assert_eq!(posted["category_id"], 2);

    let (_, bookings) = app.send("GET", "/bookings?year=2026", None).await;
    assert_eq!(bookings["total"], 1);
    assert_eq!(bookings["items"][0]["comment"], "Kaufland");
    assert_eq!(bookings["items"][0]["externalSource"], "kitchenowl");

    // And the pull that mirrors it leaves nothing to link.
    app.sync().await;
    let (_, drafts) = app.send("GET", "/kitchenowl/drafts", None).await;
    assert_eq!(drafts["total"], 0, "{drafts}");
}

/// Changing your mind takes the push off the line again.
#[tokio::test]
async fn a_staged_push_can_be_withdrawn_before_the_commit() {
    let mock = MockServer::start().await;
    let app = app!(Some(mock.url.clone()));
    app.sync().await;
    let (import, rows) = app
        .stage_statement(&[
            "03.06.2026;03.06.2026;KAUFLAND SAGT DANKE;Lastschrift;Kauf;700,00;EUR;-38,14;EUR",
        ])
        .await;
    let path = format!(
        "/imports/{import}/statement/{}",
        rows[0]["id"].as_str().unwrap()
    );
    app.send(
        "PATCH",
        &path,
        Some(json!({"decision": "accepted", "koPush": {}})),
    )
    .await;
    let (_, view) = app
        .send("PATCH", &path, Some(json!({"clearKoPush": true})))
        .await;
    assert_eq!(view["koPush"], Value::Null);

    let (_, result) = app
        .send("POST", &format!("/imports/{import}/commit"), None)
        .await;
    assert_eq!(result["inserted"], 1);
    assert_eq!(result["koQueued"], 0);
    let (_, list) = app.send("GET", "/kitchenowl/push", None).await;
    assert_eq!(list.as_array().unwrap().len(), 0);
}
