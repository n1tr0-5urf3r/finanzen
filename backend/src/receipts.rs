//! Receipts: upload, download, delete.
//!
//! Bytes live on disk under `APP_DATA_DIR/receipts/<user_id>/<uuid>.<ext>`, never in
//! `bytea`. A 200 KB – 2 MB PDF per tax row would triple the backup size and make
//! `pg_dump` unusable as a quick restore path; on disk, backup is `pg_dump` plus one
//! `rsync` of a directory tree.
//!
//! **The user's filename is metadata and nothing else.** It is stored so the download
//! can offer it back, and it is never a path component: the name on disk is a fresh
//! uuid plus an extension derived from the *content type*, so `../../etc/passwd`,
//! `C:\evil.pdf` and a 4 KB name are all simply a string in a column. The directory
//! is keyed on the user id, so even a traversal bug in the download handler could not
//! reach another tenant's tree — and RLS means the handler cannot learn another
//! tenant's `storage_key` to begin with.

use axum::{
    Json,
    extract::{Multipart, Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    AppState,
    auth::Ctx,
    error::{AppError, Result},
    export::content_disposition,
    models::Receipt,
};

/// Accepted types, with the extension each is stored under. Derived from the
/// declared content type rather than from the filename, because the filename is
/// attacker-controlled and the extension ends up in a path.
fn extension_for(content_type: &str) -> Option<&'static str> {
    match content_type.split(';').next().unwrap_or("").trim() {
        "application/pdf" => Some("pdf"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        "image/heic" => Some("heic"),
        "image/heif" => Some("heif"),
        "image/gif" => Some("gif"),
        "image/tiff" => Some("tiff"),
        // Anything else that claims to be an image still gets stored, but under a
        // neutral extension: an unknown image subtype is plausible (a new phone
        // format), an unknown top-level type is not.
        other if other.starts_with("image/") => Some("img"),
        _ => None,
    }
}

/// Reduces a user-supplied filename to something safe to store and echo back.
///
/// Everything before the last separator is dropped, control characters go, and the
/// result is capped. This is display metadata only — the value never reaches the
/// filesystem — but a name that renders as `../../../etc/passwd` in the UI is still a
/// lie about where the file is, and a 40 KB name is still a denial-of-service on
/// every list that renders it.
fn sanitize_filename(raw: &str) -> String {
    let base = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| c == '.' || c.is_whitespace());
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() || cleaned == ".." {
        "Beleg".to_string()
    } else {
        cleaned
    }
}

fn storage_path(state: &AppState, storage_key: &str) -> std::path::PathBuf {
    state.config.data_dir.join(storage_key)
}

fn row_to_receipt(r: &sqlx::postgres::PgRow) -> Receipt {
    Receipt {
        id: r.get("id"),
        booking_id: r.get("booking_id"),
        filename: r.get("filename"),
        content_type: r.get("content_type"),
        byte_size: r.get("byte_size"),
        sha256: r.get("sha256"),
        uploaded_at: r.get("uploaded_at"),
    }
}

const SELECT_RECEIPT: &str = "\
    SELECT id, booking_id, filename, content_type, byte_size, sha256, storage_key, uploaded_at \
      FROM receipts";

#[utoipa::path(
    post,
    path = "/api/v1/bookings/{id}/receipt",
    tag = "receipts",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    responses((status = 201, description = "Beleg gespeichert", body = Receipt), (status = 400, description = "Kein Bild und kein PDF", body = crate::error::ErrorBody)),
)]
pub async fn upload(
    mut ctx: Ctx,
    State(state): State<AppState>,
    Path(booking_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Receipt>)> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM bookings WHERE id = $1)")
        .bind(booking_id)
        .fetch_one(ctx.tenant.conn())
        .await?;
    if !exists {
        // 404, not 403: a 403 would confirm that a booking with this id exists in
        // somebody else's account.
        return Err(AppError::NotFound("Buchung".into()));
    }

    let mut filename = String::new();
    let mut content_type = String::new();
    let mut bytes: Vec<u8> = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Validation(format!("Upload nicht lesbar: {e}")))?
    {
        if field.name() == Some("file") {
            filename = field.file_name().unwrap_or("Beleg").to_string();
            content_type = field.content_type().unwrap_or("").to_string();
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
    let Some(ext) = extension_for(&content_type) else {
        return Err(AppError::Validation(
            "Nur Bilder und PDF-Dateien werden als Beleg akzeptiert".into(),
        ));
    };
    let filename = sanitize_filename(&filename);
    let sha = hex::encode(Sha256::digest(&bytes));

    // `receipts_dedupe` is UNIQUE (user_id, sha256), so the same bytes exist at most
    // once per user. Three cases, all deliberate:
    //
    //   * already on THIS booking  -> no-op, return the existing row. Re-uploading
    //     the same photo is a double tap, not an error.
    //   * on ANOTHER booking       -> 409 naming that booking. Silently moving the
    //     file would leave the first booking without the receipt it was audited with.
    //   * orphaned (its booking was deleted) -> adopt it. The bytes are already on
    //     disk and nothing else claims them.
    if let Some(existing) = sqlx::query(&format!("{SELECT_RECEIPT} WHERE sha256 = $1"))
        .bind(&sha)
        .fetch_optional(ctx.tenant.conn())
        .await?
    {
        let owner: Option<Uuid> = existing.get("booking_id");
        match owner {
            Some(id) if id == booking_id => {
                let out = row_to_receipt(&existing);
                ctx.tenant.commit().await?;
                return Ok((StatusCode::OK, Json(out)));
            }
            Some(other) => {
                let comment: Option<String> =
                    sqlx::query_scalar("SELECT comment FROM bookings WHERE id = $1")
                        .bind(other)
                        .fetch_optional(ctx.tenant.conn())
                        .await?;
                return Err(AppError::Conflict(format!(
                    "Dieser Beleg ist bereits bei der Buchung „{}“ hinterlegt",
                    comment.unwrap_or_else(|| other.to_string())
                )));
            }
            None => {
                sqlx::query("UPDATE receipts SET booking_id = $2 WHERE id = $1")
                    .bind(existing.get::<Uuid, _>("id"))
                    .bind(booking_id)
                    .execute(ctx.tenant.conn())
                    .await?;
                let row = sqlx::query(&format!("{SELECT_RECEIPT} WHERE id = $1"))
                    .bind(existing.get::<Uuid, _>("id"))
                    .fetch_one(ctx.tenant.conn())
                    .await?;
                let out = row_to_receipt(&row);
                ctx.tenant.commit().await?;
                return Ok((StatusCode::OK, Json(out)));
            }
        }
    }

    let id = Uuid::new_v4();
    // The uuid, not the filename, is the name on disk. There is no code path from
    // user input to a path component.
    let storage_key = format!("receipts/{}/{}.{ext}", ctx.tenant.user_id(), id);
    let path = storage_path(&state, &storage_key);
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("Belegordner: {e}")))?;
    }
    tokio::fs::write(&path, &bytes).await.map_err(|e| {
        AppError::Internal(anyhow::anyhow!("Beleg konnte nicht abgelegt werden: {e}"))
    })?;

    let insert = sqlx::query(
        "INSERT INTO receipts (id, user_id, booking_id, filename, content_type, byte_size, \
                sha256, storage_key) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(id)
    .bind(ctx.tenant.user_id())
    .bind(booking_id)
    .bind(&filename)
    .bind(content_type.split(';').next().unwrap_or("").trim())
    .bind(bytes.len() as i64)
    .bind(&sha)
    .bind(&storage_key)
    .execute(ctx.tenant.conn())
    .await;

    if let Err(e) = insert {
        // The row is the record of truth; an unreferenced file would be invisible
        // litter, so the write is undone before the error goes out.
        let _ = tokio::fs::remove_file(&path).await;
        return Err(AppError::from_db(
            e,
            "Beleg konnte nicht gespeichert werden",
        ));
    }

    let row = sqlx::query(&format!("{SELECT_RECEIPT} WHERE id = $1"))
        .bind(id)
        .fetch_one(ctx.tenant.conn())
        .await?;
    let out = row_to_receipt(&row);
    ctx.tenant.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}

#[utoipa::path(
    get,
    path = "/api/v1/bookings/{id}/receipt",
    tag = "receipts",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    responses((status = 200, description = "Der Beleg", content_type = "application/octet-stream"), (status = 404, description = "Kein Beleg vorhanden", body = crate::error::ErrorBody)),
)]
pub async fn download(
    mut ctx: Ctx,
    State(state): State<AppState>,
    Path(booking_id): Path<Uuid>,
) -> Result<Response> {
    let row = sqlx::query(&format!(
        "{SELECT_RECEIPT} WHERE booking_id = $1 ORDER BY uploaded_at DESC LIMIT 1"
    ))
    .bind(booking_id)
    .fetch_optional(ctx.tenant.conn())
    .await?
    .ok_or_else(|| AppError::NotFound("Beleg".into()))?;

    let storage_key: String = row.get("storage_key");
    let filename: String = row.get("filename");
    let content_type: String = row.get("content_type");

    // Defence in depth. `storage_key` is generated here and can only be a uuid under
    // the tenant's own directory, but the check costs nothing and makes the property
    // testable rather than merely argued.
    let expected_prefix = format!("receipts/{}/", ctx.tenant.user_id());
    ctx.tenant.commit().await?;
    if !storage_key.starts_with(&expected_prefix) || storage_key.contains("..") {
        return Err(AppError::NotFound("Beleg".into()));
    }

    let bytes = tokio::fs::read(storage_path(&state, &storage_key))
        .await
        .map_err(|_| AppError::NotFound("Beleg".into()))?;

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, content_disposition(&filename)),
            // Served as an attachment from the app's own origin, so sniffing must
            // not be allowed to turn an "image" into something scriptable.
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        bytes,
    )
        .into_response())
}

#[utoipa::path(
    delete,
    path = "/api/v1/bookings/{id}/receipt",
    tag = "receipts",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    responses((status = 204, description = "Beleg entfernt"), (status = 404, description = "Kein Beleg vorhanden", body = crate::error::ErrorBody)),
)]
pub async fn delete(
    mut ctx: Ctx,
    State(state): State<AppState>,
    Path(booking_id): Path<Uuid>,
) -> Result<StatusCode> {
    let row = sqlx::query(&format!("{SELECT_RECEIPT} WHERE booking_id = $1"))
        .bind(booking_id)
        .fetch_optional(ctx.tenant.conn())
        .await?
        .ok_or_else(|| AppError::NotFound("Beleg".into()))?;
    let storage_key: String = row.get("storage_key");

    sqlx::query("DELETE FROM receipts WHERE id = $1")
        .bind(row.get::<Uuid, _>("id"))
        .execute(ctx.tenant.conn())
        .await?;
    ctx.tenant.commit().await?;

    // The row goes first and the file second: a row pointing at a missing file is a
    // broken download, while a file with no row is invisible litter the backup can
    // absorb. Failing to unlink is therefore logged, not surfaced.
    if let Err(e) = tokio::fs::remove_file(storage_path(&state, &storage_key)).await {
        tracing::warn!(error = %e, key = %storage_key, "Belegdatei konnte nicht gelöscht werden");
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filename_can_never_become_a_path() {
        // Every one of these is a real attempt seen in the wild. What matters is not
        // that they are rejected — they are accepted, as *names* — but that nothing
        // derived from them ever reaches the filesystem.
        assert_eq!(sanitize_filename("../../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("..\\..\\windows\\system32"), "system32");
        assert_eq!(sanitize_filename("/absolute/path/beleg.pdf"), "beleg.pdf");
        assert_eq!(sanitize_filename(".."), "Beleg");
        assert_eq!(sanitize_filename("   "), "Beleg");
        assert_eq!(sanitize_filename("\u{0}\u{1}"), "Beleg");
        // German names survive intact — that is the whole reason for RFC 5987.
        assert_eq!(
            sanitize_filename("Belegübersicht.pdf"),
            "Belegübersicht.pdf"
        );
        assert!(!sanitize_filename(&"a".repeat(500)).is_empty());
        assert!(sanitize_filename(&"a".repeat(500)).len() <= 120);
    }

    #[test]
    fn only_images_and_pdfs_are_accepted_and_the_extension_comes_from_the_type() {
        assert_eq!(extension_for("application/pdf"), Some("pdf"));
        assert_eq!(extension_for("image/jpeg"), Some("jpg"));
        assert_eq!(extension_for("image/png; charset=binary"), Some("png"));
        // A format no one has heard of yet is still an image.
        assert_eq!(extension_for("image/avif"), Some("img"));
        // These are the ones that matter.
        assert_eq!(extension_for("text/html"), None);
        assert_eq!(extension_for("application/octet-stream"), None);
        assert_eq!(extension_for("application/x-sh"), None);
        assert_eq!(extension_for(""), None);
    }
}
