use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use utoipa::ToSchema;

pub type Result<T, E = AppError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Nicht angemeldet")]
    Unauthorized,
    #[error("Zugriff verweigert")]
    Forbidden,
    #[error("Nicht gefunden: {0}")]
    NotFound(String),
    #[error("Ungültige Eingabe: {0}")]
    Validation(String),
    #[error("Konflikt: {0}")]
    Conflict(String),
    /// The request itself is well-formed but its content cannot be processed —
    /// e.g. an import whose month-label sequence contradicts itself. Neither a 400
    /// (the request is fine) nor a 409 (nothing is racing).
    #[error("Nicht verarbeitbar: {0}")]
    Unprocessable(String),
    #[error("Externer Dienst nicht verfügbar: {0}")]
    Integration(String),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl AppError {
    /// Maps Postgres SQLSTATEs that carry domain meaning onto domain errors, so a
    /// constraint the schema enforces surfaces as a 409/400 rather than a 500.
    ///
    /// - `23505` unique_violation  -> Conflict (e.g. a duplicate rule match_key)
    /// - `23514` check_violation   -> Validation (e.g. a booking with no date)
    /// - `23503` foreign_key       -> Validation (e.g. an unknown categoryId)
    /// - `42501` insufficient_priv -> Forbidden (an RLS WITH CHECK rejection)
    pub fn from_db(error: sqlx::Error, context: &str) -> Self {
        let Some(db) = error.as_database_error() else {
            return Self::Database(error);
        };
        match db.code().as_deref() {
            Some("23505") => Self::Conflict(context.to_string()),
            Some("23514") | Some("23503") => Self::Validation(context.to_string()),
            Some("42501") => Self::Forbidden,
            _ => Self::Database(error),
        }
    }

    fn parts(&self) -> (StatusCode, &'static str) {
        match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            Self::Validation(_) => (StatusCode::BAD_REQUEST, "validation_error"),
            Self::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            Self::Unprocessable(_) => (StatusCode::UNPROCESSABLE_ENTITY, "unprocessable"),
            Self::Integration(_) => (StatusCode::BAD_GATEWAY, "integration_error"),
            Self::Database(_) | Self::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
            }
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = self.parts();
        let message = if status == StatusCode::INTERNAL_SERVER_ERROR {
            // Never leak internals to a client; the detail goes to the log instead.
            tracing::error!(error = %self, "request failed");
            "Interner Serverfehler".to_string()
        } else {
            self.to_string()
        };
        (
            status,
            Json(ErrorBody {
                code: code.to_string(),
                message,
                details: None,
            }),
        )
            .into_response()
    }
}
