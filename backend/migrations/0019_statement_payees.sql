-- What a payee is called in your own words.
--
-- A statement names a shop the way a card terminal does: `Studierendenwerk
-- Musterstadt-Beispielheim Anstalt des offentlichen Rechts`. Renaming that to
-- `Mensaguthaben` during a review is knowledge about the payee, and it was being
-- thrown away after every import — the next statement asked the same question
-- again. This table is that answer, keyed on the payee the bank sends.
--
-- Deliberately separate from `category_rules`, which maps a COMMENT to a
-- category. This maps a PAYEE to a comment, and the two chain: the payee becomes
-- `Mensaguthaben`, and the rule table turns `Mensaguthaben` into Mensa.
CREATE TABLE statement_payees (
  id          uuid PRIMARY KEY,
  user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  payee       text NOT NULL CHECK (btrim(payee) <> ''),
  payee_key   text GENERATED ALWAYS AS (lower(btrim(payee))) STORED,
  comment     text NOT NULL CHECK (btrim(comment) <> ''),
  -- The category that was on the line when it was renamed, if any. A hint for the
  -- next import, never a decision: the rule table still gets the final word.
  category_id uuid REFERENCES categories(id) ON DELETE SET NULL,
  hits        integer NOT NULL DEFAULT 0,
  created_at  timestamptz NOT NULL DEFAULT now(),
  updated_at  timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX statement_payees_key ON statement_payees (user_id, payee_key);

-- Off by default, and per line, because a payee is not always worth remembering:
-- a payment provider is always its own legal entity and a different purchase
-- every time. Remembering that one would rename every future PayPal line to
-- whatever the last one happened to be.
ALTER TABLE import_rows ADD COLUMN remember_payee boolean NOT NULL DEFAULT false;

SELECT app.enable_tenant_rls('statement_payees');
