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
use sqlx::{PgConnection, Row};

use crate::{
    auth::Ctx,
    error::{AppError, Result},
    locale::{div_round_half_up, month_name_de},
    models::{
        KoCategoryAnalysis, KoCategoryAnalysisRow, KoComparePayer, KoCompareRow, KoCompareTotals,
        KoMonthlySeries, KoPayerShare, KoSeriesMonth, KoSeriesSubject, KoTrailingCategory,
        KoTrailingMonth, KoTrailingWindow, KoYearComparison,
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

// ------------------------------------------------------- year against year
//
// The same two questions the personal ledger answers in `crate::compare`, asked of
// the mirror: "is this year worse than last" and "is this getting worse". The
// second needs a window that crosses the year boundary, because a calendar year is
// an accounting convention and a household's habits are not.
//
// The trap is the same one, and here it is sharper: the mirror's first expense is
// 2024-12-22, so 2024 holds one month and 2025 holds twelve. Their raw totals
// would report a 1.200 % rise that is entirely the calendar. So every figure is
// returned twice — as the years stand, and restricted to `comparable_months`.

/// Period as one orderable integer, mirroring `period_ord` in the personal ledger:
/// `year * 12 + month - 1`. December to January is one step.
fn ord(year: i32, month: u8) -> i32 {
    year * 12 + month as i32 - 1
}

fn year_month(ord: i32) -> (i32, u8) {
    (ord.div_euclid(12), (ord.rem_euclid(12) + 1) as u8)
}

/// `delta / previous`, or `None` when there was no previous to be a share of.
fn ratio(delta_cents: i64, previous_cents: i64) -> Option<f64> {
    if previous_cents == 0 {
        None
    } else {
        Some(delta_cents as f64 / (previous_cents.abs() as f64))
    }
}

/// One (year, month, category) bucket, as the mirror returns it.
struct Bucket {
    year: i32,
    month: u8,
    category_id: Option<i64>,
    category_name: Option<String>,
    amount_cents: i64,
    own_share_cents: i64,
    expense_count: i64,
}

/// Buckets for a closed `expense_date` range, which is how a window that spans two
/// years is expressed — and it uses the `(user_id, expense_date)` index rather than
/// a computed expression that could not.
async fn load_buckets(
    conn: &mut PgConnection,
    from: (i32, u8),
    to: (i32, u8),
) -> Result<Vec<Bucket>> {
    let rows = sqlx::query(
        "SELECT EXTRACT(YEAR FROM e.expense_date)::int AS y, \
                EXTRACT(MONTH FROM e.expense_date)::int AS m, \
                e.ko_category_id AS cat, \
                (array_agg(e.ko_category_name ORDER BY e.expense_date DESC))[1] AS cat_name, \
                COALESCE(SUM(e.amount_cents), 0)::bigint AS amount, \
                COALESCE(SUM(e.own_share_cents), 0)::bigint AS own, \
                count(*)::bigint AS n \
           FROM ko_expenses e \
          WHERE e.user_id = app.current_user_id() AND NOT e.exclude_from_statistics \
            AND e.expense_date >= make_date($1::int, $2::int, 1) \
            AND e.expense_date < (make_date($3::int, $4::int, 1) + INTERVAL '1 month') \
          GROUP BY y, m, e.ko_category_id",
    )
    .bind(from.0)
    .bind(from.1 as i32)
    .bind(to.0)
    .bind(to.1 as i32)
    .fetch_all(&mut *conn)
    .await?;

    Ok(rows
        .iter()
        .filter_map(|r| {
            let month: i32 = r.get("m");
            (1..=12).contains(&month).then(|| Bucket {
                year: r.get("y"),
                month: month as u8,
                category_id: r.get("cat"),
                category_name: r.get("cat_name"),
                amount_cents: r.get("amount"),
                own_share_cents: r.get("own"),
                expense_count: r.get("n"),
            })
        })
        .collect())
}

/// How many expenses KitchenOwl itself excludes from its statistics, per year.
async fn excluded_in(conn: &mut PgConnection, year: i32) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*)::bigint FROM ko_expenses e \
          WHERE e.user_id = app.current_user_id() AND e.exclude_from_statistics \
            AND EXTRACT(YEAR FROM e.expense_date)::int = $1",
    )
    .bind(year)
    .fetch_one(&mut *conn)
    .await?)
}

async fn mirror_years(conn: &mut PgConnection) -> Result<Vec<i32>> {
    Ok(sqlx::query_scalar(
        "SELECT DISTINCT EXTRACT(YEAR FROM e.expense_date)::int \
           FROM ko_expenses e WHERE e.user_id = app.current_user_id() \
          ORDER BY 1 DESC",
    )
    .fetch_all(&mut *conn)
    .await?)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KoCompareQuery {
    pub year: i32,
}

/// The household's year against the one before it, per KitchenOwl category.
#[utoipa::path(
    get,
    path = "/api/v1/kitchenowl/analysis/compare",
    tag = "kitchenowl",
    params(("year" = i32, Query, description = "Kalenderjahr; verglichen wird mit dem Vorjahr")),
    responses((status = 200, description = "Haushalt und eigener Anteil, Jahr gegen Vorjahr — roh und auf gemeinsame Monate beschränkt", body = KoYearComparison)),
)]
pub async fn compare(
    mut ctx: Ctx,
    Query(q): Query<KoCompareQuery>,
) -> Result<Json<KoYearComparison>> {
    let previous_year = q.year - 1;
    let buckets = load_buckets(ctx.tenant.conn(), (previous_year, 1), (q.year, 12)).await?;

    // The months both years hold. Intersection rather than a prefix: a household
    // that skipped a month is a real thing, and taking the first N would compare
    // August with February.
    let months_of = |year: i32| -> std::collections::BTreeSet<u8> {
        buckets
            .iter()
            .filter(|b| b.year == year)
            .map(|b| b.month)
            .collect()
    };
    let cur_months = months_of(q.year);
    let prev_months = months_of(previous_year);
    let comparable: Vec<u8> = cur_months.intersection(&prev_months).copied().collect();
    let shared = |b: &Bucket| comparable.contains(&b.month);

    // Keyed by the KitchenOwl category id, not by name: the id is stable and the
    // name is typed by hand, so a renamed category is still one history. `None` is
    // its own key — "no category" is a third of the corpus, not a missing value.
    let mut ids: Vec<Option<i64>> = buckets.iter().map(|b| b.category_id).collect();
    ids.sort();
    ids.dedup();

    let mut rows: Vec<KoCompareRow> = ids
        .into_iter()
        .map(|id| {
            let mine = |year: i32| {
                buckets
                    .iter()
                    .filter(move |b| b.category_id == id && b.year == year)
            };
            let sum = |year: i32, restricted: bool, f: fn(&Bucket) -> i64| -> i64 {
                mine(year).filter(|b| !restricted || shared(b)).map(f).sum()
            };

            let amount = sum(q.year, false, |b| b.amount_cents);
            let own = sum(q.year, false, |b| b.own_share_cents);
            let count = sum(q.year, false, |b| b.expense_count);
            let prev_amount = sum(previous_year, false, |b| b.amount_cents);
            let prev_own = sum(previous_year, false, |b| b.own_share_cents);
            let prev_count = sum(previous_year, false, |b| b.expense_count);
            let cmp_amount = sum(q.year, true, |b| b.amount_cents);
            let cmp_own = sum(q.year, true, |b| b.own_share_cents);
            let cmp_prev_amount = sum(previous_year, true, |b| b.amount_cents);
            let cmp_prev_own = sum(previous_year, true, |b| b.own_share_cents);

            let monthly = |year: i32, f: fn(&Bucket) -> i64| -> Vec<i64> {
                let mut slots = vec![0i64; 12];
                for b in mine(year) {
                    slots[(b.month - 1) as usize] += f(b);
                }
                slots
            };

            KoCompareRow {
                ko_category_id: id,
                // The most recent spelling wins, current year first.
                ko_category_name: mine(q.year)
                    .chain(mine(previous_year))
                    .find_map(|b| b.category_name.clone()),
                amount_cents: amount,
                own_share_cents: own,
                expense_count: count,
                previous_amount_cents: prev_amount,
                previous_own_share_cents: prev_own,
                previous_expense_count: prev_count,
                delta_amount_cents: amount - prev_amount,
                delta_own_share_cents: own - prev_own,
                delta_ratio: ratio(amount - prev_amount, prev_amount),
                comparable_amount_cents: cmp_amount,
                comparable_own_share_cents: cmp_own,
                comparable_previous_amount_cents: cmp_prev_amount,
                comparable_previous_own_share_cents: cmp_prev_own,
                comparable_delta_amount_cents: cmp_amount - cmp_prev_amount,
                comparable_delta_own_share_cents: cmp_own - cmp_prev_own,
                comparable_delta_ratio: ratio(cmp_amount - cmp_prev_amount, cmp_prev_amount),
                monthly_amount_cents: monthly(q.year, |b| b.amount_cents),
                monthly_own_share_cents: monthly(q.year, |b| b.own_share_cents),
                previous_monthly_amount_cents: monthly(previous_year, |b| b.amount_cents),
                previous_monthly_own_share_cents: monthly(previous_year, |b| b.own_share_cents),
                is_new: prev_count == 0 && count > 0,
                is_gone: count == 0 && prev_count > 0,
            }
        })
        .collect();
    // Largest movement first, either direction: the screen is about what changed,
    // not about what is biggest.
    rows.sort_by(|a, b| {
        b.delta_amount_cents
            .abs()
            .cmp(&a.delta_amount_cents.abs())
            .then_with(|| b.amount_cents.cmp(&a.amount_cents))
    });

    let totals_for = |year: i32, excluded: i64| -> KoCompareTotals {
        let all = buckets.iter().filter(|b| b.year == year);
        let restricted = buckets.iter().filter(|b| b.year == year && shared(b));
        let months: std::collections::BTreeSet<u8> = months_of(year);
        KoCompareTotals {
            year,
            amount_cents: all.clone().map(|b| b.amount_cents).sum(),
            own_share_cents: all.clone().map(|b| b.own_share_cents).sum(),
            expense_count: all.map(|b| b.expense_count).sum(),
            months_with_data: months.len() as i64,
            last_month_with_data: months.iter().next_back().copied(),
            comparable_amount_cents: restricted.clone().map(|b| b.amount_cents).sum(),
            comparable_own_share_cents: restricted.clone().map(|b| b.own_share_cents).sum(),
            comparable_expense_count: restricted.map(|b| b.expense_count).sum(),
            excluded_count: excluded,
        }
    };
    let current_excluded = excluded_in(ctx.tenant.conn(), q.year).await?;
    let previous_excluded = excluded_in(ctx.tenant.conn(), previous_year).await?;

    // Who paid, both years. Only a shared ledger can ask this, and "did the split
    // drift" is the question behind it.
    let payer_rows = sqlx::query(
        "SELECT EXTRACT(YEAR FROM e.expense_date)::int AS y, \
                e.paid_by_id AS id, COALESCE(m.name, '?') AS name, \
                COALESCE(SUM(e.amount_cents), 0)::bigint AS amount, \
                count(*)::bigint AS n \
           FROM ko_expenses e \
           LEFT JOIN ko_members m \
                  ON m.user_id = e.user_id AND m.member_id = e.paid_by_id \
          WHERE e.user_id = app.current_user_id() AND NOT e.exclude_from_statistics \
            AND EXTRACT(YEAR FROM e.expense_date)::int IN ($1, $2) \
          GROUP BY y, e.paid_by_id, m.name",
    )
    .bind(q.year)
    .bind(previous_year)
    .fetch_all(ctx.tenant.conn())
    .await?;

    let mut payers: Vec<KoComparePayer> = Vec::new();
    for r in &payer_rows {
        let year: i32 = r.get("y");
        let id: Option<i64> = r.get("id");
        let name: String = r.get("name");
        let amount: i64 = r.get("amount");
        let n: i64 = r.get("n");
        let slot = match payers.iter_mut().position(|p| p.member_id == id) {
            Some(i) => &mut payers[i],
            None => {
                payers.push(KoComparePayer {
                    member_id: id,
                    name,
                    amount_cents: 0,
                    previous_amount_cents: 0,
                    delta_cents: 0,
                    expense_count: 0,
                    previous_expense_count: 0,
                });
                payers.last_mut().expect("just pushed")
            }
        };
        if year == q.year {
            slot.amount_cents += amount;
            slot.expense_count += n;
        } else {
            slot.previous_amount_cents += amount;
            slot.previous_expense_count += n;
        }
    }
    for p in &mut payers {
        p.delta_cents = p.amount_cents - p.previous_amount_cents;
    }
    payers.sort_by_key(|p| std::cmp::Reverse(p.amount_cents));

    let years = mirror_years(ctx.tenant.conn()).await?;
    let out = KoYearComparison {
        year: q.year,
        previous_year,
        current: totals_for(q.year, current_excluded),
        previous: totals_for(previous_year, previous_excluded),
        fully_comparable: cur_months == prev_months && !cur_months.is_empty(),
        comparable_months: comparable,
        previous_year_has_data: !prev_months.is_empty(),
        rows,
        paid_by: payers,
        years,
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KoTrailingQuery {
    pub year: i32,
    pub month: u8,
}

/// The twelve months ending at the given period, whatever years they fall in.
#[utoipa::path(
    get,
    path = "/api/v1/kitchenowl/analysis/trailing",
    tag = "kitchenowl",
    params(
        ("year" = i32, Query, description = "Jahr des letzten Monats im Fenster"),
        ("month" = u8, Query, description = "Monat 1..12 — das Fenster endet hier"),
    ),
    responses(
        (status = 200, description = "Zwölf Monate rückwärts, über die Jahresgrenze hinweg", body = KoTrailingWindow),
        (status = 400, description = "Monat außerhalb 1..12"),
    ),
)]
pub async fn trailing(
    mut ctx: Ctx,
    Query(q): Query<KoTrailingQuery>,
) -> Result<Json<KoTrailingWindow>> {
    if !(1..=12).contains(&q.month) {
        return Err(AppError::Validation(
            "Monat muss zwischen 1 und 12 liegen".into(),
        ));
    }
    let to_ord = ord(q.year, q.month);
    let from_ord = to_ord - 11;
    let (from_year, from_month) = year_month(from_ord);

    let buckets = load_buckets(
        ctx.tenant.conn(),
        (from_year, from_month),
        (q.year, q.month),
    )
    .await?;

    // Twelve slots, oldest first, every one present. An empty month is information:
    // dropping it would slide the window and hide the gap.
    let mut months: Vec<KoTrailingMonth> = (0..12)
        .map(|i| {
            let (year, month) = year_month(from_ord + i);
            KoTrailingMonth {
                year,
                month,
                month_name: month_name_de(month).to_string(),
                amount_cents: 0,
                own_share_cents: 0,
                expense_count: 0,
            }
        })
        .collect();

    let mut rows: Vec<KoTrailingCategory> = Vec::new();
    for b in &buckets {
        let idx = (ord(b.year, b.month) - from_ord) as usize;
        if let Some(slot) = months.get_mut(idx) {
            slot.amount_cents += b.amount_cents;
            slot.own_share_cents += b.own_share_cents;
            slot.expense_count += b.expense_count;
        }
        let row = match rows
            .iter_mut()
            .position(|r| r.ko_category_id == b.category_id)
        {
            Some(i) => &mut rows[i],
            None => {
                rows.push(KoTrailingCategory {
                    ko_category_id: b.category_id,
                    ko_category_name: b.category_name.clone(),
                    amount_cents: 0,
                    own_share_cents: 0,
                    expense_count: 0,
                    average_per_month_cents: 0,
                    average_own_share_per_month_cents: 0,
                    monthly_amount_cents: vec![0; 12],
                    monthly_own_share_cents: vec![0; 12],
                });
                rows.last_mut().expect("just pushed")
            }
        };
        row.amount_cents += b.amount_cents;
        row.own_share_cents += b.own_share_cents;
        row.expense_count += b.expense_count;
        if let Some(slot) = row.monthly_amount_cents.get_mut(idx) {
            *slot += b.amount_cents;
        }
        if let Some(slot) = row.monthly_own_share_cents.get_mut(idx) {
            *slot += b.own_share_cents;
        }
    }

    let months_with_data = months.iter().filter(|m| m.expense_count > 0).count() as i64;
    for row in &mut rows {
        // Over the months the HOUSEHOLD was active in the window, never over twelve.
        row.average_per_month_cents = div_round_half_up(row.amount_cents, months_with_data.max(1));
        row.average_own_share_per_month_cents =
            div_round_half_up(row.own_share_cents, months_with_data.max(1));
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.amount_cents));

    let out = KoTrailingWindow {
        year: q.year,
        month: q.month,
        from_year,
        from_month,
        amount_cents: months.iter().map(|m| m.amount_cents).sum(),
        own_share_cents: months.iter().map(|m| m.own_share_cents).sum(),
        expense_count: months.iter().map(|m| m.expense_count).sum(),
        months_with_data,
        months,
        rows,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ord_round_trips_across_the_year_boundary() {
        // The trailing window rests on this: December to January is one step, and
        // the mirror's first expense is 2024-12-22, so that boundary is the first
        // thing this feature meets.
        assert_eq!(ord(2025, 1) - ord(2024, 12), 1);
        assert_eq!(year_month(ord(2025, 1)), (2025, 1));
        assert_eq!(year_month(ord(2025, 1) - 11), (2024, 2));
    }

    #[test]
    fn a_ratio_against_nothing_is_not_a_number() {
        // A category the household did not have last year is new, not infinitely
        // more expensive.
        assert_eq!(ratio(5_000, 0), None);
        assert_eq!(ratio(2_000, 10_000), Some(0.2));
        assert_eq!(ratio(-2_000, 10_000), Some(-0.2));
    }
}
