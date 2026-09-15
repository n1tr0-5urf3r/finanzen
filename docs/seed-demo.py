"""Seeds a throwaway instance with invented household data for the screenshots.

Every merchant, amount and date here is made up. Nothing comes from a real
workbook — the screenshots in the README are a showcase, not a leak, and this
script is committed so they can be regenerated without anyone's finances.

    docker run -d --name demo-db ... postgres:17-alpine
    docker run -d --name demo-app -p 127.0.0.1:3198:3100 ... <image>
    python3 docs/seed-demo.py
"""
import json, random, urllib.request
B = "http://127.0.0.1:3198/api/v1"
ORIGIN = "http://127.0.0.1:3198"
random.seed(20260912)

def call(path, body=None, cookie=None, method=None):
    d = json.dumps(body).encode() if body is not None else None
    h = {"Content-Type": "application/json", "Origin": ORIGIN}
    if cookie: h["Cookie"] = cookie
    r = urllib.request.Request(B + path, data=d, headers=h, method=method or ("POST" if d else "GET"))
    resp = urllib.request.urlopen(r)
    return resp, json.loads(resp.read() or b"null")

resp, _ = call("/auth/setup", {"username": "demo", "displayName": "Demo", "password": "demo-passwort-lang"})
cookie = resp.headers.get("Set-Cookie").split(";")[0]

cats = {c["name"]: c["id"] for c in call("/categories", cookie=cookie)[1]}

# comment -> category, all invented merchants
RULES = {
    "Gehalt": "Gehalt", "Nebenjob": "Freelancing",
    "Miete": "Miete", "Nebenkosten": "Nebenkosten", "Strom": "Strom",
    "Internet": "Internet & Telefon", "Rundfunkbeitrag": "Rundfunkbeitrag",
    "Hausrat": "Versicherungen", "Haftpflicht": "Versicherungen",
    "Tierfutter": "Haustier", "Streaming": "Abos & Streaming", "Musik": "Abos & Streaming",
    "Server": "Server & Domains", "Kontogebuehr": "Bank & Gebühren",
    "Semesterbeitrag": "Uni & Bildung", "Fitnessstudio": "Sport",
    "ETF": "Sparen & Anlage", "Bausparen": "Sparen & Anlage",
    "Supermarkt": "Lebensmittel", "Baeckerei": "Lebensmittel", "Getraenke": "Lebensmittel",
    "Mittagessen": "Essen auswärts", "Kaffee": "Essen auswärts", "Pizzeria": "Essen auswärts",
    "Mensa": "Mensa", "Tanken": "Auto & Parken", "Parkhaus": "Auto & Parken",
    "Zugticket": "Bahn & ÖPNV", "Apotheke": "Drogerie & Gesundheit", "Drogerie": "Drogerie & Gesundheit",
    "Baumarkt": "Haus & Garten", "Pflanzen": "Haus & Garten",
    "Kleidung": "Kleidung & Merch", "Schuhe": "Kleidung & Merch",
    "Elektronik": "Anschaffungen", "Computerspiel": "Games & Software",
    "Kino": "Freizeit & Events", "Konzert": "Freizeit & Events",
    "Hotel": "Reisen & Urlaub", "Flug": "Reisen & Urlaub",
    "Dienstreise": "Dienstreisen", "Geschenk": "Geschenke",
    "Bargeld": "Bargeld", "Sonstiges": "Sonstiges",
}
for pattern, cat in RULES.items():
    try: call("/rules", {"comment": pattern, "categoryId": cats[cat]}, cookie=cookie)
    except Exception: pass

FIXED = [("Miete", 88000, "expense"), ("Miete", 44000, "income"),
         ("Strom", 9500, "expense"), ("Internet", 4499, "expense"),
         ("Streaming", 1299, "expense"), ("Musik", 1099, "expense"),
         ("Fitnessstudio", 3490, "expense"), ("Haftpflicht", 6890, "expense"),
         ("ETF", 40000, "expense"), ("Bausparen", 15000, "expense"),
         ("Kontogebuehr", 149, "expense"), ("Tierfutter", 3200, "expense")]
VARIABLE = [("Supermarkt", 1800, 9500), ("Baeckerei", 350, 1200), ("Mittagessen", 750, 1900),
            ("Kaffee", 280, 620), ("Mensa", 400, 850), ("Tanken", 4500, 8900),
            ("Drogerie", 900, 3400), ("Zugticket", 690, 4900), ("Parkhaus", 200, 900),
            ("Pizzeria", 1400, 3800), ("Kleidung", 2500, 8900), ("Baumarkt", 1200, 6500),
            ("Computerspiel", 999, 5999), ("Kino", 1200, 2800), ("Getraenke", 600, 2400)]

n = 0
for month in range(1, 10):
    call("/bookings", {"year": 2026, "month": month, "kind": "income",
                       "amountCents": 298000 + random.randint(0, 9000),
                       "comment": "Gehalt"}, cookie=cookie); n += 1
    for comment, cents, kind in FIXED:
        call("/bookings", {"year": 2026, "month": month, "kind": kind,
                           "amountCents": cents, "comment": comment}, cookie=cookie); n += 1
    for _ in range(random.randint(14, 22)):
        comment, lo, hi = random.choice(VARIABLE)
        call("/bookings", {"year": 2026, "month": month, "kind": "expense",
                           "amountCents": random.randint(lo, hi), "comment": comment}, cookie=cookie); n += 1
    if month in (3, 7):
        call("/bookings", {"year": 2026, "month": month, "kind": "expense",
                           "amountCents": random.randint(38000, 92000), "comment": "Hotel"}, cookie=cookie); n += 1
    if month == 4:
        # a reimbursement, so the netting and the "enthält Erstattungen" pill show
        call("/bookings", {"year": 2026, "month": 4, "kind": "expense",
                           "amountCents": 74500, "comment": "Dienstreise"}, cookie=cookie)
        call("/bookings", {"year": 2026, "month": 5, "kind": "income",
                           "amountCents": 74500, "comment": "Dienstreise"}, cookie=cookie); n += 2
    if month == 6:
        call("/bookings", {"year": 2026, "month": 6, "kind": "transfer",
                           "amountCents": 20000, "comment": "Bargeld"}, cookie=cookie); n += 1

# a couple flagged for tax, and one uncategorised so the flag is visible
for m, c, a in [(2, "Semesterbeitrag", 19780), (6, "Semesterbeitrag", 19480), (8, "Server", 6000)]:
    call("/bookings", {"year": 2026, "month": m, "kind": "expense",
                       "amountCents": a, "comment": c, "taxRelevant": True}, cookie=cookie); n += 1
for m, c, a in [(7, "Wochenmarkt", 2340), (8, "Flohmarkt", 1800), (9, "Trödelladen", 950)]:
    call("/bookings", {"year": 2026, "month": m, "kind": "expense",
                       "amountCents": a, "comment": c}, cookie=cookie); n += 1

call("/years", {"year": 2026, "openingBalanceCents": 1284350}, cookie=cookie)
d = call("/dashboard?year=2026", cookie=cookie)[1]
print(f"{n} invented bookings · Bilanz {d['balanceCents']/100:.2f} · ohne Kategorie {d['uncategorizedCount']}")
open('/tmp/demo-cookie.txt','w').write(cookie)

# The household mirror, for the KitchenOwl screenshots. It normally arrives from a
# live KitchenOwl instance; the demo has none, so two invented members and 28
# invented expenses are written straight into the mirror tables instead. Run the
# SQL below against the demo database as a superuser (RLS is forced, so the app
# role could not write another user's rows):
#
#   docker exec -i demo-db psql -U postgres -d finanzen <<'SQL'
#   WITH u AS (SELECT id FROM users WHERE username = 'demo')
#   INSERT INTO ko_members (user_id, member_id, name, username, is_admin, is_owner,
#                           balance_cents, is_me)
#   SELECT u.id, m.member_id, m.name, m.username, false, m.member_id = 1, m.balance,
#          m.member_id = 1
#     FROM u, (VALUES (1,'Alex','alex',4210), (2,'Robin','robin',-4210))
#          AS m(member_id, name, username, balance);
#
#   WITH u AS (SELECT id FROM users WHERE username = 'demo'),
#        e AS (SELECT (ARRAY['Wocheneinkauf','Essen gehen','Ausflug','Haushalt',
#                            'Hobbies'])[1 + (i % 5)] AS cat,
#                     1 + (i % 5) AS cat_id,
#                     (ARRAY['Supermarkt','Pizzeria','Zoo','Baumarkt',
#                            'Kletterhalle'])[1 + (i % 5)] AS name,
#                     DATE '2026-01-05' + (i * 9) AS d,
#                     (1500 + ((i * 7919) % 6500))::bigint AS cents,
#                     CASE WHEN i % 3 = 0 THEN 1 ELSE 2 END AS payer, i
#                FROM generate_series(0, 27) AS i)
#   INSERT INTO ko_expenses (id, user_id, external_id, name, expense_date,
#                            amount_cents, own_share_cents, paid_by_id, paid_for,
#                            ko_category_id, ko_category_name,
#                            exclude_from_statistics, remote_hash)
#   SELECT gen_random_uuid(), u.id, 9000 + e.i, e.name, e.d, e.cents, e.cents / 2,
#          e.payer, '[{"user_id":1,"factor":1},{"user_id":2,"factor":1}]'::jsonb,
#          e.cat_id, e.cat, false, 'demo-' || e.i
#     FROM u, e;
#
#   -- and the two rows that mark the integration as set up for this account
#   INSERT INTO ko_sync_state (user_id, max_seen_id, household_id, household_name)
#   SELECT id, 9027, 1, 'Demo-WG' FROM users WHERE username = 'demo';
#   INSERT INTO ko_participants (user_id) SELECT id FROM users WHERE username = 'demo';
#   SQL
#
# The app then needs KITCHENOWL_URL/_TOKEN set to anything and every sync interval
# at 0, so the screens render from the mirror without reaching for a server.
