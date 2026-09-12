import { describe, expect, it } from 'vitest';

import { de } from './messages/de';
import { en } from './messages/en';

/**
 * Ground-truth data that arrives from the database in German and must stay German
 * in the English interface. These are the user's own vocabulary and the
 * vocabulary of the spreadsheet this replaces; translating them would make the
 * app disagree with the source data.
 */
const CATEGORY_NAMES = [
  'Gehalt', 'Freelancing', 'Sonstige Einnahmen', 'Miete', 'Nebenkosten', 'Strom',
  'Internet & Telefon', 'Rundfunkbeitrag', 'Versicherungen', 'Haustier',
  'Abos & Streaming', 'Server & Domains', 'Bank & Gebühren', 'Uni & Bildung', 'Sport',
  'Sparen & Anlage', 'Lebensmittel', 'Essen auswärts', 'Mensa', 'Auto & Parken',
  'Bahn & ÖPNV', 'Drogerie & Gesundheit', 'Haus & Garten', 'Kleidung & Merch',
  'Anschaffungen', 'Games & Software', 'Freizeit & Events', 'Reisen & Urlaub',
  'Dienstreisen', 'Geschenke', 'Bargeld', 'Sonstiges',
];
const TYPE_LABELS = ['Einkommen', 'Fixkosten', 'Variable Kosten', 'Sparen', 'Sonstiges'];

describe('message catalogues', () => {
  it('have exactly the same keys', () => {
    expect(Object.keys(en).sort()).toEqual(Object.keys(de).sort());
  });

  it('never translate a category or type name', () => {
    // The mechanical guard that keeps "data labels are not UI" true as the
    // catalogue grows. A contributor adding 'category.miete': 'Rent' fails here.
    const data = new Set([...CATEGORY_NAMES, ...TYPE_LABELS].map((s) => s.toLowerCase()));
    for (const [key, value] of Object.entries(en)) {
      expect(data.has(value.toLowerCase()), `en.${key} = "${value}" is a data label`).toBe(false);
    }
    for (const [key, value] of Object.entries(de)) {
      expect(data.has(value.toLowerCase()), `de.${key} = "${value}" is a data label`).toBe(false);
    }
  });

  it('carry the same interpolation placeholders in both languages', () => {
    const placeholders = (s: string) => (s.match(/\{(\w+)\}/g) ?? []).sort().join(',');
    for (const key of Object.keys(de) as (keyof typeof de)[]) {
      expect(placeholders(en[key]), `placeholders differ for ${key}`).toBe(
        placeholders(de[key]),
      );
    }
  });

  it('has no empty German string', () => {
    for (const [key, value] of Object.entries(de)) {
      if (key === 'money.basis.signed') continue; // deliberately blank
      expect(value.length, `de.${key} is empty`).toBeGreaterThan(0);
    }
  });
});
