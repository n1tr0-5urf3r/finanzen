-- A household settlement counts in every report NET of the others' shares of my
-- linked purchases, which migration 0022 already took out of the statistics.
--
-- I pay 90,00 for a household of three and link the booking: since 0022 every
-- statistic counts my 30,00 of it. When the flatmates then hand me the other 60,00
-- and the settlement lands as income, that 60,00 is the same money a second time —
-- the year looks 60,00 better than it was. Before 0022 the two errors cancelled
-- (the purchase overstated by 60,00, the income repaying exactly that); 0022 fixed
-- the first and so exposed the second.
--
-- **The rule.** For each settlement, O is the sum, over my confirmed linked
-- bookings in its window, of bank net minus counted net — the others' shares of my
-- linked purchases, signed like `net_cents` (a linked refund contributes
-- negatively). The settlement counts at `bank net + O`. Over a window the
-- statistics then add up to exactly what they did before 0022 — Σbank − O + (S + O)
-- — but the shared purchases sit at my share in their own categories, and the
-- settlement carries only what it really is: my share of what the flatmates paid,
-- minus the others' shares of my UNLINKED purchases.
--
-- **A settlement can change direction.** Income 20,00 against O = 60,00 counts as
-- an expense of 40,00, and that is right: net, the flatmates paid 40,00 for me.
-- `v_ledger` therefore derives the counted `kind` from the sign of the counted net
-- and carries a positive `amount_cents`, so every report that splits income from
-- expense by `kind` sees it on the correct side. A counted net of exactly 0 keeps
-- the booking's own kind at an amount of 0.
--
-- **Which bookings are settlements.** Confirmed income or expense bookings that
-- are not themselves linked to a KitchenOwl expense, and either
--
-- * were booked by `kitchenowl/settle.rs` (`external_source =
--   'kitchenowl_settlement'`), whatever category they ended up in; or
-- * sit in the category `Haushaltsausgleich` — the one `settle.rs` itself resolves
--   by name, case-insensitively, and the one the user's own hand-entered
--   settlements have lived in for years. `categories_user_name_key` makes the
--   name unique per user, so the match is unambiguous. Leaving the hand-entered ones
--   out would let a linked purchase before such a settlement fall into the window
--   of the NEXT app-booked one, months later.
--
-- A transfer is never a settlement (it has no net to correct), nor is a draft: a
-- settlement that has not been confirmed cannot claim a window it does not count in.
--
-- **The window** of a settlement is the linked bookings whose PERIOD lies after the
-- previous settlement's period, up to and including its own; the first settlement's
-- window starts at the beginning of the ledger. Periods rather than `booked_on`,
-- because that is how settlements are dated: `settle.rs` books the balance it reads
-- at the moment of the click under the current period with `booked_on` on the 1st,
-- so every purchase of that month up to the click is part of what it settles but
-- would sort AFTER it by date; and hand-entered settlements from the spreadsheet era
-- are month-only, with no `booked_on` at all. The cost of this granularity: a linked
-- purchase made later in the settlement's own month is attributed to that settlement
-- rather than the next one — the totals over both windows are unaffected. Two
-- settlements in the same period are ordered by `booked_on` (month-only first),
-- `created_at` and id; the first carries the period's window and the others count
-- at their bank amount. Linked bookings after the last settlement belong to no
-- window yet and stay at my share: an open receivable is not income.
--
-- Everything is a view, so linking, unlinking, editing a booking or its period,
-- recategorising a settlement or deleting one recomputes it on the next read.

-- Every booking the statistics count at something other than its bank amount, as
-- booked: a linked purchase at my share (0022) and a settlement net of linked
-- shares (above). The ONE place both rules meet; `v_ledger` and the booking
-- listing's hint read it, so they cannot disagree. The two sets are disjoint — a
-- settlement is by definition not linked to an expense.
--
-- One view rather than a settlement view beside it, so that the share of each
-- linked booking — the expensive part, a lookup per booking — is computed once per
-- statement and feeds both the shares themselves and the settlements' windows. The
-- CTEs are MATERIALIZED for the same reason: none depends on anything outside it,
-- so where a report reads `v_ledger` in a correlated subquery, each is computed
-- once and only rewound on every rescan. The linked shares are summed per period before the window join, so that join
-- is periods × settlements, not bookings.
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
settled AS (
  SELECT s.id, s.user_id, s.kind,
         (s.net_cents + COALESCE(sum(o.others_cents), 0))::bigint AS counted_net_cents
    FROM settlement s
    LEFT JOIN linked o
           ON o.user_id = s.user_id
          AND o.period_ord <= s.period_ord
          AND (s.previous_ord IS NULL OR o.period_ord > s.previous_ord)
   GROUP BY s.id, s.user_id, s.kind, s.net_cents
)
SELECT booking_id, user_id, 'ko_share'::text AS basis,
       kind AS counted_kind, counted_amount_cents, counted_net_cents
  FROM share
UNION ALL
SELECT id, user_id, 'ko_settlement'::text,
       CASE WHEN counted_net_cents > 0 THEN 'expense'
            WHEN counted_net_cents < 0 THEN 'income'
            ELSE kind END,
       abs(counted_net_cents)::bigint,
       counted_net_cents
  FROM settled;

-- Same columns as 0022, in the same order, with `kind` now the COUNTED kind as
-- well (it differs from the booking's only for a settlement that changed
-- direction), and `bank_kind` / `ko_settlement` appended.
DROP VIEW v_ledger;
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
       COALESCE(v.basis = 'ko_settlement', false) AS ko_settlement
  FROM bookings b
  LEFT JOIN categories     c ON c.id = b.category_id
  LEFT JOIN category_types t ON t.id = c.type_id
  LEFT JOIN v_counted_booking v ON v.booking_id = b.id
 WHERE b.status = 'confirmed';
