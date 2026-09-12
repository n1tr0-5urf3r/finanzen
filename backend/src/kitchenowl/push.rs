//! The push outbox.
//!
//! `POST /bookings/{id}/kitchenowl` commits the intent and its payload **before any
//! HTTP happens** and answers 202 even when KitchenOwl is down. The network attempt
//! is best-effort; the retry loop is the safety net. That ordering is the whole
//! design: a request that only succeeds when the far end is up is a request the user
//! has to remember to repeat.
//!
//! Idempotency has three layers, because each covers a case the others cannot:
//!
//! 1. `PRIMARY KEY (booking_id)` on `ko_push_intents` — editing a queued push
//!    replaces it in place instead of becoming a second post.
//! 2. A `#fin:<short-id>` marker in the description (or the name, per
//!    `KITCHENOWL_PUSH_MARKER_IN_NAME`).
//! 3. **Reconcile before post** — when an attempt has already been made, scan for
//!    the marker rather than posting again. This is the layer that covers the
//!    genuinely ambiguous case: the POST timed out, so the expense may or may not
//!    exist, and only KitchenOwl can say which.

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::{
    config::Config,
    error::{AppError, Result},
    locale::period_ord,
};

use super::{
    client::KoClient,
    wire::{self, PushPayload, ShareJson},
};

/// Retry backoff, capped. An hour is long enough that a week-long outage costs a
/// handful of requests and short enough that a fixed instance drains the queue
/// without anyone pressing anything.
const MAX_BACKOFF_SECONDS: i64 = 3600;

#[derive(Debug, Clone)]
pub struct Intent {
    pub booking_id: Uuid,
    pub state: String,
    pub marker: String,
    pub payload: PushPayload,
    pub attempts: i32,
    pub external_id: Option<i64>,
}

fn load_payload(raw: serde_json::Value) -> Result<PushPayload> {
    serde_json::from_value(raw)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Push-Payload nicht lesbar: {e}")))
}

fn row_to_intent(r: &sqlx::postgres::PgRow) -> Result<Intent> {
    Ok(Intent {
        booking_id: r.get("booking_id"),
        state: r.get("state"),
        marker: r.get("marker"),
        payload: load_payload(r.get("payload"))?,
        attempts: r.get("attempts"),
        external_id: r.get("external_id"),
    })
}

/// Writes the intent. The caller commits, then returns 202.
pub async fn queue(
    conn: &mut PgConnection,
    user_id: Uuid,
    booking_id: Uuid,
    payload: &PushPayload,
) -> Result<()> {
    let value = serde_json::to_value(payload)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Push-Payload: {e}")))?;
    sqlx::query(
        "INSERT INTO ko_push_intents (booking_id, user_id, state, idempotency_key, marker, \
                                      payload, attempts, last_error, next_attempt_at, updated_at) \
         VALUES ($1,$2,'queued',$3,$3,$4,0,NULL,now(),now()) \
         ON CONFLICT (booking_id) DO UPDATE SET \
           state = 'queued', payload = EXCLUDED.payload, attempts = 0, last_error = NULL, \
           next_attempt_at = now(), attempt_started_at = NULL, updated_at = now()",
    )
    .bind(booking_id)
    .bind(user_id)
    .bind(&payload.marker)
    .bind(&value)
    .execute(&mut *conn)
    .await
    .map_err(|e| AppError::from_db(e, "Push konnte nicht vorgemerkt werden"))?;
    Ok(())
}

/// Intents ready for an attempt.
///
/// A `sending` row older than twice the HTTP timeout is included, because that is
/// exactly the ambiguous case: the process died or the network hung mid-post and
/// nobody knows whether the expense exists. It is picked up to be **reconciled**,
/// not to be re-posted.
pub async fn due(conn: &mut PgConnection, http_timeout_seconds: i64) -> Result<Vec<Intent>> {
    let rows = sqlx::query(
        "SELECT booking_id, state, marker, payload, attempts, external_id \
           FROM ko_push_intents \
          WHERE (state = 'queued' AND (next_attempt_at IS NULL OR next_attempt_at <= now())) \
             OR (state = 'failed' AND next_attempt_at IS NOT NULL AND next_attempt_at <= now()) \
             OR (state = 'sending' AND attempt_started_at IS NOT NULL \
                 AND attempt_started_at < now() - make_interval(secs => $1)) \
          ORDER BY created_at LIMIT 25",
    )
    .bind((http_timeout_seconds * 2) as f64)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter().map(row_to_intent).collect()
}

pub async fn load_one(conn: &mut PgConnection, booking_id: Uuid) -> Result<Option<Intent>> {
    let row = sqlx::query(
        "SELECT booking_id, state, marker, payload, attempts, external_id \
           FROM ko_push_intents WHERE booking_id = $1",
    )
    .bind(booking_id)
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref().map(row_to_intent).transpose()
}

async fn mark_sending(conn: &mut PgConnection, booking_id: Uuid) -> Result<i32> {
    let attempts: i32 = sqlx::query_scalar(
        "UPDATE ko_push_intents SET state = 'sending', attempts = attempts + 1, \
                attempt_started_at = now(), updated_at = now() \
          WHERE booking_id = $1 RETURNING attempts",
    )
    .bind(booking_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(attempts)
}

async fn mark_pushed(conn: &mut PgConnection, booking_id: Uuid, external_id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE ko_push_intents SET state = 'pushed', external_id = $2, last_error = NULL, \
                next_attempt_at = NULL, updated_at = now() WHERE booking_id = $1",
    )
    .bind(booking_id)
    .bind(external_id)
    .execute(&mut *conn)
    .await?;
    // The link itself. `bookings.external_source`/`external_id` is the single source
    // of truth — it carries the partial unique index that makes a later pull of the
    // same expense a structural no-op. `ko_expenses.linked_booking_id` is a
    // denormalised copy, written here so it can never disagree.
    super::link::attach(conn, booking_id, external_id).await
}

/// Records a failure. `abandoned` after `KITCHENOWL_PUSH_MAX_ATTEMPTS`, and the last
/// error stays on the row in every state — a queue item that failed silently is a
/// booking the user believes is in KitchenOwl and is not.
async fn mark_failed(
    conn: &mut PgConnection,
    booking_id: Uuid,
    attempts: i32,
    max_attempts: i32,
    retry_seconds: u64,
    error: &str,
) -> Result<()> {
    let state = if attempts >= max_attempts {
        "abandoned"
    } else {
        "failed"
    };
    sqlx::query(
        "UPDATE ko_push_intents SET state = $2, last_error = $3, next_attempt_at = $4, \
                updated_at = now() WHERE booking_id = $1",
    )
    .bind(booking_id)
    .bind(state)
    .bind(error)
    .bind(next_attempt(attempts, retry_seconds))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub fn next_attempt(attempts: i32, retry_seconds: u64) -> DateTime<Utc> {
    Utc::now() + Duration::seconds(backoff_seconds(attempts, retry_seconds))
}

pub fn backoff_seconds(attempts: i32, retry_seconds: u64) -> i64 {
    let base = retry_seconds.max(1) as i64;
    let shift = attempts.clamp(1, 20) - 1;
    base.saturating_mul(1i64 << shift.min(12))
        .min(MAX_BACKOFF_SECONDS)
}

/// One attempt for one intent.
///
/// Returns the external id on success. Every outcome is written to the intent row
/// before returning, so a caller that drops the future still leaves a row that says
/// what happened.
pub async fn attempt(
    client: &KoClient,
    conn: &mut PgConnection,
    config: &Config,
    intent: &Intent,
) -> Result<Option<i64>> {
    let attempts = mark_sending(conn, intent.booking_id).await?;

    // Reconcile before post. From the second attempt onwards the previous one may
    // have succeeded and lost its answer, so ask before creating.
    if attempts > 1 || intent.state == "sending" {
        match reconcile(client, config, intent).await {
            Ok(Some(existing)) => {
                tracing::info!(
                    booking = %intent.booking_id,
                    expense = existing,
                    "Bestehende KitchenOwl-Ausgabe übernommen statt doppelt zu buchen"
                );
                mark_pushed(conn, intent.booking_id, existing).await?;
                return Ok(Some(existing));
            }
            Ok(None) => {}
            Err(e) => {
                // If the reconcile scan itself fails we must NOT fall through to a
                // post: that is precisely the double-booking this guards against.
                mark_failed(
                    conn,
                    intent.booking_id,
                    attempts,
                    config.kitchenowl_push_max_attempts,
                    config.kitchenowl_push_retry_seconds,
                    &format!("Abgleich vor dem Senden fehlgeschlagen: {e}"),
                )
                .await?;
                return Err(e);
            }
        }
    }

    let body = wire::push_body(&intent.payload);
    match client.create_expense(&body).await {
        Ok(created) => {
            mark_pushed(conn, intent.booking_id, created.id).await?;
            Ok(Some(created.id))
        }
        Err(e) => {
            mark_failed(
                conn,
                intent.booking_id,
                attempts,
                config.kitchenowl_push_max_attempts,
                config.kitchenowl_push_retry_seconds,
                &e.to_string(),
            )
            .await?;
            Err(e)
        }
    }
}

/// Looks for an expense this application already created, by marker.
///
/// Pages are ordered by **date** descending, not by id, so a pushed expense dated
/// three months ago is not near the top. The scan therefore walks pages until it has
/// passed the payload's own date rather than looking at "the top few".
///
/// Running out of pages is an **error**, not a "not found". The whole purpose of the
/// scan is to answer a question the caller cannot otherwise answer — did the previous
/// attempt land? — and a truncated scan has not answered it. Returning `None` there
/// would let the caller post a second time, which is the one outcome this whole
/// mechanism exists to prevent. Raising the cap is the fix; guessing is not.
async fn reconcile(client: &KoClient, config: &Config, intent: &Intent) -> Result<Option<i64>> {
    let target = intent.payload.date;
    let mut cursor: Option<i64> = None;
    let mut pages = 0u32;
    while pages < config.kitchenowl_max_pull_pages {
        let page = client.expense_page(cursor).await?;
        pages += 1;
        // The end of the household: a complete answer, the expense is not there.
        if page.is_empty() {
            return Ok(None);
        }
        if let Some(found) = wire::find_marked(&page, &intent.marker) {
            return Ok(Some(found));
        }
        // Past the target date by a day of slack — the expense cannot be below here.
        if let Some(oldest) = page.last()
            && let Ok(date) = wire::date_from_epoch_ms(oldest.date)
            && date < target - Duration::days(1)
        {
            return Ok(None);
        }
        match wire::next_cursor(&page, cursor) {
            Some(c) => cursor = Some(c),
            // The list ended before the target date. Also complete.
            None => return Ok(None),
        }
    }
    Err(AppError::Integration(format!(
        "Abgleich vor dem Senden nach {} Seiten abgebrochen, ohne das Buchungsdatum zu \
         erreichen. Es ist unklar, ob die Ausgabe bereits existiert — bitte \
         KITCHENOWL_MAX_PULL_PAGES erhöhen.",
        config.kitchenowl_max_pull_pages
    )))
}

// --------------------------------------------------------------- payloads

/// Builds the payload from a booking plus the dialogue's choices, validating every
/// reference against the cached metadata.
#[allow(clippy::too_many_arguments)]
pub async fn build_payload(
    conn: &mut PgConnection,
    booking_id: Uuid,
    name: Option<String>,
    description: Option<String>,
    amount_cents: Option<i64>,
    date: Option<NaiveDate>,
    ko_category_id: Option<i64>,
    paid_by: Option<i64>,
    paid_for: Vec<(i64, i64)>,
    marker_in_name: bool,
) -> Result<PushPayload> {
    let row = sqlx::query(
        "SELECT comment, amount_cents, kind, booked_on, period_year, period_month, \
                external_source, status \
           FROM bookings WHERE id = $1",
    )
    .bind(booking_id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| AppError::NotFound("Buchung".into()))?;

    if row.get::<Option<String>, _>("external_source").is_some() {
        return Err(AppError::Conflict(
            "Diese Buchung ist bereits mit KitchenOwl verknüpft".into(),
        ));
    }
    if row.get::<String, _>("status") != "confirmed" {
        return Err(AppError::Validation(
            "Nur bestätigte Buchungen können nach KitchenOwl übertragen werden".into(),
        ));
    }
    if row.get::<String, _>("kind") == "transfer" {
        return Err(AppError::Validation(
            "Umbuchungen gehören nicht in die geteilte Haushaltskasse".into(),
        ));
    }

    let amount = amount_cents.unwrap_or_else(|| row.get("amount_cents"));
    if amount <= 0 {
        return Err(AppError::Validation("Der Betrag muss positiv sein".into()));
    }
    let name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| row.get::<String, _>("comment"));

    let date = match date.or_else(|| row.get::<Option<NaiveDate>, _>("booked_on")) {
        Some(d) => d,
        // Imported history is month-only. Day one of the period is a deliberate,
        // visible choice rather than a fabricated "today".
        None => NaiveDate::from_ymd_opt(
            row.get::<i16, _>("period_year") as i32,
            row.get::<i16, _>("period_month") as u32,
            1,
        )
        .ok_or_else(|| AppError::Validation("Buchungsmonat ist ungültig".into()))?,
    };

    let me: Option<i64> =
        sqlx::query_scalar("SELECT member_id FROM ko_members WHERE is_me LIMIT 1")
            .fetch_optional(&mut *conn)
            .await?;
    let paid_by = paid_by.or(me).ok_or_else(|| {
        AppError::Validation(
            "Kein KitchenOwl-Mitglied bekannt — bitte zuerst synchronisieren".into(),
        )
    })?;

    let known: Vec<i64> = sqlx::query_scalar("SELECT member_id FROM ko_members")
        .fetch_all(&mut *conn)
        .await?;
    if !known.contains(&paid_by) {
        return Err(AppError::Validation(
            "„Bezahlt von“ ist kein Mitglied dieses Haushalts".into(),
        ));
    }
    let paid_for = if paid_for.is_empty() {
        known.iter().map(|m| (*m, 1)).collect::<Vec<_>>()
    } else {
        paid_for
    };
    if paid_for.is_empty() {
        return Err(AppError::Validation(
            "Mindestens eine Person muss beteiligt sein".into(),
        ));
    }
    for (member, factor) in &paid_for {
        if !known.contains(member) {
            return Err(AppError::Validation(
                "Eine beteiligte Person gehört nicht zu diesem Haushalt".into(),
            ));
        }
        if *factor < 1 {
            return Err(AppError::Validation(
                "Ein Anteil muss mindestens 1 sein".into(),
            ));
        }
    }

    if let Some(cat) = ko_category_id {
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT category_id FROM ko_categories WHERE category_id = $1")
                .bind(cat)
                .fetch_optional(&mut *conn)
                .await?;
        if exists.is_none() {
            return Err(AppError::Validation(
                "Diese KitchenOwl-Kategorie ist unbekannt".into(),
            ));
        }
    }

    let shares = wire::allocate_shares(amount, &paid_for);
    Ok(PushPayload {
        name,
        amount_cents: amount,
        date,
        description: description
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty()),
        ko_category_id,
        paid_by_id: paid_by,
        paid_for: shares
            .iter()
            .map(|s| ShareJson {
                member_id: s.member_id,
                factor: s.factor,
                share_cents: s.share_cents,
            })
            .collect(),
        marker: wire::push_marker(booking_id),
        marker_in_name,
    })
}

/// The booking's period, so the caller can invalidate the right year.
pub fn payload_ord(payload: &PushPayload) -> i32 {
    period_ord(payload.date.year(), payload.date.month() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backoff_grows_and_is_capped() {
        assert_eq!(backoff_seconds(1, 300), 300);
        assert_eq!(backoff_seconds(2, 300), 600);
        assert_eq!(backoff_seconds(3, 300), 1200);
        assert_eq!(backoff_seconds(10, 300), MAX_BACKOFF_SECONDS);
        // A zero interval must not become a zero delay and a hot loop.
        assert!(backoff_seconds(1, 0) >= 1);
    }
}
