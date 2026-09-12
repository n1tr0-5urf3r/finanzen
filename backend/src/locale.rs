//! German locale handling: month names, decimal-comma parsing, and the single
//! definition of how a source value becomes an integer cent count.

use crate::error::{AppError, Result};

pub const MONTHS_DE: [&str; 12] = [
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];

/// Parses a German month name, case-insensitively, into 1..=12.
/// Accepts the ASCII fallback spellings `Maerz`/`Marz` that some exports produce.
pub fn month_from_de(name: &str) -> Option<u8> {
    let n = name.trim().to_lowercase();
    if n.is_empty() {
        return None;
    }
    if let Some(i) = MONTHS_DE.iter().position(|m| m.to_lowercase() == n) {
        return Some(i as u8 + 1);
    }
    match n.as_str() {
        "maerz" | "marz" => Some(3),
        _ => None,
    }
}

pub fn month_name_de(month: u8) -> &'static str {
    MONTHS_DE
        .get(month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("")
}

/// Strips an apostrophe-year suffix from a month label.
///
/// The legacy sheet mixes apostrophes: `Mai ‘25` uses U+2018 while `Juni '25` uses
/// U+0027. A parser that only knows the ASCII form silently fails to recognise two of
/// the month blocks, which shifts every subsequent month by one.
pub fn split_month_label(raw: &str) -> (&str, Option<u16>) {
    let trimmed = raw.trim();
    for sep in ['\u{2018}', '\u{2019}', '\'', '`', '\u{00B4}'] {
        if let Some((head, tail)) = trimmed.split_once(sep) {
            let year = tail.trim().parse::<u16>().ok().map(|y| {
                if y < 100 { 2000 + y } else { y }
            });
            return (head.trim(), year);
        }
    }
    (trimmed, None)
}

/// The one rounding rule in the application: half away from zero, at the cent.
///
/// Applied at exactly two boundaries — a spreadsheet cell becoming cents, and a
/// KitchenOwl float becoming cents. Both sources carry IEEE-754 artifacts
/// (`67.29000000000001`, `-149.16999999999217`), so this must never be skipped, and
/// nothing downstream may re-round.
pub fn cents_from_f64(value: f64) -> Result<i64> {
    if !value.is_finite() {
        return Err(AppError::Validation(format!(
            "Betrag ist keine endliche Zahl: {value}"
        )));
    }
    let scaled = value * 100.0;
    let rounded = scaled.round();
    // Guard against genuine sub-cent precision, which this domain never has. A value
    // more than a hundredth of a cent away from an integer is a parsing mistake, not
    // an artifact, and must not be silently absorbed.
    if (scaled - rounded).abs() > 0.01 {
        return Err(AppError::Validation(format!(
            "Betrag hat mehr als zwei Nachkommastellen: {value}"
        )));
    }
    Ok(rounded as i64)
}

/// Parses a German-formatted amount: `1.234,56`, `1234,56`, `12,50 €`, `-3,99`.
/// Also accepts a plain machine form (`1234.56`) so CSV round-trips work.
pub fn cents_from_de_str(raw: &str) -> Result<i64> {
    let mut s: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '€' && *c != '\u{00A0}')
        .collect();
    if s.is_empty() {
        return Err(AppError::Validation("Leerer Betrag".into()));
    }
    match (s.rfind(','), s.rfind('.')) {
        // Both present: the rightmost is the decimal separator.
        (Some(c), Some(d)) => {
            if c > d {
                s = s.replace('.', "").replace(',', ".");
            } else {
                s = s.replace(',', "");
            }
        }
        (Some(_), None) => s = s.replace(',', "."),
        // A lone dot is ambiguous: `1.234` is thousands in de-DE but `1.23` is a
        // machine decimal. Exactly two trailing digits means decimal.
        (None, Some(d)) => {
            if s.len() - d - 1 == 3 {
                s = s.replace('.', "");
            }
        }
        (None, None) => {}
    }
    let value: f64 = s
        .parse()
        .map_err(|_| AppError::Validation(format!("Betrag nicht lesbar: {raw}")))?;
    cents_from_f64(value)
}

/// Renders cents as de-DE, e.g. `1.234,56`. The currency suffix is the caller's job,
/// because CSV wants the bare number and the UI wants ` €`.
pub fn format_de(cents: i64) -> String {
    let negative = cents < 0;
    let abs = cents.unsigned_abs();
    let whole = abs / 100;
    let frac = abs % 100;
    let digits = whole.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3 + 4);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push('.');
        }
        grouped.push(ch);
    }
    format!("{}{grouped},{frac:02}", if negative { "-" } else { "" })
}

/// Integer division rounding half away from zero. Used for per-month averages, never
/// for money that must reconcile.
pub fn div_round_half_up(numerator: i64, denominator: i64) -> i64 {
    if denominator == 0 {
        return 0;
    }
    let (q, r) = (numerator / denominator, numerator % denominator);
    if r.abs() * 2 >= denominator.abs() {
        q + if (numerator < 0) != (denominator < 0) { -1 } else { 1 }
    } else {
        q
    }
}

/// `period_ord` — the single-column month key used for ordering, cross-year ranges
/// and cumulative window frames. Must match the generated column in migration 0007.
pub fn period_ord(year: i32, month: u8) -> i32 {
    year * 12 + month as i32 - 1
}

pub fn ord_to_year_month(ord: i32) -> (i32, u8) {
    (ord.div_euclid(12), (ord.rem_euclid(12) + 1) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_german_months_including_ascii_fallbacks() {
        assert_eq!(month_from_de("Januar"), Some(1));
        assert_eq!(month_from_de("märz"), Some(3));
        assert_eq!(month_from_de("Maerz"), Some(3));
        assert_eq!(month_from_de("Dezember"), Some(12));
        assert_eq!(month_from_de("Rückerstattung"), None);
        assert_eq!(month_from_de(""), None);
    }

    #[test]
    fn splits_both_apostrophe_variants_used_in_the_legacy_sheet() {
        // U+2018 (Mai ‘25) and U+0027 (Juni '25) both occur in konten.ods. Handling
        // only the ASCII form loses two month labels and shifts the sequence.
        assert_eq!(split_month_label("Mai \u{2018}25"), ("Mai", Some(2025)));
        assert_eq!(split_month_label("Juni '25"), ("Juni", Some(2025)));
        assert_eq!(split_month_label("Oktober \u{2019}25"), ("Oktober", Some(2025)));
        assert_eq!(split_month_label("Dezember"), ("Dezember", None));
    }

    #[test]
    fn absorbs_the_float_artifacts_present_in_both_source_files() {
        // Verbatim values observed in the raw XML of konten_2026_auswertung.xlsx
        // and konten.ods.
        assert_eq!(cents_from_f64(67.290000000000006).unwrap(), 6729);
        assert_eq!(cents_from_f64(4.6500000000000004).unwrap(), 465);
        assert_eq!(cents_from_f64(8.300000000000001).unwrap(), 830);
        assert_eq!(cents_from_f64(9.699999999999999).unwrap(), 970);
        assert_eq!(cents_from_f64(45171.910000000011).unwrap(), 4000000);
        // KitchenOwl float balance
        assert_eq!(cents_from_f64(-149.16999999999217).unwrap(), -14917);
        assert_eq!(cents_from_f64(31.889999999999997).unwrap(), 3189);
    }

    #[test]
    fn rejects_genuine_sub_cent_precision() {
        assert!(cents_from_f64(1.005).is_err());
        assert!(cents_from_f64(f64::NAN).is_err());
    }

    #[test]
    fn parses_german_decimal_comma_input() {
        assert_eq!(cents_from_de_str("1.234,56").unwrap(), 123456);
        assert_eq!(cents_from_de_str("12,50 €").unwrap(), 1250);
        assert_eq!(cents_from_de_str("0,99").unwrap(), 99);
        assert_eq!(cents_from_de_str("-3,99").unwrap(), -399);
        assert_eq!(cents_from_de_str("2.529,04 €").unwrap(), 252904);
        // machine form, for CSV round-trips
        assert_eq!(cents_from_de_str("1234.56").unwrap(), 123456);
        // a lone dot with three trailing digits is a thousands separator
        assert_eq!(cents_from_de_str("1.234").unwrap(), 123400);
    }

    #[test]
    fn formats_de_de() {
        assert_eq!(format_de(2700000), "27.000,00");
        assert_eq!(format_de(900000), "9.000,00");
        assert_eq!(format_de(4900000), "49.000,00");
        assert_eq!(format_de(-30000), "-300,00");
        assert_eq!(format_de(0), "0,00");
        assert_eq!(format_de(5), "0,05");
    }

    #[test]
    fn period_ord_round_trips_and_orders_across_years() {
        assert_eq!(period_ord(2026, 1), 24312);
        assert!(period_ord(2026, 1) > period_ord(2025, 12));
        for (y, m) in [(2023, 6), (2025, 12), (2026, 9)] {
            assert_eq!(ord_to_year_month(period_ord(y, m)), (y, m));
        }
    }

    #[test]
    fn division_rounds_half_away_from_zero() {
        assert_eq!(div_round_half_up(2700000, 9), 300000);
        assert_eq!(div_round_half_up(810000, 9), 90000);
        assert_eq!(div_round_half_up(5, 2), 3);
        assert_eq!(div_round_half_up(-5, 2), -3);
    }
}
