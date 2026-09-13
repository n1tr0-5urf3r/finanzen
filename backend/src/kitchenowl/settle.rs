//! Settling up with the household.
//!
//! This is the ONE place in the module where a KitchenOwl figure is allowed to
//! cause a personal booking, and it happens only on an explicit `POST`. A pull
//! still never writes a booking; nothing here runs on a timer.
//!
//! Three decisions carry the whole design:
//!
//! **The balance is passed through, not reinterpreted.** KitchenOwl's
//! `expense_balance` is already a flow from the user's side: negative means the
//! user owes the household. The direction word is decided here, next to the
//! figure, so the two cannot drift apart — a screen that read "+142,27 € · you owe
//! the household" has been shipped once already.
//!
//! **A settlement is a `transfer`.** The household's purchases are in the personal
//! ledger at FULL value — verified against the real data: Kaufland 19,07, Kino
//! 22,50, Europapark 158,60 all match KitchenOwl's full amount — so the money paid
//! in a settlement has largely been counted already. Booking it as an expense would
//! count it twice. As a transfer its `net_cents` is 0 by construction, it belongs to
//! no category, and no analysis figure moves. The year's balance does not move
//! either, and the UI says so rather than letting the user wait for a change that
//! is not coming.
//!
//! **Idempotency is structural.** The booking carries
//! `external_source = 'kitchenowl_settlement'` and the period as its `external_id`,
//! so `bookings_external_key` — the same partial unique index that makes re-linking
//! an expense a no-op — makes a second settlement for a month impossible at the
//! database, not at the handler.

use axum::{Json, http::StatusCode};
use chrono::{Datelike, Utc};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::{
    auth::Ctx,
    bookings::{SELECT_BOOKING, assert_year_unlocked, row_to_booking},
    error::{AppError, Result},
    locale::month_name_de,
    models::{KoSettlement, Period},
};

/// Deliberately NOT `link::SOURCE`. A settlement references a balance, not an
/// expense row, so mixing the two would make `unlink` offer to detach something
/// that was never attached to anything.
pub const SOURCE: &str = "kitchenowl_settlement";

/// The period, as the external key. Sortable, readable in a database dump, and
/// stable across a rename of anything else.
fn external_key(period: Period) -> String {
    format!("{:04}-{:02}", period.year, period.month)
}

fn current_period() -> Period {
    let today = Utc::now().date_naive();
    Period {
        year: today.year(),
        month: today.month() as u8,
    }
}

/// Negative is the user owing the household. Stated once, used everywhere.
fn direction_of(balance: Option<i64>) -> &'static str {
    match balance {
        None => "unknown",
        Some(0) => "settled",
        Some(b) if b < 0 => "i_owe",
        Some(_) => "household_owes_me",
    }
}

/// My row in the mirror. `None` when metadata has never been fetched, which is a
/// normal state on a fresh instance rather than an error.
async fn my_balance(conn: &mut PgConnection) -> Result<Option<i64>> {
    let row = sqlx::query("SELECT balance_cents FROM ko_members WHERE is_me LIMIT 1")
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.map(|r| r.get::<i64, _>("balance_cents")))
}

/// What was recorded when this period was settled, if it was.
async fn snapshot_for(
    conn: &mut PgConnection,
    period: Period,
) -> Result<(Option<i64>, Option<chrono::DateTime<Utc>>)> {
    let row = sqlx::query(
        "SELECT balance_snapshot -> $1 ->> 'balanceCents' AS balance, \
                balance_snapshot -> $1 ->> 'settledAt'    AS at \
           FROM ko_sync_state LIMIT 1",
    )
    .bind(external_key(period))
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else {
        return Ok((None, None));
    };
    let balance = row
        .get::<Option<String>, _>("balance")
        .and_then(|v| v.parse::<i64>().ok());
    let at = row
        .get::<Option<String>, _>("at")
        .and_then(|v| chrono::DateTime::parse_from_rfc3339(&v).ok())
        .map(|v| v.with_timezone(&Utc));
    Ok((balance, at))
}

async fn view(conn: &mut PgConnection, period: Period) -> Result<KoSettlement> {
    let balance = my_balance(&mut *conn).await?;

    let existing = sqlx::query(&format!(
        "{SELECT_BOOKING} WHERE b.external_source = $1 AND b.external_id = $2"
    ))
    .bind(SOURCE)
    .bind(external_key(period))
    .fetch_optional(&mut *conn)
    .await?;

    let (settled_balance_cents, settled_at) = snapshot_for(&mut *conn, period).await?;

    Ok(KoSettlement {
        balance_cents: balance,
        direction: direction_of(balance).to_string(),
        amount_cents: balance.unwrap_or(0).abs(),
        period,
        suggested_comment: format!("Ausgleich {}", month_name_de(period.month)),
        already_settled: existing.is_some(),
        booking: existing.as_ref().map(row_to_booking),
        settled_balance_cents,
        settled_at,
    })
}

/// What settling up would book, and whether it already has been.
#[utoipa::path(
    get,
    path = "/api/v1/kitchenowl/settlement",
    tag = "kitchenowl",
    responses((status = 200, description = "Saldo, Richtung und der Vorschlag — aus dem Spiegel, ohne HTTP", body = KoSettlement)),
)]
pub async fn settlement(mut ctx: Ctx) -> Result<Json<KoSettlement>> {
    let out = view(ctx.tenant.conn(), current_period()).await?;
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

/// Books the settlement in the PERSONAL ledger. Writes nothing to KitchenOwl.
///
/// Returns 201 the first time and 200 with the same booking afterwards: asking
/// twice is what a double tap or a retried request looks like, and neither should
/// produce a second row.
#[utoipa::path(
    post,
    path = "/api/v1/kitchenowl/settlement",
    tag = "kitchenowl",
    responses(
        (status = 201, description = "Ausgleich als Umbuchung gebucht", body = KoSettlement),
        (status = 200, description = "Für diesen Monat bereits gebucht; dieselbe Buchung", body = KoSettlement),
        (status = 422, description = "Nichts auszugleichen", body = crate::error::ErrorBody),
    ),
)]
pub async fn settle(mut ctx: Ctx) -> Result<(StatusCode, Json<KoSettlement>)> {
    let period = current_period();
    let current = view(ctx.tenant.conn(), period).await?;

    // Already done. Not an error — the second press of a button is a normal event.
    if current.already_settled {
        ctx.tenant.commit().await?;
        return Ok((StatusCode::OK, Json(current)));
    }

    let Some(balance) = current.balance_cents else {
        return Err(AppError::Unprocessable(
            "Kein KitchenOwl-Saldo im Spiegel — erst synchronisieren".into(),
        ));
    };
    if balance == 0 {
        return Err(AppError::Unprocessable(
            "Der Haushalt ist bereits ausgeglichen".into(),
        ));
    }

    assert_year_unlocked(ctx.tenant.conn(), period.year).await?;

    let id = Uuid::new_v4();
    let booked_on = chrono::NaiveDate::from_ymd_opt(period.year, period.month as u32, 1);
    // No category, and `category_source = 'unresolved'` to satisfy the state
    // machine: a transfer legitimately has none, and `calc::totals` excludes
    // categoryless transfers from the uncategorised count for exactly this reason,
    // so booking one does not put a badge on the dashboard that cannot be cleared.
    sqlx::query(
        "INSERT INTO bookings (id, user_id, period_year, period_month, booked_on, kind, \
                               amount_cents, comment, tax_relevant, category_id, \
                               category_source, resolved_rule_id, origin, \
                               external_source, external_id) \
         VALUES ($1,$2,$3::smallint,$4::smallint,$5,'transfer',$6,$7,false,NULL, \
                 'unresolved',NULL,'kitchenowl',$8,$9)",
    )
    .bind(id)
    .bind(ctx.tenant.user_id())
    .bind(period.year)
    .bind(period.month as i16)
    .bind(booked_on)
    .bind(balance.abs())
    .bind(&current.suggested_comment)
    .bind(SOURCE)
    .bind(external_key(period))
    .execute(ctx.tenant.conn())
    .await
    .map_err(|e| AppError::from_db(e, "Der Ausgleich konnte nicht gebucht werden"))?;

    // The balance this was based on, kept because it stops being visible the moment
    // the next expense lands: without it, "settled in September" says nothing about
    // what was owed in September.
    let snapshot = serde_json::json!({
        external_key(period): {
            "balanceCents": balance,
            "bookingId": id,
            "settledAt": Utc::now().to_rfc3339(),
        }
    });
    sqlx::query("UPDATE ko_sync_state SET balance_snapshot = balance_snapshot || $1::jsonb")
        .bind(&snapshot)
        .execute(ctx.tenant.conn())
        .await?;

    let out = view(ctx.tenant.conn(), period).await?;
    ctx.tenant.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sign is KitchenOwl's and the word is ours; this is the join between
    /// them, and getting it backwards produced "+142,27 € · you owe the household"
    /// on a live screen once.
    #[test]
    fn negative_is_the_user_owing_the_household() {
        assert_eq!(direction_of(Some(-14227)), "i_owe");
        assert_eq!(direction_of(Some(14227)), "household_owes_me");
        assert_eq!(direction_of(Some(0)), "settled");
        // No member row yet is a normal state on a fresh instance, not an error.
        assert_eq!(direction_of(None), "unknown");
    }

    /// Sortable, and readable in a database dump a year later.
    #[test]
    fn the_period_key_is_zero_padded() {
        assert_eq!(
            external_key(Period {
                year: 2026,
                month: 9
            }),
            "2026-09"
        );
        assert_eq!(
            external_key(Period {
                year: 2026,
                month: 12
            }),
            "2026-12"
        );
    }
}
