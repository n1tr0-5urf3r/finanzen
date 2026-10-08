//! HTTP surface for the KitchenOwl integration.
//!
//! Every handler here obeys two rules that come straight from the product decision:
//! no response mixes a KitchenOwl figure with a personal-booking figure, and no
//! handler writes a booking as a side effect of a pull.

use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{Datelike, Utc};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::{
    AppState,
    auth::Ctx,
    error::{AppError, Result},
    models::{
        KoCategory, KoDraft, KoDraftPage, KoExpense, KoExpensePage, KoLinkRequest,
        KoMatchCandidate, KoMember, KoMetadata, KoPushIntent, KoPushRequest, KoShare, KoStatus,
        KoSummary, KoSyncResult, KoSyncRun, Period,
    },
    tenant::Tenant,
};

use super::{client::KoClient, link, mirror, push, wire};

/// The expense projection, as a column list rather than a whole statement: the
/// draft queries select the same columns alongside their own, so a column added
/// here must not need a second edit in a string that happens to look similar.
const EXPENSE_COLUMNS: &str = "\
    e.id, e.external_id, e.name, e.description, e.expense_date, e.amount_cents, \
    e.own_share_cents, e.paid_by_id, e.paid_for, e.ko_category_id, e.ko_category_name, \
    e.exclude_from_statistics, e.archived_at, e.linked_booking_id, e.updated_at, \
    b.comment AS linked_comment, b.amount_cents AS linked_amount";

/// The draft queries join `ko_drafts` to this projection, so the draft's own
/// columns are aliased there: two columns both called `id` in one row means a
/// name lookup quietly returns whichever the driver saw last, which is how a
/// draft id becomes an expense id and a link 404s.
const EXPENSE_FROM: &str = "\
    FROM ko_expenses e LEFT JOIN bookings b ON b.id = e.linked_booking_id";

fn select_expense() -> String {
    format!("SELECT {EXPENSE_COLUMNS} {EXPENSE_FROM}")
}

async fn member_names(conn: &mut PgConnection) -> Result<HashMap<i64, String>> {
    let rows = sqlx::query("SELECT member_id, name FROM ko_members")
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows
        .iter()
        .map(|r| (r.get::<i64, _>("member_id"), r.get::<String, _>("name")))
        .collect())
}

fn row_to_expense(r: &sqlx::postgres::PgRow, names: &HashMap<i64, String>) -> KoExpense {
    let shares: Vec<wire::ShareJson> =
        serde_json::from_value(r.get("paid_for")).unwrap_or_default();
    let paid_by_id: Option<i64> = r.get("paid_by_id");
    KoExpense {
        id: r.get("id"),
        external_id: r.get("external_id"),
        name: r.get("name"),
        description: r.get("description"),
        date: r.get("expense_date"),
        amount_cents: r.get("amount_cents"),
        own_share_cents: r.get("own_share_cents"),
        paid_by_id,
        paid_by_name: paid_by_id.and_then(|id| names.get(&id).cloned()),
        paid_for: shares
            .into_iter()
            .map(|s| KoShare {
                name: names.get(&s.member_id).cloned(),
                member_id: s.member_id,
                factor: s.factor,
                share_cents: s.share_cents,
            })
            .collect(),
        ko_category_id: r.get("ko_category_id"),
        ko_category_name: r.get("ko_category_name"),
        exclude_from_statistics: r.get("exclude_from_statistics"),
        archived_at: r.get("archived_at"),
        linked_booking_id: r.get("linked_booking_id"),
        linked_booking_comment: r.get("linked_comment"),
        linked_booking_amount_cents: r.get("linked_amount"),
        updated_at: r.get("updated_at"),
    }
}

fn run_to_dto(run: mirror::LastRun) -> KoSyncRun {
    KoSyncRun {
        id: run.id,
        kind: run.kind,
        status: run.status,
        started_at: run.started_at,
        finished_at: run.finished_at,
        created_count: run.created,
        updated_count: run.updated,
        archived_count: run.archived,
        failed_count: run.failed,
        error: run.error,
    }
}

// ------------------------------------------------------------------ status

/// Members and categories for the push dialogue.
///
/// **Served stale with a warning rather than withheld.** The dialogue must open when
/// KitchenOwl is down, because the whole point of the outbox is that the user can
/// queue a push during an outage.
#[utoipa::path(
    get,
    path = "/api/v1/kitchenowl/status",
    tag = "kitchenowl",
    responses((status = 200, description = "Konfiguration, Erreichbarkeit, letzter und nächster Lauf, offene Posten", body = KoStatus)),
)]
pub async fn status(State(state): State<AppState>, mut ctx: Ctx) -> Result<Json<KoStatus>> {
    let conn = ctx.tenant.conn();
    let enabled = mirror::is_enabled(conn).await?;

    let sync_state = sqlx::query(
        "SELECT household_id, household_name, metadata_fetched_at, last_error \
           FROM ko_sync_state LIMIT 1",
    )
    .fetch_optional(&mut *conn)
    .await?;

    let counts = sqlx::query(
        "SELECT count(*) FILTER (WHERE archived_at IS NULL)::bigint AS mirrored, \
                count(*) FILTER (WHERE archived_at IS NOT NULL)::bigint AS archived, \
                count(*) FILTER (WHERE linked_booking_id IS NOT NULL)::bigint AS linked \
           FROM ko_expenses",
    )
    .fetch_one(&mut *conn)
    .await?;

    let drafts = sqlx::query(
        "SELECT count(*) FILTER (WHERE status IN ('open','possible_duplicate'))::bigint AS open, \
                count(*) FILTER (WHERE status = 'likely_duplicate')::bigint AS likely \
           FROM ko_drafts",
    )
    .fetch_one(&mut *conn)
    .await?;

    let pushes = sqlx::query(
        "SELECT count(*) FILTER (WHERE state IN ('queued','sending'))::bigint AS pending, \
                count(*) FILTER (WHERE state IN ('failed','abandoned'))::bigint AS failed \
           FROM ko_push_intents",
    )
    .fetch_one(&mut *conn)
    .await?;

    let last_expense = mirror::last_run(&mut *conn, mirror::KIND_EXPENSES).await?;
    let last_metadata = mirror::last_run(&mut *conn, mirror::KIND_METADATA).await?;

    let poll_seconds = super::floor_interval(state.config.kitchenowl_expense_poll_seconds);
    // Derived, not probed: this endpoint must not block on HTTP. `next_run_at` is
    // the last run plus the interval, which is also what the loop will do.
    let next_run_at = match (&last_expense, poll_seconds) {
        (_, 0) => None,
        (Some(run), secs) => Some(run.started_at + chrono::Duration::seconds(secs as i64)),
        (None, _) => Some(Utc::now()),
    };
    let reachable = last_expense.as_ref().map(|r| r.status != "failed");

    let metadata_fetched_at: Option<chrono::DateTime<Utc>> = sync_state
        .as_ref()
        .and_then(|r| r.get("metadata_fetched_at"));
    let metadata_stale = is_stale(
        metadata_fetched_at,
        state.config.kitchenowl_metadata_stale_seconds,
    );

    let out = KoStatus {
        configured: KoClient::from_state(&state).is_some(),
        enabled,
        reachable,
        running: state
            .guards
            .ko_expenses
            .load(std::sync::atomic::Ordering::Relaxed),
        household_id: sync_state.as_ref().and_then(|r| r.get("household_id")),
        household_name: sync_state.as_ref().and_then(|r| r.get("household_name")),
        last_expense_run: last_expense.map(run_to_dto),
        last_metadata_run: last_metadata.map(run_to_dto),
        next_run_at,
        poll_seconds,
        mirrored_count: counts.get("mirrored"),
        archived_count: counts.get("archived"),
        linked_count: counts.get("linked"),
        open_draft_count: drafts.get("open"),
        likely_duplicate_count: drafts.get("likely"),
        pending_push_count: pushes.get("pending"),
        failed_push_count: pushes.get("failed"),
        metadata_fetched_at,
        metadata_stale,
        last_error: sync_state.as_ref().and_then(|r| r.get("last_error")),
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

fn is_stale(fetched_at: Option<chrono::DateTime<Utc>>, stale_seconds: i64) -> bool {
    match fetched_at {
        None => true,
        Some(at) => (Utc::now() - at).num_seconds() > stale_seconds,
    }
}

// ----------------------------------------------------------------- summary

#[utoipa::path(
    get,
    path = "/api/v1/kitchenowl/summary",
    tag = "kitchenowl",
    responses((status = 200, description = "Aus dem lokalen Spiegel; blockiert nie auf HTTP", body = KoSummary)),
)]
pub async fn summary(State(state): State<AppState>, mut ctx: Ctx) -> Result<Json<KoSummary>> {
    let conn = ctx.tenant.conn();
    let enabled = mirror::is_enabled(&mut *conn).await?;
    let names = member_names(&mut *conn).await?;

    let members = load_members(&mut *conn).await?;
    let my_balance = members.iter().find(|m| m.is_me).map(|m| m.balance_cents);

    let today = Utc::now().date_naive();
    let month = Period {
        year: today.year(),
        month: today.month() as u8,
    };

    let totals = sqlx::query(
        "SELECT COALESCE(SUM(amount_cents), 0)::bigint AS amount, \
                COALESCE(SUM(own_share_cents), 0)::bigint AS own, \
                count(*)::bigint AS n \
           FROM ko_expenses \
          WHERE archived_at IS NULL \
            AND date_part('year', expense_date) = $1 \
            AND date_part('month', expense_date) = $2",
    )
    .bind(month.year)
    .bind(month.month as i32)
    .fetch_one(&mut *conn)
    .await?;

    let rows = sqlx::query(&format!(
        "{} WHERE e.archived_at IS NULL \
         ORDER BY e.expense_date DESC, e.external_id DESC LIMIT 6",
        select_expense()
    ))
    .fetch_all(&mut *conn)
    .await?;

    let last_synced_at: Option<chrono::DateTime<Utc>> =
        mirror::last_run(&mut *conn, mirror::KIND_EXPENSES)
            .await?
            .and_then(|r| r.finished_at.filter(|_| r.status != "failed"));
    let last_error: Option<String> =
        sqlx::query_scalar("SELECT last_error FROM ko_sync_state LIMIT 1")
            .fetch_optional(&mut *conn)
            .await?
            .flatten();

    // Stale is judged against the poll interval, not against a fixed window: a
    // fifteen-minute poll that last succeeded an hour ago is a problem, a daily one
    // is not.
    let poll = super::floor_interval(state.config.kitchenowl_expense_poll_seconds);
    let stale = match (last_synced_at, poll) {
        (None, _) => enabled,
        (Some(_), 0) => false,
        (Some(at), secs) => (Utc::now() - at).num_seconds() > (secs as i64) * 3,
    };

    let household_name: Option<String> =
        sqlx::query_scalar("SELECT household_name FROM ko_sync_state LIMIT 1")
            .fetch_optional(&mut *conn)
            .await?
            .flatten();

    let out = KoSummary {
        configured: KoClient::from_state(&state).is_some(),
        enabled,
        household_name,
        members,
        my_balance_cents: my_balance,
        recent: rows.iter().map(|r| row_to_expense(r, &names)).collect(),
        month,
        month_amount_cents: totals.get("amount"),
        month_own_share_cents: totals.get("own"),
        month_count: totals.get("n"),
        last_synced_at,
        stale,
        warning: last_error,
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

async fn load_members(conn: &mut PgConnection) -> Result<Vec<KoMember>> {
    let rows = sqlx::query(
        "SELECT member_id, name, username, balance_cents, is_me, is_owner, is_admin, fetched_at \
           FROM ko_members ORDER BY member_id",
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .iter()
        .map(|r| KoMember {
            member_id: r.get("member_id"),
            name: r.get("name"),
            username: r.get("username"),
            balance_cents: r.get("balance_cents"),
            is_me: r.get("is_me"),
            is_owner: r.get("is_owner"),
            is_admin: r.get("is_admin"),
            fetched_at: r.get("fetched_at"),
        })
        .collect())
}

// ---------------------------------------------------------------- metadata

#[utoipa::path(
    get,
    path = "/api/v1/kitchenowl/metadata",
    tag = "kitchenowl",
    responses((status = 200, description = "Mitglieder und Kategorien; veraltet mit Hinweis statt gar nicht", body = KoMetadata)),
)]
pub async fn metadata(State(state): State<AppState>, mut ctx: Ctx) -> Result<Json<KoMetadata>> {
    let conn = ctx.tenant.conn();
    let members = load_members(&mut *conn).await?;
    let rows = sqlx::query(
        "SELECT category_id, name, color_argb, budget_cents, fetched_at \
           FROM ko_categories ORDER BY name",
    )
    .fetch_all(&mut *conn)
    .await?;
    let categories: Vec<KoCategory> = rows
        .iter()
        .map(|r| KoCategory {
            category_id: r.get("category_id"),
            name: r.get("name"),
            color_argb: r.get("color_argb"),
            budget_cents: r.get("budget_cents"),
            fetched_at: r.get("fetched_at"),
        })
        .collect();

    let fetched_at = members
        .iter()
        .map(|m| m.fetched_at)
        .chain(categories.iter().map(|c| c.fetched_at))
        .max();
    let stale = is_stale(fetched_at, state.config.kitchenowl_metadata_stale_seconds);
    let warning = if members.is_empty() {
        Some("Noch keine KitchenOwl-Daten — bitte einmal synchronisieren.".to_string())
    } else if stale {
        Some(
            "Diese Angaben sind möglicherweise veraltet; KitchenOwl war zuletzt nicht erreichbar."
                .to_string(),
        )
    } else {
        None
    };

    let out = KoMetadata {
        members,
        categories,
        fetched_at,
        stale,
        warning,
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

// -------------------------------------------------------------------- sync

/// Runs a sync now, synchronously, so the button gives an answer rather than a
/// promise. Concurrent clicks are absorbed by the same `AtomicBool` the periodic
/// loop uses, so two users pressing it twice cannot run two scans at once.
#[utoipa::path(
    post,
    path = "/api/v1/kitchenowl/sync",
    tag = "kitchenowl",
    responses((status = 200, description = "Lauf beendet", body = KoSyncResult), (status = 202, description = "Ein Lauf war bereits aktiv", body = KoSyncResult), (status = 502, description = "KitchenOwl nicht erreichbar", body = crate::error::ErrorBody)),
)]
pub async fn sync_now(
    State(state): State<AppState>,
    mut ctx: Ctx,
) -> Result<(StatusCode, Json<KoSyncResult>)> {
    let Some(client) = KoClient::from_state(&state) else {
        return Err(AppError::Integration(
            "KitchenOwl ist auf diesem Server nicht konfiguriert".into(),
        ));
    };
    // The opt-in. Committed before any HTTP so the account takes part in the
    // periodic loop even if this particular run fails.
    mirror::enable(ctx.tenant.conn(), ctx.user.id).await?;
    let user_id = ctx.user.id;
    ctx.tenant.commit().await?;

    let guard = super::Guard::acquire(&state.guards.ko_expenses);
    let Some(_guard) = guard else {
        return Ok((
            StatusCode::ACCEPTED,
            Json(KoSyncResult {
                started: false,
                expenses: None,
                metadata: None,
                error: None,
            }),
        ));
    };

    // The runs are bookkept in sync_runs either way, so a failure here is visible
    // in /kitchenowl/status even though this request is about to 502.
    super::run_full_sync(&state, &client, user_id).await?;

    let mut tenant = Tenant::begin(&state.db, user_id).await?;
    let expenses = mirror::last_run(tenant.conn(), mirror::KIND_EXPENSES)
        .await?
        .map(run_to_dto);
    let metadata = mirror::last_run(tenant.conn(), mirror::KIND_METADATA)
        .await?
        .map(run_to_dto);
    tenant.commit().await?;

    // A `partial` run succeeded at reaching KitchenOwl but did not see everything —
    // a page cap, or an expense whose shape this version cannot read. That is worth
    // saying out loud rather than reporting as a clean success.
    let error = expenses
        .as_ref()
        .filter(|r| r.status == "partial")
        .map(|r| {
            format!(
                "Teilweise synchronisiert: {} Ausgabe(n) übersprungen",
                r.failed_count
            )
        });

    Ok((
        StatusCode::OK,
        Json(KoSyncResult {
            started: true,
            expenses,
            metadata,
            error,
        }),
    ))
}

// ---------------------------------------------------------------- expenses

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseQuery {
    pub year: Option<i32>,
    pub month: Option<u8>,
    pub linked: Option<bool>,
    /// One KitchenOwl category, so the analysis can hand the ledger the rows behind
    /// a bar. `uncategorized` is the other half of that: "no category" is a third
    /// of the corpus and has to be reachable too.
    pub ko_category_id: Option<i64>,
    #[serde(default)]
    pub uncategorized: bool,
    pub search: Option<String>,
    #[serde(default)]
    pub include_archived: bool,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

fn expense_filter(q: &ExpenseQuery) -> (String, Vec<String>) {
    let mut clauses = vec![if q.include_archived {
        "true".to_string()
    } else {
        "e.archived_at IS NULL".to_string()
    }];
    let mut binds: Vec<String> = Vec::new();
    let mut n = 0;
    let mut next = |binds: &mut Vec<String>, value: String| {
        binds.push(value);
        n += 1;
        format!("${n}")
    };
    if let Some(v) = q.year {
        let p = next(&mut binds, v.to_string());
        clauses.push(format!("date_part('year', e.expense_date) = {p}::int"));
    }
    if let Some(v) = q.month {
        let p = next(&mut binds, v.to_string());
        clauses.push(format!("date_part('month', e.expense_date) = {p}::int"));
    }
    if let Some(v) = q.ko_category_id {
        let p = next(&mut binds, v.to_string());
        clauses.push(format!("e.ko_category_id = {p}::bigint"));
    }
    if q.uncategorized {
        clauses.push("e.ko_category_id IS NULL".into());
    }
    match q.linked {
        Some(true) => clauses.push("e.linked_booking_id IS NOT NULL".into()),
        Some(false) => clauses.push("e.linked_booking_id IS NULL".into()),
        None => {}
    }
    if let Some(v) = &q.search
        && !v.trim().is_empty()
    {
        let p = next(&mut binds, format!("%{}%", v.trim()));
        clauses.push(format!("(e.name ILIKE {p} OR e.description ILIKE {p})"));
    }
    (clauses.join(" AND "), binds)
}

/// Confirms a link. **Creates no booking** — it records that an existing booking and
/// a mirrored expense are the same purchase, and moves no figure in either ledger.
#[utoipa::path(
    get,
    path = "/api/v1/kitchenowl/expenses",
    tag = "kitchenowl",
    params(("year" = Option<i32>, Query, description = "Kalenderjahr"), ("month" = Option<u8>, Query, description = "Monat 1..12"), ("linked" = Option<bool>, Query, description = "Nur (nicht) verknüpfte"), ("koCategoryId" = Option<i64>, Query, description = "Eine KitchenOwl-Kategorie"), ("uncategorized" = Option<bool>, Query, description = "Nur Ausgaben ohne KitchenOwl-Kategorie"), ("search" = Option<String>, Query, description = "Name oder Beschreibung enthält"), ("includeArchived" = Option<bool>, Query, description = "Auch in KitchenOwl gelöschte"), ("page" = Option<u32>, Query, description = "Seite, ab 0"), ("pageSize" = Option<u32>, Query, description = "1..200, Standard 50")),
    responses((status = 200, description = "Der Spiegel — Gesamtbetrag UND eigener Anteil, nie summiert", body = KoExpensePage)),
)]
pub async fn expenses(mut ctx: Ctx, Query(q): Query<ExpenseQuery>) -> Result<Json<KoExpensePage>> {
    let page = q.page.unwrap_or(0);
    let page_size = q.page_size.unwrap_or(50).clamp(1, 200);
    let (where_sql, binds) = expense_filter(&q);
    let names = member_names(ctx.tenant.conn()).await?;

    let list_sql = format!(
        "{} WHERE {where_sql} \
         ORDER BY e.expense_date DESC, e.external_id DESC LIMIT {page_size} OFFSET {}",
        select_expense(),
        page as i64 * page_size as i64
    );
    let mut query = sqlx::query(&list_sql);
    for b in &binds {
        query = query.bind(b);
    }
    let rows = query.fetch_all(ctx.tenant.conn()).await?;

    // Both sums, always. Reporting only one of them is exactly how the shared
    // amount and the own share get confused.
    let totals_sql = format!(
        "SELECT count(*)::bigint AS total, \
                COALESCE(SUM(e.amount_cents), 0)::bigint AS amount, \
                COALESCE(SUM(e.own_share_cents), 0)::bigint AS own, \
                count(*) FILTER (WHERE e.linked_booking_id IS NOT NULL)::bigint AS linked \
           FROM ko_expenses e WHERE {where_sql}"
    );
    let mut tq = sqlx::query(&totals_sql);
    for b in &binds {
        tq = tq.bind(b);
    }
    let totals = tq.fetch_one(ctx.tenant.conn()).await?;

    let out = KoExpensePage {
        items: rows.iter().map(|r| row_to_expense(r, &names)).collect(),
        total: totals.get("total"),
        page,
        page_size,
        sum_amount_cents: totals.get("amount"),
        sum_own_share_cents: totals.get("own"),
        linked_count: totals.get("linked"),
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

// ------------------------------------------------------------------ drafts

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftQuery {
    /// Comma-separated. Defaults to everything still undecided.
    pub status: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

/// Re-scores every open suggestion against the bookings as they stand now.
///
/// Suggestions are a statement about TWO ledgers, but they were only ever computed
/// when one of them moved — written when an expense is pulled, rewritten only when
/// that expense changes upstream. Import a year of bookings afterwards and every
/// existing draft keeps the empty candidate list it was born with. That is not
/// hypothetical: the drafts here were written at 20:51 and the bookings arrived at
/// 20:58, so 459 suggestions had been scored against an empty ledger and nothing
/// ever asked them again.
#[utoipa::path(
    post,
    path = "/api/v1/kitchenowl/drafts/rescan",
    tag = "kitchenowl",
    responses((status = 200, description = "Vorschläge neu berechnet", body = super::matching::RescanResult)),
)]
pub async fn rescan_drafts(
    State(state): State<AppState>,
    mut ctx: Ctx,
) -> Result<Json<super::matching::RescanResult>> {
    let out = super::matching::rescan_open(
        ctx.tenant.conn(),
        state.config.kitchenowl_duplicate_threshold,
    )
    .await?;
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[utoipa::path(
    get,
    path = "/api/v1/kitchenowl/drafts",
    tag = "kitchenowl",
    params(("status" = Option<String>, Query, description = "Kommagetrennt; Standard: alles Unentschiedene"), ("page" = Option<u32>, Query, description = "Seite, ab 0"), ("pageSize" = Option<u32>, Query, description = "1..200, Standard 50")),
    responses((status = 200, description = "Vorschlagsliste; „ohne Treffer“ ist der Normalfall", body = KoDraftPage)),
)]
pub async fn drafts(mut ctx: Ctx, Query(q): Query<DraftQuery>) -> Result<Json<KoDraftPage>> {
    let page = q.page.unwrap_or(0);
    let page_size = q.page_size.unwrap_or(50).clamp(1, 200);
    let statuses: Vec<String> = q
        .status
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .filter(|v: &Vec<String>| !v.is_empty())
        .unwrap_or_else(|| {
            ["likely_duplicate", "possible_duplicate", "open"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        });

    let names = member_names(ctx.tenant.conn()).await?;
    let rows = sqlx::query(&format!(
        "SELECT d.id AS draft_id, d.status AS draft_status, d.match_candidates, \
                d.created_at AS draft_created_at, {EXPENSE_COLUMNS} \
           FROM ko_drafts d JOIN ko_expenses e ON e.id = d.ko_expense_id \
           LEFT JOIN bookings b ON b.id = e.linked_booking_id \
          WHERE d.status = ANY($1) \
          ORDER BY (d.status = 'likely_duplicate') DESC, e.expense_date DESC, d.created_at DESC \
          LIMIT {page_size} OFFSET {}",
        page as i64 * page_size as i64
    ))
    .bind(&statuses)
    .fetch_all(ctx.tenant.conn())
    .await?;

    let counts = sqlx::query(
        "SELECT count(*) FILTER (WHERE status = ANY($1))::bigint AS total, \
                count(*) FILTER (WHERE status IN ('open','possible_duplicate'))::bigint AS open, \
                count(*) FILTER (WHERE status = 'likely_duplicate')::bigint AS likely \
           FROM ko_drafts",
    )
    .bind(&statuses)
    .fetch_one(ctx.tenant.conn())
    .await?;

    let items = rows
        .iter()
        .map(|r| {
            let candidates: Vec<KoMatchCandidate> =
                serde_json::from_value(r.get("match_candidates")).unwrap_or_default();
            let status: String = r.get("draft_status");
            KoDraft {
                id: r.get("draft_id"),
                // Never `create`. A pull writes no booking, ever.
                suggested_action: if status == "likely_duplicate" && !candidates.is_empty() {
                    "link".into()
                } else {
                    "none".into()
                },
                status,
                expense: row_to_expense(r, &names),
                candidates,
                created_at: r.get("draft_created_at"),
            }
        })
        .collect();

    let out = KoDraftPage {
        items,
        total: counts.get("total"),
        open_count: counts.get("open"),
        likely_count: counts.get("likely"),
    };
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

#[utoipa::path(
    post,
    path = "/api/v1/kitchenowl/drafts/{id}/link",
    tag = "kitchenowl",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    request_body = KoLinkRequest,
    responses((status = 200, description = "Verknüpft. Es wurde KEINE Buchung angelegt", body = KoDraft), (status = 409, description = "Buchung oder Ausgabe ist bereits verknüpft", body = crate::error::ErrorBody)),
)]
pub async fn link_draft(
    mut ctx: Ctx,
    Path(id): Path<Uuid>,
    Json(body): Json<KoLinkRequest>,
) -> Result<Json<KoDraft>> {
    let row = sqlx::query(
        "SELECT ko_expense_id FROM ko_drafts WHERE id = $1 AND ko_expense_id IS NOT NULL",
    )
    .bind(id)
    .fetch_optional(ctx.tenant.conn())
    .await?
    .ok_or_else(|| AppError::NotFound("Vorschlag".into()))?;
    let ko_expense_id: Uuid = row.get("ko_expense_id");

    let external_id: i64 = sqlx::query_scalar("SELECT external_id FROM ko_expenses WHERE id = $1")
        .bind(ko_expense_id)
        .fetch_optional(ctx.tenant.conn())
        .await?
        .ok_or_else(|| AppError::NotFound("KitchenOwl-Ausgabe".into()))?;

    link::attach(ctx.tenant.conn(), body.booking_id, external_id).await?;
    sqlx::query("UPDATE ko_drafts SET status = 'confirmed', resolved_at = now() WHERE id = $1")
        .bind(id)
        .execute(ctx.tenant.conn())
        .await?;

    let draft = load_draft(&mut ctx, id).await?;
    ctx.tenant.commit().await?;
    Ok(Json(draft))
}

/// Dismisses a suggestion. An unlinked expense is the normal case, so this is an
/// ordinary outcome and not a rejection of anything.
#[utoipa::path(
    post,
    path = "/api/v1/kitchenowl/drafts/{id}/dismiss",
    tag = "kitchenowl",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    responses((status = 200, description = "Vorschlag verworfen", body = KoDraft), (status = 404, description = "Nicht gefunden", body = crate::error::ErrorBody)),
)]
pub async fn dismiss_draft(mut ctx: Ctx, Path(id): Path<Uuid>) -> Result<Json<KoDraft>> {
    let affected =
        sqlx::query("UPDATE ko_drafts SET status = 'discarded', resolved_at = now() WHERE id = $1")
            .bind(id)
            .execute(ctx.tenant.conn())
            .await?
            .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound("Vorschlag".into()));
    }
    let draft = load_draft(&mut ctx, id).await?;
    ctx.tenant.commit().await?;
    Ok(Json(draft))
}

async fn load_draft(ctx: &mut Ctx, id: Uuid) -> Result<KoDraft> {
    let names = member_names(ctx.tenant.conn()).await?;
    let row = sqlx::query(&format!(
        "SELECT d.id AS draft_id, d.status AS draft_status, d.match_candidates, \
                d.created_at AS draft_created_at, {EXPENSE_COLUMNS} \
           FROM ko_drafts d JOIN ko_expenses e ON e.id = d.ko_expense_id \
           LEFT JOIN bookings b ON b.id = e.linked_booking_id \
          WHERE d.id = $1"
    ))
    .bind(id)
    .fetch_optional(ctx.tenant.conn())
    .await?
    .ok_or_else(|| AppError::NotFound("Vorschlag".into()))?;

    let candidates: Vec<KoMatchCandidate> =
        serde_json::from_value(row.get("match_candidates")).unwrap_or_default();
    let status: String = row.get("draft_status");
    Ok(KoDraft {
        id: row.get("draft_id"),
        suggested_action: if status == "likely_duplicate" && !candidates.is_empty() {
            "link".into()
        } else {
            "none".into()
        },
        status,
        expense: row_to_expense(&row, &names),
        candidates,
        created_at: row.get("draft_created_at"),
    })
}

/// Removes a link. Reversible in both directions, which is the promise the product
/// decision made: nothing about the two ledgers is permanent.
#[utoipa::path(
    delete,
    path = "/api/v1/kitchenowl/expenses/{id}/link",
    tag = "kitchenowl",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    responses((status = 204, description = "Verknüpfung gelöst; keine Zahl bewegt sich"), (status = 404, description = "Nicht gefunden", body = crate::error::ErrorBody)),
)]
pub async fn unlink(mut ctx: Ctx, Path(id): Path<Uuid>) -> Result<StatusCode> {
    if !link::detach(ctx.tenant.conn(), id).await? {
        return Err(AppError::NotFound("KitchenOwl-Ausgabe".into()));
    }
    ctx.tenant.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// -------------------------------------------------------------------- push

/// Queues a push. Commits the intent **before** any HTTP and answers 202 even when
/// KitchenOwl is unreachable.
#[utoipa::path(
    post,
    path = "/api/v1/bookings/{id}/kitchenowl",
    tag = "kitchenowl",
    params(("id" = Uuid, Path, description = "Datensatz-Id")),
    request_body = KoPushRequest,
    responses((status = 202, description = "Vorgemerkt — auch wenn KitchenOwl gerade nicht erreichbar ist", body = KoPushIntent), (status = 400, description = "Umbuchung, Entwurf oder unbekanntes Mitglied", body = crate::error::ErrorBody), (status = 409, description = "Buchung ist bereits verknüpft", body = crate::error::ErrorBody)),
)]
pub async fn push_booking(
    State(state): State<AppState>,
    mut ctx: Ctx,
    Path(booking_id): Path<Uuid>,
    body: Option<Json<KoPushRequest>>,
) -> Result<(StatusCode, Json<KoPushIntent>)> {
    if KoClient::from_state(&state).is_none() {
        return Err(AppError::Integration(
            "KitchenOwl ist auf diesem Server nicht konfiguriert".into(),
        ));
    }
    let input = body.map(|Json(v)| v).unwrap_or_default();
    let payload = push::build_payload(
        ctx.tenant.conn(),
        booking_id,
        &input,
        state.config.kitchenowl_push_marker_in_name,
    )
    .await?;

    push::queue(ctx.tenant.conn(), ctx.user.id, booking_id, &payload).await?;
    let user_id = ctx.user.id;
    let intent = load_intent(&mut ctx, booking_id).await?;
    // Committed before the attempt, on purpose: the outbox row is the durable
    // record, the HTTP call is opportunistic.
    ctx.tenant.commit().await?;

    super::spawn_push_attempt(&state, user_id);

    Ok((StatusCode::ACCEPTED, Json(intent)))
}

/// Retries a failed or abandoned push. Safe by construction — the marker scan runs
/// first — and the UI says so, because otherwise the user retries by hand in the
/// KitchenOwl app and creates the duplicate this whole mechanism exists to avoid.
#[utoipa::path(
    get,
    path = "/api/v1/kitchenowl/push",
    tag = "kitchenowl",
    responses((status = 200, description = "Die Warteschlange, jeder Posten mit seinem letzten Fehler", body = Vec<KoPushIntent>)),
)]
pub async fn push_list(mut ctx: Ctx) -> Result<Json<Vec<KoPushIntent>>> {
    let rows = sqlx::query(
        "SELECT i.booking_id, i.state, i.marker, i.payload, i.attempts, i.last_error, \
                i.external_id, i.next_attempt_at, i.created_at, i.updated_at, b.comment \
           FROM ko_push_intents i LEFT JOIN bookings b ON b.id = i.booking_id \
          ORDER BY i.created_at DESC LIMIT 200",
    )
    .fetch_all(ctx.tenant.conn())
    .await?;
    let out: Vec<KoPushIntent> = rows.iter().filter_map(row_to_intent).collect();
    ctx.tenant.commit().await?;
    Ok(Json(out))
}

fn row_to_intent(r: &sqlx::postgres::PgRow) -> Option<KoPushIntent> {
    let payload: wire::PushPayload = serde_json::from_value(r.get("payload")).ok()?;
    Some(KoPushIntent {
        booking_id: r.get("booking_id"),
        state: r.get("state"),
        booking_comment: r.get("comment"),
        amount_cents: payload.amount_cents,
        date: payload.date,
        name: payload.name,
        marker: r.get("marker"),
        attempts: r.get("attempts"),
        last_error: r.get("last_error"),
        external_id: r.get("external_id"),
        next_attempt_at: r.get("next_attempt_at"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    })
}

async fn load_intent(ctx: &mut Ctx, booking_id: Uuid) -> Result<KoPushIntent> {
    let row = sqlx::query(
        "SELECT i.booking_id, i.state, i.marker, i.payload, i.attempts, i.last_error, \
                i.external_id, i.next_attempt_at, i.created_at, i.updated_at, b.comment \
           FROM ko_push_intents i LEFT JOIN bookings b ON b.id = i.booking_id \
          WHERE i.booking_id = $1",
    )
    .bind(booking_id)
    .fetch_optional(ctx.tenant.conn())
    .await?
    .ok_or_else(|| AppError::NotFound("Push-Auftrag".into()))?;
    row_to_intent(&row)
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Push-Payload nicht lesbar")))
}

#[utoipa::path(
    post,
    path = "/api/v1/kitchenowl/push/{bookingId}/retry",
    tag = "kitchenowl",
    params(("bookingId" = Uuid, Path, description = "Buchungs-Id")),
    responses((status = 200, description = "Erneut vorgemerkt; der Abgleich vor dem Senden verhindert Dubletten", body = KoPushIntent), (status = 409, description = "Wartet bereits oder ist übertragen", body = crate::error::ErrorBody)),
)]
pub async fn push_retry(
    State(state): State<AppState>,
    mut ctx: Ctx,
    Path(booking_id): Path<Uuid>,
) -> Result<Json<KoPushIntent>> {
    let affected = sqlx::query(
        "UPDATE ko_push_intents SET state = 'queued', next_attempt_at = now(), \
                last_error = NULL, updated_at = now() \
          WHERE booking_id = $1 AND state IN ('failed','abandoned','retracted')",
    )
    .bind(booking_id)
    .execute(ctx.tenant.conn())
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::Conflict(
            "Dieser Push wartet bereits oder ist schon übertragen".into(),
        ));
    }
    let user_id = ctx.user.id;
    let intent = load_intent(&mut ctx, booking_id).await?;
    ctx.tenant.commit().await?;
    super::spawn_push_attempt(&state, user_id);
    Ok(Json(intent))
}

/// Withdraws a push that has not gone out yet. A pushed one cannot be withdrawn from
/// here: deleting somebody else's household expense is not this app's decision.
#[utoipa::path(
    delete,
    path = "/api/v1/kitchenowl/push/{bookingId}",
    tag = "kitchenowl",
    params(("bookingId" = Uuid, Path, description = "Buchungs-Id")),
    responses((status = 204, description = "Zurückgezogen"), (status = 409, description = "Nur wartende Aufträge lassen sich zurückziehen", body = crate::error::ErrorBody)),
)]
pub async fn push_retract(mut ctx: Ctx, Path(booking_id): Path<Uuid>) -> Result<StatusCode> {
    let affected = sqlx::query(
        "UPDATE ko_push_intents SET state = 'retracted', next_attempt_at = NULL, \
                updated_at = now() WHERE booking_id = $1 AND state IN ('queued','failed')",
    )
    .bind(booking_id)
    .execute(ctx.tenant.conn())
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::Conflict(
            "Nur wartende Push-Aufträge können zurückgezogen werden".into(),
        ));
    }
    ctx.tenant.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
