//! Booking CRUD, filtering and bulk edits.

use axum::{
    Json,
    extract::{Path, Query},
    http::StatusCode,
};
use chrono::{Datelike, NaiveDate, Utc};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::{
    auth::Ctx,
    error::{AppError, Result},
    locale::month_name_de,
    models::{
        Booking, BookingInput, BookingKind, BookingPage, CategorySource, ConfirmBookingInput,
        SearchComment, SearchResult, SearchYearSummary,
    },
};

pub(crate) const SELECT_BOOKING: &str = "\
    SELECT b.id, b.period_year, b.period_month, b.booked_on, b.kind, b.amount_cents, \
           b.net_cents, b.comment, b.tax_relevant, b.category_id, c.name AS category_name, \
           t.label AS category_type, b.category_source, b.shared, b.external_source, \
           b.external_id, b.status, b.origin, \
           EXISTS (SELECT 1 FROM receipts r WHERE r.booking_id = b.id) AS has_receipt \
      FROM bookings b \
      LEFT JOIN categories c ON c.id = b.category_id \
      LEFT JOIN category_types t ON t.id = c.type_id";

pub(crate) fn row_to_booking(r: &sqlx::postgres::PgRow) -> Booking {
    let month: i16 = r.get("period_month");
    Booking {
        id: r.get("id"),
        year: r.get::<i16, _>("period_year") as i32,
        month: month as u8,
        month_name: month_name_de(month as u8).to_string(),
        booked_on: r.get("booked_on"),
        kind: BookingKind::parse(r.get::<String, _>("kind").as_str())
            .unwrap_or(BookingKind::Expense),
        amount_cents: r.get("amount_cents"),
        net_cents: r.get("net_cents"),
        comment: r.get("comment"),
        tax_relevant: r.get("tax_relevant"),
        category_id: r.get("category_id"),
        category_name: r.get("category_name"),
        category_type: r.get("category_type"),
        category_source: CategorySource::parse(r.get::<String, _>("category_source").as_str()),
        shared: r.get("shared"),
        external_source: r.get("external_source"),
        external_id: r.get("external_id"),
        has_receipt: r.get("has_receipt"),
        status: r.get("status"),
        origin: r.get("origin"),
    }
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookingQuery {
    /// Optional, and omitting it is how a query spans every year the ledger holds.
    /// The screens all send one because a ledger is read a year at a time; search
    /// is the case that does not.
    pub year: Option<i32>,
    pub month: Option<u8>,
    pub category_id: Option<Uuid>,
    pub category_type: Option<String>,
    pub kind: Option<String>,
    pub tax_relevant: Option<bool>,
    /// Lists only bookings no rule matched. Never a default — uncategorised rows are
    /// visible in the normal listing too.
    pub uncategorized: Option<bool>,
    pub search: Option<String>,
    /// `confirmed` (the default), `draft`, or `all`. Drafts are excluded everywhere
    /// by default — that is the point of the status — but the recurring screen has to
    /// be able to list the ones waiting for their real amount.
    pub status: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    /// `asc` (oldest first) or `desc` (newest first, the default for a ledger
    /// someone is reading rather than auditing). The tiebreakers reverse with it,
    /// so the order is a total reversal and not merely a flipped first column.
    pub direction: Option<String>,
}

/// Builds the shared WHERE clause. Note there is no `user_id` predicate anywhere:
/// row-level security supplies it, which is why forgetting one here cannot leak.
fn filter_sql(q: &BookingQuery) -> (String, Vec<String>) {
    let mut clauses = match q.status.as_deref() {
        // `true` rather than an empty clause list: the callers join with " AND " and
        // build `WHERE {…}`, so an empty string would be a syntax error.
        Some("all") => vec!["true".to_string()],
        Some("draft") => vec!["b.status = 'draft'".to_string()],
        _ => vec!["b.status = 'confirmed'".to_string()],
    };
    let mut binds = Vec::new();
    let mut n = 0;
    let mut next = |binds: &mut Vec<String>, value: String| {
        binds.push(value);
        n += 1;
        format!("${n}")
    };
    if let Some(v) = q.year {
        let p = next(&mut binds, v.to_string());
        clauses.push(format!("b.period_year = {p}::smallint"));
    }
    if let Some(v) = q.month {
        let p = next(&mut binds, v.to_string());
        clauses.push(format!("b.period_month = {p}::smallint"));
    }
    if let Some(v) = q.category_id {
        let p = next(&mut binds, v.to_string());
        clauses.push(format!("b.category_id = {p}::uuid"));
    }
    if let Some(v) = &q.category_type {
        let p = next(&mut binds, v.clone());
        clauses.push(format!("t.code = {p}"));
    }
    if let Some(v) = &q.kind {
        let p = next(&mut binds, v.clone());
        clauses.push(format!("b.kind = {p}"));
    }
    if let Some(v) = q.tax_relevant {
        let p = next(&mut binds, v.to_string());
        clauses.push(format!("b.tax_relevant = {p}::boolean"));
    }
    if q.uncategorized == Some(true) {
        clauses.push("b.category_id IS NULL".into());
    }
    if let Some(v) = &q.search
        && !v.trim().is_empty()
    {
        let p = next(&mut binds, format!("%{}%", v.trim()));
        clauses.push(format!("b.comment ILIKE {p}"));
    }
    (clauses.join(" AND "), binds)
}

/// The full ordering, in one place because the list query and the count query have
/// to agree or paging silently shuffles rows between pages.
fn order_clause(direction: Option<&str>) -> &'static str {
    match direction {
        Some("asc") => {
            "b.period_ord, b.booked_on NULLS LAST, b.created_at, b.comment, b.amount_cents, b.id"
        }
        // Newest first by default: a ledger is read from the end, and the user's
        // most recent bookings are the ones they are most likely to be fixing.
        _ => {
            "b.period_ord DESC, b.booked_on DESC NULLS LAST, b.created_at DESC, \
             b.comment DESC, b.amount_cents DESC, b.id DESC"
        }
    }
}

/// Confirms a draft booking, optionally correcting its amount.
///
/// A separate endpoint rather than a `status` field on `PUT /bookings/{id}`, for two
/// reasons. The PUT body is a full `BookingInput`, so a status field there would
/// travel on every ordinary edit and any client that forgot to echo it back would
/// silently re-draft — or silently confirm — a booking; and draft→confirmed is the
/// one transition that moves money into every total, so it deserves a request that
/// cannot be made by accident.
///
/// Correcting the amount belongs here because it is the entire reason the draft
/// exists: an `amountIsEstimate` template books 29,00 and the real invoice says
/// 34,50. Confirming twice is harmless — the second call finds it already confirmed
/// and changes nothing else.
#[utoipa::path(
    get,
    path = "/api/v1/bookings",
    tag = "bookings",
    params(("year" = Option<i32>, Query, description = "Kalenderjahr"), ("month" = Option<u8>, Query, description = "Monat 1..12"), ("categoryId" = Option<Uuid>, Query, description = "Kategorie"), ("categoryType" = Option<String>, Query, description = "Typ-Code"), ("kind" = Option<String>, Query, description = "income | expense | transfer"), ("taxRelevant" = Option<bool>, Query, description = "Nur steuerrelevante"), ("uncategorized" = Option<bool>, Query, description = "Nur ohne Kategorie"), ("search" = Option<String>, Query, description = "Kommentar enthält"), ("status" = Option<String>, Query, description = "confirmed (Standard) | draft | all"), ("page" = Option<u32>, Query, description = "Seite, ab 0"), ("pageSize" = Option<u32>, Query, description = "1..500, Standard 100"), ("direction" = Option<String>, Query, description = "desc (neueste zuerst, Standard) | asc")),
    responses((status = 200, description = "Gefilterte Buchungen samt Summen der aktuellen Filterung", body = BookingPage)),
)]
pub async fn list(mut ctx: Ctx, Query(q): Query<BookingQuery>) -> Result<Json<BookingPage>> {
    let page = q.page.unwrap_or(0);
    let page_size = q.page_size.unwrap_or(100).clamp(1, 500);
    let (where_sql, binds) = filter_sql(&q);

    let order = order_clause(q.direction.as_deref());
    let list_sql = format!(
        "{SELECT_BOOKING} WHERE {where_sql} \
         ORDER BY {order} \
         LIMIT {page_size} OFFSET {}",
        page as i64 * page_size as i64
    );
    let mut query = sqlx::query(&list_sql);
    for b in &binds {
        query = query.bind(b);
    }
    let rows = query.fetch_all(ctx.tenant.conn()).await?;

    let totals_sql = format!(
        "SELECT count(*)::bigint AS total, \
                COALESCE(SUM(b.amount_cents) FILTER (WHERE b.kind = 'income'), 0)::bigint AS inc, \
                COALESCE(SUM(b.amount_cents) FILTER (WHERE b.kind = 'expense'), 0)::bigint AS exp, \
                COALESCE(SUM(b.net_cents), 0)::bigint AS net, \
                count(*) FILTER (WHERE b.category_id IS NULL AND b.kind <> 'transfer')::bigint AS uncat \
           FROM bookings b \
           LEFT JOIN categories c ON c.id = b.category_id \
           LEFT JOIN category_types t ON t.id = c.type_id \
          WHERE {where_sql}"
    );
    let mut tq = sqlx::query(&totals_sql);
    for b in &binds {
        tq = tq.bind(b);
    }
    let totals = tq.fetch_one(ctx.tenant.conn()).await?;

    let out = BookingPage {
        items: rows.iter().map(row_to_booking).collect(),
        total: totals.get("total"),
        page,
        page_size,
        sum_income_cents: totals.get("inc"),
        sum_expense_cents: totals.get("exp"),
        sum_net_cents: totals.get("net"),
        uncategorized_count: totals.get("uncat"),
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[utoipa::path(
    get,
    path = "/api/v1/bookings/{id}",
    tag = "bookings",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    responses((status = 200, description = "Eine Buchung", body = Booking), (status = 404, description = "Nicht gefunden", body = crate::error::ErrorBody)),
)]
pub async fn get_one(mut ctx: Ctx, Path(id): Path<Uuid>) -> Result<Json<Booking>> {
    let row = sqlx::query(&format!("{SELECT_BOOKING} WHERE b.id = $1"))
        .bind(id)
        .fetch_optional(ctx.tenant.conn())
        .await?
        .ok_or_else(|| AppError::NotFound("Buchung".into()))?;
    let booking = row_to_booking(&row);
    ctx.tenant.commit().await?;
    Ok(Json(booking))
}

fn validate(body: &BookingInput) -> Result<()> {
    if body.amount_cents <= 0 {
        return Err(AppError::Validation("Der Betrag muss positiv sein".into()));
    }
    if body.comment.trim().is_empty() {
        return Err(AppError::Validation("Kommentar fehlt".into()));
    }
    if !(1..=12).contains(&body.month) {
        return Err(AppError::Validation(
            "Monat muss zwischen 1 und 12 liegen".into(),
        ));
    }
    Ok(())
}

pub(crate) async fn assert_year_unlocked(conn: &mut PgConnection, year: i32) -> Result<()> {
    let locked: Option<bool> = sqlx::query_scalar(
        "SELECT tax_locked_at IS NOT NULL FROM fiscal_years WHERE year = $1::smallint",
    )
    .bind(year)
    .fetch_optional(&mut *conn)
    .await?;
    if locked == Some(true) {
        return Err(AppError::Conflict(format!(
            "Das Jahr {year} ist für die Steuer gesperrt"
        )));
    }
    Ok(())
}

/// Resolves the category for a new or edited booking: an explicit `categoryId` is a
/// manual override and wins; otherwise the rule table decides; otherwise unresolved.
/// The outcome of resolving a comment against the rule table.
pub(crate) struct Resolution {
    pub(crate) category_id: Option<Uuid>,
    pub(crate) source: &'static str,
    pub(crate) rule_id: Option<Uuid>,
    pub(crate) kind_override: Option<BookingKind>,
}

pub(crate) async fn resolve_category(
    conn: &mut PgConnection,
    comment: &str,
    explicit: Option<Uuid>,
) -> Result<Resolution> {
    if let Some(id) = explicit {
        return Ok(Resolution {
            category_id: Some(id),
            source: "manual",
            rule_id: None,
            kind_override: None,
        });
    }
    let row = sqlx::query(
        "SELECT id, category_id, kind_override FROM category_rules \
          WHERE match_key = lower(btrim($1))",
    )
    .bind(comment)
    .fetch_optional(&mut *conn)
    .await?;
    match row {
        Some(r) => {
            let category_id: Option<Uuid> = r.get("category_id");
            let kind = r
                .get::<Option<String>, _>("kind_override")
                .and_then(|k| BookingKind::parse(&k));
            if category_id.is_some() {
                Ok(Resolution {
                    category_id,
                    source: "rule",
                    rule_id: Some(r.get("id")),
                    kind_override: kind,
                })
            } else {
                Ok(Resolution {
                    category_id: None,
                    source: "unresolved",
                    rule_id: None,
                    kind_override: kind,
                })
            }
        }
        None => Ok(Resolution {
            category_id: None,
            source: "unresolved",
            rule_id: None,
            kind_override: None,
        }),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/bookings",
    tag = "bookings",
    request_body = BookingInput,
    responses((status = 201, description = "Buchung angelegt", body = Booking), (status = 400, description = "Betrag, Kommentar oder Monat ungültig", body = crate::error::ErrorBody), (status = 409, description = "Jahr ist für die Steuer gesperrt", body = crate::error::ErrorBody)),
)]
pub async fn create(
    mut ctx: Ctx,
    Json(body): Json<BookingInput>,
) -> Result<(StatusCode, Json<Booking>)> {
    validate(&body)?;
    assert_year_unlocked(ctx.tenant.conn(), body.year).await?;

    let resolved = resolve_category(ctx.tenant.conn(), &body.comment, body.category_id).await?;
    // A rule may force the booking kind — that is how `to ING` and `from Volksbank`
    // become transfers without a hard-coded list in the binary.
    let kind = resolved.kind_override.unwrap_or(body.kind);

    // Bookings created through the API always carry a day; the schema enforces it
    // for every origin except imported history.
    //
    // Today's date when the booking is in today's month, not the 1st. A booking
    // added on the 14th is a thing that happened on the 14th, and the day matters
    // beyond tidiness: it is what a KitchenOwl push files the expense under, and
    // "the 1st" put a push two weeks up the list where nobody looked for it.
    // Another month has no defensible day, so it keeps the 1st.
    let booked_on = body.booked_on.or_else(|| {
        let today = Utc::now().date_naive();
        if today.year() == body.year && today.month() == body.month as u32 {
            Some(today)
        } else {
            NaiveDate::from_ymd_opt(body.year, body.month as u32, 1)
        }
    });

    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO bookings (id, user_id, period_year, period_month, booked_on, kind, \
                               amount_cents, comment, tax_relevant, category_id, \
                               category_source, resolved_rule_id, origin) \
         VALUES ($1,$2,$3::smallint,$4::smallint,$5,$6,$7,$8,$9,$10,$11,$12,'manual')",
    )
    .bind(id)
    .bind(ctx.tenant.user_id())
    .bind(body.year)
    .bind(body.month as i16)
    .bind(booked_on)
    .bind(kind.as_db())
    .bind(body.amount_cents)
    .bind(body.comment.trim())
    .bind(body.tax_relevant)
    .bind(resolved.category_id)
    .bind(resolved.source)
    .bind(resolved.rule_id)
    .execute(ctx.tenant.conn())
    .await
    .map_err(|e| AppError::from_db(e, "Buchung konnte nicht gespeichert werden"))?;

    let row = sqlx::query(&format!("{SELECT_BOOKING} WHERE b.id = $1"))
        .bind(id)
        .fetch_one(ctx.tenant.conn())
        .await?;
    let booking = row_to_booking(&row);
    ctx.tenant.commit().await?;
    Ok((StatusCode::CREATED, Json(booking)))
}

#[utoipa::path(
    put,
    path = "/api/v1/bookings/{id}",
    tag = "bookings",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    request_body = BookingInput,
    responses((status = 200, description = "Buchung gespeichert", body = Booking), (status = 404, description = "Nicht gefunden", body = crate::error::ErrorBody)),
)]
pub async fn update(
    mut ctx: Ctx,
    Path(id): Path<Uuid>,
    Json(body): Json<BookingInput>,
) -> Result<Json<Booking>> {
    validate(&body)?;
    assert_year_unlocked(ctx.tenant.conn(), body.year).await?;

    let resolved = if body.clear_category_override {
        resolve_category(ctx.tenant.conn(), &body.comment, None).await?
    } else {
        resolve_category(ctx.tenant.conn(), &body.comment, body.category_id).await?
    };

    // booked_on is preserved when the client omits it, and re-clamped when the
    // booking moves to a different month — otherwise the day would fall outside its
    // own period. Imported history legitimately keeps no day at all.
    let affected = sqlx::query(
        "UPDATE bookings SET period_year = $2::smallint, period_month = $3::smallint, \
                booked_on = CASE \
                  WHEN $4::date IS NOT NULL THEN $4::date \
                  WHEN origin = 'legacy_month_only' THEN NULL \
                  WHEN booked_on IS NOT NULL \
                       AND date_part('year', booked_on) = $2 \
                       AND date_part('month', booked_on) = $3 THEN booked_on \
                  ELSE make_date($2::int, $3::int, 1) END, \
                kind = $5, amount_cents = $6, comment = $7, \
                tax_relevant = $8, category_id = $9, category_source = $10, \
                resolved_rule_id = $11, updated_at = now() \
          WHERE id = $1",
    )
    .bind(id)
    .bind(body.year)
    .bind(body.month as i16)
    .bind(body.booked_on)
    .bind(body.kind.as_db())
    .bind(body.amount_cents)
    .bind(body.comment.trim())
    .bind(body.tax_relevant)
    .bind(resolved.category_id)
    .bind(resolved.source)
    .bind(resolved.rule_id)
    .execute(ctx.tenant.conn())
    .await
    .map_err(|e| AppError::from_db(e, "Buchung konnte nicht gespeichert werden"))?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound("Buchung".into()));
    }

    let row = sqlx::query(&format!("{SELECT_BOOKING} WHERE b.id = $1"))
        .bind(id)
        .fetch_one(ctx.tenant.conn())
        .await?;
    let booking = row_to_booking(&row);
    ctx.tenant.commit().await?;
    Ok(Json(booking))
}

#[utoipa::path(
    post,
    path = "/api/v1/bookings/{id}/confirm",
    tag = "bookings",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    request_body = ConfirmBookingInput,
    responses((status = 200, description = "Entwurf bestätigt", body = Booking), (status = 404, description = "Nicht gefunden", body = crate::error::ErrorBody)),
)]
pub async fn confirm(
    mut ctx: Ctx,
    Path(id): Path<Uuid>,
    body: Option<Json<ConfirmBookingInput>>,
) -> Result<Json<Booking>> {
    let input = body.map(|Json(v)| v).unwrap_or_default();
    if let Some(amount) = input.amount_cents
        && amount <= 0
    {
        return Err(AppError::Validation("Der Betrag muss positiv sein".into()));
    }

    let year: Option<i16> = sqlx::query_scalar("SELECT period_year FROM bookings WHERE id = $1")
        .bind(id)
        .fetch_optional(ctx.tenant.conn())
        .await?;
    let year = year.ok_or_else(|| AppError::NotFound("Buchung".into()))?;
    assert_year_unlocked(ctx.tenant.conn(), year as i32).await?;

    sqlx::query(
        "UPDATE bookings SET status = 'confirmed', \
                amount_cents = COALESCE($2, amount_cents), updated_at = now() \
          WHERE id = $1",
    )
    .bind(id)
    .bind(input.amount_cents)
    .execute(ctx.tenant.conn())
    .await
    .map_err(|e| AppError::from_db(e, "Buchung konnte nicht bestätigt werden"))?;

    let row = sqlx::query(&format!("{SELECT_BOOKING} WHERE b.id = $1"))
        .bind(id)
        .fetch_one(ctx.tenant.conn())
        .await?;
    let booking = row_to_booking(&row);
    ctx.tenant.commit().await?;
    Ok(Json(booking))
}

/// Distinct comments, most used first. Feeds the Quick Add suggestion tiles and the
/// filter-by-comment view.
#[utoipa::path(
    delete,
    path = "/api/v1/bookings/{id}",
    tag = "bookings",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    responses((status = 204, description = "Gelöscht"), (status = 409, description = "Mit KitchenOwl verknüpft — erst die Verknüpfung lösen", body = crate::error::ErrorBody)),
)]
pub async fn delete(mut ctx: Ctx, Path(id): Path<Uuid>) -> Result<StatusCode> {
    let linked: Option<String> =
        sqlx::query_scalar("SELECT external_source FROM bookings WHERE id = $1")
            .bind(id)
            .fetch_optional(ctx.tenant.conn())
            .await?
            .flatten();
    // Only an EXPENSE link blocks deletion, and it blocks it because the mirror
    // holds a `linked_booking_id` that would be left pointing at nothing. A
    // settlement carries an external source too, but it references a balance rather
    // than a row — there is nothing to dangle and nothing the unlink endpoint could
    // detach, so guarding on "any external source" would make it undeletable with
    // an error message naming a step that does not exist.
    if linked.as_deref() == Some(crate::kitchenowl::link::SOURCE) {
        return Err(AppError::Conflict(
            "Die Buchung ist mit KitchenOwl verknüpft. Bitte zuerst die Verknüpfung lösen.".into(),
        ));
    }
    let affected = sqlx::query("DELETE FROM bookings WHERE id = $1")
        .bind(id)
        .execute(ctx.tenant.conn())
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound("Buchung".into()));
    }
    ctx.tenant.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkRequest {
    pub booking_ids: Vec<Uuid>,
    pub set_category_id: Option<Uuid>,
    pub clear_category: Option<bool>,
    pub set_tax_relevant: Option<bool>,
    pub set_kind: Option<BookingKind>,
    pub delete: Option<bool>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkResult {
    pub affected: i64,
}

#[utoipa::path(
    post,
    path = "/api/v1/bookings/bulk",
    tag = "bookings",
    request_body = BulkRequest,
    responses((status = 200, description = "Anzahl geänderter Buchungen", body = BulkResult), (status = 400, description = "Genau eine Änderung pro Massenaktion", body = crate::error::ErrorBody)),
)]
pub async fn bulk(mut ctx: Ctx, Json(body): Json<BulkRequest>) -> Result<Json<BulkResult>> {
    if body.booking_ids.is_empty() {
        return Err(AppError::Validation("Keine Buchungen ausgewählt".into()));
    }
    // Exactly one mutation per request, so a partially-specified bulk edit cannot do
    // something the caller did not intend.
    let chosen = [
        body.set_category_id.is_some(),
        body.clear_category == Some(true),
        body.set_tax_relevant.is_some(),
        body.set_kind.is_some(),
        body.delete == Some(true),
    ]
    .iter()
    .filter(|c| **c)
    .count();
    if chosen != 1 {
        return Err(AppError::Validation(
            "Genau eine Änderung pro Massenaktion angeben".into(),
        ));
    }

    let affected = if body.delete == Some(true) {
        sqlx::query("DELETE FROM bookings WHERE id = ANY($1) AND external_source IS NULL")
            .bind(&body.booking_ids)
            .execute(ctx.tenant.conn())
            .await?
            .rows_affected()
    } else if let Some(category_id) = body.set_category_id {
        sqlx::query(
            "UPDATE bookings SET category_id = $2, category_source = 'manual', \
                    resolved_rule_id = NULL, updated_at = now() WHERE id = ANY($1)",
        )
        .bind(&body.booking_ids)
        .bind(category_id)
        .execute(ctx.tenant.conn())
        .await?
        .rows_affected()
    } else if body.clear_category == Some(true) {
        sqlx::query(
            "UPDATE bookings SET category_id = NULL, category_source = 'unresolved', \
                    resolved_rule_id = NULL, updated_at = now() WHERE id = ANY($1)",
        )
        .bind(&body.booking_ids)
        .execute(ctx.tenant.conn())
        .await?
        .rows_affected()
    } else if let Some(tax) = body.set_tax_relevant {
        sqlx::query("UPDATE bookings SET tax_relevant = $2, updated_at = now() WHERE id = ANY($1)")
            .bind(&body.booking_ids)
            .bind(tax)
            .execute(ctx.tenant.conn())
            .await?
            .rows_affected()
    } else {
        let kind = body.set_kind.expect("checked above");
        sqlx::query("UPDATE bookings SET kind = $2, updated_at = now() WHERE id = ANY($1)")
            .bind(&body.booking_ids)
            .bind(kind.as_db())
            .execute(ctx.tenant.conn())
            .await?
            .rows_affected()
    };

    ctx.tenant.commit().await?;
    Ok(Json(BulkResult {
        affected: affected as i64,
    }))
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommentSummary {
    pub comment: String,
    pub count: i64,
    pub category_name: Option<String>,
    pub is_uncategorized: bool,
}

#[utoipa::path(
    get,
    path = "/api/v1/bookings/comments",
    tag = "bookings",
    responses((status = 200, description = "Verschiedene Kommentare, häufigste zuerst", body = Vec<CommentSummary>)),
)]
pub async fn comments(mut ctx: Ctx) -> Result<Json<Vec<CommentSummary>>> {
    let rows = sqlx::query(
        "SELECT b.comment, count(*)::bigint AS n, \
                max(c.name) AS category_name, \
                bool_and(b.category_id IS NULL) AS uncategorized \
           FROM bookings b LEFT JOIN categories c ON c.id = b.category_id \
          WHERE b.status = 'confirmed' \
          GROUP BY b.comment ORDER BY n DESC, b.comment LIMIT 500",
    )
    .fetch_all(ctx.tenant.conn())
    .await?;
    let out = rows
        .iter()
        .map(|r| CommentSummary {
            comment: r.get("comment"),
            count: r.get::<i64, _>("n"),
            category_name: r.get("category_name"),
            is_uncategorized: r.get("uncategorized"),
        })
        .collect();
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

// ------------------------------------------------------------------- search

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub q: Option<String>,
    pub category_id: Option<Uuid>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

/// Everything matching a phrase, across every year at once.
///
/// The listing is deliberately year-scoped — a ledger is read a year at a time —
/// which makes "what have I ever paid this merchant" four page loads and a mental
/// addition. This answers it in one request, and the per-year summary IS the
/// answer: the matching rows are only the evidence for it.
///
/// Matching is case-insensitive on the trimmed comment, the same rule the category
/// rules use, so `Kaufland` and `kaufland` are one merchant and not two. It is a
/// CONTAINS match rather than the rule table's exact one, because a person
/// searching types a fragment where a rule states a whole key.
#[utoipa::path(
    get,
    path = "/api/v1/bookings/search",
    tag = "bookings",
    params(
        ("q" = Option<String>, Query, description = "Suchbegriff; Kommentar enthält, Groß-/Kleinschreibung egal"),
        ("categoryId" = Option<Uuid>, Query, description = "Auf eine Kategorie einschränken"),
        ("page" = Option<u32>, Query, description = "Seite, ab 0"),
        ("pageSize" = Option<u32>, Query, description = "1..500, Standard 100"),
    ),
    responses((status = 200, description = "Treffer über alle Jahre, je Jahr summiert", body = SearchResult)),
)]
pub async fn search(mut ctx: Ctx, Query(q): Query<SearchQuery>) -> Result<Json<SearchResult>> {
    let needle = q.q.as_deref().unwrap_or("").trim().to_string();
    let page = q.page.unwrap_or(0);
    let page_size = q.page_size.unwrap_or(100).clamp(1, 500);

    // Nothing asked, nothing claimed. An empty needle must not quietly become
    // "every booking you have ever made" with a grand total underneath it.
    if needle.is_empty() {
        let out = SearchResult {
            query: needle,
            items: Vec::new(),
            total: 0,
            page,
            page_size,
            sum_income_cents: 0,
            sum_expense_cents: 0,
            sum_net_cents: 0,
            by_year: Vec::new(),
            comments: Vec::new(),
        };
        ctx.tenant.commit().await?;
        return Ok(Json(out));
    }

    // `match_key` is the stored `lower(btrim(comment))`, so the case folding is the
    // schema's and not this query's opinion. `LIKE` on a folded key rather than
    // `ILIKE` on the raw comment for the same reason.
    let mut where_sql =
        "b.status = 'confirmed' AND b.match_key LIKE '%' || lower(btrim($1)) || '%'".to_string();
    if q.category_id.is_some() {
        where_sql.push_str(" AND b.category_id = $2::uuid");
    }

    let order = order_clause(None);
    let list_sql = format!(
        "{SELECT_BOOKING} WHERE {where_sql} \
         ORDER BY {order} \
         LIMIT {page_size} OFFSET {}",
        page as i64 * page_size as i64
    );
    let mut list_q = sqlx::query(&list_sql).bind(&needle);
    if let Some(id) = q.category_id {
        list_q = list_q.bind(id);
    }
    let rows = list_q.fetch_all(ctx.tenant.conn()).await?;

    let totals_sql = format!(
        "SELECT count(*)::bigint AS total, \
                COALESCE(SUM(b.amount_cents) FILTER (WHERE b.kind = 'income'), 0)::bigint AS inc, \
                COALESCE(SUM(b.amount_cents) FILTER (WHERE b.kind = 'expense'), 0)::bigint AS exp, \
                COALESCE(SUM(b.net_cents), 0)::bigint AS net \
           FROM bookings b WHERE {where_sql}"
    );
    let mut totals_q = sqlx::query(&totals_sql).bind(&needle);
    if let Some(id) = q.category_id {
        totals_q = totals_q.bind(id);
    }
    let totals = totals_q.fetch_one(ctx.tenant.conn()).await?;

    let years_sql = format!(
        "SELECT b.period_year AS y, count(*)::bigint AS n, \
                COALESCE(SUM(b.amount_cents) FILTER (WHERE b.kind = 'income'), 0)::bigint AS inc, \
                COALESCE(SUM(b.amount_cents) FILTER (WHERE b.kind = 'expense'), 0)::bigint AS exp, \
                COALESCE(SUM(b.net_cents), 0)::bigint AS net \
           FROM bookings b WHERE {where_sql} \
          GROUP BY b.period_year ORDER BY b.period_year DESC"
    );
    let mut years_q = sqlx::query(&years_sql).bind(&needle);
    if let Some(id) = q.category_id {
        years_q = years_q.bind(id);
    }
    let year_rows = years_q.fetch_all(ctx.tenant.conn()).await?;

    // Grouped by the folded key and reported under the spelling used most recently:
    // two spellings of one merchant are one row, and the row says which it is now.
    let comments_sql = format!(
        "SELECT (array_agg(b.comment ORDER BY b.period_ord DESC, b.created_at DESC))[1] AS comment, \
                count(*)::bigint AS n, \
                COALESCE(SUM(b.net_cents), 0)::bigint AS net, \
                (array_agg(c.name ORDER BY b.period_ord DESC, b.created_at DESC))[1] AS category_name \
           FROM bookings b LEFT JOIN categories c ON c.id = b.category_id \
          WHERE {where_sql} \
          GROUP BY b.match_key ORDER BY n DESC, comment LIMIT 50"
    );
    let mut comments_q = sqlx::query(&comments_sql).bind(&needle);
    if let Some(id) = q.category_id {
        comments_q = comments_q.bind(id);
    }
    let comment_rows = comments_q.fetch_all(ctx.tenant.conn()).await?;

    let out = SearchResult {
        query: needle,
        items: rows.iter().map(row_to_booking).collect(),
        total: totals.get("total"),
        page,
        page_size,
        sum_income_cents: totals.get("inc"),
        sum_expense_cents: totals.get("exp"),
        sum_net_cents: totals.get("net"),
        by_year: year_rows
            .iter()
            .map(|r| SearchYearSummary {
                year: r.get::<i16, _>("y") as i32,
                booking_count: r.get("n"),
                income_cents: r.get("inc"),
                expense_cents: r.get("exp"),
                net_cents: r.get("net"),
            })
            .collect(),
        comments: comment_rows
            .iter()
            .map(|r| SearchComment {
                comment: r.get("comment"),
                booking_count: r.get("n"),
                net_cents: r.get("net"),
                category_name: r.get("category_name"),
            })
            .collect(),
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}
