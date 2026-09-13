import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { QuickAddPage } = await import('./QuickAddPage');

const COMMENTS = [
  { comment: 'essen', count: 94, categoryName: 'Essen auswärts', categoryId: 'c1' },
  { comment: 'Miete', count: 9, categoryName: 'Miete', categoryId: 'c2' },
];

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) => {
    if (String(path).startsWith('/bookings/comments')) return Promise.resolve(COMMENTS);
    if (String(path).startsWith('/categories')) return Promise.resolve([]);
    if (String(path).startsWith('/rules')) return Promise.resolve([]);
    return Promise.resolve([]);
  });
});
afterEach(cleanup);

function renderQuickAdd() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="en">
        <MemoryRouter initialEntries={['/schnell']}>
          <QuickAddPage />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

describe('Quick Add', () => {
  /**
   * The comment used to be reachable only through a "⌨" tile inside a flex child
   * that scrolls. On a short screen that area collapses to one row and the tile is
   * not on the page at all — so the confirm bar said "enter a comment" while the
   * screen offered nowhere to enter one. A row cannot collapse.
   */
  it('offers a comment row that is on the screen whatever the tiles do', async () => {
    const { container } = renderQuickAdd();
    await screen.findByText('essen');

    const rows = [...container.querySelectorAll('.quick__category')];
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain('Comment');
    expect(rows[1].textContent).toContain('Category');
  });

  it('turns the unsaveable bar into the way to fix it', async () => {
    const user = userEvent.setup();
    const { container } = renderQuickAdd();
    await screen.findByText('essen');

    const confirm = container.querySelector('.quick__confirm') as HTMLButtonElement;
    // No amount yet: nowhere to send anyone, the keypad is right there.
    expect(confirm.textContent).toContain('Enter an amount');
    expect(confirm.disabled).toBe(true);

    await user.click(screen.getByRole('button', { name: '5' }));
    await user.click(screen.getByRole('button', { name: '00' }));

    // Amount but no comment: the bar says so AND opens the sheet rather than
    // sitting dead next to a screen with no visible comment field.
    expect(confirm.textContent).toContain('Enter a comment');
    expect(confirm.disabled).toBe(false);
    await user.click(confirm);
    expect(await screen.findByRole('dialog')).toBeInTheDocument();
  });
});
