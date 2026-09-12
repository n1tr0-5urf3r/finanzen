-- The core ledger table.
--
-- Three decisions are encoded structurally here, because each of them is something
-- an implementation would otherwise have to remember at every call site:
--
-- 1. The MONTH is the canonical key and the day is optional precision. 1404 of ~1878
--    rows genuinely have no day, so a nullable date with generated period columns
--    would leave the majority NULL and unindexable. period_ord gives single-column
--    ordering, cross-year range scans and a single-column window frame.
--
-- 2. amount_cents is always POSITIVE and the direction lives in `kind`. This holds
--    across all 1878 source rows, so it is enforceable — and a signed column would
--    make every sign error silent.
--
-- 3. net_cents is GENERATED, expense-positive. This is the netting rule compiled into
--    the schema: category net is a plain SUM(net_cents) with no CASE and no
--    two-subquery subtraction, transfers contribute 0 *structurally* so a forgotten
--    `AND kind <> 'transfer'` is harmless, and net-inflow categories fall out with
--    the correct sign automatically.
CREATE TABLE bookings (
  id            uuid PRIMARY KEY,
  user_id       uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,

  period_year   smallint NOT NULL CHECK (period_year BETWEEN 1990 AND 2200),
  period_month  smallint NOT NULL CHECK (period_month BETWEEN 1 AND 12),
  period_ord    integer GENERATED ALWAYS AS
                  (period_year::int * 12 + period_month::int - 1) STORED,
  booked_on     date,

  kind          text   NOT NULL CHECK (kind IN ('income','expense','transfer')),
  amount_cents  bigint NOT NULL CHECK (amount_cents > 0),
  net_cents     bigint GENERATED ALWAYS AS (
                    CASE kind WHEN 'expense' THEN  amount_cents
                              WHEN 'income'  THEN -amount_cents
                              ELSE 0 END) STORED,

  comment       text NOT NULL CHECK (btrim(comment) <> ''),
  match_key     text GENERATED ALWAYS AS (lower(btrim(comment))) STORED,

  category_id      uuid REFERENCES categories(id) ON DELETE SET NULL,
  category_source  text NOT NULL DEFAULT 'unresolved'
                     CHECK (category_source IN ('unresolved','rule','manual','imported')),
  resolved_rule_id uuid REFERENCES category_rules(id) ON DELETE SET NULL,

  account_id         uuid REFERENCES accounts(id) ON DELETE SET NULL,
  counter_account_id uuid REFERENCES accounts(id) ON DELETE SET NULL,

  tax_relevant  boolean NOT NULL DEFAULT false,
  tax_note      text,
  tax_seq       integer,          -- frozen running number once the year is tax-locked

  status        text NOT NULL DEFAULT 'confirmed' CHECK (status IN ('draft','confirmed')),
  origin        text NOT NULL CHECK (origin IN
                  ('manual','legacy_month_only','sheet_2026','recurring','kitchenowl')),

  external_source text,
  external_id     text,

  shared              boolean NOT NULL DEFAULT false,
  import_fingerprint  text,

  created_at    timestamptz NOT NULL DEFAULT now(),
  updated_at    timestamptz NOT NULL DEFAULT now(),

  CONSTRAINT bookings_date_in_period CHECK (
    booked_on IS NULL
    OR (booked_on >= make_date(period_year::int, period_month::int, 1)
        AND booked_on < (make_date(period_year::int, period_month::int, 1)
                         + INTERVAL '1 month')::date)),

  -- "New bookings require a date" keyed on provenance, not on a year threshold, so
  -- the rule survives a re-import and a deliberate month-only historical entry.
  CONSTRAINT bookings_date_required CHECK (
    origin = 'legacy_month_only' OR booked_on IS NOT NULL),

  -- The categorisation state machine, made unfalsifiable.
  CONSTRAINT bookings_category_source CHECK (
    (category_id IS NULL     AND category_source = 'unresolved') OR
    (category_id IS NOT NULL AND category_source <> 'unresolved')),
  CONSTRAINT bookings_rule_link CHECK (
    (category_source = 'rule') = (resolved_rule_id IS NOT NULL)),

  CONSTRAINT bookings_transfer_accounts CHECK (
    kind <> 'transfer' OR account_id IS NULL OR counter_account_id IS NULL
    OR account_id <> counter_account_id),
  CONSTRAINT bookings_external_pair CHECK (
    (external_source IS NULL) = (external_id IS NULL))
);

-- Report paths. Partial on status='confirmed' so drafts (KitchenOwl pulls,
-- materialised recurring items) neither reach a total nor bloat the reporting index.
CREATE INDEX bookings_period_idx ON bookings (user_id, period_ord)
  INCLUDE (net_cents, category_id) WHERE status = 'confirmed';
CREATE INDEX bookings_category_idx ON bookings (user_id, category_id, period_ord)
  WHERE status = 'confirmed';
CREATE INDEX bookings_tax_idx ON bookings (user_id, period_year, period_ord)
  WHERE tax_relevant AND status = 'confirmed';

-- Recategorisation path.
CREATE INDEX bookings_matchkey_idx ON bookings (user_id, match_key)
  WHERE category_source IN ('rule','unresolved');

-- Review queues.
CREATE INDEX bookings_unresolved_idx ON bookings (user_id, period_ord)
  WHERE category_source = 'unresolved';
CREATE INDEX bookings_draft_idx ON bookings (user_id, created_at) WHERE status = 'draft';

-- Re-syncing an external object is a structural no-op, not a handler-level check.
CREATE UNIQUE INDEX bookings_external_key ON bookings (user_id, external_source, external_id)
  WHERE external_source IS NOT NULL;
CREATE UNIQUE INDEX bookings_import_fingerprint_key ON bookings (user_id, import_fingerprint)
  WHERE import_fingerprint IS NOT NULL;

SELECT app.enable_tenant_rls('bookings');
