import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it } from 'vitest';

import { AppFooter } from './AppFooter';
import { AppShell } from './AppShell';
import { PrivacyToggle } from './PrivacyToggle';
import { I18nProvider } from '../lib/i18n';

afterEach(cleanup);

function renderShell() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter initialEntries={['/dashboard']}>
          <AppShell />
          <AppFooter />
          <PrivacyToggle />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

/**
 * An icon set's major version renames, splits and drops icons, and TypeScript only
 * catches the third of those. A name that still exists but resolves to nothing
 * compiles perfectly and renders an empty box — which is why this counts drawing
 * instructions rather than elements.
 */
describe('every icon in the chrome draws something', () => {
  it('renders a shape for each navigation entry, not an empty box', () => {
    const { container } = renderShell();
    const svgs = [...container.querySelectorAll('svg')];

    // Sidebar (11 links + Quick Add) and bottom bar (3 links + ⊕ + Mehr), plus the
    // footer mark and the privacy eye. The exact total matters less than the floor:
    // if an icon silently resolved to nothing, this drops.
    expect(svgs.length).toBeGreaterThanOrEqual(18);

    const empty = svgs.filter((svg) => svg.children.length === 0);
    expect(
      empty.length,
      `${empty.length} of ${svgs.length} icons rendered with no drawing instructions`,
    ).toBe(0);
  });

  it('draws the GitHub mark the icon set no longer ships', () => {
    const { container } = renderShell();
    const footer = container.querySelector('.app-footer') as HTMLElement;
    const mark = footer.querySelector('svg');
    // lucide 1.0 removed every brand icon, so this one is drawn locally. A `path`
    // with real geometry is the difference between the mark and a blank square.
    expect(mark?.querySelector('path')?.getAttribute('d')?.length ?? 0).toBeGreaterThan(100);
  });
});
