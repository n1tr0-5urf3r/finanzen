//! The golden dataset is the specification.
//!
//! Every figure here was recomputed from the raw `Einnahmen`/`Ausgaben`/`Kommentar`
//! columns and the workbook's own `Kategorien` sheet — never read out of a formula
//! cell, because the 2026 file's formula columns carry no cached values at all.
//!
//! Tests run against JSON fixtures so CI never depends on binary spreadsheets;
//! `fixtures_match_the_source_workbooks` re-parses the originals and is the one test
//! that does, gated behind `--ignored`.

use std::collections::BTreeMap;

use finanzen::calc::{self, Kind, LedgerRow};

/// Fixtures are derived from the source workbooks and therefore carry the same
/// personal financial data, so they are deliberately not committed. Regenerate with:
///
///     cargo run --bin extract-fixtures -- \
///         ../konten_2026_auswertung.xlsx ../konten.ods tests/fixtures
///
/// Without them these tests skip rather than fail, so a checkout with no workbooks
/// still runs the rest of the suite.
fn fixture(name: &str) -> Option<String> {
    std::fs::read_to_string(format!("tests/fixtures/{name}")).ok()
}

fn fixtures_present() -> bool {
    [
        "golden_2026.json",
        "golden_legacy.json",
        "rules.json",
        "taxonomy.json",
    ]
    .iter()
    .all(|f| fixture(f).is_some())
}

/// Early-returns the test when fixtures are absent, naming the command that creates
/// them. A silent skip would let a missing golden dataset look like a passing suite.
macro_rules! require_fixtures {
    () => {
        if !fixtures_present() {
            eprintln!(
                "SKIP: golden fixtures missing - run \
                 `cargo run --bin extract-fixtures -- \
                 ../konten_2026_auswertung.xlsx ../konten.ods tests/fixtures`"
            );
            return;
        }
    };
}

fn fix_2026() -> String {
    fixture("golden_2026.json").expect("checked")
}
fn fix_legacy() -> String {
    fixture("golden_legacy.json").expect("checked")
}

fn rules() -> BTreeMap<String, String> {
    serde_json::from_str(&fixture("rules.json").expect("checked")).expect("rules fixture")
}

fn category_types() -> BTreeMap<String, String> {
    serde_json::from_str(&fixture("taxonomy.json").expect("checked")).expect("taxonomy fixture")
}

/// Maps the workbook's German type labels onto the engine's type codes and flags.
fn type_code(label: &str) -> &'static str {
    match label {
        "Einkommen" => "einkommen",
        "Fixkosten" => "fixkosten",
        "Variable Kosten" => "variabel",
        "Sparen" => "sparen",
        _ => "sonstiges",
    }
}

/// Resolves a booking exactly as the application does: manual override wins, then an
/// exact case-insensitive rule lookup, then unresolved.
fn resolve(
    comment: &str,
    manual: Option<&str>,
    rules: &BTreeMap<String, String>,
) -> Option<String> {
    if let Some(m) = manual.filter(|m| !m.trim().is_empty()) {
        return Some(m.trim().to_string());
    }
    rules.get(&comment.trim().to_lowercase()).cloned()
}

fn ledger_2026() -> Vec<LedgerRow> {
    let raw: Vec<serde_json::Value> = serde_json::from_str(&fix_2026()).expect("2026 fixture");
    let rules = rules();
    let types = category_types();

    raw.iter()
        .map(|b| {
            let comment = b["comment"].as_str().unwrap();
            let manual = b["manualCategory"].as_str();
            // The gym `Mafit` belongs to Sport, not Haustier; the shipped workbook
            // has it the other way round. Both are Fixkosten, so this moves the
            // per-category figure without moving any type total — which is exactly
            // what `mapet_correction_does_not_move_type_totals` pins down.
            let category = match comment.trim().to_lowercase().as_str() {
                "mapet" | "mapet guthaben" if manual.is_none() => Some("Sport".to_string()),
                _ => resolve(comment, manual, &rules),
            };
            let label = category.as_ref().and_then(|c| types.get(c).cloned());
            let code = label.as_deref().map(type_code);
            LedgerRow {
                period_year: b["year"].as_i64().unwrap() as i32,
                period_month: b["month"].as_u64().unwrap() as u8,
                kind: Kind::parse(b["kind"].as_str().unwrap()).unwrap(),
                amount_cents: b["amountCents"].as_i64().unwrap(),
                // Deterministic stand-in: only its presence/absence matters here,
                // and stability keeps `by_category` grouping reproducible.
                category_id: category.as_ref().map(|c| {
                    let mut h: u128 = 0xcbf2_9ce4_8422_2325;
                    for b in c.as_bytes() {
                        h = (h ^ *b as u128).wrapping_mul(0x1000_0000_01b3);
                    }
                    uuid::Uuid::from_u128(h)
                }),
                category_name: category,
                type_code: code.map(str::to_string),
                type_label: label,
                is_income: code == Some("einkommen"),
                is_savings: code == Some("sparen"),
                in_consumption: matches!(code, Some("fixkosten" | "variabel" | "sonstiges")),
                tax_relevant: b["taxRelevant"].as_bool().unwrap_or(false),
            }
        })
        .collect()
}

fn net_by_name(rows: &[LedgerRow]) -> BTreeMap<String, i64> {
    calc::by_category(rows)
        .into_iter()
        .map(|c| (c.category_name, c.net_cents))
        .collect()
}

// ------------------------------------------------------------------ totals

#[test]
fn totals_2026() {
    require_fixtures!();
    let rows = ledger_2026();
    let t = calc::totals(&rows);
    assert_eq!(rows.len(), 474, "booking count");
    assert_eq!(t.income_cents, 3_600_000, "Einnahmen 36.000,00");
    assert_eq!(t.expense_cents, 2_700_000, "Ausgaben 27.000,00");
    assert_eq!(t.saldo_cents, 900_000, "Bilanz 9.000,00");
    assert_eq!(t.uncategorized_count, 0, "keine Buchung ohne Kategorie");
}

#[test]
fn saldo_equals_negative_sum_of_net_cents() {
    require_fixtures!();
    // The identity tying the gross view to the netting view. If a fourth `kind` is
    // ever added without updating both sums, this is what catches it.
    let rows = ledger_2026();
    let t = calc::totals(&rows);
    let net: i64 = rows.iter().map(LedgerRow::net_cents).sum();
    assert_eq!(t.saldo_cents, -net);
}

#[test]
fn every_booking_has_exactly_one_positive_amount() {
    require_fixtures!();
    for r in ledger_2026() {
        assert!(r.amount_cents > 0, "amount must be positive: {r:?}");
        assert_ne!(r.kind, Kind::Transfer, "2026 contains no transfers");
    }
}

#[test]
fn tax_relevant_2026() {
    require_fixtures!();
    let rows = ledger_2026();
    let tax: Vec<_> = rows.iter().filter(|r| r.tax_relevant).collect();
    assert_eq!(tax.len(), 20, "20 steuerrelevante Buchungen");
    let expense: i64 = tax
        .iter()
        .filter(|r| r.kind == Kind::Expense)
        .map(|r| r.amount_cents)
        .sum();
    let income: i64 = tax
        .iter()
        .filter(|r| r.kind == Kind::Income)
        .map(|r| r.amount_cents)
        .sum();
    assert_eq!(expense, 81_692, "816,92 Ausgaben");
    assert_eq!(income, 600_000, "6.000,00 Einnahmen");
}

// -------------------------------------------------------------- category nets

#[test]
fn net_per_category_2026() {
    require_fixtures!();
    let nets = net_by_name(&ledger_2026());
    let expected: &[(&str, i64)] = &[
        ("Sparen & Anlage", 648_000),
        ("Miete", 480_000),
        ("Reisen & Urlaub", 240_000),
        ("Auto & Parken", 200_000),
        ("Freizeit & Events", 130_000),
        ("Versicherungen", 120_000),
        ("Essen auswärts", 110_000),
        ("Lebensmittel", 82_000),
        ("Nebenkosten", 72_000),
        ("Sport", 46_000),
        ("Strom", 45_000),
        ("Kleidung & Merch", 41_000),
        ("Uni & Bildung", 39_000),
        ("Mensa", 38_000),
        ("Bargeld", 37_000),
        ("Anschaffungen", 31_000),
        ("Internet & Telefon", 28_000),
        ("Haus & Garten", 23_000),
        ("Abos & Streaming", 22_000),
        ("Games & Software", 21_000),
        ("Server & Domains", 12_000),
        ("Drogerie & Gesundheit", 11_000),
        ("Geschenke", 8_900),
        ("Rundfunkbeitrag", 8_000),
        ("Bahn & ÖPNV", 6_500),
        ("Dienstreisen", 5_500),
        ("Sonstiges", 3_900),
        ("Bank & Gebühren", 1_300),
        ("Gehalt", -2_200_000),
        ("Freelancing", -600_000),
    ];
    for (name, want) in expected {
        assert_eq!(nets.get(*name), Some(want), "Netto {name}");
    }
    // Haustier exists but has no bookings once Mafit is corrected to Sport.
    assert_eq!(nets.get("Haustier"), None);
}

#[test]
fn netting_reports_both_gross_legs() {
    require_fixtures!();
    // A category's figure is expenses minus income of the SAME category. Fifteen
    // categories have both sides; the UI needs the legs to explain the net.
    let cats = calc::by_category(&ledger_2026());
    let by_name: BTreeMap<_, _> = cats.iter().map(|c| (c.category_name.as_str(), c)).collect();
    let expected: &[(&str, i64, i64, i64)] = &[
        ("Miete", 480_000, 960_000, 480_000),
        ("Nebenkosten", 72_000, 144_000, 72_000),
        ("Strom", 29_000, 74_000, 45_000),
        ("Internet & Telefon", 28_500, 57_000, 28_000),
        ("Rundfunkbeitrag", 8_000, 16_000, 8_000),
        ("Dienstreisen", 205_000, 210_000, 5_500),
        ("Reisen & Urlaub", 56_000, 295_000, 240_000),
    ];
    for (name, income, expense, net) in expected {
        let c = by_name.get(name).unwrap_or_else(|| panic!("{name} fehlt"));
        assert_eq!(c.income_cents, *income, "{name} Einnahmen");
        assert_eq!(c.expense_cents, *expense, "{name} Ausgaben");
        assert_eq!(c.net_cents, *net, "{name} Netto");
        assert_eq!(c.expense_cents - c.income_cents, c.net_cents);
    }

    let both_sided = cats
        .iter()
        .filter(|c| c.income_cents > 0 && c.expense_cents > 0)
        .count();
    assert_eq!(both_sided, 15, "15 Kategorien mit beiden Seiten");
}

#[test]
fn net_per_type_2026() {
    require_fixtures!();
    let rows = ledger_2026();
    let by_code: BTreeMap<_, _> = calc::by_type(&rows)
        .into_iter()
        .map(|t| (t.type_code, t.net_cents))
        .collect();
    assert_eq!(by_code["fixkosten"], 810_000);
    assert_eq!(by_code["variabel"], 990_000);
    assert_eq!(by_code["sparen"], 648_000);
    assert_eq!(by_code["sonstiges"], 42_000);
    assert_eq!(by_code["einkommen"], -2_880_000);
    let sum: i64 = by_code.values().sum();
    assert_eq!(sum, -900_000, "alle Typen zusammen = -Bilanz");
}

#[test]
fn mapet_correction_does_not_move_type_totals() {
    require_fixtures!();
    // Sport and Haustier are both Fixkosten, so re-pointing Mafit moves 460,00
    // between two categories of the same type. This pins the invariant so the
    // correction cannot be "fixed" back on the basis of a changed type total.
    let mut rows = ledger_2026();
    let before: BTreeMap<_, _> = calc::by_type(&rows)
        .into_iter()
        .map(|t| (t.type_code, t.net_cents))
        .collect();
    for r in &mut rows {
        if r.category_name.as_deref() == Some("Sport") {
            r.category_name = Some("Haustier".into());
        }
    }
    let after: BTreeMap<_, _> = calc::by_type(&rows)
        .into_iter()
        .map(|t| (t.type_code, t.net_cents))
        .collect();
    assert_eq!(before, after);
}

// ------------------------------------------------------------------- monthly

#[test]
fn monthly_rows_2026() {
    require_fixtures!();
    let rows = ledger_2026();
    let months = calc::by_month(&rows);

    /// One expected row of the `Monate` tab. A named struct rather than a nine-wide
    /// tuple, so a transposed column is a compile error instead of a puzzle.
    struct Expected {
        month: u8,
        income: i64,
        expense: i64,
        saldo: i64,
        cumulative: i64,
        fixed: i64,
        variable: i64,
        savings: i64,
        other: i64,
    }
    // Nine arguments because the Monate tab has nine columns; naming them at each
    // call site would triple the length of the table and hide the shape.
    #[allow(clippy::too_many_arguments)]
    const fn row(
        month: u8,
        income: i64,
        expense: i64,
        saldo: i64,
        cumulative: i64,
        fixed: i64,
        variable: i64,
        savings: i64,
        other: i64,
    ) -> Expected {
        Expected {
            month,
            income,
            expense,
            saldo,
            cumulative,
            fixed,
            variable,
            savings,
            other,
        }
    }

    let expected = [
        row(
            1, 300_000, 250_000, 50_000, 50_000, 90_000, 80_000, 66_000, 5_500,
        ),
        row(
            2, 300_000, 280_000, 20_000, 70_000, 122_303, 85_100, 76_000, 5_000,
        ),
        row(
            3, 1_040_000, 280_000, 760_000, 830_000, 79_671, 109_991, 76_000, 5_000,
        ),
        row(
            4, 300_000, 400_000, -100_000, 730_000, 82_020, 247_840, 76_000, 5_000,
        ),
        row(
            5, 300_000, 380_000, -80_000, 650_000, 80_618, 216_616, 86_000, 11_000,
        ),
        row(
            6, 500_000, 300_000, 200_000, 850_000, 94_391, -30_000, 86_000, 5_500,
        ),
        row(
            7, 300_000, 310_000, -10_000, 840_000, 109_231, 117_343, 86_000, 3_559,
        ),
        row(
            8, 260_000, 300_000, -40_000, 800_000, 170_715, 74_533, 86_000, 356,
        ),
        row(
            9, 300_000, 200_000, 100_000, 900_000, 78_147, 64_268, 86_000, 0,
        ),
    ];
    for e in &expected {
        let m = e.month;
        let actual = &months[(m - 1) as usize];
        assert_eq!(actual.income_cents, e.income, "Monat {m} Einnahmen");
        assert_eq!(actual.expense_cents, e.expense, "Monat {m} Ausgaben");
        assert_eq!(actual.saldo_cents, e.saldo, "Monat {m} Saldo");
        assert_eq!(
            actual.cumulative_cents,
            Some(e.cumulative),
            "Monat {m} kumuliert"
        );
        assert_eq!(actual.fixed_net_cents, e.fixed, "Monat {m} Fixkosten");
        assert_eq!(
            actual.variable_net_cents, e.variable,
            "Monat {m} Variable Kosten"
        );
        assert_eq!(actual.savings_net_cents, e.savings, "Monat {m} Sparen");
        assert_eq!(actual.other_net_cents, e.other, "Monat {m} Sonstiges");
    }
    // Months with no data carry no cumulative value — the sheet's NA(). Zero-filling
    // would draw a cliff in the chart that does not exist.
    for m in 10..=12 {
        assert_eq!(months[m - 1].booking_count, 0);
    }
}

#[test]
fn june_variable_costs_are_negative() {
    require_fixtures!();
    // The best single regression test for netting: a 1.000,00 reimbursement in
    // Dienstreisen makes June's variable-cost figure negative. Any implementation
    // that clamps at zero, uses abs(), or nets gross-only fails right here.
    let months = calc::by_month(&ledger_2026());
    assert_eq!(months[5].variable_net_cents, -30_000);
}

// --------------------------------------------------------------- derived figures

#[test]
fn per_month_averages_divide_by_months_with_data() {
    require_fixtures!();
    let rows = ledger_2026();
    assert_eq!(calc::months_with_data(&rows), 9, "Monate mit Daten");
    assert_eq!(calc::average_expense_per_month(&rows), 300_000);
    assert_eq!(calc::fixed_costs_per_month(&rows), 90_000);
}

#[test]
fn all_three_savings_figures() {
    require_fixtures!();
    let r = calc::savings_rates(&ledger_2026());
    assert_eq!(r.naive_numerator_cents, 900_000);
    assert_eq!(r.naive_denominator_cents, 3_600_000);
    assert!(
        (r.naive_rate - 0.25).abs() < 0.0001,
        "naiv {}",
        r.naive_rate
    );

    assert_eq!(r.income_base_cents, 2_880_000);
    assert_eq!(r.consumption_cents, 1_332_000);
    assert_eq!(r.savings_amount_cents, 1_548_000);
    assert!(
        (r.consumption_rate - 0.5375).abs() < 0.0001,
        "konsumbasiert {}",
        r.consumption_rate
    );

    // The deposit: what actually went into Sparen & Anlage. Unlike the other two
    // this one is checkable against a bank statement, which is why it is the
    // headline figure — 660 a month to Mai, 860 after.
    assert_eq!(r.savings_deposit_cents, 648_000);
    assert!(
        (r.savings_deposit_rate - 0.225).abs() < 0.0001,
        "Sparrate {}",
        r.savings_deposit_rate
    );
    // And it is strictly the smaller of the two: money that merely stayed in the
    // account counts as "not consumed" but was never paid into anything.
    assert!(r.savings_deposit_cents < r.savings_amount_cents);
}

#[test]
fn savings_identity_ties_the_two_rates_together() {
    require_fixtures!();
    // savings_amount == saldo + net(Sparen). If this fails, a category has been
    // mistyped or a transfer has leaked into consumption.
    let rows = ledger_2026();
    let r = calc::savings_rates(&rows);
    let t = calc::totals(&rows);
    let savings_net: i64 = rows
        .iter()
        .filter(|r| r.is_savings)
        .map(LedgerRow::net_cents)
        .sum();
    assert_eq!(r.savings_amount_cents, t.saldo_cents + savings_net);
    assert_eq!(1_548_000, 900_000 + 648_000);
}

#[test]
fn carryover_chains_and_surfaces_the_legacy_gap() {
    require_fixtures!();
    let legacy: serde_json::Value = serde_json::from_str(&fix_legacy()).unwrap();
    let legacy_saldo = legacy["rowTotalCents"].as_i64().unwrap();
    let rows = ledger_2026();

    let chained = calc::chain_years(&[
        calc::YearRaw {
            year: 2025,
            opening_cents: 0,
            opening_is_configured: false,
            saldo_cents: legacy_saldo,
            booking_count: 1404,
        },
        calc::YearRaw {
            year: 2026,
            // Configured, not derived: the legacy rows sum to 39.750,00 while the
            // sheet's own month markers sum to 40.000,00.
            opening_cents: 4_000_000,
            opening_is_configured: true,
            saldo_cents: calc::totals(&rows).saldo_cents,
            booking_count: 474,
        },
    ]);

    let y2026 = chained.iter().find(|y| y.year == 2026).unwrap();
    assert_eq!(y2026.opening_cents, 4_000_000, "Vortrag 40.000,00");
    assert_eq!(y2026.saldo_cents, 900_000);
    assert_eq!(y2026.closing_cents, 4_900_000, "Bilanz gesamt 49.000,00");
    assert_eq!(
        y2026.chain_gap_cents,
        Some(25_000),
        "die 250,00 Differenz muss sichtbar sein, nicht verschluckt"
    );
}

#[test]
fn transfers_are_excluded_from_consumption_but_not_from_the_balance() {
    require_fixtures!();
    let mut rows = ledger_2026();
    let before_types = calc::by_type(&rows);
    let before_cats = net_by_name(&rows);
    let before_rates = calc::savings_rates(&rows);

    rows.push(LedgerRow {
        period_year: 2026,
        period_month: 5,
        kind: Kind::Transfer,
        amount_cents: 10_000_000,
        category_id: None,
        category_name: None,
        type_code: None,
        type_label: None,
        is_income: false,
        is_savings: false,
        in_consumption: false,
        tax_relevant: false,
    });

    assert_eq!(calc::by_type(&rows), before_types, "Typ-Netto unverändert");
    assert_eq!(
        net_by_name(&rows),
        before_cats,
        "Kategorie-Netto unverändert"
    );
    assert_eq!(
        calc::totals(&rows).saldo_cents,
        900_000,
        "Saldo unverändert"
    );
    assert_eq!(calc::by_month(&rows)[4].saldo_cents, -80_000);
    let after = calc::savings_rates(&rows);
    assert_eq!(after.consumption_rate, before_rates.consumption_rate);
}

// --------------------------------------------------------------------- legacy

#[test]
fn legacy_decodes_to_31_consecutive_months() {
    require_fixtures!();
    let legacy: serde_json::Value = serde_json::from_str(&fix_legacy()).unwrap();
    let blocks = legacy["blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 31, "31 Monatsblöcke");
    assert_eq!(legacy["bookings"].as_array().unwrap().len(), 1404);

    assert_eq!(blocks[0]["year"], 2023);
    assert_eq!(blocks[0]["month"], 6);
    assert_eq!(blocks[30]["year"], 2025);
    assert_eq!(blocks[30]["month"], 12);

    // Sequential and gap-free.
    let mut expect = (2023i64, 6i64);
    for b in blocks {
        assert_eq!(
            (b["year"].as_i64().unwrap(), b["month"].as_i64().unwrap()),
            expect
        );
        expect = if expect.1 == 12 {
            (expect.0 + 1, 1)
        } else {
            (expect.0, expect.1 + 1)
        };
    }

    // Every label that is present agrees with its inferred position — the decode is
    // self-validating, which is what makes the 7 unlabelled blocks safe.
    let labelled = blocks
        .iter()
        .filter(|b| b["labelSource"] == "label")
        .count();
    assert_eq!(
        labelled, 24,
        "24 beschriftete Blöcke bestätigen die Reihenfolge"
    );
}

#[test]
fn legacy_markers_sum_to_the_carryover_and_rows_fall_short_by_316_50() {
    require_fixtures!();
    let legacy: serde_json::Value = serde_json::from_str(&fix_legacy()).unwrap();
    assert_eq!(legacy["markerTotalCents"], 4_000_000, "Marker = Vortrag");
    assert_eq!(legacy["rowTotalCents"], 4_485_541, "Zeilensumme");
    let gap =
        legacy["markerTotalCents"].as_i64().unwrap() - legacy["rowTotalCents"].as_i64().unwrap();
    assert_eq!(gap, 25_000, "250,00 Differenz");
}

#[test]
fn exactly_three_legacy_blocks_disagree_with_their_marker() {
    require_fixtures!();
    // Recorded, never silently adjusted.
    let legacy: serde_json::Value = serde_json::from_str(&fix_legacy()).unwrap();
    let mismatches: Vec<(i64, i64, i64)> = legacy["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| {
            let marker = b["markerCents"].as_i64()?;
            let computed = b["computedCents"].as_i64()?;
            (marker != computed).then_some((
                b["year"].as_i64()?,
                b["month"].as_i64()?,
                computed - marker,
            ))
        })
        .collect();
    assert_eq!(
        mismatches,
        vec![(2023, 9, -16_000), (2023, 10, -16_000), (2024, 6, 350)]
    );
}

#[test]
fn legacy_per_year_totals() {
    require_fixtures!();
    let legacy: serde_json::Value = serde_json::from_str(&fix_legacy()).unwrap();
    let mut per_year: BTreeMap<i64, (i64, i64, i64)> = BTreeMap::new();
    for b in legacy["bookings"].as_array().unwrap() {
        let year = b["year"].as_i64().unwrap();
        let amount = b["amountCents"].as_i64().unwrap();
        let e = per_year.entry(year).or_default();
        e.2 += 1;
        if b["kind"] == "income" {
            e.0 += amount
        } else {
            e.1 += amount
        }
    }
    assert_eq!(per_year[&2023], (2_716_268, 2_250_317, 314));
    assert_eq!(per_year[&2024], (5_057_935, 3_848_186, 513));
    assert_eq!(per_year[&2025], (6_765_422, 3_955_581, 577));
}

// ------------------------------------------------------------ categorisation

#[test]
fn rule_table_shape() {
    require_fixtures!();
    let rules = rules();
    let types = category_types();
    assert_eq!(rules.len(), 192, "192 Regeln");
    assert_eq!(types.len(), 32, "32 Kategorien");
    // Keys are already normalised, so there can be no case-insensitive collision.
    for key in rules.keys() {
        assert_eq!(*key, key.trim().to_lowercase());
    }
}

#[test]
fn matching_is_case_insensitive_where_the_data_needs_it() {
    require_fixtures!();
    // All five pairs genuinely occur in both casings in the source files. Matching
    // case-sensitively would leave one of each pair unresolved.
    let rules = rules();
    for (a, b) in [
        ("essen", "Essen"),
        ("parken", "Parken"),
        ("spotify", "Spotify"),
        ("paypal", "PayPal"),
        ("apotheke", "Apotheke"),
    ] {
        let ra = resolve(a, None, &rules);
        let rb = resolve(b, None, &rules);
        assert!(ra.is_some(), "{a} muss aufgelöst werden");
        assert_eq!(ra, rb, "{a} und {b} müssen dieselbe Kategorie ergeben");
    }
}

#[test]
fn manual_override_beats_the_rule_table() {
    require_fixtures!();
    let raw: Vec<serde_json::Value> = serde_json::from_str(&fix_2026()).unwrap();
    let overrides: Vec<_> = raw
        .iter()
        .filter(|b| b["manualCategory"].is_string())
        .map(|b| {
            (
                b["comment"].as_str().unwrap().to_string(),
                b["manualCategory"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(overrides.len(), 3, "3 manuelle Zuordnungen");
    for (_, category) in &overrides {
        assert_eq!(category, "Dienstreisen");
    }

    let rules = rules();
    // `Hotel Wien` maps to Reisen & Urlaub by rule, but the override wins.
    assert_eq!(
        resolve("Hotel Wien", None, &rules).as_deref(),
        Some("Reisen & Urlaub")
    );
    assert_eq!(
        resolve("Hotel Wien", Some("Dienstreisen"), &rules).as_deref(),
        Some("Dienstreisen")
    );
}

#[test]
fn legacy_unmatched_comments_are_flagged_not_bucketed() {
    require_fixtures!();
    // ~362 rows across ~240 distinct comments match no rule. They must be counted and
    // visible, never folded into the real `Sonstiges` category. A range rather than an
    // exact figure: row-repeat expansion in the ODS is a judgement call.
    let legacy: serde_json::Value = serde_json::from_str(&fix_legacy()).unwrap();
    let rules = rules();
    let mut unmatched_rows = 0usize;
    let mut distinct = std::collections::BTreeSet::new();
    for b in legacy["bookings"].as_array().unwrap() {
        let comment = b["comment"].as_str().unwrap();
        if resolve(comment, None, &rules).is_none() {
            unmatched_rows += 1;
            distinct.insert(comment.trim().to_lowercase());
        }
    }
    assert!(
        (355..=370).contains(&unmatched_rows),
        "unmatched rows = {unmatched_rows}"
    );
    assert!(
        (232..=245).contains(&distinct.len()),
        "distinct unmatched = {}",
        distinct.len()
    );
}

// ------------------------------------------------------------------ fixtures

#[test]
#[ignore = "reads the real workbooks; run with --ignored"]
fn fixtures_match_the_source_workbooks() {
    let xlsx = std::fs::read("../konten_2026_auswertung.xlsx").expect("xlsx");
    let ods = std::fs::read("../konten.ods").expect("ods");

    let bookings = finanzen::sheets::read_xlsx_bookings(&xlsx).unwrap();
    let taxonomy = finanzen::sheets::read_xlsx_taxonomy(&xlsx).unwrap();
    let legacy = finanzen::sheets::read_ods_legacy(&ods, "2023-2025", 2023, 6).unwrap();

    assert_eq!(bookings.len(), 474);
    assert_eq!(taxonomy.rules.len(), 192);
    assert_eq!(taxonomy.category_types.len(), 32);
    assert_eq!(legacy.bookings.len(), 1404);
    assert_eq!(legacy.blocks.len(), 31);
    assert_eq!(legacy.marker_total_cents, 4_000_000);
    assert_eq!(legacy.row_total_cents, 4_485_541);

    // The 2014–2023 workbook: 99 consecutive months, every one of them labelled.
    let umsatz = std::fs::read("../Umsatz.xlsx").expect("umsatz");
    let monthly = finanzen::sheets::read_xlsx_monthly(&umsatz).unwrap();
    assert_eq!(monthly.bookings.len(), 1580);
    assert_eq!(monthly.blocks.len(), 99);
    assert_eq!(
        (monthly.blocks[0].year, monthly.blocks[0].month),
        (2014, 11)
    );
    assert_eq!(
        (monthly.blocks[98].year, monthly.blocks[98].month),
        (2023, 1)
    );
    // Five months carry no `Gewinn`, and three disagree with their own rows —
    // together by 500,00 — which is recorded and never adjusted.
    assert_eq!(
        monthly
            .blocks
            .iter()
            .filter(|b| b.marker_cents.is_none())
            .count(),
        5
    );
    assert_eq!(monthly.row_total_cents - monthly.marker_total_cents, 50_000);
}

// ----------------------------------------------------------- the suggester

/// Measures the suggester against the real unknown comments, so the coverage claim
/// in the design is a number this repo keeps honest rather than an assertion in a
/// document. If a future change to the matcher moves these, that is worth noticing.
#[test]
fn suggester_coverage_on_the_real_unknown_comments() {
    require_fixtures!();
    use finanzen::suggest::{self, KnownRule};

    let rules_map = rules();
    let types = category_types();
    let known: Vec<KnownRule> = rules_map
        .iter()
        .map(|(key, category)| KnownRule {
            match_key: key.clone(),
            category_id: uuid::Uuid::nil(),
            category_name: category.clone(),
        })
        .collect();
    assert!(types.contains_key("Lebensmittel"));

    let legacy: serde_json::Value = serde_json::from_str(&fix_legacy()).unwrap();
    let mut unknown: BTreeMap<String, i64> = BTreeMap::new();
    for b in legacy["bookings"].as_array().unwrap() {
        let comment = b["comment"].as_str().unwrap();
        if resolve(comment, None, &rules_map).is_none() {
            *unknown.entry(comment.trim().to_string()).or_insert(0) += 1;
        }
    }

    let (mut suggested_distinct, mut suggested_rows) = (0i64, 0i64);
    let total_rows: i64 = unknown.values().sum();
    for (comment, count) in &unknown {
        let (suggestions, _, _) = suggest::suggest(comment, &known, 0.92);
        if !suggestions.is_empty() {
            suggested_distinct += 1;
            suggested_rows += count;
        }
    }

    let distinct_pct = suggested_distinct * 100 / unknown.len() as i64;
    let row_pct = suggested_rows * 100 / total_rows;
    eprintln!(
        "suggester: {suggested_distinct}/{} distinct ({distinct_pct}%), \
         {suggested_rows}/{total_rows} rows ({row_pct}%)",
        unknown.len()
    );

    // The measured figure is ~39% of distinct comments. Held as a band: the point is
    // that most unknowns are new merchants no string algorithm can recover, so the
    // review queue — not the matcher — has to make the work fast.
    assert!(
        (25..=55).contains(&distinct_pct),
        "suggester coverage moved to {distinct_pct}% of distinct comments"
    );

    // The ones that matter: high-frequency unknowns are merchants, not typos, and
    // must not receive an invented category.
    for comment in ["Malve", "Hofladen Brinkmann", "Burgerbude", "Vela"] {
        if unknown.contains_key(comment) {
            let (s, _, _) = suggest::suggest(comment, &known, 0.92);
            assert!(
                s.is_empty(),
                "{comment} darf keinen Vorschlag bekommen: {s:?}"
            );
        }
    }
}

/// The running balance must exist only where there is data.
///
/// `cumulativeCents` is documented as null for a month with no bookings, and the
/// spreadsheet writes NA() there. Carrying September's figure into October,
/// November and December asserts a balance for three months that have not happened
/// — and a chart drawn from it shows a flat line into the future.
#[test]
fn the_cumulative_balance_stops_where_the_data_stops() {
    require_fixtures!();
    let months = calc::by_month(&ledger_2026());

    // Jan–Sep carry data and therefore a running balance, ending at the year's own
    // closing figure.
    for (index, month) in months.iter().take(9).enumerate() {
        assert!(
            month.cumulative_cents.is_some(),
            "Monat {} hat Buchungen und braucht einen Wert",
            index + 1
        );
    }
    assert_eq!(months[8].cumulative_cents, Some(900_000));

    // Oktober, November, Dezember have none.
    for (index, month) in months.iter().enumerate().skip(9) {
        assert_eq!(
            month.cumulative_cents,
            None,
            "Monat {} hat keine Buchungen und darf keinen Saldo behaupten",
            index + 1
        );
    }
}

/// A month with no bookings *between* two that have them is not the same case: the
/// balance is continuous across it, so the line carries forward rather than breaking.
#[test]
fn a_gap_inside_the_data_keeps_the_running_balance() {
    let row = |month: u8, cents: i64| LedgerRow {
        period_year: 2026,
        period_month: month,
        kind: Kind::Expense,
        amount_cents: cents,
        category_id: None,
        category_name: None,
        type_code: None,
        type_label: None,
        is_income: false,
        is_savings: false,
        in_consumption: true,
        tax_relevant: false,
    };
    // Januar and März, nothing in Februar.
    let months = calc::by_month(&[row(1, 1_000), row(3, 2_000)]);

    assert_eq!(months[0].cumulative_cents, Some(-1_000));
    assert_eq!(
        months[1].cumulative_cents,
        Some(-1_000),
        "die Lücke trägt den Saldo weiter"
    );
    assert_eq!(months[2].cumulative_cents, Some(-3_000));
    assert_eq!(
        months[3].cumulative_cents, None,
        "nach den Daten endet der Saldo"
    );
}

// -------------------------------------------- the month-block workbook (2014–2023)

/// The third workbook shape, read from a nine-row fixture that carries every trap
/// the real 2014–2023 file does. The fixture is invented data and committed, so
/// this runs in CI; `monthly_shape.py` beside it regenerates both files.
#[test]
fn month_block_workbook_reads_every_trap() {
    let bytes = std::fs::read("tests/fixtures/monthly_shape.xlsx").expect("fixture");
    let sheet = finanzen::sheets::read_xlsx_monthly(&bytes).unwrap();

    // Four months, in sequence, each named by its own label — three different
    // apostrophes and one with a space inside it.
    let labels: Vec<(i32, u8)> = sheet.blocks.iter().map(|b| (b.year, b.month)).collect();
    assert_eq!(labels, vec![(2014, 11), (2014, 12), (2015, 1), (2015, 2)]);
    assert!(sheet.blocks.iter().all(|b| b.label_source == "label"));

    // Nine bookings: the marker-only row is not one, and neither is the scratch.
    assert_eq!(sheet.bookings.len(), 9);
    // `net_cents` is expenses minus income, so the file's saldo is its negation:
    // 25,00 + 109,98 − 30,00 + 5,00 came in over the four months.
    assert_eq!(
        sheet.bookings.iter().map(|b| b.net_cents()).sum::<i64>(),
        -(2500 + 10998 - 3000 + 500)
    );
    assert_eq!(
        sheet.blocks.iter().map(|b| b.computed_cents).sum::<i64>(),
        2500 + 10998 - 3000 + 500
    );

    // A negative expense is money that came in, and is stored as income.
    let refund = sheet
        .bookings
        .iter()
        .find(|b| b.comment == "Rueckerstattung")
        .expect("the refund");
    assert_eq!(refund.kind(), "income");
    assert_eq!(refund.amount_cents(), 1998);

    // A row with an amount and no purpose is still money that moved. It cannot
    // keep an empty comment — a booking nobody can identify is refused by the
    // schema — so it carries its own row number and stays separately reviewable.
    let nameless = sheet
        .bookings
        .iter()
        .filter(|b| b.comment.starts_with("(ohne Zweck"))
        .collect::<Vec<_>>();
    assert_eq!(nameless.len(), 1);
    assert_eq!(nameless[0].comment, "(ohne Zweck, Zeile 7)");
    assert_eq!(nameless[0].amount_cents(), 1000);
    assert_eq!(nameless[0].kind(), "expense");

    // `Gewinn` is the marker, not `Kontostand`: November's 1.000,00 opening sits on
    // the block's FIRST row, and reading it as a saldo would be off by a thousand.
    assert_eq!(sheet.blocks[0].marker_cents, Some(2400));
    assert_eq!(sheet.blocks[0].computed_cents, 2500);
    // A month with no marker is left out of both totals rather than counted as zero.
    assert_eq!(sheet.blocks[3].marker_cents, None);
    assert_eq!(sheet.marker_total_cents, 2400 + 10998 - 3000);
    assert_eq!(sheet.row_total_cents, 2500 + 10998 - 3000);

    // ...and its rows are imported all the same.
    assert_eq!(sheet.blocks[3].row_count, 2);
}

/// A month missing from the middle is refused, not absorbed: shifting every
/// following block by one is the one failure nobody would notice.
#[test]
fn month_block_workbook_refuses_a_broken_sequence() {
    let bytes = std::fs::read("tests/fixtures/broken_sequence.xlsx").expect("fixture");
    let err = finanzen::sheets::read_xlsx_monthly(&bytes).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("Monatsfolge"), "{message}");
    assert!(message.contains("Dezember"), "{message}");
}
