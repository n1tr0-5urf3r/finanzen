import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { TaxReport } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { TaxPage } = await import('./TaxPage');

const REPORT: TaxReport = {
  year: 2026,
  totalExpenseCents: 81692,
  totalIncomeCents: 600000,
  totalNetCents: -754308,
  bookingCount: 2,
  receiptsPresent: 1,
  entries: [
    {
      bookingId: 'a',
      index: 1,
      month: 2,
      monthName: 'Februar',
      comment: 'Semestergebühr',
      categoryName: 'Uni & Bildung',
      incomeCents: 0,
      expenseCents: 39000,
      hasReceipt: false,
    },
    {
      bookingId: 'b',
      index: 2,
      month: 3,
      monthName: 'März',
      comment: 'TTTech',
      categoryName: 'Freelancing',
      incomeCents: 600000,
      expenseCents: 0,
      hasReceipt: true,
    },
  ],
  byCategory: [
    {
      categoryName: 'Uni & Bildung',
      expenseCents: 39000,
      incomeCents: 0,
      netCents: 39000,
      count: 1,
    },
  ],
};

function renderPage(locale: 'de' | 'en' = 'de') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale={locale}>
        <MemoryRouter initialEntries={['/steuer?jahr=2026']}>
          <TaxPage />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

afterEach(cleanup);

beforeEach(() => {
  api.mockReset();
  api.mockResolvedValue(REPORT);
});

describe('the tax screen', () => {
  /**
   * `capture="environment"` is one attribute, it is invisible in every desktop
   * test, and losing it turns "photograph the receipt in the shop" back into a
   * file-picker dance. It is exactly the kind of thing that silently disappears
   * in a refactor, so it is asserted.
   */
  it('opens the rear camera on a phone and still accepts a PDF on a desktop', async () => {
    renderPage();
    await screen.findByText('Semestergebühr');

    const input = document.getElementById('receipt-a') as HTMLInputElement;
    expect(input).toBeTruthy();
    expect(input.type).toBe('file');
    expect(input.getAttribute('capture')).toBe('environment');
    expect(input.getAttribute('accept')).toBe('image/*,application/pdf');
  });

  it('offers capture where a receipt is missing and open/remove where it is not', async () => {
    renderPage();
    await screen.findByText('Semestergebühr');

    // The action offered names the booking, so a row of identical icons is still
    // unambiguous to a screen reader.
    expect(screen.getByLabelText(/Beleg aufnehmen — Semestergebühr/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Beleg öffnen — TTTech/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Beleg entfernen — TTTech/)).toBeInTheDocument();
    expect(screen.queryByLabelText(/Beleg öffnen — Semestergebühr/)).not.toBeInTheDocument();
  });

  it('warns while receipts are still missing', async () => {
    renderPage();
    await screen.findByText('1 von 2 Belegen vorhanden');
    expect(screen.getByText('1 von 2 Belegen vorhanden').closest('.kpi')?.className).toContain(
      'kpi--warn',
    );
  });

  it('renders amounts de-DE and category names in German even in English', async () => {
    renderPage('en');
    await screen.findByText('Semestergebühr');
    // It appears in the entry row and again in the per-category summary.
    expect(screen.getAllByText(/390,00/).length).toBeGreaterThan(0);
    // A category name is data: it stays German and is marked as such.
    const label = screen.getAllByText('Uni & Bildung')[0];
    expect(label.getAttribute('lang')).toBe('de');
    expect(label.getAttribute('translate')).toBe('no');
  });
});
