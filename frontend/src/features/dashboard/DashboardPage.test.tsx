import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { DashboardPage } = await import('./DashboardPage');

// The verified 2026 figures.
const DASHBOARD = {
  year: 2026,
  incomeCents: 3_600_000,
  expenseCents: 2_700_000,
  balanceCents: 900_000,
  openingBalanceCents: 4_000_000,
  closingBalanceCents: 4_900_000,
  carryoverGapCents: null,
  averageExpensePerMonthCents: 300_000,
  fixedCostsPerMonthCents: 90_000,
  monthsWithData: 9,
  savingsRateNaive: 0.25,
  savingsRateConsumption: 0.5375,
  savingsAmountCents: 1_548_000,
  savingsDepositCents: 648_000,
  savingsDepositPerMonthCents: 80_444,
  savingsDepositRate: 0.225,
  bookingCount: 474,
  taxRelevantCount: 20,
  uncategorizedCount: 0,
  uncategorizedNetCents: 0,
  byType: [
    { typeCode: 'fixkosten', label: 'Fixkosten', netCents: 810_000, bookingCount: 120 },
  ],
  topCategories: [
    {
      categoryId: 'miete',
      categoryName: 'Miete',
      categoryType: 'Fixkosten',
      incomeCents: 480_000,
      expenseCents: 960_000,
      netCents: 480_000,
      netIsNegative: false,
      shareOfTotal: 0.19,
      averagePerMonthCents: 56_667,
      bookingCount: 18,
      monthlyNetCents: Array.from({ length: 12 }, () => 0),
    },
  ],
};

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) =>
    Promise.resolve(path.startsWith('/dashboard') ? DASHBOARD : ({ items: [] } as never)),
  );
});
afterEach(cleanup);

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter initialEntries={['/dashboard?jahr=2026']}>
          <DashboardPage />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

describe('the dashboard', () => {
  /**
   * Three savings figures exist and they are not equals. The DEPOSIT is a decision
   * — it is what left the account for Sparen & Anlage and the only one checkable
   * against a bank statement — so it leads, and per month, which is the form a
   * standing order is thought of in.
   */
  it('leads with what was actually paid into savings, per month', async () => {
    const { container } = renderPage();
    await screen.findByText('Sparrate / Monat');

    const tile = [...container.querySelectorAll('.kpi')].find((k) =>
      k.textContent?.includes('Sparrate / Monat'),
    ) as HTMLElement;
    // 6.480,00 over the nine months that hold bookings.
    expect(within(tile).getByText(/804,44/)).toBeInTheDocument();
    expect(tile.textContent).toContain('6.480,00');
    expect(tile.textContent).toContain('22,50');
  });

  /**
   * The spreadsheet's own rate stays computable and visible — it was on every
   * screen for years — but as a footnote to the rate that supersedes it rather
   * than as a tile of equal weight.
   */
  it('keeps the spreadsheet rate as a footnote, not as a headline', async () => {
    const { container } = renderPage();
    await screen.findByText('Sparquote');

    const tiles = [...container.querySelectorAll('.kpi')];
    expect(tiles.some((k) => k.textContent?.trim().startsWith('Sparquote (naiv)'))).toBe(false);

    const consumption = tiles.find((k) => k.textContent?.includes('53,75')) as HTMLElement;
    expect(consumption).toBeTruthy();
    expect(consumption.textContent).toContain('25,00');
    expect(within(consumption).getByText(/25,00/).className).toContain('kpi__sub');
  });

  /**
   * Two tables on one screen, one convention. Both are rankings of what things
   * COST — the server filters the second to positive nets, so no income category
   * can appear in either — and a minus pointing one way in one and the other way
   * in the other is worse than either choice on its own.
   */
  it('states costs the same way in both tables', async () => {
    const { container } = renderPage();
    await screen.findByText('Fixkosten');

    const tables = [...container.querySelectorAll('table')];
    const byType = tables[0];
    const top = tables[tables.length - 1];

    expect(within(byType).getByText(/8\.100,00/).textContent).not.toContain('-');
    // The net cell, not the income leg that happens to be the same figure.
    const net = within(top).getByRole('button', { name: /4\.800,00/ });
    expect(net.textContent).not.toContain('-');
    // ...and the note under them stays true: a negative there means money came in.
    expect(container.textContent).toContain('ein negativer Wert bedeutet');
  });
});
