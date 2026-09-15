#!/usr/bin/env sh
# A restorable backup of a running instance.
#
# WHY NOT JUST COPY data/postgres: those files are only consistent while the
# server is stopped. Copying them live gives you a torn snapshot — it will look
# fine, and it may refuse to start when you need it. `pg_dump` asks the running
# server for a consistent view instead, which is the difference between a backup
# and a directory of bytes.
#
# Receipts ARE just files, so they are copied as they are.
#
#   deploy/backup.sh [target-directory]      # default: ./backups
set -eu

cd "$(dirname "$0")/.."
TARGET="${1:-./backups}"
STAMP="$(date +%Y-%m-%d_%H%M)"
mkdir -p "$TARGET"

# shellcheck disable=SC1091
[ -f .env ] && . ./.env

DB="${POSTGRES_DB:-finanzen}"
USER="${POSTGRES_USER:-postgres}"

echo "→ database"
# --clean --if-exists so the dump can be restored over an existing database;
# --no-owner so it restores under whatever role does the restoring.
docker compose exec -T postgres pg_dump -U "$USER" -d "$DB" \
  --clean --if-exists --no-owner \
  | gzip > "$TARGET/finanzen_${STAMP}.sql.gz"

echo "→ receipts"
tar -czf "$TARGET/receipts_${STAMP}.tar.gz" -C "${DATA_ROOT:-./data}" receipts

echo "→ done"
ls -lh "$TARGET" | tail -2
echo
echo "Restore:  gunzip -c finanzen_${STAMP}.sql.gz | docker compose exec -T postgres psql -U $USER -d $DB"
