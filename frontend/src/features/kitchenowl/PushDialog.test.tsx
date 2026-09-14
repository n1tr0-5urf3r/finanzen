import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { Booking, KoMetadata } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { PushDialog } = await import('./PushDialog');

afterEach(cleanup);

const BOOKING: Booking = {
  id: 'b1',
  year: 2026,
  month: 8,
  monthName: 'August',
  bookedOn: '2026-08-21',
  kind: 'expense',
  amountCents: 1907,
  netCents: 1907,
  comment: 'Kaufland',
  taxRelevant: false,
  categoryId: 'c1',
  categoryName: 'Lebensmittel',
  categoryType: 'Variable Kosten',
  categorySource: 'rule',
  shared: false,
  externalSource: null,
  externalId: null,
  hasReceipt: false,
  status: 'confirmed',
  origin: 'manual',
};

const METADATA: KoMetadata = {
  members: [
    {
      memberId: 1,
      name: 'Fabi',
      username: 'fabi',
      balanceCents: -14917,
      isMe: true,
      isOwner: true,
      isAdmin: false,
      fetchedAt: '2026-09-12T10:00:00Z',
    },
    {
      memberId: 2,
      name: 'Ada',
      username: 'ada',
      balanceCents: 14917,
      isMe: false,
      isOwner: false,
      isAdmin: true,
      fetchedAt: '2026-09-12T10:00:00Z',
    },
  ],
  categories: [
    { categoryId: 1, name: 'Wocheneinkauf', colorArgb: null, budgetCents: null, fetchedAt: 'x' },
    { categoryId: 2, name: 'Essen gehen', colorArgb: null, budgetCents: null, fetchedAt: 'x' },
  ],
  fetchedAt: '2026-09-12T10:00:00Z',
  stale: false,
  warning: null,
};

function open(onClose = vi.fn()) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <PushDialog booking={BOOKING} onClose={onClose} />
      </I18nProvider>
    </QueryClientProvider>,
  );
  return onClose;
}

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) => {
    if (path === '/kitchenowl/metadata') return Promise.resolve(METADATA);
    return Promise.resolve(null);
  });
});

describe('the push dialogue', () => {
  it('mirrors KitchenOwl’s own add-expense form', async () => {
    open();
    expect(await screen.findByLabelText('Name')).toHaveValue('Kaufland');
    expect(screen.getByLabelText('Betrag')).toHaveValue('19,07');
    expect(screen.getByLabelText('Datum')).toHaveValue('2026-08-21');
    expect(screen.getByLabelText('KitchenOwl-Kategorie')).toBeInTheDocument();
    expect(screen.getByLabelText('Bezahlt von')).toBeInTheDocument();
    // Integer weights, one field per member — not a percentage anywhere.
    expect(screen.getByLabelText('Anteile Fabi')).toHaveValue(1);
    expect(screen.getByLabelText('Anteile Ada')).toHaveValue(1);
  });

  it('does not preselect a KitchenOwl category from the booking’s own category', async () => {
    open();
    const select = (await screen.findByLabelText('KitchenOwl-Kategorie')) as HTMLSelectElement;
    // The booking is "Lebensmittel"; KitchenOwl has "Wocheneinkauf". They are
    // different taxonomies and nothing maps one onto the other.
    expect(select.value).toBe('');
    expect(screen.getByRole('option', { name: 'ohne KitchenOwl-Kategorie' })).toBeInTheDocument();
    expect(screen.getByText(/nie automatisch zugeordnet/)).toBeInTheDocument();
  });

  it('sends the full booking amount and the chosen integer weights', async () => {
    const user = userEvent.setup();
    const onClose = open();
    await screen.findByLabelText('Name');

    // Captured rather than asserted inside the mock: an assertion that throws in
    // there is swallowed by the mutation and the test passes for the wrong reason.
    let sent: Record<string, unknown> | null = null;
    api.mockImplementation((path: string, init?: RequestInit) => {
      if (path === '/bookings/b1/kitchenowl') {
        sent = JSON.parse(String(init?.body));
        return Promise.resolve({ state: 'queued' });
      }
      // The dialogue waits for the attempt rather than closing on the 202, so the
      // queue has to answer — this is the second half of what the user sees.
      if (String(path) === '/kitchenowl/push') {
        return Promise.resolve([
          { bookingId: 'b1', state: 'pushed', date: '2026-09-01', externalId: 467 },
        ]);
      }
      return Promise.resolve(METADATA);
    });

    await user.selectOptions(screen.getByLabelText('KitchenOwl-Kategorie'), '1');
    await user.clear(screen.getByLabelText('Anteile Fabi'));
    await user.type(screen.getByLabelText('Anteile Fabi'), '3');
    await user.click(screen.getByRole('button', { name: 'Übertragen' }));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    // The FULL amount, not the user's half. Sending the share would halve the
    // household's record of what was actually spent.
    expect(sent).toEqual({
      name: 'Kaufland',
      description: null,
      amountCents: 1907,
      date: '2026-08-21',
      koCategoryId: 1,
      paidById: 1,
      paidFor: [
        { memberId: 1, factor: 3 },
        { memberId: 2, factor: 1 },
      ],
    });
    // Not "queued": the dialogue waits for the attempt and reports the outcome,
    // including the DATE it was filed under — KitchenOwl sorts by date, and a
    // booking dated three weeks ago lands three weeks down the list, which once
    // read as a failed push.
    expect(onClose).toHaveBeenCalledWith(expect.stringContaining('01.09.2026'));
  });

  it('lets a weight be cleared and retyped', async () => {
    // Coercing each keystroke to a number makes the field impossible to clear:
    // emptying it snaps back to 1, so typing "3" over it produces 13. The weight
    // is held as text and only clamped on blur.
    const user = userEvent.setup();
    open();
    const field = await screen.findByLabelText('Anteile Fabi');
    await user.clear(field);
    await user.type(field, '3');
    expect(field).toHaveValue(3);
  });

  it('keeps a weight when somebody is unticked and ticked again', async () => {
    const user = userEvent.setup();
    open();
    const field = await screen.findByLabelText('Anteile Ada');
    await user.clear(field);
    await user.type(field, '7');
    const boxes = screen.getAllByRole('checkbox');
    await user.click(boxes[1]);
    await user.click(boxes[1]);
    expect(screen.getByLabelText('Anteile Ada')).toHaveValue(7);
  });

  it('opens with a warning instead of refusing when the metadata is stale', async () => {
    api.mockImplementation(() =>
      Promise.resolve({ ...METADATA, stale: true, warning: null } satisfies KoMetadata),
    );
    open();
    // The whole point of the outbox is that a push can be queued during an outage,
    // so a dialogue that will not open would defeat it.
    expect(await screen.findByText(/möglicherweise veraltet/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Übertragen' })).toBeEnabled();
  });

  it('says the marker is what makes a retry safe', async () => {
    open();
    expect(await screen.findByText(/Kennung angehängt/)).toBeInTheDocument();
  });

  /**
   * The three ways a push ends, each said in the words the user needs.
   */
  it('reports a refusal instead of claiming it was queued', async () => {
    const user = userEvent.setup();
    const onClose = open();
    await screen.findByLabelText('Name');

    api.mockImplementation((path: string) => {
      if (String(path) === '/bookings/b1/kitchenowl') return Promise.resolve({ state: 'queued' });
      if (String(path) === '/kitchenowl/push') {
        return Promise.resolve([
          { bookingId: 'b1', state: 'failed', lastError: 'Request invalid', externalId: null },
        ]);
      }
      return Promise.resolve(METADATA);
    });

    await user.click(screen.getByRole('button', { name: 'Übertragen' }));
    await waitFor(() => expect(onClose).toHaveBeenCalled(), { timeout: 4000 });
    expect(onClose).toHaveBeenCalledWith(expect.stringContaining('Request invalid'));
  });

  it('says it is still queued rather than waiting for ever', async () => {
    const user = userEvent.setup();
    const onClose = open();
    await screen.findByLabelText('Name');

    // KitchenOwl never answers: the outbox is what makes that safe, so the
    // dialogue stops watching and says so instead of spinning.
    api.mockImplementation((path: string) => {
      if (String(path) === '/bookings/b1/kitchenowl') return Promise.resolve({ state: 'queued' });
      if (String(path) === '/kitchenowl/push') {
        return Promise.resolve([{ bookingId: 'b1', state: 'queued', externalId: null }]);
      }
      return Promise.resolve(METADATA);
    });

    await user.click(screen.getByRole('button', { name: 'Übertragen' }));
    await waitFor(() => expect(onClose).toHaveBeenCalled(), { timeout: 15000 });
    expect(onClose).toHaveBeenCalledWith(expect.stringContaining('Vorgemerkt'));
  }, 20000);
});
