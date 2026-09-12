-- Expenses disappear from KitchenOwl: the live household has exactly one id gap in
-- 464 rows, an expense deleted after it was entered. Migration 0011 gave sync_runs
-- an `archived_count` but gave ko_expenses nothing to archive, so a deleted expense
-- would have stayed in the mirror forever and the counter could never move.
--
-- Archived rather than deleted, because the row may carry the user's own decision:
-- `linked_booking_id` is a link they confirmed by hand. Dropping the row would
-- silently discard that, and a mirror is not entitled to delete a user's work just
-- because the other end changed its mind.
--
-- Only a COMPLETE scan may archive — one that reached the last page rather than
-- stopping at KITCHENOWL_MAX_PULL_PAGES. A truncated scan has not seen the rows it
-- would be archiving.
ALTER TABLE ko_expenses ADD COLUMN archived_at timestamptz;

CREATE INDEX ko_expenses_live_idx ON ko_expenses (user_id, expense_date DESC)
  WHERE archived_at IS NULL;
