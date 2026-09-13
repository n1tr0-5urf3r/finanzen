import { useMemo } from 'react';

import { ChartFrame } from '../../charts/ChartFrame';
import { bands, niceTicks } from '../../charts/scales';
import { DataLabel } from '../../components/DataLabel';
import { formatEuroCompact } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { useMaskAmount } from '../../lib/privacy';
import type { KoTrailingMonth } from '../../lib/types';

const LEFT = 62;
const RIGHT = 710;

/**
 * Twelve months of household spending, ending wherever the data ends.
 *
 * A calendar year restarts the picture every January, which is exactly when a trend
 * is most worth seeing — and the mirror's first expense is 2024-12-22, so the very
 * first thing this window has to survive is a turn of the year.
 *
 * Each month draws the household's bar with the user's share nested inside it,
 * exactly as the single-year chart does. Side by side would invite reading two
 * comparable costs; nested says what is true — one is a slice of the other, and
 * the two are never added.
 */
export function KoTrailingChart({ months }: { months: KoTrailingMonth[] }) {
  const t = useT();
  const maskAmount = useMaskAmount();

  const chart = useMemo(() => {
    const ticks = niceTicks(0, Math.max(1, ...months.map((m) => m.amountCents)));
    const band = bands(months.length, LEFT, RIGHT);
    return { ticks, band };
  }, [months]);

  const height = 220;
  const hi = chart.ticks[chart.ticks.length - 1] ?? 1;
  const y = (v: number) => 14 + (1 - v / (hi || 1)) * (height - 52);
  const baseline = y(0);

  return (
    <ChartFrame
      title={t('ko.trailingTitle')}
      note={t('ko.trailingNote')}
      columns={[t('ko.household'), t('ko.myShare')]}
      valueBasis="gross"
      data={months.map((m) => ({
        // The year belongs in the label here and nowhere else on this screen: this
        // is the one table whose rows span two of them.
        label: <DataLabel>{`${m.monthName} ${m.year}`}</DataLabel>,
        values: [m.amountCents, m.ownShareCents],
      }))}
      height={height}
    >
      {chart.ticks.map((tick) => (
        <g key={tick}>
          <line x1={LEFT} x2={RIGHT} y1={y(tick)} y2={y(tick)} className="chart__grid" />
          <text x={LEFT - 8} y={y(tick) + 4} className="chart__tick" textAnchor="end">
            {maskAmount(formatEuroCompact(tick))}
          </text>
        </g>
      ))}

      {months.map((m, i) => (
        <g key={`${m.year}-${m.month}`}>
          <rect
            x={chart.band.start(i) + chart.band.width * 0.15}
            y={y(m.amountCents)}
            width={chart.band.width * 0.7}
            height={Math.max(0, baseline - y(m.amountCents))}
            className="chart__bar chart__bar--household"
          />
          <rect
            x={chart.band.start(i) + chart.band.width * 0.3}
            y={y(m.ownShareCents)}
            width={chart.band.width * 0.4}
            height={Math.max(0, baseline - y(m.ownShareCents))}
            className="chart__bar chart__bar--share"
          />
          <text
            x={chart.band.centre(i)}
            y={height - 18}
            className={`chart__tick ${m.expenseCount === 0 ? 'chart__tick--dim' : ''}`}
            textAnchor="middle"
          >
            {/* The year is shown only where it changes: twelve repetitions of "26"
                is noise, and the one place the window crosses into a new year is
                the whole reason this is not a calendar year. */}
            {m.month === 1 || i === 0
              ? `${m.monthName.slice(0, 3)} ${String(m.year).slice(2)}`
              : m.monthName.slice(0, 3)}
          </text>
        </g>
      ))}
      <line x1={LEFT} x2={RIGHT} y1={baseline} y2={baseline} className="chart__baseline" />
    </ChartFrame>
  );
}
