//! Categories and their types.

use axum::{Json, extract::Path, http::StatusCode};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::Ctx,
    error::{AppError, Result},
    models::{Category, CategoryInput, CategoryTypeSummary},
};

const SELECT_CATEGORIES: &str = "\
    SELECT c.id, c.name, t.code AS type_code, t.label AS type_label, c.sort_order, c.archived, \
           (SELECT count(*) FROM bookings b WHERE b.category_id = c.id \
             AND b.status = 'confirmed')::bigint AS booking_count, \
           (SELECT COALESCE(SUM(b.net_cents), 0) FROM bookings b WHERE b.category_id = c.id \
             AND b.status = 'confirmed')::bigint AS net_cents \
      FROM categories c JOIN category_types t ON t.id = c.type_id";

fn row_to_category(r: &sqlx::postgres::PgRow) -> Category {
    Category {
        id: r.get("id"),
        name: r.get("name"),
        type_code: r.get("type_code"),
        type_label: r.get("type_label"),
        sort_order: r.get("sort_order"),
        archived: r.get("archived"),
        booking_count: Some(r.get::<i64, _>("booking_count")),
        net_cents: Some(r.get::<i64, _>("net_cents")),
    }
}

pub async fn list(mut ctx: Ctx) -> Result<Json<Vec<Category>>> {
    let rows = sqlx::query(&format!(
        "{SELECT_CATEGORIES} ORDER BY t.sort_order, c.sort_order, c.name"
    ))
    .fetch_all(ctx.tenant.conn())
    .await?;
    let out = rows.iter().map(row_to_category).collect();
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

pub async fn list_types(mut ctx: Ctx) -> Result<Json<Vec<CategoryTypeSummary>>> {
    let rows = sqlx::query(
        "SELECT t.code, t.label, \
                COALESCE(SUM(b.net_cents), 0)::bigint AS net_cents, \
                count(b.id)::bigint AS booking_count \
           FROM category_types t \
           LEFT JOIN categories c ON c.type_id = t.id \
           LEFT JOIN bookings b ON b.category_id = c.id AND b.status = 'confirmed' \
          GROUP BY t.code, t.label, t.sort_order ORDER BY t.sort_order",
    )
    .fetch_all(ctx.tenant.conn())
    .await?;
    let out = rows
        .iter()
        .map(|r| CategoryTypeSummary {
            type_code: r.get("code"),
            label: r.get("label"),
            net_cents: r.get::<i64, _>("net_cents"),
            booking_count: r.get::<i64, _>("booking_count"),
        })
        .collect();
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

pub async fn create(
    mut ctx: Ctx,
    Json(body): Json<CategoryInput>,
) -> Result<(StatusCode, Json<Category>)> {
    if body.name.trim().is_empty() {
        return Err(AppError::Validation("Name fehlt".into()));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO categories (id, user_id, type_id, name, sort_order) \
         VALUES ($1, $2, (SELECT id FROM category_types WHERE code = $3), $4, \
                 COALESCE($5, 999))",
    )
    .bind(id)
    .bind(ctx.tenant.user_id())
    .bind(&body.type_code)
    .bind(body.name.trim())
    .bind(body.sort_order)
    .execute(ctx.tenant.conn())
    .await
    .map_err(|e| AppError::from_db(e, "Diese Kategorie gibt es bereits"))?;

    let row = sqlx::query(&format!("{SELECT_CATEGORIES} WHERE c.id = $1"))
        .bind(id)
        .fetch_one(ctx.tenant.conn())
        .await?;
    let category = row_to_category(&row);
    ctx.tenant.commit().await?;
    Ok((StatusCode::CREATED, Json(category)))
}

pub async fn update(
    mut ctx: Ctx,
    Path(id): Path<Uuid>,
    Json(body): Json<CategoryInput>,
) -> Result<Json<Category>> {
    let affected = sqlx::query(
        "UPDATE categories SET name = $2, \
                type_id = (SELECT id FROM category_types WHERE code = $3), \
                sort_order = COALESCE($4, sort_order) WHERE id = $1",
    )
    .bind(id)
    .bind(body.name.trim())
    .bind(&body.type_code)
    .bind(body.sort_order)
    .execute(ctx.tenant.conn())
    .await
    .map_err(|e| AppError::from_db(e, "Diese Kategorie gibt es bereits"))?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound("Kategorie".into()));
    }
    let row = sqlx::query(&format!("{SELECT_CATEGORIES} WHERE c.id = $1"))
        .bind(id)
        .fetch_one(ctx.tenant.conn())
        .await?;
    let category = row_to_category(&row);
    ctx.tenant.commit().await?;
    Ok(Json(category))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteQuery {
    pub reassign_to: Option<Uuid>,
}

pub async fn delete(
    mut ctx: Ctx,
    Path(id): Path<Uuid>,
    axum::extract::Query(q): axum::extract::Query<DeleteQuery>,
) -> Result<StatusCode> {
    let in_use: i64 = sqlx::query_scalar("SELECT count(*) FROM bookings WHERE category_id = $1")
        .bind(id)
        .fetch_one(ctx.tenant.conn())
        .await?;

    if in_use > 0 {
        let Some(target) = q.reassign_to else {
            // Refusing with the count lets the UI say "23 Buchungen betroffen"
            // instead of failing opaquely.
            return Err(AppError::Conflict(format!(
                "Die Kategorie wird von {in_use} Buchungen verwendet. \
                 Bitte eine Zielkategorie zum Umhängen angeben."
            )));
        };
        sqlx::query(
            "UPDATE bookings SET category_id = $2, updated_at = now() WHERE category_id = $1",
        )
        .bind(id)
        .bind(target)
        .execute(ctx.tenant.conn())
        .await?;
    }

    let affected = sqlx::query("DELETE FROM categories WHERE id = $1")
        .bind(id)
        .execute(ctx.tenant.conn())
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound("Kategorie".into()));
    }
    ctx.tenant.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
