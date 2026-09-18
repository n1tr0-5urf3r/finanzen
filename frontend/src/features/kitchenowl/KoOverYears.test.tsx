import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { KoOverYears as KoOverYearsData } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { KoOverYears } = await import('./KoOverYears');

const DATA: KoOverYearsData = {
  years: [2024, 2025, 2026],
  amountPerYearCents: [100000, 300000, 200000],
  ownSharePerYearCents: [50000, 150000, 100000],
  monthsPerYear: [1, 12, 9],
  byCategory: [
    {
      key: '1',
      label: 'Wocheneinkauf',
      perYearAmountCents: [60000, 200000, 120000],
      perYearOwnShareCents: [30000, 100000, 60000],
      totalAmountCents: 380000,
      totalOwnShareCents: 190000,
      yearsActive: 3,
      expenseCount: 90,
    },
    {
      key: 'none',
      label: '',
      perYearAmountCents: [40000, 100000, 80000],
      perYearOwnShareCents: [20000, 50000, 40000],
      totalAmountCents: 220000,
      totalOwnShareCents: 110000,
      yearsActive: 3,
      expenseCount: 30,
    },
  ],
  byPayer: [
    {
      key: '2',
      label: 'Ante',
      perYearAmountCents: [70000, 160000, 110000],
      perYearOwnShareCents: [0, 0, 0],
      totalAmountCents: 340000,
      totalOwnShareCents: 0,
      yearsActive: 3,
      expenseCount: 70,
    },
    {
      key: '1',
      label: 'Fabi',
      perYearAmountCents: [30000, 140000, 90000],
      perYearOwnShareCents: [0, 0, 0],
      totalAmountCents: 260000,
      totalOwnShareCents: 0,
      yearsActive: 3,
      expenseCount: 50,
    },
  ],
  expenseCount: 120,
  excludedCount: 0,
};

function renderTab() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter initialEntries={['/kitchenowl?ansicht=analysis&zeitraum=jahre']}>
          <KoOverYears />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

afterEach(cleanup);

beforeEach(() => {
  api.mockReset();
  api.mockImplementation(() => Promise.resolve(DATA as never));
});

describe('the household over the years', () => {
  /** Every KitchenOwl figure is a pair, and the pair is never added up. */
  it('keeps the household total and the own share apart', async () => {
    const { container } = renderTab();
    await screen.findByText('2024–2026');

    // The euro figure carries a non-breaking space, so the assertion matches on
    // the digits rather than on a space that is not the one it looks like.
    const kpis = [...container.querySelectorAll('.kpi')].map((k) => k.textContent ?? '');
    expect(kpis.find((k) => k.includes('Haushalt gesamt'))).toMatch(/6\.000,00/);
    expect(kpis.find((k) => k.includes('Mein Anteil gesamt'))).toMatch(/3\.000,00/);
    // ...and no 9.000,00 anywhere, which is what adding them would produce.
    expect(container.textContent).not.toContain('9.000,00');
  });

  it('names the part years rather than letting them read as thrift', async () => {
    renderTab();
    await screen.findByText(/Angebrochene Jahre: 2024, 2026/);
  });

  it('switches to who fronted the money, and drops the share column with it', async () => {
    const user = userEvent.setup();
    const { container } = renderTab();
    await screen.findByText('2024–2026');

    // An expense KitchenOwl never categorised keeps its own row.
    expect(within(container).getAllByText('Ohne Kategorie').length).toBeGreaterThan(0);

    await user.click(screen.getByRole('button', { name: 'Zahler' }));

    await waitFor(() => {
      const head = [...container.querySelectorAll('.screen-table thead th')].map(
        (th) => th.textContent,
      );
      // A payer fronted the whole amount, so there is no share of it to show.
      expect(head).toEqual(['Bezahlt von', 'Gesamt', 'Jahre mit Buchungen', '2024', '2025', '2026']);
    });
    expect(screen.getByText('Ante über die Jahre')).toBeTruthy();
  });

  it('charts whichever subject the picker names', async () => {
    const user = userEvent.setup();
    renderTab();
    await screen.findByText('2024–2026');
    expect(screen.getByText('Wocheneinkauf über die Jahre')).toBeTruthy();

    await user.selectOptions(screen.getByLabelText('KitchenOwl-Kategorie'), 'none');
    await waitFor(() => expect(screen.getByText('Ohne Kategorie über die Jahre')).toBeTruthy());
  });
});
