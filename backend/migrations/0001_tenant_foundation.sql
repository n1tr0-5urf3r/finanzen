-- Tenant isolation foundation. Must run before every other migration, because each
-- user-scoped table calls app.enable_tenant_rls() at the end of its own migration.

CREATE SCHEMA IF NOT EXISTS app;

-- The tenant predicate. `true` as the second argument to current_setting makes it
-- return NULL instead of raising when the setting is absent, so a query issued with
-- no tenant context matches no rows rather than erroring in an opaque way.
CREATE OR REPLACE FUNCTION app.current_user_id() RETURNS uuid
LANGUAGE sql STABLE AS $$
  SELECT NULLIF(current_setting('app.user_id', true), '')::uuid
$$;

-- One call site for the RLS boilerplate, so a new table cannot get a subtly different
-- policy. ENABLE alone is not enough: without FORCE, the table owner bypasses the
-- policy, and the application owns its own tables so that sqlx::migrate! can run.
CREATE OR REPLACE FUNCTION app.enable_tenant_rls(tbl regclass) RETURNS void
LANGUAGE plpgsql AS $$
BEGIN
  EXECUTE format('ALTER TABLE %s ENABLE ROW LEVEL SECURITY', tbl);
  EXECUTE format('ALTER TABLE %s FORCE  ROW LEVEL SECURITY', tbl);
  EXECUTE format($f$CREATE POLICY tenant_isolation ON %s
                    USING (user_id = app.current_user_id())
                    WITH CHECK (user_id = app.current_user_id())$f$, tbl);
END $$;
