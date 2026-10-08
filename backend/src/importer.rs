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

    // A bank statement takes a different path from here: its rows are not
    // bookings yet. Every line is reviewed by hand — the comment a statement
    // gives you is a card terminal's idea of a shop name — so it stages and
    // returns, and the preview counts what is waiting rather than what is ready.
    if lower.ends_with(".csv") {
        let statement = crate::bank::read_ing_csv(&bytes)?;
        if statement.rows.len() > state.config.import_max_rows {
            return Err(AppError::Validation(format!(
                "Die Datei enthält {} Zeilen (Grenze: {})",
                statement.rows.len(),
                state.config.import_max_rows
            )));
        }
        let batch_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO import_batches (id, user_id, source, filename, sha256, sheet, row_count, \
                                         stats) \
             VALUES ($1, $2, 'csv_ing', $3, $4, $5, $6, $7)",
        )
        .bind(batch_id)
        .bind(ctx.tenant.user_id())
        .bind(&file_name)
        .bind(&sha)
        .bind(
            statement
                .meta
                .account_name
                .clone()
                .unwrap_or_else(|| "Kontoauszug".to_string()),
        )
        .bind(statement.rows.len() as i32)
        .bind(serde_json::json!({
            "iban": statement.meta.iban,
            "bank": statement.meta.bank,
            "accountName": statement.meta.account_name,
            "period": statement.meta.period,
            "balanceCents": statement.meta.balance_cents,
        }))
        .execute(ctx.tenant.conn())
        .await?;

        stage_statement(
            &mut ctx,
            batch_id,
            &statement,
            state.config.import_fuzzy_min_confidence,
        )
        .await?;
        let preview = load_preview(&mut ctx, batch_id).await?;
        ctx.tenant.commit().await?;
        return Ok((StatusCode::CREATED, Json(preview)));
    }

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
        // Two different workbooks share the extension, so the shape decides, not the
        // file name: the 2026 book has a `Buchungen` sheet, the 2014–2023 one has a
        // `Monat | Einnahmen | Ausgaben | Zweck` header and a block per month.
        match sheets::read_xlsx_bookings(&bytes) {
            Ok(bookings) => (
                "xlsx_2026",
                "Buchungen".to_string(),
                bookings,
                Vec::new(),
                None,
                None,
            ),
            Err(buchungen_err) => {
                let monthly = sheets::read_xlsx_monthly(&bytes).map_err(|monthly_err| {
                    // Neither shape fits. Reporting only the second attempt would
                    // send someone looking for a month column in a file that was
                    // meant to be the other kind, so both reasons are named.
                    AppError::Validation(format!(
                        "Unbekannte Arbeitsmappe. Als Buchungsblatt: {buchungen_err}.                          Als Monatsblöcke: {monthly_err}"
                    ))
                })?;
                (
                    "xlsx_monthly",
                    monthly
                        .bookings
                        .first()
                        .map(|b| b.source_ref.clone())
                        .and_then(|r| {
                            r.strip_prefix("xlsx!")
                                .and_then(|r| r.split(':').next())
                                .map(str::to_string)
                        })
                        .unwrap_or_else(|| "Tabelle1".to_string()),
                    monthly.bookings,
                    monthly.blocks,
                    Some(monthly.marker_total_cents),
                    Some(monthly.row_total_cents),
                )
            }
        }
    } else {
        return Err(AppError::Validation(
            "Nur .xlsx, .ods und .csv werden unterstützt".into(),
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

    // A month with no marker at all is a different thing from one that disagrees:
    // nothing is wrong with its rows, there is simply nothing to check them against,
    // and the totals above therefore leave it out.
    let unmarked: Vec<String> = blocks
        .iter()
        .filter(|b| b.marker_cents.is_none())
        .map(|b| format!("{} {}", crate::locale::month_name_de(b.month), b.year))
        .collect();
    if !unmarked.is_empty() {
        warnings.push(format!(
            "{} Monate ohne eigenen Saldo-Marker ({}). \
             Ihre Zeilen werden importiert, stehen aber in keiner Gegenprobe.",
            unmarked.len(),
            unmarked.join(", ")
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
    /// Bookings from this import queued for KitchenOwl as well.
    pub ko_queued: i64,
}

#[utoipa::path(
    post,
    path = "/api/v1/imports/{id}/commit",
    tag = "imports",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    responses((status = 200, description = "Gebucht; erneutes Buchen ist wirkungslos", body = CommitResult)),
)]
pub async fn commit(
    mut ctx: Ctx,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<CommitResult>> {
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

    // A locked year is a filed return, and an import must not add to it. Only the
    // years that would actually receive a NEW booking count: re-importing a
    // workbook that also covers a locked, already-imported year inserts nothing
    // there, and refusing it outright would block the years that are open.
    let receiving: Vec<i32> = sqlx::query_scalar(
        "SELECT DISTINCT ir.period_year::int FROM import_rows ir \
          WHERE ir.batch_id = $1 \
            AND CASE WHEN ir.booked_on IS NULL THEN ir.decision <> 'rejected' \
                     ELSE ir.decision = 'accepted' END \
            AND NOT EXISTS (SELECT 1 FROM bookings b \
                             WHERE b.import_fingerprint = ir.fingerprint)",
    )
    .bind(id)
    .fetch_all(ctx.tenant.conn())
    .await?;
    crate::bookings::assert_years_unlocked(ctx.tenant.conn(), &receiving).await?;

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
    // A reviewed statement line can leave a rule behind, so the same shop is
    // never typed twice. Written before the bookings, so the rule that classifies
    // future imports exists in the same transaction as the ones it came from, and
    // an existing rule is never overwritten — the user's own table wins.
    sqlx::query(
        "INSERT INTO category_rules (id, user_id, pattern, category_id, source) \
         SELECT DISTINCT ON (lower(btrim(ir.comment))) gen_random_uuid(), ir.user_id, \
                btrim(ir.comment), COALESCE(ir.decided_category_id, ir.suggested_category_id), \
                'review' \
           FROM import_rows ir \
          WHERE ir.batch_id = $1 AND ir.create_rule AND ir.decision = 'accepted' \
            AND NOT ir.no_category \
            AND COALESCE(ir.decided_category_id, ir.suggested_category_id) IS NOT NULL \
            AND btrim(ir.comment) <> '' \
          ORDER BY lower(btrim(ir.comment)), ir.source_ref \
         ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .execute(ctx.tenant.conn())
    .await?;

    // A statement line carries a real date and the bank's own two fields, and it
    // is only booked once somebody has accepted it: a row nobody looked at is not
    // a booking, which is the whole point of reviewing a statement by hand. A
    // workbook row has no date and no decision to make, so it books unless it was
    // rejected.
    //
    // Which category a row is booked under, `pick.chosen`, differs between the two
    // in one way that matters. A statement line was looked at with its suggestion
    // on screen and accepted, so an accepted guess IS the user's choice. A
    // workbook row's suggestion is only ever confirmed through the review queue,
    // so only an explicit decision counts there — stated here rather than left to
    // the fact that workbook rows happen not to carry a suggestion today. An exact rule match on either is left to the
    // rule itself, so the booking stays retroactive with its rule. And a statement
    // line marked "no category" is booked with none, rule or no rule.
    let inserted = sqlx::query(
        "INSERT INTO bookings (id, user_id, period_year, period_month, booked_on, kind, \
                               amount_cents, comment, counterparty, purpose, tax_relevant, \
                               category_id, category_source, \
                               resolved_rule_id, origin, import_row_id, import_fingerprint) \
         SELECT gen_random_uuid(), ir.user_id, ir.period_year, ir.period_month, ir.booked_on, \
                ir.kind, ir.amount_cents, ir.comment, ir.counterparty, ir.purpose, \
                ir.tax_relevant, \
                COALESCE(pick.chosen, CASE WHEN ir.no_category THEN NULL ELSE r.category_id END), \
                CASE WHEN pick.chosen IS NOT NULL THEN 'manual' \
                     WHEN NOT ir.no_category AND r.category_id IS NOT NULL THEN 'rule' \
                     ELSE 'unresolved' END, \
                CASE WHEN pick.chosen IS NULL AND NOT ir.no_category THEN r.id END, \
                CASE WHEN ir.booked_on IS NULL THEN 'legacy_month_only' ELSE 'bank_csv' END, \
                ir.id, ir.fingerprint \
           FROM import_rows ir \
           LEFT JOIN category_rules r ON r.match_key = lower(btrim(ir.comment)) \
           CROSS JOIN LATERAL (SELECT CASE \
                  WHEN ir.booked_on IS NULL THEN ir.decided_category_id \
                  WHEN ir.no_category THEN NULL \
                  WHEN ir.decided_category_id IS NOT NULL THEN ir.decided_category_id \
                  WHEN ir.suggestion_kind = 'exact' THEN NULL \
                  ELSE ir.suggested_category_id END AS chosen) pick \
          WHERE ir.batch_id = $1 \
            AND CASE WHEN ir.booked_on IS NULL THEN ir.decision <> 'rejected' \
                     ELSE ir.decision = 'accepted' END \
         ON CONFLICT (user_id, import_fingerprint) WHERE import_fingerprint IS NOT NULL \
         DO NOTHING",
    )
    .bind(id)
    .execute(ctx.tenant.conn())
    .await?
    .rows_affected() as i64;

    // The lines the review asked to send to KitchenOwl as well, now that they are
    // bookings. Queued in this transaction, so the booking and its push intent
    // arrive together or not at all, and checked again against the real booking:
    // the household may have changed since the line was reviewed. A line that was
    // not booked — a fingerprint that was already in the ledger — sends nothing,
    // because there is no booking for it to be linked to.
    let staged = sqlx::query(
        "SELECT b.id AS booking_id, ir.comment, ir.ko_push \
           FROM import_rows ir JOIN bookings b ON b.import_row_id = ir.id \
          WHERE ir.batch_id = $1 AND ir.decision = 'accepted' AND ir.ko_push IS NOT NULL",
    )
    .bind(id)
    .fetch_all(ctx.tenant.conn())
    .await?;
    if !staged.is_empty() && crate::kitchenowl::client::KoClient::from_state(&state).is_none() {
        return Err(AppError::Integration(
            "KitchenOwl ist auf diesem Server nicht konfiguriert".into(),
        ));
    }
    for row in &staged {
        let booking_id: Uuid = row.get("booking_id");
        let comment: Option<String> = row.get("comment");
        let choices: crate::models::KoPushRequest = serde_json::from_value(row.get("ko_push"))
            .map_err(|e| {
                AppError::Internal(anyhow::anyhow!("KitchenOwl-Angaben nicht lesbar: {e}"))
            })?;
        // Which line it was, in the words the review screen showed it under.
        let at_line =
            |message: String| format!("„{}“: {message}", comment.as_deref().unwrap_or_default());
        let payload = crate::kitchenowl::push::build_payload(
            ctx.tenant.conn(),
            booking_id,
            &choices,
            state.config.kitchenowl_push_marker_in_name,
        )
        .await
        .map_err(|e| match e {
            AppError::Validation(m) => AppError::Validation(at_line(m)),
            AppError::Conflict(m) => AppError::Conflict(at_line(m)),
            other => other,
        })?;
        crate::kitchenowl::push::queue(ctx.tenant.conn(), ctx.user.id, booking_id, &payload)
            .await?;
    }
    let ko_queued = staged.len() as i64;

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

    // An import is the one event that invalidates every KitchenOwl suggestion at
    // once: those are statements about two ledgers, and a year of bookings has
    // just appeared in one of them. Without this they keep the empty candidate
    // list they were born with — which is what happened when 459 drafts were
    // written seven minutes before the bookings arrived. In the same transaction,
    // so a rolled-back import cannot leave the suggestions talking about rows that
    // no longer exist. A failure here must not fail the import, which is done —
    // and without the savepoint it did worse than fail it: a database error left
    // the transaction aborted, the COMMIT below quietly became a ROLLBACK, and the
    // response still reported every booking as inserted.
    let threshold = state.config.kitchenowl_duplicate_threshold;
    if let Err(e) = crate::db::savepoint(ctx.tenant.conn(), "rescan_after_import", async |conn| {
        crate::kitchenowl::matching::rescan_open(conn, threshold).await
    })
    .await
    {
        tracing::warn!(error = %e, "Vorschläge konnten nach dem Import nicht neu berechnet werden");
    }

    let user_id = ctx.user.id;
    ctx.tenant.commit().await?;
    // After the commit, as for a single push: the intents are durable, the first
    // attempt is opportunistic.
    if ko_queued > 0 {
        crate::kitchenowl::spawn_push_attempt(&state, user_id);
    }
    Ok(Json(CommitResult {
        inserted,
        skipped: total - inserted,
        years_touched: years.into_iter().map(|y| y as i32).collect(),
        uncategorized_remaining,
        ko_queued,
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
    params(
        ("id" = Uuid, Path, description = "Datensatz-Id"),
        ("status" = Option<String>, Query, description = "open (Vorgabe), skipped, resolved oder all"),
        ("limit" = Option<i64>, Query, description = "Vorgabe 200, höchstens 1000"),
    ),
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

// ------------------------------------------------------- bank statements (CSV)

/// One staged statement line, as the review screen sees it.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatementRowView {
    pub id: Uuid,
    pub source_ref: String,
    pub booked_on: chrono::NaiveDate,
    pub kind: String,
    pub amount_cents: i64,
    pub counterparty: Option<String>,
    pub purpose: Option<String>,
    /// What the comment will be. Starts as a suggestion from the payee and is the
    /// field the review exists to correct.
    pub comment: String,
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    /// Where that category came from: `rule`, `suggestion`, `manual`, or none.
    pub category_source: Option<String>,
    pub suggestion_score: Option<f32>,
    /// `pending`, `accepted` (reviewed, will be booked) or `rejected` (will not).
    pub decision: String,
    pub create_rule: bool,
    pub remember_payee: bool,
    /// The booking this line looks like it already is.
    pub duplicate_booking_id: Option<Uuid>,
    pub duplicate_comment: Option<String>,
    pub duplicate_booked_on: Option<chrono::NaiveDate>,
    /// The KitchenOwl push this line takes with it when it is booked, if any.
    pub ko_push: Option<crate::models::KoPushRequest>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatementRowPage {
    pub items: Vec<StatementRowView>,
    /// Lines matching the current filter — what the pager counts, where `total`
    /// is the whole statement.
    pub matching: i64,
    pub total: i64,
    pub pending: i64,
    pub accepted: i64,
    pub rejected: i64,
    pub duplicates: i64,
}

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatementRowInput {
    pub comment: Option<String>,
    pub category_id: Option<Uuid>,
    pub kind: Option<String>,
    /// `accepted` or `rejected`; omitted leaves the decision where it is.
    pub decision: Option<String>,
    pub create_rule: Option<bool>,
    /// Remember what this payee is called, for every future statement.
    pub remember_payee: Option<bool>,
    /// "No category", said explicitly. `categoryId: null` cannot say it, because
    /// an omitted field is null too and must leave the category alone.
    #[serde(default)]
    pub clear_category: bool,
    /// Send this line to KitchenOwl too, with the push dialogue's choices. Queued
    /// when the import is committed, and only if the line is booked.
    pub ko_push: Option<crate::models::KoPushRequest>,
    /// Do not send it after all.
    #[serde(default)]
    pub clear_ko_push: bool,
}

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct StatementRowQuery {
    /// `pending`, `accepted`, `rejected`, `duplicates` or nothing for all of them.
    pub filter: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

/// Stages a bank statement: one row per line, each with a suggested comment, a
/// suggested category and — the point of the exercise — a flag when the ledger
/// already seems to hold it.
/// The category a statement line suggests, and how sure that suggestion is.
///
/// Asked once when a line is staged and again whenever its comment is renamed, so
/// both give the same answer for the same words. It only ever produces a
/// SUGGESTION: what the user picks is stored apart from it and always wins.
fn suggest_for_line(
    rules: &[KnownRule],
    fuzzy_min: f64,
    comment: &str,
    counterparty: &str,
    purpose: &str,
) -> (Option<Uuid>, Option<&'static str>, Option<f32>) {
    // An exact rule on the comment first — `exact` is the schema's word for it.
    let key = comment.trim().to_lowercase();
    if let Some(rule) = rules.iter().find(|r| r.match_key == key) {
        return (Some(rule.category_id), Some("exact"), Some(1.0));
    }
    // Then the rule table against the payee and the bank's own purpose text as
    // well: `SUPERMARKT` hides inside forty characters of card-terminal noise, and
    // the containment tier is exactly what finds it there.
    let haystack = format!("{comment} {counterparty} {purpose}");
    let (hits, _weak, ambiguous) = suggest::suggest(&haystack, rules, fuzzy_min);
    match hits.first() {
        // Two rules claiming a line with equal strength is a question for the
        // reviewer, not a default to be nudged into place.
        Some(hit) if !ambiguous => (
            Some(hit.category_id),
            Some(match hit.tier {
                "token" => "token",
                "prefix" => "prefix",
                _ => "fuzzy",
            }),
            Some(hit.confidence as f32),
        ),
        _ => (None, None, None),
    }
}

async fn stage_statement(
    ctx: &mut Ctx,
    batch_id: Uuid,
    statement: &crate::bank::Statement,
    fuzzy_min: f64,
) -> Result<()> {
    let rules = known_rules(ctx.tenant.conn()).await?;
    // Bookings this statement has already matched a line to. Two identical
    // charges must not both point at one existing booking: setting "possible
    // duplicates" aside would then reject the genuine second one as well.
    let mut claimed: Vec<Uuid> = Vec::new();
    // How often each line identity has been seen in this file; see below.
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for row in &statement.rows {
        // What this payee is called in the user's own words, if they have ever
        // said so. That answer beats anything derivable from the bank's spelling.
        let remembered: Option<(String, Option<Uuid>)> = sqlx::query_as(
            "SELECT comment, category_id FROM statement_payees \
              WHERE payee_key = lower(btrim($1))",
        )
        .bind(&row.counterparty)
        .fetch_optional(ctx.tenant.conn())
        .await?;

        let comment = match &remembered {
            Some((remembered, _)) => remembered.clone(),
            None => crate::bank::suggest_comment(row),
        };
        let (category_id, source, score) =
            match suggest_for_line(&rules, fuzzy_min, &comment, &row.counterparty, &row.purpose) {
                // Nothing matched. The category this payee carried the last time it
                // was renamed beats nothing, and is marked as the guess it is.
                (None, _, _) => match remembered.as_ref().and_then(|(_, c)| *c) {
                    Some(category_id) => (Some(category_id), Some("history"), Some(0.9)),
                    None => (None, None, None),
                },
                found => found,
            };

        // Does the ledger already hold this? Same direction, same amount, and a
        // date within two days: a card payment is booked by the shop on one day
        // and by the bank on another, and the hand-entered booking carries the
        // first. Two days is deliberately tight — a wider window starts catching
        // the weekly shop at the same supermarket for a similar amount — and it
        // is never acted on automatically anyway: a bank really does charge
        // 3,90 € at the same shop twice in a week.
        let duplicate: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM bookings \
              WHERE kind = $1 AND amount_cents = $2 \
                AND status = 'confirmed' \
                AND (booked_on BETWEEN $3::date - 2 AND $3::date + 2 \
                     OR (booked_on IS NULL \
                         AND period_year = EXTRACT(YEAR FROM $3::date)::smallint \
                         AND period_month = EXTRACT(MONTH FROM $3::date)::smallint)) \
                AND NOT (id = ANY($4)) \
              ORDER BY abs(COALESCE(booked_on, $3::date) - $3::date), created_at \
              LIMIT 1",
        )
        .bind(row.kind)
        .bind(row.amount_cents)
        .bind(row.booked_on)
        .bind(&claimed)
        .fetch_optional(ctx.tenant.conn())
        .await?;
        if let Some(id) = duplicate {
            claimed.push(id);
        }

        // The statement line's own identity. The running balance the bank states
        // after every line tells two identical same-day charges apart — but only
        // when there IS one, and a line without it would share its fingerprint
        // with its twin and be dropped at commit as a duplicate of itself. So the
        // n-th repeat within the file is numbered, the way the workbook path does
        // it. The first occurrence keeps the plain form, so lines already booked
        // from an earlier upload still recognise themselves.
        let identity = format!(
            "csv|{}|{}|{}|{}|{}",
            row.booked_on,
            row.kind,
            row.amount_cents,
            row.counterparty.trim().to_lowercase(),
            row.balance_cents.unwrap_or_default()
        );
        let occurrence = seen.entry(identity.clone()).or_insert(0);
        let fingerprint = {
            let mut hasher = Sha256::new();
            if *occurrence == 0 {
                hasher.update(&identity);
            } else {
                hasher.update(format!("{identity}|{occurrence}"));
            }
            hex::encode(hasher.finalize())
        };
        *occurrence += 1;

        sqlx::query(
            "INSERT INTO import_rows (id, user_id, batch_id, source_ref, raw, period_year, \
                                      period_month, booked_on, kind, amount_cents, comment, \
                                      counterparty, purpose, suggested_category_id, \
                                      suggestion_kind, suggestion_score, duplicate_booking_id, \
                                      status, fingerprint) \
             VALUES ($1,$2,$3,$4,$5,$6::smallint,$7::smallint,$8,$9,$10,$11,$12,$13,$14,$15,$16,\
                     $17,'new',$18) \
             ON CONFLICT (batch_id, source_ref) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(ctx.tenant.user_id())
        .bind(batch_id)
        .bind(&row.source_ref)
        .bind(serde_json::json!({
            "bookedOn": row.booked_on,
            "valueDate": row.value_date,
            "counterparty": row.counterparty,
            "bookingText": row.booking_text,
            "purpose": row.purpose,
            "amountCents": row.amount_cents,
            "balanceCents": row.balance_cents,
        }))
        .bind(
            row.booked_on
                .format("%Y")
                .to_string()
                .parse::<i16>()
                .unwrap_or(0),
        )
        .bind(
            row.booked_on
                .format("%m")
                .to_string()
                .parse::<i16>()
                .unwrap_or(1),
        )
        .bind(row.booked_on)
        .bind(row.kind)
        .bind(row.amount_cents)
        .bind(&comment)
        .bind(&row.counterparty)
        .bind(&row.purpose)
        .bind(category_id)
        .bind(source)
        .bind(score)
        .bind(duplicate)
        .bind(&fingerprint)
        .execute(ctx.tenant.conn())
        .await?;
    }
    Ok(())
}

/// What a staged statement line shows, defined once for the list and for the
/// row returned after an edit.
///
/// The category a line carries is the user's explicit choice if there is one,
/// then the suggestion — unless the user said "no category", which is a choice
/// too and beats both. `categorySource` says which of those it is, because a rule
/// match, a guess and a person's own pick must not look alike on the screen.
const STATEMENT_ROW_SELECT: &str = "\
    SELECT ir.id, ir.source_ref, ir.booked_on, ir.kind, ir.amount_cents, ir.comment, \
           ir.counterparty, ir.purpose, ir.decision, ir.create_rule, ir.remember_payee, \
           CASE WHEN ir.no_category THEN NULL \
                WHEN ir.decided_category_id IS NOT NULL THEN 'manual' \
                WHEN ir.suggestion_kind = 'exact' THEN 'rule' \
                WHEN ir.suggestion_kind IS NULL THEN NULL ELSE 'suggestion' END \
             AS category_source, \
           ir.suggestion_score, ir.duplicate_booking_id, \
           c.id AS category_id, c.name AS category_name, \
           b.comment AS dup_comment, b.booked_on AS dup_booked_on, ir.ko_push \
      FROM import_rows ir \
      LEFT JOIN categories c ON c.id = CASE WHEN ir.no_category THEN NULL \
                              ELSE COALESCE(ir.decided_category_id, ir.suggested_category_id) END \
      LEFT JOIN bookings b ON b.id = ir.duplicate_booking_id";

fn row_to_statement_view(r: &sqlx::postgres::PgRow) -> StatementRowView {
    StatementRowView {
        id: r.get("id"),
        source_ref: r.get("source_ref"),
        booked_on: r.get("booked_on"),
        kind: r.get("kind"),
        amount_cents: r.get("amount_cents"),
        counterparty: r.get("counterparty"),
        purpose: r.get("purpose"),
        comment: r.get::<Option<String>, _>("comment").unwrap_or_default(),
        category_id: r.get("category_id"),
        category_name: r.get("category_name"),
        category_source: r.get("category_source"),
        suggestion_score: r.get("suggestion_score"),
        decision: r.get("decision"),
        create_rule: r.get("create_rule"),
        remember_payee: r.get("remember_payee"),
        duplicate_booking_id: r.get("duplicate_booking_id"),
        duplicate_comment: r.get("dup_comment"),
        duplicate_booked_on: r.get("dup_booked_on"),
        // Written only by this module, from the same type; a row that does not read
        // back is shown without a push rather than failing the whole list.
        ko_push: r
            .get::<Option<serde_json::Value>, _>("ko_push")
            .and_then(|v| serde_json::from_value(v).ok()),
    }
}

/// The staged lines of a statement import, for the review screen.
#[utoipa::path(
    get,
    path = "/api/v1/imports/{id}/statement",
    tag = "imports",
    params(("id" = Uuid, Path, description = "Datensatz-Id"), StatementRowQuery),
    responses((status = 200, description = "Gebuchte Zeilen des Kontoauszugs", body = StatementRowPage)),
)]
pub async fn statement_rows(
    mut ctx: Ctx,
    Path(id): Path<Uuid>,
    Query(q): Query<StatementRowQuery>,
) -> Result<Json<StatementRowPage>> {
    let page = q.page.unwrap_or(0).max(0);
    let page_size = q.page_size.unwrap_or(50).clamp(1, 200);
    let filter = q.filter.unwrap_or_default();

    let counts = sqlx::query(
        "SELECT count(*)::bigint AS total, \
                count(*) FILTER (WHERE decision = 'pending')::bigint AS pending, \
                count(*) FILTER (WHERE decision = 'accepted')::bigint AS accepted, \
                count(*) FILTER (WHERE decision = 'rejected')::bigint AS rejected, \
                count(*) FILTER (WHERE duplicate_booking_id IS NOT NULL)::bigint AS dupes \
           FROM import_rows WHERE batch_id = $1",
    )
    .bind(id)
    .fetch_one(ctx.tenant.conn())
    .await?;

    let rows = sqlx::query(&format!(
        "{STATEMENT_ROW_SELECT} \
          WHERE ir.batch_id = $1 \
            AND ($2 = '' \
                 OR ($2 = 'duplicates' AND ir.duplicate_booking_id IS NOT NULL) \
                 OR ir.decision = $2) \
          ORDER BY ir.booked_on DESC, ir.source_ref \
          LIMIT $3 OFFSET $4"
    ))
    .bind(id)
    .bind(&filter)
    .bind(page_size)
    .bind(page * page_size)
    .fetch_all(ctx.tenant.conn())
    .await?;

    let items = rows.iter().map(row_to_statement_view).collect();
    let matching: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM import_rows ir \
          WHERE ir.batch_id = $1 \
            AND ($2 = '' \
                 OR ($2 = 'duplicates' AND ir.duplicate_booking_id IS NOT NULL) \
                 OR ir.decision = $2)",
    )
    .bind(id)
    .bind(&filter)
    .fetch_one(ctx.tenant.conn())
    .await?;

    let out = StatementRowPage {
        items,
        matching,
        total: counts.get("total"),
        pending: counts.get("pending"),
        accepted: counts.get("accepted"),
        rejected: counts.get("rejected"),
        duplicates: counts.get("dupes"),
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

/// Records one line's review: its comment, its category, and whether it is booked.
#[utoipa::path(
    patch,
    path = "/api/v1/imports/{id}/statement/{rowId}",
    tag = "imports",
    params(("id" = Uuid, Path, description = "Datensatz-Id"), ("rowId" = Uuid, Path, description = "Zeilen-Id")),
    request_body = StatementRowInput,
    responses((status = 200, description = "Zeile aktualisiert", body = StatementRowView), (status = 404, description = "Nicht gefunden", body = crate::error::ErrorBody)),
)]
pub async fn review_statement_row(
    mut ctx: Ctx,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((id, row_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<StatementRowInput>,
) -> Result<Json<StatementRowView>> {
    if let Some(decision) = &body.decision
        && !matches!(decision.as_str(), "pending" | "accepted" | "rejected")
    {
        return Err(AppError::Validation(format!(
            "Unbekannte Entscheidung '{decision}'"
        )));
    }
    if let Some(kind) = &body.kind
        && !matches!(kind.as_str(), "income" | "expense" | "transfer")
    {
        return Err(AppError::Validation(format!("Unbekannte Art '{kind}'")));
    }
    if let Some(comment) = &body.comment
        && comment.trim().is_empty()
    {
        return Err(AppError::Validation(
            "Der Kommentar darf nicht leer sein".into(),
        ));
    }

    // The line as it stands, to tell a rename from a re-send. Accept sends the
    // comment every time; only a comment that actually CHANGED is a rename.
    let current: Option<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT comment, counterparty, purpose FROM import_rows \
          WHERE batch_id = $1 AND id = $2",
    )
    .bind(id)
    .bind(row_id)
    .fetch_optional(ctx.tenant.conn())
    .await?;
    let Some((old_comment, counterparty, purpose)) = current else {
        return Err(AppError::NotFound("Importzeile".into()));
    };
    let new_comment = body.comment.as_deref().map(str::trim);
    let renamed = new_comment.is_some_and(|c| c != old_comment.as_deref().unwrap_or("").trim());

    // A rename is how a person says what a line actually was, so the suggester is
    // asked again with the new words: `tanken` over `VISA TANKSTELLE NORD` picks
    // Auto & Parken by itself. It only ever rewrites the SUGGESTION. What the user
    // picked lives in its own column and is changed by nothing but another pick:
    // before, a rename wrote the rule's category over an explicit choice, and
    // because Accept re-sends the comment, so did every Accept.
    let suggestion = if renamed {
        let rules = known_rules(ctx.tenant.conn()).await?;
        suggest_for_line(
            &rules,
            state.config.import_fuzzy_min_confidence,
            new_comment.unwrap_or_default(),
            counterparty.as_deref().unwrap_or_default(),
            purpose.as_deref().unwrap_or_default(),
        )
    } else {
        (None, None, None)
    };

    sqlx::query(
        "UPDATE import_rows SET \
             comment = COALESCE($3, comment), \
             decided_category_id = CASE WHEN $8 THEN NULL \
                                        ELSE COALESCE($4, decided_category_id) END, \
             no_category = CASE WHEN $8 THEN true \
                                WHEN $4::uuid IS NOT NULL THEN false \
                                ELSE no_category END, \
             suggested_category_id = CASE WHEN $9 THEN $10 ELSE suggested_category_id END, \
             suggestion_kind = CASE WHEN $9 THEN $11 ELSE suggestion_kind END, \
             suggestion_score = CASE WHEN $9 THEN $12 ELSE suggestion_score END, \
             kind = COALESCE($5, kind), \
             decision = COALESCE($6, decision), \
             create_rule = COALESCE($7, create_rule), \
             remember_payee = COALESCE($13, remember_payee) \
           WHERE batch_id = $1 AND id = $2",
    )
    .bind(id)
    .bind(row_id)
    .bind(new_comment.map(str::to_string))
    .bind(body.category_id)
    .bind(body.kind.as_deref())
    .bind(body.decision.as_deref())
    .bind(body.create_rule)
    .bind(body.clear_category)
    .bind(renamed)
    .bind(suggestion.0)
    .bind(suggestion.1)
    .bind(suggestion.2)
    .bind(body.remember_payee)
    .execute(ctx.tenant.conn())
    .await?;

    // The KitchenOwl push this line should take with it. Checked now, against the
    // line as it stands after the edit above, so a split naming somebody who left
    // the household is refused while the dialogue is still open — not at the
    // commit, when the person has moved on. The commit checks it again against the
    // real booking.
    if body.clear_ko_push {
        sqlx::query("UPDATE import_rows SET ko_push = NULL WHERE batch_id = $1 AND id = $2")
            .bind(id)
            .bind(row_id)
            .execute(ctx.tenant.conn())
            .await?;
    } else if let Some(choices) = &body.ko_push {
        if crate::kitchenowl::client::KoClient::from_state(&state).is_none() {
            return Err(AppError::Integration(
                "KitchenOwl ist auf diesem Server nicht konfiguriert".into(),
            ));
        }
        let (comment, amount_cents, kind, booked_on): (
            Option<String>,
            i64,
            String,
            Option<chrono::NaiveDate>,
        ) = sqlx::query_as(
            "SELECT comment, amount_cents, kind, booked_on FROM import_rows \
                  WHERE batch_id = $1 AND id = $2",
        )
        .bind(id)
        .bind(row_id)
        .fetch_one(ctx.tenant.conn())
        .await?;
        let facts = crate::kitchenowl::push::BookingFacts {
            comment: comment.unwrap_or_default(),
            amount_cents,
            kind,
            date: booked_on.ok_or_else(|| {
                AppError::Validation("Nur Kontoauszugszeilen lassen sich mitschicken".into())
            })?,
        };
        // The marker is the booking's, and there is no booking yet; the payload
        // built here is a check and is thrown away.
        crate::kitchenowl::push::payload_for(
            ctx.tenant.conn(),
            Uuid::nil(),
            &facts,
            choices,
            state.config.kitchenowl_push_marker_in_name,
        )
        .await?;
        sqlx::query("UPDATE import_rows SET ko_push = $3 WHERE batch_id = $1 AND id = $2")
            .bind(id)
            .bind(row_id)
            .bind(sqlx::types::Json(choices))
            .execute(ctx.tenant.conn())
            .await?;
    }

    // What a payee is called, remembered under the bank's own spelling — but only
    // for the lines that asked. Renaming a student-union payee with a city and a
    // legal form in its name to `Mensaguthaben` is worth keeping; a payment
    // provider is always the same legal entity and a different purchase every
    // time, and remembering THAT would rename every future line from it to
    // whatever the last one happened to be. Hence a switch per line, off by
    // default. The latest rename wins: a correction is a correction.
    if body.comment.is_some() || body.remember_payee == Some(true) {
        sqlx::query(
            "INSERT INTO statement_payees (id, user_id, payee, comment, category_id) \
             SELECT gen_random_uuid(), ir.user_id, ir.counterparty, btrim(ir.comment), \
                    CASE WHEN ir.no_category THEN NULL \
                         ELSE COALESCE(ir.decided_category_id, ir.suggested_category_id) END \
               FROM import_rows ir \
              WHERE ir.id = $1 AND ir.remember_payee \
                AND btrim(COALESCE(ir.counterparty, '')) <> '' \
                AND btrim(ir.comment) <> '' \
             ON CONFLICT (user_id, payee_key) DO UPDATE \
                SET comment = EXCLUDED.comment, \
                    category_id = COALESCE(EXCLUDED.category_id, statement_payees.category_id), \
                    hits = statement_payees.hits + 1, \
                    updated_at = now()",
        )
        .bind(row_id)
        .execute(ctx.tenant.conn())
        .await?;
    }

    let r = sqlx::query(&format!("{STATEMENT_ROW_SELECT} WHERE ir.id = $1"))
        .bind(row_id)
        .fetch_one(ctx.tenant.conn())
        .await?;
    let view = row_to_statement_view(&r);
    ctx.tenant.commit().await?;
    Ok(Json(view))
}

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkDecisionInput {
    /// Which lines: `duplicates` or `pendingWithCategory`.
    pub scope: String,
    pub decision: String,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkDecisionResult {
    pub affected: i64,
}

/// The two decisions worth making in bulk: drop everything the ledger already has,
/// and accept everything a rule matched outright.
#[utoipa::path(
    post,
    path = "/api/v1/imports/{id}/statement/bulk",
    tag = "imports",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    request_body = BulkDecisionInput,
    responses((status = 200, description = "Entscheidungen übernommen", body = BulkDecisionResult)),
)]
pub async fn bulk_statement_decision(
    mut ctx: Ctx,
    Path(id): Path<Uuid>,
    Json(body): Json<BulkDecisionInput>,
) -> Result<Json<BulkDecisionResult>> {
    if !matches!(body.decision.as_str(), "accepted" | "rejected" | "pending") {
        return Err(AppError::Validation("Unbekannte Entscheidung".into()));
    }
    let affected = match body.scope.as_str() {
        "duplicates" => sqlx::query(
            "UPDATE import_rows SET decision = $2 \
                  WHERE batch_id = $1 AND duplicate_booking_id IS NOT NULL \
                    AND decision = 'pending'",
        )
        .bind(id)
        .bind(&body.decision)
        .execute(ctx.tenant.conn())
        .await?
        .rows_affected(),
        // Only the exact rule matches, never the fuzzy ones: a suggestion the user
        // has not looked at is a guess, and this button would bulk-apply guesses.
        "ruleMatches" => sqlx::query(
            "UPDATE import_rows SET decision = $2 \
                  WHERE batch_id = $1 AND decision = 'pending' \
                    AND duplicate_booking_id IS NULL \
                    AND suggestion_kind = 'exact'",
        )
        .bind(id)
        .bind(&body.decision)
        .execute(ctx.tenant.conn())
        .await?
        .rows_affected(),
        other => {
            return Err(AppError::Validation(format!(
                "Unbekannter Bereich '{other}'"
            )));
        }
    };
    ctx.tenant.commit().await?;
    Ok(Json(BulkDecisionResult {
        affected: affected as i64,
    }))
}

// ---------------------------------------------------- what a payee is called

/// One remembered payee: the bank's spelling, and the name a person gave it.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatementPayee {
    pub id: Uuid,
    /// As the bank writes it. Ground-truth data, never translated or tidied.
    pub payee: String,
    pub comment: String,
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    /// How often this rename has been applied or reconfirmed.
    pub hits: i32,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatementPayeeInput {
    pub comment: Option<String>,
    pub category_id: Option<Uuid>,
    /// `true` clears the category rather than leaving it where it is — `null` in
    /// `categoryId` cannot say "remove it", since an omitted field is also null.
    #[serde(default)]
    pub clear_category: bool,
}

const SELECT_PAYEE: &str = "\
    SELECT p.id, p.payee, p.comment, p.category_id, c.name AS category_name, p.hits, \
           p.updated_at \
      FROM statement_payees p LEFT JOIN categories c ON c.id = p.category_id";

fn row_to_payee(r: &sqlx::postgres::PgRow) -> StatementPayee {
    StatementPayee {
        id: r.get("id"),
        payee: r.get("payee"),
        comment: r.get("comment"),
        category_id: r.get("category_id"),
        category_name: r.get("category_name"),
        hits: r.get("hits"),
        updated_at: r.get("updated_at"),
    }
}

/// Every payee whose name you have taught the app.
#[utoipa::path(
    get,
    path = "/api/v1/statement-payees",
    tag = "imports",
    responses((status = 200, description = "Gemerkte Empfänger", body = Vec<StatementPayee>)),
)]
pub async fn payees(mut ctx: Ctx) -> Result<Json<Vec<StatementPayee>>> {
    let rows = sqlx::query(&format!("{SELECT_PAYEE} ORDER BY lower(p.payee)"))
        .fetch_all(ctx.tenant.conn())
        .await?;
    let out = rows.iter().map(row_to_payee).collect();
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

/// Corrects a remembered name without waiting for the next statement.
#[utoipa::path(
    patch,
    path = "/api/v1/statement-payees/{id}",
    tag = "imports",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    request_body = StatementPayeeInput,
    responses((status = 200, description = "Gemerkter Empfänger", body = StatementPayee), (status = 404, description = "Nicht gefunden", body = crate::error::ErrorBody)),
)]
pub async fn update_payee(
    mut ctx: Ctx,
    Path(id): Path<Uuid>,
    Json(body): Json<StatementPayeeInput>,
) -> Result<Json<StatementPayee>> {
    if let Some(comment) = &body.comment
        && comment.trim().is_empty()
    {
        return Err(AppError::Validation(
            "Der Kommentar darf nicht leer sein".into(),
        ));
    }
    let affected = sqlx::query(
        "UPDATE statement_payees SET comment = COALESCE($2, comment), \
                category_id = CASE WHEN $4 THEN NULL ELSE COALESCE($3, category_id) END, \
                updated_at = now() \
          WHERE id = $1",
    )
    .bind(id)
    .bind(body.comment.as_ref().map(|c| c.trim().to_string()))
    .bind(body.category_id)
    .bind(body.clear_category)
    .execute(ctx.tenant.conn())
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound("Empfänger".into()));
    }

    let row = sqlx::query(&format!("{SELECT_PAYEE} WHERE p.id = $1"))
        .bind(id)
        .fetch_one(ctx.tenant.conn())
        .await?;
    let out = row_to_payee(&row);
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

/// Forgets a payee. The next statement asks about it again, which is the point.
#[utoipa::path(
    delete,
    path = "/api/v1/statement-payees/{id}",
    tag = "imports",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    responses((status = 204, description = "Vergessen"), (status = 404, description = "Nicht gefunden", body = crate::error::ErrorBody)),
)]
pub async fn forget_payee(mut ctx: Ctx, Path(id): Path<Uuid>) -> Result<StatusCode> {
    let affected = sqlx::query("DELETE FROM statement_payees WHERE id = $1")
        .bind(id)
        .execute(ctx.tenant.conn())
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound("Empfänger".into()));
    }
    ctx.tenant.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
