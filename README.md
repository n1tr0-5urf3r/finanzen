<p align="center">
  <img src="docs/logo.png" alt="Finanzen" width="120">
</p>

<h1 align="center">Finanzen</h1>

<p align="center">
  <em>Ein Haushaltsbuch, das die Tabelle ersetzt — und das Erfassen endlich aufs Handy bringt.</em>
</p>

<p align="center">
  <img alt="Rust" src="https://img.shields.io/badge/backend-Rust%20%2B%20axum-b7410e">
  <img alt="React" src="https://img.shields.io/badge/frontend-React%20%2B%20TypeScript-0096c4">
  <img alt="PostgreSQL" src="https://img.shields.io/badge/db-PostgreSQL%2017-336791">
  <img alt="Tests" src="https://img.shields.io/badge/tests-256-2e7d5b">
  <img alt="Deployment" src="https://img.shields.io/badge/deploy-Docker%20Compose-2496ed">
</p>

---

Die Tabelle konnte alles — zweistufige Kategorisierung, Netto-Rechnung, Steuerblatt,
Diagramme. Nur das, was am häufigsten passiert, konnte sie nicht: eine Buchung im
Laden, auf dem Handy, in zehn Sekunden. Genau dafür gibt es dieses Projekt.

**Die Arithmetik der Tabelle ist die Spezifikation.** Jede Zahl wurde aus den rohen
Spalten neu berechnet, nicht aus den Formelzellen gelesen, und die Testsuite prüft,
dass die App sie exakt reproduziert.

## Was es kann

- **Schnellerfassung in drei Tipps** — Betrag über einen Cent-zuerst-Ziffernblock,
  Kommentar aus den eigenen häufigsten Buchungen, fertig. Die Kategorie schlägt die
  Regeltabelle im Browser vor, ohne Server und ohne Verbindung; wählen kann man sie
  trotzdem jederzeit selbst.
- **Netto überall** — eine Kategorie zeigt Ausgaben *minus Einnahmen derselben
  Kategorie*. Die Miete steht mit 3.960 € netto da, weil ein Mitbewohner die Hälfte
  der 7.920 € zahlt, die tatsächlich vom Konto gingen. Beide Beträge bleiben sichtbar.
- **Zweistufige Kategorisierung** — eine Regeltabelle ordnet Kommentare zu, eine
  manuelle Zuordnung pro Buchung schlägt sie. Eine Regeländerung ordnet die
  Vergangenheit mit neu zu und sagt, wie viele Buchungen sie bewegt hat.
- **Import aus .xlsx und .ods** — mit Vorschau vor dem Übernehmen und einer Prüfliste
  für Kommentare, die keine Regel kennt: nach Häufigkeit sortiert, eine Entscheidung
  pro Kommentar, per Tastatur.
- **Steuer** — Buchungen markieren, Belege fotografieren, als CSV oder PDF exportieren.
- **Wiederkehrende Vorlagen** — die achtzehn Posten, die jeden Monat gleich sind, mit
  zwei Tipps buchen statt achtzehnmal tippen.
- **KitchenOwl** — die geteilten Ausgaben des Haushalts als *eigenes, getrenntes*
  Buch. Wird nie mit den privaten Buchungen verrechnet, und ein Abgleich bucht nichts
  von selbst.
- **Mehrere Benutzer** — getrennte Daten, per Row-Level-Security in der Datenbank
  erzwungen und nicht per Handler-Disziplin.

## Ansehen

| | |
|:--:|:--:|
| <a href="docs/screenshots/dashboard.png"><img src="docs/screenshots/dashboard.png" alt="Dashboard"></a> | <a href="docs/screenshots/monate.png"><img src="docs/screenshots/monate.png" alt="Monatsübersicht"></a> |
| **Dashboard** — beide Sparquoten, weil die naive Variante Erstattungen als Einkommen zählt | **Monatsübersicht** — die Linie endet beim letzten Monat mit Buchungen, statt flach weiterzulaufen |
| <a href="docs/screenshots/auswertung.png"><img src="docs/screenshots/auswertung.png" alt="Auswertung"></a> | <a href="docs/screenshots/buchungen.png"><img src="docs/screenshots/buchungen.png" alt="Buchungen"></a> |
| **Auswertung** — netto je Kategorie, Gutschriften als solche gekennzeichnet | **Buchungen** — filtern, suchen, bearbeiten; nicht zugeordnete Zeilen bleiben sichtbar markiert |
| <a href="docs/screenshots/quickadd.png"><img src="docs/screenshots/quickadd.png" alt="Schnellerfassung" width="260"></a> | <a href="docs/screenshots/mobil.png"><img src="docs/screenshots/mobil.png" alt="Mobil" width="260"></a> |
| **Schnellerfassung** — der Grund für das Projekt | **Mobil** — Tabellen werden zu Karten, nicht zu seitlichem Scrollen |
| <a href="docs/screenshots/pruefliste.png"><img src="docs/screenshots/pruefliste.png" alt="Prüfliste"></a> | <a href="docs/screenshots/steuer.png"><img src="docs/screenshots/steuer.png" alt="Steuer"></a> |
| **Prüfliste** — 238 unbekannte Kommentare, nach Häufigkeit sortiert | **Steuer** — Belegliste mit Kamera-Upload und CSV/PDF-Export |

<sub>Alle Screenshots zeigen erfundene Beispieldaten.</sub>

## Schnellstart

```bash
git clone <repo> finanzen && cd finanzen
cp .env.example .env
$EDITOR .env          # APP_PUBLIC_URL, APP_SESSION_SECRET und beide DB-Passwörter
docker compose up -d
```

Danach die Ersteinrichtung im Browser abschließen. Registrierung ist anschließend
geschlossen; weitere Konten legt dieses Konto unter **Einstellungen** an.

Hinter einem TLS-Proxy: `nginx.finanzen.conf` ist ein funktionierendes Beispiel.
`APP_PUBLIC_URL` muss exakt die öffentliche https-URL sein — davon hängen das
`Secure`-Flag des Session-Cookies und die CSRF-Prüfung ab.

## Konfiguration

Alles über `.env`; `.env.example` ist vollständig kommentiert.

| Variable | Bedeutung |
|---|---|
| `APP_PUBLIC_URL` | Exakte öffentliche URL. Steuert Cookie-Flag und CSRF-Prüfung. |
| `APP_SESSION_SECRET` | Mindestens 32 Zeichen. Ändern meldet alle ab. |
| `APP_DB_USER` / `APP_DB_PASSWORD` | Die Rolle, mit der die App verbindet — **kein** Superuser, sonst wäre RLS wirkungslos. Die App startet in dem Fall nicht. |
| `AUTH_ALLOW_REGISTRATION` | Standard `false`: Konten legt der Admin an. |
| `KITCHENOWL_URL` / `KITCHENOWL_TOKEN` | Leer lassen deaktiviert die Integration vollständig. |
| `KITCHENOWL_*_SECONDS` | Sync-Intervalle. `0` schaltet eine Schleife ab, sonst Minimum 60 s. |
| `IMPORT_FUZZY_MIN_CONFIDENCE` | Standard `0.92`. Niedriger erzeugt Vorschläge, die man reflexhaft annimmt und die falsch sind. |

## Entwicklung

```bash
# Postgres für die Tests
docker run -d --rm --name fin-pg -p 55432:5432 \
  -e POSTGRES_USER=finanzen -e POSTGRES_PASSWORD=finanzen -e POSTGRES_DB=finanzen \
  postgres:17-alpine

cd backend
TEST_DATABASE_URL=postgres://finanzen:finanzen@localhost:55432/finanzen cargo test

cd ../frontend && npm ci && npm run dev    # /api geht per Proxy an localhost:3100
```

Die Golden-Tests laufen gegen Fixtures, die aus den eigenen Arbeitsmappen erzeugt
werden. Sie liegen bewusst **nicht** im Repository — sie enthalten echte
Finanzdaten — und die Tests überspringen sich mit einem Hinweis, wenn sie fehlen:

```bash
cargo run --bin extract-fixtures -- ../konten_2026.xlsx ../konten.ods tests/fixtures
```

## Entscheidungen, die man vor dem Ändern kennen sollte

**Geld ist `i64` in Cent, überall.** Beide Quelldateien tragen IEEE-754-Artefakte im
rohen XML — `67.29000000000001`, und der Vortrag selbst als `45171.910000000011`. Eine
Rundungsregel, angewendet an genau zwei Stellen; danach rundet nichts mehr nach.

**Der Monat ist der Schlüssel, der Tag die Präzisierung.** Ein großer Teil der
importierten Buchungen hat gar keinen Tag. Ein nullable Datum mit generierten
Periodenspalten ließe die Mehrheit NULL und nicht indizierbar.

**Netto ist eine generierte Spalte, keine Konvention.** `net_cents` ist
`GENERATED ALWAYS AS` ausgaben-positiv — deshalb ist ein Kategorie-Netto ein
schlichtes `SUM` ohne `CASE`, und Umbuchungen tragen strukturell 0 bei.

**Mandantentrennung erzwingt Postgres, nicht Disziplin.** Jede Tabelle mit `user_id`
hat `FORCE ROW LEVEL SECURITY`; Handler bekommen eine Transaktion, in der
`app.user_id` schon gesetzt ist. Keine Query bindet eine Benutzer-ID — das fehlende
`WHERE user_id = $1` ist *by design* abwesend, nicht vergessen.

**Kategorienamen und Kommentare sind Daten.** Sie kommen auf Deutsch aus der
Datenbank und bleiben in der englischen Oberfläche deutsch. Beträge und Datumsangaben
werden immer de-DE formatiert: das Geld sind Euro und muss zum Kontoauszug passen.

## Lizenz

Noch nicht festgelegt.
