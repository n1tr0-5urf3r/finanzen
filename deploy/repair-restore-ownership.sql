-- One-time repair for a backup taken BEFORE deploy/backup.sh kept ownership.
--
-- Those dumps were made with --no-owner, so restoring them as the superuser
-- leaves every table owned by postgres, and the app cannot start ("permission
-- denied for table _sqlx_migrations"). Run this once, as the superuser, right
-- after such a restore:
--
--   docker compose exec -T postgres psql -U postgres -d finanzen < deploy/repair-restore-ownership.sql
--
-- Backups taken since then keep ownership and do not need it. Running it on a
-- healthy database changes nothing.

-- Hands every object in the application's schemas back to the application role.
DO $$
DECLARE r record;
BEGIN
  FOR r IN SELECT n.nspname, c.relname, c.relkind FROM pg_class c
             JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE n.nspname IN ('public','app') AND c.relkind IN ('r','v','S','m','p')
              AND pg_get_userbyid(c.relowner) <> 'finanzen_app'
              -- A sequence that belongs to a column moves with its table.
              AND NOT EXISTS (SELECT 1 FROM pg_depend d
                               WHERE d.objid = c.oid AND d.deptype IN ('a','i')) LOOP
    EXECUTE format('ALTER %s %I.%I OWNER TO finanzen_app',
      CASE r.relkind WHEN 'v' THEN 'VIEW' WHEN 'S' THEN 'SEQUENCE'
                     WHEN 'm' THEN 'MATERIALIZED VIEW' ELSE 'TABLE' END, r.nspname, r.relname);
  END LOOP;
  FOR r IN SELECT n.nspname, p.oid::regprocedure AS sig FROM pg_proc p
             JOIN pg_namespace n ON n.oid = p.pronamespace
            WHERE n.nspname IN ('public','app') AND pg_get_userbyid(p.proowner) <> 'finanzen_app' LOOP
    EXECUTE format('ALTER FUNCTION %s OWNER TO finanzen_app', r.sig);
  END LOOP;
  EXECUTE 'ALTER SCHEMA app OWNER TO finanzen_app';
END $$;
