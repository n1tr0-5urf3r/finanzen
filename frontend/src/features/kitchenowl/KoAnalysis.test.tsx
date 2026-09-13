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

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) => {
    if (path.includes('/analysis/series/subjects')) return Promise.resolve([]);
    if (path.includes('/analysis/series')) return Promise.resolve(SERIES);
    if (path.includes('/analysis/categories')) return Promise.resolve(ANALYSIS);
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
