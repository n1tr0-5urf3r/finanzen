//! Comment -> category rules, and the retroactive recategorisation they trigger.

use axum::{Json, extract::Path, http::StatusCode};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::{
    auth::Ctx,
    error::{AppError, Result},
    models::{ApplyRulesResult, BookingKind, Rule, RuleInput},
};

fn row_to_rule(r: &sqlx::postgres::PgRow) -> Rule {
    Rule {
        id: r.get("id"),
        comment: r.get("pattern"),
        normalized_comment: r.get("match_key"),
        category_id: r.get("category_id"),
        category_name: r.get("category_name"),
        kind_override: r
            .get::<Option<String>, _>("kind_override")
            .and_then(|k| BookingKind::parse(&k)),
        source: r.get("source"),
        match_count: r.get::<i64, _>("match_count"),
    }
}

const SELECT_RULES: &str = "\
    SELECT r.id, r.pattern, r.match_key, r.category_id, c.name AS category_name, \
           r.kind_override, r.source, \
           (SELECT count(*) FROM bookings b \
             WHERE b.match_key = r.match_key AND b.status = 'confirmed')::bigint AS match_count \
      FROM category_rules r LEFT JOIN categories c ON c.id = r.category_id";

pub async fn list(mut ctx: Ctx) -> Result<Json<Vec<Rule>>> {
    let rows = sqlx::query(&format!("{SELECT_RULES} ORDER BY r.match_key"))
        .fetch_all(ctx.tenant.conn())
        .await?;
    let out = rows.iter().map(row_to_rule).collect();
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

pub async fn create(mut ctx: Ctx, Json(body): Json<RuleInput>) -> Result<(StatusCode, Json<Rule>)> {
    validate(&body)?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO category_rules (id, user_id, pattern, category_id, kind_override, source) \
         VALUES ($1, $2, $3, $4, $5, 'user')",
    )
    .bind(id)
    .bind(ctx.tenant.user_id())
    .bind(body.comment.trim())
    .bind(body.category_id)
    .bind(body.kind_override.map(|k| k.as_db()))
    .execute(ctx.tenant.conn())
    .await
    .map_err(|e| {
        AppError::from_db(
            e,
            "Für diesen Kommentar gibt es bereits eine Regel (Groß-/Kleinschreibung egal)",
        )
    })?;

    // A new rule applies to history immediately — that is the point of the rule
    // table. The affected count is returned so the UI can say how much moved.
    recategorize(ctx.tenant.conn(), false).await?;

    let row = sqlx::query(&format!("{SELECT_RULES} WHERE r.id = $1"))
        .bind(id)
        .fetch_one(ctx.tenant.conn())
        .await?;
    let rule = row_to_rule(&row);
    ctx.tenant.commit().await?;
    Ok((StatusCode::CREATED, Json(rule)))
}

pub async fn update(
    mut ctx: Ctx,
    Path(id): Path<Uuid>,
    Json(body): Json<RuleInput>,
) -> Result<Json<Rule>> {
    validate(&body)?;
    let affected = sqlx::query(
        "UPDATE category_rules SET pattern = $2, category_id = $3, kind_override = $4, \
                updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(body.comment.trim())
    .bind(body.category_id)
    .bind(body.kind_override.map(|k| k.as_db()))
    .execute(ctx.tenant.conn())
    .await
    .map_err(|e| AppError::from_db(e, "Für diesen Kommentar gibt es bereits eine Regel"))?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound("Regel".into()));
    }
    recategorize(ctx.tenant.conn(), false).await?;
    let row = sqlx::query(&format!("{SELECT_RULES} WHERE r.id = $1"))
        .bind(id)
        .fetch_one(ctx.tenant.conn())
        .await?;
    let rule = row_to_rule(&row);
    ctx.tenant.commit().await?;
    Ok(Json(rule))
}

pub async fn delete(mut ctx: Ctx, Path(id): Path<Uuid>) -> Result<StatusCode> {
    // Release the bookings first. The FK is ON DELETE SET NULL, which would leave
    // category_source = 'rule' with a NULL resolved_rule_id and trip the
    // bookings_rule_link CHECK, so deleting a rule any booking uses would fail.
    // Bookings that relied on this rule fall back to unresolved rather than keeping
    // a stale category — a rule is a convenience for entry, not a permanent claim
    // over history.
    sqlx::query(
        "UPDATE bookings SET category_id = NULL, category_source = 'unresolved', \
                resolved_rule_id = NULL, updated_at = now() \
          WHERE resolved_rule_id = $1 AND category_source = 'rule'",
    )
    .bind(id)
    .execute(ctx.tenant.conn())
    .await?;

    let affected = sqlx::query("DELETE FROM category_rules WHERE id = $1")
        .bind(id)
        .execute(ctx.tenant.conn())
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound("Regel".into()));
    }

    // Another rule may now match those comments, so re-resolve rather than assuming
    // they stay unresolved.
    recategorize(ctx.tenant.conn(), false).await?;
    ctx.tenant.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyQuery {
    #[serde(default)]
    pub dry_run: bool,
}

pub async fn apply(
    mut ctx: Ctx,
    axum::extract::Query(q): axum::extract::Query<ApplyQuery>,
) -> Result<Json<ApplyRulesResult>> {
    let result = recategorize(ctx.tenant.conn(), q.dry_run).await?;
    if q.dry_run {
        ctx.tenant.rollback().await?;
    } else {
        ctx.tenant.commit().await?;
    }
    Ok(Json(result))
}

fn validate(body: &RuleInput) -> Result<()> {
    if body.comment.trim().is_empty() {
        return Err(AppError::Validation("Kommentar fehlt".into()));
    }
    if body.category_id.is_none() && body.kind_override.is_none() {
        return Err(AppError::Validation(
            "Eine Regel muss eine Kategorie oder eine Buchungsart setzen".into(),
        ));
    }
    Ok(())
}

/// Re-resolves every rule-derived and unresolved booking against the current rule
/// table.
///
/// Three properties make this safe to run on every rule change:
///
/// * **Manual overrides are untouchable.** The `category_source IN ('rule',
///   'unresolved')` filter means a per-booking decision survives every rule edit.
/// * **Idempotent.** The `IS DISTINCT FROM` guard means a no-op change updates zero
///   rows and bumps no `updated_at`.
/// * **Counted.** `rows_affected` drives the UI confirmation. A silent retroactive
///   change to last year's tax report would be unacceptable; a counted one is fine.
///
/// Tax-locked years are excluded, because a submitted return must not move.
pub async fn recategorize(conn: &mut PgConnection, dry_run: bool) -> Result<ApplyRulesResult> {
    let examined: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bookings WHERE category_source IN ('rule','unresolved')",
    )
    .fetch_one(&mut *conn)
    .await?;

    let sql = "\
        WITH locked AS (SELECT year FROM fiscal_years WHERE tax_locked_at IS NOT NULL),
             resolved AS (
               SELECT b.id,
                      r.id  AS rule_id,
                      r.category_id
                 FROM bookings b
                 LEFT JOIN category_rules r ON r.match_key = b.match_key
                WHERE b.category_source IN ('rule','unresolved')
                  AND b.period_year NOT IN (SELECT year FROM locked)
             )
        UPDATE bookings b
           SET category_id      = resolved.category_id,
               resolved_rule_id = CASE WHEN resolved.category_id IS NULL
                                       THEN NULL ELSE resolved.rule_id END,
               category_source  = CASE WHEN resolved.category_id IS NULL
                                       THEN 'unresolved' ELSE 'rule' END,
               updated_at       = now()
          FROM resolved
         WHERE b.id = resolved.id
           AND (b.category_id IS DISTINCT FROM resolved.category_id
             OR b.resolved_rule_id IS DISTINCT FROM
                  (CASE WHEN resolved.category_id IS NULL THEN NULL ELSE resolved.rule_id END))";

    let recategorized = sqlx::query(sql).execute(&mut *conn).await?.rows_affected() as i64;

    // Transfers carry no category by design, so they are not "still uncategorised" —
    // counting them would leave a badge the user can never clear.
    let still_uncategorized: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bookings \
          WHERE category_source = 'unresolved' AND kind <> 'transfer'",
    )
    .fetch_one(&mut *conn)
    .await?;

    Ok(ApplyRulesResult {
        examined,
        recategorized,
        still_uncategorized,
        dry_run,
    })
}
