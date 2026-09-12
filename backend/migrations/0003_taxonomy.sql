-- Category types carry behavioural flags rather than the calculation engine matching
-- on German string literals. This is what lets a user rename "Variable Kosten"
-- without breaking the savings-rate formula.
CREATE TABLE category_types (
  id             uuid PRIMARY KEY,
  user_id        uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  code           text NOT NULL,
  label          text NOT NULL,
  sort_order     smallint NOT NULL,
  is_income      boolean NOT NULL DEFAULT false,
  is_savings     boolean NOT NULL DEFAULT false,
  in_consumption boolean NOT NULL DEFAULT true,
  UNIQUE (user_id, code)
);
SELECT app.enable_tenant_rls('category_types');

CREATE TABLE categories (
  id         uuid PRIMARY KEY,
  user_id    uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  -- NOT NULL + RESTRICT is what makes the spreadsheet's IFERROR(...,"Sonstiges")
  -- fallback unrepresentable: a category without a type cannot exist, so Sport can
  -- never silently become Sonstiges.
  type_id    uuid NOT NULL REFERENCES category_types(id) ON DELETE RESTRICT,
  name       text NOT NULL,
  sort_order smallint NOT NULL DEFAULT 0,
  archived   boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX categories_user_name_key ON categories (user_id, lower(name));
CREATE INDEX categories_type_idx ON categories (user_id, type_id);
SELECT app.enable_tenant_rls('categories');

-- Seeds the five types and the 32 categories for a new user. Living in SQL rather
-- than Rust means the golden test seeds a user with one call and gets a taxonomy
-- byte-identical to production.
CREATE OR REPLACE FUNCTION app.seed_default_taxonomy(target uuid) RETURNS void
LANGUAGE plpgsql AS $$
DECLARE
  t_einkommen uuid := gen_random_uuid();
  t_fix       uuid := gen_random_uuid();
  t_sparen    uuid := gen_random_uuid();
  t_variabel  uuid := gen_random_uuid();
  t_sonstiges uuid := gen_random_uuid();
BEGIN
  INSERT INTO category_types (id, user_id, code, label, sort_order, is_income, is_savings, in_consumption) VALUES
    (t_einkommen, target, 'einkommen', 'Einkommen',       1, true,  false, false),
    (t_fix,       target, 'fixkosten', 'Fixkosten',       2, false, false, true ),
    (t_variabel,  target, 'variabel',  'Variable Kosten', 3, false, false, true ),
    (t_sparen,    target, 'sparen',    'Sparen',          4, false, true,  false),
    (t_sonstiges, target, 'sonstiges', 'Sonstiges',       5, false, false, true );

  INSERT INTO categories (id, user_id, type_id, name, sort_order)
  SELECT gen_random_uuid(), target, tid, cname, ord FROM (VALUES
    (t_einkommen, 'Gehalt',                 1),
    (t_einkommen, 'Freelancing',            2),
    (t_einkommen, 'Sonstige Einnahmen',     3),
    (t_fix,       'Miete',                  4),
    (t_fix,       'Nebenkosten',            5),
    (t_fix,       'Strom',                  6),
    (t_fix,       'Internet & Telefon',     7),
    (t_fix,       'Rundfunkbeitrag',        8),
    (t_fix,       'Versicherungen',         9),
    (t_fix,       'Haustier',              10),
    (t_fix,       'Abos & Streaming',      11),
    (t_fix,       'Server & Domains',      12),
    (t_fix,       'Bank & Gebühren',       13),
    (t_fix,       'Uni & Bildung',         14),
    (t_fix,       'Sport',                 15),
    (t_sparen,    'Sparen & Anlage',       16),
    (t_variabel,  'Lebensmittel',          17),
    (t_variabel,  'Essen auswärts',        18),
    (t_variabel,  'Mensa',                 19),
    (t_variabel,  'Auto & Parken',         20),
    (t_variabel,  'Bahn & ÖPNV',           21),
    (t_variabel,  'Drogerie & Gesundheit', 22),
    (t_variabel,  'Haus & Garten',         23),
    (t_variabel,  'Kleidung & Merch',      24),
    (t_variabel,  'Anschaffungen',         25),
    (t_variabel,  'Games & Software',      26),
    (t_variabel,  'Freizeit & Events',     27),
    (t_variabel,  'Reisen & Urlaub',       28),
    (t_variabel,  'Dienstreisen',          29),
    (t_variabel,  'Geschenke',             30),
    (t_sonstiges, 'Bargeld',               31),
    (t_sonstiges, 'Sonstiges',             32)
  ) AS seed(tid, cname, ord);
END $$;
