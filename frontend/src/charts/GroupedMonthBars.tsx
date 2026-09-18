import { ChartFrame } from './ChartFrame';
import { bands, niceTicks } from './scales';
import { formatEuroCompact, monthShort } from '../lib/format';
import { useMaskAmount } from '../lib/privacy';

/* Wide enough for the axis labels a yearly chart gets: German has no short form
   for thousands, so 28.000 stays eight characters. */
const LEFT = 76;
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
  labels,
}: {
  title: string;
  note?: string;
  labelCurrent: string;
  labelPrevious: string;
  /** Stored-convention values, one per band. Twelve months by default. */
  current: number[];
  /** The second series, or empty when the subject has only one — a payer fronted
      the whole amount, so there is no share of it to draw beside it. */
  previous: number[];
  /**
   * `net` — stored expense-positive figures, drawn as flows: money in points up,
   * money out points down, which is how every other chart in the app reads.
   * `gross` — plain magnitudes that only ever point up (the household ledger,
   * where an expense is an expense and nothing nets).
   */
  valueBasis?: 'net' | 'gross';
  height?: number;
  /**
   * The band labels. Twelve month abbreviations unless given: the household's
   * over-the-years screen draws the same pair of bars over years instead, and
   * "two series across a row of bands" is one chart, not two.
   */
  labels?: string[];
}) {
  const maskAmount = useMaskAmount();
  const flip = valueBasis === 'net' ? -1 : 1;

  const ticksLabels = labels ?? Array.from({ length: 12 }, (_, i) => monthShort(i + 1));
  const count = ticksLabels.length;
  const hasPrevious = previous.length > 0;
  const a = Array.from({ length: count }, (_, i) => (current[i] ?? 0) * flip);
  const b = Array.from({ length: count }, (_, i) => (previous[i] ?? 0) * flip);
  const ticks = niceTicks(Math.min(0, ...a, ...b), Math.max(0, ...a, ...b));

  const lo = ticks[0] ?? 0;
  const hi = ticks[ticks.length - 1] ?? 1;
  const span = hi - lo || 1;
  const y = (v: number) => 14 + (1 - (v - lo) / span) * (height - 52);
  const band = bands(count, LEFT, RIGHT);
  const zeroY = y(0);
  // Two bars in the middle 70% of each band, with a hair of air between them —
  // or one bar filling that space when there is no second series.
  const barWidth = hasPrevious
    ? Math.max(2, (band.width * 0.7) / 2 - 1)
    : Math.max(2, band.width * 0.7);

  const legend = !hasPrevious ? undefined : (
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
        columns={hasPrevious ? [labelCurrent, labelPrevious] : [labelCurrent]}
        valueBasis={valueBasis}
        data={Array.from({ length: count }, (_, i) => ({
          label: ticksLabels[i] ?? '',
          values: hasPrevious ? [current[i] ?? 0, previous[i] ?? 0] : [current[i] ?? 0],
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
        {Array.from({ length: count }, (_, i) => {
          const left = band.start(i) + band.width * 0.15;
          return (
            <g key={i}>
              {(hasPrevious ? [a[i], b[i]] : [a[i]]).map((value, slot) => (
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
                {ticksLabels[i]}
              </text>
            </g>
          );
        })}
        <line x1={LEFT} x2={RIGHT} y1={zeroY} y2={zeroY} className="chart__baseline" />
      </ChartFrame>
  );
}
