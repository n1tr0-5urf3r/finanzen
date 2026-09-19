//! Rücklagen — the annual and quarterly lumps, accrued monthly.
//!
//! A ledger's spikes are real: a car insurance lands in one month, a service charge
//! in another. The monthly saldo tells the truth about each month and, by doing so,
//! lies about the year — eleven months look better than they are and one looks like
//! a disaster.
//!
//! A fund states what a known lump costs per year and when it arrives. It books
//! NOTHING: it is an expectation, measured against the ordinary bookings already in
//! its category. A fund that created bookings would double-count the very spending
//! it exists to anticipate.
//!
//! The arithmetic has one trap, and it is the reason `accrued_through` exists.

use axum::{
    Json,
    extract::{Path, Query},
    http::StatusCode,
};
use sqlx::Row;
use std::collections::HashMap;
use uuid::Uuid;

use crate::{
    auth::Ctx,
    error::{AppError, Result},
    locale::{div_round_half_up, month_name_de},
    models::{FundOverview, FundStatus, FundSuggestion, SinkingFund, SinkingFundInput},
};

/// What to set aside each month.
///
/// An approximation by construction: a year does not divide into twelve equal cents.
/// It is a figure to act on, never a figure to build a cumulative total from.
pub fn monthly_accrual(annual_cents: i64) -> i64 {
    div_round_half_up(annual_cents, 12)
}

/// What should be aside by the end of `month`.
///
/// Computed from the ANNUAL amount rather than by multiplying the monthly one, and
/// that is the whole point: 307,00 ÷ 12 rounds to 25,58, and twelve of those is
/// 306,96 — four cents the fund never had and the bill will not accept. Taking the
/// proportion of the annual figure instead makes month 12 come back to the annual
/// amount exactly, for every input.
pub fn accrued_through(annual_cents: i64, month: u8) -> i64 {
    let months = (month.min(12)) as i64;
    div_round_half_up(annual_cents * months, 12)
}

const SELECT_FUNDS: &str = "\
    SELECT f.id, f.name, f.category_id, c.name AS category_name, f.annual_cents, \
           f.due_month, f.note, f.active, f.sort_order \
      FROM sinking_funds f \
      LEFT JOIN categories c ON c.id = f.category_id";

fn row_to_fund(r: &sqlx::postgres::PgRow) -> SinkingFund {
    let due_month: i16 = r.get("due_month");
    SinkingFund {
        id: r.get("id"),
        name: r.get("name"),
        category_id: r.get("category_id"),
        category_name: r.get("category_name"),
        annual_cents: r.get("annual_cents"),
        due_month: due_month as u8,
        due_month_name: month_name_de(due_month as u8).to_string(),
        note: r.get("note"),
        active: r.get("active"),
        sort_order: r.get("sort_order"),
    }
}

fn validate(body: &SinkingFundInput) -> Result<()> {
    if body.name.trim().is_empty() {
        return Err(AppError::Validation("Name fehlt".into()));
    }
    if body.annual_cents <= 0 {
        return Err(AppError::Validation(
            "Der Jahresbetrag muss positiv sein".into(),
        ));
    }
    if !(1..=12).contains(&body.due_month) {
        return Err(AppError::Validation("Fälliger Monat ist 1 bis 12".into()));
    }
    Ok(())
}

#[utoipa::path(
    get,
    path = "/api/v1/funds",
    tag = "funds",
    responses((status = 200, description = "Alle Rücklagen", body = Vec<SinkingFund>)),
)]
pub async fn list(mut ctx: Ctx) -> Result<Json<Vec<SinkingFund>>> {
    let rows = sqlx::query(&format!("{SELECT_FUNDS} ORDER BY f.sort_order, f.name"))
        .fetch_all(ctx.tenant.conn())
        .await?;
    let out = rows.iter().map(row_to_fund).collect();
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[utoipa::path(
    post,
    path = "/api/v1/funds",
    tag = "funds",
    request_body = SinkingFundInput,
    responses(
        (status = 201, description = "Rücklage angelegt", body = SinkingFund),
        (status = 409, description = "Für diese Kategorie gibt es bereits eine Rücklage"),
    ),
)]
pub async fn create(
    mut ctx: Ctx,
    Json(body): Json<SinkingFundInput>,
) -> Result<(StatusCode, Json<SinkingFund>)> {
    validate(&body)?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO sinking_funds (id, user_id, name, category_id, annual_cents, \
                due_month, note, active, sort_order) \
         VALUES ($1,$2,$3,$4,$5,$6::smallint,$7,$8,$9::smallint)",
    )
    .bind(id)
    .bind(ctx.tenant.user_id())
    .bind(body.name.trim())
    .bind(body.category_id)
    .bind(body.annual_cents)
    .bind(body.due_month as i16)
    .bind(
        body.note
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty()),
    )
    .bind(body.active)
    .bind(body.sort_order)
    .execute(ctx.tenant.conn())
    .await
    .map_err(|e| AppError::from_db(e, "Rücklage konnte nicht gespeichert werden"))?;

    let fund = fetch_one(&mut ctx, id).await?;
    ctx.tenant.commit().await?;
    Ok((StatusCode::CREATED, Json(fund)))
}

#[utoipa::path(
    put,
    path = "/api/v1/funds/{id}",
    tag = "funds",
    params(("id" = Uuid, Path, description = "Rücklage")),
    request_body = SinkingFundInput,
    responses(
        (status = 200, description = "Rücklage geändert", body = SinkingFund),
        (status = 404, description = "Unbekannte Rücklage"),
    ),
)]
pub async fn update(
    mut ctx: Ctx,
    Path(id): Path<Uuid>,
    Json(body): Json<SinkingFundInput>,
) -> Result<Json<SinkingFund>> {
    validate(&body)?;
    let affected = sqlx::query(
        "UPDATE sinking_funds SET name = $2, category_id = $3, annual_cents = $4, \
                due_month = $5::smallint, note = $6, active = $7, \
                sort_order = $8::smallint, updated_at = now() \
          WHERE id = $1",
    )
    .bind(id)
    .bind(body.name.trim())
    .bind(body.category_id)
    .bind(body.annual_cents)
    .bind(body.due_month as i16)
    .bind(
        body.note
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty()),
    )
    .bind(body.active)
    .bind(body.sort_order)
    .execute(ctx.tenant.conn())
    .await
    .map_err(|e| AppError::from_db(e, "Rücklage konnte nicht gespeichert werden"))?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound("Rücklage".into()));
    }
    let fund = fetch_one(&mut ctx, id).await?;
    ctx.tenant.commit().await?;
    Ok(Json(fund))
}

#[utoipa::path(
    delete,
    path = "/api/v1/funds/{id}",
    tag = "funds",
    params(("id" = Uuid, Path, description = "Rücklage")),
    responses(
        (status = 204, description = "Rücklage gelöscht"),
        (status = 404, description = "Unbekannte Rücklage"),
    ),
)]
pub async fn delete(mut ctx: Ctx, Path(id): Path<Uuid>) -> Result<StatusCode> {
    let affected = sqlx::query("DELETE FROM sinking_funds WHERE id = $1")
        .bind(id)
        .execute(ctx.tenant.conn())
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound("Rücklage".into()));
    }
    ctx.tenant.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn fetch_one(ctx: &mut Ctx, id: Uuid) -> Result<SinkingFund> {
    let row = sqlx::query(&format!("{SELECT_FUNDS} WHERE f.id = $1"))
        .bind(id)
        .fetch_optional(ctx.tenant.conn())
        .await?
        .ok_or_else(|| AppError::NotFound("Rücklage".into()))?;
    Ok(row_to_fund(&row))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusQuery {
    pub year: i32,
    /// How far through the year to measure. Defaults to December, which is the
    /// question "did this year's funds cover this year's bills".
    pub month: Option<u8>,
}

/// Every active fund, measured against what the ledger actually did.
#[utoipa::path(
    get,
    path = "/api/v1/funds/status",
    tag = "funds",
    params(
        ("year" = i32, Query, description = "Kalenderjahr"),
        ("month" = Option<u8>, Query, description = "Stichmonat 1..12, Standard 12"),
    ),
    responses((status = 200, description = "Rücklagen mit Soll, Ist und Differenz", body = FundOverview)),
)]
pub async fn status(mut ctx: Ctx, Query(q): Query<StatusQuery>) -> Result<Json<FundOverview>> {
    let month = q.month.unwrap_or(12).clamp(1, 12);

    let rows = sqlx::query(&format!(
        "{SELECT_FUNDS} WHERE f.active ORDER BY f.sort_order, f.name"
    ))
    .fetch_all(ctx.tenant.conn())
    .await?;
    let funds: Vec<SinkingFund> = rows.iter().map(row_to_fund).collect();

    // Spending per category for the year, in one query rather than one per fund.
    // Stored sign throughout: positive is cost, and a refund inside the category
    // reduces it, which is exactly what a fund wants to know.
    let spent_rows = sqlx::query(
        "SELECT b.category_id, COALESCE(SUM(b.net_cents), 0)::bigint AS spent \
           FROM v_ledger b \
          WHERE b.period_year = $1::smallint AND b.category_id IS NOT NULL \
          GROUP BY b.category_id",
    )
    .bind(q.year)
    .fetch_all(ctx.tenant.conn())
    .await?;
    let spent_by_category: HashMap<Uuid, i64> = spent_rows
        .iter()
        .map(|r| (r.get::<Uuid, _>("category_id"), r.get::<i64, _>("spent")))
        .collect();

    let statuses: Vec<FundStatus> = funds
        .into_iter()
        .map(|fund| {
            let spent = fund
                .category_id
                .and_then(|id| spent_by_category.get(&id).copied())
                .unwrap_or(0);
            let accrued = accrued_through(fund.annual_cents, month);
            FundStatus {
                monthly_accrual_cents: monthly_accrual(fund.annual_cents),
                accrued_by_month_cents: accrued,
                spent_cents: spent,
                remaining_cents: (fund.annual_cents - spent).max(0),
                over_under_cents: accrued - spent,
                due_passed: fund.due_month <= month,
                fund,
            }
        })
        .collect();

    let out = FundOverview {
        year: q.year,
        month,
        monthly_accrual_cents: statuses.iter().map(|s| s.monthly_accrual_cents).sum(),
        accrued_by_month_cents: statuses.iter().map(|s| s.accrued_by_month_cents).sum(),
        spent_cents: statuses.iter().map(|s| s.spent_cents).sum(),
        owed_to_the_future_cents: statuses.iter().map(|s| s.remaining_cents).sum(),
        funds: statuses,
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestQuery {
    pub year: i32,
}

/// A category spent in at most this many months of the year is a lump, not a habit.
const CLUSTER_MONTHS: usize = 2;
/// Below this, accruing is bookkeeping for its own sake. 100,00 €.
const MIN_ANNUAL_CENTS: i64 = 10_000;

/// Funds the history argues for.
///
/// Suggested, never created. The app does not get to decide that a holiday is a
/// recurring obligation — and the evidence (how many months, how many bookings) is
/// returned with the suggestion so the user can disagree with it on sight.
#[utoipa::path(
    get,
    path = "/api/v1/funds/suggestions",
    tag = "funds",
    params(("year" = i32, Query, description = "Jahr, aus dem die Vorschläge abgeleitet werden")),
    responses((status = 200, description = "Kategorien, deren Ausgaben sich auf ein bis zwei Monate ballen", body = Vec<FundSuggestion>)),
)]
pub async fn suggestions(
    mut ctx: Ctx,
    Query(q): Query<SuggestQuery>,
) -> Result<Json<Vec<FundSuggestion>>> {
    // Per category and month, so the clustering is visible rather than assumed.
    // Funds that already exist are excluded here rather than filtered in the UI: a
    // suggestion to do what has already been done is noise.
    let rows = sqlx::query(
        "SELECT b.category_id, max(b.category_name) AS category_name, b.period_month, \
                COALESCE(SUM(b.net_cents), 0)::bigint AS net, count(*)::bigint AS n \
           FROM v_ledger b \
          WHERE b.period_year = $1::smallint AND b.category_id IS NOT NULL \
            AND NOT EXISTS (SELECT 1 FROM sinking_funds f \
                             WHERE f.category_id = b.category_id) \
          GROUP BY b.category_id, b.period_month \
         HAVING COALESCE(SUM(b.net_cents), 0) > 0 \
          ORDER BY b.category_id, b.period_month",
    )
    .bind(q.year)
    .fetch_all(ctx.tenant.conn())
    .await?;

    struct Acc {
        name: String,
        total: i64,
        count: i64,
        months: Vec<(u8, i64)>,
    }
    let mut by_category: HashMap<Uuid, Acc> = HashMap::new();
    for r in &rows {
        let id: Uuid = r.get("category_id");
        let month: i16 = r.get("period_month");
        let net: i64 = r.get("net");
        let entry = by_category.entry(id).or_insert_with(|| Acc {
            name: r
                .get::<Option<String>, _>("category_name")
                .unwrap_or_default(),
            total: 0,
            count: 0,
            months: Vec::new(),
        });
        entry.total += net;
        entry.count += r.get::<i64, _>("n");
        entry.months.push((month as u8, net));
    }

    let mut out: Vec<FundSuggestion> = by_category
        .into_iter()
        .filter(|(_, a)| a.months.len() <= CLUSTER_MONTHS && a.total >= MIN_ANNUAL_CENTS)
        .map(|(id, a)| {
            // The due month is the one carrying the most, not the first: a bill
            // split over two months belongs to the month that hurts.
            let due = a
                .months
                .iter()
                .max_by_key(|(_, net)| *net)
                .map(|(m, _)| *m)
                .unwrap_or(1);
            FundSuggestion {
                category_id: id,
                category_name: a.name,
                annual_cents: a.total,
                due_month: due,
                due_month_name: month_name_de(due).to_string(),
                months_with_spending: a.months.len() as i64,
                booking_count: a.count,
                year: q.year,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.annual_cents
            .cmp(&a.annual_cents)
            .then_with(|| a.category_name.cmp(&b.category_name))
    });

    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The trap this module exists to avoid. A flat monthly figure multiplied by
    /// twelve does not come back to the year; a proportional one does, for every
    /// amount — which is what makes "what should be aside by now" a figure you can
    /// hand to a bank statement.
    #[test]
    fn twelve_months_of_accrual_come_back_to_the_year_exactly() {
        for annual in [
            144_000, // a yearly service charge — divides evenly
            30_700,  // a yearly car insurance — does not
            6_000,   // a monthly server bill, annualised
            4_800,   // a quarterly broadcasting fee, annualised
            500,     // a domain, 5,00 a year
            1,       // one cent a year: the degenerate case
            99_999_999,
        ] {
            assert_eq!(
                accrued_through(annual, 12),
                annual,
                "zwölf Monate müssen exakt den Jahresbetrag ergeben: {annual}"
            );
        }
    }

    #[test]
    fn the_monthly_figure_is_the_annual_one_divided_by_twelve() {
        assert_eq!(monthly_accrual(144_000), 12_000);
        // 307,00 / 12 = 25,5833 → 25,58, and twelve of THOSE would be 306,96.
        assert_eq!(monthly_accrual(30_700), 2_558);
        assert_ne!(monthly_accrual(30_700) * 12, 30_700);
    }

    #[test]
    fn accrual_never_goes_backwards_and_starts_at_zero() {
        let annual = 30_700;
        assert_eq!(accrued_through(annual, 0), 0);
        let mut previous = 0;
        for month in 1..=12u8 {
            let now = accrued_through(annual, month);
            assert!(now >= previous, "Monat {month} accrued {now} < {previous}");
            previous = now;
        }
        // And asking beyond the year does not keep accruing.
        assert_eq!(accrued_through(annual, 13), annual);
    }
}
