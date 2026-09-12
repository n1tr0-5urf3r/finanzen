-- KitchenOwl is a SEPARATE, PARALLEL LEDGER. These tables are a faithful local mirror
-- of the household's shared expenses. They are never summed with the personal
-- bookings, and a pull never writes a booking. The only connection between the two
-- ledgers is an optional, reversible LINK (bookings.external_source/external_id).
--
-- The two will never fully reconcile, by design: KitchenOwl holds data the finances
-- do not track and vice versa. Nothing here treats non-matching as an error state.

CREATE TABLE ko_members (
  user_id        uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  member_id      bigint NOT NULL,
  name           text NOT NULL,
  username       text,
  is_admin       boolean NOT NULL DEFAULT false,
  is_owner       boolean NOT NULL DEFAULT false,
  -- KitchenOwl reports this as a float carrying artifacts
  -- (observed: -149.16999999999217). Rounded half-away-from-zero at the boundary.
  balance_cents  bigint NOT NULL DEFAULT 0,
  is_me          boolean NOT NULL DEFAULT false,
  fetched_at     timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (user_id, member_id)
);
SELECT app.enable_tenant_rls('ko_members');

CREATE TABLE ko_categories (
  user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  category_id bigint NOT NULL,
  name        text NOT NULL,
  color_argb  bigint,
  budget_cents bigint,
  -- Optional hint only. The two taxonomies are unrelated and are never auto-mapped;
  -- this preselects a dropdown, it does not decide anything.
  finances_category_id uuid REFERENCES categories(id) ON DELETE SET NULL,
  fetched_at  timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (user_id, category_id)
);
SELECT app.enable_tenant_rls('ko_categories');

CREATE TABLE ko_expenses (
  id             uuid PRIMARY KEY,
  user_id        uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  external_id    bigint NOT NULL,
  name           text NOT NULL,
  description    text,
  expense_date   date NOT NULL,
  amount_cents   bigint NOT NULL,
  own_share_cents bigint NOT NULL,
  paid_by_id     bigint,
  -- [{memberId, factor, shareCents}] — factor is an INTEGER WEIGHT, not a percentage.
  paid_for       jsonb NOT NULL DEFAULT '[]'::jsonb,
  ko_category_id bigint,
  ko_category_name text,
  exclude_from_statistics boolean NOT NULL DEFAULT false,
  remote_hash    text NOT NULL,
  linked_booking_id uuid REFERENCES bookings(id) ON DELETE SET NULL,
  updated_at     timestamptz NOT NULL DEFAULT now(),
  UNIQUE (user_id, external_id)
);
CREATE INDEX ko_expenses_date_idx   ON ko_expenses (user_id, expense_date DESC);
CREATE INDEX ko_expenses_linked_idx ON ko_expenses (user_id, linked_booking_id);
SELECT app.enable_tenant_rls('ko_expenses');

-- Pulled items land here and are NEVER auto-booked. match_candidates carries the
-- suggested link to an existing booking; without it, 63 of 211 expenses in the
-- overlapping period would double-post.
CREATE TABLE ko_drafts (
  id            uuid PRIMARY KEY,
  user_id       uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  kind          text NOT NULL CHECK (kind IN ('expense','settlement','drift')),
  ko_expense_id uuid REFERENCES ko_expenses(id) ON DELETE CASCADE,
  status        text NOT NULL DEFAULT 'open'
                  CHECK (status IN ('open','likely_duplicate','possible_duplicate',
                                    'ignored_by_default','confirmed','discarded')),
  match_candidates jsonb NOT NULL DEFAULT '[]'::jsonb,
  payload       jsonb NOT NULL DEFAULT '{}'::jsonb,
  created_at    timestamptz NOT NULL DEFAULT now(),
  resolved_at   timestamptz
);
CREATE INDEX ko_drafts_open_idx ON ko_drafts (user_id, status, created_at DESC);
SELECT app.enable_tenant_rls('ko_drafts');

-- Transactional outbox. One intent per booking (PRIMARY KEY), so the idempotency key
-- is tied to the booking rather than to the payload: editing a queued push replaces
-- it in place instead of becoming a second post.
CREATE TABLE ko_push_intents (
  booking_id    uuid PRIMARY KEY REFERENCES bookings(id) ON DELETE CASCADE,
  user_id       uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  state         text NOT NULL DEFAULT 'queued'
                  CHECK (state IN ('queued','sending','pushed','failed','abandoned','retracted')),
  idempotency_key text NOT NULL,
  marker        text NOT NULL,
  payload       jsonb NOT NULL,
  external_id   bigint,
  attempts      integer NOT NULL DEFAULT 0,
  last_error    text,
  attempt_started_at timestamptz,
  next_attempt_at    timestamptz,
  created_at    timestamptz NOT NULL DEFAULT now(),
  updated_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ko_push_queue_idx ON ko_push_intents (user_id, state, created_at);
SELECT app.enable_tenant_rls('ko_push_intents');

CREATE TABLE sync_runs (
  id            uuid PRIMARY KEY,
  user_id       uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  kind          text NOT NULL,
  status        text NOT NULL CHECK (status IN ('running','success','partial','failed')),
  started_at    timestamptz NOT NULL DEFAULT now(),
  finished_at   timestamptz,
  created_count  integer NOT NULL DEFAULT 0,
  updated_count  integer NOT NULL DEFAULT 0,
  archived_count integer NOT NULL DEFAULT 0,
  failed_count   integer NOT NULL DEFAULT 0,
  error         text
);
CREATE INDEX sync_runs_recent_idx ON sync_runs (user_id, kind, started_at DESC);
SELECT app.enable_tenant_rls('sync_runs');

-- Watermark for the incremental cursor pull (KitchenOwl pages via ?startAfterId=).
CREATE TABLE ko_sync_state (
  user_id       uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  max_seen_id   bigint NOT NULL DEFAULT 0,
  household_id  bigint,
  household_name text,
  metadata_fetched_at timestamptz,
  last_error    text,
  balance_snapshot jsonb NOT NULL DEFAULT '{}'::jsonb
);
SELECT app.enable_tenant_rls('ko_sync_state');
