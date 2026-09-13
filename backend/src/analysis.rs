//! Dashboard, monthly overview, category analysis and the tax report.
//!
//! These load rows through `v_ledger` and hand them to the pure engine in
//! [`crate::calc`], so the SQL stays a projection and the arithmetic has exactly one
//! definition. The golden suite asserts the engine directly; this module is what
//! puts the same numbers on the wire.

use axum::{Json, extract::Query};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::{
    auth::Ctx,
    calc::{self, Kind, LedgerRow},
    error::{AppError, Result},
    locale::{div_round_half_up, month_name_de},
    models::{
        CategoryAnalysis, CategoryAnalysisRow, CategoryTypeSummary, Dashboard, MonthlyOverview,
        MonthlyRow, MonthlySeries, SeriesMonth, SeriesSubject, TaxCategorySummary, TaxEntry,
        TaxReport,
    },
};

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YearQuery {
    pub year: i32,
}

async fn load_ledger(conn: &mut PgConnection, year: i32) -> Result<Vec<LedgerRow>> {
    let rows = sqlx::query(
        "SELECT period_year, period_month, kind, amount_cents, category_id, category_name, \
                type_code, type_label, is_income, is_savings, in_consumption, tax_relevant \
           FROM v_ledger WHERE period_year = $1::smallint",
    )
    .bind(year)
    .fetch_all(&mut *conn)
    .await?;

    Ok(rows
        .iter()
        .map(|r| LedgerRow {
            period_year: r.get::<i16, _>("period_year") as i32,
            period_month: r.get::<i16, _>("period_month") as u8,
            kind: Kind::parse(r.get::<String, _>("kind").as_str()).unwrap_or(Kind::Expense),
            amount_cents: r.get("amount_cents"),
            category_id: r.get("category_id"),
            category_name: r.get("category_name"),
            type_code: r.get("type_code"),
            type_label: r.get("type_label"),
            is_income: r.get::<Option<bool>, _>("is_income").unwrap_or(false),
            is_savings: r.get::<Option<bool>, _>("is_savings").unwrap_or(false),
            in_consumption: r.get::<Option<bool>, _>("in_consumption").unwrap_or(false),
            tax_relevant: r.get("tax_relevant"),
        })
        .collect())
}

/// Opening balance, and whether it was configured by hand or derived from the
/// previous year's close.
async fn opening_balance(conn: &mut PgConnection, year: i32) -> Result<(i64, bool, Option<i64>)> {
    let row = sqlx::query(
        "SELECT opening_cents, opening_source = 'configured' AS configured \
           FROM fiscal_years WHERE year = $1::smallint",
    )
    .bind(year)
    .fetch_optional(&mut *conn)
    .await?;
    let (opening, configured) = match row {
        Some(r) => (
            r.get::<i64, _>("opening_cents"),
            r.get::<bool, _>("configured"),
        ),
        None => (0, false),
    };

    // The previous year's close, so a configured opening that disagrees can be
    // surfaced rather than silently diverging.
    let previous_close: Option<i64> = sqlx::query_scalar(
        "SELECT (f.opening_cents + COALESCE( \
                   (SELECT SUM(-l.net_cents) FROM v_ledger l \
                     WHERE l.period_year = f.year), 0))::bigint \
           FROM fiscal_years f WHERE f.year = $1::smallint",
    )
    .bind(year - 1)
    .fetch_optional(&mut *conn)
    .await?;

    let gap = match (configured, previous_close) {
        (true, Some(prev)) if prev != opening => Some(opening - prev),
        _ => None,
    };
    Ok((opening, configured, gap))
}

fn to_analysis_rows(rows: &[LedgerRow], months: i64) -> Vec<CategoryAnalysisRow> {
    let cats = calc::by_category(rows);
    let total_positive: i64 = cats.iter().map(|c| c.net_cents.max(0)).sum();
    cats.into_iter()
        .map(|c| CategoryAnalysisRow {
            category_id: c.category_id,
            category_name: c.category_name,
            category_type: c.type_label,
            income_cents: c.income_cents,
            expense_cents: c.expense_cents,
            net_cents: c.net_cents,
            net_is_negative: c.net_cents < 0,
            share_of_total: calc::share_of_total(c.net_cents, total_positive),
            average_per_month_cents: div_round_half_up(c.net_cents, months.max(1)),
            booking_count: c.booking_count,
            monthly_net_cents: c.monthly_net_cents.to_vec(),
        })
        .collect()
}

/// The tax report, whose running number ends up on a printed list handed to a tax
/// office — so the ordering has to be a function of the DATA, not of storage.
///
/// `created_at` defaults to `now()`, which inside a transaction is the transaction's
/// timestamp: every row of one import commit carries the identical value, so it
/// breaks no ties at all. That left `b.id` — a random uuid — as the real tiebreaker,
/// which made `Nr.` arbitrary within a month and reshuffled it on any re-import.
/// `comment, amount_cents` restore determinism from the booking's own content; `id`
/// stays last only to keep the sort total for genuinely identical rows.
#[utoipa::path(
    get,
    path = "/api/v1/dashboard",
    tag = "analysis",
    params(("year" = i32, Query, description = "Kalenderjahr")),
    responses((status = 200, description = "Jahreszahlen, beide Sparquoten, Top-Kategorien", body = Dashboard)),
)]
pub async fn dashboard(mut ctx: Ctx, Query(q): Query<YearQuery>) -> Result<Json<Dashboard>> {
    let rows = load_ledger(ctx.tenant.conn(), q.year).await?;
    let (opening, _, gap) = opening_balance(ctx.tenant.conn(), q.year).await?;

    let totals = calc::totals(&rows);
    let rates = calc::savings_rates(&rows);
    let months = calc::months_with_data(&rows);
    let by_type = calc::by_type(&rows)
        .into_iter()
        .map(|t| CategoryTypeSummary {
            type_code: t.type_code,
            label: t.type_label,
            net_cents: t.net_cents,
            booking_count: t.booking_count,
        })
        .collect();

    let mut top = to_analysis_rows(&rows, months);
    top.retain(|r| r.net_cents > 0);
    top.truncate(10);

    let out = Dashboard {
        year: q.year,
        income_cents: totals.income_cents,
        expense_cents: totals.expense_cents,
        balance_cents: totals.saldo_cents,
        opening_balance_cents: opening,
        closing_balance_cents: opening + totals.saldo_cents,
        carryover_gap_cents: gap,
        average_expense_per_month_cents: calc::average_expense_per_month(&rows),
        fixed_costs_per_month_cents: calc::fixed_costs_per_month(&rows),
        months_with_data: months,
        savings_rate_naive: rates.naive_rate,
        savings_rate_consumption: rates.consumption_rate,
        savings_amount_cents: rates.savings_amount_cents,
        booking_count: totals.booking_count,
        tax_relevant_count: rows.iter().filter(|r| r.tax_relevant).count() as i64,
        uncategorized_count: totals.uncategorized_count,
        uncategorized_net_cents: totals.uncategorized_net_cents,
        by_type,
        top_categories: top,
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[utoipa::path(
    get,
    path = "/api/v1/overview/months",
    tag = "analysis",
    params(("year" = i32, Query, description = "Kalenderjahr")),
    responses((status = 200, description = "Monatswerte netto je Typ, kumuliert", body = MonthlyOverview)),
)]
pub async fn monthly(mut ctx: Ctx, Query(q): Query<YearQuery>) -> Result<Json<MonthlyOverview>> {
    let rows = load_ledger(ctx.tenant.conn(), q.year).await?;
    let months = calc::by_month(&rows);
    let totals = calc::totals(&rows);

    let uncategorized_per_month = |month: u8| {
        rows.iter()
            .filter(|r| {
                r.period_month == month
                    && r.category_id.is_none()
                    && r.kind != crate::calc::Kind::Transfer
            })
            .count() as i64
    };

    let out_months: Vec<MonthlyRow> = months
        .iter()
        .map(|m| MonthlyRow {
            month: m.month,
            month_name: month_name_de(m.month).to_string(),
            income_cents: m.income_cents,
            expense_cents: m.expense_cents,
            balance_cents: m.saldo_cents,
            cumulative_cents: m.cumulative_cents,
            savings_rate: (m.income_cents > 0)
                .then(|| m.saldo_cents as f64 / m.income_cents as f64),
            fixed_costs_net_cents: m.fixed_net_cents,
            variable_costs_net_cents: m.variable_net_cents,
            savings_net_cents: m.savings_net_cents,
            other_net_cents: m.other_net_cents,
            booking_count: m.booking_count,
            uncategorized_count: uncategorized_per_month(m.month),
        })
        .collect();

    let total = MonthlyRow {
        month: 0,
        month_name: "Gesamt".into(),
        income_cents: totals.income_cents,
        expense_cents: totals.expense_cents,
        balance_cents: totals.saldo_cents,
        cumulative_cents: None,
        savings_rate: (totals.income_cents > 0)
            .then(|| totals.saldo_cents as f64 / totals.income_cents as f64),
        fixed_costs_net_cents: out_months.iter().map(|m| m.fixed_costs_net_cents).sum(),
        variable_costs_net_cents: out_months.iter().map(|m| m.variable_costs_net_cents).sum(),
        savings_net_cents: out_months.iter().map(|m| m.savings_net_cents).sum(),
        other_net_cents: out_months.iter().map(|m| m.other_net_cents).sum(),
        booking_count: totals.booking_count,
        uncategorized_count: totals.uncategorized_count,
    };

    ctx.tenant.commit().await?;
    Ok(Json(MonthlyOverview {
        year: q.year,
        months: out_months,
        total,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/analysis/categories",
    tag = "analysis",
    params(("year" = i32, Query, description = "Kalenderjahr")),
    responses((status = 200, description = "Netto je Kategorie mit beiden Bruttoschenkeln", body = CategoryAnalysis)),
)]
pub async fn categories(
    mut ctx: Ctx,
    Query(q): Query<YearQuery>,
) -> Result<Json<CategoryAnalysis>> {
    let rows = load_ledger(ctx.tenant.conn(), q.year).await?;
    let months = calc::months_with_data(&rows);
    let analysis_rows = to_analysis_rows(&rows, months);
    let totals = calc::totals(&rows);

    let out = CategoryAnalysis {
        year: q.year,
        total_net_cents: analysis_rows.iter().map(|r| r.net_cents.max(0)).sum(),
        months_with_data: months,
        uncategorized_count: totals.uncategorized_count,
        excluded_transfer_count: totals.transfer_count,
        rows: analysis_rows,
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[utoipa::path(
    get,
    path = "/api/v1/tax",
    tag = "tax",
    params(("year" = i32, Query, description = "Kalenderjahr")),
    responses((status = 200, description = "Steuerrelevante Buchungen mit stabiler Nummer", body = TaxReport)),
)]
pub async fn tax(mut ctx: Ctx, Query(q): Query<YearQuery>) -> Result<Json<TaxReport>> {
    let rows = sqlx::query(
        "SELECT b.id, b.period_month, b.comment, c.name AS category_name, b.kind, \
                b.amount_cents, \
                EXISTS (SELECT 1 FROM receipts r WHERE r.booking_id = b.id) AS has_receipt \
           FROM bookings b LEFT JOIN categories c ON c.id = b.category_id \
          WHERE b.tax_relevant AND b.status = 'confirmed' AND b.period_year = $1::smallint \
          ORDER BY b.period_ord, b.booked_on NULLS LAST, b.created_at, b.comment, b.amount_cents, b.id",
    )
    .bind(q.year)
    .fetch_all(ctx.tenant.conn())
    .await?;

    let mut entries = Vec::with_capacity(rows.len());
    let mut by_category: std::collections::BTreeMap<String, TaxCategorySummary> =
        std::collections::BTreeMap::new();
    let (mut total_expense, mut total_income, mut receipts_present) = (0i64, 0i64, 0i64);

    for (i, r) in rows.iter().enumerate() {
        let month: i16 = r.get("period_month");
        let kind: String = r.get("kind");
        let amount: i64 = r.get("amount_cents");
        let (income, expense) = if kind == "income" {
            (amount, 0)
        } else {
            (0, amount)
        };
        total_income += income;
        total_expense += expense;
        let has_receipt: bool = r.get("has_receipt");
        if has_receipt {
            receipts_present += 1;
        }

        let name: Option<String> = r.get("category_name");
        let key = name.clone().unwrap_or_else(|| "(ohne Kategorie)".into());
        let summary = by_category
            .entry(key.clone())
            .or_insert(TaxCategorySummary {
                category_name: key,
                expense_cents: 0,
                income_cents: 0,
                net_cents: 0,
                count: 0,
            });
        summary.expense_cents += expense;
        summary.income_cents += income;
        summary.net_cents += expense - income;
        summary.count += 1;

        entries.push(TaxEntry {
            booking_id: r.get("id"),
            index: i as i64 + 1,
            month: month as u8,
            month_name: month_name_de(month as u8).to_string(),
            comment: r.get("comment"),
            category_name: name,
            income_cents: income,
            expense_cents: expense,
            has_receipt,
        });
    }

    let out = TaxReport {
        year: q.year,
        total_expense_cents: total_expense,
        total_income_cents: total_income,
        total_net_cents: total_expense - total_income,
        booking_count: entries.len() as i64,
        receipts_present,
        entries,
        by_category: by_category.into_values().collect(),
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesQuery {
    pub year: i32,
    /// Exactly one of these. A category answers "what does Auto & Parken cost";
    /// a comment answers "what do I spend on tanken", which is the question the
    /// spreadsheet's Filter tab existed for and the one a household actually asks.
    pub category_id: Option<Uuid>,
    pub comment: Option<String>,
}

/// Twelve months for one category or one comment.
#[utoipa::path(
    get,
    path = "/api/v1/analysis/series",
    tag = "analysis",
    params(
        ("year" = i32, Query, description = "Kalenderjahr"),
        ("categoryId" = Option<Uuid>, Query, description = "Kategorie — genau eine von beiden"),
        ("comment" = Option<String>, Query, description = "Kommentar, z. B. tanken — Groß-/Kleinschreibung egal"),
    ),
    responses(
        (status = 200, description = "Zwölf Monate für eine Kategorie oder einen Kommentar", body = MonthlySeries),
        (status = 400, description = "Weder oder beide Parameter angegeben"),
    ),
)]
pub async fn series(mut ctx: Ctx, Query(q): Query<SeriesQuery>) -> Result<Json<MonthlySeries>> {
    let (mode, subject, where_sql): (&str, String, &str) = match (&q.category_id, &q.comment) {
        (Some(_), None) => ("category", String::new(), "b.category_id = $2"),
        // Matched case-insensitively on the trimmed comment, exactly as the rule
        // table matches, so "Tanken" and "tanken" are one subject and not two.
        (None, Some(c)) => (
            "comment",
            c.trim().to_string(),
            "b.match_key = lower(btrim($2))",
        ),
        _ => {
            return Err(AppError::Validation(
                "Genau eine von categoryId oder comment angeben".into(),
            ));
        }
    };

    let sql = format!(
        "SELECT b.period_month, \
                COALESCE(SUM(b.amount_cents) FILTER (WHERE b.kind = 'income'), 0)::bigint AS inc, \
                COALESCE(SUM(b.amount_cents) FILTER (WHERE b.kind = 'expense'), 0)::bigint AS exp, \
                COALESCE(SUM(b.net_cents), 0)::bigint AS net, \
                count(*)::bigint AS n \
           FROM v_ledger b \
          WHERE b.period_year = $1::smallint AND {where_sql} \
          GROUP BY b.period_month ORDER BY b.period_month"
    );

    let mut query = sqlx::query(&sql).bind(q.year);
    query = match (&q.category_id, &q.comment) {
        (Some(id), _) => query.bind(id),
        _ => query.bind(&subject),
    };
    let rows = query.fetch_all(ctx.tenant.conn()).await?;

    let mut months: Vec<SeriesMonth> = (1..=12u8)
        .map(|month| SeriesMonth {
            month,
            month_name: month_name_de(month).to_string(),
            income_cents: 0,
            expense_cents: 0,
            net_cents: 0,
            booking_count: 0,
        })
        .collect();

    for r in &rows {
        let month: i16 = r.get("period_month");
        if !(1..=12).contains(&month) {
            continue;
        }
        let slot = &mut months[(month - 1) as usize];
        slot.income_cents = r.get("inc");
        slot.expense_cents = r.get("exp");
        slot.net_cents = r.get("net");
        slot.booking_count = r.get("n");
    }

    // The name is looked up rather than echoed, so a renamed category answers with
    // its current name instead of whatever the caller happened to send.
    let subject = if mode == "category" {
        sqlx::query_scalar::<_, String>("SELECT name FROM categories WHERE id = $1")
            .bind(q.category_id)
            .fetch_optional(ctx.tenant.conn())
            .await?
            .ok_or_else(|| AppError::NotFound("Kategorie".into()))?
    } else {
        subject
    };

    let income_cents = months.iter().map(|m| m.income_cents).sum();
    let expense_cents = months.iter().map(|m| m.expense_cents).sum();
    let net_cents = months.iter().map(|m| m.net_cents).sum();
    let booking_count = months.iter().map(|m| m.booking_count).sum();
    let months_with_data = months.iter().filter(|m| m.booking_count > 0).count() as i64;

    let out = MonthlySeries {
        year: q.year,
        mode: mode.to_string(),
        subject,
        category_id: q.category_id,
        average_per_active_month_cents: div_round_half_up(net_cents, months_with_data.max(1)),
        months,
        income_cents,
        expense_cents,
        net_cents,
        booking_count,
        months_with_data,
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

/// The comments worth charting, most used first — what the picker offers.
#[utoipa::path(
    get,
    path = "/api/v1/analysis/series/subjects",
    tag = "analysis",
    params(("year" = i32, Query, description = "Kalenderjahr")),
    responses((status = 200, description = "Kommentare mit mehr als einer Buchung, häufigste zuerst", body = Vec<SeriesSubject>)),
)]
pub async fn series_subjects(
    mut ctx: Ctx,
    Query(q): Query<YearQuery>,
) -> Result<Json<Vec<SeriesSubject>>> {
    let rows = sqlx::query(
        "SELECT b.comment, count(*)::bigint AS n, \
                COALESCE(SUM(b.net_cents), 0)::bigint AS net, \
                max(b.category_name) AS category_name \
           FROM v_ledger b \
          WHERE b.period_year = $1::smallint \
          GROUP BY b.comment \
          HAVING count(*) > 1 \
          ORDER BY n DESC, b.comment LIMIT 300",
    )
    .bind(q.year)
    .fetch_all(ctx.tenant.conn())
    .await?;

    let out = rows
        .iter()
        .map(|r| SeriesSubject {
            comment: r.get("comment"),
            booking_count: r.get("n"),
            net_cents: r.get("net"),
            category_name: r.get("category_name"),
        })
        .collect();
    ctx.tenant.commit().await?;
    Ok(Json(out))
}
