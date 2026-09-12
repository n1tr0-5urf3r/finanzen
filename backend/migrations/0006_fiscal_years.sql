-- opening_source records *why* the opening balance is what it is. For 2026 it is
-- 'configured' (40.000,00 entered by hand), not 'derived' from the 1404 legacy rows,
-- whose sum is 39.750,00 — short by 250,00 because three month blocks disagree with
-- their own saldo markers. Recording the provenance in the row means the year
-- overview can *show* that gap instead of silently disagreeing with the spreadsheet.
CREATE TABLE fiscal_years (
  user_id        uuid     NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  year           smallint NOT NULL CHECK (year BETWEEN 1990 AND 2200),
  opening_cents  bigint   NOT NULL DEFAULT 0,
  opening_source text     NOT NULL DEFAULT 'derived'
                   CHECK (opening_source IN ('derived','configured')),
  closed_at      timestamptz,
  tax_locked_at  timestamptz,
  note           text,
  PRIMARY KEY (user_id, year)
);
SELECT app.enable_tenant_rls('fiscal_years');
