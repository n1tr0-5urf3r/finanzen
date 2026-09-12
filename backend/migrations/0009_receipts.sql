-- Receipt bytes live on disk under APP_DATA_DIR/receipts/<user_id>/..., never in
-- bytea: a 200 KB - 2 MB PDF per row would triple backup size and make pg_dump
-- unusable as a quick restore path. storage_key embeds the user id, so even a
-- traversal bug in the download handler cannot reach another tenant's directory —
-- and RLS means the handler cannot learn another tenant's key in the first place.
CREATE TABLE receipts (
  id           uuid PRIMARY KEY,
  user_id      uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  booking_id   uuid REFERENCES bookings(id) ON DELETE SET NULL,
  filename     text NOT NULL,
  content_type text NOT NULL,
  byte_size    bigint NOT NULL CHECK (byte_size > 0),
  sha256       text NOT NULL,
  storage_key  text NOT NULL,
  uploaded_at  timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX receipts_dedupe      ON receipts (user_id, sha256);
CREATE INDEX        receipts_booking_idx ON receipts (user_id, booking_id);
SELECT app.enable_tenant_rls('receipts');
