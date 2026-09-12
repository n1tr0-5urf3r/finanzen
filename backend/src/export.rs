//! Exports: the tax receipt list (CSV + PDF), the full account (JSON + CSV), and the
//! restore path that closes the loop.
//!
//! ## Where money is formatted, and why there is an exception
//!
//! Money is `i64` cents everywhere in this application — on the wire, in the engine,
//! in the schema. The two CSV exports and the PDF break that on purpose, because
//! their reader is not a program: it is German Excel and a tax office. `1234,56`
//! opens as a number in a de-DE locale; `123456` opens as a hundred-thousand-euro
//! line item. `format_de` is the single conversion, applied at the very last step.
//!
//! `exports/bookings.json` keeps **cents**, because its reader *is* a program —
//! `POST /exports/restore` — and a round-trip through a formatted decimal is exactly
//! how a cent goes missing.

use axum::{
    Json,
    extract::Query,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use printpdf::{BuiltinFont, Mm, PdfDocument};
use sqlx::Row;
use std::collections::HashMap;
use uuid::Uuid;

use crate::{
    auth::Ctx,
    error::{AppError, Result},
    locale::{format_de, month_name_de},
    models::{
        BookingKind, CategorySource, ExportBooking, ExportCategory, ExportCategoryType,
        ExportDocument, ExportRule, ExportTemplate, ExportYear, Period, RestoreResult,
    },
};

const FORMAT_VERSION: i32 = 1;

// --------------------------------------------------------- content disposition

/// Percent-encodes for an RFC 5987 `filename*` value.
///
/// Everything outside the `attr-char` set is escaped, which is stricter than
/// necessary but never wrong. Without this the umlaut in `Belegübersicht.pdf`
/// either mangles the name on disk or (worse) makes the header invalid and the
/// browser falls back to the URL's last path segment — `export.pdf`.
fn rfc5987(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        let c = *byte as char;
        if c.is_ascii_alphanumeric()
            || matches!(
                c,
                '!' | '#' | '$' | '&' | '+' | '-' | '.' | '^' | '_' | '`' | '|' | '~'
            )
        {
            out.push(c);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `Content-Disposition: attachment` carrying both spellings of the name.
///
/// The plain `filename=` is an ASCII fallback for anything that does not implement
/// RFC 5987; `filename*=UTF-8''…` is what modern browsers actually use. Sending only
/// the second breaks old clients, sending only the first loses the umlauts.
pub fn content_disposition(filename: &str) -> String {
    let ascii: String = filename
        .chars()
        .map(|c| match c {
            'ä' => 'a',
            'ö' => 'o',
            'ü' => 'u',
            'Ä' => 'A',
            'Ö' => 'O',
            'Ü' => 'U',
            'ß' => 's',
            c if c.is_ascii_graphic() || c == ' ' => c,
            _ => '_',
        })
        // A quote or a backslash inside the quoted-string would end it early.
        .filter(|c| *c != '"' && *c != '\\')
        .collect();
    format!(
        "attachment; filename=\"{ascii}\"; filename*=UTF-8''{}",
        rfc5987(filename)
    )
}

fn csv_response(filename: &str, body: String) -> Response {
    // The BOM is not decoration. German Excel opens a UTF-8 CSV as Windows-1252
    // without it, and every umlaut in every category name arrives as mojibake.
    let mut bytes = Vec::with_capacity(body.len() + 3);
    bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    bytes.extend_from_slice(body.as_bytes());
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, content_disposition(filename)),
        ],
        bytes,
    )
        .into_response()
}

/// A `;`-separated writer, because that is what a de-DE Excel expects — a comma is
/// the decimal separator here, so a comma-separated file with German amounts splits
/// every number in half.
fn csv_writer() -> csv::Writer<Vec<u8>> {
    csv::WriterBuilder::new()
        .delimiter(b';')
        .from_writer(Vec::new())
}

fn finish(writer: csv::Writer<Vec<u8>>) -> Result<String> {
    let bytes = writer
        .into_inner()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("CSV: {e}")))?;
    String::from_utf8(bytes).map_err(|e| AppError::Internal(anyhow::anyhow!("CSV: {e}")))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YearQuery {
    pub year: i32,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionalYearQuery {
    pub year: Option<i32>,
}

// ------------------------------------------------------------- tax exports

struct TaxRow {
    index: i64,
    month: u8,
    booked_on: Option<chrono::NaiveDate>,
    comment: String,
    category: String,
    kind: String,
    amount_cents: i64,
    has_receipt: bool,
}

async fn tax_rows(ctx: &mut Ctx, year: i32) -> Result<Vec<TaxRow>> {
    let rows = sqlx::query(
        "SELECT b.period_month, b.booked_on, b.comment, c.name AS category_name, b.kind, \
                b.amount_cents, \
                EXISTS (SELECT 1 FROM receipts r WHERE r.booking_id = b.id) AS has_receipt \
           FROM bookings b LEFT JOIN categories c ON c.id = b.category_id \
          WHERE b.tax_relevant AND b.status = 'confirmed' AND b.period_year = $1::smallint \
          ORDER BY b.period_ord, b.booked_on NULLS LAST, b.created_at, b.comment, b.amount_cents, b.id",
    )
    .bind(year)
    .fetch_all(ctx.tenant.conn())
    .await?;

    Ok(rows
        .iter()
        .enumerate()
        .map(|(i, r)| TaxRow {
            index: i as i64 + 1,
            month: r.get::<i16, _>("period_month") as u8,
            booked_on: r.get("booked_on"),
            comment: r.get("comment"),
            // The same placeholder the JSON report uses, so a row with no category is
            // visible in the tax list rather than an empty cell that reads as a bug.
            category: r
                .get::<Option<String>, _>("category_name")
                .unwrap_or_else(|| "(ohne Kategorie)".into()),
            kind: r.get("kind"),
            amount_cents: r.get("amount_cents"),
            has_receipt: r.get("has_receipt"),
        })
        .collect())
}

/// The printable receipt list.
///
/// Deliberately plain: A4, one table, a running number that matches the CSV and the
/// on-screen report, and a per-category summary at the end. The point is that it can
/// be printed, stapled to a folder of receipts and handed over — not that it is
/// pretty.
///
/// Text goes through printpdf's built-in Helvetica, which is WinAnsi-encoded.
/// Windows-1252 covers every German character including `ß`, `€` and the typographic
/// quotes; anything outside it is dropped by printpdf rather than substituted, which
/// is why the comment column is the user's own German data and not arbitrary input.
#[utoipa::path(
    get,
    path = "/api/v1/tax/export.csv",
    tag = "tax",
    params(("year" = i32, Query, description = "Kalenderjahr")),
    responses((status = 200, description = "Steuerliste als CSV, Beträge deutsch formatiert", content_type = "application/octet-stream")),
)]
pub async fn tax_csv(mut ctx: Ctx, Query(q): Query<YearQuery>) -> Result<Response> {
    let rows = tax_rows(&mut ctx, q.year).await?;
    ctx.tenant.commit().await?;

    let mut w = csv_writer();
    w.write_record([
        "Nr.",
        "Monat",
        "Datum",
        "Kommentar",
        "Kategorie",
        "Einnahmen",
        "Ausgaben",
        "Beleg",
    ])
    .map_err(csv_err)?;

    let (mut income, mut expense) = (0i64, 0i64);
    for row in &rows {
        let is_income = row.kind == "income";
        if is_income {
            income += row.amount_cents;
        } else {
            expense += row.amount_cents;
        }
        w.write_record([
            row.index.to_string(),
            month_name_de(row.month).to_string(),
            row.booked_on
                .map(|d| d.format("%d.%m.%Y").to_string())
                .unwrap_or_default(),
            row.comment.clone(),
            row.category.clone(),
            if is_income {
                format_de(row.amount_cents)
            } else {
                String::new()
            },
            if is_income {
                String::new()
            } else {
                format_de(row.amount_cents)
            },
            if row.has_receipt {
                "ja".into()
            } else {
                "nein".to_string()
            },
        ])
        .map_err(csv_err)?;
    }

    w.write_record([""; 8]).map_err(csv_err)?;
    w.write_record([
        "Summe".to_string(),
        String::new(),
        String::new(),
        format!("{} Buchungen", rows.len()),
        String::new(),
        format_de(income),
        format_de(expense),
        format!(
            "{} mit Beleg",
            rows.iter().filter(|r| r.has_receipt).count()
        ),
    ])
    .map_err(csv_err)?;

    Ok(csv_response(&format!("Steuer_{}.csv", q.year), finish(w)?))
}

fn csv_err(e: csv::Error) -> AppError {
    AppError::Internal(anyhow::anyhow!("CSV: {e}"))
}

/// Helvetica advance widths for the characters an amount is made of, in 1/1000 em.
/// Enough to right-align a money column, which is the one thing that makes a printed
/// figure list readable; anything else on the page is left-aligned and needs no
/// metrics.
fn amount_width_mm(text: &str, size_pt: f32) -> f32 {
    let units: f32 = text
        .chars()
        .map(|c| match c {
            '0'..='9' => 556.0,
            '.' | ',' => 278.0,
            '-' | '\u{2212}' => 333.0,
            ' ' => 278.0,
            _ => 556.0,
        })
        .sum();
    // 1 pt = 25.4/72 mm.
    units / 1000.0 * size_pt * 25.4 / 72.0
}

/// Truncates to fit a column, with an ellipsis so a cut is visible as a cut.
fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut s: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    s.push('…');
    s
}

#[utoipa::path(
    get,
    path = "/api/v1/tax/export.pdf",
    tag = "tax",
    params(("year" = i32, Query, description = "Kalenderjahr")),
    responses((status = 200, description = "Steuerliste als PDF zum Abheften", content_type = "application/octet-stream")),
)]
pub async fn tax_pdf(mut ctx: Ctx, Query(q): Query<YearQuery>) -> Result<Response> {
    let rows = tax_rows(&mut ctx, q.year).await?;
    ctx.tenant.commit().await?;

    let title = format!("Steuerrelevante Buchungen {}", q.year);
    let (doc, page1, layer1) = PdfDocument::new(&title, Mm(210.0), Mm(297.0), "Inhalt");
    let regular = doc
        .add_builtin_font(BuiltinFont::Helvetica)
        .map_err(pdf_err)?;
    let bold = doc
        .add_builtin_font(BuiltinFont::HelveticaBold)
        .map_err(pdf_err)?;

    // Column origins in mm from the left edge. The two money columns are right
    // edges, because amounts are right-aligned.
    const X_NR: f32 = 15.0;
    const X_MONTH: f32 = 24.0;
    const X_COMMENT: f32 = 48.0;
    const X_CATEGORY: f32 = 106.0;
    const X_INCOME_R: f32 = 158.0;
    const X_EXPENSE_R: f32 = 182.0;
    const X_RECEIPT: f32 = 186.0;

    const TOP: f32 = 272.0;
    const BOTTOM: f32 = 24.0;
    const LINE: f32 = 5.2;
    const SIZE: f32 = 8.5;

    let mut layer = doc.get_page(page1).get_layer(layer1);
    let mut y = TOP;
    let mut page_no = 1;

    let header = |layer: &printpdf::PdfLayerReference, y: &mut f32, page_no: usize| {
        layer.use_text(&title, 14.0, Mm(X_NR), Mm(*y), &bold);
        layer.use_text(
            format!("Seite {page_no}"),
            8.0,
            Mm(X_EXPENSE_R - 10.0),
            Mm(*y),
            &regular,
        );
        *y -= 8.0;
        layer.use_text("Nr.", SIZE, Mm(X_NR), Mm(*y), &bold);
        layer.use_text("Monat", SIZE, Mm(X_MONTH), Mm(*y), &bold);
        layer.use_text("Kommentar", SIZE, Mm(X_COMMENT), Mm(*y), &bold);
        layer.use_text("Kategorie", SIZE, Mm(X_CATEGORY), Mm(*y), &bold);
        layer.use_text("Einnahmen", SIZE, Mm(X_INCOME_R - 16.0), Mm(*y), &bold);
        layer.use_text("Ausgaben", SIZE, Mm(X_EXPENSE_R - 24.0), Mm(*y), &bold);
        layer.use_text("Beleg", SIZE, Mm(X_RECEIPT), Mm(*y), &bold);
        *y -= LINE * 1.4;
    };

    header(&layer, &mut y, page_no);

    let (mut income, mut expense, mut with_receipt) = (0i64, 0i64, 0i64);
    let mut by_category: std::collections::BTreeMap<String, (i64, i64, i64)> =
        std::collections::BTreeMap::new();

    for row in &rows {
        if y < BOTTOM {
            let (p, l) = doc.add_page(Mm(210.0), Mm(297.0), "Inhalt");
            layer = doc.get_page(p).get_layer(l);
            y = TOP;
            page_no += 1;
            header(&layer, &mut y, page_no);
        }
        let is_income = row.kind == "income";
        let amount = format_de(row.amount_cents);
        if is_income {
            income += row.amount_cents;
        } else {
            expense += row.amount_cents;
        }
        if row.has_receipt {
            with_receipt += 1;
        }
        let entry = by_category.entry(row.category.clone()).or_insert((0, 0, 0));
        if is_income {
            entry.1 += row.amount_cents;
        } else {
            entry.0 += row.amount_cents;
        }
        entry.2 += 1;

        layer.use_text(row.index.to_string(), SIZE, Mm(X_NR), Mm(y), &regular);
        layer.use_text(
            clip(month_name_de(row.month), 10),
            SIZE,
            Mm(X_MONTH),
            Mm(y),
            &regular,
        );
        layer.use_text(clip(&row.comment, 36), SIZE, Mm(X_COMMENT), Mm(y), &regular);
        layer.use_text(
            clip(&row.category, 24),
            SIZE,
            Mm(X_CATEGORY),
            Mm(y),
            &regular,
        );
        let right = if is_income { X_INCOME_R } else { X_EXPENSE_R };
        layer.use_text(
            &amount,
            SIZE,
            Mm(right - amount_width_mm(&amount, SIZE)),
            Mm(y),
            &regular,
        );
        // Whether a receipt is on file is the single most useful column on a printed
        // tax list: it is the checklist for the folder it gets stapled to.
        layer.use_text(
            if row.has_receipt { "x" } else { "–" },
            SIZE,
            Mm(X_RECEIPT + 3.0),
            Mm(y),
            &regular,
        );
        y -= LINE;
    }

    if y < BOTTOM + 40.0 {
        let (p, l) = doc.add_page(Mm(210.0), Mm(297.0), "Inhalt");
        layer = doc.get_page(p).get_layer(l);
        y = TOP;
        page_no += 1;
        header(&layer, &mut y, page_no);
    }

    y -= LINE;
    layer.use_text(
        format!("{} Buchungen · {} mit Beleg", rows.len(), with_receipt),
        SIZE,
        Mm(X_NR),
        Mm(y),
        &bold,
    );
    for (value, right) in [
        (format_de(income), X_INCOME_R),
        (format_de(expense), X_EXPENSE_R),
    ] {
        layer.use_text(
            &value,
            SIZE,
            Mm(right - amount_width_mm(&value, SIZE)),
            Mm(y),
            &bold,
        );
    }

    y -= LINE * 2.0;
    layer.use_text("Nach Kategorie", SIZE, Mm(X_NR), Mm(y), &bold);
    y -= LINE * 1.3;
    for (name, (exp, inc, n)) in &by_category {
        if y < BOTTOM {
            let (p, l) = doc.add_page(Mm(210.0), Mm(297.0), "Inhalt");
            layer = doc.get_page(p).get_layer(l);
            y = TOP;
            page_no += 1;
            header(&layer, &mut y, page_no);
        }
        layer.use_text(clip(name, 40), SIZE, Mm(X_NR), Mm(y), &regular);
        layer.use_text(format!("{n}"), SIZE, Mm(X_CATEGORY), Mm(y), &regular);
        let inc_s = format_de(*inc);
        let exp_s = format_de(*exp);
        layer.use_text(
            &inc_s,
            SIZE,
            Mm(X_INCOME_R - amount_width_mm(&inc_s, SIZE)),
            Mm(y),
            &regular,
        );
        layer.use_text(
            &exp_s,
            SIZE,
            Mm(X_EXPENSE_R - amount_width_mm(&exp_s, SIZE)),
            Mm(y),
            &regular,
        );
        y -= LINE;
    }

    let bytes = doc.save_to_bytes().map_err(pdf_err)?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                header::CONTENT_DISPOSITION,
                content_disposition(&format!("Belegübersicht_{}.pdf", q.year)),
            ),
        ],
        bytes,
    )
        .into_response())
}

fn pdf_err(e: printpdf::Error) -> AppError {
    AppError::Internal(anyhow::anyhow!("PDF: {e}"))
}

// ------------------------------------------------------------ full export

async fn build_document(ctx: &mut Ctx, year: Option<i32>) -> Result<ExportDocument> {
    let types = sqlx::query(
        "SELECT code, label, sort_order, is_income, is_savings, in_consumption \
           FROM category_types ORDER BY sort_order",
    )
    .fetch_all(ctx.tenant.conn())
    .await?
    .iter()
    .map(|r| ExportCategoryType {
        code: r.get("code"),
        label: r.get("label"),
        sort_order: r.get("sort_order"),
        is_income: r.get("is_income"),
        is_savings: r.get("is_savings"),
        in_consumption: r.get("in_consumption"),
    })
    .collect();

    let categories = sqlx::query(
        "SELECT c.name, t.code AS type_code, c.sort_order, c.archived \
           FROM categories c JOIN category_types t ON t.id = c.type_id \
          ORDER BY t.sort_order, c.sort_order, c.name",
    )
    .fetch_all(ctx.tenant.conn())
    .await?
    .iter()
    .map(|r| ExportCategory {
        name: r.get("name"),
        type_code: r.get("type_code"),
        sort_order: r.get("sort_order"),
        archived: r.get("archived"),
    })
    .collect();

    let rules = sqlx::query(
        "SELECT r.pattern, c.name AS category_name, r.kind_override, r.source \
           FROM category_rules r LEFT JOIN categories c ON c.id = r.category_id \
          ORDER BY r.match_key",
    )
    .fetch_all(ctx.tenant.conn())
    .await?
    .iter()
    .map(|r| ExportRule {
        comment: r.get("pattern"),
        category_name: r.get("category_name"),
        kind_override: r
            .get::<Option<String>, _>("kind_override")
            .and_then(|k| BookingKind::parse(&k)),
        source: r.get("source"),
    })
    .collect();

    let years = sqlx::query(
        "SELECT year, opening_cents, opening_source, tax_locked_at IS NOT NULL AS locked \
           FROM fiscal_years ORDER BY year",
    )
    .fetch_all(ctx.tenant.conn())
    .await?
    .iter()
    .map(|r| ExportYear {
        year: r.get::<i16, _>("year") as i32,
        opening_balance_cents: r.get("opening_cents"),
        opening_source: r.get("opening_source"),
        locked: r.get("locked"),
    })
    .collect();

    let templates = sqlx::query(
        "SELECT r.name, r.comment, r.kind, r.amount_cents, r.amount_is_estimate, \
                c.name AS category_name, r.tax_relevant, r.day_of_month, r.interval_months, \
                r.anchor_ord, r.active_from_ord, r.active_to_ord, r.active, r.sort_order \
           FROM recurring_templates r LEFT JOIN categories c ON c.id = r.category_id \
          ORDER BY r.sort_order, r.name",
    )
    .fetch_all(ctx.tenant.conn())
    .await?
    .iter()
    .map(|r| ExportTemplate {
        name: r.get("name"),
        comment: r.get("comment"),
        kind: BookingKind::parse(r.get::<String, _>("kind").as_str())
            .unwrap_or(BookingKind::Expense),
        amount_cents: r.get("amount_cents"),
        amount_is_estimate: r.get("amount_is_estimate"),
        category_name: r.get("category_name"),
        tax_relevant: r.get("tax_relevant"),
        day_of_month: r.get::<Option<i16>, _>("day_of_month").map(|d| d as u8),
        interval_months: r.get::<i16, _>("interval_months") as u8,
        anchor: Period::from_ord(r.get("anchor_ord")),
        active_from: Period::from_ord(r.get("active_from_ord")),
        active_to: r
            .get::<Option<i32>, _>("active_to_ord")
            .map(Period::from_ord),
        active: r.get("active"),
        sort_order: r.get("sort_order"),
    })
    .collect();

    // Drafts are included. They are part of the account even though they are part of
    // no total, and an export that dropped them would silently lose the recurring
    // items waiting for their real amount.
    let mut sql = String::from(
        "SELECT b.period_year, b.period_month, b.booked_on, b.kind, b.amount_cents, b.comment, \
                b.tax_relevant, c.name AS category_name, b.category_source, b.status, b.origin, \
                b.shared, b.external_source, b.external_id, b.import_fingerprint, \
                rt.name AS template_name, \
                EXISTS (SELECT 1 FROM receipts r WHERE r.booking_id = b.id) AS has_receipt \
           FROM bookings b \
           LEFT JOIN categories c ON c.id = b.category_id \
           LEFT JOIN recurring_templates rt ON rt.id = b.template_id",
    );
    if year.is_some() {
        sql.push_str(" WHERE b.period_year = $1::smallint");
    }
    sql.push_str(" ORDER BY b.period_ord, b.booked_on NULLS LAST, b.created_at, b.comment, b.amount_cents, b.id");

    let mut query = sqlx::query(&sql);
    if let Some(y) = year {
        query = query.bind(y);
    }
    let bookings = query
        .fetch_all(ctx.tenant.conn())
        .await?
        .iter()
        .map(|r| ExportBooking {
            year: r.get::<i16, _>("period_year") as i32,
            month: r.get::<i16, _>("period_month") as u8,
            booked_on: r.get("booked_on"),
            kind: BookingKind::parse(r.get::<String, _>("kind").as_str())
                .unwrap_or(BookingKind::Expense),
            amount_cents: r.get("amount_cents"),
            comment: r.get("comment"),
            tax_relevant: r.get("tax_relevant"),
            category_name: r.get("category_name"),
            category_source: CategorySource::parse(r.get::<String, _>("category_source").as_str()),
            status: r.get("status"),
            origin: r.get("origin"),
            shared: r.get("shared"),
            external_source: r.get("external_source"),
            external_id: r.get("external_id"),
            import_fingerprint: r.get("import_fingerprint"),
            template_name: r.get("template_name"),
            has_receipt: r.get("has_receipt"),
        })
        .collect();

    Ok(ExportDocument {
        format_version: FORMAT_VERSION,
        exported_at: Utc::now(),
        app: "finanzen".into(),
        year,
        category_types: types,
        categories,
        rules,
        years,
        recurring_templates: templates,
        bookings,
    })
}

/// Rebuilds an account from an export document.
///
/// **Refuses a target that already has bookings.** Merge semantics for two ledgers
/// that both claim to be the truth would be an invention — which booking wins, what
/// happens to a rule that maps the same comment elsewhere — and inventing it quietly
/// is how money goes missing. This is a restore, not a sync.
///
/// Categories, rules and templates are matched by NAME, because ids are per-user: a
/// restore into a different account has to resolve them by the only thing both
/// accounts agree on.
#[utoipa::path(
    get,
    path = "/api/v1/exports/bookings.json",
    tag = "exports",
    params(("year" = Option<i32>, Query, description = "Ohne Angabe: das ganze Konto")),
    responses((status = 200, description = "Vollsicherung in ganzen Cent; Eingabe für /exports/restore", content_type = "application/octet-stream")),
)]
pub async fn bookings_json(mut ctx: Ctx, Query(q): Query<OptionalYearQuery>) -> Result<Response> {
    let doc = build_document(&mut ctx, q.year).await?;
    ctx.tenant.commit().await?;
    let filename = match q.year {
        Some(y) => format!("Finanzen-Export_{y}.json"),
        None => "Finanzen-Export.json".to_string(),
    };
    let body = serde_json::to_vec_pretty(&doc)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("JSON: {e}")))?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json".to_string()),
            (header::CONTENT_DISPOSITION, content_disposition(&filename)),
        ],
        body,
    )
        .into_response())
}

#[utoipa::path(
    get,
    path = "/api/v1/exports/bookings.csv",
    tag = "exports",
    params(("year" = Option<i32>, Query, description = "Ohne Angabe: das ganze Konto")),
    responses((status = 200, description = "Buchungen als CSV für Excel", content_type = "application/octet-stream")),
)]
pub async fn bookings_csv(mut ctx: Ctx, Query(q): Query<OptionalYearQuery>) -> Result<Response> {
    let doc = build_document(&mut ctx, q.year).await?;
    ctx.tenant.commit().await?;

    let mut w = csv_writer();
    w.write_record([
        "Nr.",
        "Jahr",
        "Monat",
        "Datum",
        "Art",
        "Kommentar",
        "Kategorie",
        "Einnahmen",
        "Ausgaben",
        "Netto",
        "Steuer",
        "Status",
        "Herkunft",
        "Beleg",
    ])
    .map_err(csv_err)?;

    for (i, b) in doc.bookings.iter().enumerate() {
        let (income, expense, net) = match b.kind {
            BookingKind::Income => (b.amount_cents, 0, -b.amount_cents),
            BookingKind::Expense => (0, b.amount_cents, b.amount_cents),
            // A transfer nets to zero structurally; showing it as a zero rather than
            // omitting the row keeps the CSV a faithful list of what happened.
            BookingKind::Transfer => (0, 0, 0),
        };
        w.write_record([
            (i + 1).to_string(),
            b.year.to_string(),
            month_name_de(b.month).to_string(),
            b.booked_on
                .map(|d| d.format("%d.%m.%Y").to_string())
                .unwrap_or_default(),
            match b.kind {
                BookingKind::Income => "Einnahme",
                BookingKind::Expense => "Ausgabe",
                BookingKind::Transfer => "Umbuchung",
            }
            .to_string(),
            b.comment.clone(),
            b.category_name.clone().unwrap_or_default(),
            if income > 0 {
                format_de(income)
            } else {
                String::new()
            },
            if expense > 0 {
                format_de(expense)
            } else {
                String::new()
            },
            format_de(net),
            if b.tax_relevant {
                "x".into()
            } else {
                String::new()
            },
            b.status.clone(),
            b.origin.clone(),
            if b.has_receipt {
                "ja".into()
            } else {
                String::new()
            },
        ])
        .map_err(csv_err)?;
    }

    let filename = match q.year {
        Some(y) => format!("Buchungen_{y}.csv"),
        None => "Buchungen.csv".to_string(),
    };
    Ok(csv_response(&filename, finish(w)?))
}

// ----------------------------------------------------------------- restore

#[utoipa::path(
    post,
    path = "/api/v1/exports/restore",
    tag = "exports",
    request_body = ExportDocument,
    responses((status = 200, description = "Wiederhergestellt, mit Warnungen", body = RestoreResult), (status = 422, description = "Unbekannte Formatversion", body = crate::error::ErrorBody)),
)]
pub async fn restore(
    mut ctx: Ctx,
    Json(doc): Json<ExportDocument>,
) -> Result<(StatusCode, Json<RestoreResult>)> {
    if doc.format_version != FORMAT_VERSION {
        return Err(AppError::Unprocessable(format!(
            "Unbekannte Exportversion {} (erwartet {FORMAT_VERSION})",
            doc.format_version
        )));
    }
    let existing: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM bookings")
        .fetch_one(ctx.tenant.conn())
        .await?;
    if existing > 0 {
        return Err(AppError::Conflict(
            "Das Konto enthält bereits Buchungen. Ein Import ersetzt kein bestehendes Konto."
                .into(),
        ));
    }

    let mut result = RestoreResult {
        categories_created: 0,
        rules_created: 0,
        years_created: 0,
        templates_created: 0,
        bookings_created: 0,
        rule_links_downgraded: 0,
        warnings: Vec::new(),
    };

    // Types first: a category cannot exist without one (type_id is NOT NULL), which
    // is the schema-level reason the spreadsheet's "type fell out of the lookup
    // range" bug is unrepresentable here.
    for t in &doc.category_types {
        sqlx::query(
            "INSERT INTO category_types (id, user_id, code, label, sort_order, is_income, \
                    is_savings, in_consumption) \
             VALUES ($1,$2,$3,$4,$5::smallint,$6,$7,$8) \
             ON CONFLICT (user_id, code) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(ctx.tenant.user_id())
        .bind(&t.code)
        .bind(&t.label)
        .bind(t.sort_order)
        .bind(t.is_income)
        .bind(t.is_savings)
        .bind(t.in_consumption)
        .execute(ctx.tenant.conn())
        .await?;
    }

    for c in &doc.categories {
        let affected = sqlx::query(
            "INSERT INTO categories (id, user_id, type_id, name, sort_order, archived) \
             SELECT $1, $2, t.id, $3, $4::smallint, $5 FROM category_types t \
              WHERE t.code = $6 \
             ON CONFLICT (user_id, lower(name)) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(ctx.tenant.user_id())
        .bind(&c.name)
        .bind(c.sort_order)
        .bind(c.archived)
        .bind(&c.type_code)
        .execute(ctx.tenant.conn())
        .await?
        .rows_affected();
        result.categories_created += affected as i64;
    }

    let category_ids = load_category_ids(&mut ctx).await?;

    for r in &doc.rules {
        let category_id = r
            .category_name
            .as_ref()
            .and_then(|n| category_ids.get(&n.to_lowercase()).copied());
        if r.category_name.is_some() && category_id.is_none() && r.kind_override.is_none() {
            // `rule_has_effect` would reject it anyway; saying so is better than a
            // 23514 the caller has to decode.
            result.warnings.push(format!(
                "Regel „{}“ übersprungen: Kategorie „{}“ fehlt",
                r.comment,
                r.category_name.clone().unwrap_or_default()
            ));
            continue;
        }
        let affected = sqlx::query(
            "INSERT INTO category_rules (id, user_id, pattern, category_id, kind_override, source) \
             VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT (user_id, match_key) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(ctx.tenant.user_id())
        .bind(&r.comment)
        .bind(category_id)
        .bind(r.kind_override.map(|k| k.as_db()))
        .bind(&r.source)
        .execute(ctx.tenant.conn())
        .await?
        .rows_affected();
        result.rules_created += affected as i64;
    }

    let rule_ids: HashMap<String, Uuid> = sqlx::query("SELECT id, match_key FROM category_rules")
        .fetch_all(ctx.tenant.conn())
        .await?
        .iter()
        .map(|r| (r.get::<String, _>("match_key"), r.get::<Uuid, _>("id")))
        .collect();

    for y in &doc.years {
        let affected = sqlx::query(
            "INSERT INTO fiscal_years (user_id, year, opening_cents, opening_source, tax_locked_at) \
             VALUES ($1,$2::smallint,$3,$4, CASE WHEN $5 THEN now() ELSE NULL END) \
             ON CONFLICT (user_id, year) DO UPDATE \
                SET opening_cents = EXCLUDED.opening_cents, \
                    opening_source = EXCLUDED.opening_source",
        )
        .bind(ctx.tenant.user_id())
        .bind(y.year)
        .bind(y.opening_balance_cents)
        .bind(&y.opening_source)
        .bind(y.locked)
        .execute(ctx.tenant.conn())
        .await?
        .rows_affected();
        // Counts rows written, insert or update alike: the target of a restore is an
        // account with no bookings, so a pre-existing year row is the rare case and
        // overwriting its opening balance is what the caller asked for.
        result.years_created += affected as i64;
    }

    let mut template_ids: HashMap<String, Uuid> = HashMap::new();
    for t in &doc.recurring_templates {
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
        .bind(&t.name)
        .bind(&t.comment)
        .bind(t.kind.as_db())
        .bind(t.amount_cents)
        .bind(t.amount_is_estimate)
        .bind(
            t.category_name
                .as_ref()
                .and_then(|n| category_ids.get(&n.to_lowercase()).copied()),
        )
        .bind(t.tax_relevant)
        .bind(t.day_of_month.map(|d| d as i16))
        .bind(t.interval_months as i16)
        .bind(t.anchor.ord())
        .bind(t.active_from.ord())
        .bind(t.active_to.map(|p| p.ord()))
        .bind(t.active)
        .bind(t.sort_order)
        .execute(ctx.tenant.conn())
        .await
        .map_err(|e| AppError::from_db(e, "Vorlage konnte nicht wiederhergestellt werden"))?;
        template_ids.insert(t.name.clone(), id);
        result.templates_created += 1;
    }

    for b in &doc.bookings {
        let category_id = b
            .category_name
            .as_ref()
            .and_then(|n| category_ids.get(&n.to_lowercase()).copied());

        // `bookings_category_source` and `bookings_rule_link` are CHECKs, not
        // conventions, so the triple (category_id, source, rule_id) has to be made
        // consistent here rather than hoped for.
        let (source, rule_id) = match (category_id, b.category_source) {
            (None, _) => ("unresolved", None),
            (Some(_), CategorySource::Rule) => {
                match rule_ids.get(&b.comment.trim().to_lowercase()) {
                    Some(id) => ("rule", Some(*id)),
                    None => {
                        // The category still holds; only the provenance changes, so
                        // no figure moves. Counted, never silent.
                        result.rule_links_downgraded += 1;
                        ("manual", None)
                    }
                }
            }
            (Some(_), CategorySource::Imported) => ("imported", None),
            (Some(_), _) => ("manual", None),
        };

        sqlx::query(
            "INSERT INTO bookings (id, user_id, period_year, period_month, booked_on, kind, \
                    amount_cents, comment, tax_relevant, category_id, category_source, \
                    resolved_rule_id, status, origin, shared, external_source, external_id, \
                    import_fingerprint, template_id) \
             VALUES ($1,$2,$3::smallint,$4::smallint,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,\
                     $16,$17,$18,$19)",
        )
        .bind(Uuid::new_v4())
        .bind(ctx.tenant.user_id())
        .bind(b.year)
        .bind(b.month as i16)
        .bind(b.booked_on)
        .bind(b.kind.as_db())
        .bind(b.amount_cents)
        .bind(&b.comment)
        .bind(b.tax_relevant)
        .bind(category_id)
        .bind(source)
        .bind(rule_id)
        .bind(&b.status)
        .bind(&b.origin)
        .bind(b.shared)
        .bind(&b.external_source)
        .bind(&b.external_id)
        .bind(&b.import_fingerprint)
        .bind(
            b.template_name
                .as_ref()
                .and_then(|n| template_ids.get(n).copied()),
        )
        .execute(ctx.tenant.conn())
        .await
        .map_err(|e| {
            AppError::from_db(
                e,
                &format!("Buchung „{}“ konnte nicht übernommen werden", b.comment),
            )
        })?;
        result.bookings_created += 1;
    }

    ctx.tenant.commit().await?;
    Ok((StatusCode::CREATED, Json(result)))
}

async fn load_category_ids(ctx: &mut Ctx) -> Result<HashMap<String, Uuid>> {
    Ok(sqlx::query("SELECT id, lower(name) AS key FROM categories")
        .fetch_all(ctx.tenant.conn())
        .await?
        .iter()
        .map(|r| (r.get::<String, _>("key"), r.get::<Uuid, _>("id")))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german_filenames_survive_the_content_disposition_header() {
        let header = content_disposition("Belegübersicht_2026.pdf");
        // The umlaut path everybody breaks: both spellings must be present.
        assert!(
            header.contains("filename=\"Belegubersicht_2026.pdf\""),
            "{header}"
        );
        assert!(
            header.contains("filename*=UTF-8''Beleg%C3%BCbersicht_2026.pdf"),
            "{header}"
        );
        assert!(header.starts_with("attachment;"));
    }

    #[test]
    fn a_filename_cannot_break_out_of_the_quoted_string() {
        let header = content_disposition("a\"; rm -rf /; x=\".csv");
        // The quote is gone from the fallback and percent-encoded in the RFC 5987
        // form, so neither spelling can terminate the parameter early.
        let fallback = header
            .split("filename=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .expect("fallback filename");
        assert!(!fallback.contains('"'));
        assert!(header.contains("%22"));
    }

    #[test]
    fn rfc5987_escapes_everything_that_is_not_an_attr_char() {
        assert_eq!(rfc5987("Steuer_2026.csv"), "Steuer_2026.csv");
        assert_eq!(rfc5987("ü"), "%C3%BC");
        assert_eq!(rfc5987("a b"), "a%20b");
        assert_eq!(rfc5987("a;b"), "a%3Bb");
    }

    #[test]
    fn amounts_are_measured_so_the_money_column_can_be_right_aligned() {
        // Helvetica digits are all 556/1000 em; the separators are narrower. What
        // matters is that a longer number is wider, monotonically.
        let a = amount_width_mm("9,99", 8.5);
        let b = amount_width_mm("40.000,00", 8.5);
        assert!(b > a);
        assert!(a > 0.0 && b < 30.0, "{a} {b}");
    }

    #[test]
    fn clipping_is_visible_as_clipping() {
        assert_eq!(clip("Miete", 10), "Miete");
        assert_eq!(clip("Rückerstattung NetSoft GmbH", 10), "Rückersta…");
        // Multi-byte characters must not be split.
        assert!(clip("äöüäöüäöüäöü", 5).chars().count() <= 5);
    }
}
