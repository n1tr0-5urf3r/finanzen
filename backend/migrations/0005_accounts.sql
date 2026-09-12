CREATE TABLE accounts (
  id            uuid PRIMARY KEY,
  user_id       uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name          text NOT NULL,
  kind          text NOT NULL DEFAULT 'checking'
                  CHECK (kind IN ('checking','savings','cash','broker','other')),
  opening_cents bigint NOT NULL DEFAULT 0,
  archived      boolean NOT NULL DEFAULT false,
  sort_order    smallint NOT NULL DEFAULT 0,
  created_at    timestamptz NOT NULL DEFAULT now(),
  UNIQUE (user_id, name)
);
SELECT app.enable_tenant_rls('accounts');
