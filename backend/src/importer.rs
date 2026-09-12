//! Spreadsheet import: upload, dry-run preview, review queue, commit.
//!
//! Nothing is written to `bookings` until an explicit commit. The preview stages
//! every row in `import_rows` with its provenance down to the source cell, so a
//! number that disagrees by a cent can be traced back to the workbook.

use axum::{
    Json,
    extract::{Multipart, Path, Query},
    http::StatusCode,
};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::{
    AppState,
    auth::Ctx,
    error::{AppError, Result},
    sheets::{self, SheetBooking},
    suggest::{self, KnownRule},
};

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportCounts {
    pub data_rows: i64,
    pub new_bookings: i64,
    pub duplicates: i64,
    pub income: i64,
    pub expense: i64,
    pub transfer: i64,
    pub categorized: i64,
    pub uncategorized: i64,
    pub tax_relevant: i64,
    pub open_review_items: i64,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonthBlockInfo {
    pub index: i64,
    pub year: i32,
    pub month: u8,
    pub row_count: i64,
    pub label_source: String,
    pub raw_label: Option<String>,
    pub marker_cents: Option<i64>,
    pub computed_cents: i64,
    /// Non-zero when the block's own saldo marker disagrees with its rows. Recorded,
    /// never silently adjusted.
    pub delta_cents: Option<i64>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub id: Uuid,
    pub file_name: String,
    pub sheet: String,
    pub source: String,
    pub status: String,
    pub counts: ImportCounts,
    pub year_totals: Vec<YearTotals>,
    pub blocks: Vec<MonthBlockInfo>,
    /// Sum of the legacy sheet's own month-end markers, which is the authoritative
    /// carryover even where the rows disagree with it.
    pub marker_total_cents: Option<i64>,
    pub row_total_cents: Option<i64>,
    pub warnings: Vec<String>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct YearTotals {
    pub year: i32,
    pub months: i64,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub balance_cents: i64,
    pub booking_count: i64,
}

/// Stable identity for an imported row, so re-importing a corrected workbook inserts
/// only what is genuinely new. `occurrence` disambiguates the real duplicates in the
/// data — three `Mensaguthaben` 10,00 rows in one month are three distinct bookings.
fn fingerprint(b: &SheetBooking, occurrence: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!(
        "{}|{}|{}|{}|{}|{}",
        b.period_year,
        b.period_month,
        b.kind(),
        b.amount_cents(),
        b.comment.trim().to_lowercase(),
        occurrence
    ));
    hex::encode(hasher.finalize())
}

async fn known_rules(conn: &mut PgConnection) -> Result<Vec<KnownRule>> {
    let rows = sqlx::query(
        "SELECT r.match_key, r.category_id, c.name AS category_name \
           FROM category_rules r JOIN categories c ON c.id = r.category_id",
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .iter()
        .map(|r| KnownRule {
            match_key: r.get("match_key"),
            category_id: r.get("category_id"),
            category_name: r.get("category_name"),
        })
        .collect())
}

/// Resolving a review item writes a rule by default. That is what makes the queue
/// finishable: the same merchant is never asked about twice, and every future import
/// benefits.
#[utoipa::path(
    post,
    path = "/api/v1/imports",
    tag = "imports",
    responses((status = 201, description = "Vorschau; es wurde noch nichts gebucht", body = ImportPreview), (status = 422, description = "Monatsfolge widerspricht sich", body = crate::error::ErrorBody)),
)]
pub async fn upload(
    mut ctx: Ctx,
    axum::extract::State(state): axum::extract::State<AppState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<ImportPreview>)> {
    let mut file_name = String::new();
    let mut bytes: Vec<u8> = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Validation(format!("Upload nicht lesbar: {e}")))?
    {
        if field.name() == Some("file") {
            file_name = field.file_name().unwrap_or("upload").to_string();
            bytes = field
                .bytes()
                .await
                .map_err(|e| AppError::Validation(format!("Datei nicht lesbar: {e}")))?
                .to_vec();
        }
    }
    if bytes.is_empty() {
        return Err(AppError::Validation("Keine Datei übertragen".into()));
    }
    if bytes.len() > state.config.max_upload_bytes {
        return Err(AppError::Validation(format!(
            "Datei ist größer als {} MB",
            state.config.max_upload_bytes / 1_048_576
        )));
    }

    let sha = hex::encode(Sha256::digest(&bytes));
    // A byte-identical re-upload returns the existing preview instead of creating a
    // second job.
    if let Some(existing) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM import_batches WHERE sha256 = $1 AND status = 'preview'",
    )
    .bind(&sha)
    .fetch_optional(ctx.tenant.conn())
    .await?
    {
        let preview = load_preview(&mut ctx, existing).await?;
        ctx.tenant.commit().await?;
        return Ok((StatusCode::OK, Json(preview)));
    }

    let lower = file_name.to_lowercase();
    let (source, sheet, bookings, blocks, marker_total, row_total) = if lower.ends_with(".ods") {
        let legacy = sheets::read_ods_legacy(&bytes, "2023-2025", 2023, 6)?;
        (
            "ods_legacy",
            "2023-2025".to_string(),
            legacy.bookings,
            legacy.blocks,
            Some(legacy.marker_total_cents),
            Some(legacy.row_total_cents),
        )
    } else if lower.ends_with(".xlsx") {
        let bookings = sheets::read_xlsx_bookings(&bytes)?;
        (
            "xlsx_2026",
            "Buchungen".to_string(),
            bookings,
            Vec::new(),
            None,
            None,
        )
    } else {
        return Err(AppError::Validation(
            "Nur .xlsx und .ods werden unterstützt".into(),
        ));
    };

    if bookings.len() > state.config.import_max_rows {
        return Err(AppError::Validation(format!(
            "Die Datei enthält {} Zeilen (Grenze: {})",
            bookings.len(),
            state.config.import_max_rows
        )));
    }

    let batch_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO import_batches (id, user_id, source, filename, sha256, sheet, row_count) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(batch_id)
    .bind(ctx.tenant.user_id())
    .bind(source)
    .bind(&file_name)
    .bind(&sha)
    .bind(&sheet)
    .bind(bookings.len() as i32)
    .execute(ctx.tenant.conn())
    .await?;

    stage_rows(&mut ctx, batch_id, &bookings, &state).await?;
    stage_review_items(&mut ctx, batch_id, &state).await?;
    stage_blocks(&mut ctx, batch_id, &blocks, marker_total, row_total).await?;

    let preview = load_preview(&mut ctx, batch_id).await?;
    ctx.tenant.commit().await?;
    Ok((StatusCode::CREATED, Json(preview)))
}

async fn stage_rows(
    ctx: &mut Ctx,
    batch_id: Uuid,
    bookings: &[SheetBooking],
    _state: &AppState,
) -> Result<()> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for b in bookings {
        let dedupe_key = format!(
            "{}|{}|{}|{}|{}",
            b.period_year,
            b.period_month,
            b.kind(),
            b.amount_cents(),
            b.comment.trim().to_lowercase()
        );
        let occurrence = seen.entry(dedupe_key).or_insert(0);
        let fp = fingerprint(b, *occurrence);
        *occurrence += 1;

        // A rule may force the kind; that is how `to ING` / `from Volksbank` become
        // transfers without a list baked into the binary.
        let kind = if suggest::transfer_hint(&b.comment) {
            "transfer"
        } else {
            b.kind()
        };

        let already: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM bookings WHERE import_fingerprint = $1)",
        )
        .bind(&fp)
        .fetch_one(ctx.tenant.conn())
        .await?;

        // The workbook's per-row "Kategorie manuell" column is an override that beats
        // the rule table, so it is resolved to a category id here rather than left
        // in `raw` where the commit would ignore it.
        let manual_category_id: Option<Uuid> = match &b.manual_category {
            Some(name) if !name.trim().is_empty() => {
                sqlx::query_scalar("SELECT id FROM categories WHERE lower(name) = lower($1)")
                    .bind(name.trim())
                    .fetch_optional(ctx.tenant.conn())
                    .await?
            }
            _ => None,
        };

        sqlx::query(
            "INSERT INTO import_rows (id, user_id, batch_id, source_ref, raw, period_year, \
                                      period_month, kind, amount_cents, comment, tax_relevant, \
                                      status, fingerprint, decided_category_id) \
             VALUES ($1,$2,$3,$4,$5,$6::smallint,$7::smallint,$8,$9,$10,$11,$12,$13,$14) \
             ON CONFLICT (batch_id, source_ref) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(ctx.tenant.user_id())
        .bind(batch_id)
        .bind(&b.source_ref)
        // The verbatim source value, artifacts and all, so a disputed cent is
        // traceable to the original cell.
        .bind(serde_json::json!({
            "rawAmount": b.raw_amount,
            "manualCategory": b.manual_category,
        }))
        .bind(b.period_year)
        .bind(b.period_month as i16)
        .bind(kind)
        .bind(b.amount_cents())
        .bind(b.comment.trim())
        .bind(b.tax_relevant)
        .bind(if already { "duplicate" } else { "new" })
        .bind(&fp)
        .bind(manual_category_id)
        .execute(ctx.tenant.conn())
        .await?;
    }
    Ok(())
}

/// Builds the review queue, keyed by DISTINCT COMMENT rather than by row: ~240
/// decisions instead of ~362, sorted so the highest-frequency merchants come first.
async fn stage_review_items(ctx: &mut Ctx, batch_id: Uuid, state: &AppState) -> Result<()> {
    let rules = known_rules(ctx.tenant.conn()).await?;

    let rows = sqlx::query(
        "SELECT min(comment) AS comment, lower(btrim(comment)) AS normalized, \
                count(*)::bigint AS n, \
                COALESCE(SUM(amount_cents) FILTER (WHERE kind = 'expense'), 0)::bigint AS exp, \
                COALESCE(SUM(amount_cents) FILTER (WHERE kind = 'income'), 0)::bigint AS inc \
           FROM import_rows \
          WHERE batch_id = $1 \
            AND decided_category_id IS NULL \
            AND NOT EXISTS (SELECT 1 FROM category_rules r \
                             WHERE r.match_key = lower(btrim(import_rows.comment))) \
          GROUP BY lower(btrim(comment)) ORDER BY n DESC",
    )
    .bind(batch_id)
    .fetch_all(ctx.tenant.conn())
    .await?;

    for r in &rows {
        let comment: String = r.get("comment");
        let (suggestions, hints, ambiguous) =
            suggest::suggest(&comment, &rules, state.config.import_fuzzy_min_confidence);

        let to_json = |v: &Vec<suggest::Suggestion>| {
            serde_json::json!(
                v.iter()
                    .map(|s| serde_json::json!({
                        "categoryId": s.category_id,
                        "categoryName": s.category_name,
                        "matchedRule": s.matched_rule,
                        "confidence": s.confidence,
                        "tier": s.tier,
                        "isSuggestion": s.is_suggestion,
                    }))
                    .collect::<Vec<_>>()
            )
        };

        sqlx::query(
            "INSERT INTO import_review_items (id, user_id, batch_id, comment, normalized, \
                                              row_count, expense_cents, income_cents, \
                                              suggestions, weak_hints, ambiguous, suggested_kind) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) \
             ON CONFLICT (batch_id, normalized) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(ctx.tenant.user_id())
        .bind(batch_id)
        .bind(&comment)
        .bind(r.get::<String, _>("normalized"))
        .bind(r.get::<i64, _>("n") as i32)
        .bind(r.get::<i64, _>("exp"))
        .bind(r.get::<i64, _>("inc"))
        .bind(to_json(&suggestions))
        .bind(to_json(&hints))
        .bind(ambiguous)
        .bind(suggest::transfer_hint(&comment).then_some("transfer"))
        .execute(ctx.tenant.conn())
        .await?;
    }
    Ok(())
}

async fn stage_blocks(
    ctx: &mut Ctx,
    batch_id: Uuid,
    blocks: &[sheets::MonthBlock],
    marker_total: Option<i64>,
    row_total: Option<i64>,
) -> Result<()> {
    if blocks.is_empty() {
        return Ok(());
    }
    let payload: Vec<serde_json::Value> = blocks
        .iter()
        .map(|b| {
            serde_json::json!({
                "index": b.index, "year": b.year, "month": b.month,
                "rowCount": b.row_count, "labelSource": b.label_source,
                "rawLabel": b.raw_label, "markerCents": b.marker_cents,
                "computedCents": b.computed_cents,
            })
        })
        .collect();
    sqlx::query("UPDATE import_batches SET stats = $2 WHERE id = $1")
        .bind(batch_id)
        .bind(serde_json::json!({
            "blocks": payload,
            "markerTotalCents": marker_total,
            "rowTotalCents": row_total,
        }))
        .execute(ctx.tenant.conn())
        .await?;
    Ok(())
}

async fn load_preview(ctx: &mut Ctx, batch_id: Uuid) -> Result<ImportPreview> {
    let batch = sqlx::query(
        "SELECT id, filename, sheet, source, status, stats FROM import_batches WHERE id = $1",
    )
    .bind(batch_id)
    .fetch_optional(ctx.tenant.conn())
    .await?
    .ok_or_else(|| AppError::NotFound("Import".into()))?;

    let counts_row = sqlx::query(
        "SELECT count(*)::bigint AS n, \
                count(*) FILTER (WHERE status = 'new')::bigint AS new_rows, \
                count(*) FILTER (WHERE status = 'duplicate')::bigint AS dupes, \
                count(*) FILTER (WHERE kind = 'income')::bigint AS inc, \
                count(*) FILTER (WHERE kind = 'expense')::bigint AS exp, \
                count(*) FILTER (WHERE kind = 'transfer')::bigint AS xfer, \
                count(*) FILTER (WHERE tax_relevant)::bigint AS tax, \
                count(*) FILTER (WHERE decided_category_id IS NOT NULL OR EXISTS ( \
                    SELECT 1 FROM category_rules r \
                     WHERE r.match_key = lower(btrim(import_rows.comment))))::bigint AS categorized \
           FROM import_rows WHERE batch_id = $1",
    )
    .bind(batch_id)
    .fetch_one(ctx.tenant.conn())
    .await?;

    let open_review: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM import_review_items WHERE batch_id = $1 AND status = 'open'",
    )
    .bind(batch_id)
    .fetch_one(ctx.tenant.conn())
    .await?;

    let year_rows = sqlx::query(
        "SELECT period_year, count(DISTINCT period_month)::bigint AS months, \
                count(*)::bigint AS n, \
                COALESCE(SUM(amount_cents) FILTER (WHERE kind = 'income'), 0)::bigint AS inc, \
                COALESCE(SUM(amount_cents) FILTER (WHERE kind = 'expense'), 0)::bigint AS exp \
           FROM import_rows WHERE batch_id = $1 GROUP BY period_year ORDER BY period_year",
    )
    .bind(batch_id)
    .fetch_all(ctx.tenant.conn())
    .await?;

    let stats: serde_json::Value = batch.get("stats");
    let blocks: Vec<MonthBlockInfo> = stats["blocks"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|b| {
                    let marker = b["markerCents"].as_i64();
                    let computed = b["computedCents"].as_i64().unwrap_or(0);
                    MonthBlockInfo {
                        index: b["index"].as_i64().unwrap_or(0),
                        year: b["year"].as_i64().unwrap_or(0) as i32,
                        month: b["month"].as_u64().unwrap_or(1) as u8,
                        row_count: b["rowCount"].as_i64().unwrap_or(0),
                        label_source: b["labelSource"].as_str().unwrap_or("inferred").to_string(),
                        raw_label: b["rawLabel"].as_str().map(str::to_string),
                        marker_cents: marker,
                        computed_cents: computed,
                        delta_cents: marker.filter(|m| *m != computed).map(|m| computed - m),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let mut warnings = Vec::new();
    let mismatched: Vec<&MonthBlockInfo> =
        blocks.iter().filter(|b| b.delta_cents.is_some()).collect();
    if !mismatched.is_empty() {
        warnings.push(format!(
            "{} Monatsblöcke weichen von ihrem eigenen Saldo-Marker ab. \
             Die Zeilen werden unverändert übernommen; der Vortrag bleibt ein \
             konfigurierter Wert.",
            mismatched.len()
        ));
    }

    let uncategorized = counts_row.get::<i64, _>("n") - counts_row.get::<i64, _>("categorized");

    Ok(ImportPreview {
        id: batch.get("id"),
        file_name: batch.get("filename"),
        sheet: batch.get("sheet"),
        source: batch.get("source"),
        status: batch.get("status"),
        counts: ImportCounts {
            data_rows: counts_row.get("n"),
            new_bookings: counts_row.get("new_rows"),
            duplicates: counts_row.get("dupes"),
            income: counts_row.get("inc"),
            expense: counts_row.get("exp"),
            transfer: counts_row.get("xfer"),
            categorized: counts_row.get("categorized"),
            uncategorized,
            tax_relevant: counts_row.get("tax"),
            open_review_items: open_review,
        },
        year_totals: year_rows
            .iter()
            .map(|r| {
                let inc: i64 = r.get("inc");
                let exp: i64 = r.get("exp");
                YearTotals {
                    year: r.get::<i16, _>("period_year") as i32,
                    months: r.get("months"),
                    income_cents: inc,
                    expense_cents: exp,
                    balance_cents: inc - exp,
                    booking_count: r.get("n"),
                }
            })
            .collect(),
        blocks,
        marker_total_cents: stats["markerTotalCents"].as_i64(),
        row_total_cents: stats["rowTotalCents"].as_i64(),
        warnings,
    })
}

#[utoipa::path(
    get,
    path = "/api/v1/imports/{id}",
    tag = "imports",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    responses((status = 200, description = "Die Vorschau", body = ImportPreview), (status = 404, description = "Nicht gefunden", body = crate::error::ErrorBody)),
)]
pub async fn get(mut ctx: Ctx, Path(id): Path<Uuid>) -> Result<Json<ImportPreview>> {
    let preview = load_preview(&mut ctx, id).await?;
    ctx.tenant.commit().await?;
    Ok(Json(preview))
}

#[utoipa::path(
    get,
    path = "/api/v1/imports",
    tag = "imports",
    responses((status = 200, description = "Bisherige Importe", body = Vec<serde_json::Value>)),
)]
pub async fn list(mut ctx: Ctx) -> Result<Json<Vec<serde_json::Value>>> {
    let rows = sqlx::query(
        "SELECT id, filename, sheet, source, status, row_count, created_at, applied_at \
           FROM import_batches ORDER BY created_at DESC LIMIT 50",
    )
    .fetch_all(ctx.tenant.conn())
    .await?;
    let out = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<Uuid, _>("id"),
                "fileName": r.get::<String, _>("filename"),
                "sheet": r.get::<Option<String>, _>("sheet"),
                "source": r.get::<String, _>("source"),
                "status": r.get::<String, _>("status"),
                "rowCount": r.get::<i32, _>("row_count"),
                "createdAt": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
                "appliedAt": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("applied_at"),
            })
        })
        .collect();
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommitResult {
    pub inserted: i64,
    pub skipped: i64,
    pub years_touched: Vec<i32>,
    pub uncategorized_remaining: i64,
}

#[utoipa::path(
    post,
    path = "/api/v1/imports/{id}/commit",
    tag = "imports",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    responses((status = 200, description = "Gebucht; erneutes Buchen ist wirkungslos", body = CommitResult)),
)]
pub async fn commit(mut ctx: Ctx, Path(id): Path<Uuid>) -> Result<Json<CommitResult>> {
    let status: String = sqlx::query_scalar("SELECT status FROM import_batches WHERE id = $1")
        .bind(id)
        .fetch_optional(ctx.tenant.conn())
        .await?
        .ok_or_else(|| AppError::NotFound("Import".into()))?;
    if status != "preview" {
        return Err(AppError::Conflict(
            "Dieser Import wurde bereits übernommen".into(),
        ));
    }

    // Years must exist before bookings reference them; the opening balance stays a
    // configured value the user sets, never derived from the imported rows.
    let years: Vec<i16> = sqlx::query_scalar(
        "SELECT DISTINCT period_year FROM import_rows WHERE batch_id = $1 ORDER BY 1",
    )
    .bind(id)
    .fetch_all(ctx.tenant.conn())
    .await?;
    for year in &years {
        sqlx::query(
            "INSERT INTO fiscal_years (user_id, year, opening_cents, opening_source) \
             VALUES ($1, $2, 0, 'derived') ON CONFLICT (user_id, year) DO NOTHING",
        )
        .bind(ctx.tenant.user_id())
        .bind(year)
        .execute(ctx.tenant.conn())
        .await?;
    }

    // Imported rows are month-only: the source genuinely has no day, and inventing
    // one would make the "has a real date" signal meaningless.
    let inserted = sqlx::query(
        "INSERT INTO bookings (id, user_id, period_year, period_month, kind, amount_cents, \
                               comment, tax_relevant, category_id, category_source, \
                               resolved_rule_id, origin, import_row_id, import_fingerprint) \
         SELECT gen_random_uuid(), ir.user_id, ir.period_year, ir.period_month, ir.kind, \
                ir.amount_cents, ir.comment, ir.tax_relevant, \
                COALESCE(ir.decided_category_id, r.category_id), \
                CASE WHEN ir.decided_category_id IS NOT NULL THEN 'manual' \
                     WHEN r.category_id IS NULL THEN 'unresolved' ELSE 'rule' END, \
                CASE WHEN ir.decided_category_id IS NOT NULL OR r.category_id IS NULL \
                     THEN NULL ELSE r.id END, \
                'legacy_month_only', ir.id, ir.fingerprint \
           FROM import_rows ir \
           LEFT JOIN category_rules r ON r.match_key = lower(btrim(ir.comment)) \
          WHERE ir.batch_id = $1 AND ir.decision <> 'rejected' \
         ON CONFLICT (user_id, import_fingerprint) WHERE import_fingerprint IS NOT NULL \
         DO NOTHING",
    )
    .bind(id)
    .execute(ctx.tenant.conn())
    .await?
    .rows_affected() as i64;

    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM import_rows WHERE batch_id = $1")
        .bind(id)
        .fetch_one(ctx.tenant.conn())
        .await?;

    sqlx::query("UPDATE import_batches SET status = 'applied', applied_at = now() WHERE id = $1")
        .bind(id)
        .execute(ctx.tenant.conn())
        .await?;

    let uncategorized_remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bookings WHERE category_source = 'unresolved' AND kind <> 'transfer'",
    )
    .fetch_one(ctx.tenant.conn())
    .await?;

    ctx.tenant.commit().await?;
    Ok(Json(CommitResult {
        inserted,
        skipped: total - inserted,
        years_touched: years.into_iter().map(|y| y as i32).collect(),
        uncategorized_remaining,
    }))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/v1/imports/{id}/review",
    tag = "imports",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    responses((status = 200, description = "Kommentare ohne Regel, nach Häufigkeit", body = Vec<serde_json::Value>)),
)]
pub async fn review(
    mut ctx: Ctx,
    Path(id): Path<Uuid>,
    Query(q): Query<ReviewQuery>,
) -> Result<Json<Vec<serde_json::Value>>> {
    let status = q.status.unwrap_or_else(|| "open".into());
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);
    let rows = sqlx::query(
        "SELECT id, comment, normalized, row_count, expense_cents, income_cents, \
                suggestions, weak_hints, ambiguous, suggested_kind, status \
           FROM import_review_items \
          WHERE batch_id = $1 AND ($2 = 'all' OR status = $2) \
          ORDER BY row_count DESC, comment LIMIT $3",
    )
    .bind(id)
    .bind(&status)
    .bind(limit)
    .fetch_all(ctx.tenant.conn())
    .await?;

    let out = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<Uuid, _>("id"),
                "comment": r.get::<String, _>("comment"),
                "normalizedComment": r.get::<String, _>("normalized"),
                "rowCount": r.get::<i32, _>("row_count"),
                "expenseCents": r.get::<i64, _>("expense_cents"),
                "incomeCents": r.get::<i64, _>("income_cents"),
                "suggestions": r.get::<serde_json::Value, _>("suggestions"),
                "weakHints": r.get::<serde_json::Value, _>("weak_hints"),
                "ambiguous": r.get::<bool, _>("ambiguous"),
                "suggestedKind": r.get::<Option<String>, _>("suggested_kind"),
                "status": r.get::<String, _>("status"),
            })
        })
        .collect();
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolveRequest {
    pub resolutions: Vec<Resolution>,
}

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Resolution {
    pub item_id: Uuid,
    pub category_id: Option<Uuid>,
    #[serde(default = "default_true")]
    pub create_rule: bool,
    #[serde(default)]
    pub skip: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolveResult {
    pub resolved: i64,
    pub skipped: i64,
    pub rules_created: i64,
    pub remaining_open: i64,
}

#[utoipa::path(
    post,
    path = "/api/v1/imports/{id}/review",
    tag = "imports",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    request_body = ResolveRequest,
    responses((status = 200, description = "Regel angelegt, Historie zugeordnet", body = ResolveResult)),
)]
pub async fn resolve(
    mut ctx: Ctx,
    Path(batch_id): Path<Uuid>,
    Json(body): Json<ResolveRequest>,
) -> Result<Json<ResolveResult>> {
    let (mut resolved, mut skipped, mut rules_created) = (0i64, 0i64, 0i64);

    for r in &body.resolutions {
        if r.skip {
            sqlx::query(
                "UPDATE import_review_items SET status = 'skipped' \
                  WHERE id = $1 AND batch_id = $2",
            )
            .bind(r.item_id)
            .bind(batch_id)
            .execute(ctx.tenant.conn())
            .await?;
            skipped += 1;
            continue;
        }
        let Some(category_id) = r.category_id else {
            return Err(AppError::Validation(
                "Zum Zuordnen wird eine Kategorie benötigt".into(),
            ));
        };

        let comment: String = sqlx::query_scalar(
            "SELECT comment FROM import_review_items WHERE id = $1 AND batch_id = $2",
        )
        .bind(r.item_id)
        .bind(batch_id)
        .fetch_optional(ctx.tenant.conn())
        .await?
        .ok_or_else(|| AppError::NotFound("Prüflisten-Eintrag".into()))?;

        if r.create_rule {
            let created = sqlx::query(
                "INSERT INTO category_rules (id, user_id, pattern, category_id, source) \
                 VALUES ($1, $2, $3, $4, 'review') \
                 ON CONFLICT (user_id, match_key) DO NOTHING",
            )
            .bind(Uuid::new_v4())
            .bind(ctx.tenant.user_id())
            .bind(&comment)
            .bind(category_id)
            .execute(ctx.tenant.conn())
            .await?
            .rows_affected();
            rules_created += created as i64;
        }

        sqlx::query(
            "UPDATE import_review_items SET status = 'resolved', resolved_category_id = $3, \
                    rule_created = $4 WHERE id = $1 AND batch_id = $2",
        )
        .bind(r.item_id)
        .bind(batch_id)
        .bind(category_id)
        .bind(r.create_rule)
        .execute(ctx.tenant.conn())
        .await?;
        resolved += 1;
    }

    // Already-committed bookings pick up the new rules immediately.
    crate::rules::recategorize(ctx.tenant.conn(), false).await?;

    let remaining_open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM import_review_items WHERE batch_id = $1 AND status = 'open'",
    )
    .bind(batch_id)
    .fetch_one(ctx.tenant.conn())
    .await?;

    ctx.tenant.commit().await?;
    Ok(Json(ResolveResult {
        resolved,
        skipped,
        rules_created,
        remaining_open,
    }))
}
