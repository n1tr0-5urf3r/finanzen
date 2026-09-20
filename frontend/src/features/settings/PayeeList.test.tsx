import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { Category, StatementPayee } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { PayeeList } = await import('./PayeeList');

const PAYEES: StatementPayee[] = [
  {
    id: 'p1',
    payee: 'Studierendenwerk Musterstadt-Beispielheim Anstalt des offentlichen Rechts',
    comment: 'Mensaguthaben',
    categoryId: 'mensa',
    categoryName: 'Mensa',
    hits: 3,
    updatedAt: '2026-09-20T10:00:00Z',
  },
];

const CATEGORIES = [
  { id: 'mensa', name: 'Mensa', typeLabel: 'Variable Kosten' },
  { id: 'lebensmittel', name: 'Lebensmittel', typeLabel: 'Variable Kosten' },
] as unknown as Category[];

function renderList() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <PayeeList />
      </I18nProvider>
    </QueryClientProvider>,
  );
}

afterEach(cleanup);

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) =>
    Promise.resolve(
      (String(path).startsWith('/categories') ? CATEGORIES : PAYEES) as never,
    ),
  );
});

describe('the remembered payees', () => {
  /**
   * The bank's spelling is the key a future statement matches on, so it is shown
   * as well as the name that replaces it — otherwise the list is a set of names
   * with nothing to tie them to.
   */
  it('shows the bank spelling beside the name it was given', async () => {
    renderList();
    await screen.findByText(/Studierendenwerk Musterstadt/);

    const field = screen.getByLabelText('Heißt bei dir') as HTMLInputElement;
    expect(field.value).toBe('Mensaguthaben');
    expect(screen.getByText(/3× angewendet/)).toBeTruthy();
  });

  it('corrects a name without waiting for the next statement', async () => {
    const user = userEvent.setup();
    renderList();
    await screen.findByText(/Studierendenwerk Musterstadt/);

    const field = screen.getByLabelText('Heißt bei dir');
    await user.clear(field);
    await user.type(field, 'Mensa aufladen');
    await user.tab();

    await waitFor(() => {
      const patch = api.mock.calls.find(([, init]) => (init as RequestInit)?.method === 'PATCH');
      expect(JSON.parse(String((patch![1] as RequestInit).body))).toEqual({
        comment: 'Mensa aufladen',
      });
    });
  });

  /** Forgetting is not destructive: the next statement simply asks again. */
  it('forgets one on request', async () => {
    const user = userEvent.setup();
    renderList();
    await screen.findByText(/Studierendenwerk Musterstadt/);

    await user.click(screen.getByRole('button', { name: /Vergessen/ }));

    await waitFor(() => {
      const del = api.mock.calls.find(([, init]) => (init as RequestInit)?.method === 'DELETE');
      expect(String(del![0])).toBe('/statement-payees/p1');
    });
    // ...and it says so, naming what it forgot.
    expect(await screen.findByText(/„Mensaguthaben“ vergessen/)).toBeTruthy();
  });

  /** "No category" has to be said out loud: an omitted field is null too. */
  it('clears a category explicitly rather than by omission', async () => {
    const user = userEvent.setup();
    renderList();
    await screen.findByText(/Studierendenwerk Musterstadt/);

    await user.selectOptions(screen.getByLabelText('Kategorie'), '');

    await waitFor(() => {
      const patch = api.mock.calls.find(([, init]) => (init as RequestInit)?.method === 'PATCH');
      expect(JSON.parse(String((patch![1] as RequestInit).body))).toEqual({ clearCategory: true });
    });
  });
});
