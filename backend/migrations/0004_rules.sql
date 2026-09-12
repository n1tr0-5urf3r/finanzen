-- Comment -> category rules.
--
-- match_key is GENERATED, so case-insensitive exact matching is enforced by the
-- schema rather than by remembering to lower() at every call site. The real data
-- needs it: essen/Essen, parken/Parken, spotify/Spotify, paypal/PayPal and
-- apotheke/Apotheke all occur in both casings.
--
-- kind_override is how 'to ING' and 'from Volksbank' become transfers: an ordinary
-- rule the user can edit, not a hard-coded list in the binary.
CREATE TABLE category_rules (
  id          uuid PRIMARY KEY,
  user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  pattern     text NOT NULL CHECK (btrim(pattern) <> ''),
  match_key   text GENERATED ALWAYS AS (lower(btrim(pattern))) STORED,
  category_id uuid REFERENCES categories(id) ON DELETE CASCADE,
  kind_override text CHECK (kind_override IN ('income','expense','transfer')),
  source      text NOT NULL DEFAULT 'user' CHECK (source IN ('seed','user','review')),
  note        text,
  created_at  timestamptz NOT NULL DEFAULT now(),
  updated_at  timestamptz NOT NULL DEFAULT now(),
  -- A rule must do something: assign a category, force a kind, or both.
  CONSTRAINT rule_has_effect CHECK (category_id IS NOT NULL OR kind_override IS NOT NULL)
);
CREATE UNIQUE INDEX category_rules_key ON category_rules (user_id, match_key);
CREATE INDEX category_rules_category_idx ON category_rules (user_id, category_id);
SELECT app.enable_tenant_rls('category_rules');
