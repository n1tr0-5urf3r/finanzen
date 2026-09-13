import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, cleanup, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../lib/i18n';
import { AppShell } from './AppShell';

afterEach(cleanup);

function renderShell() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter initialEntries={['/dashboard']}>
          <AppShell />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

function scrollTo(y: number) {
  act(() => {
    Object.defineProperty(window, 'scrollY', { value: y, writable: true, configurable: true });
    window.dispatchEvent(new Event('scroll'));
  });
}

describe('the bottom bar', () => {
  /**
   * The bar cannot hold still while the browser's URL bar animates in or out, and
   * that animation only happens during a scroll. So it leaves for the duration of
   * the gesture and returns once the viewport has stopped moving — where its
   * position is unambiguous. Four attempts at holding it still failed; this is the
   * one that does not need the viewport to cooperate.
   */
  it('steps out of the way while the page is scrolling and comes back when it settles', () => {
    vi.useFakeTimers();
    const { container } = renderShell();
    const dock = () => container.querySelector('.mobile-dock') as HTMLElement;

    expect(dock().dataset.scrolling).toBeUndefined();

    // A stray pixel is not a gesture.
    scrollTo(8);
    expect(dock().dataset.scrolling).toBeUndefined();

    scrollTo(300);
    expect(dock().dataset.scrolling).toBe('true');

    act(() => {
      vi.advanceTimersByTime(400);
    });
    expect(dock().dataset.scrolling).toBeUndefined();
    vi.useRealTimers();
  });

  it('keeps the bar in place while the sheet is open, so the sheet cannot leave with it', async () => {
    vi.useFakeTimers();
    const { container } = renderShell();
    const dock = () => container.querySelector('.mobile-dock') as HTMLElement;

    screen.getByRole('button', { name: /Mehr/ }).click();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    scrollTo(300);
    expect(dock().dataset.scrolling).toBeUndefined();
    vi.useRealTimers();
  });
});
