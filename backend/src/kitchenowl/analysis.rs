//! The household ledger's own analysis, deliberately parallel to the personal one.
//!
//! Same questions, same shape of answer — what did this category cost over the
//! year, which months, how does one recurring purchase move — asked of the mirror
//! instead of the bookings. Nothing here reads a booking, and no figure returned
//! from this module may be added to one.
//!
//! Two differences from `crate::analysis`, both forced by what KitchenOwl is:
//!
//! - **Every figure comes in pairs.** The household's amount and the user's share
//!   are different numbers with different meanings, and the single most likely
//!   mistake in this whole integration is reporting one of them as the other. They
//!   are returned together, always, and never summed.
//! - **There is no netting.** A KitchenOwl expense is an expense; there is no
//!   income side, no transfer kind and no credit case. So the figures here are
//!   plain costs, and the sign never flips.
//!
//! `exclude_from_statistics` is honoured, because KitchenOwl's own statistics
//! honour it — an analysis that disagreed with the app it mirrors would be worse
//! than none. The count of what was left out is returned so the UI can say so.

use axum::{Json, extract::Query};
use sqlx::Row;

use crate::{
    auth::Ctx,
    error::{AppError, Result},
    locale::{div_round_half_up, month_name_de},
    models::{
        KoCategoryAnalysis, KoCategoryAnalysisRow, KoMonthlySeries, KoPayerShare, KoSeriesMonth,
        KoSeriesSubject,
    },
};

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KoYearQuery {
    pub year: i32,
}

/// Only what KitchenOwl itself counts: the household's own statistics skip
/// `exclude_from_statistics`, so ours do too.
const COUNTED: &str = "e.user_id = app.current_user_id() AND NOT e.exclude_from_statistics \
                       AND EXTRACT(YEAR FROM e.expense_date)::int = $1";

/// Every KitchenOwl category over one year, with the twelve months behind it.
#[utoipa::path(
    get,
    path = "/api/v1/kitchenowl/analysis/categories",
    tag = "kitchenowl",
    params(("year" = i32, Query, description = "Kalenderjahr")),
    responses((status = 200, description = "Haushaltsbetrag UND eigener Anteil je Kategorie — nie summiert", body = KoCategoryAnalysis)),
)]
pub async fn categories(
    mut ctx: Ctx,
    Query(q): Query<KoYearQuery>,
) -> Result<Json<KoCategoryAnalysis>> {
    let sql = format!(
        "SELECT e.ko_category_id AS cat, \
                max(e.ko_category_name) AS cat_name, \
                EXTRACT(MONTH FROM e.expense_date)::int AS m, \
                COALESCE(SUM(e.amount_cents), 0)::bigint AS amount, \
                COALESCE(SUM(e.own_share_cents), 0)::bigint AS own, \
                count(*)::bigint AS n \
           FROM ko_expenses e \
          WHERE {COUNTED} \
          GROUP BY e.ko_category_id, m"
    );
    let rows = sqlx::query(&sql)
        .bind(q.year)
        .fetch_all(ctx.tenant.conn())
        .await?;

    // One row per category, twelve slots each. Built in Rust rather than in SQL
    // because a crosstab would need the category set up front and would hide the
    // "no expenses at all" case behind an empty result either way.
    let mut by_category: Vec<KoCategoryAnalysisRow> = Vec::new();
    let mut months_seen = [false; 12];

    for r in &rows {
        let cat: Option<i64> = r.get("cat");
        let name: Option<String> = r.get("cat_name");
        let month: i32 = r.get("m");
        let amount: i64 = r.get("amount");
        let own: i64 = r.get("own");
        let count: i64 = r.get("n");
        if !(1..=12).contains(&month) {
            continue;
        }
        months_seen[(month - 1) as usize] = true;

        let slot = match by_category
            .iter_mut()
            .position(|row| row.ko_category_id == cat)
        {
            Some(i) => &mut by_category[i],
            None => {
                by_category.push(KoCategoryAnalysisRow {
                    ko_category_id: cat,
                    ko_category_name: name,
                    amount_cents: 0,
                    own_share_cents: 0,
                    expense_count: 0,
                    share_of_total: 0.0,
                    average_per_month_cents: 0,
                    average_own_share_per_month_cents: 0,
                    monthly_amount_cents: vec![0; 12],
                    monthly_own_share_cents: vec![0; 12],
                });
                by_category.last_mut().expect("just pushed")
            }
        };
        slot.amount_cents += amount;
        slot.own_share_cents += own;
        slot.expense_count += count;
        slot.monthly_amount_cents[(month - 1) as usize] = amount;
        slot.monthly_own_share_cents[(month - 1) as usize] = own;
    }

    let total_amount_cents: i64 = by_category.iter().map(|r| r.amount_cents).sum();
    let total_own_share_cents: i64 = by_category.iter().map(|r| r.own_share_cents).sum();
    let expense_count: i64 = by_category.iter().map(|r| r.expense_count).sum();
    let months_with_data = months_seen.iter().filter(|seen| **seen).count() as i64;

    for row in &mut by_category {
        row.share_of_total = if total_amount_cents > 0 {
            row.amount_cents as f64 / total_amount_cents as f64
        } else {
            0.0
        };
        // Divided by the months the HOUSEHOLD was active, not by twelve: a mirror
        // that started in March must not report two months of zeroes as spending.
        row.average_per_month_cents = div_round_half_up(row.amount_cents, months_with_data.max(1));
        row.average_own_share_per_month_cents =
            div_round_half_up(row.own_share_cents, months_with_data.max(1));
    }
    by_category.sort_by_key(|r| std::cmp::Reverse(r.amount_cents));

    let uncategorized_count = by_category
        .iter()
        .filter(|r| r.ko_category_id.is_none())
        .map(|r| r.expense_count)
        .sum();

    let excluded_count: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM ko_expenses e \
          WHERE e.user_id = app.current_user_id() AND e.exclude_from_statistics \
            AND EXTRACT(YEAR FROM e.expense_date)::int = $1",
    )
    .bind(q.year)
    .fetch_one(ctx.tenant.conn())
    .await?;

    // Who paid. This has no analogue in the personal ledger — there is only one
    // payer there — and it is half of what a shared ledger is for.
    let payer_rows = sqlx::query(&format!(
        "SELECT e.paid_by_id AS id, COALESCE(m.name, '?') AS name, \
                COALESCE(SUM(e.amount_cents), 0)::bigint AS amount, \
                count(*)::bigint AS n \
           FROM ko_expenses e \
           LEFT JOIN ko_members m \
                  ON m.user_id = e.user_id AND m.member_id = e.paid_by_id \
          WHERE {COUNTED} \
          GROUP BY e.paid_by_id, m.name ORDER BY amount DESC"
    ))
    .bind(q.year)
    .fetch_all(ctx.tenant.conn())
    .await?;

    let paid_by = payer_rows
        .iter()
        .map(|r| KoPayerShare {
            member_id: r.get("id"),
            name: r.get("name"),
            amount_cents: r.get("amount"),
            expense_count: r.get("n"),
        })
        .collect();

    let years: Vec<i32> = sqlx::query_scalar(
        "SELECT DISTINCT EXTRACT(YEAR FROM e.expense_date)::int \
           FROM ko_expenses e WHERE e.user_id = app.current_user_id() \
          ORDER BY 1 DESC",
    )
    .fetch_all(ctx.tenant.conn())
    .await?;

    let out = KoCategoryAnalysis {
        year: q.year,
        rows: by_category,
        total_amount_cents,
        total_own_share_cents,
        expense_count,
        months_with_data,
        uncategorized_count,
        excluded_count,
        paid_by,
        years,
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KoSeriesQuery {
    pub year: i32,
    /// Exactly one of these. `koCategoryId` charts a KitchenOwl category;
    /// `name` charts one recurring purchase, which is the "how much Kaufland"
    /// question the household actually asks.
    pub ko_category_id: Option<i64>,
    pub name: Option<String>,
    /// Charts the expenses with no KitchenOwl category — 73 of 211 in the live
    /// corpus, so it cannot be an unreachable state.
    pub uncategorized: Option<bool>,
}

/// Twelve months for one KitchenOwl category or one recurring name.
#[utoipa::path(
    get,
    path = "/api/v1/kitchenowl/analysis/series",
    tag = "kitchenowl",
    params(
        ("year" = i32, Query, description = "Kalenderjahr"),
        ("koCategoryId" = Option<i64>, Query, description = "KitchenOwl-Kategorie"),
        ("name" = Option<String>, Query, description = "Name der Ausgabe, z. B. Kaufland"),
        ("uncategorized" = Option<bool>, Query, description = "Ausgaben ohne KitchenOwl-Kategorie"),
    ),
    responses(
        (status = 200, description = "Zwölf Monate, Haushaltsbetrag und eigener Anteil", body = KoMonthlySeries),
        (status = 400, description = "Weder oder mehrere Parameter angegeben"),
    ),
)]
pub async fn series(mut ctx: Ctx, Query(q): Query<KoSeriesQuery>) -> Result<Json<KoMonthlySeries>> {
    let uncategorized = q.uncategorized.unwrap_or(false);
    let (mode, where_sql): (&str, &str) = match (&q.ko_category_id, &q.name, uncategorized) {
        (Some(_), None, false) => ("category", "e.ko_category_id = $2"),
        // Case-insensitive on the trimmed name, so "Kaufland" and "kaufland" are
        // one subject — the same rule the personal side applies to comments.
        (None, Some(_), false) => ("name", "lower(btrim(e.name)) = lower(btrim($2))"),
        (None, None, true) => ("uncategorized", "e.ko_category_id IS NULL"),
        _ => {
            return Err(AppError::Validation(
                "Genau eines von koCategoryId, name oder uncategorized angeben".into(),
            ));
        }
    };

    let sql = format!(
        "SELECT EXTRACT(MONTH FROM e.expense_date)::int AS m, \
                COALESCE(SUM(e.amount_cents), 0)::bigint AS amount, \
                COALESCE(SUM(e.own_share_cents), 0)::bigint AS own, \
                count(*)::bigint AS n \
           FROM ko_expenses e \
          WHERE {COUNTED} AND {where_sql} \
          GROUP BY m ORDER BY m"
    );

    let query = sqlx::query(&sql).bind(q.year);
    let rows = match (&q.ko_category_id, &q.name) {
        (Some(id), _) => query.bind(*id).fetch_all(ctx.tenant.conn()).await?,
        (None, Some(name)) => {
            query
                .bind(name.trim().to_string())
                .fetch_all(ctx.tenant.conn())
                .await?
        }
        _ => query.fetch_all(ctx.tenant.conn()).await?,
    };

    let mut months: Vec<KoSeriesMonth> = (1..=12u8)
        .map(|month| KoSeriesMonth {
            month,
            month_name: month_name_de(month).to_string(),
            amount_cents: 0,
            own_share_cents: 0,
            expense_count: 0,
        })
        .collect();

    for r in &rows {
        let month: i32 = r.get("m");
        if !(1..=12).contains(&month) {
            continue;
        }
        let slot = &mut months[(month - 1) as usize];
        slot.amount_cents = r.get("amount");
        slot.own_share_cents = r.get("own");
        slot.expense_count = r.get("n");
    }

    // Looked up rather than echoed, so a category renamed in KitchenOwl answers
    // with its current name.
    let subject = match (mode, &q.ko_category_id, &q.name) {
        ("category", Some(id), _) => {
            sqlx::query_scalar::<_, String>("SELECT name FROM ko_categories WHERE category_id = $1")
                .bind(id)
                .fetch_optional(ctx.tenant.conn())
                .await?
                .ok_or_else(|| AppError::NotFound("KitchenOwl-Kategorie".into()))?
        }
        ("name", _, Some(name)) => name.trim().to_string(),
        _ => String::new(),
    };

    let amount_cents = months.iter().map(|m| m.amount_cents).sum();
    let own_share_cents = months.iter().map(|m| m.own_share_cents).sum();
    let expense_count = months.iter().map(|m| m.expense_count).sum();
    let months_with_data = months.iter().filter(|m| m.expense_count > 0).count() as i64;

    let out = KoMonthlySeries {
        year: q.year,
        mode: mode.to_string(),
        subject,
        ko_category_id: q.ko_category_id,
        average_per_active_month_cents: div_round_half_up(amount_cents, months_with_data.max(1)),
        average_own_share_per_active_month_cents: div_round_half_up(
            own_share_cents,
            months_with_data.max(1),
        ),
        months,
        amount_cents,
        own_share_cents,
        expense_count,
        months_with_data,
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

/// The recurring names worth charting, most frequent first.
#[utoipa::path(
    get,
    path = "/api/v1/kitchenowl/analysis/series/subjects",
    tag = "kitchenowl",
    params(("year" = i32, Query, description = "Kalenderjahr")),
    responses((status = 200, description = "Namen mit mehr als einer Ausgabe, häufigste zuerst", body = Vec<KoSeriesSubject>)),
)]
pub async fn series_subjects(
    mut ctx: Ctx,
    Query(q): Query<KoYearQuery>,
) -> Result<Json<Vec<KoSeriesSubject>>> {
    // Grouped case-insensitively, and reported under the spelling used most
    // recently — KitchenOwl names are typed by hand by two people.
    let sql = format!(
        "SELECT (array_agg(e.name ORDER BY e.expense_date DESC))[1] AS name, \
                count(*)::bigint AS n, \
                COALESCE(SUM(e.amount_cents), 0)::bigint AS amount, \
                COALESCE(SUM(e.own_share_cents), 0)::bigint AS own, \
                (array_agg(e.ko_category_name ORDER BY e.expense_date DESC))[1] AS cat \
           FROM ko_expenses e \
          WHERE {COUNTED} \
          GROUP BY lower(btrim(e.name)) \
         HAVING count(*) > 1 \
          ORDER BY n DESC, amount DESC LIMIT 60"
    );
    let rows = sqlx::query(&sql)
        .bind(q.year)
        .fetch_all(ctx.tenant.conn())
        .await?;

    let out: Vec<KoSeriesSubject> = rows
        .iter()
        .map(|r| KoSeriesSubject {
            name: r.get("name"),
            expense_count: r.get("n"),
            amount_cents: r.get("amount"),
            own_share_cents: r.get("own"),
            ko_category_name: r.get("cat"),
        })
        .collect();
    ctx.tenant.commit().await?;
    Ok(Json(out))
}
