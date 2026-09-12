import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { Money } from '../components/Money';
import { PrivacyToggle } from '../components/PrivacyToggle';
import { MonthlyBars } from '../charts/MonthlyCharts';
import { MONTHS_DE, MONTHS_DE_SHORT } from './format';
import { I18nProvider } from './i18n';
import { PrivacyProvider } from './privacy';

function wrap(ui: React.ReactNode, hidden?: boolean) {
  return render(
    <I18nProvider initialLocale="de">
      <PrivacyProvider initialHidden={hidden}>{ui}</PrivacyProvider>
    </I18nProvider>,
  );
}

beforeEach(() => localStorage.clear());
afterEach(() => localStorage.clear());

describe('privacy mode', () => {
  it('is off unless it was switched on', () => {
    wrap(<Money cents={480_000} basis="net" />);
    expect(screen.getByText(/4\.800,00/)).toBeInTheDocument();
  });

  /**
   * The whole point: the sentence around the figure still makes sense. A category
   * is still named, a credit still reads as a credit, a net figure still says it
   * is net — only the amount is gone.
   */
  it('replaces the amount and keeps everything that says what kind of amount it is', () => {
    wrap(<Money cents={-2_200_000} basis="net" tone="auto" />, true);

    expect(screen.queryByText(/22\.000,00/)).not.toBeInTheDocument();
    const el = screen.getByText(/•/);
    expect(el.className).toContain('money--credit');
    expect(el.className).toContain('money--income');
    // Not read aloud either — a masked figure with its real value in the
    // accessible name would leak it to anyone screen-sharing with a reader on.
    expect(el.getAttribute('aria-label')).toBe('Betrag verborgen');
  });

  it('leaves the chart itself intact and masks only its scale', () => {
    const points = MONTHS_DE_SHORT.map((short, i) => ({
      month: i + 1,
      monthName: MONTHS_DE[i],
      short,
      hasData: true,
      incomeCents: 100_000,
      expenseCents: 80_000,
      netCents: -20_000,
      cumulativeCents: null,
      bookingCount: 3,
    }));
    const { container } = wrap(<MonthlyBars points={points as never} />, true);

    // Every bar is still drawn: the shape of the year is not the secret.
    expect(container.querySelectorAll('.chart__bar').length).toBeGreaterThan(0);
    // The axis is, because it is an absolute figure.
    // Month names stay; every euro label on the axis is gone. (Month short names
    // carry no digits, so "no digits left" is a fair test of the axis.)
    const ticks = [...container.querySelectorAll('.chart__tick')].map((n) => n.textContent);
    expect(ticks).toContain('Jan');
    expect(ticks.every((label) => !/\d/.test(label ?? ''))).toBe(true);
  });

  it('toggles from any page header and is remembered', async () => {
    const user = userEvent.setup();
    wrap(
      <>
        <PrivacyToggle />
        <Money cents={900_000} />
      </>,
    );

    await user.click(screen.getByRole('button', { name: 'Beträge verbergen' }));
    expect(screen.queryByText(/9\.000,00/)).not.toBeInTheDocument();
    expect(localStorage.getItem('finanzen.privacy')).toBe('on');

    await user.click(screen.getByRole('button', { name: 'Beträge anzeigen' }));
    expect(screen.getByText(/9\.000,00/)).toBeInTheDocument();
    expect(localStorage.getItem('finanzen.privacy')).toBe('off');
  });
});
