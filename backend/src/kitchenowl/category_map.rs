//! Which finance category each KitchenOwl category counts in.
//!
//! Since migration 0024 my shares of what the flatmates paid count in my
//! statistics, and they need a category there. KitchenOwl's are a different
//! taxonomy — a handful of household labels against the app's 32 — so the bridge
//! is a map the user sets, one finance category per KitchenOwl category, kept in
//! `ko_categories.finances_category_id`. A sync never writes it (`mirror.rs`), and
//! nothing here guesses one into place: a name that matches is offered as a
//! suggestion, and only a `PUT` makes it the mapping. An unmapped category's shares
//! count without a category, where the "ohne Kategorie" figures show the gap.
//!
//! The same map also feeds the tagging queue's rule suggestions (`tagging.rs`),
//! which read it in the other direction.

use axum::{Json, extract::Path};
use sqlx::{PgConnection, Row};

use crate::{
    auth::Ctx,
    error::{AppError, Result},
    models::{KoCategoryMap, KoCategoryMapInput, KoCategoryMapping},
};

/// Every KitchenOwl category with its mapping, a suggestion where the names agree,
/// and what the mapping currently decides: how many of my shares, for how much.
///
/// The suggestion is an exact, case- and space-insensitive name match against the
/// finance categories that are not archived. "Haushalt" may well be "Haushalt";
/// anything looser ("Wocheneinkauf" → "Lebensmittel") is the user's call, not a
/// heuristic's. It is offered only while the category is unmapped — once the user
/// has decided, repeating the guess would only suggest they had not.
const SELECT_MAP: &str = "\
    SELECT k.category_id, k.name, k.finances_category_id, c.name AS finances_category_name, \
           CASE WHEN k.finances_category_id IS NULL THEN \
             (SELECT s.id FROM categories s \
               WHERE NOT s.archived AND lower(btrim(s.name)) = lower(btrim(k.name)) \
               ORDER BY s.id LIMIT 1) END AS suggested_category_id, \
           COALESCE(h.n, 0)::bigint AS share_count, \
           COALESCE(h.own, 0)::bigint AS own_share_cents \
      FROM ko_categories k \
      LEFT JOIN categories c ON c.id = k.finances_category_id \
      LEFT JOIN (SELECT ko_category_id, count(*) AS n, SUM(own_share_cents) AS own \
                   FROM v_ko_household_share GROUP BY ko_category_id) h \
             ON h.ko_category_id = k.category_id";

fn row_to_mapping(r: &sqlx::postgres::PgRow) -> KoCategoryMapping {
    KoCategoryMapping {
        ko_category_id: r.get("category_id"),
        name: r.get("name"),
        finances_category_id: r.get("finances_category_id"),
        finances_category_name: r.get("finances_category_name"),
        suggested_category_id: r.get("suggested_category_id"),
        share_count: r.get("share_count"),
        own_share_cents: r.get("own_share_cents"),
    }
}

async fn load(conn: &mut PgConnection) -> Result<KoCategoryMap> {
    let rows = sqlx::query(&format!("{SELECT_MAP} ORDER BY k.name, k.category_id"))
        .fetch_all(&mut *conn)
        .await?;
    // Expenses KitchenOwl has no category for cannot be mapped at all. They count
    // without a category too, and saying how many there are points at the fix —
    // tagging them in KitchenOwl — instead of leaving the gap unexplained.
    let untagged = sqlx::query(
        "SELECT count(*)::bigint AS n, COALESCE(SUM(own_share_cents), 0)::bigint AS own \
           FROM v_ko_household_share WHERE ko_category_id IS NULL",
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(KoCategoryMap {
        categories: rows.iter().map(row_to_mapping).collect(),
        untagged_share_count: untagged.get("n"),
        untagged_own_share_cents: untagged.get("own"),
    })
}

#[utoipa::path(
    get,
    path = "/api/v1/kitchenowl/category-map",
    tag = "kitchenowl",
    responses((status = 200, description = "Jede KitchenOwl-Kategorie mit ihrer Finanzkategorie, einem Vorschlag bei gleichem Namen und dem, was sie zählt", body = KoCategoryMap)),
)]
pub async fn list(mut ctx: Ctx) -> Result<Json<KoCategoryMap>> {
    let out = load(ctx.tenant.conn()).await?;
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[utoipa::path(
    put,
    path = "/api/v1/kitchenowl/category-map/{categoryId}",
    tag = "kitchenowl",
    params(("categoryId" = i64, Path, description = "KitchenOwl-Kategorie")),
    request_body = KoCategoryMapInput,
    responses(
        (status = 200, description = "Zuordnung gespeichert; `null` hebt sie auf", body = KoCategoryMapping),
        (status = 400, description = "Unbekannte Finanzkategorie", body = crate::error::ErrorBody),
        (status = 404, description = "Unbekannte KitchenOwl-Kategorie", body = crate::error::ErrorBody),
    ),
)]
pub async fn set(
    mut ctx: Ctx,
    Path(category_id): Path<i64>,
    Json(body): Json<KoCategoryMapInput>,
) -> Result<Json<KoCategoryMapping>> {
    let conn = ctx.tenant.conn();
    // Checked rather than left to the foreign key: the key would accept another
    // user's category id at the database level only to have row-level security
    // hide it again, and "unknown category" is the honest answer either way.
    if let Some(target) = body.finances_category_id {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM categories WHERE id = $1)")
                .bind(target)
                .fetch_one(&mut *conn)
                .await?;
        if !exists {
            return Err(AppError::Validation("Unbekannte Kategorie".into()));
        }
    }
    let updated =
        sqlx::query("UPDATE ko_categories SET finances_category_id = $2 WHERE category_id = $1")
            .bind(category_id)
            .bind(body.finances_category_id)
            .execute(&mut *conn)
            .await
            .map_err(|e| AppError::from_db(e, "Unbekannte Kategorie"))?
            .rows_affected();
    if updated == 0 {
        return Err(AppError::NotFound("KitchenOwl-Kategorie".into()));
    }
    let row = sqlx::query(&format!("{SELECT_MAP} WHERE k.category_id = $1"))
        .bind(category_id)
        .fetch_one(&mut *conn)
        .await?;
    let out = row_to_mapping(&row);
    ctx.tenant.commit().await?;
    Ok(Json(out))
}
