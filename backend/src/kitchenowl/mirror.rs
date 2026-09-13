//! The local mirror and the periodic pull.
//!
//! **A pull never writes a booking.** It writes `ko_expenses`, `ko_members`,
//! `ko_categories` and `ko_drafts`, and nothing else. The two ledgers are parallel,
//! never summed, and an expense that matches no booking is the normal case rather
//! than an error state.

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::error::{AppError, Result};

use super::{
    client::KoClient,
    matching,
    wire::{self, MirrorExpense},
};

/// What one sync run did. Mirrors the `sync_runs` columns so the bookkeeping and
/// the return value cannot drift apart.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SyncCounts {
    pub created: i64,
    pub updated: i64,
    pub archived: i64,
    pub failed: i64,
}

pub const KIND_EXPENSES: &str = "ko_expenses";
pub const KIND_METADATA: &str = "ko_metadata";
pub const KIND_PUSH: &str = "ko_push";

// ------------------------------------------------------------ sync_runs

pub async fn begin_run(conn: &mut PgConnection, user_id: Uuid, kind: &str) -> Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO sync_runs (id, user_id, kind, status) VALUES ($1, $2, $3, 'running')")
        .bind(id)
        .bind(user_id)
        .bind(kind)
        .execute(&mut *conn)
        .await?;
    Ok(id)
}

/// Closes a run. A failure is **recorded**, never swallowed: the status endpoint
/// reads the last run of each kind, so a broken sync is visible in the UI rather
/// than only in a log nobody reads.
pub async fn finish_run(
    conn: &mut PgConnection,
    run_id: Uuid,
    status: &str,
    counts: SyncCounts,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE sync_runs SET status = $2, finished_at = now(), created_count = $3, \
                updated_count = $4, archived_count = $5, failed_count = $6, error = $7 \
          WHERE id = $1",
    )
    .bind(run_id)
    .bind(status)
    .bind(counts.created as i32)
    .bind(counts.updated as i32)
    .bind(counts.archived as i32)
    .bind(counts.failed as i32)
    .bind(error)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// A user takes part in KitchenOwl sync iff they have a `ko_sync_state` row.
///
/// The credentials are process-global — one URL, one token — so mirroring for every
/// account would copy one household into other people's books. The row is created
/// only by an explicit "jetzt synchronisieren", which makes participation an opt-in
/// the user performed rather than a side effect of the server's environment.
pub async fn enable(conn: &mut PgConnection, user_id: Uuid) -> Result<()> {
    sqlx::query("INSERT INTO ko_sync_state (user_id) VALUES ($1) ON CONFLICT (user_id) DO NOTHING")
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
    // The same fact, in the one place a loop with no tenant can read it. Written in
    // the caller's transaction, so a user is never a participant for the background
    // loop without the state row the loop expects to find.
    sqlx::query(
        "INSERT INTO ko_participants (user_id) VALUES ($1) ON CONFLICT (user_id) DO NOTHING",
    )
    .bind(user_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn is_enabled(conn: &mut PgConnection) -> Result<bool> {
    let n: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM ko_sync_state")
        .fetch_one(&mut *conn)
        .await?;
    Ok(n > 0)
}

/// Every user who opted in.
///
/// Read from `ko_participants`, NOT from `ko_sync_state`: the background loop has no
/// request and therefore no `app.user_id`, and a tenant-scoped read in that state
/// returns an empty list instead of an error. This function is the one place that
/// distinction matters, and getting it wrong disables every automatic sync without
/// a single log line.
pub async fn participating_users(pool: &sqlx::PgPool) -> Result<Vec<Uuid>> {
    let rows = sqlx::query("SELECT user_id FROM ko_participants ORDER BY enabled_at")
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(|r| r.get::<Uuid, _>(0)).collect())
}

// --------------------------------------------------------------- metadata

/// Members and expense categories. Cached with `fetched_at` so they can be served
/// **stale with a warning rather than withheld** — a push dialogue that refuses to
/// open because KitchenOwl is down is worse than one that opens with yesterday's
/// member list and says so.
pub async fn sync_metadata(
    client: &KoClient,
    conn: &mut PgConnection,
    user_id: Uuid,
) -> Result<SyncCounts> {
    let me = client.me().await?;
    let households = client.households().await?;
    let household_id = client.household_id().await?;
    let household = households.iter().find(|h| h.id == household_id);

    let mut counts = SyncCounts::default();
    let mut seen_members: Vec<i64> = Vec::new();
    for raw in household.map(|h| h.member.as_slice()).unwrap_or_default() {
        let (member, is_me) = wire::to_member(raw, me)?;
        seen_members.push(member.member_id);
        let affected = sqlx::query(
            "INSERT INTO ko_members (user_id, member_id, name, username, is_admin, is_owner, \
                                     balance_cents, is_me, fetched_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8, now()) \
             ON CONFLICT (user_id, member_id) DO UPDATE SET \
               name = EXCLUDED.name, username = EXCLUDED.username, \
               is_admin = EXCLUDED.is_admin, is_owner = EXCLUDED.is_owner, \
               balance_cents = EXCLUDED.balance_cents, is_me = EXCLUDED.is_me, \
               fetched_at = now()",
        )
        .bind(user_id)
        .bind(member.member_id)
        .bind(&member.name)
        .bind(&member.username)
        .bind(member.is_admin)
        .bind(member.is_owner)
        .bind(member.balance_cents)
        .bind(is_me)
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if affected > 0 {
            counts.updated += 1;
        }
    }
    if !seen_members.is_empty() {
        counts.archived += sqlx::query("DELETE FROM ko_members WHERE member_id <> ALL($1)")
            .bind(&seen_members)
            .execute(&mut *conn)
            .await?
            .rows_affected() as i64;
    }

    let categories = client.categories().await?;
    let mut seen_categories: Vec<i64> = Vec::new();
    for raw in &categories {
        let category = wire::to_category(raw)?;
        seen_categories.push(category.category_id);
        // NOTE: finances_category_id is deliberately NOT written here. KitchenOwl's
        // seven categories and the app's 32 are different taxonomies; the column is
        // a hint the user may set, never something a sync decides.
        sqlx::query(
            "INSERT INTO ko_categories (user_id, category_id, name, color_argb, budget_cents, \
                                        fetched_at) \
             VALUES ($1,$2,$3,$4,$5, now()) \
             ON CONFLICT (user_id, category_id) DO UPDATE SET \
               name = EXCLUDED.name, color_argb = EXCLUDED.color_argb, \
               budget_cents = EXCLUDED.budget_cents, fetched_at = now()",
        )
        .bind(user_id)
        .bind(category.category_id)
        .bind(&category.name)
        .bind(category.color_argb)
        .bind(category.budget_cents)
        .execute(&mut *conn)
        .await?;
        counts.updated += 1;
    }
    if !seen_categories.is_empty() {
        counts.archived += sqlx::query("DELETE FROM ko_categories WHERE category_id <> ALL($1)")
            .bind(&seen_categories)
            .execute(&mut *conn)
            .await?
            .rows_affected() as i64;
    }

    sqlx::query(
        "UPDATE ko_sync_state SET household_id = $1, household_name = $2, \
                metadata_fetched_at = now(), last_error = NULL",
    )
    .bind(household_id)
    .bind(household.map(|h| h.name.as_str()))
    .execute(&mut *conn)
    .await?;

    Ok(counts)
}

// ------------------------------------------------------------- expenses

/// Mirrors every expense, page by page.
///
/// The scan is **complete by default** rather than incremental, and that is a
/// correction to the original design. KitchenOwl orders expenses by `date`
/// descending, not by id, so a back-dated expense entered today appears deep in the
/// list — stopping at a high-water-mark id would silently never see it. 464 expenses
/// is 16 pages; a full pass every poll costs less than a missing week of groceries.
/// `KITCHENOWL_MAX_PULL_PAGES` still caps it, and a capped scan is reported as
/// `partial` and may not archive.
pub async fn sync_expenses(
    client: &KoClient,
    conn: &mut PgConnection,
    user_id: Uuid,
    max_pages: u32,
    duplicate_threshold: f64,
) -> Result<(SyncCounts, bool)> {
    let me = client.me().await?;

    let mut cursor: Option<i64> = None;
    let mut pages = 0u32;
    let mut counts = SyncCounts::default();
    let mut seen: Vec<i64> = Vec::new();
    let mut complete = false;
    let mut max_id: i64 = 0;

    while pages < max_pages {
        let page = client.expense_page(cursor).await?;
        pages += 1;
        if page.is_empty() {
            complete = true;
            break;
        }
        let next = wire::next_cursor(&page, cursor);

        for raw in &page {
            max_id = max_id.max(raw.id);
            match wire::to_mirror(raw, me) {
                Ok(expense) => {
                    seen.push(expense.external_id);
                    match upsert_expense(conn, user_id, &expense).await? {
                        Upsert::Created(id) => {
                            counts.created += 1;
                            matching::create_draft(
                                conn,
                                user_id,
                                id,
                                &expense,
                                duplicate_threshold,
                            )
                            .await?;
                        }
                        Upsert::Updated(id) => {
                            counts.updated += 1;
                            matching::refresh_draft(
                                conn,
                                user_id,
                                id,
                                &expense,
                                duplicate_threshold,
                            )
                            .await?;
                        }
                        // The whole point of remote_hash: a re-sync of an untouched
                        // expense touches no row and creates no draft.
                        Upsert::Unchanged => {}
                    }
                }
                Err(e) => {
                    // One unreadable expense must not abandon the other 463.
                    counts.failed += 1;
                    tracing::warn!(expense = raw.id, error = %e, "KitchenOwl-Ausgabe übersprungen");
                }
            }
        }

        match next {
            Some(c) => cursor = Some(c),
            None => {
                complete = true;
                break;
            }
        }
    }

    if complete && !seen.is_empty() {
        counts.archived = sqlx::query(
            "UPDATE ko_expenses SET archived_at = now() \
              WHERE archived_at IS NULL AND external_id <> ALL($1)",
        )
        .bind(&seen)
        .execute(&mut *conn)
        .await?
        .rows_affected() as i64;
    }

    sqlx::query(
        "UPDATE ko_sync_state SET max_seen_id = GREATEST(max_seen_id, $1), last_error = NULL",
    )
    .bind(max_id)
    .execute(&mut *conn)
    .await?;

    Ok((counts, complete))
}

pub(crate) enum Upsert {
    Created(Uuid),
    Updated(Uuid),
    Unchanged,
}

/// Re-syncing an already-mirrored, unchanged expense writes nothing.
///
/// `WHERE ko_expenses.remote_hash IS DISTINCT FROM EXCLUDED.remote_hash` on the
/// conflict path is what makes that structural instead of a comparison somebody has
/// to remember: an unchanged row updates zero rows, so the RETURNING is empty and
/// the caller learns "unchanged" without a second query.
pub(crate) async fn upsert_expense(
    conn: &mut PgConnection,
    user_id: Uuid,
    e: &MirrorExpense,
) -> Result<Upsert> {
    let shares: Vec<wire::ShareJson> = e
        .shares
        .iter()
        .map(|s| wire::ShareJson {
            member_id: s.member_id,
            factor: s.factor,
            share_cents: s.share_cents,
        })
        .collect();
    let paid_for = serde_json::to_value(&shares).map_err(|err| {
        AppError::Internal(anyhow::anyhow!("paid_for nicht serialisierbar: {err}"))
    })?;

    let row = sqlx::query(
        "INSERT INTO ko_expenses (id, user_id, external_id, name, description, expense_date, \
                                  amount_cents, own_share_cents, paid_by_id, paid_for, \
                                  ko_category_id, ko_category_name, exclude_from_statistics, \
                                  remote_hash, updated_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14, now()) \
         ON CONFLICT (user_id, external_id) DO UPDATE SET \
           name = EXCLUDED.name, description = EXCLUDED.description, \
           expense_date = EXCLUDED.expense_date, amount_cents = EXCLUDED.amount_cents, \
           own_share_cents = EXCLUDED.own_share_cents, paid_by_id = EXCLUDED.paid_by_id, \
           paid_for = EXCLUDED.paid_for, ko_category_id = EXCLUDED.ko_category_id, \
           ko_category_name = EXCLUDED.ko_category_name, \
           exclude_from_statistics = EXCLUDED.exclude_from_statistics, \
           remote_hash = EXCLUDED.remote_hash, archived_at = NULL, updated_at = now() \
         WHERE ko_expenses.remote_hash IS DISTINCT FROM EXCLUDED.remote_hash \
            OR ko_expenses.archived_at IS NOT NULL \
         RETURNING id, (xmax = 0) AS inserted",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(e.external_id)
    .bind(&e.name)
    .bind(&e.description)
    .bind(e.expense_date)
    .bind(e.amount_cents)
    .bind(e.own_share_cents)
    .bind(e.paid_by_id)
    .bind(&paid_for)
    .bind(e.ko_category_id)
    .bind(&e.ko_category_name)
    .bind(e.exclude_from_statistics)
    .bind(&e.remote_hash)
    .fetch_optional(&mut *conn)
    .await
    .map_err(|err| AppError::from_db(err, "KitchenOwl-Ausgabe konnte nicht gespiegelt werden"))?;

    Ok(match row {
        Some(r) if r.get::<bool, _>("inserted") => Upsert::Created(r.get("id")),
        Some(r) => Upsert::Updated(r.get("id")),
        None => Upsert::Unchanged,
    })
}

pub async fn record_error(conn: &mut PgConnection, message: &str) -> Result<()> {
    sqlx::query("UPDATE ko_sync_state SET last_error = $1")
        .bind(message)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// The last run of a kind, for the status endpoint.
pub struct LastRun {
    pub id: Uuid,
    pub kind: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created: i64,
    pub updated: i64,
    pub archived: i64,
    pub failed: i64,
    pub error: Option<String>,
}

pub async fn last_run(conn: &mut PgConnection, kind: &str) -> Result<Option<LastRun>> {
    let row = sqlx::query(
        "SELECT id, kind, status, started_at, finished_at, created_count, updated_count, \
                archived_count, failed_count, error \
           FROM sync_runs WHERE kind = $1 ORDER BY started_at DESC LIMIT 1",
    )
    .bind(kind)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|r| LastRun {
        id: r.get("id"),
        kind: r.get("kind"),
        status: r.get("status"),
        started_at: r.get("started_at"),
        finished_at: r.get("finished_at"),
        created: r.get::<i32, _>("created_count") as i64,
        updated: r.get::<i32, _>("updated_count") as i64,
        archived: r.get::<i32, _>("archived_count") as i64,
        failed: r.get::<i32, _>("failed_count") as i64,
        error: r.get("error"),
    }))
}
