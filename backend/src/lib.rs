pub mod analysis;
pub mod auth;
pub mod bookings;
pub mod calc;
pub mod categories;
pub mod config;
pub mod db;
pub mod error;
pub mod locale;
pub mod models;
pub mod rules;
pub mod sheets;
pub mod tenant;
pub mod years;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, atomic::AtomicBool},
    time::Instant,
};

use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, header},
    middleware::{self, Next},
    response::Response,
    routing::{get, post, put},
};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

pub use config::Config;
pub use error::{AppError, Result};

use auth::{AuthProvider, LocalAuth};
use db::Db;
use models::StatusResponse;

/// Guards for the background loops. Separate flags rather than one shared one,
/// because the loops are independent and must not block each other.
#[derive(Default)]
pub struct SyncGuards {
    pub ko_expenses: AtomicBool,
    pub ko_push: AtomicBool,
    pub ko_metadata: AtomicBool,
    pub import_commit: AtomicBool,
}

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub config: Arc<Config>,
    pub http: reqwest::Client,
    pub auth: Arc<AuthProvider>,
    pub guards: Arc<SyncGuards>,
    /// Resolved once per process. schmauserei re-resolves the household on every
    /// outbound call when the env var is unset; this avoids that.
    pub ko_household: Arc<tokio::sync::OnceCell<i64>>,
    pub login_attempts: Arc<Mutex<HashMap<String, (u32, Instant)>>>,
}

impl AppState {
    pub fn new(db: Db, config: Config) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("Finanzen/0.1")
            .timeout(config.kitchenowl_http_timeout)
            .build()
            .expect("build http client");
        // Only one provider today. The enum — not a branch here — is what lets OIDC
        // arrive without touching any call site; this becomes a match then.
        let auth = AuthProvider::Local(LocalAuth);
        Self {
            db,
            config: Arc::new(config),
            http,
            auth: Arc::new(auth),
            guards: Arc::new(SyncGuards::default()),
            ko_household: Arc::new(tokio::sync::OnceCell::new()),
            login_attempts: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/auth/setup-status", get(auth_routes::setup_status))
        .route("/auth/setup", post(auth_routes::setup))
        .route("/auth/login", post(auth_routes::login))
        .route("/auth/logout", post(auth_routes::logout))
        .route("/auth/me", get(auth_routes::me))
        .route("/auth/password", put(auth_routes::change_password))
        .route(
            "/admin/users",
            get(auth_routes::list_users).post(auth_routes::create_user),
        )
        .route("/bookings", get(bookings::list).post(bookings::create))
        .route("/bookings/comments", get(bookings::comments))
        .route(
            "/bookings/{id}",
            get(bookings::get_one)
                .put(bookings::update)
                .delete(bookings::delete),
        )
        .route("/bookings/bulk", post(bookings::bulk))
        .route(
            "/categories",
            get(categories::list).post(categories::create),
        )
        .route(
            "/categories/{id}",
            put(categories::update).delete(categories::delete),
        )
        .route("/category-types", get(categories::list_types))
        .route("/rules", get(rules::list).post(rules::create))
        .route("/rules/{id}", put(rules::update).delete(rules::delete))
        .route("/rules/apply", post(rules::apply))
        .route("/dashboard", get(analysis::dashboard))
        .route("/overview/months", get(analysis::monthly))
        .route("/analysis/categories", get(analysis::categories))
        .route("/tax", get(analysis::tax))
        .route("/years", get(years::list).post(years::create))
        .route("/years/{year}", put(years::update))
        .layer(middleware::from_fn_with_state(state.clone(), csrf))
        .with_state(state.clone());

    // The SPA is served from the same origin as the API, so there is no CORS and no
    // second container. The ServeFile fallback is what makes client-side deep links
    // survive a reload.
    let static_files = ServeDir::new(&state.config.frontend_dir)
        .fallback(ServeFile::new(state.config.frontend_dir.join("index.html")));

    Router::new()
        .nest("/api/v1", api)
        .fallback_service(static_files)
        .layer(TraceLayer::new_for_http())
}

async fn health() -> Json<StatusResponse> {
    Json(StatusResponse {
        status: "ok".into(),
    })
}

/// Readiness means the migrations ran and the pool answers, which is what the
/// container healthcheck and the CI smoke test actually care about.
async fn ready(State(state): State<AppState>) -> Result<Json<StatusResponse>> {
    sqlx::query("SELECT 1")
        .execute(state.db.system().inner())
        .await?;
    Ok(Json(StatusResponse {
        status: "ready".into(),
    }))
}

/// CSRF by Origin check.
///
/// With `SameSite=Lax` the browser already withholds the cookie from cross-site POSTs;
/// this closes the remaining gap by refusing any mutating request that carries a
/// session cookie and a foreign Origin. Only requests that actually carry the cookie
/// are checked, so unauthenticated API use is unaffected.
async fn csrf(State(state): State<AppState>, request: Request, next: Next) -> Result<Response> {
    let mutating = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    let has_session = request
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains(auth::COOKIE));

    if mutating && has_session {
        let origin = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .ok_or(AppError::Forbidden)?;
        let actual = url::Url::parse(origin).map_err(|_| AppError::Forbidden)?;
        if !origin_matches(&actual, &state.config.public_url, request.headers()) {
            return Err(AppError::Forbidden);
        }
    }
    Ok(next.run(request).await)
}

fn origin_matches(origin: &url::Url, public_url: &str, headers: &axum::http::HeaderMap) -> bool {
    if origin.as_str().trim_end_matches('/') == public_url {
        return true;
    }
    // Behind the TLS proxy the public Host is preserved but the scheme is only
    // recoverable from X-Forwarded-Proto, so honour the first value.
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let forwarded_proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim);
    let scheme_ok = match forwarded_proto {
        Some(proto) => origin.scheme() == proto,
        None => true,
    };
    scheme_ok && origin.authority() == host
}

mod auth_routes;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_matching_accepts_the_public_url_and_the_forwarded_host() {
        let headers = axum::http::HeaderMap::new();
        let public = "https://finanzen.example.org";
        assert!(origin_matches(
            &url::Url::parse("https://finanzen.example.org").unwrap(),
            public,
            &headers
        ));
        assert!(!origin_matches(
            &url::Url::parse("https://evil.example").unwrap(),
            public,
            &headers
        ));

        // Local development on a different port, with the Host header agreeing.
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(header::HOST, "localhost:5173".parse().unwrap());
        assert!(origin_matches(
            &url::Url::parse("http://localhost:5173").unwrap(),
            "http://localhost:3100",
            &headers
        ));
    }

    #[test]
    fn forwarded_proto_must_agree_with_the_origin_scheme() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(header::HOST, "finanzen.example.org".parse().unwrap());
        headers.insert("x-forwarded-proto", "https".parse().unwrap());
        assert!(origin_matches(
            &url::Url::parse("https://finanzen.example.org").unwrap(),
            "https://other.example",
            &headers
        ));
        // A plain-http Origin behind an https proxy is not the same site.
        assert!(!origin_matches(
            &url::Url::parse("http://finanzen.example.org").unwrap(),
            "https://other.example",
            &headers
        ));
    }
}
