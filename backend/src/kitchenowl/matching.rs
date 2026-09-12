//! Link suggestions between a mirrored KitchenOwl expense and an existing booking.
//!
//! The decisive measurement: of the 211 expenses overlapping the imported 2026
//! bookings, **63 match a booking on the FULL amount** — because the user records
//! what actually left the account, not their half. Auto-booking a pulled expense
//! would therefore double-post most of the groceries. So a match is an optional,
//! reversible **link**, a high score pre-selects "verknüpfen" and never "anlegen",
//! and no score at all is the ordinary outcome for most expenses.

use chrono::Datelike;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::{
    error::Result,
    locale::{month_name_de, period_ord},
    suggest,
};

use super::wire::MirrorExpense;

/// Candidate weights. Full amount outranks own share because the data says the user
/// records the full amount; ±2 cents exists for the rounding disagreements between a
/// card terminal and a split, not as a licence to match loosely.
const SCORE_FULL: f64 = 0.60;
const SCORE_OWN_SHARE: f64 = 0.45;
const SCORE_FULL_NEAR: f64 = 0.35;
const SCORE_OWN_SHARE_NEAR: f64 = 0.25;
const SCORE_SAME_MONTH: f64 = 0.25;
const SCORE_ADJACENT_MONTH: f64 = 0.12;
const SCORE_NAME_WEIGHT: f64 = 0.15;
const NEAR_CENTS: i64 = 2;
/// Below this a candidate is not worth showing at all.
const SCORE_FLOOR: f64 = 0.45;
/// Between the floor and the configured threshold the UI shows the candidate but
/// pre-selects nothing.
const SCORE_POSSIBLE: f64 = 0.55;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub booking_id: Uuid,
    pub comment: String,
    pub amount_cents: i64,
    pub year: i32,
    pub month: u8,
    pub month_name: String,
    pub score: f64,
    /// Which rule fired, so the UI can say *why* rather than showing a bare number.
    pub basis: String,
}

#[derive(Debug, Clone, Copy)]
pub struct BookingRow {
    pub id: Uuid,
    pub amount_cents: i64,
    pub period_ord: i32,
}

/// Pure scoring, so every weight is unit-testable without a database.
pub fn score(
    expense: &MirrorExpense,
    expense_ord: i32,
    booking: BookingRow,
    booking_comment: &str,
) -> Option<(f64, &'static str)> {
    let delta_full = (booking.amount_cents - expense.amount_cents).abs();
    let delta_share = (booking.amount_cents - expense.own_share_cents).abs();

    let (amount_score, basis) = if delta_full == 0 {
        (SCORE_FULL, "fullAmount")
    } else if delta_share == 0 && expense.own_share_cents != expense.amount_cents {
        (SCORE_OWN_SHARE, "ownShare")
    } else if delta_full <= NEAR_CENTS {
        (SCORE_FULL_NEAR, "fullAmountNear")
    } else if delta_share <= NEAR_CENTS && expense.own_share_cents != expense.amount_cents {
        (SCORE_OWN_SHARE_NEAR, "ownShareNear")
    } else {
        return None;
    };

    let month_delta = (booking.period_ord - expense_ord).abs();
    let month_score = match month_delta {
        0 => SCORE_SAME_MONTH,
        1 => SCORE_ADJACENT_MONTH,
        _ => return None,
    };

    let name_score = suggest::similarity(&expense.name, booking_comment) * SCORE_NAME_WEIGHT;
    let total = amount_score + month_score + name_score;
    (total >= SCORE_FLOOR).then_some((total, basis))
}

/// Ranks candidate bookings for one expense.
///
/// A booking already linked to a *different* KitchenOwl expense is excluded in SQL:
/// the partial unique index on `(user_id, external_source, external_id)` would
/// refuse the link anyway, and offering a choice the database will reject is worse
/// than not offering it.
pub async fn candidates(
    conn: &mut PgConnection,
    expense: &MirrorExpense,
    limit: usize,
) -> Result<Vec<Candidate>> {
    let ord = period_ord(
        expense.expense_date.year(),
        expense.expense_date.month() as u8,
    );
    let low = expense.own_share_cents.min(expense.amount_cents) - NEAR_CENTS;
    let high = expense.own_share_cents.max(expense.amount_cents) + NEAR_CENTS;

    let rows = sqlx::query(
        "SELECT b.id, b.comment, b.amount_cents, b.period_year, b.period_month, b.period_ord \
           FROM bookings b \
          WHERE b.status = 'confirmed' AND b.kind <> 'transfer' \
            AND b.period_ord BETWEEN $1 - 1 AND $1 + 1 \
            AND b.amount_cents BETWEEN $2 AND $3 \
            AND (b.external_source IS NULL \
                 OR (b.external_source = 'kitchenowl' AND b.external_id = $4))",
    )
    .bind(ord)
    .bind(low)
    .bind(high)
    .bind(expense.external_id.to_string())
    .fetch_all(&mut *conn)
    .await?;

    let mut scored: Vec<Candidate> = rows
        .iter()
        .filter_map(|r| {
            let comment: String = r.get("comment");
            let booking = BookingRow {
                id: r.get("id"),
                amount_cents: r.get("amount_cents"),
                period_ord: r.get("period_ord"),
            };
            let (total, basis) = score(expense, ord, booking, &comment)?;
            let month: i16 = r.get("period_month");
            Some(Candidate {
                booking_id: booking.id,
                comment,
                amount_cents: booking.amount_cents,
                year: r.get::<i16, _>("period_year") as i32,
                month: month as u8,
                month_name: month_name_de(month as u8).to_string(),
                score: total,
                basis: basis.to_string(),
            })
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.booking_id.cmp(&b.booking_id))
    });
    scored.truncate(limit);
    Ok(scored)
}

/// The draft status a candidate list implies. Never `confirmed`: a suggestion is
/// never an action.
pub fn status_for(expense: &MirrorExpense, best: Option<f64>, threshold: f64) -> &'static str {
    if expense.exclude_from_statistics {
        // KitchenOwl's own "leave this out of the statistics" flag. Respected rather
        // than second-guessed, and still visible under a filter.
        return "ignored_by_default";
    }
    match best {
        Some(s) if s >= threshold => "likely_duplicate",
        Some(s) if s >= SCORE_POSSIBLE => "possible_duplicate",
        _ => "open",
    }
}

pub async fn create_draft(
    conn: &mut PgConnection,
    user_id: Uuid,
    ko_expense_id: Uuid,
    expense: &MirrorExpense,
    threshold: f64,
) -> Result<()> {
    let candidates = candidates(conn, expense, 5).await?;
    let status = status_for(expense, candidates.first().map(|c| c.score), threshold);
    let payload = serde_json::json!({
        "externalId": expense.external_id,
        "name": expense.name,
        "amountCents": expense.amount_cents,
        "ownShareCents": expense.own_share_cents,
        "date": expense.expense_date,
    });

    sqlx::query(
        "INSERT INTO ko_drafts (id, user_id, kind, ko_expense_id, status, match_candidates, payload) \
         VALUES ($1,$2,'expense',$3,$4,$5,$6)",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(ko_expense_id)
    .bind(status)
    .bind(serde_json::to_value(&candidates).unwrap_or_else(|_| serde_json::json!([])))
    .bind(payload)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Re-scores an existing draft after the expense changed upstream.
///
/// Only unresolved drafts are touched. A draft the user already confirmed or
/// discarded stays decided — a sync is not entitled to reopen a decision, and an
/// upstream typo fix must not make a dismissed suggestion come back.
pub async fn refresh_draft(
    conn: &mut PgConnection,
    user_id: Uuid,
    ko_expense_id: Uuid,
    expense: &MirrorExpense,
    threshold: f64,
) -> Result<()> {
    let open: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM ko_drafts WHERE ko_expense_id = $1 \
           AND status IN ('open','likely_duplicate','possible_duplicate','ignored_by_default') \
         ORDER BY created_at LIMIT 1",
    )
    .bind(ko_expense_id)
    .fetch_optional(&mut *conn)
    .await?;

    let Some(draft_id) = open else {
        let decided: i64 =
            sqlx::query_scalar("SELECT count(*)::bigint FROM ko_drafts WHERE ko_expense_id = $1")
                .bind(ko_expense_id)
                .fetch_one(&mut *conn)
                .await?;
        if decided == 0 {
            create_draft(conn, user_id, ko_expense_id, expense, threshold).await?;
        }
        return Ok(());
    };

    let candidates = candidates(conn, expense, 5).await?;
    let status = status_for(expense, candidates.first().map(|c| c.score), threshold);
    sqlx::query("UPDATE ko_drafts SET status = $2, match_candidates = $3 WHERE id = $1")
        .bind(draft_id)
        .bind(status)
        .bind(serde_json::to_value(&candidates).unwrap_or_else(|_| serde_json::json!([])))
        .execute(&mut *conn)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn expense(amount: i64, own: i64, name: &str) -> MirrorExpense {
        MirrorExpense {
            external_id: 1,
            name: name.into(),
            description: None,
            expense_date: NaiveDate::from_ymd_opt(2026, 5, 3).unwrap(),
            amount_cents: amount,
            own_share_cents: own,
            paid_by_id: Some(2),
            shares: Vec::new(),
            ko_category_id: None,
            ko_category_name: None,
            exclude_from_statistics: false,
            remote_hash: "h".into(),
        }
    }

    fn booking(amount: i64, ord: i32) -> BookingRow {
        BookingRow {
            id: Uuid::nil(),
            amount_cents: amount,
            period_ord: ord,
        }
    }

    const MAY: i32 = 2026 * 12 + 5 - 1;

    #[test]
    fn the_full_amount_in_the_same_month_clears_the_default_threshold_alone() {
        // This is the 63-of-211 case, and it must not need a name match to be
        // recognised: the user's comment is "Kaufland" and KitchenOwl's name may be
        // "Wocheneinkauf".
        let e = expense(1907, 954, "Wocheneinkauf");
        let (s, basis) = score(&e, MAY, booking(1907, MAY), "Kaufland").unwrap();
        assert_eq!(basis, "fullAmount");
        assert!(s >= 0.80, "{s} must clear the default threshold");
    }

    #[test]
    fn the_full_amount_outranks_the_own_share() {
        let e = expense(1907, 954, "Wocheneinkauf");
        let (full, _) = score(&e, MAY, booking(1907, MAY), "Kaufland").unwrap();
        let (share, basis) = score(&e, MAY, booking(954, MAY), "Kaufland").unwrap();
        assert_eq!(basis, "ownShare");
        assert!(full > share);
        // The own share alone must not pre-select a link: a coincidental half is
        // exactly the kind of match that silently attaches the wrong booking.
        assert!(share < 0.80);
    }

    #[test]
    fn two_cents_away_ranks_below_exact_and_three_cents_is_no_match() {
        let e = expense(1907, 954, "Wocheneinkauf");
        let (near, basis) = score(&e, MAY, booking(1909, MAY), "Kaufland").unwrap();
        assert_eq!(basis, "fullAmountNear");
        assert!(near < score(&e, MAY, booking(1907, MAY), "Kaufland").unwrap().0);
        assert!(score(&e, MAY, booking(1910, MAY), "Kaufland").is_none());
    }

    #[test]
    fn an_adjacent_month_is_a_candidate_and_two_months_away_is_not() {
        let e = expense(1907, 954, "Wocheneinkauf");
        assert!(score(&e, MAY, booking(1907, MAY - 1), "Kaufland").is_some());
        assert!(score(&e, MAY, booking(1907, MAY + 1), "Kaufland").is_some());
        assert!(score(&e, MAY, booking(1907, MAY + 2), "Kaufland").is_none());
        // An adjacent month scores below the same month, so a same-month booking
        // always wins a tie on amount.
        let same = score(&e, MAY, booking(1907, MAY), "Kaufland").unwrap().0;
        let next = score(&e, MAY, booking(1907, MAY + 1), "Kaufland")
            .unwrap()
            .0;
        assert!(same > next);
    }

    #[test]
    fn a_matching_name_lifts_a_weaker_amount_match() {
        let e = expense(1907, 954, "Kaufland");
        let bare = score(&e, MAY, booking(954, MAY), "Zug Berlin").unwrap().0;
        let named = score(&e, MAY, booking(954, MAY), "Kaufland").unwrap().0;
        assert!(named > bare);
    }

    #[test]
    fn a_solo_expense_never_double_counts_its_own_share_as_a_separate_rule() {
        // amount == own share, so the "own share" rules must not fire and inflate
        // the score for what is really just the full amount.
        let e = expense(1000, 1000, "Kiosk");
        let (_, basis) = score(&e, MAY, booking(1000, MAY), "Kiosk").unwrap();
        assert_eq!(basis, "fullAmount");
        assert!(score(&e, MAY, booking(1003, MAY), "Kiosk").is_none());
    }

    #[test]
    fn a_high_score_pre_selects_link_and_never_create() {
        let e = expense(1907, 954, "Wocheneinkauf");
        assert_eq!(status_for(&e, Some(0.90), 0.80), "likely_duplicate");
        assert_eq!(status_for(&e, Some(0.60), 0.80), "possible_duplicate");
        assert_eq!(status_for(&e, Some(0.46), 0.80), "open");
        // No candidate at all is the ORDINARY case, not an error state.
        assert_eq!(status_for(&e, None, 0.80), "open");
        for status in ["likely_duplicate", "possible_duplicate", "open"] {
            assert_ne!(status, "confirmed");
        }
    }

    #[test]
    fn kitchenowls_own_exclude_flag_is_respected() {
        let mut e = expense(1907, 954, "Wocheneinkauf");
        e.exclude_from_statistics = true;
        assert_eq!(status_for(&e, Some(0.99), 0.80), "ignored_by_default");
    }
}
