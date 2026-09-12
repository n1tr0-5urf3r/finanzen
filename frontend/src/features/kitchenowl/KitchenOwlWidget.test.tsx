import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { KoSummary } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { KitchenOwlWidget } = await import('./KitchenOwlWidget');

afterEach(cleanup);

const SUMMARY: KoSummary = {
  configured: true,
  enabled: true,
  householdName: 'Beispielhaushalt',
  members: [],
  myBalanceCents: -14917,
  recent: [
    {
      id: 'e1',
      externalId: 450,
      name: 'Supermarkt',
      description: null,
      date: '2026-09-07',
      amountCents: 4494,
      ownShareCents: 2247,
      paidById: 2,
      paidByName: 'Ada',
      paidFor: [],
      koCategoryId: null,
      koCategoryName: null,
      excludeFromStatistics: false,
      archivedAt: null,
      linkedBookingId: null,
      linkedBookingComment: null,
      linkedBookingAmountCents: null,
      updatedAt: '2026-09-07T10:00:00Z',
    },
  ],
  month: { year: 2026, month: 9 },
  monthAmountCents: 8100,
  monthOwnShareCents: 4050,
  monthCount: 3,
  lastSyncedAt: '2026-09-12T10:00:00Z',
  stale: false,
  warning: null,
};

function mount() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter>
          <KitchenOwlWidget />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  api.mockReset();
  api.mockImplementation(() => Promise.resolve(SUMMARY));
});

describe('the dashboard KitchenOwl tile', () => {
  it('labels itself as a separate ledger and shows both month figures', async () => {
    mount();
    expect(await screen.findByText('Getrennt von deinen Buchungen')).toBeInTheDocument();
    expect(screen.getByText(/81,00/).getAttribute('aria-label')).toContain(
      'Gesamtbetrag des Haushalts',
    );
    expect(screen.getByText(/40,50/).getAttribute('aria-label')).toContain('mein Anteil');
    // Their sum would be 121,50 and means nothing.
    expect(screen.queryByText(/121,50/)).toBeNull();
  });

  it('shows the household balance with the direction spelled out', async () => {
    mount();
    expect(await screen.findByText(/149,17/)).toBeInTheDocument();
    expect(screen.getByText('Du schuldest dem Haushalt')).toBeInTheDocument();
  });

  it('degrades visibly rather than disappearing when KitchenOwl is unreachable', async () => {
    api.mockImplementation(() =>
      Promise.resolve({
        ...SUMMARY,
        stale: true,
        warning: 'KitchenOwl /api/household: keine Verbindung',
      } satisfies KoSummary),
    );
    mount();
    expect(await screen.findByText(/keine Verbindung/)).toBeInTheDocument();
    // The last known figures stay; a widget that empties itself looks like the
    // household spent nothing.
    expect(screen.getByText(/81,00/)).toBeInTheDocument();
  });

  it('renders nothing at all when the server has no KitchenOwl configured', async () => {
    api.mockImplementation(() =>
      Promise.resolve({ ...SUMMARY, configured: false } satisfies KoSummary),
    );
    const { container } = mount();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(container.querySelector('.ko-widget')).toBeNull();
  });
});
