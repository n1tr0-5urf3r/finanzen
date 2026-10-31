-- What the flatmates paid for me counts in my statistics, at my share.
--
-- Since 0022 a purchase *I* paid and linked counts at my share, and since 0023 the
-- settlement that repays the rest counts net of that. What was still missing is the
-- other half of the household: the flatmate pays 90,00 for the three of us, 30,00
-- of it is my food, and none of it appeared anywhere in my statistics — until the
-- settlement, months later, as one lump in `Haushaltsausgleich`. Food, drugstore
-- and cleaning bought by somebody else were invisible as food, drugstore and
-- cleaning.
--
-- From here on each such expense counts as its own row, at my share, in the finance
-- category its KitchenOwl category is mapped to, and in the period of its date.
--
-- **Which expenses.** One row per mirrored expense that
--
-- * was paid by another member — `paid_by_id` is a member and not my `is_me` one.
--   What I paid myself is never added: my bank booking is the record of it, linked
--   or not. Without an `is_me` member nothing is known about "me" and nothing is
--   added; an expense whose payer is unknown is not known to be somebody else's;
-- * has a share of mine other than 0 — I was not part of a purchase I have no
--   share of;
-- * is live: an archived row was deleted in KitchenOwl (migration 0014);
-- * is not `exclude_from_statistics`. KitchenOwl sets that on what is not
--   consumption — the paybacks between members, the "I gave you 50,00" entries —
--   and its own statistics skip them, as did this app's household analysis. A
--   payback is a settlement, and the settlements are already in the ledger as
--   bookings;
-- * is not linked to a confirmed booking of mine. A link says the bank booking IS
--   this purchase, and that booking already counts at my share (0022); counting the
--   expense as well would count it twice. Normally only my own purchases are linked,
--   but nothing forbids linking another one, so the rule does not rely on it.
--
-- A refund in KitchenOwl carries a negative amount and a negative share. It counts
-- as income of the absolute share, the direction the sign says — the same reading
-- 0022 gives a linked refund.
--
-- **The category** is `ko_categories.finances_category_id`, which the user sets per
-- KitchenOwl category on the KitchenOwl page. Until 0024 it was only a hint for the
-- tagging queue; it now decides where a share lands. Unmapped — or no KitchenOwl
-- category at all — counts without a category, so the totals are right and the gap
-- is visible in every "ohne Kategorie" figure instead of being silently left out.
--
-- **Settlements.** With these rows in, a settlement paying the flatmate for my share
-- of their purchases would count that share a second time. So 0023's rule extends
-- by one term: within the settlement's window, M is the sum of these shares, and
-- the settlement counts at `bank net + O − M`. Over a window the statistics then
-- add up to my linked shares, my shares of the flatmates' purchases and my unlinked
-- purchases at full — and the settlement shrinks to the correction for the unlinked
-- ones, the only part it still really is. As in 0023 the direction can flip.
--
-- **Optional, but on by default.** The analysis page can switch the household
-- shares off, and the tax report never has them (somebody else's receipt is not a
-- deduction of mine). Both read `v_ledger_personal`, which is `v_ledger` without
-- the household rows and with the settlements at their 0023 figure, so "off" shows
-- exactly what the app showed before this migration. Everything else reads
-- `v_ledger` and gets the default.
--
-- **Balances do not move.** These rows carry a bank amount of 0 and no account: not
-- a cent of them passed through my account, so the opening/closing chain, the
-- closing balance and the forecast's running balance — all built from
-- `bank_net_cents` — are exactly what they were.

-- The ONE definition of which household expenses count for me, and at what. The
-- ledger reads it for the rows themselves, the settlement netting for M, and the
-- analysis drilldown to list them; none of them restates the conditions above.
CREATE VIEW v_ko_household_share AS
SELECT e.id, e.user_id, e.external_id, e.name, e.expense_date,
       EXTRACT(YEAR  FROM e.expense_date)::smallint AS period_year,
       EXTRACT(MONTH FROM e.expense_date)::smallint AS period_month,
       (EXTRACT(YEAR FROM e.expense_date)::int * 12
        + EXTRACT(MONTH FROM e.expense_date)::int - 1) AS period_ord,
       e.amount_cents, e.own_share_cents,
       e.paid_by_id, payer.name AS paid_by_name,
       e.ko_category_id, e.ko_category_name,
       k.finances_category_id AS category_id,
       e.updated_at
  FROM ko_expenses e
  JOIN ko_members me ON me.user_id = e.user_id AND me.is_me
  LEFT JOIN ko_members payer ON payer.user_id = e.user_id
                            AND payer.member_id = e.paid_by_id
  LEFT JOIN ko_categories k ON k.user_id = e.user_id
                           AND k.category_id = e.ko_category_id
 WHERE e.archived_at IS NULL
   AND NOT e.exclude_from_statistics
   AND e.paid_by_id <> me.member_id
   AND e.own_share_cents <> 0
   AND NOT EXISTS (
         SELECT 1 FROM bookings b
          WHERE b.user_id = e.user_id
            AND b.external_source = 'kitchenowl'
            AND b.external_id = e.external_id::text
            AND b.status = 'confirmed');

-- 0023's view with M added. Each settlement now carries two figures: `counted_*`
-- with the household shares netted (the default every statistic uses), and
-- `personal_*` without them — 0023's figure exactly, for the analysis with the
-- household switched off and for the tax report. A linked share is the same in
-- both. One view rather than two, so the share of each linked booking and the
-- windows are still computed once per statement.
DROP VIEW v_ledger;
DROP VIEW v_counted_booking;
CREATE VIEW v_counted_booking AS
WITH share AS MATERIALIZED (
  SELECT sh.booking_id, sh.user_id, b.kind, b.status, b.period_ord, b.net_cents,
         sh.counted_amount_cents,
         (CASE b.kind WHEN 'expense' THEN  sh.counted_amount_cents
                      WHEN 'income'  THEN -sh.counted_amount_cents
                      ELSE 0 END)::bigint AS counted_net_cents
    FROM v_ko_booking_share sh
    JOIN bookings b ON b.id = sh.booking_id
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
     AND b.external_source IS DISTINCT FROM 'kitchenowl'
     AND (b.external_source = 'kitchenowl_settlement'
          OR lower(c.name) = lower('Haushaltsausgleich'))
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
-- The window sums, one grouped join each — joining both at once would multiply
-- the rows. A join rather than a correlated subquery per settlement, because the
-- counted figure is referenced several times below and each reference would get
-- its own copy of the subplan.
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

-- Every confirmed booking as 0023 had it, followed by one row per household share.
--
-- `kind` / `amount_cents` / `net_cents` are the default counted figures — household
-- shares included, settlements net of them — so every report that reads `v_ledger`
-- without knowing about the switch gets the default. `personal_*` are the same
-- figures with the household left out; `v_ledger_personal` presents them under the
-- usual names.
--
-- A household row: its id is the mirrored expense's own uuid, which is stable
-- across syncs (nothing joins it to `bookings`); its comment is the expense's
-- name, so a series by comment
-- finds "Wocheneinkauf" whoever paid for it. No account, never tax-relevant, and a
-- bank amount of 0, because no money of mine moved. `ko_household` marks it for
-- the few places that must tell it from a booking: listing what to edit, counting
-- what is uncategorised in the booking list.
CREATE VIEW v_ledger AS
SELECT b.id, b.user_id,
       b.period_year, b.period_month, b.period_ord, b.booked_on,
       COALESCE(v.counted_kind, b.kind) AS kind,
       COALESCE(v.counted_amount_cents, b.amount_cents) AS amount_cents,
       COALESCE(v.counted_net_cents, b.net_cents) AS net_cents,
       b.comment, b.match_key, b.tax_relevant, b.tax_seq,
       b.category_id, b.category_source, c.name AS category_name,
       t.id AS type_id, t.code AS type_code, t.label AS type_label,
       t.is_income, t.is_savings, t.in_consumption,
       b.account_id, b.counter_account_id, b.external_source, b.external_id,
       b.created_at,
       b.amount_cents AS bank_amount_cents,
       b.net_cents    AS bank_net_cents,
       COALESCE(v.basis = 'ko_share', false) AS ko_shared,
       b.kind AS bank_kind,
       COALESCE(v.basis = 'ko_settlement', false) AS ko_settlement,
       false AS ko_household,
       COALESCE(v.personal_kind, b.kind) AS personal_kind,
       COALESCE(v.personal_amount_cents, b.amount_cents) AS personal_amount_cents,
       COALESCE(v.personal_net_cents, b.net_cents) AS personal_net_cents
  FROM bookings b
  LEFT JOIN categories     c ON c.id = b.category_id
  LEFT JOIN category_types t ON t.id = c.type_id
  LEFT JOIN v_counted_booking v ON v.booking_id = b.id
 WHERE b.status = 'confirmed'
UNION ALL
SELECT h.id, h.user_id,
       h.period_year, h.period_month, h.period_ord, h.expense_date,
       CASE WHEN h.own_share_cents > 0 THEN 'expense' ELSE 'income' END,
       abs(h.own_share_cents)::bigint,
       h.own_share_cents::bigint,
       h.name, lower(btrim(h.name)), false, NULL::integer,
       c.id, CASE WHEN c.id IS NULL THEN 'unresolved' ELSE 'kitchenowl' END, c.name,
       t.id, t.code, t.label,
       t.is_income, t.is_savings, t.in_consumption,
       NULL::uuid, NULL::uuid, 'kitchenowl_household', h.external_id::text,
       h.updated_at,
       0::bigint, 0::bigint,
       false,
       CASE WHEN h.own_share_cents > 0 THEN 'expense' ELSE 'income' END,
       false,
       true,
       CASE WHEN h.own_share_cents > 0 THEN 'expense' ELSE 'income' END,
       0::bigint, 0::bigint
  FROM v_ko_household_share h
  LEFT JOIN categories     c ON c.id = h.category_id
  LEFT JOIN category_types t ON t.id = c.type_id;

-- The ledger with the household switched off: bookings only, settlements at their
-- 0023 figure, under the column names every report already reads. Derived from
-- `v_ledger` rather than written out again, so the two can only differ by the
-- switch. The `NOT ko_household` qual is constant-false on the household branch,
-- so the planner drops that branch instead of filtering it.
CREATE VIEW v_ledger_personal AS
SELECT id, user_id,
       period_year, period_month, period_ord, booked_on,
       personal_kind AS kind,
       personal_amount_cents AS amount_cents,
       personal_net_cents AS net_cents,
       comment, match_key, tax_relevant, tax_seq,
       category_id, category_source, category_name,
       type_id, type_code, type_label,
       is_income, is_savings, in_consumption,
       account_id, counter_account_id, external_source, external_id,
       created_at,
       bank_amount_cents, bank_net_cents,
       ko_shared, bank_kind, ko_settlement, ko_household
  FROM v_ledger
 WHERE NOT ko_household;

-- The map now decides something, so say so where the schema is read.
COMMENT ON COLUMN ko_categories.finances_category_id IS
  'The finance category my shares of the flatmates'' purchases in this KitchenOwl '
  'category count in (migration 0024). Set by the user, never by a sync; NULL '
  'counts them without a category.';
