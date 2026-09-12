-- Recurring templates. interval_months + anchor_ord covers monthly (1), quarterly (3)
-- and annual (12); Versicherung and Rundfunkbeitrag are quarterly-shaped in the real
-- data. A template is due in period p iff
--   active AND p >= active_from_ord AND (active_to_ord IS NULL OR p <= active_to_ord)
--   AND (p - anchor_ord) % interval_months = 0
--
-- amount_is_estimate matters concretely: Mafit is 29,00 in January, 31,50 from March
-- and 34,50 in February and August. Materialising such a template must produce a
-- DRAFT the user confirms, never a confirmed booking.
CREATE TABLE recurring_templates (
  id                 uuid PRIMARY KEY,
  user_id            uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name               text NOT NULL,
  comment            text NOT NULL CHECK (btrim(comment) <> ''),
  kind               text NOT NULL CHECK (kind IN ('income','expense','transfer')),
  amount_cents       bigint NOT NULL CHECK (amount_cents > 0),
  amount_is_estimate boolean NOT NULL DEFAULT false,
  category_id        uuid REFERENCES categories(id) ON DELETE SET NULL,
  account_id         uuid REFERENCES accounts(id) ON DELETE SET NULL,
  counter_account_id uuid REFERENCES accounts(id) ON DELETE SET NULL,
  tax_relevant       boolean NOT NULL DEFAULT false,
  day_of_month       smallint CHECK (day_of_month BETWEEN 1 AND 31),
  interval_months    smallint NOT NULL DEFAULT 1 CHECK (interval_months BETWEEN 1 AND 12),
  anchor_ord         integer NOT NULL,
  active_from_ord    integer NOT NULL,
  active_to_ord      integer,
  active             boolean NOT NULL DEFAULT true,
  sort_order         smallint NOT NULL DEFAULT 0,
  created_at         timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT tmpl_window CHECK (active_to_ord IS NULL OR active_to_ord >= active_from_ord)
);
CREATE INDEX recurring_active_idx ON recurring_templates (user_id, active_from_ord) WHERE active;
SELECT app.enable_tenant_rls('recurring_templates');

ALTER TABLE bookings ADD COLUMN template_id uuid
  REFERENCES recurring_templates(id) ON DELETE SET NULL;

-- Idempotent materialisation: one booking per template per period, ever. Running
-- "materialise October" twice creates nothing the second time.
CREATE UNIQUE INDEX bookings_template_period_key
  ON bookings (user_id, template_id, period_ord) WHERE template_id IS NOT NULL;
