//! Filing the household's uncategorised expenses.
//!
//! 173 of the mirrored expenses carry no KitchenOwl category — a third of the
//! corpus, 5.309,35 € of household spending — which makes "Ohne Kategorie" the
//! largest slice of every household analysis and the least informative one.
//!
//! This is the second place in the project that writes to KitchenOwl, and the first
//! that modifies data already there. Three properties make that defensible:
//!
//! **One field.** [`super::client::KoClient::update_expense_category`] reads the
//! expense, changes the category, and posts every other field back exactly as
//! KitchenOwl handed it over. Amounts, names, dates, payers and splits are echoed,
//! not rebuilt — this is a shared household and the other member's data is on the
//! other side of that call.
//!
//! **KitchenOwl first, mirror second.** A row is only re-mirrored after KitchenOwl
//! has accepted the change and returned the expense. A failure leaves the row
//! untagged and is reported per expense, because a mirror claiming a category
//! KitchenOwl never accepted would be a lie that survives every later sync.
//!
//! **Nothing automatic.** No loop tags anything; there is no "tag everything that
//! looks like a supermarket" button. The queue suggests, a person decides, and the
//! suggestion always says where it came from.
//!
//! The queue is grouped by NAME, because that is the shape of the decision: sixteen
//! Kaufland receipts are one judgement about Kaufland.

use std::collections::HashMap;

use axum::{Json, extract::State};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::{
    AppState,
    auth::Ctx,
    error::{AppError, Result},
    models::{KoTagFailure, KoTagRequest, KoTagResult, KoTagSuggestion, KoUntaggedGroup},
    tenant::Tenant,
};

use super::{client::KoClient, mirror, wire};

/// Standing corrections, applied ahead of every other signal.
///
/// The mirror's own history is normally the best evidence there is — it is the
/// user's own past decision, made with the receipt in hand. A user instruction
/// still outranks it: Hornbach sits under `Hobbies` twice in this household's
/// history and the user says it belongs in `Haushalt`. A builders' merchant is
/// household, and two old clicks are not an argument against that.
///
/// Keyed on the folded name, exactly as every other lookup here is, so adding the
/// next correction is one line. Category by NAME rather than id: ids are per
/// instance, names are what the user said.
const OVERRIDES: &[(&str, &str)] = &[("hornbach", "Haushalt")];

/// Untagged, unarchived, and therefore still changeable.
///
/// Archived expenses are excluded on purpose: they were deleted in KitchenOwl, so a
/// `POST` against them would 404 and there is nothing to file.
const UNTAGGED: &str = "e.user_id = app.current_user_id() \
                        AND e.ko_category_id IS NULL AND e.archived_at IS NULL";

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UntaggedQuery {
    /// Optional. Absent means every year the mirror holds, which is the default the
    /// queue wants: a backlog is not a calendar question.
    pub year: Option<i32>,
}

/// The queue: one row per name, most frequent first.
#[utoipa::path(
    get,
    path = "/api/v1/kitchenowl/untagged",
    tag = "kitchenowl",
    params(("year" = Option<i32>, Query, description = "Kalenderjahr; ohne Angabe alle")),
    responses((status = 200, description = "Ausgaben ohne Kategorie, nach Name gruppiert", body = Vec<KoUntaggedGroup>)),
)]
pub async fn untagged(
    mut ctx: Ctx,
    axum::extract::Query(q): axum::extract::Query<UntaggedQuery>,
) -> Result<Json<Vec<KoUntaggedGroup>>> {
    let year_filter = match q.year {
        Some(_) => " AND EXTRACT(YEAR FROM e.expense_date)::int = $1",
        None => "",
    };
    let sql = format!(
        "SELECT lower(btrim(e.name)) AS key, \
                (array_agg(e.name ORDER BY e.expense_date DESC))[1] AS name, \
                count(*)::bigint AS n, \
                COALESCE(SUM(e.amount_cents), 0)::bigint AS amount, \
                COALESCE(SUM(e.own_share_cents), 0)::bigint AS own, \
                min(e.expense_date) AS first_date, \
                max(e.expense_date) AS last_date \
           FROM ko_expenses e \
          WHERE {UNTAGGED}{year_filter} \
          GROUP BY lower(btrim(e.name)) \
          ORDER BY n DESC, amount DESC, key"
    );
    let mut query = sqlx::query(&sql);
    if let Some(year) = q.year {
        query = query.bind(year);
    }
    let rows = query.fetch_all(ctx.tenant.conn()).await?;

    let precedents = precedents(ctx.tenant.conn()).await?;
    let by_rule = rule_suggestions(ctx.tenant.conn()).await?;
    let by_name = categories_by_name(ctx.tenant.conn()).await?;

    let out = rows
        .iter()
        .map(|r| {
            let key: String = r.get("key");
            KoUntaggedGroup {
                suggestion: suggest(&key, &precedents, &by_rule, &by_name),
                name: r.get("name"),
                match_key: key,
                expense_count: r.get("n"),
                amount_cents: r.get("amount"),
                own_share_cents: r.get("own"),
                first_date: r.get("first_date"),
                last_date: r.get("last_date"),
            }
        })
        .collect();
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

/// Override, then precedent, then the rule table. Never a guess.
fn suggest(
    key: &str,
    precedents: &HashMap<String, (i64, String, i64)>,
    by_rule: &HashMap<String, (i64, String)>,
    by_name: &HashMap<String, i64>,
) -> Option<KoTagSuggestion> {
    // An override settles the question outright: if it names a category this
    // instance does not have, the answer is "no suggestion", never the weaker
    // signal the user has just overruled.
    if let Some((_, name)) = OVERRIDES.iter().find(|(k, _)| *k == key) {
        return by_name.get(&name.to_lowercase()).map(|id| KoTagSuggestion {
            ko_category_id: *id,
            ko_category_name: (*name).to_string(),
            source: "override".into(),
            times_seen: 0,
        });
    }
    if let Some((id, name, seen)) = precedents.get(key) {
        return Some(KoTagSuggestion {
            ko_category_id: *id,
            ko_category_name: name.clone(),
            source: "precedent".into(),
            times_seen: *seen,
        });
    }
    by_rule.get(key).map(|(id, name)| KoTagSuggestion {
        ko_category_id: *id,
        ko_category_name: name.clone(),
        source: "rule".into(),
        times_seen: 0,
    })
}

/// What this household already filed the same name under, and how often.
///
/// Deliberately unfiltered by year: a decision made in 2025 is still the user's
/// decision about Kaufland, and the backlog being tagged is mostly older anyway.
/// The most-used category wins, ties broken by the most recent use.
async fn precedents(conn: &mut PgConnection) -> Result<HashMap<String, (i64, String, i64)>> {
    let rows = sqlx::query(
        "SELECT lower(btrim(e.name)) AS key, e.ko_category_id AS cat, \
                (array_agg(e.ko_category_name ORDER BY e.expense_date DESC))[1] AS cat_name, \
                count(*)::bigint AS n, max(e.expense_date) AS last_date \
           FROM ko_expenses e \
          WHERE e.ko_category_id IS NOT NULL AND e.archived_at IS NULL \
          GROUP BY lower(btrim(e.name)), e.ko_category_id \
          ORDER BY n DESC, last_date DESC",
    )
    .fetch_all(&mut *conn)
    .await?;

    let mut best: HashMap<String, (i64, String, i64)> = HashMap::new();
    for r in &rows {
        let key: String = r.get("key");
        let n: i64 = r.get("n");
        let name: Option<String> = r.get("cat_name");
        // Ordered strongest first, so the first entry per key is the winner and
        // later ones are ignored.
        best.entry(key)
            .or_insert_with(|| (r.get("cat"), name.unwrap_or_default(), n));
    }
    Ok(best)
}

/// The personal rule table, projected onto KitchenOwl's categories.
///
/// Only reaches a suggestion where the user has already mapped a finance category
/// to a KitchenOwl one (`ko_categories.finances_category_id`). That map is a hint
/// the user maintains, not an automatic correspondence — the two taxonomies are
/// unrelated and nothing here invents a link between them.
async fn rule_suggestions(conn: &mut PgConnection) -> Result<HashMap<String, (i64, String)>> {
    let rows = sqlx::query(
        "SELECT r.match_key AS key, k.category_id AS cat, k.name AS cat_name \
           FROM category_rules r \
           JOIN ko_categories k ON k.finances_category_id = r.category_id",
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                r.get::<String, _>("key"),
                (r.get("cat"), r.get::<String, _>("cat_name")),
            )
        })
        .collect())
}

/// Folded category name to id, for the override table and for validating a request.
async fn categories_by_name(conn: &mut PgConnection) -> Result<HashMap<String, i64>> {
    let rows = sqlx::query("SELECT category_id, name FROM ko_categories")
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                r.get::<String, _>("name").trim().to_lowercase(),
                r.get::<i64, _>("category_id"),
            )
        })
        .collect())
}

struct Target {
    external_id: i64,
    name: String,
    current: Option<i64>,
}

/// Files every expense of one name — or an explicit set — under one category.
#[utoipa::path(
    post,
    path = "/api/v1/kitchenowl/untagged/apply",
    tag = "kitchenowl",
    request_body = KoTagRequest,
    responses(
        (status = 200, description = "Je Ausgabe: gesetzt, übersprungen oder mit Grund gescheitert", body = KoTagResult),
        (status = 400, description = "Weder Name noch Auswahl, oder unbekannte Kategorie"),
        (status = 502, description = "KitchenOwl nicht erreichbar"),
    ),
)]
pub async fn apply(
    State(state): State<AppState>,
    mut ctx: Ctx,
    Json(body): Json<KoTagRequest>,
) -> Result<Json<KoTagResult>> {
    let Some(client) = KoClient::from_state(&state) else {
        return Err(AppError::Integration(
            "KitchenOwl ist auf diesem Server nicht konfiguriert".into(),
        ));
    };

    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty());
    let ids = body.expense_ids.as_deref().filter(|v| !v.is_empty());
    let (name, ids) = match (name, ids) {
        (Some(n), None) => (Some(n), None),
        (None, Some(v)) => (None, Some(v)),
        _ => {
            return Err(AppError::Validation(
                "Genau eines von name oder expenseIds angeben".into(),
            ));
        }
    };

    // The category must exist in the mirrored list. KitchenOwl would accept an
    // unknown id and file the expense under nothing visible, which looks like a
    // success and reads as a disappearance.
    let category_name: Option<String> =
        sqlx::query_scalar("SELECT name FROM ko_categories WHERE category_id = $1")
            .bind(body.ko_category_id)
            .fetch_optional(ctx.tenant.conn())
            .await?;
    let Some(category_name) = category_name else {
        return Err(AppError::Validation(format!(
            "Unbekannte KitchenOwl-Kategorie: {}",
            body.ko_category_id
        )));
    };

    let targets = match (name, ids) {
        (Some(n), _) => {
            sqlx::query(
                "SELECT e.external_id, e.name, e.ko_category_id \
                   FROM ko_expenses e \
                  WHERE e.archived_at IS NULL AND lower(btrim(e.name)) = lower(btrim($1)) \
                    AND e.ko_category_id IS NULL \
                  ORDER BY e.expense_date, e.external_id",
            )
            .bind(n)
            .fetch_all(ctx.tenant.conn())
            .await?
        }
        (_, Some(v)) => {
            sqlx::query(
                "SELECT e.external_id, e.name, e.ko_category_id \
                   FROM ko_expenses e \
                  WHERE e.archived_at IS NULL AND e.id = ANY($1) \
                  ORDER BY e.expense_date, e.external_id",
            )
            .bind(v)
            .fetch_all(ctx.tenant.conn())
            .await?
        }
        _ => unreachable!("validated above"),
    };
    let targets: Vec<Target> = targets
        .iter()
        .map(|r| Target {
            external_id: r.get("external_id"),
            name: r.get("name"),
            current: r.get("ko_category_id"),
        })
        .collect();

    // Whose share is whose, for re-mirroring the expense afterwards. Read from the
    // mirror rather than asked of KitchenOwl: it is already synced, and this loop
    // makes enough round trips as it is.
    let me: Option<i64> =
        sqlx::query_scalar("SELECT member_id FROM ko_members WHERE is_me ORDER BY member_id")
            .fetch_optional(ctx.tenant.conn())
            .await?;
    let me = me.unwrap_or(0);
    let user_id = ctx.user.id;

    // Committed before the first network call. Everything after this point runs one
    // expense at a time, each in its own short transaction, so a failure halfway
    // through leaves the expenses before it correctly filed rather than rolling the
    // whole batch back out of a system that has already accepted it.
    ctx.tenant.commit().await?;

    let mut result = KoTagResult {
        ko_category_id: body.ko_category_id,
        ko_category_name: category_name,
        requested: targets.len() as i64,
        tagged: 0,
        skipped: 0,
        failed: 0,
        failures: Vec::new(),
    };

    for target in &targets {
        if target.current == Some(body.ko_category_id) {
            result.skipped += 1;
            continue;
        }
        match tag_one(
            &state,
            &client,
            user_id,
            target.external_id,
            body.ko_category_id,
            me,
        )
        .await
        {
            Ok(()) => result.tagged += 1,
            Err(e) => {
                result.failed += 1;
                result.failures.push(KoTagFailure {
                    external_id: target.external_id,
                    name: target.name.clone(),
                    error: e.to_string(),
                });
            }
        }
    }

    Ok(Json(result))
}

/// KitchenOwl first, mirror second — and the mirror is rebuilt from what KitchenOwl
/// returned, not from what was asked for.
///
/// Going through `to_mirror` + `upsert_expense` rather than a targeted `UPDATE …
/// SET ko_category_id` also keeps `remote_hash` correct, which is what makes the
/// next pull see an unchanged row and write nothing.
async fn tag_one(
    state: &AppState,
    client: &KoClient,
    user_id: Uuid,
    external_id: i64,
    ko_category_id: i64,
    me: i64,
) -> Result<()> {
    let raw = client
        .update_expense_category(external_id, Some(ko_category_id))
        .await?;
    let mirrored = wire::to_mirror(&raw, me)?;
    let mut tenant = Tenant::begin(&state.db, user_id).await?;
    mirror::upsert_expense(tenant.conn(), user_id, &mirrored).await?;
    tenant.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cats() -> HashMap<String, i64> {
        HashMap::from([
            ("haushalt".to_string(), 3),
            ("wocheneinkauf".to_string(), 1),
        ])
    }

    #[test]
    fn a_user_correction_outranks_the_households_own_history() {
        // Hornbach is filed under Hobbies twice in the real mirror; the user says
        // Haushalt. The instruction wins, and says so.
        let precedents = HashMap::from([("hornbach".to_string(), (7, "Hobbies".to_string(), 2))]);
        let s = suggest("hornbach", &precedents, &HashMap::new(), &cats()).expect("suggestion");
        assert_eq!(s.ko_category_name, "Haushalt");
        assert_eq!(s.source, "override");
    }

    #[test]
    fn precedent_beats_the_rule_table_and_carries_its_evidence() {
        let precedents =
            HashMap::from([("kaufland".to_string(), (1, "Wocheneinkauf".to_string(), 39))]);
        let by_rule = HashMap::from([("kaufland".to_string(), (3, "Haushalt".to_string()))]);
        let s = suggest("kaufland", &precedents, &by_rule, &cats()).expect("suggestion");
        assert_eq!(s.ko_category_id, 1);
        assert_eq!(s.source, "precedent");
        assert_eq!(s.times_seen, 39);
    }

    #[test]
    fn an_unknown_name_gets_no_suggestion_rather_than_a_guess() {
        assert!(suggest("padefke", &HashMap::new(), &HashMap::new(), &cats()).is_none());
    }

    #[test]
    fn an_override_naming_a_category_this_instance_lacks_suggests_nothing() {
        // Rather than inventing an id, or falling through to a weaker signal that
        // the user has already overruled.
        let precedents = HashMap::from([("hornbach".to_string(), (7, "Hobbies".to_string(), 2))]);
        let no_categories = HashMap::new();
        assert!(suggest("hornbach", &precedents, &HashMap::new(), &no_categories).is_none());
    }
}
