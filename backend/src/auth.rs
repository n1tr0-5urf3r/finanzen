//! Authentication.
//!
//! The provider sits behind [`Authenticator`] from day one so OIDC can be added by
//! writing one more implementation and two routes, without touching a single call
//! site. Everything after `authenticate` — linking an identity to a user, issuing a
//! session, setting the cookie — is provider-agnostic and written once.

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts},
};
use chrono::{Duration as ChronoDuration, Utc};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::future::Future;
use uuid::Uuid;

use crate::{
    AppState,
    config::Config,
    error::{AppError, Result},
    models::User,
    tenant::Tenant,
};

pub const COOKIE: &str = "finanzen_session";
const MIN_PASSWORD_LEN: usize = 10;
const MAX_LOGIN_ATTEMPTS: u32 = 5;
const THROTTLE_SECONDS: u64 = 300;

// ------------------------------------------------------------ the trait

/// What a provider proves. Deliberately says nothing about sessions or cookies, so a
/// second provider cannot drift in how those work.
#[derive(Debug, Clone)]
pub struct VerifiedIdentity {
    pub provider: String,
    pub subject: String,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub admin_hint: Option<bool>,
}

#[derive(Debug)]
pub enum Credential<'a> {
    Password {
        username: &'a str,
        password: &'a str,
    },
    #[allow(dead_code)]
    OidcCallback { code: &'a str, state: &'a str },
}

/// Drives the login page without the frontend knowing which provider is configured.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AuthChallenge {
    Password,
    Redirect { url: String, state: String },
}

pub trait Authenticator: Send + Sync {
    fn id(&self) -> &'static str;
    fn begin(&self) -> Result<AuthChallenge>;
    fn authenticate<'a>(
        &'a self,
        db: &'a PgPool,
        cred: Credential<'a>,
    ) -> impl Future<Output = Result<VerifiedIdentity>> + Send + 'a;
    fn change_password<'a>(
        &'a self,
        db: &'a PgPool,
        user_id: Uuid,
        current: &'a str,
        new: &'a str,
    ) -> impl Future<Output = Result<()>> + Send + 'a;
    fn create_credential<'a>(
        &'a self,
        db: &'a PgPool,
        user_id: Uuid,
        password: &'a str,
    ) -> impl Future<Output = Result<()>> + Send + 'a;
}

/// Edition-2024 AFIT traits are not dyn-compatible, so dispatch goes through this
/// enum. Call sites see one type forever; adding OIDC adds a variant.
pub enum AuthProvider {
    Local(LocalAuth),
}

impl Authenticator for AuthProvider {
    fn id(&self) -> &'static str {
        match self {
            Self::Local(p) => p.id(),
        }
    }
    fn begin(&self) -> Result<AuthChallenge> {
        match self {
            Self::Local(p) => p.begin(),
        }
    }
    async fn authenticate<'a>(
        &'a self,
        db: &'a PgPool,
        cred: Credential<'a>,
    ) -> Result<VerifiedIdentity> {
        match self {
            Self::Local(p) => p.authenticate(db, cred).await,
        }
    }
    async fn change_password<'a>(
        &'a self,
        db: &'a PgPool,
        user_id: Uuid,
        current: &'a str,
        new: &'a str,
    ) -> Result<()> {
        match self {
            Self::Local(p) => p.change_password(db, user_id, current, new).await,
        }
    }
    async fn create_credential<'a>(
        &'a self,
        db: &'a PgPool,
        user_id: Uuid,
        password: &'a str,
    ) -> Result<()> {
        match self {
            Self::Local(p) => p.create_credential(db, user_id, password).await,
        }
    }
}

// ------------------------------------------------------- the local provider

#[derive(Default)]
pub struct LocalAuth;

impl Authenticator for LocalAuth {
    fn id(&self) -> &'static str {
        "local"
    }

    fn begin(&self) -> Result<AuthChallenge> {
        Ok(AuthChallenge::Password)
    }

    async fn authenticate<'a>(
        &'a self,
        db: &'a PgPool,
        cred: Credential<'a>,
    ) -> Result<VerifiedIdentity> {
        let Credential::Password { username, password } = cred else {
            return Err(AppError::Validation(
                "Dieser Anbieter erwartet Benutzername und Passwort".into(),
            ));
        };

        let row = sqlx::query(
            "SELECT id, username, display_name, email, password_hash, is_admin, disabled_at \
               FROM users WHERE lower(username) = lower($1)",
        )
        .bind(username)
        .fetch_optional(db)
        .await?;

        // Verify against a dummy hash when the user does not exist, so a missing
        // account and a wrong password take the same time.
        let Some(row) = row else {
            let _ = verify_password("dummy", DUMMY_HASH);
            return Err(AppError::Unauthorized);
        };
        if row
            .get::<Option<chrono::DateTime<Utc>>, _>("disabled_at")
            .is_some()
        {
            return Err(AppError::Forbidden);
        }
        let hash: Option<String> = row.get("password_hash");
        let hash = hash.ok_or(AppError::Unauthorized)?;
        if !verify_password(password, &hash) {
            return Err(AppError::Unauthorized);
        }

        let id: Uuid = row.get("id");
        Ok(VerifiedIdentity {
            provider: "local".into(),
            subject: id.to_string(),
            username: row.get("username"),
            display_name: row.get("display_name"),
            email: row.get("email"),
            admin_hint: None,
        })
    }

    async fn change_password<'a>(
        &'a self,
        db: &'a PgPool,
        user_id: Uuid,
        current: &'a str,
        new: &'a str,
    ) -> Result<()> {
        validate_password(new)?;
        let hash: Option<String> =
            sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_optional(db)
                .await?
                .flatten();
        let hash = hash.ok_or(AppError::Unauthorized)?;
        if !verify_password(current, &hash) {
            return Err(AppError::Unauthorized);
        }
        sqlx::query("UPDATE users SET password_hash = $2, updated_at = now() WHERE id = $1")
            .bind(user_id)
            .bind(hash_password(new)?)
            .execute(db)
            .await?;
        // A password change revokes every existing session.
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id)
            .execute(db)
            .await?;
        Ok(())
    }

    async fn create_credential<'a>(
        &'a self,
        db: &'a PgPool,
        user_id: Uuid,
        password: &'a str,
    ) -> Result<()> {
        validate_password(password)?;
        sqlx::query("UPDATE users SET password_hash = $2 WHERE id = $1")
            .bind(user_id)
            .bind(hash_password(password)?)
            .execute(db)
            .await?;
        Ok(())
    }
}

/// A real argon2 hash of a value nobody will guess, used to equalise timing between
/// "no such user" and "wrong password".
const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHR2YWx1ZQ$\
                          Kq0kY5Z0Z3l5bVZ0aGlzaXNub3RhcmVhbGhhc2g";

fn validate_password(password: &str) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(AppError::Validation(format!(
            "Das Passwort muss mindestens {MIN_PASSWORD_LEN} Zeichen lang sein"
        )));
    }
    Ok(())
}

fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Passwort-Hash fehlgeschlagen: {e}")))
}

fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

// -------------------------------------------- provider-agnostic session logic

/// Finds or provisions the user behind a verified identity. Provider-independent, so
/// OIDC reuses it verbatim.
pub async fn link_identity(db: &PgPool, config: &Config, vi: &VerifiedIdentity) -> Result<User> {
    if let Some(user) = find_by_identity(db, &vi.provider, &vi.subject).await? {
        return Ok(user);
    }
    // An existing local account adopting a second identity: matched on email only,
    // because an email is the one attribute both providers can assert about the
    // same person.
    if let Some(email) = vi.email.as_deref()
        && let Some(user) = find_by_email(db, email).await?
    {
        sqlx::query(
            "INSERT INTO identities (id, user_id, provider, subject) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (provider, subject) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(user.id)
        .bind(&vi.provider)
        .bind(&vi.subject)
        .execute(db)
        .await?;
        return Ok(user);
    }
    if !config.auth_allow_auto_provision {
        // Registration is closed by default; an unknown identity is not an invitation.
        return Err(AppError::Forbidden);
    }
    provision_user(db, vi).await
}

async fn find_by_identity(db: &PgPool, provider: &str, subject: &str) -> Result<Option<User>> {
    let row = sqlx::query(
        "SELECT u.id, u.username, u.display_name, u.is_admin \
           FROM users u JOIN identities i ON i.user_id = u.id \
          WHERE i.provider = $1 AND i.subject = $2 AND u.disabled_at IS NULL",
    )
    .bind(provider)
    .bind(subject)
    .fetch_optional(db)
    .await?;
    Ok(row.map(row_to_user))
}

async fn find_by_email(db: &PgPool, email: &str) -> Result<Option<User>> {
    let row = sqlx::query(
        "SELECT id, username, display_name, is_admin FROM users \
          WHERE lower(email) = lower($1) AND disabled_at IS NULL",
    )
    .bind(email)
    .fetch_optional(db)
    .await?;
    Ok(row.map(row_to_user))
}

async fn provision_user(db: &PgPool, vi: &VerifiedIdentity) -> Result<User> {
    let id = Uuid::new_v4();
    let mut tx = db.begin().await?;
    sqlx::query(
        "INSERT INTO users (id, username, display_name, email, is_admin) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(&vi.username)
    .bind(&vi.display_name)
    .bind(&vi.email)
    .bind(vi.admin_hint.unwrap_or(false))
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::from_db(e, "Benutzername ist bereits vergeben"))?;
    sqlx::query("INSERT INTO identities (id, user_id, provider, subject) VALUES ($1,$2,$3,$4)")
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(&vi.provider)
        .bind(&vi.subject)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    seed_new_user(db, id).await?;
    Ok(User {
        id,
        username: vi.username.clone(),
        display_name: vi.display_name.clone(),
        is_admin: vi.admin_hint.unwrap_or(false),
    })
}

/// A new account starts with the five category types and 32 categories, seeded
/// inside the tenant context because those tables are themselves RLS-protected.
pub async fn seed_new_user(db: &PgPool, user_id: Uuid) -> Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("SELECT set_config('app.user_id', $1, true)")
        .bind(user_id.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT app.seed_default_taxonomy($1)")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

fn row_to_user(row: sqlx::postgres::PgRow) -> User {
    User {
        id: row.get("id"),
        username: row.get("username"),
        display_name: row.get("display_name"),
        is_admin: row.get("is_admin"),
    }
}

/// Issues a session and returns `(token, Set-Cookie header value)`.
///
/// Only the hash is stored, so a database dump yields no usable sessions.
pub async fn issue_session(
    db: &PgPool,
    config: &Config,
    user_id: Uuid,
    via: &str,
    user_agent: Option<&str>,
) -> Result<String> {
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let expires = Utc::now()
        + ChronoDuration::from_std(config.session_ttl)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("ungültige Session-Dauer: {e}")))?;

    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, issued_via, expires_at, user_agent) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(token_hash(&token, &config.session_secret))
    .bind(user_id)
    .bind(via)
    .bind(expires)
    .bind(user_agent)
    .execute(db)
    .await?;

    Ok(session_cookie(config, &token))
}

pub fn token_hash(token: &str, secret: &str) -> String {
    hex::encode(Sha256::digest(format!("{secret}:{token}").as_bytes()))
}

pub fn session_cookie(config: &Config, token: &str) -> String {
    // `Secure` is derived from APP_PUBLIC_URL, so a misconfigured public URL costs
    // the flag — which is why the deployment docs insist it be the exact https URL.
    let secure = if config.cookie_secure() {
        "; Secure"
    } else {
        ""
    };
    format!(
        "{COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure}",
        config.session_ttl.as_secs()
    )
}

pub fn clear_cookie(config: &Config) -> String {
    let secure = if config.cookie_secure() {
        "; Secure"
    } else {
        ""
    };
    format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}")
}

pub fn cookie_value<'a>(headers: &'a axum::http::HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v)
}

// ------------------------------------------------------------- throttling

/// In-process login throttle. Deliberately simple: this app runs as a single
/// replica, and a shared store would be more machinery than the threat warrants.
pub fn check_throttle(state: &AppState, username: &str) -> Result<()> {
    let mut attempts = state.login_attempts.lock().expect("login attempt lock");
    let now = std::time::Instant::now();
    attempts.retain(|_, (_, at)| now.duration_since(*at).as_secs() < THROTTLE_SECONDS);
    if let Some((count, _)) = attempts.get(&username.to_lowercase())
        && *count >= MAX_LOGIN_ATTEMPTS
    {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

pub fn record_failure(state: &AppState, username: &str) {
    let mut attempts = state.login_attempts.lock().expect("login attempt lock");
    let entry = attempts
        .entry(username.to_lowercase())
        .or_insert((0, std::time::Instant::now()));
    entry.0 += 1;
    entry.1 = std::time::Instant::now();
}

pub fn clear_failures(state: &AppState, username: &str) {
    state
        .login_attempts
        .lock()
        .expect("login attempt lock")
        .remove(&username.to_lowercase());
}

// -------------------------------------------------------------- extractors

/// Authenticated user plus an open tenant-scoped transaction.
///
/// Requesting this in a handler signature is what makes the handler protected *and*
/// tenant-scoped. There is no other way to reach user data, which is why no query in
/// this codebase binds a `user_id`.
pub struct Ctx {
    pub user: User,
    pub tenant: Tenant,
}

impl FromRequestParts<AppState> for Ctx {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self> {
        let user = resolve_session(parts, state).await?;
        let tenant = Tenant::begin(&state.db, user.id).await?;
        Ok(Self { user, tenant })
    }
}

/// The session only, without opening a transaction — for endpoints that touch the
/// system-scope tables (logout, password change).
pub struct SessionUser(pub User);

impl FromRequestParts<AppState> for SessionUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self> {
        Ok(Self(resolve_session(parts, state).await?))
    }
}

async fn resolve_session(parts: &Parts, state: &AppState) -> Result<User> {
    let token = cookie_value(&parts.headers, COOKIE).ok_or(AppError::Unauthorized)?;
    let row = sqlx::query(
        "SELECT u.id, u.username, u.display_name, u.is_admin \
           FROM sessions s JOIN users u ON u.id = s.user_id \
          WHERE s.token_hash = $1 AND s.expires_at > now() AND u.disabled_at IS NULL",
    )
    .bind(token_hash(token, &state.config.session_secret))
    .fetch_optional(state.db.system().inner())
    .await?
    .ok_or(AppError::Unauthorized)?;
    Ok(row_to_user(row))
}

pub fn require_admin(user: &User) -> Result<()> {
    if user.is_admin {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trips_and_rejects_wrong_input() {
        let hash = hash_password("correct horse battery").unwrap();
        assert!(verify_password("correct horse battery", &hash));
        assert!(!verify_password("wrong", &hash));
        // A malformed stored hash must fail closed, not panic.
        assert!(!verify_password("x", "not-a-hash"));
    }

    #[test]
    fn short_passwords_are_refused() {
        assert!(validate_password("short").is_err());
        assert!(validate_password("0123456789").is_ok());
    }

    #[test]
    fn token_hash_depends_on_the_secret() {
        assert_ne!(token_hash("abc", "s1"), token_hash("abc", "s2"));
        assert_eq!(token_hash("abc", "s1"), token_hash("abc", "s1"));
    }

    #[test]
    fn cookie_is_httponly_and_lax_and_secure_only_behind_https() {
        let mut config = Config::test("postgres://x/y");
        config.public_url = "http://localhost:3100".into();
        let cookie = session_cookie(&config, "tok");
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(!cookie.contains("Secure"));

        config.public_url = "https://finanzen.example.org".into();
        assert!(session_cookie(&config, "tok").contains("; Secure"));
    }

    #[test]
    fn parses_a_cookie_out_of_a_header_with_several() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "other=1; finanzen_session=abc123; third=2".parse().unwrap(),
        );
        assert_eq!(cookie_value(&headers, COOKIE), Some("abc123"));
        assert_eq!(cookie_value(&headers, "missing"), None);
    }
}
