import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
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

const CATEGORIES = [
  { id: 'c1', name: 'Lebensmittel', typeCode: 'variabel', typeLabel: 'Variable Kosten',
    sortOrder: 1, archived: false, bookingCount: 12, netCents: 5000 },
  { id: 'c2', name: 'Miete', typeCode: 'fixkosten', typeLabel: 'Fixkosten',
    sortOrder: 2, archived: false, bookingCount: 18, netCents: 480000 },
];

/** Answers per endpoint, the way the real API does — a single catch-all response
 *  hid a crash in the editor when it received a booking page as its category list. */
function route(path: string) {
  if (path.startsWith('/categories')) return CATEGORIES;
  if (path.startsWith('/integrations/kitchenowl')) return { configured: false };
  return PAGE;
}

beforeEach(() => {
  mocked.mockReset();
  mocked.mockImplementation((path: string) => Promise.resolve(route(path) as never));
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

describe('the filter summary', () => {
  /**
   * The per-category convention is expense-positive — "Miete cost me 5.100" —
   * and that is right for a category. Summed over a whole filter the same number
   * is the balance, and rendering 9.000,00 gained as "-9.000,00" reads as a
   * loss. The sign is flipped and made explicit.
   */
  it('shows a balance gained as positive, not as a negative net', async () => {
    mocked.mockImplementation((path: string) =>
      Promise.resolve(
        (path.startsWith('/bookings?')
          ? { ...PAGE, sumIncomeCents: 3_600_000, sumExpenseCents: 2_700_000, sumNetCents: -900_000 }
          : route(path)) as never,
      ),
    );

    renderPage();
    await screen.findAllByText('Kaufland');

    const summary = screen.getByText(/36\.000,00/);
    expect(summary.textContent).toMatch(/\+9\.000,00/);
    expect(summary.textContent).not.toMatch(/[-−]9\.000,00/);
  });

  it('shows a balance lost as negative', async () => {
    mocked.mockImplementation((path: string) =>
      Promise.resolve(
        (path.startsWith('/bookings?')
          ? { ...PAGE, sumIncomeCents: 100_000, sumExpenseCents: 250_000, sumNetCents: 150_000 }
          : route(path)) as never,
      ),
    );

    renderPage();
    await screen.findAllByText('Kaufland');
    expect(screen.getByText(/1\.000,00/).textContent).toMatch(/[-−]1\.500,00/);
  });
});

describe('the booking editor', () => {
  /**
   * The editor is rendered at the end of the page's DOM, so without explicit
   * positioning it appears below the list rather than over it — the click looks
   * like it did nothing until you scroll to the bottom.
   */
  it('opens as a modal over the page, not inline at the end of the list', async () => {
    const user = userEvent.setup();
    const { container } = renderPage();
    await screen.findAllByText('Kaufland');

    expect(container.querySelector('.booking-editor')).toBeNull();

    await user.click(screen.getAllByText('Kaufland')[0]);

    const panel = container.querySelector('.booking-editor');
    expect(panel).not.toBeNull();
    expect(panel?.getAttribute('role')).toBe('dialog');
    expect(panel?.getAttribute('aria-modal')).toBe('true');
    // A scrim behind it, so the page underneath is visibly inert.
    expect(container.querySelector('.sheet-scrim')).not.toBeNull();
  });

  it('loads the clicked booking, not the first one', async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findAllByText('Hofladen Brinkmann');

    await user.click(screen.getAllByText('Hofladen Brinkmann')[0]);

    const comment = screen.getByLabelText('Kommentar') as HTMLInputElement;
    expect(comment.value).toBe('Hofladen Brinkmann');
  });
});
