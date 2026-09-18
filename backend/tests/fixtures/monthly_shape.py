"""Regenerates the two small workbooks the month-block reader is tested against.

    python3 backend/tests/fixtures/monthly_shape.py

Every name and amount here is invented. The point of these files is not the data
but the SHAPE: each trap the real 2014–2023 workbook contains, in nine rows —

  * three different apostrophes in the month labels (U+0027, U+2018, U+201A) and
    one label with a space inside it, `Januar ' 15`;
  * a negative amount in the Ausgaben column, which is money that came IN;
  * a row with an amount and no purpose at all;
  * `Kontostand` on a block's first row (the previous month's close, restated) as
    well as its last — which is why `Gewinn` is the marker and not `Kontostand`;
  * a month with no marker of its own;
  * a block whose marker disagrees with its own rows;
  * scratch values far to the right of the six real columns.

`broken_sequence.xlsx` skips a month, which must be refused rather than shifted.
"""
from openpyxl import Workbook

HEAD = ["Monat", "Einnahmen", "Ausgaben", "Zweck", "Kontostand", "Gewinn"]


def write(path, rows, scratch=None):
    wb = Workbook()
    ws = wb.active
    ws.title = "Tabelle1"
    ws.append(HEAD)
    for row in rows:
        ws.append(row)
    for cell, value in (scratch or {}).items():
        ws[cell] = value
    wb.save(path)


write(
    "backend/tests/fixtures/monthly_shape.xlsx",
    [
        ["November '14", None, 20.00, "Kfz-Versicherung", 1000.00, None],
        [None, 50.00, None, "Gehalt", None, None],
        [None, None, 5.00, "Kaffee", 1025.00, 24.00],  # marker disagrees by 1,00
        ["Dezember ‘14", 100.00, None, "Gehalt", None, None],
        [None, None, -19.98, "Rueckerstattung", None, None],  # negative = income
        [None, None, 10.00, None, 1134.98, 109.98],  # no purpose at all
        ["Januar ' 15", None, 30.00, "Miete", None, None],
        [None, None, None, None, 1104.98, -30.00],  # a marker-only row
        ["Februar ‚15", 7.00, None, "Zinsen", None, None],  # no marker at all
        [None, None, 2.00, "Brot", None, None],
    ],
    # Somebody's mental arithmetic, far to the right of the real columns.
    scratch={"I2": "Sparkonto", "J2": 2691.93, "U5": 50},
)

write(
    "backend/tests/fixtures/broken_sequence.xlsx",
    [
        ["November '14", None, 20.00, "Kfz-Versicherung", 1000.00, -20.00],
        ["Januar '15", None, 30.00, "Miete", 970.00, -30.00],
    ],
)
