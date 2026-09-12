import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
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

describe('the monthly checklist', () => {
  /**
   * The day-to-day win in one assertion: everything due and not yet booked is
   * already ticked, so the common case is open-the-page-and-tap. If a future
   * change makes the user tick eighteen boxes by hand, this breaks.
   */
  it('preselects exactly what is due and not already booked', async () => {
    renderPage();
    await screen.findByLabelText('Miete');

    expect(screen.getByLabelText('Miete')).toBeChecked();
    expect(screen.getByLabelText('Spotify')).toBeChecked();
    // Already booked: visible, but neither ticked nor tickable — pressing the
    // button again must not look like it would double-book.
    expect(screen.getByLabelText('Strom')).not.toBeChecked();
    expect(screen.getByLabelText('Strom')).toBeDisabled();
    // Not due this month.
    expect(screen.getByLabelText('Versicherung')).not.toBeChecked();
    expect(screen.getByLabelText('Versicherung')).toBeDisabled();

    expect(screen.getByRole('button', { name: /Alle buchen/ })).toBeEnabled();
  });

  it('books exactly the ticked templates and nothing else', async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findByLabelText('Miete');

    await user.click(screen.getByLabelText('Spotify'));
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
    await screen.findByLabelText('Miete');
    await user.click(screen.getByRole('button', { name: /Alle buchen/ }));

    // A draft counts towards nothing until confirmed, so silence about it would be
    // a figure quietly missing from the month.
    expect(await screen.findByText(/2 gebucht · 1 übersprungen/)).toBeInTheDocument();
    expect(screen.getByText(/davon 1 als Entwurf/)).toBeInTheDocument();
  });
});
