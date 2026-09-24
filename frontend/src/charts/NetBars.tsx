import { ChartFrame, Gridlines, type ChartDatum } from './ChartFrame';
import { bands, niceTicks } from './scales';
import { DataLabel } from '../components/DataLabel';
import { formatEuroCompact } from '../lib/format';
import { useMaskAmount } from '../lib/privacy';
import { useT } from '../lib/i18n';
import { useNarrow } from '../lib/useNarrow';

/* The axis gutter follows the font size the stylesheet picks. German has no short
   form for thousands, so "10.000 €" is eight characters either way — at the
   phone's 22-unit text that needs half again as much room as at 13. */
const LEFT = { wide: 76, narrow: 118 };
const RIGHT = 710;

export interface NetBar {
  /** A year, a month — whatever the bands are. Data, so it is never translated. */
  label: string;
  short: string;
  /** Stored expense-positive convention: positive is what it cost. */
  netCents: number;
  /** A band the subject has no bookings in at all, as opposed to one that nets to
      zero. Drawn as nothing rather than as a zero-height bar on the baseline. */
  hasData: boolean;
}

/**
 * One subject across a row of bands, drawn as a flow.
 *
 * The stored convention is expense-positive, and this chart flips it: money out
 * points DOWN, money in points UP, which is the direction the rest of the app
 * reads. A category whose net goes negative in one year — a reimbursement year, a
 * refunded deposit — therefore rises above the line instead of disappearing, which
 * is the whole reason this is not a plain magnitude chart.
 */
export function NetBars({
  title,
  note,
  bars,
  height = 220,
  tableOnPhone = true,
}: {
  title: string;
  note?: string;
  bars: NetBar[];
  height?: number;
  tableOnPhone?: boolean;
}) {
  const mask = useMaskAmount();
  const t = useT();
  const left = LEFT[useNarrow() ? 'narrow' : 'wide'];
  const bottom = height - 26;
  const flow = bars.map((b) => -b.netCents);
  const ticks = niceTicks(Math.min(0, ...flow), Math.max(0, ...flow));
  const lo = ticks[0] ?? 0;
  const hi = ticks[ticks.length - 1] ?? 1;
  const span = hi - lo || 1;
  const y = (v: number) => 12 + (1 - (v - lo) / span) * (bottom - 12);
  const band = bands(bars.length, left, RIGHT);
  const width = Math.max(4, band.width * 0.62);
  const zeroY = y(0);

  const data: ChartDatum[] = bars.map((b) => ({
    label: <DataLabel>{b.label}</DataLabel>,
    values: [b.hasData ? b.netCents : null],
  }));

  return (
    <ChartFrame
      title={title}
      note={note}
      columns={[t('bookings.net')]}
      data={data}
      valueBasis="net"
      height={height}
      tableOnPhone={tableOnPhone}
    >
      <Gridlines ticks={ticks} y={y} left={left} right={RIGHT} format={formatEuroCompact} />
      {bars.map((b, i) => {
        const value = flow[i] as number;
        const top = Math.min(y(value), zeroY);
        return (
          <g key={b.label}>
            {b.hasData && (
              <rect
                x={band.centre(i) - width / 2}
                y={top}
                width={width}
                height={Math.max(1, Math.abs(y(value) - zeroY))}
                className={`chart__bar ${value >= 0 ? 'chart__bar--credit' : 'chart__bar--expense'}`}
              >
                <title>{`${b.label}: ${mask(formatEuroCompact(Math.abs(b.netCents)))}`}</title>
              </rect>
            )}
            <text
              x={band.centre(i)}
              y={bottom + 15}
              className={b.hasData ? 'chart__tick' : 'chart__tick chart__tick--dim'}
              textAnchor="middle"
            >
              {b.short}
            </text>
          </g>
        );
      })}
    </ChartFrame>
  );
}
