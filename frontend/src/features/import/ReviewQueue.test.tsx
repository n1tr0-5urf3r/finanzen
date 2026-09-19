import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { Category, ReviewItem } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { ReviewQueue } = await import('./ReviewQueue');

const CATEGORIES: Category[] = [
  { id: 'essen-auswaerts', name: 'Essen auswärts', typeCode: 'variabel', typeLabel: 'Variable Kosten', sortOrder: 1, archived: false, bookingCount: 0, netCents: 0 },
  { id: 'sport', name: 'Sport', typeCode: 'fixkosten', typeLabel: 'Fixkosten', sortOrder: 2, archived: false, bookingCount: 0, netCents: 0 },
  { id: 'reisen', name: 'Reisen & Urlaub', typeCode: 'variabel', typeLabel: 'Variable Kosten', sortOrder: 3, archived: false, bookingCount: 0, netCents: 0 },
];

function item(over: Partial<ReviewItem>): ReviewItem {
  return {
    id: 'x',
    comment: 'x',
    normalizedComment: 'x',
    rowCount: 1,
    expenseCents: 0,
    incomeCents: 0,
    suggestions: [],
    weakHints: [],
    ambiguous: false,
    suggestedKind: null,
    status: 'open',
    ...over,
  };
}

/**
 * The shape a legacy queue produces: `Malve` is the top item by frequency and has
 * only an edit-distance hint (`mafit` → Sport), which is exactly the confidently
 * wrong answer the design refuses to preselect.
 */
const ITEMS: ReviewItem[] = [
  item({
    id: 'malve',
    comment: 'Malve',
    normalizedComment: 'malve',
    rowCount: 11,
    expenseCents: 10729,
    weakHints: [
      { categoryId: 'sport', categoryName: 'Sport', matchedRule: 'mafit', confidence: 0.78, tier: 'similar', isSuggestion: false },
    ],
  }),
  item({
    id: 'essen-berlin',
    comment: 'Essen Berlin',
    normalizedComment: 'essen berlin',
    rowCount: 9,
    expenseCents: 9020,
    suggestions: [
      { categoryId: 'essen-auswaerts', categoryName: 'Essen auswärts', matchedRule: 'essen', confidence: 0.95, tier: 'token', isSuggestion: true },
    ],
  }),
  item({
    id: 'paypal-essen',
    comment: 'Paypal essen',
    normalizedComment: 'paypal essen',
    rowCount: 3,
    ambiguous: true,
    suggestions: [
      { categoryId: 'sport', categoryName: 'Sonstiges', matchedRule: 'paypal', confidence: 0.9, tier: 'token', isSuggestion: true },
      { categoryId: 'essen-auswaerts', categoryName: 'Essen auswärts', matchedRule: 'essen', confidence: 0.9, tier: 'token', isSuggestion: true },
    ],
  }),
];

function renderQueue() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter>
          <ReviewQueue batchId="batch-1" categories={CATEGORIES} />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

afterEach(cleanup);

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((_path: string, options?: RequestInit) => {
    if (options?.method === 'POST') {
      const body = JSON.parse(String(options.body)) as { resolutions: unknown[] };
      return Promise.resolve({
        resolved: body.resolutions.length,
        skipped: 0,
        rulesCreated: body.resolutions.length,
        remainingOpen: ITEMS.length - body.resolutions.length,
      });
    }
    return Promise.resolve(ITEMS);
  });
});

describe('the import review queue', () => {
  it('keeps the queue in frequency order — the item clearing eleven rows comes first', async () => {
    const { container } = renderQueue();
    await screen.findAllByText('Malve');

    const queue = [...container.querySelectorAll('.queue__item')].map((b) => b.textContent);
    expect(queue[0]).toContain('Malve');
    expect(queue[0]).toContain('11');
    expect(queue[1]).toContain('Essen Berlin');
    expect(queue[2]).toContain('Paypal essen');
  });

  /**
   * Edit-distance hints are where `Malve → Mafit` comes from. They are offered,
   * greyed, and never chosen for the user.
   */
  it('never preselects a weak hint', async () => {
    renderQueue();
    await screen.findAllByText('Malve');

    const hint = screen.getByRole('button', { name: /Sport/ });
    expect(hint.className).toContain('suggestion--hint');
    expect(hint.getAttribute('aria-pressed')).toBe('false');
    // With nothing chosen, there is nothing to confirm.
    expect(screen.getByRole('button', { name: /Übernehmen/ })).toBeDisabled();
    expect(screen.getByText(/ein Händler, den die Regeltabelle noch nicht kennt/)).toBeInTheDocument();
  });

  it('sends createRule: true when "als Regel merken" is left checked, and advances', async () => {
    const user = userEvent.setup();
    renderQueue();
    await screen.findAllByText('Malve');

    // Choose the hint deliberately; the checkbox is already on.
    await user.click(screen.getByRole('button', { name: /Sport/ }));
    expect(screen.getByLabelText('Als Regel merken')).toBeChecked();
    await user.click(screen.getByRole('button', { name: /Übernehmen/ }));

    expect(api).toHaveBeenCalledWith(
      '/imports/batch-1/review',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({
          resolutions: [{ itemId: 'malve', categoryId: 'sport', createRule: true }],
        }),
      }),
    );
    // Auto-advance: the next item is already on screen.
    expect(await screen.findByText('9 Buchungen')).toBeInTheDocument();
  });

  it('can be told not to write a rule', async () => {
    const user = userEvent.setup();
    renderQueue();
    await screen.findAllByText('Malve');

    await user.click(screen.getByRole('button', { name: /Sport/ }));
    await user.click(screen.getByLabelText('Als Regel merken'));
    await user.click(screen.getByRole('button', { name: /Übernehmen/ }));

    const body = JSON.parse(String(api.mock.calls.at(-1)![1].body));
    expect(body.resolutions[0].createRule).toBe(false);
  });

  it('preselects a containment suggestion but not an ambiguous one', async () => {
    const user = userEvent.setup();
    const { container } = renderQueue();
    const shown = () =>
      (container.querySelector('.review-card__comment') as HTMLElement).textContent;

    await screen.findAllByText('Malve');
    // Malve has only a hint, so nothing is chosen for the user; move past it.
    await user.click(screen.getByRole('button', { name: /Überspringen/ }));

    // Essen Berlin: a containment match, preselected.
    await vi.waitFor(() => expect(shown()).toBe('Essen Berlin'));
    expect(
      screen.getByRole('button', { name: /Essen auswärts/ }).getAttribute('aria-pressed'),
    ).toBe('true');

    await user.click(screen.getByRole('button', { name: /Übernehmen/ }));

    // Paypal essen: two rules claim it equally, so the user must choose.
    await vi.waitFor(() => expect(shown()).toBe('Paypal essen'));
    expect(screen.getByText('mehrdeutig — bitte selbst wählen')).toBeInTheDocument();
    for (const button of screen.getAllByRole('button', { name: /Sonstiges|Essen auswärts/ })) {
      expect(button.getAttribute('aria-pressed')).toBe('false');
    }
    expect(screen.getByRole('button', { name: /Übernehmen/ })).toBeDisabled();
  });

  it('bulk-accepts only the containment suggestions', async () => {
    const user = userEvent.setup();
    renderQueue();
    await screen.findAllByText('Malve');

    // One of three: Malve has only a hint, Paypal essen is ambiguous.
    const bulk = screen.getByRole('button', { name: /1 sichere Vorschläge übernehmen/ });
    await user.click(bulk);

    const body = JSON.parse(String(api.mock.calls.at(-1)![1].body));
    expect(body.resolutions).toEqual([
      { itemId: 'essen-berlin', categoryId: 'essen-auswaerts', createRule: true },
    ]);
  });

  it('picks a suggestion with a digit key', async () => {
    const user = userEvent.setup();
    renderQueue();
    await screen.findAllByText('Malve');

    await user.keyboard('1');
    expect(screen.getByRole('button', { name: /Sport/ }).getAttribute('aria-pressed')).toBe('true');
    await user.keyboard('{Enter}');

    const body = JSON.parse(String(api.mock.calls.at(-1)![1].body));
    expect(body.resolutions[0]).toEqual({
      itemId: 'malve',
      categoryId: 'sport',
      createRule: true,
    });
  });

  it('shows the comment as data and the row count it clears', async () => {
    const { container } = renderQueue();
    await screen.findAllByText('Malve');

    const card = container.querySelector('.review-card__comment') as HTMLElement;
    const label = within(card).getByText('Malve');
    expect(label.getAttribute('lang')).toBe('de');
    expect(label.getAttribute('translate')).toBe('no');
    expect(screen.getByText('11 Buchungen')).toBeInTheDocument();
    expect(screen.getByText(/107,29/)).toBeInTheDocument();
  });
});

describe('the keyboard loop', () => {
  /**
   * ~154 of the 238 items have no suggestion, so the filter IS the decision for
   * most of the queue. Walking it needs arrow keys, and backing out of a typo
   * must not cost the item.
   */
  it('walks the matches with the arrow keys and takes the highlighted one', async () => {
    const user = userEvent.setup();
    renderQueue();
    await screen.findAllByText('Malve');

    const filter = screen.getByLabelText(/Kategorie wählen/i);
    await user.type(filter, 'e');

    const options = await screen.findAllByRole('option');
    expect(options.length).toBeGreaterThan(1);
    expect(options[0].className).toContain('is-highlighted');

    await user.keyboard('{ArrowDown}');
    const after = screen.getAllByRole('option');
    expect(after[1].className).toContain('is-highlighted');
    expect(after[0].className).not.toContain('is-highlighted');

    // Wrapping means holding the key never dead-ends.
    await user.keyboard('{ArrowUp}{ArrowUp}');
    const wrapped = screen.getAllByRole('option');
    expect(wrapped[wrapped.length - 1].className).toContain('is-highlighted');
  });

  it('clears the filter on Escape rather than skipping the item', async () => {
    const user = userEvent.setup();
    renderQueue();
    // The first item is the most frequent unknown comment; losing it to a typo
    // would be losing your place.
    const before = await screen.findAllByText('Malve');
    expect(before.length).toBeGreaterThan(0);

    const filter = screen.getByLabelText(/Kategorie wählen/i);
    await user.type(filter, 'leb');
    expect((filter as HTMLInputElement).value).toBe('leb');

    await user.keyboard('{Escape}');
    expect((filter as HTMLInputElement).value).toBe('');
    // Still the same item: Escape backed out of the search, not the decision.
    expect(screen.getAllByText('Malve').length).toBeGreaterThan(0);
  });
});
