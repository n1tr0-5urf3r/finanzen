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
//! **A settlement is an expense or an income, never a `transfer`.** This was the
//! other way round when the module was written, on the argument that the
//! household's purchases already sit in the personal ledger at FULL value — which
//! is true — and that booking the settlement again would count them twice — which
//! is not. Work it through: I pay 20,00 at the supermarket for both of us and book
//! 20,00, so my ledger says my food cost 20,00 when it really cost 10,00. It is
//! ALREADY overstated, by exactly the settlement. When the flatmate hands me 10,00
//! and that lands as income, the ledger comes to 10,00 and agrees with reality. The
//! same in reverse: they pay, nothing enters my ledger, I owe 10,00, and the expense
//! I book when I hand it over is the only record that my food cost anything at all.
//!
//! `transfer` models money moving between the user's OWN accounts: `net_cents` is 0
//! by generated column, so it moves neither the balance nor any category. This money
//! is gone — it went to another person. The user's own ledger has said so for three
//! years: of the thirteen `Ausgleich` bookings in it, nine are expenses and four are
//! income, not one is a transfer, and twelve carry the category `Haushaltsausgleich`
//! as a manual assignment.
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
    bookings::{SELECT_BOOKING, assert_year_unlocked, resolve_category, row_to_booking},
    error::{AppError, Result},
    locale::month_name_de,
    models::{BookingKind, KoSettlement, Period},
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

/// The optional body of a `POST`. Everything in it has a sensible default, which is
/// why the body itself is optional.
#[derive(Debug, Default, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettleRequest {
    /// Overrides `Haushaltsausgleich` for this one booking.
    pub category_id: Option<Uuid>,
}

/// The category a settlement lands in, matched by name.
///
/// Twelve of the user's thirteen historical `Ausgleich` bookings carry it, assigned
/// by hand — the comment ends in a month name and rules match the whole comment
/// exactly, so no rule could ever have done it. Landing in the same category is what
/// puts a new settlement in the same row of the analysis as the old ones.
const CATEGORY_NAME: &str = "Haushaltsausgleich";

/// Which way the money goes, and therefore what kind of booking it is. Money handed
/// to another person leaves the account for good; money received arrives in it.
/// Neither is a transfer, which is for moving money between one's own accounts.
fn kind_of(direction: &str) -> Option<BookingKind> {
    match direction {
        "i_owe" => Some(BookingKind::Expense),
        "household_owes_me" => Some(BookingKind::Income),
        _ => None,
    }
}

/// `None` when the category does not exist for this tenant — a fresh instance has
/// the 32 seeded categories and not this one. The caller then falls back to the rule
/// table and says so, rather than inventing taxonomy behind the user's back.
async fn settlement_category(conn: &mut PgConnection) -> Result<Option<(Uuid, String)>> {
    let row = sqlx::query("SELECT id, name FROM categories WHERE lower(name) = lower($1) LIMIT 1")
        .bind(CATEGORY_NAME)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.map(|r| (r.get::<Uuid, _>("id"), r.get::<String, _>("name"))))
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
    let direction = direction_of(balance);

    let existing = sqlx::query(&format!(
        "{SELECT_BOOKING} WHERE b.external_source = $1 AND b.external_id = $2"
    ))
    .bind(SOURCE)
    .bind(external_key(period))
    .fetch_optional(&mut *conn)
    .await?;

    let (settled_balance_cents, settled_at) = snapshot_for(&mut *conn, period).await?;
    // Reported rather than left to be discovered after the fact: the UI has to be
    // able to say which kind of booking this will be and where it will land BEFORE
    // the button is pressed, because both of those change the year's figures.
    let category = settlement_category(&mut *conn).await?;

    Ok(KoSettlement {
        balance_cents: balance,
        direction: direction.to_string(),
        kind: kind_of(direction),
        category_id: category.as_ref().map(|(id, _)| *id),
        category_name: category.as_ref().map(|(_, name)| name.clone()),
        category_is_fallback: category.is_none(),
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
    request_body = Option<SettleRequest>,
    responses(
        (status = 201, description = "Ausgleich gebucht — als Ausgabe oder Einnahme, nie als Umbuchung", body = KoSettlement),
        (status = 200, description = "Für diesen Monat bereits gebucht; dieselbe Buchung", body = KoSettlement),
        (status = 422, description = "Nichts auszugleichen", body = crate::error::ErrorBody),
    ),
)]
pub async fn settle(
    mut ctx: Ctx,
    // Optional: the button sends `{}` and the tests send no body at all. A missing
    // body means "the default category", not a 400.
    body: Option<Json<SettleRequest>>,
) -> Result<(StatusCode, Json<KoSettlement>)> {
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

    let Some(kind) = kind_of(&current.direction) else {
        return Err(AppError::Unprocessable(
            "Die Richtung des Saldos ist unklar — erst synchronisieren".into(),
        ));
    };

    // An explicit category from the caller wins; otherwise `Haushaltsausgleich`,
    // which is where three years of these bookings already live. Either way it goes
    // through the ordinary resolver, so the `category_source` state machine — a
    // category may not be `unresolved`, and `rule` iff a rule id is present — is
    // satisfied by the same code path every other booking uses.
    let explicit = body
        .and_then(|Json(b)| b.category_id)
        .or(current.category_id);
    let resolution =
        resolve_category(ctx.tenant.conn(), &current.suggested_comment, explicit).await?;

    let id = Uuid::new_v4();
    let booked_on = chrono::NaiveDate::from_ymd_opt(period.year, period.month as u32, 1);
    sqlx::query(
        "INSERT INTO bookings (id, user_id, period_year, period_month, booked_on, kind, \
                               amount_cents, comment, tax_relevant, category_id, \
                               category_source, resolved_rule_id, origin, \
                               external_source, external_id) \
         VALUES ($1,$2,$3::smallint,$4::smallint,$5,$6,$7,$8,false,$9, \
                 $10,$11,'kitchenowl',$12,$13)",
    )
    .bind(id)
    .bind(ctx.tenant.user_id())
    .bind(period.year)
    .bind(period.month as i16)
    .bind(booked_on)
    .bind(kind.as_db())
    .bind(balance.abs())
    .bind(&current.suggested_comment)
    .bind(resolution.category_id)
    .bind(resolution.source)
    .bind(resolution.rule_id)
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
