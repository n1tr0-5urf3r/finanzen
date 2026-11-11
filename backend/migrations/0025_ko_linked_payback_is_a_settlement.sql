-- A bank booking linked to a KitchenOwl payback is a settlement, not a purchase.
--
-- KitchenOwl records a settlement between members as an expense of its own — the
-- "Money transfer" / "Geldtransfer" entry, paid by one member for the other and
-- flagged `exclude_from_statistics`. Linking the bank transfer to that entry is the
-- natural thing to do.
-- But 0023 only counted a booking as a settlement when it was NOT linked to an
-- expense, and 0022 counted a linked booking at my share of the expense — of a
-- payback, 0. So a linked settlement
--
-- * counted nothing at all: what the bank shows disappeared from every
--   statistic; and
-- * did not close a window, so the next settlement, possibly a year later,
--   carried the netting of every linked purchase and household share since the
--   last unlinked one: a transfer of 30,00 paid to a flatmate could count as
--   several hundred euros of income.
--
-- 0024 already reads the flag this way — "a payback is a settlement" — when it
-- leaves those rows out of the household shares. This applies the same reading to
-- links, with one guard: a confirmed income or expense booking is a settlement,
-- whatever its category, and not a linked share, when the mirrored expense it is
-- linked to
--
-- * carries `exclude_from_statistics`, and
-- * moved money one way only: my share is 0 (I paid it for the others) or the
--   whole amount (they paid it to me). That is the shape of every payback
--   KitchenOwl creates.
--
-- The flag is also a toggle in KitchenOwl. A real purchase excluded there by hand
-- is still split between us, so the second condition keeps it a linked share at my
-- share, as 0022 decided. The flag is read from the mirror only; a push from this
-- app never creates a payback. Its window and netting are exactly those of any
-- other settlement.
--
-- Only `v_counted_booking` changes, with the same columns, so `v_ledger` and
-- `v_ledger_personal` pick it up as they are.
CREATE OR REPLACE VIEW v_counted_booking AS
WITH payback AS MATERIALIZED (
  SELECT b.id
    FROM bookings b
    JOIN ko_expenses e
      ON e.user_id = b.user_id
     -- CASE, not a bare cast, as in `v_ko_booking_share`: a non-numeric id must
     -- not raise whatever order the planner evaluates the conditions in.
     AND e.external_id = CASE WHEN b.external_id ~ '^[0-9]{1,18}$'
                              THEN b.external_id::bigint END
   WHERE b.external_source = 'kitchenowl'
     AND e.exclude_from_statistics
     AND (e.own_share_cents = 0 OR e.own_share_cents = e.amount_cents)
),
share AS MATERIALIZED (
  SELECT sh.booking_id, sh.user_id, b.kind, b.status, b.period_ord, b.net_cents,
         sh.counted_amount_cents,
         (CASE b.kind WHEN 'expense' THEN  sh.counted_amount_cents
                      WHEN 'income'  THEN -sh.counted_amount_cents
                      ELSE 0 END)::bigint AS counted_net_cents
    FROM v_ko_booking_share sh
    JOIN bookings b ON b.id = sh.booking_id
   WHERE NOT EXISTS (SELECT 1 FROM payback p WHERE p.id = sh.booking_id)
),
settlement AS MATERIALIZED (
  SELECT b.id, b.user_id, b.kind, b.period_ord, b.net_cents,
         lag(b.period_ord) OVER (PARTITION BY b.user_id
                                 ORDER BY b.period_ord, b.booked_on NULLS FIRST,
                                          b.created_at, b.id) AS previous_ord
    FROM bookings b
    LEFT JOIN categories c ON c.id = b.category_id
   WHERE b.status = 'confirmed'
     AND b.kind IN ('income', 'expense')
     AND ((b.external_source IS DISTINCT FROM 'kitchenowl'
           AND (b.external_source = 'kitchenowl_settlement'
                OR lower(c.name) = lower('Haushaltsausgleich')))
          OR EXISTS (SELECT 1 FROM payback p WHERE p.id = b.id))
),
-- O per period: bank net minus counted net of my confirmed linked bookings.
linked AS MATERIALIZED (
  SELECT user_id, period_ord, sum(net_cents - counted_net_cents) AS others_cents
    FROM share
   WHERE status = 'confirmed'
   GROUP BY user_id, period_ord
),
-- M per period: my shares of what the flatmates paid, signed like `net_cents`.
household AS MATERIALIZED (
  SELECT user_id, period_ord, sum(own_share_cents) AS mine_cents
    FROM v_ko_household_share
   GROUP BY user_id, period_ord
),
others_in_window AS (
  SELECT s.id, sum(o.others_cents) AS others_cents
    FROM settlement s
    JOIN linked o
      ON o.user_id = s.user_id
     AND o.period_ord <= s.period_ord
     AND (s.previous_ord IS NULL OR o.period_ord > s.previous_ord)
   GROUP BY s.id
),
mine_in_window AS (
  SELECT s.id, sum(h.mine_cents) AS mine_cents
    FROM settlement s
    JOIN household h
      ON h.user_id = s.user_id
     AND h.period_ord <= s.period_ord
     AND (s.previous_ord IS NULL OR h.period_ord > s.previous_ord)
   GROUP BY s.id
),
settled AS MATERIALIZED (
  SELECT s.id, s.user_id, s.kind,
         (s.net_cents + COALESCE(o.others_cents, 0)
                      - COALESCE(m.mine_cents, 0))::bigint AS counted_net_cents,
         (s.net_cents + COALESCE(o.others_cents, 0))::bigint AS personal_net_cents
    FROM settlement s
    LEFT JOIN others_in_window o ON o.id = s.id
    LEFT JOIN mine_in_window m ON m.id = s.id
)
SELECT booking_id, user_id, 'ko_share'::text AS basis,
       kind AS counted_kind, counted_amount_cents, counted_net_cents,
       kind AS personal_kind, counted_amount_cents AS personal_amount_cents,
       counted_net_cents AS personal_net_cents
  FROM share
UNION ALL
SELECT id, user_id, 'ko_settlement'::text,
       CASE WHEN counted_net_cents > 0 THEN 'expense'
            WHEN counted_net_cents < 0 THEN 'income'
            ELSE kind END,
       abs(counted_net_cents)::bigint,
       counted_net_cents,
       CASE WHEN personal_net_cents > 0 THEN 'expense'
            WHEN personal_net_cents < 0 THEN 'income'
            ELSE kind END,
       abs(personal_net_cents)::bigint,
       personal_net_cents
  FROM settled;
