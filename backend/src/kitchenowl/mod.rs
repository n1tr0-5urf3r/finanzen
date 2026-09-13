//! KitchenOwl — a **separate, parallel ledger**.
//!
//! The user's constraint, verbatim: *"Do not mix kitchenowl with the personal
//! bookings. It would be nice if you could match them, but keep it separate. There
//! may be other or lacking data from kitchenowl that is not tracked and they will
//! never match up."*
//!
//! What that means mechanically, and what every module here is built to preserve:
//!
//! - The mirror is faithful and local. It is **never** a source of personal
//!   bookings, and no response adds a KitchenOwl figure to a booking figure.
//! - **A pull never writes a booking.** It writes `ko_*` rows and a review list.
//! - Matching is an optional, reversible **link**. Confirming one moves no money.
//! - The two ledgers will never reconcile, by design. There is no "unreconciled
//!   difference" error state anywhere, because an unlinked expense is normal.
//! - If KitchenOwl is unreachable the rest of the app keeps working and the failure
//!   is visible, as `AppError::Integration` -> 502 and as a recorded `sync_runs` row.

pub mod analysis;
pub mod client;
pub mod link;
pub mod matching;
pub mod mirror;
pub mod push;
pub mod routes;
pub mod settle;
pub mod tagging;
pub mod wire;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use uuid::Uuid;

use crate::{AppState, error::Result, tenant::Tenant};

use client::KoClient;

/// schmauserei's interval contract: `0` disables the loop entirely, anything else is
/// floored at 60 seconds so a mistyped `5` cannot hammer somebody else's server.
pub fn floor_interval(seconds: u64) -> u64 {
    if seconds == 0 { 0 } else { seconds.max(60) }
}

/// An RAII hold on one of the `AppState.guards` flags.
///
/// The guards are separate per loop, so a long expense pull does not block the push
/// retry. Releasing on drop is what makes a `?` inside a loop body safe: an early
/// return cannot leave the flag stuck and the loop dead for the rest of the process.
pub struct Guard(Arc<AtomicBool>);

impl Guard {
    pub fn acquire(flag: &Arc<AtomicBool>) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Guard(flag.clone()))
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// One complete sync for one user: metadata first (the push dialogue needs members),
/// then expenses.
///
/// Each half is bookkept in its own `sync_runs` row and each half's failure is
/// recorded rather than swallowed — a metadata failure must not hide the expense
/// pull's result, and vice versa.
pub async fn run_full_sync(state: &AppState, client: &KoClient, user_id: Uuid) -> Result<()> {
    let metadata = sync_metadata_for(state, client, user_id).await;
    let expenses = sync_expenses_for(state, client, user_id).await;
    metadata.and(expenses)
}

async fn sync_metadata_for(state: &AppState, client: &KoClient, user_id: Uuid) -> Result<()> {
    let mut tenant = Tenant::begin(&state.db, user_id).await?;
    let run_id = mirror::begin_run(tenant.conn(), user_id, mirror::KIND_METADATA).await?;
    // The run row is committed before the network call, so a process that dies
    // mid-sync leaves a visible `running` row rather than no trace at all.
    tenant.commit().await?;

    let outcome = {
        let mut tenant = Tenant::begin(&state.db, user_id).await?;
        let result = mirror::sync_metadata(client, tenant.conn(), user_id).await;
        match result {
            Ok(counts) => {
                mirror::finish_run(tenant.conn(), run_id, "success", counts, None).await?;
                tenant.commit().await?;
                Ok(())
            }
            Err(e) => {
                // Roll back whatever half-wrote, then record the failure in a fresh
                // transaction so the bookkeeping survives the rollback.
                tenant.rollback().await?;
                let mut tenant = Tenant::begin(&state.db, user_id).await?;
                mirror::finish_run(
                    tenant.conn(),
                    run_id,
                    "failed",
                    mirror::SyncCounts::default(),
                    Some(&e.to_string()),
                )
                .await?;
                mirror::record_error(tenant.conn(), &e.to_string()).await?;
                tenant.commit().await?;
                Err(e)
            }
        }
    };
    if let Err(e) = &outcome {
        tracing::warn!(error = %e, "KitchenOwl-Metadaten konnten nicht aktualisiert werden");
    }
    outcome
}

async fn sync_expenses_for(state: &AppState, client: &KoClient, user_id: Uuid) -> Result<()> {
    let mut tenant = Tenant::begin(&state.db, user_id).await?;
    let run_id = mirror::begin_run(tenant.conn(), user_id, mirror::KIND_EXPENSES).await?;
    tenant.commit().await?;

    let mut tenant = Tenant::begin(&state.db, user_id).await?;
    let result = mirror::sync_expenses(
        client,
        tenant.conn(),
        user_id,
        state.config.kitchenowl_max_pull_pages,
        state.config.kitchenowl_duplicate_threshold,
    )
    .await;

    match result {
        Ok((counts, complete)) => {
            // Pushed expenses arrive in the mirror one pull later than the link, so
            // the denormalised copy is re-derived after every scan.
            link::reconcile_links(tenant.conn()).await?;
            // `partial` is not `success`: a scan capped by KITCHENOWL_MAX_PULL_PAGES
            // has not seen the whole household and must not be reported as if it had.
            let status = if complete && counts.failed == 0 {
                "success"
            } else {
                "partial"
            };
            mirror::finish_run(tenant.conn(), run_id, status, counts, None).await?;
            tenant.commit().await?;
            Ok(())
        }
        Err(e) => {
            tenant.rollback().await?;
            let mut tenant = Tenant::begin(&state.db, user_id).await?;
            mirror::finish_run(
                tenant.conn(),
                run_id,
                "failed",
                mirror::SyncCounts::default(),
                Some(&e.to_string()),
            )
            .await?;
            mirror::record_error(tenant.conn(), &e.to_string()).await?;
            tenant.commit().await?;
            tracing::warn!(error = %e, "KitchenOwl-Ausgaben konnten nicht gespiegelt werden");
            Err(e)
        }
    }
}

/// Drains the push outbox for one user.
pub async fn run_push_queue(state: &AppState, client: &KoClient, user_id: Uuid) -> Result<()> {
    let timeout = state.config.kitchenowl_http_timeout.as_secs() as i64;
    let mut tenant = Tenant::begin(&state.db, user_id).await?;
    let due = push::due(tenant.conn(), timeout).await?;
    tenant.commit().await?;
    if due.is_empty() {
        return Ok(());
    }

    let mut tenant = Tenant::begin(&state.db, user_id).await?;
    let run_id = mirror::begin_run(tenant.conn(), user_id, mirror::KIND_PUSH).await?;
    tenant.commit().await?;

    let mut counts = mirror::SyncCounts::default();
    let mut last_error: Option<String> = None;
    for intent in &due {
        // One transaction per intent, so one failing push cannot roll back the
        // successful ones that preceded it in the same drain.
        let mut tenant = Tenant::begin(&state.db, user_id).await?;
        match push::attempt(client, tenant.conn(), &state.config, intent).await {
            Ok(Some(_)) => counts.created += 1,
            Ok(None) => {}
            Err(e) => {
                counts.failed += 1;
                last_error = Some(e.to_string());
            }
        }
        // The attempt wrote the outcome onto the intent row either way, so this
        // commits both the success and the recorded failure.
        tenant.commit().await?;
    }

    let mut tenant = Tenant::begin(&state.db, user_id).await?;
    let status = if counts.failed == 0 {
        "success"
    } else if counts.created > 0 {
        "partial"
    } else {
        "failed"
    };
    mirror::finish_run(tenant.conn(), run_id, status, counts, last_error.as_deref()).await?;
    tenant.commit().await?;
    Ok(())
}

/// Best-effort immediate attempt after `POST /bookings/{id}/kitchenowl`.
///
/// Deliberately fire-and-forget: the 202 has already been sent, the intent is
/// already durable, and the retry loop is the safety net. Nothing here may make the
/// request fail.
pub fn spawn_push_attempt(state: &AppState, user_id: Uuid) {
    let Some(client) = KoClient::from_state(state) else {
        return;
    };
    let state = state.clone();
    tokio::spawn(async move {
        let Some(_guard) = Guard::acquire(&state.guards.ko_push) else {
            return;
        };
        if let Err(e) = run_push_queue(&state, &client, user_id).await {
            tracing::warn!(error = %e, "KitchenOwl-Push fehlgeschlagen; wird erneut versucht");
        }
    });
}

/// Starts the three background loops.
///
/// Three, not one: they have different natural periods (a quarter-hour, a day, five
/// minutes) and independent `AtomicBool` guards, so a slow expense scan does not
/// delay a push that is waiting to go out.
pub fn spawn_loops(state: AppState) {
    spawn_loop(
        state.clone(),
        "ko_expenses",
        state.config.kitchenowl_expense_poll_seconds,
        state.guards.ko_expenses.clone(),
        |state, client, user| {
            Box::pin(async move { sync_expenses_for(&state, &client, user).await })
        },
    );
    spawn_loop(
        state.clone(),
        "ko_metadata",
        state.config.kitchenowl_metadata_refresh_seconds,
        state.guards.ko_metadata.clone(),
        |state, client, user| {
            Box::pin(async move { sync_metadata_for(&state, &client, user).await })
        },
    );
    spawn_loop(
        state.clone(),
        "ko_push",
        state.config.kitchenowl_push_retry_seconds,
        state.guards.ko_push.clone(),
        |state, client, user| Box::pin(async move { run_push_queue(&state, &client, user).await }),
    );

    if state.config.kitchenowl_sync_on_start {
        tokio::spawn(async move {
            let Some(client) = KoClient::from_state(&state) else {
                return;
            };
            let users = match mirror::participating_users(state.db.system().inner()).await {
                Ok(users) => users,
                Err(e) => {
                    tracing::warn!(error = %e, "KitchenOwl-Teilnehmer nicht ermittelbar");
                    return;
                }
            };
            let Some(_guard) = Guard::acquire(&state.guards.ko_expenses) else {
                return;
            };
            for user in users {
                let _ = run_full_sync(&state, &client, user).await;
            }
        });
    }
}

type LoopBody = fn(
    AppState,
    KoClient,
    Uuid,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>>;

fn spawn_loop(
    state: AppState,
    name: &'static str,
    seconds: u64,
    guard: Arc<AtomicBool>,
    body: LoopBody,
) {
    let interval = floor_interval(seconds);
    if interval == 0 {
        tracing::info!(
            loop_name = name,
            "KitchenOwl-Schleife deaktiviert (Intervall 0)"
        );
        return;
    }
    if KoClient::from_state(&state).is_none() {
        return;
    }
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(interval));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // The first tick fires immediately; the start-up sync already covers that.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let Some(client) = KoClient::from_state(&state) else {
                continue;
            };
            // Skipping rather than queueing: if the previous pass is still running,
            // the work it is doing is the work this tick would do.
            let Some(_held) = Guard::acquire(&guard) else {
                tracing::debug!(
                    loop_name = name,
                    "vorheriger Lauf noch aktiv, Tick übersprungen"
                );
                continue;
            };
            let users = match mirror::participating_users(state.db.system().inner()).await {
                Ok(users) => users,
                Err(e) => {
                    tracing::warn!(loop_name = name, error = %e, "Teilnehmer nicht ermittelbar");
                    continue;
                }
            };
            for user in users {
                // A failure is already recorded in sync_runs by the body; logging it
                // here as well is what makes it visible without a database query.
                if let Err(e) = body(state.clone(), client.clone(), user).await {
                    tracing::warn!(loop_name = name, %user, error = %e, "KitchenOwl-Lauf fehlgeschlagen");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_disables_and_everything_else_is_floored_at_a_minute() {
        assert_eq!(floor_interval(0), 0);
        assert_eq!(floor_interval(1), 60);
        assert_eq!(floor_interval(59), 60);
        assert_eq!(floor_interval(60), 60);
        assert_eq!(floor_interval(900), 900);
    }

    #[test]
    fn a_guard_is_exclusive_and_is_released_on_drop() {
        let flag = Arc::new(AtomicBool::new(false));
        let held = Guard::acquire(&flag).expect("first acquire");
        assert!(
            Guard::acquire(&flag).is_none(),
            "a second hold must be refused"
        );
        drop(held);
        assert!(
            Guard::acquire(&flag).is_some(),
            "an early return must not leave the loop dead"
        );
    }
}
