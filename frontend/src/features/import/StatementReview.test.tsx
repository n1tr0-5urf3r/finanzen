import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { Category, StatementRow, StatementRowPage } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { StatementReview } = await import('./StatementReview');

const CATEGORIES = [
  { id: 'lebensmittel', name: 'Lebensmittel', typeLabel: 'Variable Kosten' },
  { id: 'auto', name: 'Auto & Parken', typeLabel: 'Variable Kosten' },
] as unknown as Category[];

function row(over: Partial<StatementRow>): StatementRow {
  return {
    id: 'r1',
    sourceRef: 'csv!zeile:15',
    bookedOn: '2026-09-18',
    kind: 'expense',
    amountCents: 385,
    counterparty: 'VISA SUPERMARKT SAGT DANKE',
    purpose: 'NR XXXX 0000 MUSTERSTADT DE KAUFUMSATZ 07.03 3.85',
    comment: 'Supermarkt Sagt Danke',
    categoryId: 'lebensmittel',
    categoryName: 'Lebensmittel',
    categorySource: 'suggestion',
    suggestionScore: 0.9,
    decision: 'pending',
    createRule: false,
    duplicateBookingId: null,
    duplicateComment: null,
    duplicateBookedOn: null,
    ...over,
  };
}

const PAGE: StatementRowPage = {
  items: [
    row({}),
    row({
      id: 'r2',
      counterparty: 'VISA TANKSTELLE',
      comment: 'Tankstelle',
      amountCents: 5820,
      categoryId: 'auto',
      categoryName: 'Auto & Parken',
      categorySource: 'rule',
      // The ledger already holds this one: entered by hand two days earlier.
      duplicateBookingId: 'b9',
      duplicateComment: 'tanken',
      duplicateBookedOn: '2026-09-16',
    }),
  ],
  total: 2,
  pending: 2,
  accepted: 0,
  rejected: 0,
  duplicates: 1,
};

function renderReview() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter initialEntries={['/import?ansicht=pruefliste']}>
          <StatementReview batchId="batch-1" categories={CATEGORIES} applied={false} />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

afterEach(cleanup);

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((_path: string, init?: RequestInit) => {
    if (init?.method === 'PATCH') return Promise.resolve(row({}) as never);
    if (init?.method === 'POST') return Promise.resolve({ affected: 1 } as never);
    return Promise.resolve(PAGE as never);
  });
});

describe('the bank statement review', () => {
  /**
   * The bank's own words are the only evidence of what a line was, so they are on
   * the card — not behind a tooltip, and not replaced by the app's guess.
   */
  it('shows what the bank wrote beside what the app guessed', async () => {
    renderReview();
    await screen.findByText('VISA SUPERMARKT SAGT DANKE');

    // The suggested comment is in an editable field, not printed as a fact.
    const comment = screen.getAllByLabelText('Kommentar')[0] as HTMLInputElement;
    expect(comment.value).toBe('Supermarkt Sagt Danke');

    // An exact rule and a guess are labelled differently: they must not be
    // confirmed with the same shrug.
    expect(screen.getByText(/geraten/)).toBeTruthy();
    expect(screen.getByText(/aus einer Regel/)).toBeTruthy();

    // The reference text is one click away rather than filling the card.
    expect(screen.queryByText(/KAUFUMSATZ/)).toBeNull();
    await userEvent.click(screen.getAllByRole('button', { name: 'Verwendungszweck' })[0]!);
    expect(screen.getByText(/KAUFUMSATZ/)).toBeTruthy();
  });

  /** A statement overlaps whatever was typed in by hand; that is the normal case. */
  it('flags a line the ledger may already hold, and can set them all aside', async () => {
    const user = userEvent.setup();
    renderReview();
    await screen.findByText(/Gibt es vielleicht schon: tanken/);

    await user.click(screen.getByRole('button', { name: /1 mögliche Doppel aussortieren/ }));

    await waitFor(() => {
      const posts = api.mock.calls.filter(([, init]) => (init as RequestInit)?.method === 'POST');
      expect(posts).toHaveLength(1);
      expect(JSON.parse(String((posts[0]![1] as RequestInit).body))).toEqual({
        scope: 'duplicates',
        decision: 'rejected',
      });
    });
  });

  /** Nothing is booked until a line has been looked at, so accepting is explicit. */
  it('accepts a line with the comment as edited', async () => {
    const user = userEvent.setup();
    renderReview();
    await screen.findByText('VISA SUPERMARKT SAGT DANKE');

    const comment = screen.getAllByLabelText('Kommentar')[0] as HTMLInputElement;
    await user.clear(comment);
    await user.type(comment, 'Supermarkt');
    await user.click(screen.getAllByRole('button', { name: 'Übernehmen' })[0]!);

    await waitFor(() => {
      const patches = api.mock.calls.filter(([, init]) => (init as RequestInit)?.method === 'PATCH');
      const bodies = patches.map(([, init]) => JSON.parse(String((init as RequestInit).body)));
      expect(bodies.some((b) => b.comment === 'Supermarkt' && b.decision === 'accepted')).toBe(true);
    });
  });
});
