#!/usr/bin/env bash
# Does a backup actually restore into a database the app can use?
#
# Builds a database the way production builds it (the init script, then the app's
# own migrations), dumps it with the same pg_dump flags as deploy/backup.sh,
# restores that onto a fresh "new host", and starts the app against it. Throwaway
# containers only; no real data is touched.
#
#   tools/backup-restore-check.sh                      # the flags backup.sh uses
#   tools/backup-restore-check.sh "--clean --no-owner" # try others
#
# "B (restored): ready" and a 200 login is a backup that works.
set -euo pipefail
FLAGS="${1:---clean --if-exists}"
IMAGE="${IMAGE:-ghcr.io/n1tr0-5urf3r/finanzen:latest}"
ROLE="$(cd "$(dirname "$0")/.." && pwd)/deploy/init-app-role.sh"
NET=restore-test
cleanup() { docker rm -f rt-a rt-b rt-app >/dev/null 2>&1 || true; docker network rm $NET >/dev/null 2>&1 || true; }
cleanup; docker network create $NET >/dev/null

pg() { # name
  docker run -d --name "$1" --network $NET -e POSTGRES_DB=finanzen -e POSTGRES_USER=postgres \
    -e POSTGRES_PASSWORD=pw -e APP_DB_USER=finanzen_app -e APP_DB_PASSWORD=app-pw \
    -v "$ROLE":/docker-entrypoint-initdb.d/10-app-role.sh:ro postgres:17-alpine >/dev/null
  for i in $(seq 1 30); do docker exec "$1" pg_isready -U postgres -q && sleep 1 && docker exec "$1" pg_isready -U postgres -q && return; sleep 1; done
}
app() { # db-host -> prints ready/failed
  docker rm -f rt-app >/dev/null 2>&1 || true
  docker run -d --name rt-app --network $NET -p 127.0.0.1:3197:3100 \
    -e APP_SESSION_SECRET=restore-test-secret-that-is-long-enough -e APP_PUBLIC_URL=http://127.0.0.1:3197 \
    -e DATABASE_URL="postgres://finanzen_app:app-pw@$1:5432/finanzen" -e KITCHENOWL_SYNC_ON_START=false \
    "$IMAGE" >/dev/null
  for i in $(seq 1 30); do curl -fsS http://127.0.0.1:3197/api/v1/ready >/dev/null 2>&1 && { echo ready; return; }; sleep 1; done
  echo "FAILED: $(docker logs rt-app 2>&1 | grep -i -m1 'error')"
}

pg rt-a
echo "A (original host): $(app rt-a)"
# Something to find again after the restore.
curl -fsS -X POST http://127.0.0.1:3197/api/v1/auth/setup -H 'Content-Type: application/json' \
  -H 'Origin: http://127.0.0.1:3197' -d '{"username":"probe","displayName":"Probe","password":"ein-langes-passwort"}' >/dev/null
docker exec rt-a pg_dump -U postgres -d finanzen $FLAGS > /tmp/rt-dump.sql
docker rm -f rt-app >/dev/null

pg rt-b          # a fresh host: the role exists, the tables do not
docker exec -i rt-b psql -q -U postgres -d finanzen < /tmp/rt-dump.sql >/dev/null 2>&1
echo "B (restored):      $(app rt-b)"
echo "  table owners:    $(docker exec rt-b psql -At -U postgres -d finanzen -c "select string_agg(distinct tableowner, ',') from pg_tables where schemaname='public'")"
echo "  login after:     $(curl -s -o /dev/null -w '%{http_code}' -X POST http://127.0.0.1:3197/api/v1/auth/login -H 'Content-Type: application/json' -H 'Origin: http://127.0.0.1:3197' -d '{"username":"probe","password":"ein-langes-passwort"}')"
rm -f /tmp/rt-dump.sql
cleanup
