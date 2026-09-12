# Finanzen

A personal finance web app that replaces `konten_2026_auswertung.xlsx`.

The spreadsheet was well built — two-layer categorisation, netting, a tax tab, five
charts — but it is a desktop artefact, and the thing it cannot do is the thing that
happens most: recording a booking on a phone, in a shop, in ten seconds. That is why
this exists. Everything else is porting what already worked.

**The spreadsheet's arithmetic is the specification.** Every figure below was
recomputed from the raw `Einnahmen` / `Ausgaben` / `Kommentar` columns and the
workbook's own `Kategorien` sheet, and the test suite asserts the app reproduces all
of it exactly.

## What it does

- **Bookings** with income / expense / transfer, per-month or per-day, tax flags.
- **Two-layer categorisation**: a rule table mapping comment → category, plus a
  per-booking manual override that wins. A rule change recategorises history.
- **Netting everywhere**: a category's figure is expenses minus income *of the same
  category*. Rent reads 5.100 € net because a flatmate pays half of the 10.200 €
  that actually left the account.
- **Dashboard, monthly overview, category analysis, tax report**, all net, all with
  transfers excluded from consumption.
- **Import** of both `.xlsx` and `.ods`, with a dry-run preview and a review queue
  for comments no rule matches.
- **Recurring templates** with a per-month checklist and one *alle buchen* action.
  Monthly, quarterly and annual; templates whose amount varies book as drafts.
- **Receipts** photographed straight from the phone camera and attached to a booking.
- **Exports**: the tax list as CSV and a printable PDF, and the whole account as
  JSON (a restorable backup) or CSV.
- **Multi-user** with local auth; the authenticator sits behind a trait so OIDC can
  be added without touching call sites.

## Running it

```bash
cp .env.example .env
$EDITOR .env          # set APP_PUBLIC_URL, APP_SESSION_SECRET and both DB passwords
docker compose up -d
```

Then open the app and complete the first-run setup. Registration is closed after
that; further accounts are created by the admin under **Einstellungen**.

Behind nginx: `nginx.finanzen.conf` is a working example. `APP_PUBLIC_URL` must be
the exact public https URL — the session cookie's `Secure` flag and the CSRF origin
check are both derived from it.

## Importing the spreadsheets

1. **Kategorien → Regeln**: load the rule table (or let the review queue build it).
2. **Import**: upload `konten_2026_auswertung.xlsx`, check the preview, commit.
3. **Einstellungen → Jahre**: set the 2026 carry-over to `40.000,00 €`.
4. Repeat for `konten.ods` to bring in 2023–2025.

The legacy sheet has no month column; months are recovered from the saldo markers in
column D and the month labels that sit in column D or E. The importer reports which
blocks it inferred, and warns about the three that disagree with their own marker
rather than adjusting rows to make the sheet balance.

## Development

```bash
# Postgres for the tests
docker run -d --rm --name fin-pg -p 55432:5432 \
  -e POSTGRES_USER=finanzen -e POSTGRES_PASSWORD=finanzen -e POSTGRES_DB=finanzen \
  postgres:17-alpine

cd backend
cargo test                    # golden tests skip without fixtures
TEST_DATABASE_URL=postgres://finanzen:finanzen@localhost:55432/finanzen cargo test

# Real-data assertions need fixtures extracted from the workbooks first.
# They are gitignored: they carry every booking amount and comment.
cargo run --bin extract-fixtures -- \
  ../konten_2026_auswertung.xlsx ../konten.ods tests/fixtures

cd ../frontend && npm ci && npm run dev    # proxies /api to localhost:3100
```

## Design notes worth knowing before changing things

**Money is `i64` cents, everywhere.** Both source files carry IEEE-754 artifacts in
their raw XML — `67.29000000000001`, and the carry-over itself stored as
`45171.910000000011`. One rounding rule (half away from zero) is applied at exactly
two boundaries: a spreadsheet cell becoming cents, and a KitchenOwl float becoming
cents. Nothing downstream re-rounds.

**Netting is a generated column, not a convention.** `bookings.net_cents` is
`GENERATED ALWAYS AS` expense-positive, so a category net is a plain `SUM` with no
`CASE`, transfers contribute `0` structurally, and a forgotten
`AND kind <> 'transfer'` is harmless.

**The month is the canonical key; the day is optional.** 1404 of ~1878 bookings
genuinely have no day. A nullable date with generated period columns would leave the
majority NULL and unindexable, so `period_year`/`period_month` are NOT NULL and
`booked_on` is the refinement. "New bookings need a date" is a CHECK keyed on
provenance, not on a year threshold.

**Tenant isolation is enforced by Postgres, not by discipline.** Every user-scoped
table has `FORCE ROW LEVEL SECURITY`; handlers receive a `Tenant` that has already
set `app.user_id`, so no query binds a user id and the `WHERE user_id = $1` is
absent *by design* rather than forgotten. The app refuses to start if its database
role can bypass RLS — a superuser silently makes all of it inert. A metadata test
fails when any future table grows a `user_id` without a policy.

**Category names, type labels and comments are data.** They come from the database
in German and stay German in the English interface. So do all amounts and dates: the
money is euros and must match the bank statement. A test asserts no English string
equals any category or type name.

**Recurring bookings are idempotent by index, not by check.** One booking per
template per month, ever, enforced by a partial unique index — so pressing *alle
buchen* twice is a no-op rather than a double posting. A template whose amount
varies (the gym is 29,00 / 31,50 / 34,50) materialises as a **draft**: drafts are
outside `v_ledger`, so they move no total until confirmed with the real amount.

**Receipts live on disk, never in the database.** `APP_DATA_DIR/receipts/<user
id>/<uuid>.<ext>`, with the extension taken from the content type. The filename
the browser sends is metadata and is never a path component. Backup is `pg_dump`
plus that one directory; a 2 MB PDF per row in `bytea` would triple the dump and
make it useless as a quick restore path.

**Money is de-DE formatted in exactly one place outside the UI: the CSV and PDF
exports.** Their reader is a German Excel and a tax office, and `1234,56` opens as
a number there while `123456` opens as a six-figure line item. The JSON export
keeps integer cents, because its reader is `POST /exports/restore` — and a
round-trip through a formatted decimal is how a cent goes missing. A test exports
an account, restores it into a fresh user and compares every report field by
field.

**Both savings rates ship.** The naive one (`balance / gross income`) reproduces the
spreadsheet and is misleading on its own, because gross income includes cost-sharing
and refunds that are really negative expenses — it understates the rate by 26 points.
The consumption rate divides real income by real consumption. The identity
`savings_amount = balance + net(Sparen)` ties them together and is asserted.

**KitchenOwl is a separate, parallel ledger.** Its expenses are mirrored locally and
never summed with the personal bookings; pulled items land as drafts and are never
auto-booked. The two ledgers will not fully reconcile, by design.

## Verified against the source data

| | |
|---|---|
| Bookings 2026 | 474 |
| Einnahmen / Ausgaben | 36.000,00 € / 27.000,00 € |
| Bilanz 2026 | 9.000,00 € |
| Vortrag → Bilanz gesamt | 40.000,00 € → 49.000,00 € |
| Tax-relevant | 20 (816,92 € out, 6.000,00 € in) |
| Uncategorised | 0 |
| Savings rate | 25,00 % naive · 53,75 % consumption-based |
| Legacy | 1404 bookings, Juni 2023 – Dezember 2025, 31 month blocks |

Two bugs in the original workbook the app does not reproduce: the `Typ` column's
`VLOOKUP` range stopped one row short of the category table, so one category's
bookings silently became `Sonstiges` and the `Auswertung` tab's total was 460,00 €
short. Here the type is a foreign key, so a category without a type cannot exist.
