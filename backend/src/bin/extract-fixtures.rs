//! Reads the two source workbooks once and writes JSON fixtures the test suite runs
//! against, so CI never depends on binary spreadsheets.
//!
//!     cargo run --bin extract-fixtures -- <xlsx> <ods> <out-dir>

use std::{collections::BTreeMap, fs, path::PathBuf};

use finanzen::sheets;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        anyhow::bail!("usage: extract-fixtures <xlsx> <ods> <out-dir>");
    }
    let (xlsx_path, ods_path, out_dir) = (&args[1], &args[2], PathBuf::from(&args[3]));
    fs::create_dir_all(&out_dir)?;

    let xlsx = fs::read(xlsx_path)?;
    let ods = fs::read(ods_path)?;

    let taxonomy = sheets::read_xlsx_taxonomy(&xlsx)?;
    let bookings = sheets::read_xlsx_bookings(&xlsx)?;
    let legacy = sheets::read_ods_legacy(&ods, "2023-2025", 2023, 6)?;

    let rules: BTreeMap<&String, &String> = taxonomy.rules.iter().collect();
    write(&out_dir, "rules.json", &rules)?;
    write(&out_dir, "taxonomy.json", &taxonomy.category_types)?;
    write(
        &out_dir,
        "golden_2026.json",
        &bookings
            .iter()
            .map(|b| {
                serde_json::json!({
                    "sourceRef": b.source_ref, "year": b.period_year, "month": b.period_month,
                    "kind": b.kind(), "amountCents": b.amount_cents(), "comment": b.comment,
                    "taxRelevant": b.tax_relevant, "manualCategory": b.manual_category,
                })
            })
            .collect::<Vec<_>>(),
    )?;
    write(
        &out_dir,
        "golden_legacy.json",
        &serde_json::json!({
            "bookings": legacy.bookings.iter().map(|b| serde_json::json!({
                "sourceRef": b.source_ref, "year": b.period_year, "month": b.period_month,
                "kind": b.kind(), "amountCents": b.amount_cents(), "comment": b.comment,
            })).collect::<Vec<_>>(),
            "blocks": legacy.blocks.iter().map(|b| serde_json::json!({
                "index": b.index, "year": b.year, "month": b.month, "rowCount": b.row_count,
                "labelSource": b.label_source, "rawLabel": b.raw_label,
                "markerCents": b.marker_cents, "computedCents": b.computed_cents,
            })).collect::<Vec<_>>(),
            "markerTotalCents": legacy.marker_total_cents,
            "rowTotalCents": legacy.row_total_cents,
        }),
    )?;

    println!(
        "2026: {} bookings, {} rules, {} categories | legacy: {} bookings, {} blocks",
        bookings.len(),
        taxonomy.rules.len(),
        taxonomy.category_types.len(),
        legacy.bookings.len(),
        legacy.blocks.len()
    );
    Ok(())
}

fn write<T: serde::Serialize>(dir: &PathBuf, name: &str, value: &T) -> anyhow::Result<()> {
    fs::write(dir.join(name), serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
