import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { KoAnalysis } = await import('./KoAnalysis');

const months = (filled: Record<number, number>) =>
  Array.from({ length: 12 }, (_, i) => filled[i] ?? 0);

const ANALYSIS = {
  year: new Date().getFullYear(),
  rows: [
    {
      koCategoryId: 1,
      koCategoryName: 'Wocheneinkauf',
      amountCents: 5000,
      ownShareCents: 2500,
      expenseCount: 2,
      shareOfTotal: 0.6,
      averagePerMonthCents: 1667,
      averageOwnSharePerMonthCents: 833,
      monthlyAmountCents: months({ 0: 2000, 2: 3000 }),
      monthlyOwnShareCents: months({ 0: 1000, 2: 1500 }),
    },
    {
      koCategoryId: null,
      koCategoryName: null,
      amountCents: 1000,
      ownShareCents: 1000,
      expenseCount: 1,
      shareOfTotal: 0.12,
      averagePerMonthCents: 333,
      averageOwnSharePerMonthCents: 333,
      monthlyAmountCents: months({ 3: 1000 }),
      monthlyOwnShareCents: months({ 3: 1000 }),
    },
  ],
  totalAmountCents: 6000,
  totalOwnShareCents: 3500,
  expenseCount: 3,
  monthsWithData: 3,
  uncategorizedCount: 1,
  excludedCount: 1,
  paidBy: [{ memberId: 1, name: 'Fabi', amountCents: 3000, expenseCount: 2 }],
  years: [2026, 2025],
};

const SERIES = {
  year: ANALYSIS.year,
  mode: 'category',
  subject: 'Wocheneinkauf',
  koCategoryId: 1,
  months: Array.from({ length: 12 }, (_, i) => ({
    month: i + 1,
    monthName: `M${i + 1}`,
    amountCents: i === 0 ? 2000 : i === 2 ? 3000 : 0,
    ownShareCents: i === 0 ? 1000 : i === 2 ? 1500 : 0,
    expenseCount: i === 0 || i === 2 ? 1 : 0,
  })),
  amountCents: 5000,
  ownShareCents: 2500,
  expenseCount: 2,
  averagePerActiveMonthCents: 2500,
  averageOwnSharePerActiveMonthCents: 1250,
  monthsWithData: 2,
};

// Februar is the only month both years hold: 2026 stops there and 2025 ran on to
// Juni. So the raw pair says the household halved its spending and the shared-month
// pair says it rose — the entire reason this screen has two bases.
const COMPARE = {
  year: 2026,
  previousYear: 2025,
  current: {
    year: 2026,
    amountCents: 7000,
    ownShareCents: 3500,
    expenseCount: 2,
    monthsWithData: 1,
    lastMonthWithData: 2,
    comparableAmountCents: 7000,
    comparableOwnShareCents: 3500,
    comparableExpenseCount: 2,
    excludedCount: 0,
  },
  previous: {
    year: 2025,
    amountCents: 13000,
    ownShareCents: 6500,
    expenseCount: 3,
    monthsWithData: 2,
    lastMonthWithData: 6,
    comparableAmountCents: 7000,
    comparableOwnShareCents: 3500,
    comparableExpenseCount: 2,
    excludedCount: 0,
  },
  comparableMonths: [2],
  fullyComparable: false,
  previousYearHasData: true,
  rows: [
    {
      koCategoryId: 1,
      koCategoryName: 'Wocheneinkauf',
      amountCents: 5000,
      ownShareCents: 2500,
      expenseCount: 1,
      previousAmountCents: 10000,
      previousOwnShareCents: 5000,
      previousExpenseCount: 2,
      deltaAmountCents: -5000,
      deltaOwnShareCents: -2500,
      deltaRatio: -0.5,
      comparableAmountCents: 5000,
      comparableOwnShareCents: 2500,
      comparablePreviousAmountCents: 4000,
      comparablePreviousOwnShareCents: 2000,
      comparableDeltaAmountCents: 1000,
      comparableDeltaOwnShareCents: 500,
      comparableDeltaRatio: 0.25,
      monthlyAmountCents: months({ 1: 5000 }),
      monthlyOwnShareCents: months({ 1: 2500 }),
      previousMonthlyAmountCents: months({ 1: 4000, 5: 6000 }),
      previousMonthlyOwnShareCents: months({ 1: 2000, 5: 3000 }),
      isNew: false,
      isGone: false,
    },
  ],
  paidBy: [
    {
      memberId: 1,
      name: 'Fabi',
      amountCents: 5000,
      previousAmountCents: 10000,
      deltaCents: -5000,
      expenseCount: 1,
      previousExpenseCount: 2,
    },
  ],
  years: [2026, 2025],
};

const TRAILING = {
  year: 2026,
  month: 2,
  fromYear: 2025,
  fromMonth: 3,
  amountCents: 13000,
  ownShareCents: 6500,
  expenseCount: 3,
  monthsWithData: 2,
  months: Array.from({ length: 12 }, (_, i) => ({
    year: i < 10 ? 2025 : 2026,
    month: ((2 + i) % 12) + 1,
    monthName: `M${i + 1}`,
    amountCents: i === 3 ? 6000 : i === 11 ? 7000 : 0,
    ownShareCents: i === 3 ? 3000 : i === 11 ? 3500 : 0,
    expenseCount: i === 3 || i === 11 ? 1 : 0,
  })),
  rows: [],
};

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) => {
    if (path.includes('/analysis/series/subjects')) return Promise.resolve([]);
    if (path.includes('/analysis/series')) return Promise.resolve(SERIES);
    if (path.includes('/analysis/categories')) return Promise.resolve(ANALYSIS);
    if (path.includes('/analysis/compare')) return Promise.resolve(COMPARE);
    if (path.includes('/analysis/trailing')) return Promise.resolve(TRAILING);
    if (path.startsWith('/years')) return Promise.resolve([]);
    return Promise.resolve({ items: [], total: 0 });
  });
});
afterEach(cleanup);

function renderIt() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter>
          <KoAnalysis />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

describe("the household's analysis", () => {
  /**
   * The rule the whole integration is built on. Both figures appear, they carry
   * their markers, and nothing on the screen is their sum.
   */
  it('reports the household amount and the own share as two separate figures', async () => {
    const { container } = renderIt();
    await screen.findAllByText('Wocheneinkauf');

    const row = [...container.querySelectorAll('.screen-table tbody tr')].find((r) =>
      r.textContent?.includes('Wocheneinkauf'),
    ) as HTMLElement;
    expect(within(row).getByText(/50,00/)).toBeInTheDocument();
    expect(within(row).getByText(/25,00/)).toBeInTheDocument();
    // 50,00 + 25,00 = 75,00 must appear nowhere: the two are never added.
    expect(container.textContent).not.toContain('75,00');
    // ...and each says which it is, for a screen reader too.
    expect(within(row).getByText(/50,00/).getAttribute('aria-label')).toMatch(/Haushalt/i);
    expect(within(row).getByText(/25,00/).getAttribute('aria-label')).toMatch(/Anteil/i);
  });

  it('says what KitchenOwl itself leaves out instead of quietly matching it', async () => {
    renderIt();
    expect(await screen.findByText(/1 Ausgaben sind in KitchenOwl/)).toBeInTheDocument();
  });

  it('charts the category that was clicked, and can reach the expenses behind it', async () => {
    const user = userEvent.setup();
    renderIt();
    await screen.findAllByText('Wocheneinkauf');

    api.mockClear();
    await user.click(screen.getAllByTitle('Diese Kategorie im Diagramm zeigen')[1]);
    const charted = api.mock.calls.map(([p]) => String(p));
    // The uncategorised row is a legitimate subject, not a dead end.
    expect(charted.some((p) => p.includes('uncategorized=true'))).toBe(true);

    api.mockClear();
    await user.click(await screen.findByRole('button', { name: /Ausgaben ansehen/ }));
    const listed = api.mock.calls.map(([p]) => String(p));
    expect(listed.some((p) => p.startsWith('/kitchenowl/expenses'))).toBe(true);
  });

  /** A mirror that starts in March must not be averaged over twelve months. */
  it('averages over the months the household was active', async () => {
    renderIt();
    await screen.findAllByText('Wocheneinkauf');
    expect(screen.getAllByText(/16,67/).length).toBeGreaterThan(0);
  });
});

describe("the household's year against last", () => {
  /**
   * The trap, and the reason the comparison is not just a subtraction.
   *
   * Raw, the household spent 70,00 against 130,00 — a 46 % saving that is entirely
   * Juni not having happened yet. Over the month both years actually hold it rose
   * from 40,00 to 50,00. The screen leads with the honest pair and says so.
   */
  it('leads with the shared months when the two years cover different ones', async () => {
    const user = userEvent.setup();
    const { container } = renderIt();

    await user.click(await screen.findByRole('button', { name: 'Jahresvergleich' }));

    expect(await screen.findByText(/Nur die 1 gemeinsamen Monate|1 gemeinsame Monate/)).toBeTruthy();
    const row = [...container.querySelectorAll('.screen-table tbody tr')].find((r) =>
      r.textContent?.includes('Wocheneinkauf'),
    ) as HTMLElement;
    // The shared-month pair: 50,00 against 40,00, so the change is a RISE.
    expect(within(row).getByText(/40,00/)).toBeInTheDocument();
    expect(within(row).getByText(/\+10,00/)).toBeInTheDocument();
    // The raw −50,00 is not what leads.
    expect(row.textContent).not.toContain('-50,00');
  });

  it('shows the raw years when asked, and says which basis is in use', async () => {
    const user = userEvent.setup();
    const { container } = renderIt();
    await user.click(await screen.findByRole('button', { name: 'Jahresvergleich' }));

    await user.click(await screen.findByRole('button', { name: 'Ganze Jahre zeigen' }));
    const cells = comparisonRow(container);
    // Whole years: 50,00 against 100,00, so the change is a fall.
    expect(cells[1].textContent).toContain('100,00');
    expect(cells[2].textContent).toContain('-50,00');
    expect(screen.getByText('Ganze Jahre')).toBeInTheDocument();
  });

  /** The pair rule survives the comparison: both figures, never their sum. */
  it('compares the household amount and the own share separately', async () => {
    const user = userEvent.setup();
    const { container } = renderIt();
    await user.click(await screen.findByRole('button', { name: 'Jahresvergleich' }));

    const cells = comparisonRow(container);
    // Column 0 is the household's amount, column 4 the user's share of it. They sit
    // in different columns, carry different markers, and 75,00 appears nowhere.
    expect(within(cells[0]).getByText(/50,00/).getAttribute('aria-label')).toMatch(/Haushalt/i);
    expect(within(cells[4]).getByText(/25,00/).getAttribute('aria-label')).toMatch(/Anteil/i);
    expect(container.textContent).not.toContain('75,00');
  });
});

/** The Wocheneinkauf row of the comparison table, cell by cell. */
function comparisonRow(container: HTMLElement): HTMLElement[] {
  const row = [...container.querySelectorAll('.screen-table tbody tr')].find((r) =>
    r.textContent?.includes('Wocheneinkauf'),
  ) as HTMLElement;
  return [...row.querySelectorAll('td')] as HTMLElement[];
}
