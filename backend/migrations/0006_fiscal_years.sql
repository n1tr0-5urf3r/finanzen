-- opening_source records *why* the opening balance is what it is. It may be
-- 'configured' — entered by hand — rather than 'derived' from the previous year's
-- rows, whose sum can fall short when month blocks disagree with their own saldo
-- markers. Recording the provenance in the row means the year overview can *show*
-- that gap instead of silently disagreeing with the spreadsheet.
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
