use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

/// Configuration is read from the process environment with plain `std::env::var`.
/// There is deliberately no dotenv crate in the binary: Docker Compose supplies the
/// environment in production and the shell supplies it in development.
#[derive(Clone, Debug)]
pub struct Config {
    pub dev_mode: bool,
    pub bind: SocketAddr,
    pub public_url: String,
    pub data_dir: PathBuf,
    pub frontend_dir: PathBuf,
    pub database_url: String,
    pub db_max_connections: u32,
    pub db_connect_timeout: Duration,
    pub session_secret: String,
    pub session_ttl: Duration,
    pub session_idle_refresh: Duration,
    pub max_upload_bytes: usize,

    pub auth_provider: String,
    pub auth_allow_registration: bool,
    pub auth_allow_auto_provision: bool,
    pub oidc_issuer: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret: Option<String>,
    pub oidc_admin_group: Option<String>,

    pub kitchenowl_url: Option<String>,
    pub kitchenowl_token: Option<String>,
    pub kitchenowl_household_id: Option<i64>,
    pub kitchenowl_http_timeout: Duration,
    pub kitchenowl_expense_poll_seconds: u64,
    pub kitchenowl_metadata_refresh_seconds: u64,
    pub kitchenowl_push_retry_seconds: u64,
    pub kitchenowl_sync_on_start: bool,
    pub kitchenowl_metadata_stale_seconds: i64,
    pub kitchenowl_max_pull_pages: u32,
    pub kitchenowl_push_max_attempts: i32,
    pub kitchenowl_push_marker_in_name: bool,
    pub kitchenowl_duplicate_threshold: f64,

    pub import_fuzzy_min_confidence: f64,
    pub import_auto_create_rule: bool,
    pub import_max_rows: usize,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let dev_mode = read_bool("APP_DEV_MODE", false);
        let session_secret = optional("APP_SESSION_SECRET").unwrap_or_default();
        if !dev_mode && session_secret.len() < 32 {
            anyhow::bail!(
                "APP_SESSION_SECRET muss mindestens 32 Zeichen lang sein \
                 (oder APP_DEV_MODE=true setzen)"
            );
        }
        let data_dir = PathBuf::from(env::var("APP_DATA_DIR").unwrap_or_else(|_| "./data".into()));
        let database_url = env::var("DATABASE_URL").map_err(|_| {
            anyhow::anyhow!("DATABASE_URL ist erforderlich (PostgreSQL-Verbindungsstring)")
        })?;

        Ok(Self {
            dev_mode,
            bind: env::var("APP_BIND")
                .unwrap_or_else(|_| "0.0.0.0:3100".into())
                .parse()?,
            public_url: env::var("APP_PUBLIC_URL")
                .unwrap_or_else(|_| "http://localhost:3100".into())
                .trim_end_matches('/')
                .to_string(),
            data_dir,
            frontend_dir: PathBuf::from(
                env::var("APP_FRONTEND_DIR").unwrap_or_else(|_| "./frontend".into()),
            ),
            database_url,
            db_max_connections: read_u64("DB_MAX_CONNECTIONS", 10) as u32,
            db_connect_timeout: Duration::from_secs(read_u64("DB_CONNECT_TIMEOUT_SECONDS", 10)),
            session_secret: if session_secret.is_empty() {
                "development-only-change-me".into()
            } else {
                session_secret
            },
            session_ttl: Duration::from_secs(read_u64("APP_SESSION_TTL_SECONDS", 60 * 60 * 24 * 30)),
            session_idle_refresh: Duration::from_secs(read_u64("APP_SESSION_IDLE_REFRESH", 3600)),
            max_upload_bytes: read_u64("APP_MAX_UPLOAD_BYTES", 26_214_400) as usize,

            auth_provider: env::var("AUTH_PROVIDER").unwrap_or_else(|_| "local".into()),
            auth_allow_registration: read_bool("AUTH_ALLOW_REGISTRATION", false),
            auth_allow_auto_provision: read_bool("AUTH_ALLOW_AUTO_PROVISION", false),
            oidc_issuer: optional("OIDC_ISSUER"),
            oidc_client_id: optional("OIDC_CLIENT_ID"),
            oidc_client_secret: optional("OIDC_CLIENT_SECRET"),
            oidc_admin_group: optional("OIDC_ADMIN_GROUP"),

            kitchenowl_url: optional("KITCHENOWL_URL").map(|v| v.trim_end_matches('/').to_string()),
            kitchenowl_token: optional("KITCHENOWL_TOKEN"),
            kitchenowl_household_id: optional("KITCHENOWL_HOUSEHOLD_ID")
                .and_then(|v| v.parse().ok()),
            kitchenowl_http_timeout: Duration::from_secs(read_u64(
                "KITCHENOWL_HTTP_TIMEOUT_SECONDS",
                30,
            )),
            kitchenowl_expense_poll_seconds: read_u64("KITCHENOWL_EXPENSE_POLL_SECONDS", 900),
            kitchenowl_metadata_refresh_seconds: read_u64(
                "KITCHENOWL_METADATA_REFRESH_SECONDS",
                86_400,
            ),
            kitchenowl_push_retry_seconds: read_u64("KITCHENOWL_PUSH_RETRY_SECONDS", 300),
            kitchenowl_sync_on_start: read_bool("KITCHENOWL_SYNC_ON_START", true),
            kitchenowl_metadata_stale_seconds: read_u64("KITCHENOWL_METADATA_STALE_SECONDS", 172_800)
                as i64,
            kitchenowl_max_pull_pages: read_u64("KITCHENOWL_MAX_PULL_PAGES", 40) as u32,
            kitchenowl_push_max_attempts: read_u64("KITCHENOWL_PUSH_MAX_ATTEMPTS", 10) as i32,
            kitchenowl_push_marker_in_name: read_bool("KITCHENOWL_PUSH_MARKER_IN_NAME", false),
            kitchenowl_duplicate_threshold: read_f64("KITCHENOWL_DUPLICATE_THRESHOLD", 0.80),

            import_fuzzy_min_confidence: read_f64("IMPORT_FUZZY_MIN_CONFIDENCE", 0.92),
            import_auto_create_rule: read_bool("IMPORT_AUTO_CREATE_RULE", true),
            import_max_rows: read_u64("IMPORT_MAX_ROWS", 20_000) as usize,
        })
    }

    /// Fixture configuration for tests. `dev_mode` is true so the RLS superuser guard
    /// does not refuse the owning role the test harness connects as; `FORCE ROW LEVEL
    /// SECURITY` keeps the isolation tests meaningful regardless.
    pub fn test(database_url: impl Into<String>) -> Self {
        Self {
            dev_mode: true,
            bind: "127.0.0.1:0".parse().expect("valid test bind address"),
            public_url: "http://localhost:3100".into(),
            data_dir: std::env::temp_dir().join("finanzen-test"),
            frontend_dir: PathBuf::from("./frontend"),
            database_url: database_url.into(),
            db_max_connections: 5,
            db_connect_timeout: Duration::from_secs(10),
            session_secret: "test-session-secret-that-is-long-enough".into(),
            session_ttl: Duration::from_secs(3600),
            session_idle_refresh: Duration::from_secs(3600),
            max_upload_bytes: 26_214_400,
            auth_provider: "local".into(),
            auth_allow_registration: false,
            auth_allow_auto_provision: false,
            oidc_issuer: None,
            oidc_client_id: None,
            oidc_client_secret: None,
            oidc_admin_group: None,
            kitchenowl_url: None,
            kitchenowl_token: None,
            kitchenowl_household_id: None,
            kitchenowl_http_timeout: Duration::from_secs(30),
            kitchenowl_expense_poll_seconds: 0,
            kitchenowl_metadata_refresh_seconds: 0,
            kitchenowl_push_retry_seconds: 0,
            kitchenowl_sync_on_start: false,
            kitchenowl_metadata_stale_seconds: 172_800,
            kitchenowl_max_pull_pages: 40,
            kitchenowl_push_max_attempts: 10,
            kitchenowl_push_marker_in_name: false,
            kitchenowl_duplicate_threshold: 0.80,
            import_fuzzy_min_confidence: 0.92,
            import_auto_create_rule: true,
            import_max_rows: 20_000,
        }
    }

    pub fn cookie_secure(&self) -> bool {
        self.public_url.starts_with("https://")
    }
}

fn optional(name: &str) -> Option<String> {
    env::var(name).ok().filter(|s| !s.trim().is_empty())
}

fn read_u64(name: &str, default: u64) -> u64 {
    optional(name).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn read_f64(name: &str, default: f64) -> f64 {
    optional(name).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn read_bool(name: &str, default: bool) -> bool {
    optional(name)
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default)
}
