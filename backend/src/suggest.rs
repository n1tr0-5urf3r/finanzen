//! Category suggestions for comments no rule matches.
//!
//! Measured against a legacy workbook's 240 unknown comments, a three-stage
//! suggester produces a suggestion for 95 of them (39%), covering 130 of 362 rows.
//! Essentially all of those hits come from **containment** — a known rule key
//! appearing as a token or substring of the unknown comment. Edit distance adds
//! almost nothing and is where the confidently-wrong answers come from
//! (`trinken`→`tanken` is the shape of it: one letter apart, unrelated category),
//! so it is gated hard and its output is labelled a hint rather than a suggestion.
//!
//! The honest consequence: the matcher is not the feature, the review queue is. The
//! unknowns that recur are new merchants, not typos, and no string algorithm can
//! recover a category that is not in the rule table at all.

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub category_id: Uuid,
    pub category_name: String,
    pub matched_rule: String,
    pub confidence: f64,
    pub tier: &'static str,
    /// `false` for low-confidence hints, which the UI must show greyed and never
    /// preselect.
    pub is_suggestion: bool,
}

#[derive(Debug, Clone)]
pub struct KnownRule {
    pub match_key: String,
    pub category_id: Uuid,
    pub category_name: String,
}

pub(crate) fn normalize(raw: &str) -> String {
    raw.trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Jaro-Winkler, for the gated similarity tier only.
pub(crate) fn jaro_winkler(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let jaro = {
        if a.is_empty() || b.is_empty() {
            return 0.0;
        }
        let window = (a.len().max(b.len()) / 2).saturating_sub(1);
        let mut a_match = vec![false; a.len()];
        let mut b_match = vec![false; b.len()];
        let mut matches = 0usize;
        for (i, ca) in a.iter().enumerate() {
            let lo = i.saturating_sub(window);
            let hi = (i + window + 1).min(b.len());
            for j in lo..hi {
                if !b_match[j] && b[j] == *ca {
                    a_match[i] = true;
                    b_match[j] = true;
                    matches += 1;
                    break;
                }
            }
        }
        if matches == 0 {
            return 0.0;
        }
        let mut transpositions = 0usize;
        let mut k = 0usize;
        for i in 0..a.len() {
            if a_match[i] {
                while !b_match[k] {
                    k += 1;
                }
                if a[i] != b[k] {
                    transpositions += 1;
                }
                k += 1;
            }
        }
        let m = matches as f64;
        (m / a.len() as f64 + m / b.len() as f64 + (m - transpositions as f64 / 2.0) / m) / 3.0
    };
    let prefix = a
        .iter()
        .zip(b.iter())
        .take(4)
        .take_while(|(x, y)| x == y)
        .count() as f64;
    jaro + prefix * 0.1 * (1.0 - jaro)
}

/// Returns `(suggestions, weak_hints, ambiguous)`.
///
/// `ambiguous` marks a comment that two different rules claim with equal strength —
/// `Paypal essen` genuinely matches both `Paypal` and `essen`. Those must be shown
/// side by side with nothing preselected, because guessing is wrong half the time.
pub fn suggest(
    comment: &str,
    rules: &[KnownRule],
    min_confidence: f64,
) -> (Vec<Suggestion>, Vec<Suggestion>, bool) {
    let needle = normalize(comment);
    if needle.is_empty() {
        return (Vec::new(), Vec::new(), false);
    }
    let tokens: Vec<&str> = needle.split(' ').collect();

    // Tier 1: containment. A known rule key that appears as a whole token, or as a
    // substring of at least four characters. High precision: this is what gets
    // `Edeka Berlin` -> Lebensmittel and `Geschenk Mama` -> Geschenke.
    let mut contained: Vec<Suggestion> = Vec::new();
    for rule in rules {
        let key = normalize(&rule.match_key);
        if key.is_empty() || key == needle {
            continue;
        }
        let whole_token = tokens.iter().any(|t| *t == key);
        let substring = key.chars().count() >= 4 && needle.contains(&key);
        if whole_token || substring {
            contained.push(Suggestion {
                category_id: rule.category_id,
                category_name: rule.category_name.clone(),
                matched_rule: rule.match_key.clone(),
                // Longer keys are more specific, so they rank above shorter ones.
                confidence: if whole_token { 0.95 } else { 0.90 },
                tier: if whole_token { "token" } else { "contain" },
                is_suggestion: true,
            });
        }
    }
    if !contained.is_empty() {
        contained.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.matched_rule.len().cmp(&a.matched_rule.len()))
        });
        // Two different categories claiming the same comment with the same strength
        // is genuine ambiguity, not a ranking problem.
        let top = contained[0].confidence;
        let distinct: std::collections::BTreeSet<&str> = contained
            .iter()
            .filter(|s| s.confidence >= top)
            .map(|s| s.category_name.as_str())
            .collect();
        let ambiguous = distinct.len() > 1;
        contained.truncate(3);
        return (contained, Vec::new(), ambiguous);
    }

    // Tier 2: similarity, gated. Everything below the gate is a hint, never a
    // suggestion — see the module note on why this tier earns its keep only barely.
    let mut scored: Vec<Suggestion> = rules
        .iter()
        .map(|rule| {
            let score = jaro_winkler(&needle, &normalize(&rule.match_key));
            Suggestion {
                category_id: rule.category_id,
                category_name: rule.category_name.clone(),
                matched_rule: rule.match_key.clone(),
                confidence: score,
                tier: "similar",
                is_suggestion: score >= min_confidence,
            }
        })
        .collect();
    scored.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(3);

    let (suggestions, hints): (Vec<_>, Vec<_>) = scored.into_iter().partition(|s| s.is_suggestion);
    (suggestions, hints, false)
}

/// How alike two free-text labels are, in `0.0..=1.0`.
///
/// Shared with the KitchenOwl link suggester so both use one definition of "looks
/// like the same purchase". Containment scores higher than pure edit distance
/// because that is what the measurement on the real data showed: `Kaufland` inside
/// `Kaufland Wocheneinkauf` is a real signal, whereas `trinken` vs `tanken` is not.
pub fn similarity(a: &str, b: &str) -> f64 {
    let (a, b) = (normalize(a), normalize(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let contained = (a.chars().count() >= 4 && b.contains(&a))
        || (b.chars().count() >= 4 && a.contains(&b))
        || a.split(' ')
            .any(|t| t.chars().count() >= 4 && b.split(' ').any(|u| u == t));
    if contained {
        return 0.9_f64.max(jaro_winkler(&a, &b));
    }
    jaro_winkler(&a, &b)
}

/// Comments the importer classifies as transfers without asking. Deliberately tiny:
/// only moves between the user's own accounts, which is what was decided. Cash
/// withdrawals and settlements import as income/expense and can be changed later.
pub fn transfer_hint(comment: &str) -> bool {
    let n = normalize(comment);
    n == "to ing" || n == "from volksbank"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Vec<KnownRule> {
        [
            ("edeka", "Lebensmittel"),
            ("essen", "Essen auswärts"),
            ("geschenk", "Geschenke"),
            ("gehalt", "Gehalt"),
            ("tanken", "Auto & Parken"),
            ("mafit", "Sport"),
            ("pfand", "Lebensmittel"),
            ("steam", "Games & Software"),
            ("paypal", "Sonstiges"),
            ("ticket berlin", "Dienstreisen"),
        ]
        .iter()
        .map(|(k, c)| KnownRule {
            match_key: k.to_string(),
            category_id: Uuid::new_v4(),
            category_name: c.to_string(),
        })
        .collect()
    }

    #[test]
    fn containment_carries_the_suggester() {
        let r = rules();
        for (comment, expected) in [
            ("Edeka Berlin", "Lebensmittel"),
            ("essen Berlin", "Essen auswärts"),
            ("Essen München", "Essen auswärts"),
            ("Geschenk Mama", "Geschenke"),
            ("Gehalt + Weihnachtsgeld", "Gehalt"),
            ("DB Ticket Berlin", "Dienstreisen"),
        ] {
            let (s, _, _) = suggest(comment, &r, 0.92);
            assert_eq!(
                s.first().map(|s| s.category_name.as_str()),
                Some(expected),
                "{comment}"
            );
        }
    }

    #[test]
    fn new_merchants_get_no_suggestion_rather_than_a_wrong_one() {
        // A merchant the rule table has never seen is the common case in a review
        // queue. None of these is recoverable from it, and inventing an answer
        // would be worse than admitting none.
        let r = rules();
        for comment in [
            "Malve",
            "Hofladen Brinkmann",
            "Burgerbude",
            "Vela",
            "Kiosk am Park",
        ] {
            let (s, _, _) = suggest(comment, &r, 0.92);
            assert!(
                s.is_empty(),
                "{comment} must not get a suggestion, got {s:?}"
            );
        }
    }

    #[test]
    fn the_confidently_wrong_similarity_matches_are_suppressed() {
        // Each of these is what an ungated edit-distance matcher proposes, and each
        // is wrong. They may appear as hints, never as suggestions.
        let r = rules();
        for (comment, wrong) in [
            ("trinken", "Auto & Parken"),
            ("Malve", "Sport"),
            ("iPad", "Lebensmittel"),
        ] {
            let (s, hints, _) = suggest(comment, &r, 0.92);
            assert!(s.is_empty(), "{comment} must not be suggested as {wrong}");
            assert!(hints.iter().all(|h| !h.is_suggestion));
        }
    }

    #[test]
    fn genuinely_ambiguous_comments_are_flagged() {
        let r = rules();
        let (s, _, ambiguous) = suggest("Paypal essen", &r, 0.92);
        assert!(ambiguous, "two categories claim this comment equally");
        let names: Vec<&str> = s.iter().map(|x| x.category_name.as_str()).collect();
        assert!(names.contains(&"Sonstiges") && names.contains(&"Essen auswärts"));
    }

    #[test]
    fn similarity_is_shared_with_the_kitchenowl_link_suggester() {
        assert_eq!(similarity("Kaufland", "kaufland"), 1.0);
        assert!(similarity("Kaufland", "Kaufland Wocheneinkauf") >= 0.9);
        assert!(similarity("Kino", "Kinokarten Ada") >= 0.9);
        // Different purchases must not look alike just because both are short.
        assert!(similarity("Kino", "Miete") < 0.6);
        assert_eq!(similarity("", "Kaufland"), 0.0);
    }

    #[test]
    fn account_moves_are_the_only_automatic_transfers() {
        assert!(transfer_hint("to ING"));
        assert!(transfer_hint("from Volksbank"));
        // Decided deliberately: these import as income/expense and can be changed.
        assert!(!transfer_hint("abgehoben"));
        assert!(!transfer_hint("Bargeld"));
        assert!(!transfer_hint("Ausgleich August"));
    }
}
