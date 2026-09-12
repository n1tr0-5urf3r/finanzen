//! Recurring booking templates and their materialisation.
//!
//! The monthly fixed-cost ritual is ~18 entries the user types by hand every month.
//! This module turns it into one request. The two things that must be true, and are
//! asserted in `tests/api.rs`:
//!
//! 1. **Running it twice creates nothing the second time.** Idempotence is the
//!    partial unique index `bookings_template_period_key`, not a handler-side check —
//!    a check would race and would silently break the moment a retry arrives.
//! 2. **An estimate materialises as a draft.** `v_ledger` filters `status =
//!    'confirmed'`, so a draft cannot reach any total until the user confirms it with
//!    the real amount.

use axum::{
    Json,
    extract::{Path, Query},
    http::StatusCode,
};
use chrono::NaiveDate;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::Ctx,
    bookings::{assert_year_unlocked, resolve_category},
    error::{AppError, Result},
    locale::{month_name_de, period_ord},
    models::{
        BookingKind, MaterializeRequest, MaterializeResult, MaterializedItem, Period,
        RecurringTemplate, RecurringTemplateInput,
    },
};

const SELECT_TEMPLATES: &str = "\
    SELECT r.id, r.name, r.comment, r.kind, r.amount_cents, r.amount_is_estimate, \
           r.category_id, c.name AS category_name, t.label AS category_type, \
           r.tax_relevant, r.day_of_month, r.interval_months, r.anchor_ord, \
           r.active_from_ord, r.active_to_ord, r.active, r.sort_order, \
           (SELECT count(*) FROM bookings b WHERE b.template_id = r.id)::bigint AS booking_count, \
           (SELECT max(b.period_ord) FROM bookings b WHERE b.template_id = r.id) AS last_ord, \
           EXISTS (SELECT 1 FROM bookings b \
                    WHERE b.template_id = r.id AND b.period_ord = $1::int) AS booked_in_period \
      FROM recurring_templates r \
      LEFT JOIN categories c ON c.id = r.category_id \
      LEFT JOIN category_types t ON t.id = c.type_id";

/// The due rule, in one place. A template is due in period `p` iff it is active, `p`
/// lies inside its window, and `p` sits on the interval grid measured from the
/// anchor. Written as SQL so the listing, the materialiser and the tests cannot
/// diverge; `is_due` below is the same predicate for Rust callers.
const DUE_SQL: &str = "\
    r.active AND $1::int >= r.active_from_ord \
    AND (r.active_to_ord IS NULL OR $1::int <= r.active_to_ord) \
    AND (($1::int - r.anchor_ord) % r.interval_months) = 0";

/// Note `rem_euclid`, not `%`: a period before the anchor gives a negative
/// difference, and Rust's `%` keeps the sign, so `-3 % 3 == 0` is right but
/// `-4 % 3 == -1` would compare unequal to `0` by accident in a future rewrite.
/// Postgres' `%` has the same sign behaviour, and both are only ever compared to
/// zero, where the two agree.
fn is_due(period: i32, anchor: i32, interval: i32, from: i32, to: Option<i32>) -> bool {
    period >= from
        && to.is_none_or(|t| period <= t)
        && (period - anchor).rem_euclid(interval.max(1)) == 0
}

fn row_to_template(r: &sqlx::postgres::PgRow, period: Option<i32>) -> RecurringTemplate {
    let anchor_ord: i32 = r.get("anchor_ord");
    let from_ord: i32 = r.get("active_from_ord");
    let to_ord: Option<i32> = r.get("active_to_ord");
    let interval: i16 = r.get("interval_months");
    let active: bool = r.get("active");

    RecurringTemplate {
        id: r.get("id"),
        name: r.get("name"),
        comment: r.get("comment"),
        kind: BookingKind::parse(r.get::<String, _>("kind").as_str())
            .unwrap_or(BookingKind::Expense),
        amount_cents: r.get("amount_cents"),
        amount_is_estimate: r.get("amount_is_estimate"),
        category_id: r.get("category_id"),
        category_name: r.get("category_name"),
        category_type: r.get("category_type"),
        tax_relevant: r.get("tax_relevant"),
        day_of_month: r.get::<Option<i16>, _>("day_of_month").map(|d| d as u8),
        interval_months: interval as u8,
        anchor: Period::from_ord(anchor_ord),
        active_from: Period::from_ord(from_ord),
        active_to: to_ord.map(Period::from_ord),
        active,
        sort_order: r.get("sort_order"),
        due_in_period: period
            .map(|p| active && is_due(p, anchor_ord, interval as i32, from_ord, to_ord)),
        booked_in_period: period.map(|_| r.get::<bool, _>("booked_in_period")),
        last_booked: r.get::<Option<i32>, _>("last_ord").map(Period::from_ord),
        booking_count: r.get("booking_count"),
    }
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateQuery {
    /// Naming a period fills in `dueInPeriod` / `bookedInPeriod`, which is what the
    /// month checklist renders from. Both months must be given or neither.
    pub year: Option<i32>,
    pub month: Option<u8>,
    /// Restricts the listing to what is due in that period.
    pub due_only: Option<bool>,
}

fn requested_period(q: &TemplateQuery) -> Result<Option<i32>> {
    match (q.year, q.month) {
        (Some(y), Some(m)) if (1..=12).contains(&m) => Ok(Some(period_ord(y, m))),
        (None, None) => Ok(None),
        _ => Err(AppError::Validation(
            "Jahr und Monat müssen zusammen angegeben werden".into(),
        )),
    }
}

pub async fn list(
    mut ctx: Ctx,
    Query(q): Query<TemplateQuery>,
) -> Result<Json<Vec<RecurringTemplate>>> {
    let period = requested_period(&q)?;
    // A period of i32::MIN can never be due, so the un-parameterised listing still
    // binds $1 and the SQL stays one string.
    let bind = period.unwrap_or(i32::MIN);
    let sql = if q.due_only == Some(true) && period.is_some() {
        format!("{SELECT_TEMPLATES} WHERE {DUE_SQL} ORDER BY r.sort_order, r.name")
    } else {
        format!("{SELECT_TEMPLATES} ORDER BY r.sort_order, r.name")
    };
    let rows = sqlx::query(&sql)
        .bind(bind)
        .fetch_all(ctx.tenant.conn())
        .await?;
    let out = rows.iter().map(|r| row_to_template(r, period)).collect();
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

fn validate(body: &RecurringTemplateInput) -> Result<Period> {
    if body.name.trim().is_empty() {
        return Err(AppError::Validation("Name fehlt".into()));
    }
    if body.comment.trim().is_empty() {
        return Err(AppError::Validation("Kommentar fehlt".into()));
    }
    if body.amount_cents <= 0 {
        return Err(AppError::Validation("Der Betrag muss positiv sein".into()));
    }
    if !(1..=12).contains(&body.interval_months) {
        return Err(AppError::Validation(
            "Das Intervall muss zwischen 1 und 12 Monaten liegen".into(),
        ));
    }
    if let Some(day) = body.day_of_month
        && !(1..=31).contains(&day)
    {
        return Err(AppError::Validation(
            "Der Tag muss zwischen 1 und 31 liegen".into(),
        ));
    }
    for p in [Some(body.active_from), body.anchor, body.active_to]
        .into_iter()
        .flatten()
    {
        if !(1..=12).contains(&p.month) {
            return Err(AppError::Validation(
                "Monat muss zwischen 1 und 12 liegen".into(),
            ));
        }
    }
    if let Some(to) = body.active_to
        && to.ord() < body.active_from.ord()
    {
        return Err(AppError::Validation(
            "Das Ende darf nicht vor dem Beginn liegen".into(),
        ));
    }
    // An omitted anchor means "the cycle starts when the template does", which is
    // what makes a plain monthly template two fields instead of four.
    Ok(body.anchor.unwrap_or(body.active_from))
}

pub async fn create(
    mut ctx: Ctx,
    Json(body): Json<RecurringTemplateInput>,
) -> Result<(StatusCode, Json<RecurringTemplate>)> {
    let anchor = validate(&body)?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO recurring_templates (id, user_id, name, comment, kind, amount_cents, \
                amount_is_estimate, category_id, tax_relevant, day_of_month, interval_months, \
                anchor_ord, active_from_ord, active_to_ord, active, sort_order) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10::smallint,$11::smallint,$12,$13,$14,$15,\
                 $16::smallint)",
    )
    .bind(id)
    .bind(ctx.tenant.user_id())
    .bind(body.name.trim())
    .bind(body.comment.trim())
    .bind(body.kind.as_db())
    .bind(body.amount_cents)
    .bind(body.amount_is_estimate)
    .bind(body.category_id)
    .bind(body.tax_relevant)
    .bind(body.day_of_month.map(|d| d as i16))
    .bind(body.interval_months as i16)
    .bind(anchor.ord())
    .bind(body.active_from.ord())
    .bind(body.active_to.map(|p| p.ord()))
    .bind(body.active)
    .bind(body.sort_order)
    .execute(ctx.tenant.conn())
    .await
    .map_err(|e| AppError::from_db(e, "Vorlage konnte nicht gespeichert werden"))?;

    let out = fetch_one(&mut ctx, id).await?;
    ctx.tenant.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}

pub async fn update(
    mut ctx: Ctx,
    Path(id): Path<Uuid>,
    Json(body): Json<RecurringTemplateInput>,
) -> Result<Json<RecurringTemplate>> {
    let anchor = validate(&body)?;
    let affected = sqlx::query(
        "UPDATE recurring_templates SET name = $2, comment = $3, kind = $4, amount_cents = $5, \
                amount_is_estimate = $6, category_id = $7, tax_relevant = $8, \
                day_of_month = $9::smallint, interval_months = $10::smallint, anchor_ord = $11, \
                active_from_ord = $12, active_to_ord = $13, active = $14, sort_order = $15::smallint \
          WHERE id = $1",
    )
    .bind(id)
    .bind(body.name.trim())
    .bind(body.comment.trim())
    .bind(body.kind.as_db())
    .bind(body.amount_cents)
    .bind(body.amount_is_estimate)
    .bind(body.category_id)
    .bind(body.tax_relevant)
    .bind(body.day_of_month.map(|d| d as i16))
    .bind(body.interval_months as i16)
    .bind(anchor.ord())
    .bind(body.active_from.ord())
    .bind(body.active_to.map(|p| p.ord()))
    .bind(body.active)
    .bind(body.sort_order)
    .execute(ctx.tenant.conn())
    .await
    .map_err(|e| AppError::from_db(e, "Vorlage konnte nicht gespeichert werden"))?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound("Vorlage".into()));
    }

    let out = fetch_one(&mut ctx, id).await?;
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

/// Deleting a template leaves the bookings it already produced alone —
/// `bookings.template_id` is `ON DELETE SET NULL`. Money that has been spent does not
/// disappear because the plan for it was cancelled.
pub async fn delete(mut ctx: Ctx, Path(id): Path<Uuid>) -> Result<StatusCode> {
    let affected = sqlx::query("DELETE FROM recurring_templates WHERE id = $1")
        .bind(id)
        .execute(ctx.tenant.conn())
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound("Vorlage".into()));
    }
    ctx.tenant.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn fetch_one(ctx: &mut Ctx, id: Uuid) -> Result<RecurringTemplate> {
    let row = sqlx::query(&format!("{SELECT_TEMPLATES} WHERE r.id = $2"))
        .bind(i32::MIN)
        .bind(id)
        .fetch_optional(ctx.tenant.conn())
        .await?
        .ok_or_else(|| AppError::NotFound("Vorlage".into()))?;
    Ok(row_to_template(&row, None))
}

/// Clamps a day to the length of its month. February with `day_of_month = 31` must
/// land on the 28th (or 29th), not raise — `bookings_date_in_period` would otherwise
/// reject the row and the whole run would fail on one badly-configured template.
fn booking_date(year: i32, month: u8, day: Option<u8>) -> Option<NaiveDate> {
    let wanted = day.unwrap_or(1).clamp(1, 31) as u32;
    (0..4).find_map(|back| NaiveDate::from_ymd_opt(year, month as u32, wanted - back))
}

/// Creates the bookings due in one month.
///
/// The guard against double-booking is `ON CONFLICT ... DO NOTHING` on the partial
/// unique index. **The index predicate has to be repeated in the conflict target** —
/// without `WHERE template_id IS NOT NULL` Postgres cannot infer a partial index and
/// raises 42P10, which is exactly the trap the importer hit.
pub async fn materialize(
    mut ctx: Ctx,
    Json(body): Json<MaterializeRequest>,
) -> Result<Json<MaterializeResult>> {
    if !(1..=12).contains(&body.month) {
        return Err(AppError::Validation(
            "Monat muss zwischen 1 und 12 liegen".into(),
        ));
    }
    assert_year_unlocked(ctx.tenant.conn(), body.year).await?;
    let period = period_ord(body.year, body.month);

    let mut sql = format!("{SELECT_TEMPLATES} WHERE {DUE_SQL}");
    if body.template_ids.is_some() {
        sql.push_str(" AND r.id = ANY($2)");
    }
    sql.push_str(" ORDER BY r.sort_order, r.name");

    let mut query = sqlx::query(&sql).bind(period);
    if let Some(ids) = &body.template_ids {
        query = query.bind(ids);
    }
    let rows = query.fetch_all(ctx.tenant.conn()).await?;
    let templates: Vec<RecurringTemplate> = rows
        .iter()
        .map(|r| row_to_template(r, Some(period)))
        .collect();

    let booked_on = booking_date(body.year, body.month, None);
    let mut items = Vec::with_capacity(templates.len());
    let (mut created, mut skipped, mut drafts) = (0i64, 0i64, 0i64);

    for tmpl in &templates {
        // An estimate is never a confirmed figure. The gym is 29,00 / 31,50 / 34,50
        // depending on the month, so booking the template's amount as fact would put
        // a wrong number into every total until somebody noticed.
        let status = if tmpl.amount_is_estimate {
            "draft"
        } else {
            "confirmed"
        };
        let date = booking_date(body.year, body.month, tmpl.day_of_month).or(booked_on);

        if body.dry_run || tmpl.booked_in_period == Some(true) {
            let already = tmpl.booked_in_period == Some(true);
            if already {
                skipped += 1;
            } else {
                created += 1;
                if status == "draft" {
                    drafts += 1;
                }
            }
            items.push(MaterializedItem {
                template_id: tmpl.id,
                template_name: tmpl.name.clone(),
                comment: tmpl.comment.clone(),
                amount_cents: tmpl.amount_cents,
                kind: tmpl.kind,
                status: status.to_string(),
                booking_id: None,
                skipped_reason: already.then(|| "alreadyBooked".to_string()),
            });
            continue;
        }

        // Same precedence as a hand-entered booking: an explicit category on the
        // template is a manual override, otherwise the rule table decides — at
        // materialisation time, so a rule fixed today reaches next month's rent.
        //
        // The rule's `kind_override` is deliberately NOT applied here: unlike a
        // free-text comment, a template carries a kind the user chose explicitly.
        let resolved = resolve_category(ctx.tenant.conn(), &tmpl.comment, tmpl.category_id).await?;

        let booking_id = Uuid::new_v4();
        let inserted = sqlx::query(
            "INSERT INTO bookings (id, user_id, period_year, period_month, booked_on, kind, \
                    amount_cents, comment, tax_relevant, category_id, category_source, \
                    resolved_rule_id, status, origin, template_id) \
             VALUES ($1,$2,$3::smallint,$4::smallint,$5,$6,$7,$8,$9,$10,$11,$12,$13,\
                     'recurring',$14) \
             ON CONFLICT (user_id, template_id, period_ord) WHERE template_id IS NOT NULL \
             DO NOTHING",
        )
        .bind(booking_id)
        .bind(ctx.tenant.user_id())
        .bind(body.year)
        .bind(body.month as i16)
        .bind(date)
        .bind(tmpl.kind.as_db())
        .bind(tmpl.amount_cents)
        .bind(tmpl.comment.trim())
        .bind(tmpl.tax_relevant)
        .bind(resolved.category_id)
        .bind(resolved.source)
        .bind(resolved.rule_id)
        .bind(status)
        .bind(tmpl.id)
        .execute(ctx.tenant.conn())
        .await
        .map_err(|e| AppError::from_db(e, "Buchung konnte nicht angelegt werden"))?
        .rows_affected();

        if inserted == 0 {
            skipped += 1;
            items.push(MaterializedItem {
                template_id: tmpl.id,
                template_name: tmpl.name.clone(),
                comment: tmpl.comment.clone(),
                amount_cents: tmpl.amount_cents,
                kind: tmpl.kind,
                status: status.to_string(),
                booking_id: None,
                skipped_reason: Some("alreadyBooked".into()),
            });
        } else {
            created += 1;
            if status == "draft" {
                drafts += 1;
            }
            items.push(MaterializedItem {
                template_id: tmpl.id,
                template_name: tmpl.name.clone(),
                comment: tmpl.comment.clone(),
                amount_cents: tmpl.amount_cents,
                kind: tmpl.kind,
                status: status.to_string(),
                booking_id: Some(booking_id),
                skipped_reason: None,
            });
        }
    }

    let out = MaterializeResult {
        year: body.year,
        month: body.month,
        month_name: month_name_de(body.month).to_string(),
        created,
        skipped,
        drafts,
        dry_run: body.dry_run,
        items,
    };
    // A dry run still has to leave the transaction clean; it wrote nothing, so the
    // commit is a no-op, but rolling back here would also discard nothing and the
    // symmetry keeps the handler shape identical to every other one.
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_monthly_template_is_due_every_month_inside_its_window() {
        let anchor = period_ord(2026, 1);
        for m in 1..=12u8 {
            assert!(is_due(period_ord(2026, m), anchor, 1, anchor, None));
        }
        // Before it starts, and after it ends.
        assert!(!is_due(period_ord(2025, 12), anchor, 1, anchor, None));
        let to = period_ord(2026, 6);
        assert!(!is_due(period_ord(2026, 7), anchor, 1, anchor, Some(to)));
        assert!(is_due(period_ord(2026, 6), anchor, 1, anchor, Some(to)));
    }

    #[test]
    fn a_quarterly_template_is_due_only_in_its_own_quarter() {
        // Versicherung, anchored in February: Feb, Mai, Aug, Nov — and nothing else.
        let anchor = period_ord(2026, 2);
        let due: Vec<u8> = (1..=12u8)
            .filter(|m| is_due(period_ord(2026, *m), anchor, 3, anchor, None))
            .collect();
        assert_eq!(due, vec![2, 5, 8, 11]);
    }

    #[test]
    fn an_annual_template_is_due_once_a_year_across_the_year_boundary() {
        let anchor = period_ord(2025, 11);
        let due: Vec<(i32, u8)> = (0..24)
            .map(|i| crate::locale::ord_to_year_month(anchor + i))
            .filter(|(y, m)| is_due(period_ord(*y, *m), anchor, 12, anchor, None))
            .collect();
        assert_eq!(due, vec![(2025, 11), (2026, 11)]);
    }

    #[test]
    fn the_due_grid_is_measured_from_the_anchor_not_from_january() {
        // Anchored in March, quarterly: March, June, September, December.
        let anchor = period_ord(2026, 3);
        assert!(is_due(period_ord(2026, 6), anchor, 3, anchor, None));
        assert!(!is_due(period_ord(2026, 5), anchor, 3, anchor, None));
    }

    #[test]
    fn a_day_beyond_the_end_of_the_month_is_clamped_not_rejected() {
        assert_eq!(
            booking_date(2026, 2, Some(31)),
            NaiveDate::from_ymd_opt(2026, 2, 28)
        );
        // 2024 is a leap year, so the same template lands on the 29th there.
        assert_eq!(
            booking_date(2024, 2, Some(30)),
            NaiveDate::from_ymd_opt(2024, 2, 29)
        );
        assert_eq!(
            booking_date(2026, 4, Some(31)),
            NaiveDate::from_ymd_opt(2026, 4, 30)
        );
        // The default is the first, never "today".
        assert_eq!(
            booking_date(2026, 7, None),
            NaiveDate::from_ymd_opt(2026, 7, 1)
        );
    }
}
