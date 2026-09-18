import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { OverYears, Year } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { OverYearsTab } = await import('./OverYearsTab');

/** Three years, the first of them a part year — this ledger starts in November. */
const OVER_YEARS: OverYears = {
  years: [2014, 2015, 2016],
  incomePerYearCents: [109885, 975679, 1040763],
  expensePerYearCents: [72556, 586336, 624401],
  balancePerYearCents: [37329, 389343, 416362],
  monthsPerYear: [2, 12, 12],
  byType: [
    {
      key: 'fixkosten',
      label: 'Fixkosten',
      categoryId: null,
      categoryType: 'fixkosten',
      perYearCents: [10000, 120000, 130000],
      totalCents: 260000,
      yearsActive: 3,
      bookingCount: 60,
    },
  ],
  byCategory: [
    {
      key: 'miete',
      label: 'Miete',
      categoryId: 'miete',
      categoryType: 'Fixkosten',
      perYearCents: [8000, 96000, 108000],
      totalCents: 212000,
      yearsActive: 3,
      bookingCount: 26,
    },
    {
      // Bought once, never again: the case a year-on-year screen cannot show.
      key: 'auto',
      label: 'Anschaffungen',
      categoryId: 'auto',
      categoryType: 'Variable Kosten',
      perYearCents: [0, 0, 870000],
      totalCents: 870000,
      yearsActive: 1,
      bookingCount: 2,
    },
    {
      // A year where a reimbursement outweighed the spending.
      key: 'dienstreisen',
      label: 'Dienstreisen',
      categoryId: 'dienstreisen',
      categoryType: 'Variable Kosten',
      perYearCents: [0, -30000, 12000],
      totalCents: -35849,
      yearsActive: 2,
      bookingCount: 9,
    },
  ],
  bookingCount: 284,
  uncategorizedCount: 0,
};

const YEARS: Year[] = [2014, 2015, 2016].map((year, i) => ({
  year,
  openingBalanceCents: [109139, 146468, 535811][i] as number,
  openingSource: i === 0 ? 'configured' : 'derived',
  locked: false,
  bookingCount: 10,
  incomeCents: OVER_YEARS.incomePerYearCents[i] as number,
  expenseCents: OVER_YEARS.expensePerYearCents[i] as number,
  balanceCents: OVER_YEARS.balancePerYearCents[i] as number,
  closingBalanceCents: [146468, 535811, 952173][i] as number,
  carryoverGapCents: null,
}));

function renderTab() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter initialEntries={['/auswertung?ansicht=jahre']}>
          <OverYearsTab />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

afterEach(cleanup);

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) =>
    Promise.resolve((String(path).startsWith('/years') ? YEARS : OVER_YEARS) as never),
  );
});

describe('the over-the-years screen', () => {
  it('puts every year in one row, with the span and the extremes above it', async () => {
    const { container } = renderTab();
    await screen.findByText('2014–2016');

    // Every year is a column of the matrix, not a page of its own.
    const head = container.querySelectorAll('.screen-table thead th');
    expect([...head].map((th) => th.textContent)).toEqual([
      'Kategorie',
      'Gesamt',
      'Jahre mit Buchungen',
      '2014',
      '2015',
      '2016',
    ]);

    const row = [...container.querySelectorAll('.screen-table tbody tr')].find((r) =>
      r.textContent?.includes('Anschaffungen'),
    ) as HTMLElement;
    // Bought once: two of the three years are empty, and empty is a dash rather
    // than 0,00 €, which would claim a year of free cars.
    expect(within(row).getAllByText('–')).toHaveLength(2);
    // ...and the row says so: one year of three carries a booking.
    expect(row.querySelectorAll('td')[1]?.textContent).toBe('1');
  });

  /**
   * The one way this screen could lie. Two months of spending beside twelve is not
   * thrift, and the ledger's first year has exactly two.
   */
  it('names the part years instead of letting them read as frugal', async () => {
    renderTab();
    await screen.findByText(/Angebrochene Jahre: 2014/);
  });

  it('charts the row that was clicked, across all years', async () => {
    const user = userEvent.setup();
    const { container } = renderTab();
    await screen.findByText('2014–2016');

    // The first row is charted before anything is touched, so the screen answers
    // something on arrival.
    expect(screen.getByText('Miete über die Jahre')).toBeTruthy();

    await user.click(screen.getAllByTitle('Diese Zeile im Diagramm zeigen')[2] as HTMLElement);

    await waitFor(() => expect(screen.getByText('Dienstreisen über die Jahre')).toBeTruthy());
    // A reimbursement year is money IN and is shown as a gain, not as a negative
    // cost: the stored −300,00 reads as +300,00 in the chart's own table.
    const table = container.querySelectorAll('.chart__data table');
    const chart = table[table.length - 1] as HTMLElement;
    expect(within(chart).getByText(/^\+300,00/)).toBeTruthy();
  });

  it('switches the matrix to the five types', async () => {
    const user = userEvent.setup();
    const { container } = renderTab();
    await screen.findByText('2014–2016');

    await user.click(screen.getByRole('button', { name: 'Typen' }));

    await waitFor(() => {
      const head = container.querySelector('.screen-table thead th');
      expect(head?.textContent).toBe('Typ');
    });
    expect(screen.getByText('Fixkosten über die Jahre')).toBeTruthy();
  });
});
