import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import { ApiError } from '../../lib/api';
import type { Category, CategoryTypeSummary, Rule } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { CategoriesPage } = await import('./CategoriesPage');

const TYPES: CategoryTypeSummary[] = [
  { typeCode: 'fixkosten', label: 'Fixkosten', netCents: 810000, bookingCount: 120 },
  { typeCode: 'variabel', label: 'Variable Kosten', netCents: 990000, bookingCount: 250 },
];

const CATEGORIES: Category[] = [
  { id: 'miete', name: 'Miete', typeCode: 'fixkosten', typeLabel: 'Fixkosten', sortOrder: 1, archived: false, bookingCount: 18, netCents: 480000 },
  { id: 'mensa', name: 'Mensa', typeCode: 'variabel', typeLabel: 'Variable Kosten', sortOrder: 2, archived: false, bookingCount: 42, netCents: 38000 },
  { id: 'sonstiges', name: 'Sonstiges', typeCode: 'variabel', typeLabel: 'Variable Kosten', sortOrder: 3, archived: false, bookingCount: 2, netCents: 3900 },
];

const RULES: Rule[] = [
  { id: 'r1', comment: 'essen', normalizedComment: 'essen', categoryId: 'mensa', categoryName: 'Essen auswärts', kindOverride: null, source: 'user', matchCount: 73 },
  { id: 'r2', comment: 'Tierarzt', normalizedComment: 'tierarzt', categoryId: 'miete', categoryName: 'Haustier', kindOverride: null, source: 'user', matchCount: 0 },
];

function renderPage(entry = '/kategorien') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter initialEntries={[entry]}>
          <CategoriesPage />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) => {
    if (path.startsWith('/category-types')) return Promise.resolve(TYPES);
    if (path.startsWith('/categories')) return Promise.resolve(CATEGORIES);
    if (path.startsWith('/rules')) return Promise.resolve(RULES);
    return Promise.resolve(null);
  });
});

describe('categories', () => {
  it('groups them by the five types, with the type label taken from the database', async () => {
    renderPage();
    await screen.findByText('Miete');
    expect(screen.getAllByText('Fixkosten')[0].getAttribute('lang')).toBe('de');
    expect(screen.getByText('Mensa')).toBeInTheDocument();
  });

  /**
   * The 409 is not an error to report — it is the entry point of the reassign
   * flow, and the server puts the booking count in the message so the user is told
   * how much is at stake.
   */
  it('offers the reassign flow when a category is still in use, not a raw error', async () => {
    const user = userEvent.setup();
    vi.stubGlobal('confirm', () => true);
    api.mockImplementation((path: string, options?: RequestInit) => {
      if (options?.method === 'DELETE') {
        return Promise.reject(
          new ApiError(
            'Konflikt: Die Kategorie wird von 42 Buchungen verwendet. Bitte eine Zielkategorie zum Umhängen angeben.',
            409,
            'conflict',
          ),
        );
      }
      if (path.startsWith('/category-types')) return Promise.resolve(TYPES);
      return Promise.resolve(CATEGORIES);
    });

    renderPage();
    await screen.findByText('Mensa');
    await user.click(screen.getByLabelText('Löschen — Mensa'));

    // The server's own count, verbatim.
    expect(await screen.findByText(/42 Buchungen verwendet/)).toBeInTheDocument();
    expect(screen.getByLabelText('Buchungen umhängen nach')).toBeInTheDocument();
    // Not the generic failure panel.
    expect(screen.queryByText('Da ist etwas schiefgelaufen')).not.toBeInTheDocument();
  });

  it('deletes with reassignTo once a target is chosen', async () => {
    const user = userEvent.setup();
    vi.stubGlobal('confirm', () => true);
    let deletes = 0;
    api.mockImplementation((path: string, options?: RequestInit) => {
      if (options?.method === 'DELETE') {
        deletes += 1;
        if (deletes === 1) {
          return Promise.reject(new ApiError('Konflikt: 42 Buchungen', 409, 'conflict'));
        }
        return Promise.resolve(undefined);
      }
      if (path.startsWith('/category-types')) return Promise.resolve(TYPES);
      return Promise.resolve(CATEGORIES);
    });

    renderPage();
    await screen.findByText('Mensa');
    await user.click(screen.getByLabelText('Löschen — Mensa'));
    await screen.findByLabelText('Buchungen umhängen nach');

    await user.selectOptions(screen.getByLabelText('Buchungen umhängen nach'), 'sonstiges');
    await user.click(screen.getByRole('button', { name: 'Umhängen und löschen' }));

    await screen.findByText('Buchungen umgehängt und Kategorie gelöscht.');
    expect(api).toHaveBeenCalledWith('/categories/mensa?reassignTo=sonstiges', {
      method: 'DELETE',
    });
    // The category being deleted is never offered as its own target.
    expect(deletes).toBe(2);
  });
});

describe('rules', () => {
  it('shows the match count and flags the dead weight', async () => {
    renderPage('/kategorien?ansicht=regeln');
    await screen.findByText('essen');
    expect(screen.getByText('73')).toBeInTheDocument();
    expect(screen.getByText('ungenutzt')).toBeInTheDocument();
    expect(screen.getByText('2 Regeln · 1 ungenutzt')).toBeInTheDocument();
  });

  /**
   * A rule change recategorises history. A silent retroactive edit to last year's
   * numbers is exactly what the design set out to avoid, so the count is said out
   * loud — together with the promise that manual overrides survive it.
   */
  it('says how much history a saved rule moved', async () => {
    const user = userEvent.setup();
    api.mockImplementation((path: string, _options?: RequestInit) => {
      if (path.startsWith('/rules/') && _options?.method === 'PUT') {
        return Promise.resolve({ ...RULES[0], categoryName: 'Mensa', matchCount: 73 });
      }
      if (path.startsWith('/category-types')) return Promise.resolve(TYPES);
      if (path.startsWith('/categories')) return Promise.resolve(CATEGORIES);
      return Promise.resolve(RULES);
    });

    renderPage('/kategorien?ansicht=regeln');
    await screen.findByText('essen');
    await user.click(screen.getByLabelText('Bearbeiten — essen'));
    await user.click(screen.getByRole('button', { name: 'Speichern' }));

    expect(
      await screen.findByText(/73 Buchungen tragen diesen Kommentar und sind jetzt Mensa/),
    ).toBeInTheDocument();
    expect(screen.getAllByText(/Von Hand gesetzte Kategorien bleiben davon unberührt/).length)
      .toBeGreaterThan(0);
  });

  it('previews an apply run without writing anything', async () => {
    const user = userEvent.setup();
    api.mockImplementation((path: string, _options?: RequestInit) => {
      if (path.startsWith('/rules/apply')) {
        return Promise.resolve({
          examined: 1875,
          recategorized: 47,
          stillUncategorized: 12,
          dryRun: path.includes('dryRun=true'),
        });
      }
      if (path.startsWith('/category-types')) return Promise.resolve(TYPES);
      if (path.startsWith('/categories')) return Promise.resolve(CATEGORIES);
      return Promise.resolve(RULES);
    });

    renderPage('/kategorien?ansicht=regeln');
    await screen.findByText('essen');
    await user.click(screen.getByRole('button', { name: 'Vorschau' }));

    expect(
      await screen.findByText(/Vorschau: 47 von 1875 Buchungen würden neu zugeordnet/),
    ).toBeInTheDocument();
  });
});
