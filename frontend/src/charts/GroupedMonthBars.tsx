import { ChartFrame } from './ChartFrame';
import { bands, niceTicks } from './scales';
import { formatEuroCompact, monthShort } from '../lib/format';
import { useMaskAmount } from '../lib/privacy';

const LEFT = 62;
const RIGHT = 710;

/**
 * Twelve months, two years, side by side.
 *
 * A table of two totals says which year was larger. It cannot say WHEN — whether
 * the rise is a new standing cost from March or one holiday in August — and that
 * is the question a comparison is actually opened with. Two bars per month answer
 * it at a glance and the table underneath keeps the figures.
 *
 * The pair is drawn in year colours, not in income/expense colours: what is being
 * compared here is two periods, and the direction of the money is already carried
 * by which side of the baseline a bar sits on.
 */
export function GroupedMonthBars({
  title,
  note,
  labelCurrent,
  labelPrevious,
  current,
  previous,
  valueBasis = 'net',
  height = 240,
}: {
  title: string;
  note?: string;
  labelCurrent: string;
  labelPrevious: string;
  /** Twelve stored-convention values, Januar first. */
  current: number[];
  previous: number[];
  /**
   * `net` — stored expense-positive figures, drawn as flows: money in points up,
   * money out points down, which is how every other chart in the app reads.
   * `gross` — plain magnitudes that only ever point up (the household ledger,
   * where an expense is an expense and nothing nets).
   */
  valueBasis?: 'net' | 'gross';
  height?: number;
}) {
  const maskAmount = useMaskAmount();
  const flip = valueBasis === 'net' ? -1 : 1;

  const a = Array.from({ length: 12 }, (_, i) => (current[i] ?? 0) * flip);
  const b = Array.from({ length: 12 }, (_, i) => (previous[i] ?? 0) * flip);
  const ticks = niceTicks(Math.min(0, ...a, ...b), Math.max(0, ...a, ...b));

  const lo = ticks[0] ?? 0;
  const hi = ticks[ticks.length - 1] ?? 1;
  const span = hi - lo || 1;
  const y = (v: number) => 14 + (1 - (v - lo) / span) * (height - 52);
  const band = bands(12, LEFT, RIGHT);
  const zeroY = y(0);
  // Two bars in the middle 70% of each band, with a hair of air between them.
  const barWidth = Math.max(2, (band.width * 0.7) / 2 - 1);

  const legend = (
      <p className="chart-legend">
        <span className="chart-legend__item">
          <span className="chart-legend__swatch chart-legend__swatch--now" aria-hidden="true" />
          {labelCurrent}
        </span>
        <span className="chart-legend__item">
          <span className="chart-legend__swatch chart-legend__swatch--then" aria-hidden="true" />
          {labelPrevious}
        </span>
      </p>
  );

  return (
      <ChartFrame
        title={title}
        legend={legend}
        note={note}
        columns={[labelCurrent, labelPrevious]}
        valueBasis={valueBasis}
        data={Array.from({ length: 12 }, (_, i) => ({
          label: monthShort(i + 1),
          values: [current[i] ?? 0, previous[i] ?? 0],
        }))}
        height={height}
      >
        {ticks.map((tick) => (
          <g key={tick}>
            <line x1={LEFT} x2={RIGHT} y1={y(tick)} y2={y(tick)} className="chart__grid" />
            <text x={LEFT - 8} y={y(tick) + 4} className="chart__tick" textAnchor="end">
              {maskAmount(formatEuroCompact(tick))}
            </text>
          </g>
        ))}
        {Array.from({ length: 12 }, (_, i) => {
          const left = band.start(i) + band.width * 0.15;
          return (
            <g key={i}>
              {[a[i], b[i]].map((value, slot) => (
                <rect
                  key={slot}
                  x={left + slot * (barWidth + 2)}
                  y={Math.min(y(value), zeroY)}
                  width={barWidth}
                  height={Math.max(value === 0 ? 0 : 1, Math.abs(y(value) - zeroY))}
                  className={`chart__bar ${slot === 0 ? 'chart__bar--now' : 'chart__bar--then'}`}
                />
              ))}
              <text
                x={band.centre(i)}
                y={height - 18}
                className="chart__tick"
                textAnchor="middle"
              >
                {monthShort(i + 1)}
              </text>
            </g>
          );
        })}
        <line x1={LEFT} x2={RIGHT} y1={zeroY} y2={zeroY} className="chart__baseline" />
      </ChartFrame>
  );
}
