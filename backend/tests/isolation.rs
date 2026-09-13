//! Proves tenant isolation is enforced by the database, not by handler discipline.
//!
//! The requirement is that a forgotten `WHERE user_id = $1` cannot leak data. Only
//! the database can guarantee that, because queries here are runtime strings with no
//! compile-time link between the SQL text and the bind list. These tests therefore
//! issue deliberately unscoped SQL and assert the database refuses it.
//!
//! Needs a Postgres. Skips (loudly) when TEST_DATABASE_URL is unset:
//!   docker run -d --rm --name fin-pg -e POSTGRES_PASSWORD=finanzen \
//!     -e POSTGRES_USER=finanzen -e POSTGRES_DB=finanzen -p 55432:5432 postgres:17-alpine
//!   TEST_DATABASE_URL=postgres://finanzen:finanzen@localhost:55432/finanzen cargo test

use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use uuid::Uuid;

/// Tables read before a tenant context exists — resolving a session cookie is what
/// establishes `app.user_id` in the first place, so a policy on them would deadlock.
/// Any OTHER table growing a `user_id` column without RLS is a bug, which is what
/// `every_user_scoped_table_has_forced_rls` catches.
/// `ko_participants` is here for the same reason and with the same justification:
/// the KitchenOwl loop runs with no request and no tenant, and a tenant-scoped read
/// would hand it an empty list rather than an error. It holds a user id and nothing
/// else — no amount, no household, no name.
const SYSTEM_SCOPE: &[&str] = &["identities", "sessions", "ko_participants"];

/// Each test gets its own freshly-migrated database, so the suite stays parallel and
/// one test's rows can never be mistaken for another's leaked data.
///
/// Crucially the returned pool connects as a **non-superuser** role. A superuser
/// bypasses row-level security unconditionally, which would make every isolation
/// assertion below pass vacuously — the exact silent failure `db::assert_rls_effective`
/// refuses to boot on. The default `postgres`/`POSTGRES_USER` role in a stock
/// container *is* a superuser, so connecting as it would prove nothing.
async fn pool() -> Option<PgPool> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("connect to TEST_DATABASE_URL");

    let name = format!("fin_test_{}", Uuid::new_v4().simple());
    let role = format!("fin_role_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .expect("create test database");
    sqlx::query(&format!(
        "CREATE ROLE {role} LOGIN PASSWORD 'test' NOSUPERUSER NOBYPASSRLS NOCREATEDB"
    ))
    .execute(&admin)
    .await
    .expect("create restricted role");
    admin.close().await;

    let mut as_admin = url::Url::parse(&url).expect("TEST_DATABASE_URL must be a URL");
    as_admin.set_path(&name);
    let migrator = PgPoolOptions::new()
        .max_connections(1)
        .connect(as_admin.as_str())
        .await
        .expect("connect as owner");
    sqlx::migrate!("./migrations")
        .run(&migrator)
        .await
        .expect("migrations");
    // The application role owns nothing; it only reads and writes. FORCE ROW LEVEL
    // SECURITY means even the owner would be policed, but running as a plain role
    // keeps the test honest about production.
    for grant in [
        format!("GRANT USAGE ON SCHEMA public, app TO {role}"),
        format!("GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO {role}"),
        format!("GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO {role}"),
        format!("GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA app TO {role}"),
    ] {
        sqlx::query(&grant)
            .execute(&migrator)
            .await
            .unwrap_or_else(|e| panic!("{grant}: {e}"));
    }
    migrator.close().await;

    let mut as_app = url::Url::parse(&url).expect("url");
    as_app.set_path(&name);
    as_app.set_username(&role).expect("set username");
    as_app.set_password(Some("test")).expect("set password");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(as_app.as_str())
        .await
        .expect("connect as restricted role");

    // Guard the guard: if this ever connects as a superuser again, say so loudly
    // instead of letting every isolation test pass for the wrong reason.
    let bypasses: bool = sqlx::query_scalar(
        "SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&pool)
    .await
    .expect("check role privileges");
    assert!(
        !bypasses,
        "Testrolle umgeht RLS — die Isolationstests waeren wirkungslos"
    );

    Some(pool)
}

macro_rules! require_db {
    () => {
        match pool().await {
            Some(p) => p,
            None => {
                eprintln!("SKIP: TEST_DATABASE_URL not set");
                return;
            }
        }
    };
}

async fn seed_user(pool: &PgPool, username: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO users (id, username, display_name) VALUES ($1, $2, $2)")
        .bind(id)
        .bind(username)
        .execute(pool)
        .await
        .expect("insert user");
    // Seeding runs with the tenant context set, because the taxonomy tables are
    // themselves RLS-protected.
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.user_id', $1, true)")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT app.seed_default_taxonomy($1)")
        .bind(id)
        .execute(&mut *tx)
        .await
        .expect("seed taxonomy");
    sqlx::query(
        "INSERT INTO bookings (id, user_id, period_year, period_month, kind, amount_cents, \
                               comment, origin, category_id, category_source) \
         VALUES (gen_random_uuid(), $1, 2026, 1, 'expense', 123456, 'Miete', \
                 'legacy_month_only', (SELECT id FROM categories WHERE name = 'Miete'), 'imported')",
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .expect("insert booking");
    tx.commit().await.unwrap();
    id
}

async fn tenant_tx(pool: &PgPool, user: Uuid) -> sqlx::Transaction<'static, sqlx::Postgres> {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.user_id', $1, true)")
        .bind(user.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx
}

async fn tenant_scoped_tables(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_class c \
           JOIN pg_namespace n ON n.oid = c.relnamespace \
           JOIN pg_attribute a ON a.attrelid = c.oid AND a.attname = 'user_id' AND a.attnum > 0 \
          WHERE n.nspname = 'public' AND c.relkind = 'r' \
          ORDER BY c.relname",
    )
    .fetch_all(pool)
    .await
    .expect("discover tables")
}

/// The guard that keeps working as the schema grows: a table added later with a
/// `user_id` column but no forced policy fails here, before anyone writes a handler
/// for it.
#[tokio::test]
async fn every_user_scoped_table_has_forced_rls() {
    let pool = require_db!();
    let unprotected: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_class c \
           JOIN pg_namespace n ON n.oid = c.relnamespace \
           JOIN pg_attribute a ON a.attrelid = c.oid AND a.attname = 'user_id' AND a.attnum > 0 \
          WHERE n.nspname = 'public' AND c.relkind = 'r' \
            AND (NOT c.relrowsecurity OR NOT c.relforcerowsecurity \
                 OR NOT EXISTS (SELECT 1 FROM pg_policies p \
                                 WHERE p.schemaname = 'public' AND p.tablename = c.relname))",
    )
    .fetch_all(&pool)
    .await
    .expect("query rls metadata");

    let unexpected: Vec<&String> = unprotected
        .iter()
        .filter(|t| !SYSTEM_SCOPE.contains(&t.as_str()))
        .collect();
    assert!(
        unexpected.is_empty(),
        "Tabellen ohne erzwungene RLS: {unexpected:?}"
    );

    let protected = tenant_scoped_tables(&pool).await.len() - SYSTEM_SCOPE.len();
    assert!(
        protected >= 15,
        "nur {protected} geschützte Tabellen gefunden"
    );
}

/// Table-driven cross-tenant probe. Every tenant-scoped table is queried with SQL
/// that carries no user predicate at all — exactly the "forgotten WHERE" case.
#[tokio::test]
async fn another_tenant_can_neither_read_nor_write_any_table() {
    let pool = require_db!();
    let alice = seed_user(&pool, "alice").await;
    let bob = seed_user(&pool, "bob").await;
    assert_ne!(alice, bob);

    let tables = tenant_scoped_tables(&pool).await;
    let mut tx = tenant_tx(&pool, bob).await;

    for table in tables
        .iter()
        .filter(|t| !SYSTEM_SCOPE.contains(&t.as_str()))
    {
        // Bob's own rows are excluded so any row seen belongs to Alice.
        let visible: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM {table} WHERE user_id <> '{bob}'"
        ))
        .fetch_one(&mut *tx)
        .await
        .unwrap_or_else(|e| panic!("select on {table}: {e}"));
        assert_eq!(visible, 0, "Tenant b sieht fremde Zeilen in {table}");

        let updated = sqlx::query(&format!("UPDATE {table} SET user_id = user_id"))
            .execute(&mut *tx)
            .await
            .map(|r| r.rows_affected())
            .unwrap_or(0);
        let own: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(updated, own as u64, "UPDATE auf {table} traf fremde Zeilen");
    }
    tx.rollback().await.unwrap();

    // Alice's data survived Bob's attempts untouched.
    let mut tx = tenant_tx(&pool, alice).await;
    let alice_bookings: i64 = sqlx::query_scalar("SELECT count(*) FROM bookings")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(alice_bookings, 1);
}

/// A forged `user_id` must be rejected by the policy's WITH CHECK, not merely
/// filtered on read.
#[tokio::test]
async fn inserting_a_row_for_another_tenant_is_refused() {
    let pool = require_db!();
    let alice = seed_user(&pool, "alice2").await;
    let bob = seed_user(&pool, "bob2").await;

    let mut tx = tenant_tx(&pool, bob).await;
    let err = sqlx::query(
        "INSERT INTO bookings (id, user_id, period_year, period_month, kind, amount_cents, \
                               comment, origin, booked_on) \
         VALUES (gen_random_uuid(), $1, 2026, 1, 'expense', 100, 'forged', 'manual', '2026-01-05')",
    )
    .bind(alice)
    .execute(&mut *tx)
    .await
    .expect_err("WITH CHECK muss die Fremdbuchung ablehnen");

    assert_eq!(
        err.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("42501"),
        "erwartet insufficient_privilege, war: {err}"
    );
}

/// Without a tenant context every query must return nothing. The dangerous failure
/// mode is the opposite: a missing context returning *everything*.
#[tokio::test]
async fn queries_without_a_tenant_context_see_nothing() {
    let pool = require_db!();
    seed_user(&pool, "alice3").await;

    let mut conn = pool.acquire().await.unwrap();
    for table in ["bookings", "categories", "category_rules", "fiscal_years"] {
        let n: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(
            n, 0,
            "{table} ohne Mandantenkontext sichtbar — RLS greift nicht"
        );
    }
}

/// The netting column and the reporting view are the two things every figure in the
/// application rests on, so they are asserted against the database itself, not only
/// against the pure engine.
#[tokio::test]
async fn generated_net_cents_and_the_ledger_view_behave() {
    let pool = require_db!();
    let alice = seed_user(&pool, "alice4").await;
    let mut tx = tenant_tx(&pool, alice).await;

    // The flatmate's share: 1100 out, 550 in, same category -> net 550.
    sqlx::query(
        "INSERT INTO bookings (id, user_id, period_year, period_month, kind, amount_cents, \
                               comment, origin, category_id, category_source) \
         SELECT gen_random_uuid(), $1, 2026, 2, k, a, 'Miete', 'legacy_month_only', \
                (SELECT id FROM categories WHERE name = 'Miete'), 'imported' \
           FROM (VALUES ('expense', 110000::bigint), ('income', 55000::bigint)) v(k, a)",
    )
    .bind(alice)
    .execute(&mut *tx)
    .await
    .unwrap();

    // A transfer must move the balance but no category or type figure.
    sqlx::query(
        "INSERT INTO bookings (id, user_id, period_year, period_month, kind, amount_cents, \
                               comment, origin) \
         VALUES (gen_random_uuid(), $1, 2026, 2, 'transfer', 9999999, 'abgehoben', \
                 'legacy_month_only')",
    )
    .bind(alice)
    .execute(&mut *tx)
    .await
    .unwrap();

    let row = sqlx::query(
        "SELECT COALESCE(SUM(net_cents), 0)::bigint AS net, \
                COALESCE(SUM(amount_cents) FILTER (WHERE kind = 'income'), 0)::bigint AS inc, \
                COALESCE(SUM(amount_cents) FILTER (WHERE kind = 'expense'), 0)::bigint AS exp \
           FROM v_ledger WHERE period_month = 2 AND category_name = 'Miete'",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(row.get::<i64, _>("net"), 55_000, "Miete netto 550,00");
    assert_eq!(row.get::<i64, _>("inc"), 55_000);
    assert_eq!(row.get::<i64, _>("exp"), 110_000);

    let transfer_net: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(net_cents), 0)::bigint FROM v_ledger WHERE kind = 'transfer'",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(transfer_net, 0, "Umbuchungen tragen strukturell 0 bei");

    // Drafts must not reach any total.
    sqlx::query(
        "INSERT INTO bookings (id, user_id, period_year, period_month, kind, amount_cents, \
                               comment, origin, status) \
         VALUES (gen_random_uuid(), $1, 2026, 2, 'expense', 99999900, 'Entwurf', \
                 'legacy_month_only', 'draft')",
    )
    .bind(alice)
    .execute(&mut *tx)
    .await
    .unwrap();
    let after: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(net_cents), 0)::bigint FROM v_ledger WHERE period_month = 2",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(after, 55_000, "Entwürfe dürfen in keiner Summe auftauchen");
}

/// The schema-level guards that make the categorisation state machine and the
/// month/day model unfalsifiable.
#[tokio::test]
async fn schema_constraints_reject_impossible_bookings() {
    let pool = require_db!();
    let alice = seed_user(&pool, "alice5").await;

    let cases: &[(&str, &str)] = &[
        (
            "ein neuer Eintrag ohne Datum",
            "INSERT INTO bookings (id, user_id, period_year, period_month, kind, amount_cents, \
             comment, origin) VALUES (gen_random_uuid(), $1, 2026, 1, 'expense', 100, 'x', 'manual')",
        ),
        (
            "ein negativer Betrag",
            "INSERT INTO bookings (id, user_id, period_year, period_month, kind, amount_cents, \
             comment, origin) VALUES (gen_random_uuid(), $1, 2026, 1, 'expense', -100, 'x', \
             'legacy_month_only')",
        ),
        (
            "eine Kategorie mit Quelle 'unresolved'",
            "INSERT INTO bookings (id, user_id, period_year, period_month, kind, amount_cents, \
             comment, origin, category_id, category_source) VALUES (gen_random_uuid(), $1, 2026, \
             1, 'expense', 100, 'x', 'legacy_month_only', \
             (SELECT id FROM categories WHERE name = 'Miete'), 'unresolved')",
        ),
        (
            "ein Tag außerhalb seines Monats",
            "INSERT INTO bookings (id, user_id, period_year, period_month, kind, amount_cents, \
             comment, origin, booked_on) VALUES (gen_random_uuid(), $1, 2026, 1, 'expense', 100, \
             'x', 'manual', '2026-02-05')",
        ),
    ];

    for (what, sql) in cases {
        let mut tx = tenant_tx(&pool, alice).await;
        let result = sqlx::query(sql).bind(alice).execute(&mut *tx).await;
        assert!(result.is_err(), "{what} haette abgelehnt werden muessen");
        let err = result.unwrap_err();
        assert_eq!(
            err.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("23514"),
            "{what}: erwartet check_violation, war: {err}"
        );
    }
}

/// Case-insensitive rule matching is enforced by a generated column plus a unique
/// index, so it cannot be bypassed by forgetting to lower() at a call site.
#[tokio::test]
async fn rule_keys_collide_case_insensitively() {
    let pool = require_db!();
    let alice = seed_user(&pool, "alice6").await;
    let mut tx = tenant_tx(&pool, alice).await;

    sqlx::query(
        "INSERT INTO category_rules (id, user_id, pattern, category_id) \
         VALUES (gen_random_uuid(), $1, 'Spotify', \
                 (SELECT id FROM categories WHERE name = 'Abos & Streaming'))",
    )
    .bind(alice)
    .execute(&mut *tx)
    .await
    .unwrap();

    let err = sqlx::query(
        "INSERT INTO category_rules (id, user_id, pattern, category_id) \
         VALUES (gen_random_uuid(), $1, '  spotify  ', \
                 (SELECT id FROM categories WHERE name = 'Sonstiges'))",
    )
    .bind(alice)
    .execute(&mut *tx)
    .await
    .expect_err("zweite Regel mit gleichem Schluessel muss scheitern");
    assert_eq!(
        err.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("23505")
    );
}
