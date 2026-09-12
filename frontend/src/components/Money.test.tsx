import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { ReactNode } from 'react';

import { I18nProvider } from '../lib/i18n';
import { Money } from './Money';

function wrap(ui: ReactNode, locale: 'de' | 'en' = 'de') {
  return render(<I18nProvider initialLocale={locale}>{ui}</I18nProvider>);
}

describe('<Money>', () => {
  it('marks a net figure so it cannot be read as a plain amount', () => {
    wrap(<Money cents={480_000} basis="net" />);
    const el = screen.getByText(/4\.800,00/);
    expect(el.className).toContain('money');
    // The marker is visible to sighted users and the basis word is in the
    // accessible name, so both channels carry the distinction.
    expect(el.getAttribute('aria-label')).toContain('netto');
    expect(screen.getByTitle(/Netto = Ausgaben minus Einnahmen/)).toBeInTheDocument();
  });

  it('does not mark a gross figure', () => {
    wrap(<Money cents={960_000} basis="gross" />);
    expect(screen.getByText(/9\.600,00/).getAttribute('aria-label')).not.toContain('netto');
  });

  it('renders a negative net as a credit rather than a bare minus', () => {
    // The single most misreadable cell in the app: a category that earned money.
    wrap(<Money cents={-2_200_000} basis="net" tone="auto" />);
    const el = screen.getByText(/22\.000,00/);
    expect(el.className).toContain('money--credit');
    expect(el.className).toContain('money--income');
  });

  it('formats de-DE even when the interface is English', () => {
    wrap(<Money cents={2_700_000} basis="net" />, 'en');
    expect(screen.getByText(/27\.000,00/)).toBeInTheDocument();
    // ...and the basis word follows the interface, not the number format.
    expect(screen.getByText(/27\.000,00/).getAttribute('aria-label')).toContain('net');
  });

  it('renders an absent value without inventing a zero', () => {
    wrap(<Money cents={null} />);
    expect(screen.getByText('–')).toBeInTheDocument();
  });
});
