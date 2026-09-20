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

const { SearchPage } = await import('./SearchPage');

const booking = (over: Record<string, unknown>) => ({
  id: 'b1',
  year: 2026,
  month: 2,
  monthName: 'Februar',
  bookedOn: null,
  kind: 'expense',
  amountCents: 2_500,
  netCents: 2_500,
  comment: 'Hofladen Brinkmann',
  counterparty: null,
  purpose: null,
  taxRelevant: false,
  categoryId: 'haus',
  categoryName: 'Haus & Garten',
  categoryType: 'Variable Kosten',
  categorySource: 'rule',
  shared: false,
  externalSource: null,
  externalId: null,
  hasReceipt: false,
  status: 'confirmed',
  origin: 'manual',
  ...over,
});

const RESULT = {
  query: 'hofladen brinkmann',
  items: [
    booking({}),
    booking({ id: 'b2', year: 2024, month: 3, monthName: 'März', amountCents: 4_210, netCents: 4_210 }),
  ],
  total: 5,
  page: 0,
  pageSize: 100,
  sumIncomeCents: 500,
  sumExpenseCents: 11_709,
  sumNetCents: 11_209,
  byYear: [
    { year: 2026, bookingCount: 2, incomeCents: 500, expenseCents: 2_500, netCents: 2_000 },
    { year: 2025, bookingCount: 2, incomeCents: 0, expenseCents: 4_999, netCents: 4_999 },
    { year: 2024, bookingCount: 1, incomeCents: 0, expenseCents: 4_210, netCents: 4_210 },
  ],
  comments: [
    { comment: 'Hofladen Brinkmann', bookingCount: 4, netCents: 8_209, categoryName: 'Haus & Garten' },
    { comment: 'Hofladen Brinkmann Berlin', bookingCount: 1, netCents: 3_000, categoryName: null },
  ],
};

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) =>
    Promise.resolve(
      String(path).startsWith('/bookings/search') ? RESULT : ([] as never),
    ),
  );
});
afterEach(cleanup);

function renderPage(entry = '/suche') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter initialEntries={[entry]}>
          <SearchPage />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

describe('the cross-year search', () => {
  /**
   * The listing is year-scoped, so "what have I ever paid this merchant" is four
   * page loads and a mental addition. The per-year table is the answer this screen
   * exists to give — it comes before the rows, and it must be complete.
   */
  it('answers per year, newest first, before showing a single booking', async () => {
    const { container } = renderPage('/suche?q=hofladen%20brinkmann');
    await screen.findByText('2026');

    const rows = [...container.querySelectorAll<HTMLElement>('.search-years tbody tr')];
    expect(rows).toHaveLength(3);
    expect(rows.map((r) => r.querySelector('th')?.textContent)).toEqual([
      '2026',
      '2025',
      '2024',
    ]);
    // 42,10 € in 2024. The expense column states the magnitude; the net column
    // states the direction, and the stored sign is expense-positive, so a cost
    // reads as money out.
    expect(within(rows[2]).getByText(/^42,10/)).toBeInTheDocument();
    expect(within(rows[2]).getByText(/^-42,10/)).toBeInTheDocument();
  });

  it('does not query until something has been asked', async () => {
    renderPage();
    await screen.findByText(/Noch nichts gesucht/);
    expect(api.mock.calls.some(([p]) => String(p).startsWith('/bookings/search'))).toBe(
      false,
    );
  });

  /**
   * Hand-typed comments drift. A total that silently folds two spellings together
   * is only trustworthy if it says which ones.
   */
  it('names the spellings it folded together', async () => {
    renderPage('/suche?q=hofladen');
    await screen.findByText('Hofladen Brinkmann Berlin');
    expect(screen.getByText(/×4/)).toBeInTheDocument();
  });

  it('sends the typed phrase and keeps it in the URL', async () => {
    const user = userEvent.setup();
    renderPage();
    await user.type(screen.getByLabelText('Suchbegriff'), 'kaufland');
    await user.click(screen.getByRole('button', { name: /Suchen/ }));

    const call = api.mock.calls.map(([p]) => String(p)).find((p) => p.includes('/bookings/search'));
    expect(call).toContain('q=kaufland');
  });

  /** A result is usually how a miscategorised booking gets found, so it opens. */
  it('opens a hit in the editor', async () => {
    const user = userEvent.setup();
    renderPage('/suche?q=hofladen');
    const cards = await screen.findAllByRole('button', { name: /Hofladen Brinkmann/ });
    await user.click(cards[0]);
    expect(await screen.findByRole('dialog')).toBeInTheDocument();
  });
});
