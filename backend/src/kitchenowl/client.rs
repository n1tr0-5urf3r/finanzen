//! HTTP transport for KitchenOwl. Knows about requests, status codes and timeouts;
//! knows nothing about JSON shapes, which all live in [`super::wire`].
//!
//! Every failure leaves here as [`AppError::Integration`], which is the only path a
//! KitchenOwl problem takes to a client — a 502, never a 500 and never a silent
//! empty list.

use std::sync::Arc;

use reqwest::StatusCode;
use serde_json::Value;

use crate::{
    AppState,
    config::Config,
    error::{AppError, Result},
};

use super::wire;

#[derive(Clone)]
pub struct KoClient {
    http: reqwest::Client,
    config: Arc<Config>,
    household: Arc<tokio::sync::OnceCell<i64>>,
}

impl KoClient {
    /// `None` when KitchenOwl is not configured. Every caller treats that as
    /// "the feature is off", never as an error.
    pub fn from_state(state: &AppState) -> Option<Self> {
        state.config.kitchenowl_url.as_ref()?;
        state.config.kitchenowl_token.as_ref()?;
        Some(Self {
            http: state.http.clone(),
            config: state.config.clone(),
            household: state.ko_household.clone(),
        })
    }

    fn base(&self) -> &str {
        self.config
            .kitchenowl_url
            .as_deref()
            .expect("checked in from_state")
    }

    fn token(&self) -> &str {
        self.config
            .kitchenowl_token
            .as_deref()
            .expect("checked in from_state")
    }

    async fn get(&self, path: &str) -> Result<Vec<u8>> {
        let url = format!("{}{path}", self.base());
        let response = self
            .http
            .get(&url)
            .bearer_auth(self.token())
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| unreachable(path, e))?;
        read_body(path, response).await
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Vec<u8>> {
        let url = format!("{}{path}", self.base());
        let response = self
            .http
            .post(&url)
            .bearer_auth(self.token())
            .header(reqwest::header::ACCEPT, "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| unreachable(path, e))?;
        read_body(path, response).await
    }

    /// The household id, resolved **once per process**.
    ///
    /// schmauserei re-resolves this on every outbound call, which turns each push
    /// into two round trips and makes a flaky network fail the wrong request. The
    /// `OnceCell` lives on `AppState` so the resolution is shared.
    pub async fn household_id(&self) -> Result<i64> {
        if let Some(id) = self.config.kitchenowl_household_id {
            return Ok(id);
        }
        self.household
            .get_or_try_init(|| async {
                let households = wire::parse_households(&self.get("/api/household").await?)?;
                households
                    .iter()
                    .find(|h| h.expenses_feature)
                    .or_else(|| households.first())
                    .map(|h| h.id)
                    .ok_or_else(|| {
                        AppError::Integration(
                            "KitchenOwl meldet keinen Haushalt für dieses Token".into(),
                        )
                    })
            })
            .await
            .copied()
    }

    pub async fn households(&self) -> Result<Vec<wire::RawHousehold>> {
        wire::parse_households(&self.get("/api/household").await?)
    }

    /// The member id to treat as "me". There is no `is_me` flag on the member
    /// objects, so it comes from the token's own user.
    pub async fn me(&self) -> Result<i64> {
        Ok(wire::parse_user(&self.get("/api/user").await?)?.id)
    }

    pub async fn categories(&self) -> Result<Vec<wire::RawExpenseCategory>> {
        let id = self.household_id().await?;
        wire::parse_categories(
            &self
                .get(&format!("/api/household/{id}/expense/categories"))
                .await?,
        )
    }

    /// One page of expenses: 30, ordered by date descending. `after` is the id of
    /// the **last item of the previous page in that order** — see
    /// [`wire::next_cursor`] for why it is not the lowest id.
    pub async fn expense_page(&self, after: Option<i64>) -> Result<Vec<wire::RawExpense>> {
        let id = self.household_id().await?;
        let path = match after {
            Some(cursor) => format!("/api/household/{id}/expense?startAfterId={cursor}"),
            None => format!("/api/household/{id}/expense"),
        };
        wire::parse_expenses(&self.get(&path).await?)
    }

    /// One expense by its KitchenOwl id.
    ///
    /// `/api/expense/{id}` — NOT `/api/household/{hid}/expense/{id}`, which answers
    /// 404: that path serves the collection only. Verified against the live
    /// instance, where `OPTIONS /api/expense/{id}` reports
    /// `Allow: POST, OPTIONS, GET, HEAD, DELETE`.
    pub async fn expense(&self, expense_id: i64) -> Result<wire::RawExpense> {
        wire::parse_expense(&self.get(&format!("/api/expense/{expense_id}")).await?)
    }

    /// Files an existing expense under a category, and answers with the expense as
    /// KitchenOwl holds it afterwards.
    ///
    /// Read, rebuild, write, read again. The first read is what makes the write
    /// safe: `POST` replaces the expense, so the body has to carry every field, and
    /// the only trustworthy source for those fields is KitchenOwl itself a moment
    /// earlier — not the mirror, which is a rounded, re-dated projection of it.
    ///
    /// The second read is what makes the result honest. `create_expense` already
    /// learned that this API's write responses are thinner than its reads, so the
    /// caller is handed a re-fetched object and the mirror is updated from that
    /// rather than from an assumption about what the write did.
    ///
    /// This is the only call in the project that modifies existing household data,
    /// and it may modify exactly one field.
    pub async fn update_expense_category(
        &self,
        expense_id: i64,
        ko_category_id: Option<i64>,
    ) -> Result<wire::RawExpense> {
        let before = self.expense(expense_id).await?;
        let body = wire::recategorize_body(&before, ko_category_id);
        self.post(&format!("/api/expense/{expense_id}"), &body)
            .await?;
        self.expense(expense_id).await
    }

    pub async fn create_expense(&self, body: &Value) -> Result<wire::RawExpense> {
        let id = self.household_id().await?;
        let raw = self
            .post(&format!("/api/household/{id}/expense"), body)
            .await?;
        // VERIFY: the source suggests the create response does not echo `paid_for`.
        // Nothing here depends on it — only `id` is read, and the mirror is
        // refreshed by the next pull.
        wire::parse_expense(&raw)
    }
}

fn unreachable(path: &str, e: reqwest::Error) -> AppError {
    let why = if e.is_timeout() {
        "Zeitüberschreitung".to_string()
    } else if e.is_connect() {
        "keine Verbindung".to_string()
    } else {
        e.to_string()
    };
    AppError::Integration(format!("KitchenOwl {path}: {why}"))
}

/// Turns a response into bytes, or into a German integration error carrying the
/// first line of whatever the server actually said.
///
/// The bodies are not JSON. `?limit=` answers `400` with `text/html` and the literal
/// text `Request invalid`; an unknown path answers `404` with `Requested resource
/// not found`; a bad token answers **422** (not 401) with `{"msg": …}`. Reading any
/// of those as JSON panics a naive client, so nothing here parses before the status
/// has been checked.
async fn read_body(path: &str, response: reqwest::Response) -> Result<Vec<u8>> {
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|e| AppError::Integration(format!("KitchenOwl {path}: {e}")))?;
    if status.is_success() {
        return Ok(bytes.to_vec());
    }
    Err(AppError::Integration(format!(
        "KitchenOwl {path}: {} {}",
        status.as_u16(),
        describe(status, &bytes)
    )))
}

fn describe(status: StatusCode, body: &[u8]) -> String {
    // A JSON error body from this API is `{"msg": "..."}`; everything else is
    // plain text that happens to be served as text/html.
    if let Ok(value) = serde_json::from_slice::<Value>(body)
        && let Some(msg) = value.get("msg").and_then(Value::as_str)
    {
        return msg.to_string();
    }
    let text = String::from_utf8_lossy(body);
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let trimmed: String = first.trim().chars().take(200).collect();
    if trimmed.is_empty() {
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::UNPROCESSABLE_ENTITY => "Token abgelehnt".into(),
            _ => "keine Antwort".into(),
        }
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_text_error_body_is_reported_verbatim() {
        // The two bodies the live instance actually returns.
        assert_eq!(
            describe(StatusCode::BAD_REQUEST, b"Request invalid"),
            "Request invalid"
        );
        assert_eq!(
            describe(StatusCode::NOT_FOUND, b"Requested resource not found"),
            "Requested resource not found"
        );
    }

    #[test]
    fn a_json_error_body_reports_its_message() {
        assert_eq!(
            describe(
                StatusCode::UNPROCESSABLE_ENTITY,
                br#"{"msg":"Not enough segments"}"#
            ),
            "Not enough segments"
        );
    }

    #[test]
    fn an_html_error_page_does_not_become_a_wall_of_markup() {
        let html = b"<!doctype html>\n<html><body>very long error page</body></html>";
        let described = describe(StatusCode::BAD_GATEWAY, html);
        assert!(described.len() <= 200);
        assert_eq!(described, "<!doctype html>");
    }

    #[test]
    fn an_empty_body_still_says_something_useful() {
        assert_eq!(describe(StatusCode::UNAUTHORIZED, b""), "Token abgelehnt");
        assert_eq!(describe(StatusCode::BAD_GATEWAY, b""), "keine Antwort");
    }
}
