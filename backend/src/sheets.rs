//! Workbook readers for the two source files.
//!
//! Both readers deliberately work from the **raw stored value**, never from a display
//! string and never from a cached formula result:
//!
//! * The 2026 xlsx's `Kategorie` / `Typ` / `Netto` columns are formulas whose cached
//!   values are absent in the current file, so categories must be recomputed from the
//!   workbook's own `Kategorien` sheet.
//! * The ODS stores amounts as `office:value-type="currency"` with an `office:value`
//!   attribute; the display text is `"2.529,04 €"`. Reading the text would lose the
//!   value and the sign.
//! * Both carry IEEE-754 artifacts in the raw XML (`67.29000000000001`,
//!   `45171.910000000011`), which is why every amount goes through
//!   [`crate::locale::cents_from_f64`].

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use calamine::{Data, Reader, Xlsx};
use quick_xml::events::Event;

use crate::{
    error::{AppError, Result},
    locale::{self, month_from_de, split_month_label},
};

/// A booking as it appears in a source workbook, before categorisation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetBooking {
    pub source_ref: String,
    pub period_year: i32,
    pub period_month: u8,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub comment: String,
    pub tax_relevant: bool,
    pub manual_category: Option<String>,
    pub raw_amount: String,
}

impl SheetBooking {
    pub fn kind(&self) -> &'static str {
        if self.income_cents > 0 {
            "income"
        } else {
            "expense"
        }
    }
    pub fn amount_cents(&self) -> i64 {
        if self.income_cents > 0 {
            self.income_cents
        } else {
            self.expense_cents
        }
    }
    pub fn net_cents(&self) -> i64 {
        self.expense_cents - self.income_cents
    }
}

/// The workbook's own `Kategorien` sheet: comment -> category, and category -> type.
#[derive(Debug, Clone, Default)]
pub struct SheetTaxonomy {
    /// keyed by `lower(trim(comment))`
    pub rules: BTreeMap<String, String>,
    /// preserves the original casing for display
    pub rule_display: BTreeMap<String, String>,
    pub category_types: BTreeMap<String, String>,
}

// ---------------------------------------------------------------- xlsx (2026)

fn cell_f64(cell: &Data) -> Option<f64> {
    match cell {
        Data::Float(f) => Some(*f),
        Data::Int(i) => Some(*i as f64),
        Data::String(s) if !s.trim().is_empty() => {
            locale::cents_from_de_str(s).ok().map(|c| c as f64 / 100.0)
        }
        _ => None,
    }
}

fn cell_text(cell: &Data) -> Option<String> {
    match cell {
        Data::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Data::Float(f) => Some(f.to_string()),
        Data::Int(i) => Some(i.to_string()),
        _ => None,
    }
}

pub fn read_xlsx_taxonomy(bytes: &[u8]) -> Result<SheetTaxonomy> {
    let mut wb: Xlsx<_> = Xlsx::new(Cursor::new(bytes))
        .map_err(|e| AppError::Validation(format!("Arbeitsmappe nicht lesbar: {e}")))?;
    let range = wb
        .worksheet_range("Kategorien")
        .map_err(|e| AppError::Validation(format!("Blatt 'Kategorien' fehlt: {e}")))?;

    let mut tax = SheetTaxonomy::default();
    // Columns A/B hold comment -> category; columns E/F hold category -> type.
    // Data starts at row 5 (index 4); the header is row 4.
    for (idx, row) in range.rows().enumerate() {
        if idx < 4 {
            continue;
        }
        if let (Some(comment), Some(category)) = (
            row.first().and_then(cell_text),
            row.get(1).and_then(cell_text),
        ) {
            let key = comment.trim().to_lowercase();
            if !key.is_empty() {
                tax.rules.insert(key.clone(), category);
                tax.rule_display.insert(key, comment.trim().to_string());
            }
        }
        if let (Some(category), Some(ctype)) = (
            row.get(4).and_then(cell_text),
            row.get(5).and_then(cell_text),
        ) {
            tax.category_types.insert(category, ctype);
        }
    }
    if tax.rules.is_empty() || tax.category_types.is_empty() {
        return Err(AppError::Validation(
            "Blatt 'Kategorien' enthält keine Regeln oder keine Typen".into(),
        ));
    }
    Ok(tax)
}

pub fn read_xlsx_bookings(bytes: &[u8]) -> Result<Vec<SheetBooking>> {
    let mut wb: Xlsx<_> = Xlsx::new(Cursor::new(bytes))
        .map_err(|e| AppError::Validation(format!("Arbeitsmappe nicht lesbar: {e}")))?;
    let range = wb
        .worksheet_range("Buchungen")
        .map_err(|e| AppError::Validation(format!("Blatt 'Buchungen' fehlt: {e}")))?;

    let mut out = Vec::new();
    for (idx, row) in range.rows().enumerate() {
        if idx == 0 {
            continue; // header
        }
        let sheet_row = idx + 1;
        let monat = row.first().and_then(cell_text);
        let income = row.get(1).and_then(cell_f64);
        let expense = row.get(2).and_then(cell_f64);
        let comment = row.get(3).and_then(cell_text);

        if monat.is_none() && income.is_none() && expense.is_none() && comment.is_none() {
            continue;
        }
        let monat =
            monat.ok_or_else(|| AppError::Validation(format!("Zeile {sheet_row}: Monat fehlt")))?;
        let month = month_from_de(&monat).ok_or_else(|| {
            AppError::Validation(format!("Zeile {sheet_row}: unbekannter Monat '{monat}'"))
        })?;
        let comment = comment
            .ok_or_else(|| AppError::Validation(format!("Zeile {sheet_row}: Kommentar fehlt")))?;

        let income_cents = income.map(locale::cents_from_f64).transpose()?.unwrap_or(0);
        let expense_cents = expense
            .map(locale::cents_from_f64)
            .transpose()?
            .unwrap_or(0);
        if (income_cents > 0) == (expense_cents > 0) {
            return Err(AppError::Validation(format!(
                "Zeile {sheet_row}: genau eine von Einnahmen/Ausgaben muss gesetzt sein"
            )));
        }

        // Column E is the tax marker ("x"); column I is the manual category override.
        let tax_relevant = row
            .get(4)
            .and_then(cell_text)
            .map(|v| v.trim().eq_ignore_ascii_case("x"))
            .unwrap_or(false);
        let manual_category = row.get(8).and_then(cell_text);

        out.push(SheetBooking {
            source_ref: format!("xlsx!Buchungen:{sheet_row}"),
            period_year: 2026,
            period_month: month,
            income_cents,
            expense_cents,
            comment,
            tax_relevant,
            manual_category,
            raw_amount: format!("{:?}", income.or(expense)),
        });
    }
    Ok(out)
}

// ----------------------------------------------------------------- ods (legacy)

#[derive(Debug, Clone)]
struct OdsCell {
    value: Option<f64>,
    text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MonthBlock {
    pub index: usize,
    pub year: i32,
    pub month: u8,
    pub first_row: usize,
    pub last_row: usize,
    pub row_count: usize,
    pub label_source: &'static str,
    pub raw_label: Option<String>,
    pub marker_cents: Option<i64>,
    pub computed_cents: i64,
}

#[derive(Debug, Clone)]
pub struct LegacySheet {
    pub bookings: Vec<SheetBooking>,
    pub blocks: Vec<MonthBlock>,
    pub marker_total_cents: i64,
    pub row_total_cents: i64,
}

/// Flattens an ODS table into rows of cells, honouring `number-columns-repeated` and
/// `number-rows-repeated`.
fn read_ods_table(bytes: &[u8], table_name: &str) -> Result<Vec<Vec<OdsCell>>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| AppError::Validation(format!("ODS nicht lesbar: {e}")))?;
    let mut content = String::new();
    archive
        .by_name("content.xml")
        .map_err(|e| AppError::Validation(format!("content.xml fehlt: {e}")))?
        .read_to_string(&mut content)
        .map_err(|e| AppError::Validation(format!("content.xml nicht lesbar: {e}")))?;

    let mut reader = quick_xml::Reader::from_str(&content);
    reader.config_mut().trim_text(false);

    let mut rows: Vec<Vec<OdsCell>> = Vec::new();
    let mut in_target = false;
    let mut depth_guard = 0usize;

    let mut current_row: Vec<OdsCell> = Vec::new();
    let mut row_repeat = 1usize;
    let mut in_row = false;

    let mut cell_repeat = 1usize;
    let mut cell_value: Option<f64> = None;
    let mut cell_text = String::new();
    let mut in_cell = false;
    let mut in_text = false;

    loop {
        match reader.read_event() {
            Err(e) => return Err(AppError::Validation(format!("ODS XML-Fehler: {e}"))),
            Ok(Event::Eof) => break,
            Ok(ev @ (Event::Start(_) | Event::Empty(_))) => {
                let is_empty = matches!(ev, Event::Empty(_));
                let e = match &ev {
                    Event::Start(e) | Event::Empty(e) => e,
                    _ => unreachable!(),
                };
                let name = e.name();
                let local = String::from_utf8_lossy(name.local_name().as_ref()).to_string();
                match local.as_str() {
                    "table" => {
                        let mut this = String::new();
                        for a in e.attributes().flatten() {
                            if a.key.local_name().as_ref() == b"name" {
                                this = String::from_utf8_lossy(&a.value).to_string();
                            }
                        }
                        if this == table_name {
                            in_target = true;
                            depth_guard = 0;
                        }
                    }
                    "table-row" if in_target => {
                        in_row = true;
                        current_row = Vec::new();
                        row_repeat = 1;
                        for a in e.attributes().flatten() {
                            if a.key.local_name().as_ref() == b"number-rows-repeated" {
                                row_repeat = String::from_utf8_lossy(&a.value).parse().unwrap_or(1);
                            }
                        }
                    }
                    "table-cell" | "covered-table-cell" if in_row => {
                        in_cell = true;
                        cell_repeat = 1;
                        cell_value = None;
                        cell_text.clear();
                        for a in e.attributes().flatten() {
                            let key = a.key.local_name();
                            match key.as_ref() {
                                b"number-columns-repeated" => {
                                    cell_repeat =
                                        String::from_utf8_lossy(&a.value).parse().unwrap_or(1);
                                }
                                b"value" => {
                                    cell_value =
                                        String::from_utf8_lossy(&a.value).parse::<f64>().ok();
                                }
                                _ => {}
                            }
                        }
                    }
                    "p" if in_cell => in_text = true,
                    _ => {}
                }
                // A self-closing <table:table-cell/> is an empty cell. It carries no
                // End event, so it must be committed here or every empty cell would
                // collapse and shift all following columns left.
                if is_empty && in_cell {
                    in_cell = false;
                    let repeat = if cell_repeat > 4096 { 1 } else { cell_repeat };
                    for _ in 0..repeat {
                        current_row.push(OdsCell {
                            value: cell_value,
                            text: None,
                        });
                    }
                }
                if is_empty && local == "table-row" && in_row {
                    in_row = false;
                    let repeat = if row_repeat > 4096 { 1 } else { row_repeat };
                    for _ in 0..repeat {
                        rows.push(Vec::new());
                    }
                }
                depth_guard += 1;
                if depth_guard > 50_000_000 {
                    return Err(AppError::Validation("ODS zu groß".into()));
                }
            }
            Ok(Event::Text(t)) if in_text => {
                cell_text.push_str(&t.unescape().unwrap_or_default());
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let local = String::from_utf8_lossy(name.local_name().as_ref()).to_string();
                match local.as_str() {
                    "p" => in_text = false,
                    "table-cell" | "covered-table-cell" if in_cell => {
                        in_cell = false;
                        let text = cell_text.trim();
                        let cell = OdsCell {
                            value: cell_value,
                            text: if text.is_empty() {
                                None
                            } else {
                                Some(text.to_string())
                            },
                        };
                        // A huge repeat count is the trailing padding of the sheet.
                        let repeat = if cell_repeat > 4096 { 1 } else { cell_repeat };
                        for _ in 0..repeat {
                            current_row.push(cell.clone());
                        }
                    }
                    "table-row" if in_row => {
                        in_row = false;
                        while current_row
                            .last()
                            .is_some_and(|c| c.value.is_none() && c.text.is_none())
                        {
                            current_row.pop();
                        }
                        let repeat = if row_repeat > 4096 { 1 } else { row_repeat };
                        for _ in 0..repeat {
                            rows.push(current_row.clone());
                        }
                    }
                    "table" if in_target => in_target = false,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(rows)
}

/// Decodes the legacy sheet.
///
/// It has no month column. The structure is: the LAST row of a month block carries
/// that month's saldo as a number in column D, and the FIRST row of the next block
/// carries the month NAME as text in column D (column E earlier in the file).
/// Months are then assigned sequentially from `start_year`/`start_month`.
///
/// Guards that matter, each one a real trap in this file:
/// * a label must be one of the twelve German month names — column E also holds
///   decoys such as `"Rückerstattung"` next to an unrelated sum;
/// * the apostrophe in `Mai ‘25` (U+2018) differs from `Juni '25` (U+0027);
/// * if any label disagrees with its inferred position, fail loudly rather than
///   silently shifting every subsequent month.
pub fn read_ods_legacy(
    bytes: &[u8],
    table_name: &str,
    start_year: i32,
    start_month: u8,
) -> Result<LegacySheet> {
    let rows = read_ods_table(bytes, table_name)?;
    let data: Vec<(usize, &Vec<OdsCell>)> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.iter().any(|c| c.value.is_some() || c.text.is_some()))
        .skip(1) // header
        .collect();

    // Column D (index 3) holds either a numeric saldo marker or a month label.
    let marker_positions: Vec<usize> = data
        .iter()
        .enumerate()
        .filter(|(_, (_, r))| r.get(3).and_then(|c| c.value).is_some())
        .map(|(i, _)| i)
        .collect();

    if marker_positions.is_empty() {
        return Err(AppError::Unprocessable(
            "Keine Monatsmarker in Spalte D gefunden".into(),
        ));
    }

    let mut blocks = Vec::new();
    let mut bookings = Vec::new();
    let mut marker_total = 0i64;
    let mut row_total = 0i64;

    let mut start = 0usize;
    let (mut year, mut month) = (start_year, start_month);

    for (block_index, &end) in marker_positions.iter().enumerate() {
        // A label may sit on any row of the block, not only its first.
        let mut raw_label = None;
        for (_, row) in &data[start..=end] {
            for col in [3usize, 4] {
                if let Some(text) = row.get(col).and_then(|c| c.text.as_ref()) {
                    let (head, _) = split_month_label(text);
                    if month_from_de(head).is_some() {
                        raw_label = Some(text.clone());
                        break;
                    }
                }
            }
            if raw_label.is_some() {
                break;
            }
        }

        if let Some(label) = &raw_label {
            let (head, label_year) = split_month_label(label);
            let labelled_month = month_from_de(head).expect("checked above");
            if labelled_month != month || label_year.is_some_and(|y| y as i32 != year) {
                return Err(AppError::Unprocessable(format!(
                    "Monatsbezeichnung '{label}' in Block {block_index} widerspricht der \
                     fortlaufenden Reihenfolge (erwartet {} {year})",
                    locale::month_name_de(month)
                )));
            }
        }

        let mut computed = 0i64;
        for (sheet_row, row) in &data[start..=end] {
            let income = row.first().and_then(|c| c.value);
            let expense = row.get(1).and_then(|c| c.value);
            let comment = row.get(2).and_then(|c| c.text.clone());
            let (Some(comment), true) = (comment, income.is_some() || expense.is_some()) else {
                continue;
            };
            let income_cents = income.map(locale::cents_from_f64).transpose()?.unwrap_or(0);
            let expense_cents = expense
                .map(locale::cents_from_f64)
                .transpose()?
                .unwrap_or(0);
            if (income_cents > 0) == (expense_cents > 0) {
                continue;
            }
            computed += income_cents - expense_cents;
            bookings.push(SheetBooking {
                source_ref: format!("ods!{table_name}:{}", sheet_row + 1),
                period_year: year,
                period_month: month,
                income_cents,
                expense_cents,
                comment: comment.trim().to_string(),
                tax_relevant: false,
                manual_category: None,
                raw_amount: format!("{:?}", income.or(expense)),
            });
        }

        let marker_cents = data[end]
            .1
            .get(3)
            .and_then(|c| c.value)
            .map(locale::cents_from_f64)
            .transpose()?;
        if let Some(m) = marker_cents {
            marker_total += m;
        }
        row_total += computed;

        blocks.push(MonthBlock {
            index: block_index,
            year,
            month,
            first_row: start,
            last_row: end,
            row_count: end - start + 1,
            label_source: if raw_label.is_some() {
                "label"
            } else {
                "inferred"
            },
            raw_label,
            marker_cents,
            computed_cents: computed,
        });

        start = end + 1;
        month += 1;
        if month > 12 {
            month = 1;
            year += 1;
        }
    }

    Ok(LegacySheet {
        bookings,
        blocks,
        marker_total_cents: marker_total,
        row_total_cents: row_total,
    })
}
