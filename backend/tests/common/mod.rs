//! Housekeeping shared by every integration suite.
//!
//! Each test builds its own database and its own non-superuser role, which is what
//! makes the suite parallel and makes one test's rows impossible to mistake for
//! another's. Nothing ever removed them. After a few dozen full runs the test
//! server held thousands of abandoned databases, crash recovery had to fsync every
//! file in all of them, and the server spent ten minutes refusing connections —
//! which reads exactly like a broken test suite and is not one.
//!
//! So a database now carries the second it was created in its name, and every
//! suite sweeps the older ones before it makes another. The window is fifteen
//! minutes: comfortably longer than a full run (about five), so a database still
//! in use is never a candidate, and short enough that one run's leftovers are gone
//! before the next hour's work. The sweep is best-effort, because failing to tidy
//! up must never fail a test.
//!
//! Per-test cleanup would be tighter, but `Drop` cannot await and `#[tokio::test]`
//! gives each test a current-thread runtime, so there is no honest way to drop a
//! database at the end of the test that created it. Sweeping on the way in is the
//! version that actually works.

use sqlx::PgPool;
use std::time::{SystemTime, UNIX_EPOCH};

/// `fin_api_1789…_3f2c…` — suite, creation second, uniqueness.
pub fn database_name(prefix: &str) -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{prefix}_{epoch}_{}", uuid::Uuid::new_v4().simple())
}

/// Drops test databases and roles left behind by earlier runs.
///
/// Only names this suite family created, only those older than an hour, and only
/// with `WITH (FORCE)` so a connection still lingering from a killed run cannot
/// block the drop. Errors are swallowed on purpose: this is tidying, not a test.
pub async fn reap_stale(admin: &PgPool, max_age_secs: u64) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cutoff = now.saturating_sub(max_age_secs);

    // Two shapes: `fin_api_<epoch>_<uuid>` from this scheme, and the older
    // `fin_api_<uuid>` from before it — those carry no timestamp at all, so they
    // are unconditionally stale by the time anything reads this.
    let Ok(names) = sqlx::query_scalar::<_, String>(
        "SELECT datname FROM pg_database \
          WHERE datname ~ '^fin_(api|ko|test|contract)_([0-9]+_)?[0-9a-f]{32}$' \
          ORDER BY datname",
    )
    .fetch_all(admin)
    .await
    else {
        return;
    };

    for name in names {
        // `fin_<suite>_<epoch>_<uuid>` keeps its age in the name; the older
        // `fin_<suite>_<uuid>` has none, and anything without an age is old.
        let epoch = name
            .split('_')
            .nth(2)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        if epoch >= cutoff {
            continue;
        }
        let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
            .execute(admin)
            .await;
    }

    // Roles outlive their databases and are just as numerous.
    let Ok(roles) = sqlx::query_scalar::<_, String>(
        "SELECT rolname FROM pg_roles \
          WHERE rolname ~ '^fin_[a-z]+_role_([0-9]+_)?[0-9a-f]{32}$' \
             OR rolname ~ '^fin_role_[0-9a-f]{32}$'",
    )
    .fetch_all(admin)
    .await
    else {
        return;
    };
    for role in roles {
        let epoch = role
            .split('_')
            .nth(3)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        if epoch < cutoff {
            let _ = sqlx::query(&format!("DROP ROLE IF EXISTS {role}"))
                .execute(admin)
                .await;
        }
    }
}

/// The name of a role created alongside a test database, carrying the same epoch.
pub fn role_name(prefix: &str, database: &str) -> String {
    let epoch = database.split('_').nth(2).unwrap_or("0");
    format!("{prefix}_role_{epoch}_{}", uuid::Uuid::new_v4().simple())
}
