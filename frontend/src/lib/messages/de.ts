/**
 * German UI strings.
 *
 * This file defines the KEY SET. `en.ts` is typed against it, so adding a key here
 * and forgetting it there is a build error rather than a missing label in
 * production.
 *
 * What belongs here: interface chrome — navigation, buttons, headings, help text,
 * error messages.
 *
 * What must NEVER appear here: data. Category names ("Essen auswärts"), category
 * type labels ("Fixkosten"), booking comments and rule keys are ground truth that
 * comes from the database in German and stays German in both languages. A test
 * asserts no value in en.ts equals any category or type name.
 */
export const de = {
  'app.name': 'Finanzen',
  'app.tagline': 'Haushaltsbuch',

  'nav.dashboard': 'Dashboard',
  'nav.bookings': 'Buchungen',
  'nav.months': 'Monate',
  'nav.analysis': 'Auswertung',
  'nav.tax': 'Steuer',
  'nav.categories': 'Kategorien',
  'nav.import': 'Import',
  'nav.settings': 'Einstellungen',
  'nav.quickAdd': 'Erfassen',
  'nav.uncategorizedBadge': '{count} ohne Kategorie',

  'common.loading': 'Wird geladen …',
  'common.retry': 'Erneut versuchen',
  'common.save': 'Speichern',
  'common.cancel': 'Abbrechen',
  'common.delete': 'Löschen',
  'common.close': 'Schließen',
  'common.back': 'Zurück',
  'common.year': 'Jahr',
  'common.month': 'Monat',
  'common.noValue': 'kein Wert',
  'common.total': 'Gesamt',
  'common.of': 'von',
  'common.skipToContent': 'Zum Inhalt springen',
  'common.sessionChecking': 'Sitzung wird geprüft …',
  'common.unknownError': 'Ein unbekannter Fehler ist aufgetreten.',

  'auth.login': 'Anmelden',
  'auth.logout': 'Abmelden',
  'auth.username': 'Benutzername',
  'auth.password': 'Passwort',
  'auth.displayName': 'Anzeigename',
  'auth.setupTitle': 'Ersteinrichtung',
  'auth.setupIntro':
    'Lege das Administratorkonto an. Weitere Konten legt danach dieses Konto an — die Registrierung ist standardmäßig geschlossen.',
  'auth.loginTitle': 'Anmelden',
  'auth.passwordHint': 'Mindestens 10 Zeichen.',

  'money.netAbbr': 'netto',
  'money.basis.net': 'netto',
  'money.basis.gross': 'brutto',
  'money.basis.signed': '',
  'money.netExplainer':
    'Netto = Ausgaben minus Einnahmen derselben Kategorie. Erstattungen und Anteile sind bereits abgezogen.',
  'money.credit': 'Gutschrift',
  'money.containsRefunds': 'enthält Erstattungen',

  'scope.withoutTransfers': 'Ohne Umbuchungen',
  'scope.withTransfers': 'Mit Umbuchungen',

  'dashboard.title': 'Finanzübersicht {year}',
  'dashboard.income': 'Einnahmen',
  'dashboard.expense': 'Ausgaben',
  'dashboard.balance': 'Bilanz',
  'dashboard.savingsRateNaive': 'Sparquote (naiv)',
  'dashboard.savingsRateNaiveHint':
    'Bilanz geteilt durch Bruttoeinnahmen — wie in der Tabelle. Zählt Sparbeiträge als Ausgabe und Erstattungen als Einkommen.',
  'dashboard.savingsRateConsumption': 'Sparquote',
  'dashboard.savingsRateConsumptionHint':
    'Echtes Einkommen minus echtem Konsum. Sparen und Umbuchungen zählen nicht als Verbrauch.',
  'dashboard.averageExpense': 'Ø Ausgaben / Monat',
  'dashboard.fixedCosts': 'Fixkosten / Monat',
  'dashboard.saved': 'Gespart',
  'dashboard.carryover': 'Vortrag',
  'dashboard.closingBalance': 'Bilanz gesamt',
  'dashboard.monthsWithData': 'Monate mit Daten',
  'dashboard.taxRelevant': 'Steuerrelevant',
  'dashboard.uncategorized': 'Ohne Kategorie',
  'dashboard.bookings': 'Buchungen',
  'dashboard.byType': 'Wohin das Geld fließt',
  'dashboard.topCategories': 'Größte Posten',
  'dashboard.carryoverGap':
    'Der eingetragene Vortrag weicht um {amount} vom Abschluss des Vorjahres ab.',

  'bookings.title': 'Buchungen',
  'bookings.new': 'Neu',
  'bookings.comment': 'Kommentar',
  'bookings.category': 'Kategorie',
  'bookings.type': 'Typ',
  'bookings.income': 'Einnahmen',
  'bookings.expense': 'Ausgaben',
  'bookings.net': 'Netto',
  'bookings.tax': 'Steuer',
  'bookings.filterAll': 'Alle',
  'bookings.filterUncategorized': 'Nur ohne Kategorie',
  'bookings.search': 'Kommentar suchen',
  'bookings.empty': 'Keine Buchungen für diesen Filter.',
  'bookings.summary': '{count} Buchungen · Einnahmen {income} · Ausgaben {expense} · Netto {net}',
  'bookings.summaryUncategorized': 'davon {count} ohne Kategorie',
  'bookings.sourceRule': 'Regel',
  'bookings.sourceManual': 'Manuell',
  'bookings.sourceNone': 'ohne Kategorie',
  'bookings.kind.income': 'Einnahme',
  'bookings.kind.expense': 'Ausgabe',
  'bookings.kind.transfer': 'Umbuchung',
  'bookings.noDay': 'kein Tag erfasst',

  'months.title': 'Monatsübersicht {year}',
  'months.intro':
    'Fixkosten, variable Kosten und Sparen sind Netto-Werte — Erstattungen und Anteile sind bereits abgezogen.',
  'months.cumulative': 'Kumuliert',
  'months.balance': 'Saldo',
  'months.savingsRate': 'Sparquote',

  'analysis.title': 'Auswertung nach Kategorie',
  'analysis.intro':
    'Alle Beträge netto: Ausgaben minus Einnahmen derselben Kategorie. Negativ bedeutet, dass unter dem Strich Geld hereinkam.',
  'analysis.transfersExcluded': '{count} Umbuchungen sind in dieser Auswertung nicht enthalten.',
  'analysis.share': 'Anteil',
  'analysis.shareNote': 'Gutschriften haben keinen Anteil an den Kosten.',
  'analysis.perMonth': 'Ø / Monat',
  'analysis.count': 'Buchungen',

  'tax.title': 'Steuerrelevante Buchungen {year}',
  'tax.intro': 'Markiert wird über die Spalte „Steuer“ in den Buchungen.',
  'tax.receipt': 'Beleg',
  'tax.receiptMissing': 'Beleg fehlt',
  'tax.receiptSummary': '{present} von {total} Belegen vorhanden',

  'categories.title': 'Kategorien & Regeln',
  'categories.tabCategories': 'Kategorien',
  'categories.tabRules': 'Regeln',
  'categories.rulesSummary': '{rules} Regeln · {unused} ungenutzt',
  'categories.ruleUnused': 'ungenutzt',
  'categories.ruleMatches': 'Treffer',
  'categories.newRule': 'Regel anlegen',
  'categories.ruleTarget': 'Kategorie',

  'import.title': 'Import',
  'import.dropHint': 'Arbeitsmappe hierher ziehen oder auswählen (.xlsx, .ods)',
  'import.preview': 'Vorschau',
  'import.rows': 'Zeilen',
  'import.willInsert': 'neu',
  'import.duplicates': 'bereits vorhanden',
  'import.commit': '{count} Buchungen übernehmen',
  'import.committed': '{count} Buchungen übernommen.',
  'import.reviewTitle': 'Prüfliste',
  'import.reviewIntro':
    'Nach Kommentar gruppiert und nach Häufigkeit sortiert: eine Entscheidung ordnet alle Buchungen mit diesem Kommentar zu.',
  'import.reviewCreateRule': 'Als Regel merken',
  'import.reviewAffects': '{count} Buchungen',
  'import.reviewSimilar': 'ähnliche Regeln',
  'import.reviewAmbiguous': 'mehrdeutig — bitte selbst wählen',
  'import.reviewSkip': 'Überspringen',
  'import.reviewDone': 'Prüfliste ist leer.',

  'quick.title': 'Schnellerfassung',
  'quick.frequent': 'Häufig',
  'quick.otherComment': 'Anderer Kommentar',
  'quick.commentSearch': 'Kommentar suchen oder neu eingeben',
  'quick.useAsNew': '„{comment}“ als neuen Kommentar verwenden',
  'quick.keypad': 'Ziffernblock',
  'quick.backspace': 'Letzte Ziffer löschen',
  'quick.saveWith': 'Speichern · {category}',
  'quick.saveGuess': 'Speichern · {category} ?',
  'quick.saveUnknown': 'Speichern · ohne Kategorie',
  'quick.saved': 'Gespeichert · {amount} · „{comment}“',
  'quick.savedUnknown': 'Kommentar ist noch keiner Kategorie zugeordnet.',
  'quick.createRule': 'Regel anlegen',
  'quick.later': 'Später',
  'quick.undo': 'Rückgängig',
  'quick.switchKind': 'Buchungsart wechseln',
  'quick.pickMonth': 'Monat wählen',
  'quick.templates': 'Vorlagen',

  'settings.title': 'Einstellungen',
  'settings.language': 'Sprache',
  'settings.theme': 'Darstellung',
  'settings.themeSystem': 'System',
  'settings.themeLight': 'Hell',
  'settings.themeDark': 'Dunkel',
  'settings.dataLanguageNote':
    'Kategorien, Typen und Kommentare bleiben immer auf Deutsch: sie sind Daten, keine Oberfläche. Beträge und Datumsangaben werden immer deutsch formatiert.',
  'settings.years': 'Jahre & Vortrag',
  'settings.openingBalance': 'Vortrag',
  'settings.users': 'Benutzer',

  'error.title': 'Da ist etwas schiefgelaufen',
  'error.offline': 'Keine Verbindung zum Server.',
  'empty.title': 'Nichts zu sehen',
} as const;

export type MessageKey = keyof typeof de;
export type Messages = Record<MessageKey, string>;
