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

const { FundsPage } = await import('./FundsPage');

const YEAR = new Date().getFullYear();

/** Nebenkosten: 1.440,00 a year, the bill lands in August, and it has landed. */
const NEBENKOSTEN = {
  fund: {
    id: 'nk',
    name: 'Nebenkosten',
    categoryId: 'c-nk',
    categoryName: 'Nebenkosten',
    annualCents: 144_000,
    dueMonth: 8,
    dueMonthName: 'August',
    note: null,
    active: true,
    sortOrder: 0,
  },
  monthlyAccrualCents: 12_000,
  accruedByMonthCents: 96_000, // eight months
  spentCents: 144_000,
  remainingCents: 0,
  overUnderCents: -48_000,
  duePassed: true,
};

/** Kfz-Versicherung: 307,00, due in Juli, not yet paid. */
const KFZ = {
  fund: {
    id: 'kfz',
    name: 'Kfz-Versicherung',
    categoryId: 'c-kfz',
    categoryName: 'Versicherungen',
    annualCents: 30_700,
    dueMonth: 7,
    dueMonthName: 'Juli',
    note: null,
    active: true,
    sortOrder: 1,
  },
  monthlyAccrualCents: 2_558,
  accruedByMonthCents: 15_350,
  spentCents: 0,
  remainingCents: 30_700,
  overUnderCents: 15_350,
  duePassed: false,
};

const OVERVIEW = {
  year: YEAR,
  month: 6,
  funds: [NEBENKOSTEN, KFZ],
  monthlyAccrualCents: 14_558,
  accruedByMonthCents: 111_350,
  spentCents: 144_000,
  owedToTheFutureCents: 30_700,
};

const SUGGESTIONS = [
  {
    categoryId: 'c-server',
    categoryName: 'Server & Domains',
    annualCents: 6_000,
    dueMonth: 8,
    dueMonthName: 'August',
    monthsWithSpending: 1,
    bookingCount: 2,
    year: YEAR,
  },
];

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) => {
    const p = String(path);
    if (p.startsWith('/funds/status')) return Promise.resolve(OVERVIEW);
    if (p.startsWith('/funds/suggestions')) return Promise.resolve(SUGGESTIONS);
    if (p.startsWith('/categories')) return Promise.resolve([{ id: 'c-nk', name: 'Nebenkosten' }]);
    return Promise.resolve([]);
  });
});
afterEach(cleanup);

/** The numeric cells of a row, in header order: annual, monthly, accrued, spent,
 *  difference. Queried by position because several of them legitimately carry the
 *  same figure — an unspent fund's target and its cushion are the same number. */
function cells(container: HTMLElement, name: string) {
  const row = [...container.querySelectorAll('.screen-table tbody tr')].find((r) =>
    r.textContent?.includes(name),
  ) as HTMLElement;
  const nums = [...row.querySelectorAll('td.num')] as HTMLElement[];
  return { row, annual: nums[0], monthly: nums[1], accrued: nums[2], spent: nums[3], diff: nums[4] };
}

function renderPage() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter initialEntries={[`/ruecklagen?jahr=${YEAR}`]}>
          <FundsPage />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

describe('Rücklagen', () => {
  /**
   * The figure the monthly saldo cannot give: of the lumps that are known, how
   * much is still coming. Nebenkosten has already been paid, so only the
   * Kfz-Versicherung counts towards it — a total that simply added the annual
   * amounts would be double the truth by August.
   */
  it('leads with what is still to come, not with what the year costs', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Nebenkosten');

    const totals = container.querySelector('.grid--kpi') as HTMLElement;
    expect(within(totals).getByText(/307,00/)).toBeInTheDocument();
    expect(totals.textContent).not.toContain('1.822,53');
  });

  /**
   * Soll and Ist side by side, never one number that could be either. And the
   * monthly rate is the annual figure divided by twelve — 1.440,00 → 120,00.
   */
  it('shows the monthly rate, what should be aside, and what was actually spent', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Nebenkosten');

    const c = cells(container, 'Nebenkosten');
    expect(c.annual.textContent).toContain('1.440,00');
    // 1.440,00 ÷ 12, to the cent.
    expect(c.monthly.textContent).toContain('120,00');
    // Eight months of it accrued, and the bill itself already paid in full.
    expect(c.accrued.textContent).toContain('960,00');
    expect(c.spent.textContent).toContain('1.440,00');
  });

  /**
   * The one column that can point either way. A bill that landed before the fund
   * caught up is money already gone — red and negative — and a fund still ahead of
   * its bill is a cushion. Getting this backwards would flatter exactly the case
   * the feature exists to warn about.
   */
  it('points the difference the right way in both directions', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Nebenkosten');

    const shortfall = within(cells(container, 'Nebenkosten').diff).getByText(/480,00/);
    expect(shortfall.textContent).toContain('-');
    expect(shortfall.className).toContain('money--expense');

    const cushion = within(cells(container, 'Kfz-Versicherung').diff).getByText(/153,50/);
    expect(cushion.textContent).toContain('+');
    expect(cushion.className).toContain('money--income');
  });

  /**
   * Suggested, never created. The evidence travels with the suggestion so the user
   * can disagree with it on sight, and accepting only opens the form.
   */
  it('offers suggestions with their evidence and creates nothing until saved', async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findByText('Server & Domains');

    expect(screen.getByText(/2 Buchungen in 1 Monat/)).toBeInTheDocument();

    api.mockClear();
    await user.click(screen.getByRole('button', { name: 'Übernehmen' }));

    // The form opens, filled in — and nothing has been POSTed.
    expect(await screen.findByRole('dialog')).toBeInTheDocument();
    expect(screen.getByLabelText('Name')).toHaveValue('Server & Domains');
    expect(api.mock.calls.every(([, init]) => !init || (init as RequestInit).method !== 'POST')).toBe(
      true,
    );
  });
});
