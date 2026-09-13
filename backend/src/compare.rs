//! Year against year, and the trailing twelve months.
//!
//! 1.404 of the ~1.878 bookings in this ledger are from 2023–2025, and until this
//! module existed every screen looked at one calendar year at a time — so all of that
//! history was imported, verified, and then invisible.
//!
//! Two questions, two endpoints. "Is this year worse than last" is [`compare`]; "is
//! this getting worse" is [`trailing`], which needs a window that crosses the year
//! boundary because a calendar year is an accounting convention rather than a unit of
//! behaviour. In January a year-to-date view contains one month.
//!
//! **The trap.** 2026 holds nine months and 2025 holds twelve. Comparing their totals
//! makes this year look 25 % thriftier for no reason other than the calendar, and it
//! is the single easiest way for a comparison feature to lie. So every figure here is
//! returned twice: as the years stand, and restricted to `comparable_months` — the
//! months both years actually carry bookings in. `fully_comparable` says whether the
//! distinction matters, and the UI leads with the restricted pair when it does.
//!
//! No arithmetic is defined here. The rows go through [`crate::calc`] exactly as the
//! single-year analysis does; this module loads two sets of them, filters months, and
//! pairs the results up. Two implementations of netting is one too many.

use axum::{Json, extract::Query};
use sqlx::{PgConnection, Row};

use crate::{
    auth::Ctx,
    calc::{self, Kind, LedgerRow},
    error::{AppError, Result},
    locale::{div_round_half_up, month_name_de},
    models::{
        CompareRow, CompareTotals, CompareTypeRow, TrailingCategory, TrailingMonth, TrailingWindow,
        YearComparison,
    },
};

/// Calendar period as a single orderable integer, mirroring the generated
/// `period_ord` column: `year * 12 + month - 1`. The whole point of the trailing
/// window is that this arithmetic does not care where a year ends.
fn ord(year: i32, month: u8) -> i32 {
    year * 12 + month as i32 - 1
}

fn year_month(ord: i32) -> (i32, u8) {
    (ord.div_euclid(12), (ord.rem_euclid(12) + 1) as u8)
}

/// Rows for a closed `period_ord` range. One query for one window, including the
/// range that spans two years, which is what the column exists for.
async fn load_ledger_range(
    conn: &mut PgConnection,
    from_ord: i32,
    to_ord: i32,
) -> Result<Vec<LedgerRow>> {
    let rows = sqlx::query(
        "SELECT period_year, period_month, kind, amount_cents, category_id, category_name, \
                type_code, type_label, is_income, is_savings, in_consumption, tax_relevant \
           FROM v_ledger WHERE period_ord BETWEEN $1::int AND $2::int",
    )
    .bind(from_ord)
    .bind(to_ord)
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

/// Which months of a year carry at least one booking.
fn months_present(rows: &[LedgerRow]) -> std::collections::BTreeSet<u8> {
    rows.iter().map(|r| r.period_month).collect()
}

/// `delta / |previous|`, or `None` when there is no previous to be a share of.
///
/// Dividing by zero would produce infinity, and rendering "+∞ %" for a category that
/// simply did not exist last year is worse than rendering nothing: `is_new` says what
/// actually happened.
fn ratio(delta_cents: i64, previous_cents: i64) -> Option<f64> {
    if previous_cents == 0 {
        None
    } else {
        Some(delta_cents as f64 / (previous_cents.abs() as f64))
    }
}

fn totals_for(year: i32, all: &[LedgerRow], comparable: &[LedgerRow]) -> CompareTotals {
    let t = calc::totals(all);
    let c = calc::totals(comparable);
    CompareTotals {
        year,
        income_cents: t.income_cents,
        expense_cents: t.expense_cents,
        saldo_cents: t.saldo_cents,
        booking_count: t.booking_count,
        months_with_data: calc::months_with_data(all),
        last_month_with_data: all.iter().map(|r| r.period_month).max(),
        comparable_income_cents: c.income_cents,
        comparable_expense_cents: c.expense_cents,
        comparable_saldo_cents: c.saldo_cents,
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareQuery {
    pub year: i32,
}

/// This year against last, per category and per type.
#[utoipa::path(
    get,
    path = "/api/v1/analysis/compare",
    tag = "analysis",
    params(("year" = i32, Query, description = "Kalenderjahr; verglichen wird mit dem Vorjahr")),
    responses((status = 200, description = "Jahr gegen Vorjahr, roh und auf gemeinsame Monate beschränkt", body = YearComparison)),
)]
pub async fn compare(mut ctx: Ctx, Query(q): Query<CompareQuery>) -> Result<Json<YearComparison>> {
    let previous_year = q.year - 1;
    let current = load_ledger_range(ctx.tenant.conn(), ord(q.year, 1), ord(q.year, 12)).await?;
    let previous = load_ledger_range(
        ctx.tenant.conn(),
        ord(previous_year, 1),
        ord(previous_year, 12),
    )
    .await?;

    // The months both years have. Intersection rather than "first N months": a year
    // with a gap in the middle is a real thing in this data, and taking a prefix
    // would compare August with February.
    let cur_months = months_present(&current);
    let prev_months = months_present(&previous);
    let comparable: Vec<u8> = cur_months.intersection(&prev_months).copied().collect();
    let in_window = |r: &LedgerRow| comparable.contains(&r.period_month);
    let current_cmp: Vec<LedgerRow> = current.iter().filter(|r| in_window(r)).cloned().collect();
    let previous_cmp: Vec<LedgerRow> = previous.iter().filter(|r| in_window(r)).cloned().collect();

    // Keyed by name, which is how `calc::by_category` already groups: a category
    // renamed between the two years is genuinely a different row, and pretending
    // otherwise would silently merge two histories.
    let mut cur_cats: std::collections::BTreeMap<String, calc::CategoryNet> =
        calc::by_category(&current)
            .into_iter()
            .map(|c| (c.category_name.clone(), c))
            .collect();
    let mut prev_cats: std::collections::BTreeMap<String, calc::CategoryNet> =
        calc::by_category(&previous)
            .into_iter()
            .map(|c| (c.category_name.clone(), c))
            .collect();
    let cur_cmp_cats: std::collections::BTreeMap<String, calc::CategoryNet> =
        calc::by_category(&current_cmp)
            .into_iter()
            .map(|c| (c.category_name.clone(), c))
            .collect();
    let prev_cmp_cats: std::collections::BTreeMap<String, calc::CategoryNet> =
        calc::by_category(&previous_cmp)
            .into_iter()
            .map(|c| (c.category_name.clone(), c))
            .collect();

    let mut names: Vec<String> = cur_cats.keys().cloned().collect();
    names.extend(prev_cats.keys().cloned());
    names.sort();
    names.dedup();

    let mut rows: Vec<CompareRow> = names
        .into_iter()
        .map(|name| {
            let cur = cur_cats.remove(&name);
            let prev = prev_cats.remove(&name);
            let cur_cmp = cur_cmp_cats.get(&name);
            let prev_cmp = prev_cmp_cats.get(&name);

            let net = cur.as_ref().map(|c| c.net_cents).unwrap_or(0);
            let previous_net = prev.as_ref().map(|c| c.net_cents).unwrap_or(0);
            let cmp_net = cur_cmp.map(|c| c.net_cents).unwrap_or(0);
            let cmp_previous_net = prev_cmp.map(|c| c.net_cents).unwrap_or(0);
            let booking_count = cur.as_ref().map(|c| c.booking_count).unwrap_or(0);
            let previous_booking_count = prev.as_ref().map(|c| c.booking_count).unwrap_or(0);

            CompareRow {
                category_id: cur
                    .as_ref()
                    .and_then(|c| c.category_id)
                    .or_else(|| prev.as_ref().and_then(|c| c.category_id)),
                category_type: cur
                    .as_ref()
                    .and_then(|c| c.type_label.clone())
                    .or_else(|| prev.as_ref().and_then(|c| c.type_label.clone())),
                category_name: name,
                net_cents: net,
                previous_net_cents: previous_net,
                delta_cents: net - previous_net,
                delta_ratio: ratio(net - previous_net, previous_net),
                comparable_net_cents: cmp_net,
                comparable_previous_net_cents: cmp_previous_net,
                comparable_delta_cents: cmp_net - cmp_previous_net,
                comparable_delta_ratio: ratio(cmp_net - cmp_previous_net, cmp_previous_net),
                monthly_net_cents: cur
                    .as_ref()
                    .map(|c| c.monthly_net_cents.to_vec())
                    .unwrap_or_else(|| vec![0; 12]),
                previous_monthly_net_cents: prev
                    .as_ref()
                    .map(|c| c.monthly_net_cents.to_vec())
                    .unwrap_or_else(|| vec![0; 12]),
                booking_count,
                previous_booking_count,
                is_new: previous_booking_count == 0 && booking_count > 0,
                is_gone: booking_count == 0 && previous_booking_count > 0,
            }
        })
        .collect();
    // Largest movement first, in either direction: the point of the screen is what
    // changed, not what is biggest.
    rows.sort_by(|a, b| {
        b.delta_cents
            .abs()
            .cmp(&a.delta_cents.abs())
            .then(a.category_name.cmp(&b.category_name))
    });

    let mut cur_types: std::collections::BTreeMap<String, calc::TypeNet> = calc::by_type(&current)
        .into_iter()
        .map(|t| (t.type_code.clone(), t))
        .collect();
    let mut prev_types: std::collections::BTreeMap<String, calc::TypeNet> =
        calc::by_type(&previous)
            .into_iter()
            .map(|t| (t.type_code.clone(), t))
            .collect();
    let cur_cmp_types: std::collections::BTreeMap<String, calc::TypeNet> =
        calc::by_type(&current_cmp)
            .into_iter()
            .map(|t| (t.type_code.clone(), t))
            .collect();
    let prev_cmp_types: std::collections::BTreeMap<String, calc::TypeNet> =
        calc::by_type(&previous_cmp)
            .into_iter()
            .map(|t| (t.type_code.clone(), t))
            .collect();

    let mut type_codes: Vec<String> = cur_types.keys().cloned().collect();
    type_codes.extend(prev_types.keys().cloned());
    type_codes.sort();
    type_codes.dedup();

    let by_type: Vec<CompareTypeRow> = type_codes
        .into_iter()
        .map(|code| {
            let cur = cur_types.remove(&code);
            let prev = prev_types.remove(&code);
            let net = cur.as_ref().map(|t| t.net_cents).unwrap_or(0);
            let previous_net = prev.as_ref().map(|t| t.net_cents).unwrap_or(0);
            let cmp_net = cur_cmp_types.get(&code).map(|t| t.net_cents).unwrap_or(0);
            let cmp_prev_net = prev_cmp_types.get(&code).map(|t| t.net_cents).unwrap_or(0);
            CompareTypeRow {
                label: cur
                    .as_ref()
                    .map(|t| t.type_label.clone())
                    .or_else(|| prev.as_ref().map(|t| t.type_label.clone()))
                    .unwrap_or_default(),
                type_code: code,
                net_cents: net,
                previous_net_cents: previous_net,
                delta_cents: net - previous_net,
                delta_ratio: ratio(net - previous_net, previous_net),
                comparable_net_cents: cmp_net,
                comparable_previous_net_cents: cmp_prev_net,
                comparable_delta_cents: cmp_net - cmp_prev_net,
            }
        })
        .collect();

    let out = YearComparison {
        year: q.year,
        previous_year,
        current: totals_for(q.year, &current, &current_cmp),
        previous: totals_for(previous_year, &previous, &previous_cmp),
        fully_comparable: cur_months == prev_months && !cur_months.is_empty(),
        comparable_months: comparable,
        rows,
        by_type,
        previous_year_has_data: !previous.is_empty(),
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrailingQuery {
    pub year: i32,
    pub month: u8,
}

/// The twelve months ending at the given period, whatever years they fall in.
#[utoipa::path(
    get,
    path = "/api/v1/analysis/trailing",
    tag = "analysis",
    params(
        ("year" = i32, Query, description = "Jahr des letzten Monats im Fenster"),
        ("month" = u8, Query, description = "Monat 1..12 — das Fenster endet hier"),
    ),
    responses(
        (status = 200, description = "Zwölf Monate rückwärts, über die Jahresgrenze hinweg", body = TrailingWindow),
        (status = 400, description = "Monat außerhalb 1..12"),
    ),
)]
pub async fn trailing(
    mut ctx: Ctx,
    Query(q): Query<TrailingQuery>,
) -> Result<Json<TrailingWindow>> {
    if !(1..=12).contains(&q.month) {
        return Err(AppError::Validation(
            "Monat muss zwischen 1 und 12 liegen".into(),
        ));
    }
    let to_ord = ord(q.year, q.month);
    let from_ord = to_ord - 11;
    let (from_year, from_month) = year_month(from_ord);

    let rows = load_ledger_range(ctx.tenant.conn(), from_ord, to_ord).await?;

    // Twelve slots in window order, oldest first — and every one of them present,
    // including the empty ones. A month with nothing in it is information; dropping
    // it would slide the line and hide the gap.
    let mut months: Vec<TrailingMonth> = (0..12)
        .map(|i| {
            let (year, month) = year_month(from_ord + i);
            TrailingMonth {
                year,
                month,
                month_name: month_name_de(month).to_string(),
                income_cents: 0,
                expense_cents: 0,
                saldo_cents: 0,
                net_cents: 0,
                booking_count: 0,
            }
        })
        .collect();

    for r in &rows {
        let idx = (ord(r.period_year, r.period_month) - from_ord) as usize;
        let Some(slot) = months.get_mut(idx) else {
            continue;
        };
        match r.kind {
            Kind::Income => slot.income_cents += r.amount_cents,
            Kind::Expense => slot.expense_cents += r.amount_cents,
            Kind::Transfer => {}
        }
        slot.net_cents += r.net_cents();
        slot.booking_count += 1;
    }
    for m in &mut months {
        m.saldo_cents = m.income_cents - m.expense_cents;
    }

    let months_with_data = months.iter().filter(|m| m.booking_count > 0).count() as i64;

    // Categories over the window. `by_category` buckets by CALENDAR month, which is
    // the wrong axis here, so the twelve values are re-bucketed by window position.
    let mut rows_out: Vec<TrailingCategory> = calc::by_category(&rows)
        .into_iter()
        .map(|c| TrailingCategory {
            category_id: c.category_id,
            category_name: c.category_name,
            category_type: c.type_label,
            net_cents: c.net_cents,
            average_per_month_cents: div_round_half_up(c.net_cents, months_with_data.max(1)),
            booking_count: c.booking_count,
            monthly_net_cents: vec![0; 12],
        })
        .collect();
    for r in &rows {
        if r.kind == Kind::Transfer {
            continue;
        }
        let name = r
            .category_name
            .clone()
            .unwrap_or_else(|| "(ohne Kategorie)".to_string());
        let idx = (ord(r.period_year, r.period_month) - from_ord) as usize;
        if let Some(row) = rows_out.iter_mut().find(|x| x.category_name == name)
            && let Some(slot) = row.monthly_net_cents.get_mut(idx)
        {
            *slot += r.net_cents();
        }
    }

    let out = TrailingWindow {
        year: q.year,
        month: q.month,
        from_year,
        from_month,
        income_cents: months.iter().map(|m| m.income_cents).sum(),
        expense_cents: months.iter().map(|m| m.expense_cents).sum(),
        saldo_cents: months.iter().map(|m| m.saldo_cents).sum(),
        booking_count: months.iter().map(|m| m.booking_count).sum(),
        months_with_data,
        months,
        rows: rows_out,
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ord_round_trips_across_the_year_boundary() {
        // The whole trailing window rests on this: December to January must be one
        // step, not a jump of eleven.
        assert_eq!(ord(2026, 1) - ord(2025, 12), 1);
        assert_eq!(year_month(ord(2026, 1)), (2026, 1));
        assert_eq!(year_month(ord(2025, 12)), (2025, 12));
        // Eleven back from January 2026 lands in February 2025.
        assert_eq!(year_month(ord(2026, 1) - 11), (2025, 2));
    }

    #[test]
    fn a_ratio_against_nothing_is_not_a_number() {
        assert_eq!(ratio(5_000, 0), None);
        // Against an income base the sign follows the DELTA, not the base: a
        // category that earned more has a negative delta in the stored convention.
        assert_eq!(ratio(-2_500, -25_000), Some(-0.1));
        assert_eq!(ratio(1_000, 10_000), Some(0.1));
    }
}
