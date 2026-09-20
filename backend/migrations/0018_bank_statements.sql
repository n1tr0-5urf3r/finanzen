-- Bank statements: a booking can now carry who it was with and what the bank
-- wrote on it, and an import row can carry a date, those two fields, and a
-- pointer at the booking it looks like a duplicate of.
--
-- Both booking columns are nullable and both stay empty for everything that came
-- from a spreadsheet: they are what a bank export knows and a hand-kept sheet
-- never did.
ALTER TABLE bookings
  ADD COLUMN counterparty text,
  ADD COLUMN purpose      text;

ALTER TABLE bookings DROP CONSTRAINT bookings_origin_check;
ALTER TABLE bookings ADD CONSTRAINT bookings_origin_check
  CHECK (origin IN ('manual','legacy_month_only','sheet_2026','recurring','kitchenowl','bank_csv'));

ALTER TABLE import_rows
  ADD COLUMN booked_on    date,
  ADD COLUMN counterparty text,
  ADD COLUMN purpose      text,
  -- The booking this row probably already is. Set at staging time and shown in
  -- the review, never acted on by itself: "looks like a duplicate" is a question
  -- for a person, and a bank genuinely does charge 3,90 € at the same shop twice.
  ADD COLUMN duplicate_booking_id uuid REFERENCES bookings(id) ON DELETE SET NULL;

CREATE INDEX import_rows_duplicate_idx ON import_rows (user_id, batch_id)
  WHERE duplicate_booking_id IS NOT NULL;

ALTER TABLE import_batches DROP CONSTRAINT import_batches_source_check;
ALTER TABLE import_batches ADD CONSTRAINT import_batches_source_check
  CHECK (source IN ('xlsx_2026','xlsx_monthly','ods_legacy','csv','csv_ing','kitchenowl'));

-- A statement line is unique in the account it came from: same day, same amount,
-- same counterparty and the bank's own running balance after it. Re-importing an
-- overlapping export therefore stages the same rows and the fingerprint stops
-- them at the commit, exactly as it does for the workbooks.
