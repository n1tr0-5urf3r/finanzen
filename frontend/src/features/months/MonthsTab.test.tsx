import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import { MONTHS_DE } from '../../lib/format';
import type { CategoryTypeSummary, MonthlyOverview, MonthlyRow } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { MonthsTab } = await import('./MonthsTab');

function month(m: number, over: Partial<MonthlyRow> = {}): MonthlyRow {
  return {
    month: m,
    monthName: MONTHS_DE[m - 1],
    incomeCents: 0,
    expenseCents: 0,
    balanceCents: 0,
    cumulativeCents: null,
    savingsRate: null,
    fixedCostsNetCents: 0,
    variableCostsNetCents: 0,
    savingsNetCents: 0,
    otherNetCents: 0,
    bookingCount: 0,
    uncategorizedCount: 0,
    ...over,
  };
}

/** The verified 2026 figures, thinned to the rows the assertions need. */
const OVERVIEW: MonthlyOverview = {
  year: 2026,
  months: [
    month(1, { incomeCents: 300000, expenseCents: 250000, balanceCents: 50000, cumulativeCents: 50000, bookingCount: 57, fixedCostsNetCents: 90000, variableCostsNetCents: 80000 }),
    month(2, { incomeCents: 300000, expenseCents: 280000, balanceCents: 20000, cumulativeCents: 70000, bookingCount: 53 }),
    month(3, { incomeCents: 1040000, expenseCents: 280000, balanceCents: 760000, cumulativeCents: 830000, bookingCount: 54 }),
    month(4, { incomeCents: 300000, expenseCents: 400000, balanceCents: -100000, cumulativeCents: 730000, bookingCount: 52 }),
    month(5, { incomeCents: 300000, expenseCents: 380000, balanceCents: -80000, cumulativeCents: 650000, bookingCount: 52 }),
    // The netting artefact: Juni's variable costs are NEGATIVE because a refund
    // landed in Dienstreisen. Any implementation that clamps or takes an absolute
    // value fails right here.
    month(6, { incomeCents: 500000, expenseCents: 300000, balanceCents: 200000, cumulativeCents: 850000, bookingCount: 45, variableCostsNetCents: -30000 }),
    month(7, { incomeCents: 300000, expenseCents: 310000, balanceCents: -10000, cumulativeCents: 840000, bookingCount: 74 }),
    month(8, { incomeCents: 260000, expenseCents: 300000, balanceCents: -40000, cumulativeCents: 800000, bookingCount: 61 }),
    month(9, { incomeCents: 300000, expenseCents: 200000, balanceCents: 100000, cumulativeCents: 900000, bookingCount: 26 }),
    // What the server really sends for the tail: it keeps the running value alive
    // once data has been seen, so these are NOT null on the wire.
    month(10, { cumulativeCents: 900000 }),
    month(11, { cumulativeCents: 900000 }),
    month(12, { cumulativeCents: 900000 }),
  ],
  total: month(0, {
    incomeCents: 3600000,
    expenseCents: 2700000,
    balanceCents: 900000,
    bookingCount: 474,
    fixedCostsNetCents: 810000,
    variableCostsNetCents: 990000,
    savingsNetCents: 648000,
    otherNetCents: 42000,
  }),
};
OVERVIEW.total.monthName = 'Gesamt';

const TYPES: CategoryTypeSummary[] = [
  { typeCode: 'einkommen', label: 'Einkommen', netCents: -2880000, bookingCount: 40 },
  { typeCode: 'fixkosten', label: 'Fixkosten', netCents: 810000, bookingCount: 120 },
  { typeCode: 'variabel', label: 'Variable Kosten', netCents: 990000, bookingCount: 250 },
  { typeCode: 'sparen', label: 'Sparen', netCents: 648000, bookingCount: 9 },
  { typeCode: 'sonstiges', label: 'Sonstiges', netCents: 42000, bookingCount: 55 },
];

function renderPage(locale: 'de' | 'en' = 'de') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale={locale}>
        <MemoryRouter initialEntries={['/auswertung?ansicht=monate&jahr=2026']}>
          <MonthsTab year={2026} />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

/** vitest runs without globals, so cleanup is not auto-registered. */
afterEach(cleanup);

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) =>
    Promise.resolve(path.startsWith('/category-types') ? TYPES : OVERVIEW),
  );
});

function tableRows(container: HTMLElement) {
  return [...container.querySelectorAll('.screen-table tbody tr')] as HTMLElement[];
}

describe('the monthly overview', () => {
  it('keeps months with no bookings as rows instead of dropping them', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Januar');

    const rows = tableRows(container);
    expect(rows).toHaveLength(12);
    expect(within(rows[11]).getByText('Dezember')).toBeInTheDocument();
    expect(rows[11].className).toContain('row--empty');
  });

  /**
   * The one that matters. A zero-filled cumulative draws a cliff in the running
   * balance that never happened, so an absent figure must stay absent — and a
   * repeated one asserts three months that did not happen, which is the same lie
   * in the other direction.
   */
  it('shows no cumulative for a month with no bookings, neither 0,00 nor the carried value', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Januar');

    const october = tableRows(container)[9];
    expect(within(october).getByText('Oktober')).toBeInTheDocument();
    expect(october.textContent).not.toContain('0,00');
    expect(october.textContent).not.toContain('9.000,00');
    // Money's explicit "no value", not a number.
    expect(within(october).getAllByLabelText('kein Wert').length).toBeGreaterThan(0);

    // September, by contrast, carries the real running balance.
    expect(tableRows(container)[8].textContent).toContain('9.000,00');
  });

  it('renders a leading month, where the wire really is null, as absent too', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Januar');
    // Nothing precedes January in this fixture, so use the total row, which the
    // server deliberately sends without a cumulative rather than repeating the
    // balance beside it.
    const foot = container.querySelector('.screen-table tfoot tr') as HTMLElement;
    expect(within(foot).getAllByLabelText('kein Wert').length).toBeGreaterThan(0);
  });

  it('renders a negative net as a credit rather than a bare minus', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Januar');

    const june = tableRows(container)[5];
    const credit = within(june).getByText(/-300,00/);
    expect(credit.className).toContain('money--credit');
    // And it says "netto" aloud, so the sign is never read without its basis.
    expect(credit.getAttribute('aria-label')).toContain('netto');
  });

  it('takes the four type column headings from the database, not the catalogue', async () => {
    const { container } = renderPage('en');
    await screen.findAllByText('Januar');

    const headers = [...container.querySelectorAll('.screen-table thead th')].map(
      (th) => th.textContent,
    );
    // Type labels are data: German even in the English interface.
    expect(headers).toContain('Variable Kosten');
    expect(headers).toContain('Fixkosten');

    // Marked as data everywhere it appears, chart axis included.
    for (const label of screen.getAllByText('Variable Kosten')) {
      expect(label.getAttribute('lang')).toBe('de');
      expect(label.getAttribute('translate')).toBe('no');
    }
  });

  it('totals the year to the verified figures', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Januar');

    const foot = container.querySelector('.screen-table tfoot tr') as HTMLElement;
    expect(foot.textContent).toContain('36.000,00');
    expect(foot.textContent).toContain('27.000,00');
    expect(foot.textContent).toContain('9.000,00');
  });

  it('stops the cumulative chart at the last month with bookings', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Januar');

    // Nine points drawn, not twelve: October onward has no data to plot.
    expect(container.querySelectorAll('.chart__dot')).toHaveLength(9);
  });

  /**
   * The chart answers the question the screen is opened with — did this month end
   * up or down — rather than showing the two figures that question is computed
   * from. Those stay in the table, a row per month.
   */
  it('charts what each month came to, not what it came from', async () => {
    const { container } = renderPage();
    await screen.findByText('Saldo je Monat');

    expect(screen.queryByText('Einnahmen und Ausgaben je Monat')).toBeNull();

    // One bar per month with bookings, drawn as a flow: a month that gained goes
    // up, a month that cost more goes down.
    const table = container.querySelector('.chart__data table') as HTMLElement;
    const rows = [...table.querySelectorAll('tbody tr')].map((r) => r.textContent ?? '');
    expect(rows[0]).toContain('Januar');
    expect(rows.some((r) => /\+/.test(r))).toBe(true);
  });
});
