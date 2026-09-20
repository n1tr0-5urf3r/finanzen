//! The bank statement reader.
//!
//! One format so far — ING's `Umsatzanzeige` CSV — but the shape is the shape every
//! German bank exports: a block of metadata, a blank line, a header row, then the
//! lines, semicolon-separated, German numbers, `dd.mm.yyyy` dates.
//!
//! What this file is careful about, each of it visible in a real export:
//!
//! * the file is **Latin-1** more often than not, and the header row itself carries
//!   `Auftraggeber/Empfänger` and `Währung` — so a UTF-8-only reader loses the
//!   column names as well as the payees;
//! * the metadata block is not a table. It has two columns, three columns
//!   (`Saldo;512,40;EUR`) and a free-text paragraph, so the header row is found by
//!   its own first column and everything above it is read as loose key/value pairs;
//! * the amount's SIGN is the direction. There is no income column and no expense
//!   column, and `Buchungstext` (`Lastschrift`, `Gutschrift`) describes the
//!   mechanism, not the direction — a `Lastschrift` can be a refund;
//! * `Betrag` and `Saldo` both carry a currency column, and they are not always the
//!   same currency. Only EUR is accepted, loudly;
//! * a `Verwendungszweck` is wrapped by the exporter mid-word (`Berli n`,
//!   `EC-Aufwertun g`), so it is kept verbatim and never parsed for meaning.

use chrono::NaiveDate;

use crate::{
    error::{AppError, Result},
    locale,
};

/// One line of a statement, before anything is decided about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementRow {
    /// `Buchung` — the day the bank booked it, which is the day the money moved.
    pub booked_on: NaiveDate,
    /// `Wertstellungsdatum`. Kept because it is occasionally the more honest date,
    /// never used as the booking's own.
    pub value_date: Option<NaiveDate>,
    pub counterparty: String,
    /// `Buchungstext`: Lastschrift, Gutschrift, Überweisung, Dauerauftrag…
    pub booking_text: String,
    pub purpose: String,
    /// Always positive; the direction is in `kind`.
    pub amount_cents: i64,
    pub kind: &'static str,
    /// The account's running balance after this line, as the bank states it.
    pub balance_cents: Option<i64>,
    /// `zeile:<n>` of the file, so a figure can be traced back to its line.
    pub source_ref: String,
}

/// What the header block said about the account.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatementMeta {
    pub iban: Option<String>,
    pub account_name: Option<String>,
    pub bank: Option<String>,
    pub period: Option<String>,
    pub balance_cents: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Statement {
    pub meta: StatementMeta,
    pub rows: Vec<StatementRow>,
}

/// Decodes the bytes, whatever the bank felt like writing.
///
/// UTF-8 when it is valid UTF-8, Latin-1 otherwise — in that order, because every
/// Latin-1 byte sequence is *some* string and testing UTF-8 first is the only way
/// round that does not silently mojibake a correct file.
fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.trim_start_matches('\u{feff}').to_string(),
        Err(_) => bytes.iter().map(|b| *b as char).collect(),
    }
}

/// Splits a semicolon-separated line, honouring the quotes some exporters add.
fn split_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ';' if !quoted => out.push(std::mem::take(&mut field)),
            _ => field.push(c),
        }
    }
    out.push(field);
    out.into_iter().map(|f| f.trim().to_string()).collect()
}

fn parse_date(raw: &str) -> Option<NaiveDate> {
    let raw = raw.trim();
    for format in ["%d.%m.%Y", "%d.%m.%y", "%Y-%m-%d"] {
        if let Ok(date) = NaiveDate::parse_from_str(raw, format) {
            return Some(date);
        }
    }
    None
}

/// Is this the row that names the columns?
fn is_header(fields: &[String]) -> bool {
    let first = fields.first().map(|f| f.to_lowercase()).unwrap_or_default();
    first == "buchung" || first == "buchungstag" || first.starts_with("buchungsdatum")
}

fn column(headers: &[String], names: &[&str]) -> Option<usize> {
    headers.iter().position(|h| {
        let h = h.to_lowercase();
        names.iter().any(|n| h.starts_with(n))
    })
}

/// Reads an ING-style `Umsatzanzeige` export.
pub fn read_ing_csv(bytes: &[u8]) -> Result<Statement> {
    let text = decode(bytes);
    let lines: Vec<&str> = text.lines().collect();

    let header_at = lines
        .iter()
        .position(|line| is_header(&split_line(line)))
        .ok_or_else(|| {
            AppError::Validation(
                "Keine Kopfzeile gefunden. Erwartet wird eine Zeile, die mit 'Buchung' beginnt."
                    .into(),
            )
        })?;

    // ── the block above the table ────────────────────────────────────────────
    let mut meta = StatementMeta::default();
    for line in &lines[..header_at] {
        let fields = split_line(line);
        let (Some(key), Some(value)) = (fields.first(), fields.get(1)) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        match key.to_lowercase().as_str() {
            "iban" => meta.iban = Some(value.clone()),
            "kontoname" => meta.account_name = Some(value.clone()),
            "bank" => meta.bank = Some(value.clone()),
            "zeitraum" => meta.period = Some(value.clone()),
            "saldo" => meta.balance_cents = locale::cents_from_de_str(value).ok(),
            _ => {}
        }
    }

    let headers = split_line(lines[header_at]);
    let col_booked = column(&headers, &["buchung"]).unwrap_or(0);
    let col_value = column(&headers, &["wertstellung", "valuta"]);
    let col_party = column(
        &headers,
        &["auftraggeber", "beguenstigter", "begünstigter", "name"],
    );
    let col_text = column(&headers, &["buchungstext", "umsatzart"]);
    let col_purpose = column(&headers, &["verwendungszweck"]);
    let col_balance = column(&headers, &["saldo"]);
    // `Betrag` must be found AFTER `Saldo`: both are followed by a `Währung`
    // column, and a search for "währung" alone would find the wrong one.
    let col_amount = column(&headers, &["betrag", "umsatz"])
        .ok_or_else(|| AppError::Validation("Der Kontoauszug hat keine Spalte 'Betrag'".into()))?;
    let col_currency = headers
        .iter()
        .enumerate()
        .skip(col_amount + 1)
        .find(|(_, h)| {
            h.to_lowercase().starts_with("währung") || h.to_lowercase().starts_with("waehrung")
        })
        .map(|(i, _)| i);

    let mut rows = Vec::new();
    for (offset, line) in lines[header_at + 1..].iter().enumerate() {
        let number = header_at + offset + 2; // 1-based, as a spreadsheet counts
        if line.trim().is_empty() {
            continue;
        }
        let fields = split_line(line);
        let get = |i: Option<usize>| i.and_then(|i| fields.get(i)).cloned().unwrap_or_default();

        let Some(booked_on) = fields.get(col_booked).and_then(|f| parse_date(f)) else {
            // Trailing notes and totals live below the table in some exports and
            // are not statement lines. A line that has no date is one of those.
            continue;
        };

        let raw_amount = get(Some(col_amount));
        if raw_amount.is_empty() {
            continue;
        }
        let signed = locale::cents_from_de_str(&raw_amount).map_err(|_| {
            AppError::Unprocessable(format!("Zeile {number}: '{raw_amount}' ist kein Betrag"))
        })?;
        if signed == 0 {
            continue;
        }
        if let Some(currency) = col_currency.map(|i| get(Some(i)))
            && !currency.is_empty()
            && !currency.eq_ignore_ascii_case("EUR")
        {
            return Err(AppError::Unprocessable(format!(
                "Zeile {number}: Betrag in {currency}. Dieser Import rechnet nicht um."
            )));
        }

        rows.push(StatementRow {
            booked_on,
            value_date: col_value
                .and_then(|i| fields.get(i))
                .and_then(|f| parse_date(f)),
            counterparty: get(col_party),
            booking_text: get(col_text),
            purpose: get(col_purpose),
            amount_cents: signed.abs(),
            // The sign is the direction. `Buchungstext` describes the mechanism —
            // a Lastschrift can be a refund — so it is recorded and not believed.
            kind: if signed > 0 { "income" } else { "expense" },
            balance_cents: col_balance
                .map(|i| get(Some(i)))
                .filter(|v| !v.is_empty())
                .and_then(|v| locale::cents_from_de_str(&v).ok()),
            source_ref: format!("csv!zeile:{number}"),
        });
    }

    if rows.is_empty() {
        return Err(AppError::Unprocessable(
            "Der Kontoauszug enthält keine Umsätze".into(),
        ));
    }

    Ok(Statement { meta, rows })
}

/// A first guess at what a person would have written in the comment column.
///
/// Card payments arrive as `VISA SUPERMARKT SAGT DANKE`, direct debits as the company's
/// full legal name in capitals. Neither is what anybody would type, so the prefix
/// goes, the shouting is softened, and the result lands in the review as a
/// SUGGESTION — every row of a statement is reviewed by hand, and this only has to
/// be a better starting point than an empty field.
pub fn suggest_comment(row: &StatementRow) -> String {
    let mut name = row.counterparty.trim().to_string();
    for prefix in ["VISA ", "Visa ", "PayPal ", "PAYPAL "] {
        if let Some(rest) = name.strip_prefix(prefix) {
            name = rest.trim().to_string();
            break;
        }
    }
    // A name that is one long SHOUT is title-cased; one that already has lower-case
    // letters is somebody's chosen spelling and is left alone.
    if !name.is_empty()
        && name
            .chars()
            .filter(|c| c.is_alphabetic())
            .all(|c| c.is_uppercase())
    {
        name = name
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => {
                        first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                    }
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
    }
    if name.is_empty() {
        name = row.booking_text.trim().to_string();
    }
    // Long enough to identify a shop, short enough to read in a table.
    if name.chars().count() > 60 {
        name = name.chars().take(59).collect::<String>() + "…";
    }
    name
}
