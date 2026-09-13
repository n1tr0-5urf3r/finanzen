import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { RecurringTemplate } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { RecurringPage } = await import('./RecurringPage');

function template(overrides: Partial<RecurringTemplate>): RecurringTemplate {
  return {
    id: 'x',
    name: 'Vorlage',
    comment: 'Vorlage',
    kind: 'expense',
    amountCents: 1000,
    amountIsEstimate: false,
    categoryId: null,
    categoryName: null,
    categoryType: null,
    categoryFromRule: false,
    taxRelevant: false,
    dayOfMonth: 1,
    intervalMonths: 1,
    anchor: { year: 2026, month: 1 },
    activeFrom: { year: 2026, month: 1 },
    activeTo: null,
    active: true,
    sortOrder: 0,
    dueInPeriod: true,
    bookedInPeriod: false,
    lastBooked: null,
    bookingCount: 0,
    ...overrides,
  };
}

const TEMPLATES: RecurringTemplate[] = [
  template({ id: 'miete', name: 'Miete', comment: 'Miete', amountCents: 110000 }),
  template({ id: 'spotify', name: 'Spotify', comment: 'Spotify', amountCents: 300 }),
  // Already booked this month: present, visible, and not bookable again.
  template({ id: 'strom', name: 'Strom', comment: 'Strom', bookedInPeriod: true }),
  // Quarterly, not due in this month.
  template({
    id: 'versicherung',
    name: 'Versicherung',
    comment: 'Versicherung',
    intervalMonths: 3,
    dueInPeriod: false,
  }),
];

function renderPage() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter initialEntries={['/vorlagen?jahr=2026&monat=3']}>
          <RecurringPage />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

afterEach(cleanup);

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) => {
    if (path.startsWith('/recurring?')) return Promise.resolve(TEMPLATES);
    if (path === '/categories') return Promise.resolve([]);
    if (path.startsWith('/bookings?')) {
      return Promise.resolve({
        items: [],
        total: 0,
        page: 0,
        pageSize: 100,
        sumIncomeCents: 0,
        sumExpenseCents: 0,
        sumNetCents: 0,
        uncategorizedCount: 0,
      });
    }
    if (path === '/recurring/materialize') {
      return Promise.resolve({
        year: 2026,
        month: 3,
        monthName: 'März',
        created: 2,
        skipped: 0,
        drafts: 0,
        dryRun: false,
        items: [],
      });
    }
    return Promise.resolve(null);
  });
});

/**
 * Both views are in the DOM at once and CSS picks, so a template's checkbox exists
 * twice. These tests speak to the table's copy; `.screen-cards` is asserted on its
 * own further down.
 */
function tick(container: HTMLElement, name: string) {
  const table = container.querySelector('.screen-table') as HTMLElement;
  return within(table).getByLabelText(name);
}

describe('the monthly checklist', () => {
  /**
   * The day-to-day win in one assertion: everything due and not yet booked is
   * already ticked, so the common case is open-the-page-and-tap. If a future
   * change makes the user tick eighteen boxes by hand, this breaks.
   */
  it('preselects exactly what is due and not already booked', async () => {
    const { container } = renderPage();
    await screen.findAllByLabelText('Miete');

    expect(tick(container, 'Miete')).toBeChecked();
    expect(tick(container, 'Spotify')).toBeChecked();
    // Already booked: visible, but neither ticked nor tickable — pressing the
    // button again must not look like it would double-book.
    expect(tick(container, 'Strom')).not.toBeChecked();
    expect(tick(container, 'Strom')).toBeDisabled();
    // Not due this month.
    expect(tick(container, 'Versicherung')).not.toBeChecked();
    expect(tick(container, 'Versicherung')).toBeDisabled();

    expect(screen.getByRole('button', { name: /Alle buchen/ })).toBeEnabled();
  });

  it('books exactly the ticked templates and nothing else', async () => {
    const user = userEvent.setup();
    const { container } = renderPage();
    await screen.findAllByLabelText('Miete');

    await user.click(tick(container, 'Spotify'));
    // Unticking one changes the button from "alle" to a count, so the user can see
    // that this is no longer the whole list.
    const button = await screen.findByRole('button', { name: /1 buchen/ });
    await user.click(button);

    await waitFor(() => {
      expect(api).toHaveBeenCalledWith(
        '/recurring/materialize',
        expect.objectContaining({ method: 'POST' }),
      );
    });
    const call = api.mock.calls.find((c) => c[0] === '/recurring/materialize');
    const body = JSON.parse((call?.[1] as { body: string }).body);
    expect(body).toEqual({ year: 2026, month: 3, templateIds: ['miete'] });
  });

  it('says what happened, including how many became drafts', async () => {
    const user = userEvent.setup();
    api.mockImplementation((path: string) => {
      if (path.startsWith('/recurring?')) return Promise.resolve(TEMPLATES);
      if (path === '/categories') return Promise.resolve([]);
      if (path === '/recurring/materialize') {
        return Promise.resolve({
          year: 2026,
          month: 3,
          monthName: 'März',
          created: 2,
          skipped: 1,
          drafts: 1,
          dryRun: false,
          items: [],
        });
      }
      return Promise.resolve({
        items: [],
        total: 0,
        page: 0,
        pageSize: 100,
        sumIncomeCents: 0,
        sumExpenseCents: 0,
        sumNetCents: 0,
        uncategorizedCount: 0,
      });
    });
    renderPage();
    await screen.findAllByLabelText('Miete');
    await user.click(screen.getByRole('button', { name: /Alle buchen/ }));

    // A draft counts towards nothing until confirmed, so silence about it would be
    // a figure quietly missing from the month.
    expect(await screen.findByText(/2 gebucht · 1 übersprungen/)).toBeInTheDocument();
    expect(screen.getByText(/davon 1 als Entwurf/)).toBeInTheDocument();
  });
});

describe('the template list', () => {
  /**
   * Leaving the category to the rule table is the recommended way to write a
   * template — a rule change then still reaches future bookings — and the list
   * used to render that as an empty cell, which reads as "uncategorised" and
   * argues for setting an override that nobody needs.
   */
  it('shows the category a rule-driven template will book into, and says it is the rule', async () => {
    api.mockImplementation((path: string) =>
      Promise.resolve(
        String(path).startsWith('/recurring')
          ? [
              template({
                id: 'sport',
                name: 'Sport',
                comment: 'Mafit',
                categoryId: null,
                categoryName: 'Sport',
                categoryType: 'Fixkosten',
                categoryFromRule: true,
              }),
            ]
          : ({ items: [], total: 0 } as never),
      ),
    );
    const { container } = renderPage();
    await screen.findAllByText('Sport');

    const row = [...container.querySelectorAll('tbody tr')].find((r) =>
      r.textContent?.includes('Sport'),
    ) as HTMLElement;
    expect(row).toBeTruthy();
    // The chip carries the category the rule resolves, not an empty cell...
    expect(within(row).getAllByText('Sport').length).toBeGreaterThan(0);
    // ...and it says where that came from, because a rule change will move it.
    expect(row.textContent).toContain('über Regel');
  });

  /**
   * Seven columns do not fit a phone, and this screen's whole point is ticking
   * boxes — so the card view carries the checkbox itself rather than showing a
   * read-only summary you would have to rotate the phone to act on.
   */
  it('offers the same ticking on a phone as in the table', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Miete');

    const cards = container.querySelector('.screen-cards') as HTMLElement;
    expect(cards).toBeTruthy();
    // Every template is present in both views; CSS picks which one is seen.
    expect(within(cards).getByLabelText('Miete')).toBeChecked();
    // Already booked stays visible and untickable here too.
    expect(within(cards).getByLabelText('Strom')).toBeDisabled();
  });
});
