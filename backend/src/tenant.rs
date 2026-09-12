use sqlx::{PgConnection, Postgres, Transaction};
use uuid::Uuid;

use crate::{db::Db, error::Result};

/// A tenant-scoped unit of work.
///
/// Constructible **only** from a transaction, because `set_config('app.user_id', …,
/// true)` is transaction-local. On a bare pooled connection the setting would survive
/// the response and leak into whichever request picked up that connection next.
///
/// Handlers receive this (via the `Ctx` extractor) and pass `conn()` to every query.
/// That is why no query in this codebase binds a `user_id`: the RLS policy supplies
/// the predicate, so the `WHERE user_id = $1` is absent by design rather than
/// forgotten, and a reviewer never has to check for it.
pub struct Tenant {
    user_id: Uuid,
    tx: Transaction<'static, Postgres>,
}

impl Tenant {
    pub async fn begin(db: &Db, user_id: Uuid) -> Result<Self> {
        let mut tx = db.pool().begin().await?;
        sqlx::query("SELECT set_config('app.user_id', $1, true)")
            .bind(user_id.to_string())
            .execute(&mut *tx)
            .await?;
        Ok(Self { user_id, tx })
    }

    pub fn user_id(&self) -> Uuid {
        self.user_id
    }

    pub fn conn(&mut self) -> &mut PgConnection {
        &mut self.tx
    }

    pub async fn commit(self) -> Result<()> {
        self.tx.commit().await?;
        Ok(())
    }

    /// Explicit rollback. Dropping without committing also rolls back, but saying so
    /// at the call site documents intent.
    pub async fn rollback(self) -> Result<()> {
        self.tx.rollback().await?;
        Ok(())
    }
}
