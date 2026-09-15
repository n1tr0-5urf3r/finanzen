import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { KoCategoryAnalysis } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { KoFlow, KoFlowPeriodPicker } = await import('./KoFlow');

function twelve(values: number[]): number[] {
  return Array.from({ length: 12 }, (_, i) => values[i] ?? 0);
}

const ANALYSIS: KoCategoryAnalysis = {
  year: 2026,
  rows: [
    {
      koCategoryId: 1,
      koCategoryName: 'Wocheneinkauf',
      amountCents: 6000,
      ownShareCents: 3000,
      expenseCount: 4,
      shareOfTotal: 0.6,
      averagePerMonthCents: 3000,
      averageOwnSharePerMonthCents: 1500,
      monthlyAmountCents: twelve([4000, 2000]),
      monthlyOwnShareCents: twelve([2000, 1000]),
    },
    {
      koCategoryId: null,
      koCategoryName: null,
      amountCents: 4000,
      ownShareCents: 2000,
      expenseCount: 2,
      shareOfTotal: 0.4,
      averagePerMonthCents: 2000,
      averageOwnSharePerMonthCents: 1000,
      monthlyAmountCents: twelve([1000, 3000]),
      monthlyOwnShareCents: twelve([500, 1500]),
    },
  ],
  totalAmountCents: 10000,
  totalOwnShareCents: 5000,
  expenseCount: 6,
  monthsWithData: 2,
  uncategorizedCount: 2,
  excludedCount: 0,
  paidBy: [
    {
      memberId: 2,
      name: 'Ante',
      amountCents: 7000,
      expenseCount: 4,
      monthlyAmountCents: twelve([4000, 3000]),
    },
    {
      memberId: 1,
      name: 'Fabi',
      amountCents: 3000,
      expenseCount: 2,
      monthlyAmountCents: twelve([1000, 2000]),
    },
  ],
  years: [2026],
};

function renderFlow() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter initialEntries={['/kitchenowl?ansicht=analysis&zeitraum=fluss']}>
          <KoFlowPeriodPicker year={2026} />
          <KoFlow year={2026} />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

afterEach(cleanup);

beforeEach(() => {
  api.mockReset();
  api.mockImplementation(() => Promise.resolve(ANALYSIS as never));
});

describe('the household money flow', () => {
  /**
   * The household ledger's own question. The personal diagram asks where income
   * went; this one asks who fronted the money — a fact the personal ledger cannot
   * have, and the reason the shared one is worth a diagram of its own.
   */
  it('shows who paid on the left and what it was for on the right', async () => {
    const { container } = renderFlow();
    await screen.findByText(/Haushalts-Geldfluss 2026/);

    const rows = [...container.querySelectorAll('.chart__data tbody tr')].map(
      (r) => r.textContent ?? '',
    );
    expect(rows.find((r) => r.includes('Ante → Haushaltsausgaben'))).toContain('70,00');
    expect(rows.find((r) => r.includes('Fabi → Haushaltsausgaben'))).toContain('30,00');
    expect(rows.some((r) => r.includes('Haushaltsausgaben → Wocheneinkauf'))).toBe(true);
    // An expense KitchenOwl never categorised is named, not dropped.
    expect(rows.some((r) => r.includes('Haushaltsausgaben → Ohne Kategorie'))).toBe(true);
  });

  /** The two figures of every household screen, side by side and never added. */
  it('reports the household total and the own share separately', async () => {
    const { container } = renderFlow();
    await screen.findByText(/Haushalts-Geldfluss 2026/);

    expect(within(container).getByText('100,00 €')).toBeTruthy();
    expect(within(container).getByText('50,00 €')).toBeTruthy();
    // ...and no balance: a household ledger has none, and one here would invite
    // adding it to the personal one.
    expect(container.textContent).not.toContain('Übrig');
  });

  it('splits the same total into the shares on request', async () => {
    const user = userEvent.setup();
    const { container } = renderFlow();
    await screen.findByText(/Haushalts-Geldfluss 2026/);

    await user.click(screen.getByRole('button', { name: 'Nach Anteil' }));

    await waitFor(() => {
      const rows = [...container.querySelectorAll('.chart__data tbody tr')].map(
        (r) => r.textContent ?? '',
      );
      expect(rows.find((r) => r.includes('→ Mein Anteil'))).toContain('50,00');
      expect(rows.find((r) => r.includes('→ Anteil der anderen'))).toContain('50,00');
      expect(rows.some((r) => r.includes('Wocheneinkauf'))).toBe(false);
    });
  });

  it('follows a single month through both columns', async () => {
    const user = userEvent.setup();
    const { container } = renderFlow();
    await screen.findByText(/Haushalts-Geldfluss 2026/);

    await user.selectOptions(screen.getByLabelText('Zeitraum'), '2');
    await screen.findByText(/Haushalts-Geldfluss Februar 2026/);

    const rows = [...container.querySelectorAll('.chart__data tbody tr')].map(
      (r) => r.textContent ?? '',
    );
    // Februar: Ante 30, Fabi 20 — and the categories add up to the same 50.
    expect(rows.find((r) => r.includes('Ante → Haushaltsausgaben'))).toContain('30,00');
    expect(rows.find((r) => r.includes('Fabi → Haushaltsausgaben'))).toContain('20,00');
    expect(rows.find((r) => r.includes('→ Ohne Kategorie'))).toContain('30,00');
  });
});
