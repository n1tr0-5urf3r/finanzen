use sqlx::{PgPool, Row, postgres::PgPoolOptions};

use crate::config::Config;

/// Owns the pool. The pool itself is deliberately not reachable from request
/// handlers: all tenant-scoped access goes through [`crate::tenant::Tenant`], and the
/// three system-scope tables go through [`Db::system`].
#[derive(Clone, Debug)]
pub struct Db(PgPool);

impl Db {
    /// Only [`crate::tenant::Tenant`] may open a transaction from the raw pool.
    /// CI greps for uses of this outside `tenant.rs`.
    pub(crate) fn pool(&self) -> &PgPool {
        &self.0
    }

    /// Explicit escape hatch for `users`, `identities` and `sessions`, which are read
    /// *before* a tenant context exists and therefore carry no RLS policy.
    pub fn system(&self) -> SystemDb<'_> {
        SystemDb(&self.0)
    }

    /// Wraps an existing pool. Used by the integration tests, which manage their
    /// own per-test database and role.
    pub fn from_pool(pool: PgPool) -> Self {
        Self(pool)
    }
}

/// A pool handle restricted by convention to the system-scope tables.
pub struct SystemDb<'a>(&'a PgPool);

impl<'a> SystemDb<'a> {
    pub fn inner(&self) -> &'a PgPool {
        self.0
    }
}

pub async fn connect(config: &Config) -> anyhow::Result<Db> {
    let pool = PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .acquire_timeout(config.db_connect_timeout)
        .connect(&config.database_url)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;
    assert_rls_effective(&pool, config).await?;

    Ok(Db(pool))
}

/// Row-Level Security is the backstop that makes a forgotten `WHERE user_id` a
/// non-event. A superuser or a BYPASSRLS role silently defeats it, turning tenant
/// isolation into a no-op — so refuse to start rather than run unprotected.
async fn assert_rls_effective(pool: &PgPool, config: &Config) -> anyhow::Result<()> {
    let row = sqlx::query(
        "SELECT current_user::text, \
                COALESCE((SELECT rolsuper FROM pg_roles WHERE rolname = current_user), false), \
                COALESCE((SELECT rolbypassrls FROM pg_roles WHERE rolname = current_user), false)",
    )
    .fetch_one(pool)
    .await?;

    let who: String = row.get(0);
    let is_superuser: bool = row.get(1);
    let bypasses_rls: bool = row.get(2);

    if is_superuser || bypasses_rls {
        if config.dev_mode {
            tracing::warn!(
                role = %who,
                "Datenbankrolle umgeht Row-Level-Security; im Entwicklungsmodus erlaubt"
            );
        } else {
            anyhow::bail!(
                "Datenbankrolle '{who}' umgeht Row-Level-Security \
                 (superuser={is_superuser}, bypassrls={bypasses_rls}). \
                 Die Mandantentrennung waere wirkungslos. Bitte eine eingeschraenkte Rolle \
                 verwenden oder APP_DEV_MODE=true setzen."
            );
        }
    }
    Ok(())
}

/// Runs `work` so that its failure cannot take the surrounding transaction with it.
///
/// A failed statement puts a Postgres transaction into the aborted state, and
/// from then on the `COMMIT` that ends it silently becomes a `ROLLBACK` — the
/// server answers without an error, so the caller believes it committed. Logging
/// the error of a best-effort step and carrying on is therefore not "best effort"
/// at all: it quietly undoes everything the transaction did before it. A
/// savepoint scopes the damage to the step that failed.
///
/// Returns whatever `work` returned, having rolled back to the savepoint if it
/// failed.
pub async fn savepoint<T, F>(
    conn: &mut sqlx::PgConnection,
    name: &str,
    work: F,
) -> crate::error::Result<T>
where
    F: AsyncFnOnce(&mut sqlx::PgConnection) -> crate::error::Result<T>,
{
    sqlx::query(&format!("SAVEPOINT {name}"))
        .execute(&mut *conn)
        .await?;
    match work(&mut *conn).await {
        Ok(value) => {
            sqlx::query(&format!("RELEASE SAVEPOINT {name}"))
                .execute(&mut *conn)
                .await?;
            Ok(value)
        }
        Err(e) => {
            sqlx::query(&format!("ROLLBACK TO SAVEPOINT {name}"))
                .execute(&mut *conn)
                .await?;
            Err(e)
        }
    }
}
