//! The optional, reversible link between a booking and a mirrored expense.
//!
//! Confirming a link **does not create a booking** and does not change a single
//! figure in either ledger. It records that these two rows describe the same
//! purchase, so the next pull of that expense is a structural no-op and the UI can
//! stop offering it.
//!
//! `bookings.external_source` / `external_id` is the single source of truth: it
//! carries the partial unique index that makes a second link to the same expense
//! impossible. `ko_expenses.linked_booking_id` is a denormalised copy that exists
//! only so the mirror can be filtered by it, and it is written in the same
//! statement pair as the authoritative one — never separately.

use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::error::{AppError, Result};

pub const SOURCE: &str = "kitchenowl";

pub async fn attach(conn: &mut PgConnection, booking_id: Uuid, external_id: i64) -> Result<()> {
    let external = external_id.to_string();
    let affected = sqlx::query(
        "UPDATE bookings SET external_source = $2, external_id = $3, updated_at = now() \
          WHERE id = $1 AND (external_source IS NULL \
                             OR (external_source = $2 AND external_id = $3))",
    )
    .bind(booking_id)
    .bind(SOURCE)
    .bind(&external)
    .execute(&mut *conn)
    .await
    .map_err(|e| {
        AppError::from_db(
            e,
            "Diese KitchenOwl-Ausgabe ist bereits mit einer anderen Buchung verknüpft",
        )
    })?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::Conflict(
            "Die Buchung ist bereits anderweitig verknüpft".into(),
        ));
    }

    sqlx::query("UPDATE ko_expenses SET linked_booking_id = $2 WHERE external_id = $1")
        .bind(external_id)
        .bind(booking_id)
        .execute(&mut *conn)
        .await?;
    settle_drafts(conn).await?;
    Ok(())
}

/// Closes every open suggestion whose expense is already linked.
///
/// A linked expense has had its question answered, whichever way the link came
/// about. Before this, only a link made from the suggestion itself closed it: a
/// pushed booking is linked before its expense is ever mirrored, the pull that
/// mirrors it then opens a fresh suggestion — offering, as like as not, the very
/// booking it was pushed from — and nothing ever closed that one again.
pub async fn settle_drafts(conn: &mut PgConnection) -> Result<u64> {
    let affected = sqlx::query(
        "UPDATE ko_drafts d SET status = 'confirmed', resolved_at = now() \
           FROM ko_expenses e \
          WHERE e.id = d.ko_expense_id AND e.linked_booking_id IS NOT NULL \
            AND d.status IN ('open','likely_duplicate','possible_duplicate','ignored_by_default')",
    )
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok(affected)
}

/// Removes a link. Both rows survive, and so does every figure — that is what makes
/// the link reversible in the sense the user was promised.
pub async fn detach(conn: &mut PgConnection, ko_expense_id: Uuid) -> Result<bool> {
    let row = sqlx::query("SELECT external_id, linked_booking_id FROM ko_expenses WHERE id = $1")
        .bind(ko_expense_id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::NotFound("KitchenOwl-Ausgabe".into()))?;

    let external_id: i64 = row.get("external_id");
    sqlx::query(
        "UPDATE bookings SET external_source = NULL, external_id = NULL, updated_at = now() \
          WHERE external_source = $1 AND external_id = $2",
    )
    .bind(SOURCE)
    .bind(external_id.to_string())
    .execute(&mut *conn)
    .await?;
    let affected = sqlx::query("UPDATE ko_expenses SET linked_booking_id = NULL WHERE id = $1")
        .bind(ko_expense_id)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    Ok(affected > 0)
}

/// Re-derives `linked_booking_id` for one expense from the authoritative column.
/// Called after a pull creates a mirror row for an expense this app pushed earlier,
/// which is the one case where the booking is linked before the mirror row exists.
pub async fn reconcile_links(conn: &mut PgConnection) -> Result<u64> {
    let affected = sqlx::query(
        "UPDATE ko_expenses e SET linked_booking_id = b.id \
           FROM bookings b \
          WHERE b.external_source = $1 AND b.external_id = e.external_id::text \
            AND e.linked_booking_id IS DISTINCT FROM b.id",
    )
    .bind(SOURCE)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    // And the other direction: a booking whose link was removed leaves a stale copy.
    sqlx::query(
        "UPDATE ko_expenses e SET linked_booking_id = NULL \
          WHERE e.linked_booking_id IS NOT NULL \
            AND NOT EXISTS (SELECT 1 FROM bookings b WHERE b.id = e.linked_booking_id \
                              AND b.external_source = $1 \
                              AND b.external_id = e.external_id::text)",
    )
    .bind(SOURCE)
    .execute(&mut *conn)
    .await?;
    settle_drafts(conn).await?;
    Ok(affected)
}
