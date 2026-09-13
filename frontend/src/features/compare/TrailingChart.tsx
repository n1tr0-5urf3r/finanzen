import { useMemo } from 'react';

import { ChartFrame } from '../../charts/ChartFrame';
import { bands, linearScale } from '../../charts/scales';
import { DataLabel } from '../../components/DataLabel';
import { formatEuroCompact } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { useMaskAmount } from '../../lib/privacy';
import type { TrailingMonth } from '../../lib/types';

const LEFT = 62;
const RIGHT = 710;
const TOP = 14;

/**
 * Twelve months of saldo, ending wherever the data ends.
 *
 * A calendar year restarts the picture every January, which is exactly when a trend
 * is most worth seeing — so this window ignores the year boundary and always holds
 * twelve months. The line is the monthly saldo rather than a running total: the
 * question here is "is each month better than it was", and a cumulative line answers
 * a different one that the Monate screen already answers.
 *
 * Months with nothing in them are drawn as gaps in the line rather than as zeroes.
 * A zero saldo means income and expenses cancelled; no bookings means no data, and
 * flattening one into the other invents a month that never happened.
 */
export function TrailingChart({ months }: { months: TrailingMonth[] }) {
  const t = useT();
  const maskAmount = useMaskAmount();

  const chart = useMemo(() => {
    const present = months.map((m) => m.bookingCount > 0);
    // Saldo is already a flow: positive means the month gained.
    const values = months.filter((_, i) => present[i]).map((m) => m.saldoCents);
    const scale = linearScale(values.length > 0 ? values : [0], TOP, 174);
    const band = bands(months.length, LEFT, RIGHT);

    // One path per unbroken run, so a gap stays a gap instead of being bridged by
    // a straight line through a month nobody booked.
    const runs: { i: number; m: TrailingMonth }[][] = [];
    months.forEach((m, i) => {
      if (!present[i]) return;
      const last = runs[runs.length - 1];
      if (last && last[last.length - 1].i === i - 1) last.push({ i, m });
      else runs.push([{ i, m }]);
    });

    return { present, scale, band, runs };
  }, [months]);

  const height = 200;

  return (
    <ChartFrame
      title={t('compare.trailingTitle')}
      note={t('compare.trailingNote')}
      columns={[t('months.balance')]}
      valueBasis="flow"
      data={months.map((m) => ({
        // The year belongs in the label here and nowhere else in the app: this is
        // the one table whose rows span two of them.
        label: <DataLabel>{`${m.monthName} ${m.year}`}</DataLabel>,
        values: [m.bookingCount > 0 ? m.saldoCents : null],
      }))}
      height={height}
    >
      {chart.scale.ticks.map((tick) => (
        <g key={tick}>
          <line
            x1={LEFT}
            x2={RIGHT}
            y1={chart.scale.y(tick)}
            y2={chart.scale.y(tick)}
            className={tick === 0 ? 'chart__baseline' : 'chart__grid'}
          />
          <text
            x={LEFT - 8}
            y={chart.scale.y(tick) + 4}
            className="chart__tick"
            textAnchor="end"
          >
            {maskAmount(formatEuroCompact(tick))}
          </text>
        </g>
      ))}

      {chart.runs.map((run) => (
        <path
          key={run[0].i}
          d={run
            .map(
              ({ i, m }, n) =>
                `${n === 0 ? 'M' : 'L'}${chart.band.centre(i).toFixed(1)},${chart.scale
                  .y(m.saldoCents)
                  .toFixed(1)}`,
            )
            .join(' ')}
          className="chart__line"
        />
      ))}
      {chart.runs.flat().map(({ i, m }) => (
        <circle
          key={i}
          cx={chart.band.centre(i)}
          cy={chart.scale.y(m.saldoCents)}
          r={3}
          className="chart__dot"
        />
      ))}

      {months.map((m, i) => (
        <text
          key={`${m.year}-${m.month}`}
          x={chart.band.centre(i)}
          y={height - 18}
          className={`chart__tick ${chart.present[i] ? '' : 'chart__tick--dim'}`}
          textAnchor="middle"
        >
          {/* The year is shown only where it changes: twelve repetitions of "26"
              is noise, but the one place the window crosses into a new year is
              the whole reason this chart is not a calendar year. */}
          {m.month === 1 || i === 0 ? `${m.monthName.slice(0, 3)} ${String(m.year).slice(2)}` : m.monthName.slice(0, 3)}
        </text>
      ))}
    </ChartFrame>
  );
}
