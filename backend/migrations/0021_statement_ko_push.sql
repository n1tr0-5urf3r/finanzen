-- A statement line can be sent to KitchenOwl as well as booked.
--
-- The booking does not exist until the import is committed, so the push dialogue's
-- choices wait on the line itself: name, amount, date, KitchenOwl category, who
-- paid and the split, exactly as `POST /bookings/{id}/kitchenowl` takes them. The
-- commit books the line and queues the push in the same transaction, so a line
-- that becomes a booking becomes a push intent with it or not at all.
ALTER TABLE import_rows ADD COLUMN ko_push jsonb;
