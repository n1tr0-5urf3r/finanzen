CREATE TABLE user_settings (
  user_id    uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  data       jsonb NOT NULL DEFAULT '{}'::jsonb,
  updated_at timestamptz NOT NULL DEFAULT now()
);
SELECT app.enable_tenant_rls('user_settings');

-- Bookings are hard-deleted: a soft-deleted row that leaks into one aggregate and not
-- another is worse than no row. The `before` snapshot here is what makes that undoable.
CREATE TABLE audit_log (
  id        bigserial PRIMARY KEY,
  user_id   uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  entity    text NOT NULL,
  entity_id uuid,
  action    text NOT NULL,
  before    jsonb,
  after     jsonb,
  at        timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX audit_log_entity_idx ON audit_log (user_id, entity, entity_id, at DESC);
SELECT app.enable_tenant_rls('audit_log');
