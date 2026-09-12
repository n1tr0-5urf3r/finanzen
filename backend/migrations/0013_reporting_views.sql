-- The ONLY thing reports read. Filtering status here means drafts — KitchenOwl pulls,
-- materialised recurring items — can never leak into a total, in any query, ever.
-- Views are not SECURITY DEFINER, so they inherit RLS from the base tables for free.
CREATE VIEW v_ledger AS
SELECT b.id, b.user_id,
       b.period_year, b.period_month, b.period_ord, b.booked_on,
       b.kind, b.amount_cents, b.net_cents,
       b.comment, b.match_key, b.tax_relevant, b.tax_seq,
       b.category_id, b.category_source, c.name AS category_name,
       t.id AS type_id, t.code AS type_code, t.label AS type_label,
       t.is_income, t.is_savings, t.in_consumption,
       b.account_id, b.counter_account_id, b.external_source, b.external_id,
       b.created_at
  FROM bookings b
  LEFT JOIN categories     c ON c.id = b.category_id
  LEFT JOIN category_types t ON t.id = c.type_id
 WHERE b.status = 'confirmed';

-- Transfers as two signed legs, for per-account balances only. Never used for
-- consumption or category analysis.
CREATE VIEW v_account_legs AS
  SELECT user_id, account_id, period_ord, -net_cents AS delta_cents
    FROM bookings
   WHERE status = 'confirmed' AND kind IN ('income','expense') AND account_id IS NOT NULL
  UNION ALL
  SELECT user_id, account_id, period_ord, -amount_cents
    FROM bookings
   WHERE status = 'confirmed' AND kind = 'transfer' AND account_id IS NOT NULL
  UNION ALL
  SELECT user_id, counter_account_id, period_ord, amount_cents
    FROM bookings
   WHERE status = 'confirmed' AND kind = 'transfer' AND counter_account_id IS NOT NULL;
