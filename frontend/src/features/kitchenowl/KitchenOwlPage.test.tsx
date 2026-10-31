import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { KoDraftPage, KoExpensePage, KoStatus } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { KitchenOwlPage } = await import('./KitchenOwlPage');

// vitest runs without `globals`, so testing-library's automatic cleanup is never
// registered and a second render in this file would stack on the first one's DOM.
afterEach(cleanup);

const STATUS: KoStatus = {
  configured: true,
  enabled: true,
  reachable: true,
  running: false,
  householdId: 1,
  householdName: 'Beispielhaushalt',
  lastExpenseRun: {
    id: 'r1',
    kind: 'ko_expenses',
    status: 'success',
    startedAt: '2026-09-12T10:00:00Z',
    finishedAt: '2026-09-12T10:00:04Z',
    createdCount: 3,
    updatedCount: 0,
    archivedCount: 0,
    failedCount: 0,
    error: null,
  },
  lastMetadataRun: null,
  nextRunAt: '2026-09-12T10:15:00Z',
  pollSeconds: 900,
  mirroredCount: 3,
  archivedCount: 0,
  linkedCount: 1,
  openDraftCount: 1,
  likelyDuplicateCount: 1,
  pendingPushCount: 0,
  failedPushCount: 0,
  metadataFetchedAt: '2026-09-12T10:00:00Z',
  metadataStale: false,
  lastError: null,
};

/** 19,07 € shared evenly: the household spent 19,07 and the user owes 9,54. */
const EXPENSES: KoExpensePage = {
  items: [
    {
      id: 'e1',
      externalId: 450,
      name: 'Supermarkt',
      description: null,
      date: '2026-08-21',
      amountCents: 1907,
      ownShareCents: 954,
      paidById: 2,
      paidByName: 'Ada',
      paidFor: [
        { memberId: 1, name: 'Fabi', factor: 1, shareCents: 954 },
        { memberId: 2, name: 'Ada', factor: 1, shareCents: 953 },
      ],
      koCategoryId: 1,
      koCategoryName: 'Wocheneinkauf',
      excludeFromStatistics: false,
      archivedAt: null,
      linkedBookingId: 'b1',
      linkedBookingComment: 'Kaufland',
      linkedBookingAmountCents: 1907,
      updatedAt: '2026-08-21T10:00:00Z',
    },
    {
      // 73 of 211 expenses in the real corpus have no KitchenOwl category, so the
      // absent state is the common one and must render as a real label.
      id: 'e2',
      externalId: 456,
      name: 'Kiosk',
      description: null,
      date: '2026-08-30',
      amountCents: 790,
      ownShareCents: 790,
      paidById: 1,
      paidByName: 'Fabi',
      paidFor: [{ memberId: 1, name: 'Fabi', factor: 1, shareCents: 790 }],
      koCategoryId: null,
      koCategoryName: null,
      excludeFromStatistics: false,
      archivedAt: null,
      linkedBookingId: null,
      linkedBookingComment: null,
      linkedBookingAmountCents: null,
      updatedAt: '2026-08-30T10:00:00Z',
    },
  ],
  total: 2,
  page: 0,
  pageSize: 100,
  sumAmountCents: 2697,
  sumOwnShareCents: 1744,
  linkedCount: 1,
};

const DRAFTS: KoDraftPage = {
  items: [
    {
      id: 'd1',
      status: 'likely_duplicate',
      expense: EXPENSES.items[1],
      candidates: [
        {
          bookingId: 'b9',
          comment: 'Kiosk Bahnhof',
          amountCents: 790,
          year: 2026,
          month: 8,
          monthName: 'August',
          score: 0.87,
          basis: 'fullAmount',
        },
      ],
      suggestedAction: 'link',
      createdAt: '2026-08-30T10:00:00Z',
    },
  ],
  total: 1,
  openCount: 0,
  likelyCount: 1,
};

function route(path = '/kitchenowl') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter initialEntries={[path]}>
          <KitchenOwlPage />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

/**
 * The ledger renders twice — the wide table and the phone cards are both in the
 * DOM and CSS picks one — so a bare `getByText` finds two of everything. Every
 * other dual-view screen's tests scope to `.screen-table` the same way.
 */
function tableRow(container: HTMLElement, text: string) {
  const row = [...container.querySelectorAll('.screen-table tbody tr')].find((r) =>
    r.textContent?.includes(text),
  );
  if (!row) throw new Error(`no table row containing ${text}`);
  return row as HTMLElement;
}

function inTable(container: HTMLElement) {
  return within(container.querySelector('.screen-table') as HTMLElement);
}

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) => {
    if (path === '/kitchenowl/status') return Promise.resolve(STATUS);
    if (path.startsWith('/kitchenowl/expenses')) return Promise.resolve(EXPENSES);
    if (path.startsWith('/kitchenowl/drafts')) return Promise.resolve(DRAFTS);
    if (path === '/kitchenowl/push') return Promise.resolve([]);
    return Promise.resolve(null);
  });
});

describe('the KitchenOwl ledger', () => {
  it('shows the household amount and the own share as two labelled figures', async () => {
    const { container } = route();
    await screen.findAllByText('Supermarkt');
    const row = tableRow(container, 'Supermarkt');

    // Both numbers, in their own columns, each carrying its basis in the
    // accessible name. This pair is the most confusable in the feature.
    const total = within(row).getByText(/19,07/);
    const share = within(row).getByText(/9,54/);
    expect(total.getAttribute('aria-label')).toContain('Gesamtbetrag des Haushalts');
    expect(share.getAttribute('aria-label')).toContain('mein Anteil');

    // And nowhere is their sum — 28,61 — rendered as if it meant anything.
    expect(screen.queryByText(/28,61/)).toBeNull();
  });

  it('never sums the two ledgers in the footer either', async () => {
    const { container } = route();
    await screen.findAllByText('Supermarkt');
    // The footer totals exist only in the table; the cards have no footer.
    expect(inTable(container).getByText(/26,97/).getAttribute('aria-label')).toContain(
      'Gesamtbetrag des Haushalts',
    );
    expect(inTable(container).getByText(/17,44/).getAttribute('aria-label')).toContain(
      'mein Anteil',
    );
  });

  it('renders the missing KitchenOwl category as a real state, not an empty cell', async () => {
    const { container } = route();
    await screen.findAllByText('Kiosk');
    const row = tableRow(container, 'Kiosk');
    expect(within(row).getByText('ohne KitchenOwl-Kategorie')).toBeInTheDocument();
  });

  it('does not style a KitchenOwl category like one of the app categories', async () => {
    const { container } = route();
    await screen.findAllByText('Wocheneinkauf');
    const chip = inTable(container).getByText('Wocheneinkauf');
    const box = chip.closest('span')!.parentElement!;
    // Two unrelated taxonomies. Looking alike would teach that they map onto each
    // other, and nothing in this feature ever maps them.
    expect(box.className).toContain('ko-chip');
    expect(box.className).not.toContain('category-chip');
  });

  it('states that an unlinked expense is normal rather than flagging it', async () => {
    const { container } = route();
    await screen.findAllByText('Kiosk');
    const row = tableRow(container, 'Kiosk');
    const state = within(row).getByText('Ohne Verknüpfung');
    expect(state.className).toContain('ko-link--none');
    // Not the uncategorised/warning treatment used for a booking with no category.
    expect(row.className).not.toContain('row--uncategorized');
  });

  it('says outright that the two ledgers are separate, and when it last synced', async () => {
    route();
    await screen.findAllByText('Supermarkt');
    expect(screen.getByText(/werden nie zu deinen Buchungen addiert/)).toBeInTheDocument();
    expect(screen.getByText(/^Zuletzt:/)).toBeInTheDocument();
    expect(screen.getByText(/^Nächste:/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Jetzt synchronisieren/ })).toBeEnabled();
  });

  it('offers a link and never an "create booking" action', async () => {
    route('/kitchenowl?ansicht=review');
    await screen.findByText('Kiosk Bahnhof');
    expect(screen.getByRole('button', { name: 'Verknüpfen' })).toBeInTheDocument();
    expect(screen.getByText(/Gleicher Gesamtbetrag/)).toBeInTheDocument();
    // The copy must not suggest a booking appears; auto-booking is what would
    // double-post most of the groceries.
    expect(screen.getByText(/legt KEINE Buchung an/i)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Buchung anlegen/i })).toBeNull();
  });

  it('degrades visibly and keeps the last known figures when KitchenOwl is down', async () => {
    api.mockImplementation((path: string) => {
      if (path === '/kitchenowl/status') {
        return Promise.resolve({
          ...STATUS,
          reachable: false,
          lastExpenseRun: {
            ...STATUS.lastExpenseRun!,
            status: 'failed',
            error: 'KitchenOwl /api/household: keine Verbindung',
          },
        } satisfies KoStatus);
      }
      if (path.startsWith('/kitchenowl/expenses')) return Promise.resolve(EXPENSES);
      return Promise.resolve(null);
    });
    route();

    expect(await screen.findByText(/nicht erreichbar/)).toBeInTheDocument();
    expect(screen.getByText(/keine Verbindung/)).toBeInTheDocument();
    // The mirror is still readable — it is the last true state, not a guess.
    expect((await screen.findAllByText('Supermarkt')).length).toBeGreaterThan(0);
  });

  it('tells the user a sync is not configured rather than showing an empty page', async () => {
    api.mockImplementation(() =>
      Promise.resolve({ ...STATUS, configured: false, enabled: false } satisfies KoStatus),
    );
    route();
    expect(
      await screen.findByText('KitchenOwl ist auf diesem Server nicht eingerichtet.'),
    ).toBeInTheDocument();
  });

  it('runs a manual sync and reports what it did', async () => {
    const user = userEvent.setup();
    route();
    await screen.findAllByText('Supermarkt');

    api.mockImplementation((path: string, init?: RequestInit) => {
      if (path === '/kitchenowl/sync' && init?.method === 'POST') {
        return Promise.resolve({
          started: true,
          expenses: { ...STATUS.lastExpenseRun!, createdCount: 2, updatedCount: 1 },
          metadata: null,
          error: null,
        });
      }
      if (path === '/kitchenowl/status') return Promise.resolve(STATUS);
      if (path.startsWith('/kitchenowl/expenses')) return Promise.resolve(EXPENSES);
      return Promise.resolve(null);
    });

    await user.click(screen.getByRole('button', { name: /Jetzt synchronisieren/ }));
    expect(await screen.findByText(/2 neu, 1 geändert/)).toBeInTheDocument();
  });

  /**
   * The switcher is the first thing under the header, on this screen as on every
   * other. It used to sit below the sync strip and the settlement card, which on a
   * phone put five tabs below the fold — the complaint that prompted the whole
   * consistency pass.
   */
  it('puts the tab bar directly under the header, above everything else', async () => {
    const { container } = route();
    await screen.findAllByText('Supermarkt');

    const tabs = container.querySelector('[role="tablist"]')!;
    const strip = container.querySelector('.ko-strip')!;
    expect(tabs).toBeTruthy();
    expect(strip).toBeTruthy();
    // Document order: the tabs come first.
    expect(tabs.compareDocumentPosition(strip) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    // Only what is true of every tab may precede them: what this screen is, and
    // whether it is current.
    const beforeTabs = [...container.querySelectorAll('.banner, [role="tablist"]')];
    expect(beforeTabs.length).toBeGreaterThan(1); // the separate-ledger banner is there
    expect(beforeTabs[beforeTabs.length - 1]).toBe(tabs);
  });

  /** The sync strip and the settlement belong to the ledger, not to all five tabs. */
  it('leaves the sync strip and the settlement behind when another tab is open', async () => {
    const { container } = route('/kitchenowl?ansicht=push');
    await screen.findByRole('tablist');
    expect(container.querySelector('.ko-strip')).toBeNull();
    expect(screen.queryByRole('button', { name: /Ausgleich buchen/ })).toBeNull();
  });
});

describe('the address of the old household analysis', () => {
  /**
   * The household's own analysis tab is gone — the main analysis counts my shares
   * of the household's purchases now. A bookmark to it opens the ledger rather
   * than a page with tabs and nothing under them.
   */
  it('opens the ledger for a tab that no longer exists', async () => {
    route('/kitchenowl?ansicht=analysis');
    await screen.findAllByText('Supermarkt');
    expect(screen.getByRole('tab', { name: 'Ausgaben' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    expect(screen.queryByRole('tab', { name: 'Auswertung' })).toBeNull();
  });
});

describe('the category map', () => {
  const MAP = {
    categories: [
      {
        // Mapped already.
        koCategoryId: 1,
        name: 'Wocheneinkauf',
        financesCategoryId: 'food',
        financesCategoryName: 'Lebensmittel',
        suggestedCategoryId: null,
        shareCount: 12,
        ownShareCents: 36000,
      },
      {
        // Unmapped, with a finance category of the same name on offer.
        koCategoryId: 7,
        name: 'Hobbies',
        financesCategoryId: null,
        financesCategoryName: null,
        suggestedCategoryId: 'hobbies',
        shareCount: 2,
        ownShareCents: 1500,
      },
      {
        // Unmapped and nothing to suggest.
        koCategoryId: 3,
        name: 'Haushalt',
        financesCategoryId: null,
        financesCategoryName: null,
        suggestedCategoryId: null,
        shareCount: 4,
        ownShareCents: 2200,
      },
    ],
    untaggedShareCount: 5,
    untaggedOwnShareCents: 3100,
  };
  const CATEGORIES = [
    { id: 'food', name: 'Lebensmittel', archived: false },
    { id: 'hobbies', name: 'Hobbies', archived: false },
    { id: 'old', name: 'Alt', archived: true },
  ];

  beforeEach(() => {
    // The server's state, so a refetch after a save sees the save.
    const state = structuredClone(MAP);
    api.mockImplementation((path: string, init?: RequestInit) => {
      if (path === '/kitchenowl/status') return Promise.resolve(STATUS);
      if (path === '/kitchenowl/category-map') return Promise.resolve(structuredClone(state));
      if (path === '/categories') return Promise.resolve(CATEGORIES);
      if (path.startsWith('/kitchenowl/category-map/') && init?.method === 'PUT') {
        const id = Number(path.split('/').pop());
        const row = state.categories.find((c) => c.koCategoryId === id)!;
        Object.assign(row, JSON.parse(String(init.body)), { suggestedCategoryId: null });
        return Promise.resolve({ ...row });
      }
      return Promise.resolve(null);
    });
  });

  const puts = () =>
    api.mock.calls.filter(([, init]) => (init as RequestInit | undefined)?.method === 'PUT');

  function mapRow(container: HTMLElement, name: string) {
    const row = container.querySelector(`[data-ko-category="${name}"]`);
    if (!row) throw new Error(`no map row for ${name}`);
    return within(row as HTMLElement);
  }

  async function open() {
    const view = route('/kitchenowl?ansicht=categories');
    // The finance categories arrive separately; until then there is nothing to select.
    await screen.findAllByRole('option', { name: 'Lebensmittel' });
    return view;
  }

  it('is one list: the KitchenOwl category, and what it counts as for me', async () => {
    const { container } = await open();
    expect(screen.getByRole('tab', { name: 'Zuordnung' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    expect(screen.getByText('Zählt bei mir als')).toBeInTheDocument();
    const food = mapRow(container, 'Wocheneinkauf');
    expect(food.getByRole('combobox')).toHaveValue('food');
    expect(food.getByText(/12 Anteile/)).toBeInTheDocument();
    // No save step: a mapped row has nothing to press.
    expect(food.queryByRole('button')).toBeNull();
    // An archived category is not on offer for a new mapping.
    expect(
      [...food.getByRole('combobox').querySelectorAll('option')].map((o) => o.textContent),
    ).not.toContain('Alt');
    expect(
      screen.getByText(/5 Anteile stammen aus Ausgaben ohne KitchenOwl-Kategorie/),
    ).toBeInTheDocument();
  });

  it('offers a same-name category as a suggestion, and maps it only when taken', async () => {
    const user = userEvent.setup();
    const { container } = await open();
    const hobbies = mapRow(container, 'Hobbies');
    // Not preselected: the select shows what is actually in effect.
    expect(hobbies.getByRole('combobox')).toHaveValue('');
    expect(puts()).toHaveLength(0);

    await user.click(hobbies.getByRole('button', { name: 'Vorschlag übernehmen: Hobbies' }));

    expect(puts()).toHaveLength(1);
    const [path, init] = puts()[0]!;
    expect(path).toBe('/kitchenowl/category-map/7');
    expect(JSON.parse(String((init as RequestInit).body))).toEqual({
      financesCategoryId: 'hobbies',
    });
    await waitFor(() =>
      expect(mapRow(container, 'Hobbies').getByRole('combobox')).toHaveValue('hobbies'),
    );
    expect(mapRow(container, 'Hobbies').queryByRole('button')).toBeNull();
  });

  it('saves a choice the moment it is made, including "no category"', async () => {
    const user = userEvent.setup();
    const { container } = await open();

    await user.selectOptions(mapRow(container, 'Haushalt').getByRole('combobox'), 'food');
    await waitFor(() => expect(puts()).toHaveLength(1));
    await user.selectOptions(mapRow(container, 'Wocheneinkauf').getByRole('combobox'), '');
    await waitFor(() => expect(puts()).toHaveLength(2));

    expect(puts().map(([path]) => path)).toEqual([
      '/kitchenowl/category-map/3',
      '/kitchenowl/category-map/1',
    ]);
    expect(JSON.parse(String((puts()[0]![1] as RequestInit).body))).toEqual({
      financesCategoryId: 'food',
    });
    expect(JSON.parse(String((puts()[1]![1] as RequestInit).body))).toEqual({
      financesCategoryId: null,
    });
  });
});
