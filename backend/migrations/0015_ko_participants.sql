-- Who takes part in KitchenOwl sync, readable WITHOUT a tenant context.
--
-- `ko_sync_state` already holds this fact, and it is the right place for it — but
-- it carries forced row-level security, and the background sync loop runs with no
-- request, no session and therefore no `app.user_id`. Under RLS that read returns
-- zero rows rather than failing, so the loop iterated an empty list of users and
-- did nothing, every time, silently. Manual "jetzt synchronisieren" worked, because
-- a request has a tenant. That is the bug this table fixes.
--
-- It is system scope on purpose and holds nothing but a user id — the same class of
-- data as `sessions` and `identities`, which are also read before a tenant exists.
-- No amount, no household, no name. The financial mirror stays behind RLS.
CREATE TABLE ko_participants (
  user_id    uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  enabled_at timestamptz NOT NULL DEFAULT now()
);

-- Backfill from the existing opt-ins. The owner is subject to FORCE row-level
-- security like everyone else, so the read has to be let out of it for the length
-- of this statement — which is precisely the mechanism that broke the loop.
ALTER TABLE ko_sync_state NO FORCE ROW LEVEL SECURITY;
INSERT INTO ko_participants (user_id)
SELECT user_id FROM ko_sync_state
ON CONFLICT (user_id) DO NOTHING;
ALTER TABLE ko_sync_state FORCE ROW LEVEL SECURITY;
