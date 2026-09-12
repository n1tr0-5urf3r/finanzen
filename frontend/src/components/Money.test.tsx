import { cleanup, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import type { ReactNode } from 'react';

import { I18nProvider } from '../lib/i18n';
import { FlowMoney, Money } from './Money';

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

describe('<FlowMoney>', () => {
  // This file has no global auto-cleanup, and every case here renders the same
  // figure with a different sign.
  afterEach(cleanup);

  /**
   * The regression that made every expense in the analysis table green: `Money`
   * infers "credit" from a negative net, and a flipped figure hands it a negative
   * for an ordinary cost. Colour and marking must follow the STORED direction,
   * not the sign on screen.
   */
  it('does not mark an ordinary expense as a credit', () => {
    const { container } = render(
      <I18nProvider initialLocale="de">
        <FlowMoney netCents={110000} />
      </I18nProvider>,
    );
    const el = within(container).getByText(/1\.100,00/);
    // Money went out: minus, expense colouring, no credit.
    expect(el.textContent).toContain('-');
    expect(el.className).toContain('money--expense');
    expect(el.className).not.toContain('money--credit');
  });

  it('shows money that came in as a signed gain', () => {
    const { container } = render(
      <I18nProvider initialLocale="de">
        <FlowMoney netCents={-2200000} />
      </I18nProvider>,
    );
    const el = within(container).getByText(/22\.000,00/);
    expect(el.textContent).toContain('+');
    expect(el.className).toContain('money--income');
    expect(el.className).toContain('money--credit');
  });

  /** A balance is already a flow, so its sign is kept and only coloured. */
  it('keeps the sign of a figure that is already a balance', () => {
    const { container } = render(
      <I18nProvider initialLocale="de">
        <FlowMoney flowCents={900000} />
      </I18nProvider>,
    );
    const el = within(container).getByText(/9\.000,00/);
    expect(el.textContent).toContain('+');
    expect(el.className).toContain('money--income');
  });
});
