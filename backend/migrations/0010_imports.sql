CREATE TABLE import_batches (
  id         uuid PRIMARY KEY,
  user_id    uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  source     text NOT NULL CHECK (source IN ('xlsx_2026','ods_legacy','csv','kitchenowl')),
  filename   text,
  sha256     text,
  sheet      text,
  status     text NOT NULL DEFAULT 'preview'
               CHECK (status IN ('preview','review','applied','discarded','failed')),
  row_count  integer NOT NULL DEFAULT 0,
  stats      jsonb NOT NULL DEFAULT '{}'::jsonb,
  created_at timestamptz NOT NULL DEFAULT now(),
  applied_at timestamptz
);
-- Re-uploading a byte-identical file returns the existing preview instead of
-- creating a second job.
CREATE UNIQUE INDEX import_batches_file_key ON import_batches (user_id, sha256)
  WHERE sha256 IS NOT NULL;
SELECT app.enable_tenant_rls('import_batches');

CREATE TABLE import_rows (
  id           uuid PRIMARY KEY,
  user_id      uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  batch_id     uuid NOT NULL REFERENCES import_batches(id) ON DELETE CASCADE,
  -- Provenance down to the cell, e.g. 'ods!2023-2025:A664'. When a number disagrees
  -- by a cent, you can point at the original cell.
  source_ref   text NOT NULL,
  raw          jsonb NOT NULL,
  period_year  smallint,
  period_month smallint,
  kind         text,
  amount_cents bigint,
  comment      text,
  tax_relevant boolean NOT NULL DEFAULT false,
  suggested_category_id uuid REFERENCES categories(id) ON DELETE SET NULL,
  suggestion_kind  text CHECK (suggestion_kind IN
                     ('exact','prefix','token','fuzzy','history','transfer')),
  suggestion_score real,
  status       text NOT NULL DEFAULT 'new' CHECK (status IN ('new','duplicate','error')),
  decision     text NOT NULL DEFAULT 'pending'
                 CHECK (decision IN ('pending','accepted','skipped','rejected')),
  decided_category_id uuid REFERENCES categories(id) ON DELETE SET NULL,
  create_rule  boolean NOT NULL DEFAULT false,
  fingerprint  text,
  booking_id   uuid REFERENCES bookings(id) ON DELETE SET NULL,
  error        text,
  UNIQUE (batch_id, source_ref)
);
CREATE INDEX import_rows_queue_idx ON import_rows (user_id, batch_id, decision);
SELECT app.enable_tenant_rls('import_rows');

ALTER TABLE bookings ADD COLUMN import_row_id uuid
  REFERENCES import_rows(id) ON DELETE SET NULL;

-- The review queue is keyed by DISTINCT COMMENT, not by row: ~240 decisions instead
-- of ~362. Confirming one fixes every occurrence.
CREATE TABLE import_review_items (
  id             uuid PRIMARY KEY,
  user_id        uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  batch_id       uuid NOT NULL REFERENCES import_batches(id) ON DELETE CASCADE,
  comment        text NOT NULL,
  normalized     text NOT NULL,
  row_count      integer NOT NULL DEFAULT 0,
  expense_cents  bigint NOT NULL DEFAULT 0,
  income_cents   bigint NOT NULL DEFAULT 0,
  suggestions    jsonb NOT NULL DEFAULT '[]'::jsonb,
  weak_hints     jsonb NOT NULL DEFAULT '[]'::jsonb,
  ambiguous      boolean NOT NULL DEFAULT false,
  suggested_kind text CHECK (suggested_kind IN ('income','expense','transfer')),
  status         text NOT NULL DEFAULT 'open' CHECK (status IN ('open','resolved','skipped')),
  resolved_category_id uuid REFERENCES categories(id) ON DELETE SET NULL,
  resolved_kind  text CHECK (resolved_kind IN ('income','expense','transfer')),
  rule_created   boolean NOT NULL DEFAULT false,
  UNIQUE (batch_id, normalized)
);
CREATE INDEX import_review_order_idx
  ON import_review_items (user_id, batch_id, status, row_count DESC);
SELECT app.enable_tenant_rls('import_review_items');
