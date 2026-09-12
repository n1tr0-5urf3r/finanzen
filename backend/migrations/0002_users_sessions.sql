-- System-scope identity tables.
--
-- These deliberately carry NO row-level security. They are read *before* a tenant
-- context exists: resolving a session cookie is what establishes app.user_id in the
-- first place. A policy here would be a chicken-and-egg deadlock. They are reachable
-- only through Db::system().

CREATE TABLE users (
  id            uuid PRIMARY KEY,
  username      text NOT NULL,
  display_name  text NOT NULL,
  email         text,
  password_hash text,                      -- NULL for identity-provider-only users
  is_admin      boolean NOT NULL DEFAULT false,
  disabled_at   timestamptz,
  created_at    timestamptz NOT NULL DEFAULT now(),
  updated_at    timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX users_username_key ON users (lower(username));
CREATE UNIQUE INDEX users_email_key    ON users (lower(email)) WHERE email IS NOT NULL;

-- One row per (provider, subject). Local accounts get provider='local' and the user
-- uuid as subject; OIDC accounts get 'oidc:<issuer-hash>' and the `sub` claim. Adding
-- OIDC later therefore needs no schema change.
CREATE TABLE identities (
  id         uuid PRIMARY KEY,
  user_id    uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  provider   text NOT NULL,
  subject    text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (provider, subject)
);
CREATE INDEX identities_user_idx ON identities (user_id);

CREATE TABLE sessions (
  token_hash   text PRIMARY KEY,           -- sha256(session_secret || ':' || token)
  user_id      uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  issued_via   text NOT NULL,
  expires_at   timestamptz NOT NULL,
  last_seen_at timestamptz NOT NULL DEFAULT now(),
  user_agent   text,
  created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sessions_user_idx   ON sessions (user_id);
CREATE INDEX sessions_expiry_idx ON sessions (expires_at);
