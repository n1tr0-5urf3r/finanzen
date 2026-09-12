import { ChartFrame, Gridlines, type ChartDatum } from './ChartFrame';
import { bands, linearScale, niceTicks } from './scales';
import { DataLabel } from '../components/DataLabel';
import { formatEuroCompact } from '../lib/format';
import { useMaskAmount } from '../lib/privacy';
import { useT } from '../lib/i18n';

/**
 * A type label drawn inside the SVG is still ground-truth data, and browser page
 * translation will happily rewrite `<text>`. React's SVG typings carry no
 * `translate` prop, so the pair is spread in as attributes — the same contract
 * `<DataLabel>` gives every other data string in the app.
 */
const DATA_TEXT: Record<string, string> = { lang: 'de', translate: 'no' };

const LEFT = 58;
const RIGHT = 710;
const TOP = 12;

export interface MonthPoint {
  monthName: string;
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
export function MonthlyBars({ points, height = 220 }: { points: MonthPoint[]; height?: number }) {
  const t = useT();
  const bottom = height - 26;
  const scale = linearScale(
    points.flatMap((p) => [p.incomeCents, p.expenseCents]),
    TOP,
    bottom,
  );
  const band = bands(points.length, LEFT, RIGHT);
  const barWidth = Math.max(3, band.width / 2 - 4);

  const data: ChartDatum[] = points.map((p) => ({
    label: <DataLabel>{p.monthName}</DataLabel>,
    values: p.hasData ? [p.incomeCents, p.expenseCents] : [null, null],
  }));

  return (
    <ChartFrame
      title={t('chart.monthly')}
      columns={[t('bookings.income'), t('bookings.expense')]}
      data={data}
      height={height}
    >
      <Gridlines
        ticks={scale.ticks}
        y={scale.y}
        left={LEFT}
        right={RIGHT}
        format={formatEuroCompact}
      />
      {points.map((p, i) => {
        const x = band.start(i);
        return (
          <g key={p.monthName}>
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
}: {
  points: MonthPoint[];
  height?: number;
}) {
  const t = useT();
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
  const band = bands(points.length, LEFT, RIGHT);

  const path = drawn
    .map(
      ({ p, i }, n) =>
        `${n === 0 ? 'M' : 'L'}${band.centre(i).toFixed(1)},${scale
          .y(p.cumulativeCents as number)
          .toFixed(1)}`,
    )
    .join(' ');

  const data: ChartDatum[] = points.map((p, i) => ({
    label: <DataLabel>{p.monthName}</DataLabel>,
    values: [i <= last ? p.cumulativeCents : null],
  }));

  return (
    <ChartFrame
      title={t('chart.cumulative')}
      note={t('chart.stopsAtLastMonth')}
      columns={[t('months.cumulative')]}
      data={data}
      height={height}
    >
      <Gridlines
        ticks={scale.ticks}
        y={scale.y}
        left={LEFT}
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
          key={p.monthName}
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

export interface TypeSlice {
  /** A type label straight from the database. Data, so it is never translated. */
  label: string;
  typeCode: string;
  netCents: number;
}

/**
 * Net per type, hanging off a zero line rather than growing from the left edge.
 *
 * A negative net is a credit, not a small cost, and a chart that clamps at zero or
 * takes an absolute value hides exactly the figure the user most needs to
 * question — Juni's −300,00 € variable costs being the live example.
 */
export function TypeBreakdown({ slices }: { slices: TypeSlice[] }) {
  const t = useT();
  const maskAmount = useMaskAmount();
  const rowHeight = 34;
  // "Variable Kosten" is the widest label in the set and the month charts' 58-unit
  // gutter cuts it in half. The viewBox is 720 units wide regardless of the
  // rendered size, so this is measured in the same units as LEFT/RIGHT.
  const labelGutter = 132;
  const height = Math.max(90, slices.length * rowHeight + 34);
  // A horizontal chart needs the same nice, zero-containing domain, mapped along x.
  const ticks = niceTicks(
    Math.min(0, ...slices.map((s) => s.netCents)),
    Math.max(0, ...slices.map((s) => s.netCents)),
  );
  const lo = ticks[0];
  const hi = ticks[ticks.length - 1];
  const span = hi - lo || 1;
  const x = (v: number) => labelGutter + ((v - lo) / span) * (RIGHT - labelGutter);
  const zeroX = x(0);

  const data: ChartDatum[] = slices.map((s) => ({
    label: <DataLabel>{s.label}</DataLabel>,
    values: [s.netCents],
  }));

  return (
    <ChartFrame
      title={t('chart.byType')}
      columns={[t('bookings.net')]}
      data={data}
      height={height}
    >
      <line
        x1={zeroX}
        x2={zeroX}
        y1={4}
        y2={slices.length * rowHeight + 4}
        className="chart__baseline"
      />
      {slices.map((s, i) => {
        const y = i * rowHeight + 8;
        const left = Math.min(zeroX, x(s.netCents));
        const width = Math.abs(x(s.netCents) - zeroX);
        return (
          <g key={s.typeCode}>
            <rect
              x={left}
              y={y}
              width={Math.max(1, width)}
              height={rowHeight - 14}
              className={`chart__bar chart__bar--type chart__bar--${s.typeCode}`}
            />
            <text
              x={labelGutter - 8}
              y={y + rowHeight / 2 - 3}
              className="chart__tick chart__tick--label"
              textAnchor="end"
              {...DATA_TEXT}
            >
              {s.label}
            </text>
          </g>
        );
      })}
      {ticks.map((tick) => (
        <text
          key={tick}
          x={x(tick)}
          y={slices.length * rowHeight + 20}
          className="chart__tick"
          textAnchor="middle"
        >
          {maskAmount(formatEuroCompact(tick))}
        </text>
      ))}
    </ChartFrame>
  );
}
