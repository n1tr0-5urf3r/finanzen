import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { KoSettlement } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { SettlementCard } = await import('./SettlementCard');

function settlement(over: Partial<KoSettlement>): KoSettlement {
  return {
    balanceCents: -14227,
    direction: 'i_owe',
    kind: 'expense',
    categoryId: 'cat-haushaltsausgleich',
    categoryName: 'Haushaltsausgleich',
    categoryIsFallback: false,
    amountCents: 14227,
    period: { year: 2026, month: 9 },
    suggestedComment: 'Ausgleich September',
    alreadySettled: false,
    booking: null,
    settledBalanceCents: null,
    settledAt: null,
    ...over,
  };
}

beforeEach(() => api.mockReset());
afterEach(cleanup);

function renderCard(data: KoSettlement) {
  api.mockImplementation((_path: string, init?: { method?: string }) =>
    Promise.resolve(
      init?.method === 'POST'
        ? ({ ...data, alreadySettled: true } as never)
        : (data as never),
    ),
  );
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter>
          <SettlementCard onNotice={() => {}} />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

describe('settling up', () => {
  /**
   * The figure and the sentence under it have to agree, in BOTH directions.
   * KitchenOwl's balance is signed from the user's side — negative means they owe —
   * and a card that printed "+142,27 € · you owe the household" shipped once
   * already. The sign is never flipped for display here.
   */
  it('reads a negative balance as owing the household', async () => {
    const { container } = renderCard(settlement({ balanceCents: -14227, direction: 'i_owe' }));
    await screen.findByText('Du schuldest dem Haushalt');

    // The balance itself, not the amount repeated in the suggestion line.
    const figure = within(
      container.querySelector('.ko-settle__figure') as HTMLElement,
    ).getByText(/142,27/);
    expect(figure.textContent).toContain('-');
    expect(figure.className).toContain('money--expense');
  });

  it('reads a positive balance as the household owing', async () => {
    const { container } = renderCard(
      settlement({ balanceCents: 14227, direction: 'household_owes_me' }),
    );
    await screen.findByText('Der Haushalt schuldet dir');

    // The balance itself, not the amount repeated in the suggestion line.
    const figure = within(
      container.querySelector('.ko-settle__figure') as HTMLElement,
    ).getByText(/142,27/);
    expect(figure.textContent).toContain('+');
    expect(figure.className).toContain('money--income');
  });

  /**
   * What it books has to be on the card before the button is pressed, because both
   * the kind and the category change the year's figures. This money goes to another
   * person — it is not a transfer between the user's own accounts — so the card says
   * that outright rather than leaving it to be discovered in the balance.
   */
  it('says which kind of booking it makes, and where it lands', async () => {
    const { container } = renderCard(settlement({}));
    await screen.findByText('Ausgleich September');

    expect(container.textContent).toContain('als Ausgabe');
    expect(container.textContent).toContain('Haushaltsausgleich');
    expect(container.textContent).toContain('verändert also die Bilanz');
    // The claim this card used to make, which was the wrong way round.
    expect(container.textContent).not.toContain('doppelt zählen');
    expect(container.textContent).not.toContain('verändert die Jahresbilanz nicht');
  });

  /** The other direction books the opposite kind, and the card says so. */
  it('calls it income when the household owes the user', async () => {
    const { container } = renderCard(
      settlement({ balanceCents: 14227, direction: 'household_owes_me', kind: 'income' }),
    );
    await screen.findByText('Ausgleich September');

    expect(container.textContent).toContain('als Einnahme');
  });

  /** A missing category is announced, not discovered afterwards in the badge. */
  it('warns when Haushaltsausgleich does not exist', async () => {
    const { container } = renderCard(
      settlement({ categoryId: null, categoryName: null, categoryIsFallback: true }),
    );
    await screen.findByText('Ausgleich September');

    expect(container.textContent).toContain('Regeltabelle');
  });

  it('books the settlement and then stops offering it', async () => {
    const user = userEvent.setup();
    renderCard(settlement({}));
    await screen.findByText('Ausgleich September');

    await user.click(screen.getByRole('button', { name: /Ausgleich buchen/ }));
    const posts = api.mock.calls.filter(
      ([, init]) => (init as { method?: string } | undefined)?.method === 'POST',
    );
    expect(posts).toHaveLength(1);
    expect(posts[0][0]).toBe('/kitchenowl/settlement');
  });

  /** Nothing owed is an answer, not an empty state. */
  it('offers no button when the household is settled', async () => {
    renderCard(
      settlement({ balanceCents: 0, direction: 'settled', amountCents: 0 }),
    );
    await screen.findByText('Ausgeglichen');
    expect(screen.getByRole('button', { name: /Ausgleich buchen/ })).toBeDisabled();
  });

  it('shows the booking instead of the button once the month is settled', async () => {
    renderCard(
      settlement({
        alreadySettled: true,
        settledBalanceCents: -14227,
        settledAt: '2026-09-13T10:00:00Z',
        booking: { comment: 'Ausgleich September' } as never,
      }),
    );
    expect(await screen.findByText(/Für diesen Monat gebucht/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Ausgleich buchen/ })).toBeNull();
  });
});
