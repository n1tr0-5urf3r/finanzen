import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { KoTagging } = await import('./KoTagging');

const METADATA = {
  categories: [
    { categoryId: 1, name: 'Wocheneinkauf', colorArgb: null, budgetCents: null },
    { categoryId: 3, name: 'Haushalt', colorArgb: null, budgetCents: null },
    { categoryId: 7, name: 'Hobbies', colorArgb: null, budgetCents: null },
  ],
  members: [],
  stale: false,
  warning: null,
};

const GROUPS = [
  {
    name: 'Kaufland',
    matchKey: 'kaufland',
    expenseCount: 16,
    amountCents: 44654,
    ownShareCents: 22327,
    firstDate: '2026-01-10',
    lastDate: '2026-05-10',
    suggestion: {
      koCategoryId: 1,
      koCategoryName: 'Wocheneinkauf',
      source: 'precedent',
      timesSeen: 39,
    },
  },
  {
    name: 'Hornbach',
    matchKey: 'hornbach',
    expenseCount: 5,
    amountCents: 5898,
    ownShareCents: 2949,
    firstDate: '2026-02-01',
    lastDate: '2026-06-20',
    suggestion: {
      koCategoryId: 3,
      koCategoryName: 'Haushalt',
      source: 'override',
      timesSeen: 0,
    },
  },
  {
    name: 'Padefke',
    matchKey: 'padefke',
    expenseCount: 2,
    amountCents: 1924,
    ownShareCents: 962,
    firstDate: '2026-03-03',
    lastDate: '2026-04-04',
    suggestion: null,
  },
];

let applied: unknown[] = [];

beforeEach(() => {
  applied = [];
  api.mockReset();
  api.mockImplementation((path: string, init?: { body?: string }) => {
    if (String(path).includes('/untagged/apply')) {
      applied.push(JSON.parse(init?.body ?? '{}'));
      return Promise.resolve({
        koCategoryId: 1,
        koCategoryName: 'Wocheneinkauf',
        requested: 16,
        tagged: 16,
        skipped: 0,
        failed: 0,
        failures: [],
      } as never);
    }
    if (String(path).includes('/untagged')) return Promise.resolve(GROUPS as never);
    if (String(path).includes('/metadata')) return Promise.resolve(METADATA as never);
    return Promise.resolve([] as never);
  });
});
afterEach(cleanup);

function renderQueue() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter>
          <KoTagging />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

describe('the tagging queue', () => {
  /**
   * One row per NAME, because that is the shape of the decision: sixteen Kaufland
   * receipts are one judgement about Kaufland, not sixteen.
   */
  it('lists names rather than expenses, with both figures kept apart', async () => {
    const { container } = renderQueue();
    await screen.findAllByText('Kaufland');

    const rows = [...container.querySelectorAll('.screen-table tbody tr')];
    expect(rows).toHaveLength(3);

    const kaufland = rows.find((r) => r.textContent?.includes('Kaufland')) as HTMLElement;
    expect(within(kaufland).getByText('16')).toBeInTheDocument();
    expect(within(kaufland).getByText(/446,54/)).toBeInTheDocument();
    expect(within(kaufland).getByText(/223,27/)).toBeInTheDocument();
    // The household amount and the user's share are never summed.
    expect(kaufland.textContent).not.toContain('669,81');
  });

  /**
   * A preselected dropdown with no explanation is how one wrong guess becomes
   * thirty-nine wrong expenses in somebody else's ledger.
   */
  it('preselects the suggestion and says what it is based on', async () => {
    const { container } = renderQueue();
    await screen.findAllByText('Kaufland');
    const rows = [...container.querySelectorAll('.screen-table tbody tr')];

    const kaufland = rows.find((r) => r.textContent?.includes('Kaufland')) as HTMLElement;
    expect((within(kaufland).getByRole('combobox') as HTMLSelectElement).value).toBe('1');
    expect(kaufland.textContent).toContain('39');

    // A standing correction says so in its own words, not as "seen 0 times".
    const hornbach = rows.find((r) => r.textContent?.includes('Hornbach')) as HTMLElement;
    expect((within(hornbach).getByRole('combobox') as HTMLSelectElement).value).toBe('3');
    expect(hornbach.textContent).toContain('festgelegt');

    // No evidence, no preselection: the button stays out of reach until a human
    // picks something.
    const padefke = rows.find((r) => r.textContent?.includes('Padefke')) as HTMLElement;
    expect((within(padefke).getByRole('combobox') as HTMLSelectElement).value).toBe('0');
    expect(within(padefke).getByRole('button')).toBeDisabled();
  });

  it('applies one name at a time, sending the chosen category', async () => {
    const user = userEvent.setup();
    const { container } = renderQueue();
    await screen.findAllByText('Kaufland');

    const rows = [...container.querySelectorAll('.screen-table tbody tr')];
    const kaufland = rows.find((r) => r.textContent?.includes('Kaufland')) as HTMLElement;
    await user.click(within(kaufland).getByRole('button'));

    expect(applied).toEqual([{ name: 'Kaufland', koCategoryId: 1 }]);
    expect(await screen.findByText(/16 Ausgaben/)).toBeInTheDocument();
  });

  /** A different choice overrides the suggestion, and that is what gets sent. */
  it('sends what the user picked, not what was suggested', async () => {
    const user = userEvent.setup();
    const { container } = renderQueue();
    await screen.findAllByText('Kaufland');

    const rows = [...container.querySelectorAll('.screen-table tbody tr')];
    const kaufland = rows.find((r) => r.textContent?.includes('Kaufland')) as HTMLElement;
    await user.selectOptions(within(kaufland).getByRole('combobox'), '7');
    await user.click(within(kaufland).getByRole('button'));

    expect(applied).toEqual([{ name: 'Kaufland', koCategoryId: 7 }]);
  });

  /**
   * Failures are named, one per expense. A count alone would leave the user with
   * no way to find the ones that did not land.
   */
  it('names every expense KitchenOwl refused', async () => {
    const user = userEvent.setup();
    api.mockImplementation((path: string) => {
      if (String(path).includes('/untagged/apply')) {
        return Promise.resolve({
          koCategoryId: 1,
          koCategoryName: 'Wocheneinkauf',
          requested: 16,
          tagged: 14,
          skipped: 0,
          failed: 2,
          failures: [
            { externalId: 294, name: 'Kaufland', error: 'KitchenOwl: 400 Request invalid' },
            { externalId: 298, name: 'Kaufland', error: 'KitchenOwl: 400 Request invalid' },
          ],
        } as never);
      }
      if (String(path).includes('/untagged')) return Promise.resolve(GROUPS as never);
      if (String(path).includes('/metadata')) return Promise.resolve(METADATA as never);
      return Promise.resolve([] as never);
    });

    const { container } = renderQueue();
    await screen.findAllByText('Kaufland');
    const rows = [...container.querySelectorAll('.screen-table tbody tr')];
    const kaufland = rows.find((r) => r.textContent?.includes('Kaufland')) as HTMLElement;
    await user.click(within(kaufland).getByRole('button'));

    const failures = await screen.findByRole('list');
    expect(within(failures).getAllByText(/Request invalid/)).toHaveLength(2);
  });

  it('says that this writes to KitchenOwl before anything is pressed', async () => {
    renderQueue();
    expect(await screen.findByText(/in KitchenOwl selbst gesetzt/)).toBeInTheDocument();
  });

  /**
   * The server hands back a few writes at a time, because KitchenOwl needs about
   * two seconds for each and a browser will not wait half a minute for sixteen.
   * The queue asks again until nothing is left — and stops on its own if a round
   * achieves nothing, which is what an earlier version did not do: it span until
   * the test runner ran out of memory.
   */
  it('keeps asking until nothing is left, and gives up on a round that achieves nothing', async () => {
    const user = userEvent.setup();
    const batches = [
      { requested: 16, tagged: 8, skipped: 0, failed: 0, failures: [], remaining: 8,
        koCategoryId: 1, koCategoryName: 'Wocheneinkauf' },
      { requested: 16, tagged: 8, skipped: 0, failed: 0, failures: [], remaining: 0,
        koCategoryId: 1, koCategoryName: 'Wocheneinkauf' },
    ];
    let posts = 0;
    api.mockImplementation((path: string, init?: { method?: string }) => {
      if (String(path).includes('/untagged/apply') || init?.method === 'POST') {
        return Promise.resolve(batches[Math.min(posts++, batches.length - 1)] as never);
      }
      return Promise.resolve(GROUPS as never);
    });

    renderQueue();
    // Rendered twice — the wide table and the phone cards are both in the DOM.
    await screen.findAllByText('Kaufland');
    await user.click(screen.getAllByRole('button', { name: /16/ })[0]);

    // Two rounds: the first reports eight still owed, the second reports none.
    await waitFor(() => expect(posts).toBe(2));
  });

  it('stops after a refusal instead of collecting the same error sixteen times', async () => {
    const user = userEvent.setup();
    let posts = 0;
    api.mockImplementation((path: string, init?: { method?: string }) => {
      if (String(path).includes('/untagged/apply') || init?.method === 'POST') {
        posts += 1;
        return Promise.resolve({
          requested: 16, tagged: 0, skipped: 0, failed: 1, remaining: 15,
          koCategoryId: 1, koCategoryName: 'Wocheneinkauf',
          failures: [{ externalId: 289, name: 'Kaufland', error: 'Request invalid' }],
        } as never);
      }
      return Promise.resolve(GROUPS as never);
    });

    renderQueue();
    await screen.findAllByText('Kaufland');
    await user.click(screen.getAllByRole('button', { name: /16/ })[0]);

    // One round only: a refusal is not worth repeating fifteen times.
    await waitFor(() => expect(posts).toBe(1));
    await new Promise((r) => setTimeout(r, 60));
    expect(posts).toBe(1);
  });
});
