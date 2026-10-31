-- A booking linked to a KitchenOwl expense counts in every report at MY SHARE of
-- that expense, not at what left the account.
--
-- I pay 90,00 at the supermarket for a household of three and book 90,00, because
-- that is what the bank says. Linked to the KitchenOwl expense, it is visibly a
-- shared purchase of which 30,00 is mine — the other 60,00 is owed back to me, not
-- food I ate. Until now every statistic counted the 90,00 and the link changed
-- nothing; from here on every statistic counts 30,00.
--
-- What does NOT change is anything describing money in an account. The full 90,00
-- did leave it, so `v_account_legs`, the booking itself and every listing of it
-- keep the bank amount. `v_ledger` carries that amount along as
-- `bank_amount_cents` / `bank_net_cents` for the few report figures that are
-- balances rather than flows: the opening/closing chain between years.

-- One row per booking the share rule applies to, and the ONE definition of the
-- amount it counts at. `v_ledger` and the booking listing's hint both read it, so
-- there is nothing to keep in step.
--
-- **The link.** `bookings.external_source = 'kitchenowl'` with the expense id in
-- `external_id` — the authoritative column pair, the one carrying the unique index
-- (see `kitchenowl/link.rs`). It covers both ways a link comes about: confirming a
-- pulled expense, and a push from this app, which attaches the same pair once
-- KitchenOwl has answered with an id. `ko_expenses.linked_booking_id` is only a
-- denormalised copy and is deliberately not consulted. A settlement booking
-- (`kitchenowl_settlement`) references a balance, not an expense, and is not linked.
--
-- **Where the share comes from**, first match wins:
--
-- 1. The mirrored expense — what KitchenOwl currently says, including edits made
--    there afterwards. Archived rows and `exclude_from_statistics` rows count too:
--    the link still says this purchase was shared, and excluding an expense from
--    KitchenOwl's own charts is a KitchenOwl display choice, not a statement that
--    the user bore all of it.
-- 2. A pushed booking that has not been mirrored yet (the window between the push
--    and the next pull): the push payload. It is exactly what was sent — the
--    amount and the weights — and its `shareCents` came from the same
--    largest-remainder allocation the mirror runs, so it agrees with the row the
--    pull will write. My member is the `is_me` row; without one the payload says
--    nothing about me and the source is skipped. Being absent from `paidFor` is a
--    share of 0, which is correct: I paid for the others only.
-- 3. Neither: no row here, and the booking counts at its full amount. A push still
--    queued is not a link yet — it may never land — so it counts in full until it
--    does.
--
-- **The rule.** `counted = round(booking × own / total)`, half away from zero
-- (Postgres `round(numeric)`), clamped to [0, booking]. Proportional, not the
-- absolute own share, because the BOOKING is what moved money: a 90,00 booking
-- linked to a KitchenOwl entry of 89,99 (a tip left off, a rounding difference)
-- should count a third of 90,00, not 29,99 exactly, and a booking for half of a
-- larger expense must not suddenly count the whole of my share of it. Signs are
-- taken as magnitudes — a KitchenOwl refund carries a negative amount and a
-- negative share, and the fraction is the same; the direction stays with the
-- booking's `kind`. A KitchenOwl amount of 0 gives no fraction at all, so the
-- booking counts in full rather than as a division by zero or an invented 0.
CREATE VIEW v_ko_booking_share AS
SELECT b.id AS booking_id, b.user_id,
       src.own_cents, src.total_cents, src.source,
       CASE WHEN src.total_cents = 0 THEN b.amount_cents
            ELSE GREATEST(0, LEAST(b.amount_cents,
                   round(b.amount_cents::numeric * abs(src.own_cents)
                         / abs(src.total_cents))::bigint))
       END AS counted_amount_cents
  FROM bookings b
  JOIN LATERAL (
    SELECT x.own_cents, x.total_cents, x.source
      FROM (
        SELECT e.own_share_cents AS own_cents, e.amount_cents AS total_cents,
               'mirror'::text AS source, 1 AS pref
          FROM ko_expenses e
         WHERE e.user_id = b.user_id
           -- CASE, not a bare cast: the planner may evaluate this before the
           -- external_source test, and a non-numeric id must not raise.
           AND e.external_id = CASE WHEN b.external_id ~ '^[0-9]{1,18}$'
                                    THEN b.external_id::bigint END
        UNION ALL
        SELECT COALESCE((SELECT sum((p->>'shareCents')::bigint)
                           FROM jsonb_array_elements(i.payload->'paidFor') p
                          WHERE (p->>'memberId')::bigint = me.member_id), 0),
               (i.payload->>'amountCents')::bigint,
               'push'::text, 2
          FROM ko_push_intents i
          JOIN ko_members me ON me.user_id = i.user_id AND me.is_me
         WHERE i.booking_id = b.id AND i.state = 'pushed'
           AND i.external_id::text = b.external_id
      ) x
     ORDER BY x.pref
     LIMIT 1
  ) src ON true
 WHERE b.external_source = 'kitchenowl';

-- Same columns as before, in the same order, with `amount_cents` / `net_cents` now
-- the COUNTED amount, and the bank figures appended. Every report already reads
-- these two names, so pointing them at the counted amount is what makes the rule
-- hold everywhere at once instead of at every call site that remembers it.
-- `net_cents` restates the generated column's netting rule from migration 0007 on
-- the counted amount: expense positive, income negative, transfer 0.
DROP VIEW v_ledger;
CREATE VIEW v_ledger AS
SELECT b.id, b.user_id,
       b.period_year, b.period_month, b.period_ord, b.booked_on,
       b.kind,
       COALESCE(s.counted_amount_cents, b.amount_cents) AS amount_cents,
       CASE b.kind WHEN 'expense' THEN  COALESCE(s.counted_amount_cents, b.amount_cents)
                   WHEN 'income'  THEN -COALESCE(s.counted_amount_cents, b.amount_cents)
                   ELSE 0 END::bigint AS net_cents,
       b.comment, b.match_key, b.tax_relevant, b.tax_seq,
       b.category_id, b.category_source, c.name AS category_name,
       t.id AS type_id, t.code AS type_code, t.label AS type_label,
       t.is_income, t.is_savings, t.in_consumption,
       b.account_id, b.counter_account_id, b.external_source, b.external_id,
       b.created_at,
       b.amount_cents AS bank_amount_cents,
       b.net_cents    AS bank_net_cents,
       s.booking_id IS NOT NULL AS ko_shared
  FROM bookings b
  LEFT JOIN categories     c ON c.id = b.category_id
  LEFT JOIN category_types t ON t.id = c.type_id
  LEFT JOIN v_ko_booking_share s ON s.booking_id = b.id
 WHERE b.status = 'confirmed';
