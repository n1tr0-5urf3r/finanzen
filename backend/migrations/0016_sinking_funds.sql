-- Rücklagen: the annual and quarterly lumps, accrued monthly.
--
-- The real ledger has Kfz-Versicherung 307,00 in Juli, Nebenkosten 1.440,00 in
-- August, vServer 60,00, GEZ 48,00 quarterly. Each lands in one month, so that month
-- looks terrible and the other eleven look better than they are — the monthly saldo
-- is telling the truth about a month and lying about a year.
--
-- A fund states what a known lump costs per year and when it falls due. Nothing here
-- books anything: this table is an EXPECTATION, and the bookings it is compared
-- against are the ordinary ones in the ledger. A fund that created bookings would
-- double-count the very spending it exists to anticipate.
CREATE TABLE sinking_funds (
  id           uuid PRIMARY KEY,
  user_id      uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name         text NOT NULL CHECK (btrim(name) <> ''),
  -- Nullable on purpose: a fund can be a plain reminder ("Urlaub") before the user
  -- decides which category it will be booked into. Without one, only the accrual is
  -- known and the spent-so-far column stays empty rather than guessing.
  category_id  uuid REFERENCES categories(id) ON DELETE SET NULL,
  annual_cents bigint NOT NULL CHECK (annual_cents > 0),
  -- The month the bill actually arrives, 1..12. Used to say "fällig im Juli" and to
  -- tell "not accrued yet" apart from "already paid".
  due_month    smallint NOT NULL CHECK (due_month BETWEEN 1 AND 12),
  note         text,
  active       boolean NOT NULL DEFAULT true,
  sort_order   smallint NOT NULL DEFAULT 0,
  created_at   timestamptz NOT NULL DEFAULT now(),
  updated_at   timestamptz NOT NULL DEFAULT now()
);

-- One fund per category, so the spent-so-far figure can never be claimed twice by
-- two funds pointing at the same bookings. Partial, because several funds may sit
-- without a category at all.
CREATE UNIQUE INDEX sinking_funds_category_key
  ON sinking_funds (user_id, category_id) WHERE category_id IS NOT NULL;

SELECT app.enable_tenant_rls('sinking_funds');
