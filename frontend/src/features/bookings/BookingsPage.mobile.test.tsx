import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import { BookingsPage } from './BookingsPage';

vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: vi.fn() };
});
const { api } = await import('../../lib/api');
const mocked = vi.mocked(api);

const PAGE = {
  items: [
    {
      id: 'b1', year: 2026, month: 6, monthName: 'Juni', bookedOn: null,
      kind: 'expense', amountCents: 4235, netCents: 4235, comment: 'Kaufland',
      taxRelevant: false, categoryId: 'c1', categoryName: 'Lebensmittel',
      categoryType: 'Variable Kosten', categorySource: 'rule', shared: false,
      externalSource: null, externalId: null, hasReceipt: false,
      status: 'confirmed', origin: 'legacy_month_only',
    },
    {
      id: 'b2', year: 2026, month: 6, monthName: 'Juni', bookedOn: null,
      kind: 'expense', amountCents: 999, netCents: 999, comment: 'Hofladen Brinkmann',
      taxRelevant: false, categoryId: null, categoryName: null,
      categoryType: null, categorySource: 'unresolved', shared: false,
      externalSource: null, externalId: null, hasReceipt: false,
      status: 'confirmed', origin: 'legacy_month_only',
    },
  ],
  total: 2, page: 0, pageSize: 200,
  sumIncomeCents: 0, sumExpenseCents: 5234, sumNetCents: 5234,
  uncategorizedCount: 1,
};

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/buchungen?jahr=2026']}>
        <I18nProvider initialLocale="de">
          <BookingsPage />
        </I18nProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  mocked.mockReset();
  mocked.mockResolvedValue(PAGE as never);
});
// vitest runs without `globals`, so cleanup is not auto-registered.
afterEach(cleanup);

describe('Buchungen on a phone', () => {
  /**
   * Eight columns need roughly 730px of width and a phone has 375. The table is
   * kept for the desktop and a card list rendered alongside it, with CSS choosing
   * — so the assertion is that BOTH exist, because a card list that only appears
   * under a media query cannot be seen by jsdom.
   */
  it('renders a card per booking beside the table', async () => {
    const { container } = renderPage();
    expect(await screen.findAllByText('Kaufland')).toHaveLength(2);

    expect(container.querySelector('.screen-table')).not.toBeNull();
    const cards = container.querySelectorAll('.screen-cards .bcard');
    expect(cards).toHaveLength(2);
  });

  it('keeps an unmatched booking as loud in a card as in a row', async () => {
    // The whole point of flagging: an unmatched booking must not become quieter
    // just because the viewport got narrower.
    const { container } = renderPage();
    await screen.findAllByText('Kaufland');

    const flagged = container.querySelectorAll('.bcard--uncategorized');
    expect(flagged).toHaveLength(1);
    expect(flagged[0].textContent).toContain('Hofladen Brinkmann');
  });

  it('leads each card with the amount, signed', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Kaufland');

    const first = container.querySelector('.screen-cards .bcard header .money');
    // An expense is money leaving, so it reads negative here even though the
    // stored amount is positive and the direction lives in `kind`.
    expect(first?.textContent).toMatch(/42,35/);
    expect(first?.textContent).toMatch(/[-−]/);
  });
});
