import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { AuthProvider } from '../../lib/auth';
import { I18nProvider } from '../../lib/i18n';
import { ThemeProvider } from '../../lib/theme';
import type { User, Year } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { SettingsPage } = await import('./SettingsPage');

const USER: User = { id: 'u1', username: 'fabi', displayName: 'Fabi', isAdmin: true };

const YEARS: Year[] = [
  {
    year: 2025,
    openingBalanceCents: 1701750,
    openingSource: 'derived',
    locked: false,
    bookingCount: 577,
    incomeCents: 6765422,
    expenseCents: 3955581,
    balanceCents: 2809841,
    closingBalanceCents: 4511591,
    carryoverGapCents: null,
  },
  {
    // The 250,00 € the legacy rows are short of their own month markers.
    year: 2026,
    openingBalanceCents: 4000000,
    openingSource: 'configured',
    locked: false,
    bookingCount: 474,
    incomeCents: 3600000,
    expenseCents: 2700000,
    balanceCents: 900000,
    closingBalanceCents: 4900000,
    carryoverGapCents: 25000,
  },
];

function renderPage(entry = '/einstellungen') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <ThemeProvider>
          <AuthProvider>
            <MemoryRouter initialEntries={[entry]}>
              <SettingsPage />
            </MemoryRouter>
          </AuthProvider>
        </ThemeProvider>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

afterEach(cleanup);

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) => {
    if (path.startsWith('/auth/setup-status')) {
      return Promise.resolve({ setupRequired: false, provider: 'local', registrationOpen: false });
    }
    if (path.startsWith('/auth/me')) return Promise.resolve(USER);
    if (path.startsWith('/years')) return Promise.resolve(YEARS);
    if (path.startsWith('/admin/users')) return Promise.resolve([USER]);
    return Promise.resolve(null);
  });
});

describe('settings', () => {
  /**
   * The carry-over gap is an explanation, not a failure. The legacy sheet's own
   * month markers add up to 250,00 € more than its rows, and the standing decision
   * is to import the rows unchanged rather than invent correction bookings — so
   * this must never render as an error state.
   */
  it('explains the carry-over gap rather than reporting it as an error', async () => {
    renderPage();
    const banner = await screen.findByText(/weicht um \+250,00/);
    expect(banner.closest('.banner')?.className).toContain('banner--info');
    expect(banner.textContent).toContain('erfundene Buchungen');
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('says whether an opening balance was entered or inherited', async () => {
    renderPage();
    await screen.findByText('40.000,00 €');
    expect(screen.getByText('eingetragen')).toBeInTheDocument();
    expect(screen.getByText('aus dem Vorjahr')).toBeInTheDocument();
  });

  it('sends a German-typed amount as whole cents', async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findByText('40.000,00 €');

    await user.click(screen.getAllByRole('button', { name: 'Bearbeiten' })[1]);
    const input = await screen.findByLabelText('Vortrag');
    await user.clear(input);
    await user.type(input, '40.000,00');
    await user.click(screen.getByRole('button', { name: 'Speichern' }));

    expect(api).toHaveBeenCalledWith(
      '/years/2026',
      expect.objectContaining({
        method: 'PUT',
        body: JSON.stringify({ year: 2026, openingBalanceCents: 4000000, locked: false }),
      }),
    );
  });

  it('links to the templates screen instead of duplicating it', async () => {
    renderPage('/einstellungen?bereich=vorlagen');
    const link = await screen.findByRole('link', { name: /Vorlagen öffnen/ });
    expect(link.getAttribute('href')).toBe('/vorlagen');
  });

  it('refuses a password change where the two entries disagree', async () => {
    const user = userEvent.setup();
    renderPage('/einstellungen?bereich=konto');
    await screen.findByLabelText('Aktuelles Passwort');

    await user.type(screen.getByLabelText('Aktuelles Passwort'), 'altespasswort');
    await user.type(screen.getByLabelText('Neues Passwort'), 'neuespasswort');
    await user.type(screen.getByLabelText('Neues Passwort wiederholen'), 'vertippt');
    await user.click(screen.getByRole('button', { name: 'Passwort ändern' }));

    expect(await screen.findByText('Die beiden Eingaben stimmen nicht überein.')).toBeInTheDocument();
    expect(api).not.toHaveBeenCalledWith('/auth/password', expect.anything());
  });

  it('offers the export in both shapes, with the round-trippable one labelled', async () => {
    renderPage('/einstellungen?bereich=export');
    const json = await screen.findByRole('button', { name: /JSON/ });
    expect(json.getAttribute('title')).toContain('wiederherstellen');
  });
});
