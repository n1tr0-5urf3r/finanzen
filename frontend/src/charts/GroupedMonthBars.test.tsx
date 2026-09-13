import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { GroupedMonthBars } from './GroupedMonthBars';
import { I18nProvider } from '../lib/i18n';

function wrap(ui: React.ReactNode) {
  return render(<I18nProvider initialLocale="de">{ui}</I18nProvider>);
}

// Januar and März only, so the empty months are visible as empty.
const CURRENT = [12_000, 0, 30_000, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const PREVIOUS = [10_000, 0, 8_000, 0, 0, 0, 0, 0, 0, 0, 0, 0];

describe('two years side by side', () => {
  it('draws a pair of bars for every month, not one merged figure', () => {
    const { container } = wrap(
      <GroupedMonthBars
        title="Lebensmittel · 2026 / 2025"
        labelCurrent="2026"
        labelPrevious="2025"
        current={CURRENT}
        previous={PREVIOUS}
      />,
    );
    // Twelve months × two years, zero-height months included: a month with no
    // spending is information, and dropping it would shift the year's shape.
    expect(container.querySelectorAll('.chart__bar--now')).toHaveLength(12);
    expect(container.querySelectorAll('.chart__bar--then')).toHaveLength(12);
  });

  /**
   * The stored convention is expense-positive, so a cost has to be drawn BELOW the
   * baseline — the same way every other chart in the app draws it. Two years of
   * costs pointing up would contradict the analysis screen.
   */
  it('draws costs downward and keeps both years on the same scale', () => {
    const { container } = wrap(
      <GroupedMonthBars
        title="t"
        labelCurrent="2026"
        labelPrevious="2025"
        current={CURRENT}
        previous={PREVIOUS}
      />,
    );
    const now = [...container.querySelectorAll('.chart__bar--now')] as SVGRectElement[];
    const then = [...container.querySelectorAll('.chart__bar--then')] as SVGRectElement[];
    const januaryNow = Number(now[0].getAttribute('height'));
    const januaryThen = Number(then[0].getAttribute('height'));
    const marchNow = Number(now[2].getAttribute('height'));

    // 12.000 against 10.000 in Januar: the taller bar is the larger cost...
    expect(januaryNow).toBeGreaterThan(januaryThen);
    // ...and 30.000 in März is taller still, because one scale serves both years.
    expect(marchNow).toBeGreaterThan(januaryNow);
  });

  it('names both years in the legend and in the table underneath', () => {
    const { container } = wrap(
      <GroupedMonthBars
        title="Lebensmittel"
        labelCurrent="2026"
        labelPrevious="2025"
        current={CURRENT}
        previous={PREVIOUS}
      />,
    );
    const legend = container.querySelector('.chart-legend') as HTMLElement;
    expect(within(legend).getByText('2026')).toBeInTheDocument();
    expect(within(legend).getByText('2025')).toBeInTheDocument();
    // The data table is the chart's accessible equivalent; both columns are there.
    const table = container.querySelector('.data-table') as HTMLElement;
    expect(within(table).getAllByText('2026').length).toBeGreaterThan(0);
    expect(screen.getAllByText(/120,00/).length).toBeGreaterThan(0);
  });

  /** The household ledger does not net, so its figures only ever point up. */
  it('keeps gross figures above the baseline', () => {
    const { container } = wrap(
      <GroupedMonthBars
        title="Wocheneinkauf"
        labelCurrent="2026"
        labelPrevious="2025"
        current={CURRENT}
        previous={PREVIOUS}
        valueBasis="gross"
      />,
    );
    const baseline = container.querySelector('.chart__baseline') as SVGLineElement;
    const bar = container.querySelector('.chart__bar--now') as SVGRectElement;
    const barBottom = Number(bar.getAttribute('y')) + Number(bar.getAttribute('height'));
    expect(barBottom).toBeCloseTo(Number(baseline.getAttribute('y1')), 0);
  });
});
