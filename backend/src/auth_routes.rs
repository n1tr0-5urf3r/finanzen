//! Auth endpoints. Every login path funnels through `Authenticator::authenticate`
//! followed by the provider-agnostic `link_identity` + `issue_session`, so adding
//! OIDC means adding a callback route that calls the same two functions.

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{
        self, Authenticator, Credential, SessionUser, VerifiedIdentity, clear_cookie,
        issue_session, link_identity, require_admin, seed_new_user,
    },
    error::{AppError, Result},
    models::{CreateUserRequest, LoginRequest, PasswordRequest, SetupRequest, SetupStatus, User},
};

fn user_agent(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
}

fn with_cookie(cookie: String, status: StatusCode, body: impl serde::Serialize) -> Response {
    (
        status,
        [(header::SET_COOKIE, cookie)],
        Json(serde_json::to_value(body).unwrap_or_default()),
    )
        .into_response()
}

/// First-run bootstrap. Re-checks the user count inside the transaction, so two
/// concurrent setup requests cannot both create an admin.
#[utoipa::path(
    get,
    path = "/api/v1/auth/setup-status",
    tag = "auth",
    responses((status = 200, description = "Ob die Ersteinrichtung noch aussteht", body = SetupStatus)),
)]
pub async fn setup_status(State(state): State<AppState>) -> Result<Json<SetupStatus>> {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(state.db.system().inner())
        .await?;
    Ok(Json(SetupStatus {
        setup_required: count == 0,
        provider: state.auth.id().to_string(),
        registration_open: state.config.auth_allow_registration,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/setup",
    tag = "auth",
    request_body = SetupRequest,
    responses((status = 201, description = "Administratorkonto angelegt", body = User), (status = 409, description = "Es existiert bereits ein Konto", body = crate::error::ErrorBody)),
)]
pub async fn setup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SetupRequest>,
) -> Result<Response> {
    if body.username.trim().is_empty() {
        return Err(AppError::Validation("Benutzername fehlt".into()));
    }
    let pool = state.db.system().inner();
    let mut tx = pool.begin().await?;
    // Serialise concurrent first-run attempts. `SELECT count(*) ... FOR UPDATE` is
    // not valid with an aggregate, and there is no row to lock yet anyway, so a
    // transaction-scoped advisory lock is what makes the re-check meaningful.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('finanzen:setup'))")
        .execute(&mut *tx)
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&mut *tx)
        .await?;
    if count > 0 {
        return Err(AppError::Conflict(
            "Die Einrichtung ist bereits abgeschlossen".into(),
        ));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, username, display_name, is_admin) VALUES ($1, $2, $3, true)",
    )
    .bind(id)
    .bind(body.username.trim())
    .bind(body.display_name.trim())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::from_db(e, "Benutzername ist bereits vergeben"))?;
    sqlx::query(
        "INSERT INTO identities (id, user_id, provider, subject) VALUES ($1,$2,'local',$3)",
    )
    .bind(Uuid::new_v4())
    .bind(id)
    .bind(id.to_string())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    state
        .auth
        .create_credential(pool, id, &body.password)
        .await?;
    seed_new_user(pool, id).await?;

    let cookie = issue_session(pool, &state.config, id, "local", user_agent(&headers)).await?;
    let user = User {
        id,
        username: body.username.trim().to_string(),
        display_name: body.display_name.trim().to_string(),
        is_admin: true,
    };
    Ok(with_cookie(cookie, StatusCode::CREATED, user))
}

/// Registration is closed by default, so accounts are created here by an admin.
#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses((status = 200, description = "Angemeldet; Session als HttpOnly-Cookie", body = User), (status = 401, description = "Benutzername oder Passwort falsch", body = crate::error::ErrorBody)),
)]
pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Result<Response> {
    auth::check_throttle(&state, &body.username)?;
    let pool = state.db.system().inner();

    let verified: VerifiedIdentity = match state
        .auth
        .authenticate(
            pool,
            Credential::Password {
                username: &body.username,
                password: &body.password,
            },
        )
        .await
    {
        Ok(v) => v,
        Err(e) => {
            auth::record_failure(&state, &body.username);
            return Err(e);
        }
    };
    auth::clear_failures(&state, &body.username);

    let user = link_identity(pool, &state.config, &verified).await?;
    let cookie = issue_session(
        pool,
        &state.config,
        user.id,
        state.auth.id(),
        user_agent(&headers),
    )
    .await?;
    Ok(with_cookie(cookie, StatusCode::OK, user))
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    tag = "auth",
    responses((status = 204, description = "Sitzung beendet")),
)]
pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    if let Some(token) = auth::cookie_value(&headers, auth::COOKIE) {
        sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(auth::token_hash(token, &state.config.session_secret))
            .execute(state.db.system().inner())
            .await?;
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, clear_cookie(&state.config))],
    )
        .into_response())
}

#[utoipa::path(
    get,
    path = "/api/v1/auth/me",
    tag = "auth",
    responses((status = 200, description = "Die angemeldete Person", body = User), (status = 401, description = "Nicht angemeldet", body = crate::error::ErrorBody)),
)]
pub async fn me(SessionUser(user): SessionUser) -> Json<User> {
    Json(user)
}

#[utoipa::path(
    put,
    path = "/api/v1/auth/password",
    tag = "auth",
    request_body = PasswordRequest,
    responses((status = 204, description = "Passwort geändert, alle Sitzungen beendet"), (status = 401, description = "Aktuelles Passwort falsch", body = crate::error::ErrorBody)),
)]
pub async fn change_password(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Json(body): Json<PasswordRequest>,
) -> Result<StatusCode> {
    state
        .auth
        .change_password(
            state.db.system().inner(),
            user.id,
            &body.current_password,
            &body.new_password,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/users",
    tag = "admin",
    responses((status = 200, description = "Alle Konten", body = Vec<User>), (status = 403, description = "Nur für Administratoren", body = crate::error::ErrorBody)),
)]
pub async fn list_users(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
) -> Result<Json<Vec<User>>> {
    require_admin(&user)?;
    let rows = sqlx::query(
        "SELECT id, username, display_name, is_admin FROM users ORDER BY lower(username)",
    )
    .fetch_all(state.db.system().inner())
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| User {
                id: r.get("id"),
                username: r.get("username"),
                display_name: r.get("display_name"),
                is_admin: r.get("is_admin"),
            })
            .collect(),
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/users",
    tag = "admin",
    request_body = CreateUserRequest,
    responses((status = 201, description = "Konto angelegt", body = User), (status = 409, description = "Benutzername vergeben", body = crate::error::ErrorBody)),
)]
pub async fn create_user(
    State(state): State<AppState>,
    SessionUser(actor): SessionUser,
    Json(body): Json<CreateUserRequest>,
) -> Result<Response> {
    require_admin(&actor)?;
    if body.username.trim().is_empty() {
        return Err(AppError::Validation("Benutzername fehlt".into()));
    }
    let pool = state.db.system().inner();
    let id = Uuid::new_v4();
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO users (id, username, display_name, is_admin) VALUES ($1,$2,$3,$4)")
        .bind(id)
        .bind(body.username.trim())
        .bind(body.display_name.trim())
        .bind(body.is_admin)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::from_db(e, "Benutzername ist bereits vergeben"))?;
    sqlx::query(
        "INSERT INTO identities (id, user_id, provider, subject) VALUES ($1,$2,'local',$3)",
    )
    .bind(Uuid::new_v4())
    .bind(id)
    .bind(id.to_string())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    state
        .auth
        .create_credential(pool, id, &body.password)
        .await?;
    seed_new_user(pool, id).await?;

    Ok((
        StatusCode::CREATED,
        Json(User {
            id,
            username: body.username.trim().to_string(),
            display_name: body.display_name.trim().to_string(),
            is_admin: body.is_admin,
        }),
    )
        .into_response())
}
