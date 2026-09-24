import { ChartFrame, Gridlines, type ChartDatum } from './ChartFrame';
import { bands, linearScale } from './scales';
import { DataLabel } from '../components/DataLabel';
import { formatEuroCompact } from '../lib/format';
import { useT } from '../lib/i18n';
import { useNarrow } from '../lib/useNarrow';


/* Wide enough for the axis labels these charts actually get. German has no short
   form for thousands — CLDR renders 28.000 as "28.000", not "28 Tsd." — so a
   yearly figure is eight or nine characters and a narrower gutter clips it
   against the panel edge. At the phone's 22-unit text it needs half as much
   again, so the gutter follows the font size. */
const LEFT = { wide: 76, narrow: 118 };
const RIGHT = 710;
const TOP = 12;

/**
 * One band of a bar chart: a month on the monthly overview, a year on the
 * over-the-years screen. The shape is the same and so is every rule about drawing
 * it, which is why this is not two charts.
 */
export interface BandPoint {
  /** The full label, used in the data table and as the row key. Data, never
      translated. */
  label: string;
  short: string;
  incomeCents: number;
  expenseCents: number;
  cumulativeCents: number | null;
  /** The presence signal. A month with no bookings is not a month with zero. */
  hasData: boolean;
}

/**
 * Income against expense, twelve months, side by side.
 *
 * Gross on purpose: this is the one chart in the app that is not net, because
 * "what came in and what went out" is a cash question. The net story is the type
 * chart below it, and the table beside both.
 */
export function MonthlyBars({
  points,
  height = 220,
  title,
}: {
  points: BandPoint[];
  height?: number;
  /** Defaults to the monthly wording; the yearly screen passes its own. */
  title?: string;
}) {
  const t = useT();
  const narrow = useNarrow();
  const bottom = height - 26;
  const scale = linearScale(
    points.flatMap((p) => [p.incomeCents, p.expenseCents]),
    TOP,
    bottom,
  );
  const left = LEFT[narrow ? 'narrow' : 'wide'];
  const band = bands(points.length, left, RIGHT);
  const barWidth = Math.max(3, band.width / 2 - 4);

  const data: ChartDatum[] = points.map((p) => ({
    label: <DataLabel>{p.label}</DataLabel>,
    values: p.hasData ? [p.incomeCents, p.expenseCents] : [null, null],
  }));

  return (
    <ChartFrame
      title={title ?? t('chart.monthly')}
      columns={[t('bookings.income'), t('bookings.expense')]}
      data={data}
      valueBasis="gross"
      height={height}
    >
      <Gridlines
        ticks={scale.ticks}
        y={scale.y}
        left={left}
        right={RIGHT}
        format={formatEuroCompact}
      />
      {points.map((p, i) => {
        const x = band.start(i);
        return (
          <g key={p.label}>
            {p.hasData && (
              <>
                <rect
                  x={x + 2}
                  y={scale.y(p.incomeCents)}
                  width={barWidth}
                  height={Math.max(0, scale.zero - scale.y(p.incomeCents))}
                  className="chart__bar chart__bar--income"
                />
                <rect
                  x={x + barWidth + 6}
                  y={scale.y(p.expenseCents)}
                  width={barWidth}
                  height={Math.max(0, scale.zero - scale.y(p.expenseCents))}
                  className="chart__bar chart__bar--expense"
                />
              </>
            )}
            <text x={band.centre(i)} y={bottom + 15} className="chart__tick" textAnchor="middle">
              {p.short}
            </text>
          </g>
        );
      })}
    </ChartFrame>
  );
}

/**
 * The running balance.
 *
 * It **stops** at the last month that has bookings rather than continuing flat or
 * dropping to zero. A line that runs to December on a year with data through
 * September asserts nine months of nothing; a line that falls to zero asserts the
 * money left the account. Neither happened.
 */
export function CumulativeLine({
  points,
  height = 200,
  title,
  note,
  tableOnPhone = true,
}: {
  points: BandPoint[];
  height?: number;
  title?: string;
  note?: string;
  /** See `ChartFrame`: off where the screen already lists every figure. */
  tableOnPhone?: boolean;
}) {
  const t = useT();
  const left = LEFT[useNarrow() ? 'narrow' : 'wide'];
  const bottom = height - 26;
  // The server keeps the running value alive past the last month with data, so
  // the cut is made here rather than trusting `cumulativeCents` to be null.
  const last = points.reduce((acc, p, i) => (p.hasData ? i : acc), -1);
  const drawn = points
    .map((p, i) => ({ p, i }))
    .filter(({ p, i }) => i <= last && p.cumulativeCents !== null);

  const scale = linearScale(
    drawn.map(({ p }) => p.cumulativeCents as number),
    TOP,
    bottom,
  );
  const band = bands(points.length, left, RIGHT);

  const path = drawn
    .map(
      ({ p, i }, n) =>
        `${n === 0 ? 'M' : 'L'}${band.centre(i).toFixed(1)},${scale
          .y(p.cumulativeCents as number)
          .toFixed(1)}`,
    )
    .join(' ');

  const data: ChartDatum[] = points.map((p, i) => ({
    label: <DataLabel>{p.label}</DataLabel>,
    values: [i <= last ? p.cumulativeCents : null],
  }));

  return (
    <ChartFrame
      title={title ?? t('chart.cumulative')}
      note={note ?? t('chart.stopsAtLastMonth')}
      columns={[t('months.cumulative')]}
      tableOnPhone={tableOnPhone}
      valueBasis="flow"
      data={data}
      height={height}
    >
      <Gridlines
        ticks={scale.ticks}
        y={scale.y}
        left={left}
        right={RIGHT}
        format={formatEuroCompact}
      />
      {drawn.length > 0 && <path d={path} className="chart__line" />}
      {drawn.map(({ p, i }) => (
        <circle
          key={i}
          cx={band.centre(i)}
          cy={scale.y(p.cumulativeCents as number)}
          r={3}
          className="chart__dot"
        />
      ))}
      {points.map((p, i) => (
        <text
          key={p.label}
          x={band.centre(i)}
          y={bottom + 15}
          className={i <= last ? 'chart__tick' : 'chart__tick chart__tick--dim'}
          textAnchor="middle"
        >
          {p.short}
        </text>
      ))}
    </ChartFrame>
  );
}
