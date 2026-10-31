import { cleanup, render } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { Booking } from '../../lib/types';
import { CountedShare } from './BookingCard';

afterEach(cleanup);

const BASE: Booking = {
  id: 'b1', year: 2026, month: 6, monthName: 'Juni', bookedOn: '2026-06-02',
  kind: 'income', amountCents: 2000, netCents: -2000, comment: 'Ausgleich Juni',
  counterparty: null, purpose: null, taxRelevant: false, categoryId: 'c1',
  categoryName: 'Haushaltsausgleich', categoryType: 'Variable Kosten',
  categorySource: 'manual', shared: false, externalSource: null, externalId: null,
  hasReceipt: false, countedAmountCents: null, countedKind: null, countedBasis: null,
  status: 'confirmed', origin: 'manual',
};

function text(booking: Booking): string {
  const { container } = render(
    <I18nProvider initialLocale="de">
      <CountedShare booking={booking} />
    </I18nProvider>,
  );
  return (container.textContent ?? '').replace(/\s+/g, ' ');
}

describe('CountedShare', () => {
  it('names my share under a linked purchase', () => {
    expect(
      text({ ...BASE, kind: 'expense', amountCents: 9000, countedAmountCents: 3000,
             countedKind: 'expense', countedBasis: 'koShare' }),
    ).toBe('dein Anteil: 30,00 €');
  });

  it('says what a settlement counts at when linked shares are netted out of it', () => {
    expect(
      text({ ...BASE, amountCents: 6000, countedAmountCents: 0,
             countedKind: 'income', countedBasis: 'koSettlement' }),
    ).toBe('gezählt: 0,00 €');
  });

  it('says so when a settlement counts on the other side', () => {
    expect(
      text({ ...BASE, countedAmountCents: 4000, countedKind: 'expense',
             countedBasis: 'koSettlement' }),
    ).toBe('gezählt als Ausgabe: 40,00 €');
  });

  it('says so when a paid-out settlement counts as income', () => {
    expect(
      text({ ...BASE, kind: 'expense', amountCents: 1000, netCents: 1000,
             countedAmountCents: 2000, countedKind: 'income', countedBasis: 'koSettlement' }),
    ).toBe('gezählt als Einnahme: 20,00 €');
  });

  it('shows nothing when a settlement counts as booked', () => {
    expect(
      text({ ...BASE, countedAmountCents: 2000, countedKind: 'income',
             countedBasis: 'koSettlement' }),
    ).toBe('');
  });
});
