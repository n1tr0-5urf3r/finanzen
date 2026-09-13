//! What the rest of the year looks like, and which categories are behaving oddly.
//!
//! Both answers come from data the app already holds, which is the reason to build
//! them at all: the recurring templates encode the fixed side of every remaining
//! month exactly, and nine months of history give every category a typical value.
//! Nobody has to maintain a budget for either.
//!
//! **Median, never mean.** One 1.440,00 € Nebenkosten settlement in a six-month
//! window moves a mean by ~250 € a month and a median not at all. The same argument
//! decides the spread: median absolute deviation, not standard deviation.
//!
//! **A projection is never presented as an actual.** Every month carries
//! `isProjected`, and the two are summed separately, because a forecast that reads
//! like a fact is worse than no forecast.
//!
//! Sign convention is the stored one throughout: `net_cents` is expenses minus
//! income, so a cost is POSITIVE and a balance delta is `-net_cents`.

use axum::{Json, extract::Query};
use serde::Serialize;
use sqlx::{PgConnection, Row};
use utoipa::ToSchema;

use crate::{
    analysis::opening_balance,
    auth::Ctx,
    error::{AppError, Result},
    locale::{div_round_half_up, month_name_de, ord_to_year_month, period_ord},
    recurring::DUE_SQL,
};

/// How many months of history a median is taken over.
const WINDOW_MONTHS: i32 = 6;

/// Below this a category has no median worth reporting — two points have no middle
/// and three is the fewest that can outvote a single odd month.
const MIN_HISTORY_MONTHS: usize = 3;

/// A category must be at least this far from its median, in both relative and
/// absolute terms, before it is worth interrupting anyone about. Ratio alone makes
/// a 4 € category that doubled look like news; cents alone makes every large
/// category permanently newsworthy.
const ANOMALY_MIN_RATIO: f64 = 1.4;
const ANOMALY_MIN_DELTA_CENTS: i64 = 2_000;

// ----------------------------------------------------------------- pure arithmetic

/// The middle value, averaging the two middles for an even count.
///
/// Takes a slice and sorts a copy: callers hold history in the order the database
/// returned it and would otherwise each have to remember to sort.
pub fn median(values: &[i64]) -> i64 {
    if values.is_empty() {
        return 0;
    }
    let mut v = values.to_vec();
    v.sort_unstable();
    let mid = v.len() / 2;
    if v.len() % 2 == 1 {
        v[mid]
    } else {
        div_round_half_up(v[mid - 1] + v[mid], 2)
    }
}

/// Median absolute deviation: the median of the distances from the median.
///
/// This is the spread that matches the estimate. A standard deviation would be
/// dragged by exactly the outlier month the median was chosen to ignore, and the
/// range around the projection would then be widest precisely where the projection
/// is most trustworthy.
pub fn median_absolute_deviation(values: &[i64], centre: i64) -> i64 {
    if values.is_empty() {
        return 0;
    }
    let deviations: Vec<i64> = values.iter().map(|v| (v - centre).abs()).collect();
    median(&deviations)
}

/// Which of two figures to project with.
///
/// The template is what is KNOWN to be due; the median is what the category
/// typically costs in total. A category can have both — Internet & Telefon is a
/// 64,94 standing order plus occasional Handyguthaben — so taking the larger
/// magnitude means a standing order is never under-projected and a category with
/// extra spending is not under-projected either. Adding them would double-count the
/// standing order, which is the trap here.
fn projected_for(template_cents: i64, median_cents: Option<i64>) -> (i64, &'static str) {
    match median_cents {
        Some(m) if m.abs() > template_cents.abs() => (m, "median"),
        Some(_) if template_cents != 0 => (template_cents, "template"),
        Some(m) => (m, "median"),
        None => (template_cents, "template"),
    }
}

// ----------------------------------------------------------------- loading

/// One category's net in one month.
struct HistoryRow {
    category_name: String,
    type_label: Option<String>,
    is_savings: bool,
    ord: i32,
    net_cents: i64,
}

/// Per-category monthly nets over a closed `period_ord` window.
///
/// A window, not a year: the six months before January 2027 are mostly in 2026, and
/// a forecast that silently restarted its history every 1 January would be at its
/// least informed exactly when the year is longest.
async fn load_history(
    conn: &mut PgConnection,
    from_ord: i32,
    to_ord: i32,
) -> Result<Vec<HistoryRow>> {
    let rows = sqlx::query(
        "SELECT COALESCE(l.category_name, '(ohne Kategorie)') AS name, \
                max(l.type_label) AS type_label, \
                bool_or(COALESCE(l.is_savings, false)) AS is_savings, \
                l.period_ord AS ord, \
                COALESCE(SUM(l.net_cents), 0)::bigint AS net \
           FROM v_ledger l \
          WHERE l.period_ord BETWEEN $1::int AND $2::int AND l.kind <> 'transfer' \
          GROUP BY name, l.period_ord \
          ORDER BY name, l.period_ord",
    )
    .bind(from_ord)
    .bind(to_ord)
    .fetch_all(&mut *conn)
    .await?;

    Ok(rows
        .iter()
        .map(|r| HistoryRow {
            category_name: r.get("name"),
            type_label: r.get("type_label"),
            is_savings: r.get::<Option<bool>, _>("is_savings").unwrap_or(false),
            ord: r.get("ord"),
            net_cents: r.get("net"),
        })
        .collect())
}

/// What the templates say one month will cost, per category.
///
/// The due predicate is [`crate::recurring::DUE_SQL`] verbatim rather than restated:
/// the listing, the materialiser and this projection have to agree about what "due"
/// means, and three copies of a modulo would not stay in agreement.
async fn templates_due(conn: &mut PgConnection, period: i32) -> Result<Vec<(String, i64, i64)>> {
    let sql = format!(
        // `GROUP BY 1`, not `GROUP BY name`: `recurring_templates` has a `name`
        // column of its own, so the bare name binds to the TEMPLATE's name and
        // Postgres then rejects the ungrouped `c.name` in the select list. The
        // positional form can only mean the output column.
        "SELECT COALESCE(c.name, rc.name, '(ohne Kategorie)') AS category_name, \
                COALESCE(SUM(CASE r.kind WHEN 'expense' THEN r.amount_cents \
                                         WHEN 'income'  THEN -r.amount_cents \
                                         ELSE 0 END), 0)::bigint AS net, \
                count(*)::bigint AS n \
           FROM recurring_templates r \
           LEFT JOIN categories c ON c.id = r.category_id \
           LEFT JOIN category_rules cr ON r.category_id IS NULL \
                                      AND cr.match_key = lower(btrim(r.comment)) \
           LEFT JOIN categories rc ON rc.id = cr.category_id \
          WHERE {DUE_SQL} \
          GROUP BY 1"
    );
    let rows = sqlx::query(&sql).bind(period).fetch_all(&mut *conn).await?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                r.get::<String, _>("category_name"),
                r.get::<i64, _>("net"),
                r.get::<i64, _>("n"),
            )
        })
        .collect())
}

/// The months of `year` that actually hold bookings, as (month, net, count).
async fn actual_months(conn: &mut PgConnection, year: i32) -> Result<Vec<(u8, i64, i64)>> {
    let rows = sqlx::query(
        "SELECT l.period_month AS m, \
                COALESCE(SUM(l.net_cents), 0)::bigint AS net, \
                count(*)::bigint AS n \
           FROM v_ledger l WHERE l.period_year = $1::smallint \
          GROUP BY l.period_month ORDER BY l.period_month",
    )
    .bind(year)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                r.get::<i16, _>("m") as u8,
                r.get::<i64, _>("net"),
                r.get::<i64, _>("n"),
            )
        })
        .collect())
}

// ----------------------------------------------------------------- forecast

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForecastMonth {
    pub month: u8,
    /// German month name; data, not chrome.
    pub month_name: String,
    /// Expense-positive, like every stored net: a cost is positive.
    pub net_cents: i64,
    /// The part the recurring templates account for. Zero in an actual month —
    /// there the figure is what happened, not what was planned.
    pub fixed_cents: i64,
    /// What the medians add on top of the templates.
    pub variable_cents: i64,
    /// Half-width of the range, from the median absolute deviation. Zero for an
    /// actual month, which has no uncertainty left.
    pub spread_cents: i64,
    /// The distinction that must survive into the UI.
    pub is_projected: bool,
    pub booking_count: i64,
    /// Running balance at the end of this month, projections included.
    pub closing_balance_cents: i64,
}

/// One category's contribution to the projection, so the figure can be inspected
/// rather than believed.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForecastBasisRow {
    pub category_name: String,
    pub category_type: Option<String>,
    pub months_of_history: i64,
    /// Per month, expense-positive.
    pub median_cents: i64,
    /// Over all remaining months.
    pub projected_total_cents: i64,
    /// `template`, `median` or `mixed` — where the figure came from.
    pub source: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Forecast {
    pub year: i32,
    pub opening_balance_cents: i64,
    /// The last month that holds bookings; `null` for a year with none.
    pub actual_through_month: Option<u8>,
    /// The first projected month; `null` when the year is complete.
    pub projected_from_month: Option<u8>,
    pub months: Vec<ForecastMonth>,
    /// Actual saldo so far — income minus expenses, so a gain is positive.
    pub actual_balance_cents: i64,
    /// The same for the projected months alone.
    pub projected_balance_cents: i64,
    pub projected_closing_balance_cents: i64,
    pub projected_closing_low_cents: i64,
    pub projected_closing_high_cents: i64,
    /// How many templates fall due across the remaining months, counted once each
    /// time they are due.
    pub due_template_count: i64,
    /// Months of history the medians were taken over.
    pub history_months: i64,
    /// Stated on the wire so the UI never has to guess what it is drawing.
    pub method: String,
    pub rows: Vec<ForecastBasisRow>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForecastQuery {
    pub year: i32,
}

/// The rest of the year, from what is known and what is typical.
#[utoipa::path(
    get,
    path = "/api/v1/analysis/forecast",
    tag = "analysis",
    params(("year" = i32, Query, description = "Kalenderjahr")),
    responses((status = 200, description = "Ist-Monate und projizierte Monate, getrennt ausgewiesen", body = Forecast)),
)]
pub async fn forecast(mut ctx: Ctx, Query(q): Query<ForecastQuery>) -> Result<Json<Forecast>> {
    let (opening, _, _) = opening_balance(ctx.tenant.conn(), q.year).await?;
    let actuals = actual_months(ctx.tenant.conn(), q.year).await?;

    let actual_through = actuals.last().map(|(m, _, _)| *m);
    let first_projected = actual_through.map_or(1, |m| m + 1);

    // History window: the six months with data ending at the last actual month. It
    // is expressed in period_ord, so it crosses the year boundary by construction.
    let window_end = period_ord(q.year, actual_through.unwrap_or(1));
    let window_start = window_end - (WINDOW_MONTHS - 1);
    let history = load_history(ctx.tenant.conn(), window_start, window_end).await?;

    // Savings categories stay in: a standing order into an ETF is money that really
    // leaves the account, so a projection that dropped it would flatter the balance.
    struct Stat {
        type_label: Option<String>,
        nets: Vec<i64>,
    }
    let mut stats: std::collections::BTreeMap<String, Stat> = std::collections::BTreeMap::new();
    for h in &history {
        let e = stats
            .entry(h.category_name.clone())
            .or_insert_with(|| Stat {
                type_label: h.type_label.clone(),
                nets: Vec::new(),
            });
        e.nets.push(h.net_cents);
    }

    let mut months: Vec<ForecastMonth> = Vec::with_capacity(12);
    let mut running = opening;
    let mut actual_balance = 0i64;
    let mut projected_balance = 0i64;
    let mut due_template_count = 0i64;
    // Per category, what the projection adds across every remaining month.
    let mut projected_totals: std::collections::BTreeMap<String, (i64, &'static str)> =
        std::collections::BTreeMap::new();

    for month in 1..=12u8 {
        if let Some((_, net, n)) = actuals.iter().find(|(m, _, _)| *m == month) {
            running += -net;
            actual_balance += -net;
            months.push(ForecastMonth {
                month,
                month_name: month_name_de(month).to_string(),
                net_cents: *net,
                fixed_cents: 0,
                variable_cents: 0,
                spread_cents: 0,
                is_projected: false,
                booking_count: *n,
                closing_balance_cents: running,
            });
            continue;
        }
        if month < first_projected {
            // A gap inside the actual range: no bookings, and nothing to project
            // either, because the months around it are facts.
            months.push(ForecastMonth {
                month,
                month_name: month_name_de(month).to_string(),
                net_cents: 0,
                fixed_cents: 0,
                variable_cents: 0,
                spread_cents: 0,
                is_projected: false,
                booking_count: 0,
                closing_balance_cents: running,
            });
            continue;
        }

        let period = period_ord(q.year, month);
        let due = templates_due(ctx.tenant.conn(), period).await?;
        due_template_count += due.iter().map(|(_, _, n)| n).sum::<i64>();

        let mut fixed = 0i64;
        let mut net = 0i64;
        let mut spread = 0i64;
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

        for (name, template_net, _) in &due {
            fixed += template_net;
            let stat = stats.get(name);
            let med = stat
                .filter(|s| s.nets.len() >= MIN_HISTORY_MONTHS)
                .map(|s| median(&s.nets));
            let (value, source) = projected_for(*template_net, med);
            net += value;
            if source == "median"
                && let Some(s) = stat
            {
                spread += median_absolute_deviation(&s.nets, median(&s.nets));
            }
            let entry = projected_totals.entry(name.clone()).or_insert((0, source));
            entry.0 += value;
            if entry.1 != source {
                entry.1 = "mixed";
            }
            seen.insert(name.clone());
        }

        for (name, stat) in &stats {
            if seen.contains(name) || stat.nets.len() < MIN_HISTORY_MONTHS {
                continue;
            }
            let med = median(&stat.nets);
            net += med;
            spread += median_absolute_deviation(&stat.nets, med);
            let entry = projected_totals
                .entry(name.clone())
                .or_insert((0, "median"));
            entry.0 += med;
            if entry.1 != "median" {
                entry.1 = "mixed";
            }
        }

        running += -net;
        projected_balance += -net;
        months.push(ForecastMonth {
            month,
            month_name: month_name_de(month).to_string(),
            net_cents: net,
            fixed_cents: fixed,
            variable_cents: net - fixed,
            spread_cents: spread,
            is_projected: true,
            booking_count: 0,
            closing_balance_cents: running,
        });
    }

    let total_spread: i64 = months.iter().map(|m| m.spread_cents).sum();
    let rows = {
        let mut rows: Vec<ForecastBasisRow> = projected_totals
            .into_iter()
            .map(|(name, (total, source))| {
                let stat = stats.get(&name);
                ForecastBasisRow {
                    category_name: name,
                    category_type: stat.and_then(|s| s.type_label.clone()),
                    months_of_history: stat.map_or(0, |s| s.nets.len() as i64),
                    median_cents: stat.map_or(0, |s| median(&s.nets)),
                    projected_total_cents: total,
                    source: source.to_string(),
                }
            })
            .collect();
        rows.sort_by(|a, b| {
            b.projected_total_cents
                .cmp(&a.projected_total_cents)
                .then(a.category_name.cmp(&b.category_name))
        });
        rows
    };

    let history_months: i64 = history
        .iter()
        .map(|h| h.ord)
        .collect::<std::collections::BTreeSet<_>>()
        .len() as i64;

    let out = Forecast {
        year: q.year,
        opening_balance_cents: opening,
        actual_through_month: actual_through,
        projected_from_month: (first_projected <= 12).then_some(first_projected),
        actual_balance_cents: actual_balance,
        projected_balance_cents: projected_balance,
        projected_closing_balance_cents: running,
        projected_closing_low_cents: running - total_spread,
        projected_closing_high_cents: running + total_spread,
        due_template_count,
        history_months,
        method: "median".to_string(),
        months,
        rows,
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

// ----------------------------------------------------------------- anomalies

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Anomaly {
    pub category_name: String,
    pub category_type: Option<String>,
    /// Expense-positive, like every stored net.
    pub current_cents: i64,
    pub median_cents: i64,
    /// Current minus median: positive means it cost more than usual.
    pub delta_cents: i64,
    pub ratio: f64,
    /// `above` or `below`, so the UI does not have to infer it from a sign it may
    /// be displaying flipped.
    pub direction: String,
    pub months_of_history: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnomalyReport {
    pub year: i32,
    pub month: u8,
    pub month_name: String,
    /// Empty is the normal case and must render as nothing at all.
    pub items: Vec<Anomaly>,
    pub compared_months: i64,
    pub min_ratio: f64,
    pub min_delta_cents: i64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnomalyQuery {
    pub year: i32,
    pub month: u8,
}

/// Categories running unusually far from their own median, in either direction.
///
/// Deliberately quiet: no budgets to maintain, no thresholds to configure, and an
/// empty list whenever nothing is unusual — which is most months.
#[utoipa::path(
    get,
    path = "/api/v1/analysis/anomalies",
    tag = "analysis",
    params(
        ("year" = i32, Query, description = "Kalenderjahr"),
        ("month" = u8, Query, description = "Monat 1..12"),
    ),
    responses(
        (status = 200, description = "Auffällige Kategorien; leer ist der Normalfall", body = AnomalyReport),
        (status = 400, description = "Monat außerhalb 1..12"),
    ),
)]
pub async fn anomalies(mut ctx: Ctx, Query(q): Query<AnomalyQuery>) -> Result<Json<AnomalyReport>> {
    if !(1..=12).contains(&q.month) {
        return Err(AppError::Validation(
            "Monat muss zwischen 1 und 12 liegen".into(),
        ));
    }
    let current_ord = period_ord(q.year, q.month);
    // The window ENDS the month before: comparing a month with a median that
    // includes it would pull the median towards the very thing being tested.
    let window_end = current_ord - 1;
    let window_start = window_end - (WINDOW_MONTHS - 1);

    let history = load_history(ctx.tenant.conn(), window_start, window_end).await?;
    let current = load_history(ctx.tenant.conn(), current_ord, current_ord).await?;

    struct Stat {
        type_label: Option<String>,
        nets: Vec<i64>,
        is_savings: bool,
    }
    let mut stats: std::collections::BTreeMap<String, Stat> = std::collections::BTreeMap::new();
    for h in &history {
        let e = stats
            .entry(h.category_name.clone())
            .or_insert_with(|| Stat {
                type_label: h.type_label.clone(),
                nets: Vec::new(),
                is_savings: h.is_savings,
            });
        e.nets.push(h.net_cents);
    }

    let mut items: Vec<Anomaly> = Vec::new();
    for c in &current {
        // Saving is a decision, not overspending; transfers are already excluded by
        // the query. Neither belongs in a list of things to look at.
        if c.is_savings {
            continue;
        }
        let Some(stat) = stats.get(&c.category_name) else {
            continue;
        };
        if stat.is_savings || stat.nets.len() < MIN_HISTORY_MONTHS {
            continue;
        }
        let med = median(&stat.nets);
        if med == 0 {
            continue;
        }
        let delta = c.net_cents - med;
        if delta.abs() < ANOMALY_MIN_DELTA_CENTS {
            continue;
        }
        // Magnitudes, so an income category that earned more is "above" its usual
        // size rather than mathematically below it.
        let ratio = c.net_cents.abs() as f64 / med.abs() as f64;
        if ratio < ANOMALY_MIN_RATIO && ratio > 1.0 / ANOMALY_MIN_RATIO {
            continue;
        }
        items.push(Anomaly {
            category_name: c.category_name.clone(),
            category_type: c.type_label.clone().or_else(|| stat.type_label.clone()),
            current_cents: c.net_cents,
            median_cents: med,
            delta_cents: delta,
            ratio,
            direction: if ratio >= 1.0 { "above" } else { "below" }.to_string(),
            months_of_history: stat.nets.len() as i64,
        });
    }
    items.sort_by_key(|a| std::cmp::Reverse(a.delta_cents.abs()));

    let compared_months = history
        .iter()
        .map(|h| h.ord)
        .collect::<std::collections::BTreeSet<_>>()
        .len() as i64;

    let (_, month) = ord_to_year_month(current_ord);
    let out = AnomalyReport {
        year: q.year,
        month,
        month_name: month_name_de(q.month).to_string(),
        items,
        compared_months,
        min_ratio: ANOMALY_MIN_RATIO,
        min_delta_cents: ANOMALY_MIN_DELTA_CENTS,
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_ignores_a_single_huge_month() {
        // Five ordinary months and one Nebenkosten settlement. The mean of these is
        // 32.998 cents; the median is what the month actually tends to cost.
        let history = [8_000, 8_500, 7_900, 144_000, 8_200, 8_100];
        assert_eq!(median(&history), 8_150);

        let mean = history.iter().sum::<i64>() / history.len() as i64;
        assert!(
            mean > 30_000,
            "the mean really is dragged: {mean} — that is why this is a median"
        );
    }

    #[test]
    fn an_even_count_averages_the_two_middles() {
        assert_eq!(median(&[100, 200, 300, 400]), 250);
        // Half-away-from-zero, like every other rounding boundary in the app.
        assert_eq!(median(&[100, 101]), 101);
        assert_eq!(median(&[-101, -100]), -101);
    }

    #[test]
    fn an_empty_history_has_no_median_and_no_spread() {
        assert_eq!(median(&[]), 0);
        assert_eq!(median_absolute_deviation(&[], 0), 0);
    }

    #[test]
    fn the_spread_is_not_dragged_by_the_outlier_either() {
        let history = [8_000, 8_500, 7_900, 144_000, 8_200, 8_100];
        let m = median(&history);
        // Deviations are 150, 350, 250, 139.750, 50, 50 — the middle of which is
        // 200, not the 23.000 a standard deviation would report.
        assert_eq!(median_absolute_deviation(&history, m), 200);
    }

    #[test]
    fn the_larger_of_template_and_history_wins_and_neither_is_added_to_the_other() {
        // Internet: a 64,94 standing order plus occasional top-ups, so history is
        // the better figure — and 64,94 + 80,00 would be a category that does not
        // exist.
        assert_eq!(projected_for(6_494, Some(8_000)), (8_000, "median"));
        // Rent: the template is the whole story even if a quiet month says less.
        assert_eq!(projected_for(120_000, Some(60_000)), (120_000, "template"));
        // No template at all.
        assert_eq!(projected_for(0, Some(4_200)), (4_200, "median"));
        // No history at all.
        assert_eq!(projected_for(16_000, None), (16_000, "template"));
    }

    #[test]
    fn income_keeps_its_sign_through_the_choice() {
        // Stored convention: income is negative. The larger MAGNITUDE wins, so a
        // salary rise in the history is not overruled by a stale template.
        assert_eq!(
            projected_for(-300_000, Some(-320_000)),
            (-320_000, "median")
        );
        assert_eq!(
            projected_for(-300_000, Some(-100_000)),
            (-300_000, "template")
        );
    }
}
