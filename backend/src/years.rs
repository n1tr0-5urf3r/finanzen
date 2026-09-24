//! Fiscal years and the opening balance carried between them.

use axum::{Json, extract::Path, http::StatusCode};
use sqlx::Row;

use crate::{
    auth::Ctx,
    calc::{self, YearRaw},
    error::{AppError, Result},
    models::{Year, YearInput, YearUpdate},
};

async fn load(ctx: &mut Ctx) -> Result<Vec<Year>> {
    let rows = sqlx::query(
        "SELECT f.year, f.opening_cents, f.opening_source, f.tax_locked_at IS NOT NULL AS locked, \
                COALESCE(agg.n, 0)::bigint AS booking_count, \
                COALESCE(agg.inc, 0)::bigint AS income_cents, \
                COALESCE(agg.exp, 0)::bigint AS expense_cents \
           FROM fiscal_years f \
           LEFT JOIN ( \
             SELECT period_year, count(*) AS n, \
                    SUM(amount_cents) FILTER (WHERE kind = 'income')  AS inc, \
                    SUM(amount_cents) FILTER (WHERE kind = 'expense') AS exp \
               FROM v_ledger GROUP BY period_year \
           ) agg ON agg.period_year = f.year \
          ORDER BY f.year",
    )
    .fetch_all(ctx.tenant.conn())
    .await?;

    let raw: Vec<YearRaw> = rows
        .iter()
        .map(|r| {
            let income: i64 = r.get("income_cents");
            let expense: i64 = r.get("expense_cents");
            YearRaw {
                year: r.get::<i16, _>("year") as i32,
                opening_cents: r.get("opening_cents"),
                opening_is_configured: r.get::<String, _>("opening_source") == "configured",
                saldo_cents: income - expense,
                booking_count: r.get("booking_count"),
            }
        })
        .collect();

    let chained = calc::chain_years(&raw);
    Ok(rows
        .iter()
        .zip(chained)
        .map(|(r, c)| Year {
            year: c.year,
            opening_balance_cents: c.opening_cents,
            opening_source: r.get("opening_source"),
            locked: r.get("locked"),
            booking_count: c.booking_count,
            income_cents: r.get("income_cents"),
            expense_cents: r.get("expense_cents"),
            balance_cents: c.saldo_cents,
            closing_balance_cents: c.closing_cents,
            carryover_gap_cents: c.chain_gap_cents,
        })
        .collect())
}

#[utoipa::path(
    get,
    path = "/api/v1/years",
    tag = "years",
    responses((status = 200, description = "Jahre mit Vortrag, Saldo und Übertragslücke", body = Vec<Year>)),
)]
pub async fn list(mut ctx: Ctx) -> Result<Json<Vec<Year>>> {
    let out = load(&mut ctx).await?;
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[utoipa::path(
    post,
    path = "/api/v1/years",
    tag = "years",
    request_body = YearInput,
    responses((status = 201, description = "Jahr angelegt", body = Year), (status = 409, description = "Dieses Jahr gibt es bereits", body = crate::error::ErrorBody)),
)]
pub async fn create(mut ctx: Ctx, Json(body): Json<YearInput>) -> Result<(StatusCode, Json<Year>)> {
    sqlx::query(
        "INSERT INTO fiscal_years (user_id, year, opening_cents, opening_source) \
         VALUES ($1, $2::smallint, $3, 'configured')",
    )
    .bind(ctx.tenant.user_id())
    .bind(body.year)
    .bind(body.opening_balance_cents)
    .execute(ctx.tenant.conn())
    .await
    .map_err(|e| AppError::from_db(e, "Dieses Jahr gibt es bereits"))?;

    let years = load(&mut ctx).await?;
    let year = years
        .into_iter()
        .find(|y| y.year == body.year)
        .ok_or_else(|| AppError::NotFound("Jahr".into()))?;
    ctx.tenant.commit().await?;
    Ok((StatusCode::CREATED, Json(year)))
}

#[utoipa::path(
    put,
    path = "/api/v1/years/{year}",
    tag = "years",
    params(("year" = i32, Path, description = "Kalenderjahr")),
    request_body = YearUpdate,
    responses((status = 200, description = "Jahr geändert; ein gesetzter Vortrag gilt damit als konfiguriert", body = Year), (status = 404, description = "Nicht gefunden", body = crate::error::ErrorBody)),
)]
pub async fn update(
    mut ctx: Ctx,
    Path(year): Path<i32>,
    Json(body): Json<YearUpdate>,
) -> Result<Json<Year>> {
    // Editing the opening balance makes it configured by definition — the user has
    // just asserted a number rather than inheriting one. Not editing it keeps it
    // whatever it was, derived included.
    let affected = sqlx::query(
        "UPDATE fiscal_years SET \
                opening_cents = COALESCE($2, opening_cents), \
                opening_source = CASE WHEN $2 IS NULL THEN opening_source ELSE 'configured' END, \
                tax_locked_at = CASE WHEN $3 IS NULL THEN tax_locked_at \
                                     WHEN $3 THEN COALESCE(tax_locked_at, now()) \
                                     ELSE NULL END \
          WHERE year = $1::smallint",
    )
    .bind(year)
    .bind(body.opening_balance_cents)
    .bind(body.locked)
    .execute(ctx.tenant.conn())
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound("Jahr".into()));
    }

    let years = load(&mut ctx).await?;
    let out = years
        .into_iter()
        .find(|y| y.year == year)
        .ok_or_else(|| AppError::NotFound("Jahr".into()))?;
    ctx.tenant.commit().await?;
    Ok(Json(out))
}
