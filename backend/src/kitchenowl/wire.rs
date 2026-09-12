//! Everything that touches KitchenOwl's JSON.
//!
//! One module, on purpose: KitchenOwl's shapes were probed against a live instance
//! but the instance is production data, so **no write path could be exercised**.
//! Keeping every field name, every type coercion and every payload constructor in
//! one file means correcting a shape is a single-function change with a fixture
//! test next to it, not a hunt through the integration.
//!
//! Shapes verified live (read-only, 2026-09-12, 464 expenses):
//!
//! - `GET /api/household` -> array. `member[].expense_balance` is a float with
//!   IEEE-754 artifacts (`-142.26999999999217`) and is the only source of balances.
//!   Member flags are `owner` / `admin`, not `is_owner` / `is_admin`.
//! - `GET /api/user` -> the token's own user; its `id` is the member id to treat as
//!   "me". There is no `is_me` flag on the member objects.
//! - `GET /api/household/{id}/expense` -> 30 per page, ordered by **`date`
//!   descending**, NOT by id. See [`next_cursor`].
//! - `amount` is a float with artifacts (`213.87999999999994`). It becomes cents
//!   exactly once, here, through `locale::cents_from_f64`.
//! - `date` / `created_at` / `updated_at` are epoch **milliseconds**.
//! - `category` (the nested object) and a non-null `category_id` are present or
//!   absent together — 173 of 464 expenses carry neither. The `category_id` key is
//!   always present but its value is **`null`**, not `-1`; `-1` appears only as a
//!   bucket key in the `/overview` response.
//! - `paid_for[].factor` is an **integer weight**, not a percentage. 439 expenses
//!   split `{1:1, 2:1}`, 24 are solo, and one is `{1:12, 2:7}` — so weights above
//!   one occur in the real data.
//! - `?page=` / `?limit=` / `?offset=` answer **400 with the plain-text body
//!   `Request invalid`** and `Content-Type: text/html`. A bad token answers **422**
//!   with `{"msg": "Not enough segments"}` — not 401. Nothing here may assume the
//!   error body is JSON.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    error::{AppError, Result},
    locale::cents_from_f64,
};

// ------------------------------------------------------------- raw shapes

#[derive(Debug, Clone, Deserialize)]
pub struct RawHousehold {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub expenses_feature: bool,
    /// Singular, and singular in the wire too. Absent on some KitchenOwl versions,
    /// which is a household with no member list rather than a parse failure.
    #[serde(default)]
    pub member: Vec<RawMember>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawMember {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub username: Option<String>,
    /// The only source of balances in the whole API. `/balances` is a 404.
    #[serde(default)]
    pub expense_balance: Option<f64>,
    #[serde(default)]
    pub owner: bool,
    #[serde(default)]
    pub admin: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawUser {
    pub id: i64,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawExpenseCategory {
    pub id: i64,
    pub name: String,
    /// ARGB packed into an integer (`4289003611`), or null. Not a CSS colour.
    #[serde(default)]
    pub color: Option<i64>,
    /// VERIFY: null on every category of the live instance, so the unit is
    /// unobserved. Treated as euros-as-float, like `amount`; if it turns out to be
    /// cents already, this is the one line that changes.
    #[serde(default)]
    pub budget: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawPaidFor {
    pub user_id: i64,
    /// An integer weight. Own share is `amount * my_factor / sum(factors)`.
    #[serde(default = "one")]
    pub factor: i64,
}

fn one() -> i64 {
    1
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawExpense {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub amount: f64,
    /// Epoch milliseconds.
    pub date: i64,
    /// Present as a key on every observed expense, but `null` for the 173
    /// uncategorised ones. `-1` is an overview bucket key, never a value here —
    /// mapped to `None` anyway so both spellings behave the same.
    #[serde(default)]
    pub category_id: Option<i64>,
    /// Absent entirely (not null) whenever `category_id` is null.
    #[serde(default)]
    pub category: Option<RawExpenseCategory>,
    #[serde(default)]
    pub paid_by_id: Option<i64>,
    #[serde(default)]
    pub paid_for: Vec<RawPaidFor>,
    #[serde(default)]
    pub exclude_from_statistics: bool,
}

// --------------------------------------------------------- parsed results

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Share {
    pub member_id: i64,
    pub factor: i64,
    pub share_cents: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShareJson {
    pub member_id: i64,
    pub factor: i64,
    pub share_cents: i64,
}

/// One mirrored expense, already in the app's own units: integer cents, a calendar
/// date, no floats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorExpense {
    pub external_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub expense_date: NaiveDate,
    /// The full shared amount — what actually left somebody's account.
    pub amount_cents: i64,
    /// The token owner's slice of it. Never summed with `amount_cents`.
    pub own_share_cents: i64,
    pub paid_by_id: Option<i64>,
    pub shares: Vec<Share>,
    pub ko_category_id: Option<i64>,
    pub ko_category_name: Option<String>,
    pub exclude_from_statistics: bool,
    /// Changes iff any mirrored field changed, so re-syncing an untouched expense
    /// is a structural no-op rather than a handler-level comparison.
    pub remote_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorMember {
    pub member_id: i64,
    pub name: String,
    pub username: Option<String>,
    pub balance_cents: i64,
    pub is_owner: bool,
    pub is_admin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorCategory {
    pub category_id: i64,
    pub name: String,
    pub color_argb: Option<i64>,
    pub budget_cents: Option<i64>,
}

// ------------------------------------------------------------- conversions

fn integration<T>(what: &str, e: impl std::fmt::Display) -> Result<T> {
    Err(AppError::Integration(format!(
        "KitchenOwl-Antwort für {what} nicht lesbar: {e}"
    )))
}

pub fn parse_households(body: &[u8]) -> Result<Vec<RawHousehold>> {
    match serde_json::from_slice::<Vec<RawHousehold>>(body) {
        Ok(v) => Ok(v),
        // Some deployments answer a bare object for a single household.
        Err(e) => match serde_json::from_slice::<RawHousehold>(body) {
            Ok(one) => Ok(vec![one]),
            Err(_) => integration("die Haushaltsliste", e),
        },
    }
}

pub fn parse_user(body: &[u8]) -> Result<RawUser> {
    serde_json::from_slice(body).or_else(|e| integration("den angemeldeten Benutzer", e))
}

pub fn parse_expenses(body: &[u8]) -> Result<Vec<RawExpense>> {
    serde_json::from_slice(body).or_else(|e| integration("die Ausgabenliste", e))
}

pub fn parse_categories(body: &[u8]) -> Result<Vec<RawExpenseCategory>> {
    serde_json::from_slice(body).or_else(|e| integration("die Ausgabenkategorien", e))
}

pub fn parse_expense(body: &[u8]) -> Result<RawExpense> {
    serde_json::from_slice(body).or_else(|e| integration("die angelegte Ausgabe", e))
}

/// Epoch milliseconds to the calendar date a German user would write down.
///
/// VERIFY: the corpus cannot settle the timezone on its own — no expense falls in
/// the 00:00–02:00 Berlin window where UTC and Berlin dates differ. The Berlin-local
/// hour histogram runs 06:00–23:00 with a 17:00–19:00 peak and never touches the
/// small hours, which is a shopping-hours distribution in local time, so the value
/// is a true instant and is rendered in Europe/Berlin. If KitchenOwl ever stores a
/// date-only value as UTC midnight, this is the function that changes.
pub fn date_from_epoch_ms(ms: i64) -> Result<NaiveDate> {
    match Utc.timestamp_millis_opt(ms).single() {
        Some(dt) => Ok(dt.with_timezone(&Berlin).date_naive()),
        None => Err(AppError::Integration(format!(
            "KitchenOwl-Zeitstempel liegt außerhalb des darstellbaren Bereichs: {ms}"
        ))),
    }
}

/// The inverse, at noon Berlin time.
///
/// Noon rather than midnight deliberately: a midnight-local timestamp lands on the
/// previous day for anyone reading it in UTC, and the one thing a pushed expense
/// must not do is show up on the wrong day in the other household member's app.
pub fn epoch_ms_from_date(date: NaiveDate) -> i64 {
    date.and_hms_opt(12, 0, 0)
        .and_then(|naive| Berlin.from_local_datetime(&naive).single())
        .map(|dt| dt.timestamp_millis())
        // A date with no valid noon does not exist in any timezone Berlin has ever
        // used, but falling back to UTC noon beats panicking in a sync loop.
        .unwrap_or_else(|| {
            date.and_hms_opt(12, 0, 0)
                .unwrap_or_default()
                .and_utc()
                .timestamp_millis()
        })
}

pub fn datetime_from_epoch_ms(ms: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_millis_opt(ms).single()
}

/// Splits `total_cents` across integer weights so the parts sum to the total
/// **exactly**, by largest remainder.
///
/// Naive per-share rounding is what makes a 10,00 € three-way split show up as
/// 3×3,33 € = 9,99 € and leaves a cent unaccounted for in a ledger whose entire
/// value is being correct. Ties go to the lower member id, so the allocation is
/// stable across syncs and the mirror does not churn.
pub fn allocate_shares(total_cents: i64, weights: &[(i64, i64)]) -> Vec<Share> {
    let sum: i64 = weights.iter().map(|(_, f)| *f).sum();
    if weights.is_empty() || sum <= 0 {
        return weights
            .iter()
            .map(|(member_id, factor)| Share {
                member_id: *member_id,
                factor: *factor,
                share_cents: 0,
            })
            .collect();
    }

    let mut shares: Vec<Share> = weights
        .iter()
        .map(|(member_id, factor)| Share {
            member_id: *member_id,
            factor: *factor,
            // div_euclid floors for a positive divisor, so the remainder below is
            // never negative even if a future KitchenOwl emits a negative amount.
            share_cents: ((total_cents as i128 * *factor as i128).div_euclid(sum as i128)) as i64,
        })
        .collect();

    let mut remainder = total_cents - shares.iter().map(|s| s.share_cents).sum::<i64>();
    if remainder == 0 {
        return shares;
    }

    let mut order: Vec<usize> = (0..shares.len()).collect();
    order.sort_by_key(|&i| {
        let frac = (total_cents as i128 * shares[i].factor as i128).rem_euclid(sum as i128);
        (std::cmp::Reverse(frac), shares[i].member_id)
    });
    let mut k = 0usize;
    while remainder > 0 && !order.is_empty() {
        shares[order[k % order.len()]].share_cents += 1;
        remainder -= 1;
        k += 1;
    }
    shares
}

/// A KitchenOwl expense becomes a mirror row. The only place a KitchenOwl float
/// becomes cents.
pub fn to_mirror(raw: &RawExpense, me: i64) -> Result<MirrorExpense> {
    let amount_cents = cents_from_f64(raw.amount)?;
    let expense_date = date_from_epoch_ms(raw.date)?;

    let weights: Vec<(i64, i64)> = raw.paid_for.iter().map(|p| (p.user_id, p.factor)).collect();
    let shares = allocate_shares(amount_cents, &weights);
    let own_share_cents = shares
        .iter()
        .filter(|s| s.member_id == me)
        .map(|s| s.share_cents)
        .sum();

    // `-1` is the overview's bucket key for "no category"; it never appears as a
    // value here, but folding it into None costs nothing and removes a trap.
    let ko_category_id = raw.category_id.filter(|id| *id > 0);
    let ko_category_name = raw
        .category
        .as_ref()
        .map(|c| c.name.clone())
        .filter(|_| ko_category_id.is_some());

    let description = raw
        .description
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(str::to_string);

    let mut expense = MirrorExpense {
        external_id: raw.id,
        name: raw.name.trim().to_string(),
        description,
        expense_date,
        amount_cents,
        own_share_cents,
        paid_by_id: raw.paid_by_id,
        shares,
        ko_category_id,
        ko_category_name,
        exclude_from_statistics: raw.exclude_from_statistics,
        remote_hash: String::new(),
    };
    expense.remote_hash = hash_expense(&expense);
    Ok(expense)
}

fn hash_expense(e: &MirrorExpense) -> String {
    let mut hasher = Sha256::new();
    hasher.update(e.external_id.to_le_bytes());
    hasher.update(e.name.as_bytes());
    hasher.update([0]);
    hasher.update(e.description.as_deref().unwrap_or("").as_bytes());
    hasher.update([0]);
    hasher.update(e.expense_date.to_string().as_bytes());
    hasher.update(e.amount_cents.to_le_bytes());
    hasher.update(e.own_share_cents.to_le_bytes());
    hasher.update(e.paid_by_id.unwrap_or(0).to_le_bytes());
    hasher.update(e.ko_category_id.unwrap_or(0).to_le_bytes());
    hasher.update(e.ko_category_name.as_deref().unwrap_or("").as_bytes());
    hasher.update([u8::from(e.exclude_from_statistics)]);
    for s in &e.shares {
        hasher.update(s.member_id.to_le_bytes());
        hasher.update(s.factor.to_le_bytes());
        hasher.update(s.share_cents.to_le_bytes());
    }
    hex::encode(hasher.finalize())
}

pub fn to_member(raw: &RawMember, me: i64) -> Result<(MirrorMember, bool)> {
    let balance_cents = match raw.expense_balance {
        Some(v) => cents_from_f64(v)?,
        None => 0,
    };
    Ok((
        MirrorMember {
            member_id: raw.id,
            name: raw.name.trim().to_string(),
            username: raw.username.clone(),
            balance_cents,
            is_owner: raw.owner,
            is_admin: raw.admin,
        },
        raw.id == me,
    ))
}

pub fn to_category(raw: &RawExpenseCategory) -> Result<MirrorCategory> {
    Ok(MirrorCategory {
        category_id: raw.id,
        name: raw.name.trim().to_string(),
        color_argb: raw.color,
        budget_cents: raw.budget.map(cents_from_f64).transpose()?,
    })
}

/// The cursor for the next page.
///
/// **The ordering is by `date` descending, not by id** — observed live: expense 464
/// sits between 448 and 447 because it was entered late and back-dated. So the
/// cursor must be the id of the **last item in the returned order**, not the lowest
/// id on the page. Using the lowest id resumes at that row's position in date order,
/// which can be near the top of the page just delivered and makes the pull crawl
/// forward a few rows at a time.
///
/// Returns `None` for an empty page or when the cursor would not advance, which is
/// the loop's stop condition and its guard against spinning forever.
pub fn next_cursor(page: &[RawExpense], previous: Option<i64>) -> Option<i64> {
    let last = page.last()?.id;
    if previous == Some(last) {
        None
    } else {
        Some(last)
    }
}

// ------------------------------------------------------------------- push

/// The marker appended to a pushed expense so a retry can recognise its own work.
pub fn push_marker(booking_id: uuid::Uuid) -> String {
    format!("#fin:{}", &booking_id.simple().to_string()[..8])
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PushPayload {
    pub name: String,
    pub amount_cents: i64,
    pub date: NaiveDate,
    pub description: Option<String>,
    pub ko_category_id: Option<i64>,
    pub paid_by_id: i64,
    pub paid_for: Vec<ShareJson>,
    pub marker: String,
    #[serde(default)]
    pub marker_in_name: bool,
}

/// Builds the POST body.
///
/// VERIFY: **no write was ever made against the live instance**, so every field name
/// below is the GET spelling assumed to round-trip, which is the usual shape for
/// this API's Flask/marshmallow handlers but is not observed. If the shape is wrong
/// the instance answers `400 Request invalid` as plain text, the intent lands in
/// `failed` with that body as its visible error, and nothing is double-posted — so
/// the failure mode of guessing wrong is loud and safe rather than silent.
///
/// VERIFY: whether `category_id: null` clears a category on update. Not exercised:
/// push only ever creates.
pub fn push_body(payload: &PushPayload) -> serde_json::Value {
    let description = marked_description(payload);
    serde_json::json!({
        "name": marked_name(payload),
        "amount": payload.amount_cents as f64 / 100.0,
        "date": epoch_ms_from_date(payload.date),
        "description": description,
        "category_id": payload.ko_category_id,
        "paid_by_id": payload.paid_by_id,
        "paid_for": payload.paid_for.iter().map(|s| serde_json::json!({
            "user_id": s.member_id,
            "factor": s.factor,
        })).collect::<Vec<_>>(),
        "exclude_from_statistics": false,
    })
}

/// The marker goes in the description by default. `KITCHENOWL_PUSH_MARKER_IN_NAME`
/// moves it into the name for instances that drop descriptions — at the cost of the
/// other household member seeing it, which is why it is not the default.
pub fn marked_name(payload: &PushPayload) -> String {
    if payload.marker_in_name {
        format!("{} {}", payload.name.trim(), payload.marker)
    } else {
        payload.name.trim().to_string()
    }
}

pub fn marked_description(payload: &PushPayload) -> String {
    if payload.marker_in_name {
        payload.description.clone().unwrap_or_default()
    } else {
        match payload
            .description
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
        {
            Some(d) => format!("{d} {}", payload.marker),
            None => payload.marker.clone(),
        }
    }
}

/// Finds an expense this application already posted, by its marker.
///
/// This is the third idempotency layer and the one that covers the genuinely
/// ambiguous case: the POST timed out, so the expense may or may not exist. Scanning
/// for the marker is the only way to tell without asking KitchenOwl to create a
/// second one.
pub fn find_marked(page: &[RawExpense], marker: &str) -> Option<i64> {
    page.iter()
        .find(|e| {
            e.description.as_deref().is_some_and(|d| d.contains(marker)) || e.name.contains(marker)
        })
        .map(|e| e.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = include_str!("../../tests/fixtures/kitchenowl/expenses_page.json");
    const HOUSEHOLD: &str = include_str!("../../tests/fixtures/kitchenowl/household.json");
    const CATEGORIES: &str = include_str!("../../tests/fixtures/kitchenowl/categories.json");

    #[test]
    fn an_uncategorised_expense_has_neither_key_and_still_parses() {
        // 173 of 464 live expenses look exactly like this: `category` absent
        // entirely (not null) and `category_id` present but null. Anything that
        // requires either key fails on more than a third of the corpus.
        let raw: RawExpense = serde_json::from_str(
            r#"{"id":9,"name":"Kiosk","description":"","amount":4.2,"date":1786202059648,
                "category_id":null,"paid_by_id":1,
                "paid_for":[{"user_id":1,"factor":1}],"exclude_from_statistics":false}"#,
        )
        .unwrap();
        assert!(raw.category.is_none());
        let mirror = to_mirror(&raw, 1).unwrap();
        assert_eq!(mirror.ko_category_id, None);
        assert_eq!(mirror.ko_category_name, None);
        assert_eq!(mirror.amount_cents, 420);
    }

    #[test]
    fn the_overview_minus_one_bucket_key_is_not_a_category_id() {
        let raw: RawExpense = serde_json::from_str(
            r#"{"id":9,"name":"x","amount":1.0,"date":1786202059648,"category_id":-1,
                "paid_by_id":1,"paid_for":[{"user_id":1,"factor":1}]}"#,
        )
        .unwrap();
        assert_eq!(to_mirror(&raw, 1).unwrap().ko_category_id, None);
    }

    #[test]
    fn float_artifacts_become_exact_cents() {
        // Verbatim from the live instance.
        let raw: RawExpense = serde_json::from_str(
            r#"{"id":18,"name":"x","amount":213.87999999999994,"date":1786202059648,
                "category_id":null,"paid_by_id":1,"paid_for":[{"user_id":1,"factor":1}]}"#,
        )
        .unwrap();
        assert_eq!(to_mirror(&raw, 1).unwrap().amount_cents, 21388);
    }

    #[test]
    fn a_balance_artifact_becomes_exact_cents() {
        let raw: RawMember = serde_json::from_str(
            r#"{"id":1,"name":"Fabi","username":"fabi",
                "expense_balance":-142.26999999999217,"owner":true,"admin":false}"#,
        )
        .unwrap();
        let (member, is_me) = to_member(&raw, 1).unwrap();
        assert_eq!(member.balance_cents, -14227);
        assert!(is_me);
        assert!(member.is_owner);
        assert!(!member.is_admin);
    }

    #[test]
    fn integer_weights_are_weights_and_not_percentages() {
        // Observed live on expense 431: 19,00 € split 12 : 7.
        let shares = allocate_shares(1900, &[(1, 12), (2, 7)]);
        assert_eq!(shares[0].share_cents, 1200);
        assert_eq!(shares[1].share_cents, 700);
        assert_eq!(shares.iter().map(|s| s.share_cents).sum::<i64>(), 1900);
    }

    #[test]
    fn shares_sum_to_the_total_exactly_under_largest_remainder() {
        // A three-way split of 10,00 € is the canonical lost-cent case: naive
        // rounding gives 3 x 3,33 = 9,99.
        for (total, weights) in [
            (1000i64, vec![(1i64, 1i64), (2, 1), (3, 1)]),
            (1381, vec![(1, 1), (2, 1)]),
            (1, vec![(1, 1), (2, 1)]),
            (100, vec![(1, 1), (2, 2), (3, 3), (4, 5)]),
            (0, vec![(1, 1), (2, 1)]),
            (811381, vec![(1, 1), (2, 1)]),
        ] {
            let shares = allocate_shares(total, &weights);
            assert_eq!(
                shares.iter().map(|s| s.share_cents).sum::<i64>(),
                total,
                "total {total} weights {weights:?}"
            );
        }
        let three = allocate_shares(1000, &[(1, 1), (2, 1), (3, 1)]);
        assert_eq!(
            three.iter().map(|s| s.share_cents).collect::<Vec<_>>(),
            vec![334, 333, 333],
            "the spare cent goes to the lowest member id, deterministically"
        );
    }

    #[test]
    fn an_empty_or_zero_weighted_split_is_not_a_division_by_zero() {
        assert!(allocate_shares(500, &[]).is_empty());
        let zero = allocate_shares(500, &[(1, 0), (2, 0)]);
        assert!(zero.iter().all(|s| s.share_cents == 0));
    }

    #[test]
    fn own_share_uses_my_factor_over_the_sum_of_factors() {
        let raw: RawExpense = serde_json::from_str(
            r#"{"id":1,"name":"Kaufland","amount":19.07,"date":1787326931985,
                "category_id":1,"paid_by_id":2,
                "paid_for":[{"user_id":1,"factor":1},{"user_id":2,"factor":1}]}"#,
        )
        .unwrap();
        let mirror = to_mirror(&raw, 1).unwrap();
        // The full value is what the user records in the spreadsheet; the share is
        // the other number. Both are kept, neither replaces the other.
        assert_eq!(mirror.amount_cents, 1907);
        assert_eq!(mirror.own_share_cents, 954);
        assert_eq!(
            mirror.shares.iter().map(|s| s.share_cents).sum::<i64>(),
            1907
        );
    }

    #[test]
    fn a_solo_expense_by_someone_else_is_not_my_share() {
        let raw: RawExpense = serde_json::from_str(
            r#"{"id":1,"name":"x","amount":10.0,"date":1787326931985,"category_id":null,
                "paid_by_id":2,"paid_for":[{"user_id":2,"factor":1}]}"#,
        )
        .unwrap();
        assert_eq!(to_mirror(&raw, 1).unwrap().own_share_cents, 0);
    }

    #[test]
    fn the_hash_changes_only_when_a_mirrored_field_changes() {
        let base: RawExpense = serde_json::from_str(
            r#"{"id":1,"name":"Kaufland","amount":19.07,"date":1787326931985,
                "category_id":1,"paid_by_id":2,
                "paid_for":[{"user_id":1,"factor":1},{"user_id":2,"factor":1}]}"#,
        )
        .unwrap();
        let a = to_mirror(&base, 1).unwrap();
        let b = to_mirror(&base, 1).unwrap();
        assert_eq!(a.remote_hash, b.remote_hash, "re-parsing must be a no-op");

        let mut changed = base.clone();
        changed.amount = 19.08;
        assert_ne!(a.remote_hash, to_mirror(&changed, 1).unwrap().remote_hash);

        let mut renamed = base.clone();
        renamed.name = "Kaufland Süd".into();
        assert_ne!(a.remote_hash, to_mirror(&renamed, 1).unwrap().remote_hash);
    }

    #[test]
    fn dates_are_epoch_milliseconds_rendered_in_berlin() {
        assert_eq!(
            date_from_epoch_ms(1789223333900).unwrap(),
            NaiveDate::from_ymd_opt(2026, 9, 12).unwrap()
        );
        assert_eq!(
            date_from_epoch_ms(1734885945906).unwrap(),
            NaiveDate::from_ymd_opt(2024, 12, 22).unwrap()
        );
        // Round-trip through noon, which is what push sends.
        let d = NaiveDate::from_ymd_opt(2026, 3, 29).unwrap();
        assert_eq!(date_from_epoch_ms(epoch_ms_from_date(d)).unwrap(), d);
        // 2026-03-29 is the DST switch; 02:00 does not exist, noon does.
        let winter = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        assert_eq!(
            date_from_epoch_ms(epoch_ms_from_date(winter)).unwrap(),
            winter
        );
    }

    #[test]
    fn a_nonsense_timestamp_is_an_integration_error_not_a_panic() {
        assert!(date_from_epoch_ms(i64::MAX).is_err());
    }

    #[test]
    fn the_cursor_is_the_last_item_in_date_order_not_the_lowest_id() {
        // This exact page shape is why: 464 was back-dated, so it sits below 448
        // in date order while carrying a higher id. Taking min(id) would resume at
        // 447's position and re-deliver most of the page on every pass.
        let page = parse_expenses(PAGE.as_bytes()).unwrap();
        let lowest = page.iter().map(|e| e.id).min().unwrap();
        let last = page.last().unwrap().id;
        assert_ne!(lowest, last, "the fixture must keep the back-dated expense");
        assert_eq!(next_cursor(&page, None), Some(last));
        // A page that would not advance the cursor stops the loop.
        assert_eq!(next_cursor(&page, Some(last)), None);
        assert_eq!(next_cursor(&[], None), None);
    }

    #[test]
    fn the_whole_fixture_page_maps_without_loss() {
        let page = parse_expenses(PAGE.as_bytes()).unwrap();
        assert_eq!(page.len(), 30);
        let mirrored: Vec<MirrorExpense> = page.iter().map(|e| to_mirror(e, 1).unwrap()).collect();
        assert_eq!(mirrored.len(), 30);
        assert!(
            mirrored.iter().any(|m| m.ko_category_id.is_none()),
            "the uncategorised state must be represented"
        );
        for m in &mirrored {
            assert_eq!(
                m.shares.iter().map(|s| s.share_cents).sum::<i64>(),
                m.amount_cents
            );
            assert!(m.own_share_cents <= m.amount_cents);
        }
    }

    #[test]
    fn the_household_fixture_yields_balances_and_identifies_me() {
        let households = parse_households(HOUSEHOLD.as_bytes()).unwrap();
        assert_eq!(households.len(), 1);
        let h = &households[0];
        assert!(h.expenses_feature);
        assert_eq!(h.member.len(), 2);
        let me: Vec<_> = h
            .member
            .iter()
            .map(|m| to_member(m, 1).unwrap())
            .filter(|(_, is_me)| *is_me)
            .collect();
        assert_eq!(me.len(), 1);
        // The two balances are equal and opposite; the app never adds them to
        // anything else.
        let sum: i64 = h
            .member
            .iter()
            .map(|m| to_member(m, 1).unwrap().0.balance_cents)
            .sum();
        assert_eq!(sum, 0);
    }

    #[test]
    fn categories_parse_with_a_null_budget_and_a_packed_argb_colour() {
        let cats = parse_categories(CATEGORIES.as_bytes()).unwrap();
        assert_eq!(cats.len(), 7);
        let mapped: Vec<MirrorCategory> = cats.iter().map(|c| to_category(c).unwrap()).collect();
        assert!(mapped.iter().all(|c| c.budget_cents.is_none()));
        assert!(mapped.iter().any(|c| c.color_argb == Some(4289003611)));
    }

    #[test]
    fn a_non_json_error_body_is_an_integration_error_not_a_panic() {
        // The live instance answers `?limit=5` with text/html "Request invalid".
        assert!(matches!(
            parse_expenses(b"Request invalid"),
            Err(AppError::Integration(_))
        ));
        assert!(matches!(
            parse_households(b"Requested resource not found"),
            Err(AppError::Integration(_))
        ));
        // And a bad token answers 422 with JSON that is not an expense list.
        assert!(parse_expenses(br#"{"msg":"Not enough segments"}"#).is_err());
    }

    #[test]
    fn the_marker_is_stable_and_short_enough_to_live_in_a_description() {
        let id = uuid::Uuid::parse_str("2f1c6f6e-4b7a-4a5e-9a1e-0f1b2c3d4e5f").unwrap();
        let marker = push_marker(id);
        assert_eq!(marker, "#fin:2f1c6f6e");
        assert_eq!(push_marker(id), marker);
    }

    #[test]
    fn the_marker_lands_in_the_description_by_default_and_in_the_name_on_request() {
        let mut payload = PushPayload {
            name: "Kaufland".into(),
            amount_cents: 1907,
            date: NaiveDate::from_ymd_opt(2026, 5, 3).unwrap(),
            description: None,
            ko_category_id: Some(1),
            paid_by_id: 1,
            paid_for: vec![
                ShareJson {
                    member_id: 1,
                    factor: 1,
                    share_cents: 954,
                },
                ShareJson {
                    member_id: 2,
                    factor: 1,
                    share_cents: 953,
                },
            ],
            marker: "#fin:abc12345".into(),
            marker_in_name: false,
        };
        assert_eq!(marked_name(&payload), "Kaufland");
        assert_eq!(marked_description(&payload), "#fin:abc12345");

        payload.description = Some("Wocheneinkauf".into());
        assert_eq!(marked_description(&payload), "Wocheneinkauf #fin:abc12345");

        payload.marker_in_name = true;
        assert_eq!(marked_name(&payload), "Kaufland #fin:abc12345");
        assert_eq!(marked_description(&payload), "Wocheneinkauf");

        let body = push_body(&payload);
        assert_eq!(body["amount"], serde_json::json!(19.07));
        assert_eq!(body["paid_for"][0]["factor"], serde_json::json!(1));
        assert_eq!(body["category_id"], serde_json::json!(1));
    }

    #[test]
    fn a_pushed_expense_is_recognised_again_by_its_marker() {
        let page = parse_expenses(PAGE.as_bytes()).unwrap();
        assert_eq!(find_marked(&page, "#fin:deadbeef"), None);

        let mut with_marker = page.clone();
        with_marker[3].description = Some("Wocheneinkauf #fin:deadbeef".into());
        assert_eq!(find_marked(&with_marker, "#fin:deadbeef"), Some(page[3].id));

        // The name variant, for KITCHENOWL_PUSH_MARKER_IN_NAME.
        let mut in_name = page.clone();
        in_name[7].name = "Kaufland #fin:deadbeef".into();
        assert_eq!(find_marked(&in_name, "#fin:deadbeef"), Some(page[7].id));
    }
}
