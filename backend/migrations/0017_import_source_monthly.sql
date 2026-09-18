-- A third workbook shape: one sheet, one block per month, 2014–2023.
--
-- The check exists so a typo in a source name fails at the database rather than
-- becoming a batch nobody can find again, which means adding a reader means
-- adding its name here.
ALTER TABLE import_batches DROP CONSTRAINT import_batches_source_check;
ALTER TABLE import_batches ADD CONSTRAINT import_batches_source_check
  CHECK (source IN ('xlsx_2026','xlsx_monthly','ods_legacy','csv','kitchenowl'));
