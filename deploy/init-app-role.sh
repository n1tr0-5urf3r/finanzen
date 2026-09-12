#!/bin/bash
# Runs once, on first initialisation of the Postgres data directory.
#
# POSTGRES_USER is a SUPERUSER, and a superuser bypasses row-level security
# unconditionally — which would make this application's tenant isolation silently
# inert. So the app connects as a separate NOSUPERUSER role that OWNS its own
# database: it can still run sqlx migrations at boot, and FORCE ROW LEVEL SECURITY
# means the policies apply to it anyway.
#
# The backend refuses to start if its role can bypass RLS, so a mistake here fails
# loudly at boot rather than leaking data quietly.
set -euo pipefail

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-SQL
    CREATE ROLE ${APP_DB_USER} LOGIN PASSWORD '${APP_DB_PASSWORD}'
        NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS;
    ALTER DATABASE ${POSTGRES_DB} OWNER TO ${APP_DB_USER};
    GRANT ALL ON SCHEMA public TO ${APP_DB_USER};
SQL
