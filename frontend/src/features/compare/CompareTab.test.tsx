import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { CompareRow, CompareTotals, YearComparison } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

// Rendered through the screen that owns it: the comparison is a tab of
// /auswertung now, and testing the tab in isolation would stop proving that the
// tab is reachable at all.
const { AnalysisPage } = await import('../analysis/AnalysisPage');

function row(over: Partial<CompareRow>): CompareRow {
  return {
    categoryId: over.categoryName ?? 'x',
    categoryName: 'Sonstiges',
    categoryType: 'Sonstiges',
    netCents: 0,
    previousNetCents: 0,
    deltaCents: 0,
    deltaRatio: null,
    comparableNetCents: 0,
    comparablePreviousNetCents: 0,
    comparableDeltaCents: 0,
    comparableDeltaRatio: null,
    monthlyNetCents: Array.from({ length: 12 }, () => 0),
    previousMonthlyNetCents: Array.from({ length: 12 }, () => 0),
    bookingCount: 0,
    previousBookingCount: 0,
    isNew: false,
    isGone: false,
    ...over,
  };
}

function totals(over: Partial<CompareTotals>): CompareTotals {
  return {
    year: 2026,
    incomeCents: 0,
    expenseCents: 0,
    saldoCents: 0,
    bookingCount: 0,
    monthsWithData: 12,
    lastMonthWithData: 12,
    comparableIncomeCents: 0,
    comparableExpenseCents: 0,
    comparableSaldoCents: 0,
    ...over,
  };
}

/** Nine months of 2026 against a full 2025 — the real shape of this ledger. */
const PART_YEAR: YearComparison = {
  year: 2026,
  previousYear: 2025,
  current: totals({
    year: 2026,
    expenseCents: 90_000,
    incomeCents: 120_000,
    saldoCents: 30_000,
    monthsWithData: 9,
    lastMonthWithData: 9,
    comparableExpenseCents: 90_000,
    comparableIncomeCents: 120_000,
    comparableSaldoCents: 30_000,
  }),
  previous: totals({
    year: 2025,
    expenseCents: 120_000,
    incomeCents: 160_000,
    saldoCents: 40_000,
    monthsWithData: 12,
    lastMonthWithData: 12,
    comparableExpenseCents: 90_000,
    comparableIncomeCents: 120_000,
    comparableSaldoCents: 30_000,
  }),
  comparableMonths: [1, 2, 3, 4, 5, 6, 7, 8, 9],
  fullyComparable: false,
  rows: [
    // Identical spending in the nine shared months; the raw figures differ only
    // because 2025 has three more of them.
    row({
      categoryId: 'miete',
      categoryName: 'Miete',
      categoryType: 'Fixkosten',
      netCents: 90_000,
      previousNetCents: 120_000,
      deltaCents: -30_000,
      deltaRatio: -0.25,
      comparableNetCents: 90_000,
      comparablePreviousNetCents: 90_000,
      comparableDeltaCents: 0,
      comparableDeltaRatio: 0,
      bookingCount: 9,
      previousBookingCount: 12,
    }),
    row({
      categoryId: 'spotify',
      categoryName: 'Abos & Streaming',
      categoryType: 'Fixkosten',
      netCents: 2_700,
      previousNetCents: 0,
      deltaCents: 2_700,
      deltaRatio: null,
      comparableNetCents: 2_700,
      comparablePreviousNetCents: 0,
      comparableDeltaCents: 2_700,
      comparableDeltaRatio: null,
      bookingCount: 9,
      previousBookingCount: 0,
      isNew: true,
    }),
  ],
  byType: [],
  previousYearHasData: true,
};

const TRAILING = {
  year: 2026,
  month: 9,
  fromYear: 2025,
  fromMonth: 10,
  months: Array.from({ length: 12 }, (_, i) => ({
    year: i < 3 ? 2025 : 2026,
    month: i < 3 ? 10 + i : i - 2,
    monthName: 'Januar',
    incomeCents: 10_000,
    expenseCents: 8_000,
    saldoCents: 2_000,
    netCents: -2_000,
    bookingCount: 3,
  })),
  incomeCents: 120_000,
  expenseCents: 96_000,
  saldoCents: 24_000,
  bookingCount: 36,
  monthsWithData: 12,
  rows: [],
};

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) => {
    if (String(path).startsWith('/analysis/trailing')) return Promise.resolve(TRAILING as never);
    if (String(path).startsWith('/analysis/compare')) return Promise.resolve(PART_YEAR as never);
    return Promise.resolve([] as never);
  });
});
afterEach(cleanup);

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter initialEntries={['/auswertung?jahr=2026&ansicht=vergleich']}>
          <AnalysisPage />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

function tableRows(container: HTMLElement) {
  return [...container.querySelectorAll('.screen-table tbody tr')] as HTMLElement[];
}

describe('the year comparison', () => {
  /**
   * The trap the whole screen is built around. Nine months of identical spending
   * is 25 % less than twelve months of it, and reporting that as an improvement
   * would be the feature's first and worst lie — so the shared months lead.
   */
  it('leads with the shared months when the two years are different lengths', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Miete');

    const miete = tableRows(container).find((r) => r.textContent?.includes('Miete'))!;
    // Restricted: nothing changed, which is the truth.
    expect(within(miete).getAllByText(/900,00/).length).toBeGreaterThan(0);
    expect(miete.textContent).not.toContain('1.200,00');
    // And the banner explains why these are the figures on screen.
    expect(screen.getByText(/9 Monate mit Buchungen/)).toBeInTheDocument();
  });

  it('will show the raw years when asked, and says that is what it is doing', async () => {
    const user = userEvent.setup();
    const { container } = renderPage();
    await screen.findAllByText('Miete');

    await user.click(screen.getByRole('button', { name: /Trotzdem volle Jahre/ }));

    const miete = tableRows(container).find((r) => r.textContent?.includes('Miete'))!;
    expect(miete.textContent).toContain('1.200,00');
    expect(screen.getByText('Volle Jahre')).toBeInTheDocument();
  });

  /**
   * A cost that fell is money that stayed, so it is a plus — the same rule the
   * rest of the app applies to a net figure. The percentage flips with it, or the
   * two halves of one cell would point in opposite directions.
   */
  it('shows a cost that fell as money gained, in both the figure and the percentage', async () => {
    const user = userEvent.setup();
    const { container } = renderPage();
    await screen.findAllByText('Miete');
    await user.click(screen.getByRole('button', { name: /Trotzdem volle Jahre/ }));

    const miete = tableRows(container).find((r) => r.textContent?.includes('Miete'))!;
    // Stored delta is -30.000 (it cost less); displayed as +300,00 gained.
    const delta = within(miete).getByText(/\+300,00/);
    expect(delta.className).toContain('money--income');
    // de-DE puts U+00A0 before the percent sign, so match the digits and the sign
    // rather than the spacing.
    expect(miete.textContent).toMatch(/(^|[^-])25,00\s%/);
    expect(miete.textContent).not.toMatch(/-25,00\s%/);
  });

  it('prints a dash rather than a percentage for a category that is new', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Abos & Streaming');

    const spotify = tableRows(container).find((r) =>
      r.textContent?.includes('Abos & Streaming'),
    )!;
    expect(within(spotify).getByTitle(/im Vorjahr gab es in dieser Kategorie nichts/)).toBeTruthy();
    expect(within(spotify).getAllByText('neu').length).toBeGreaterThan(0);
  });

  it('asks for a trailing window that ends where the data ends, not at December', async () => {
    renderPage();
    await screen.findAllByText('Miete');

    const calls = api.mock.calls.map(([p]) => String(p));
    expect(calls.some((p) => p === '/analysis/trailing?year=2026&month=9')).toBe(true);
  });

  it('says a comparison needs two years when the first one has no predecessor', async () => {
    api.mockImplementation((path: string) => {
      if (String(path).startsWith('/analysis/compare'))
        return Promise.resolve({
          ...PART_YEAR,
          previousYearHasData: false,
          rows: [],
        } as never);
      return Promise.resolve([] as never);
    });
    renderPage();
    expect(await screen.findByText(/Für 2025 liegen keine Buchungen vor/)).toBeInTheDocument();
  });
});
