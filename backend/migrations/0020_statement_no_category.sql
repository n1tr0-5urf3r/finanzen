-- "No category" as a decision of its own.
--
-- A statement line shows COALESCE(what the user chose, what was suggested), so a
-- user who picked "no category" had no way to say it: an empty choice fell back
-- to the suggestion, and the wrong guess was booked anyway. This is that choice,
-- recorded rather than inferred from an absence.
ALTER TABLE import_rows ADD COLUMN no_category boolean NOT NULL DEFAULT false;
