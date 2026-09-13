import { useQuery } from '@tanstack/react-query';

import { ChartFrame, Gridlines } from '../../charts/ChartFrame';
import { bands, linearScale } from '../../charts/scales';
import { DataLabel } from '../../components/DataLabel';
import { FlowMoney, Money } from '../../components/Money';
import { ErrorState } from '../../components/ui';
import { api, asList } from '../../lib/api';
import { formatEuro, formatEuroCompact, monthShort } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { useMaskAmount } from '../../lib/privacy';
import { qk } from '../../lib/queryKeys';
import type { Forecast, ForecastBasisRow, ForecastMonth } from '../../lib/types';

const LEFT = 58;
const RIGHT = 710;
const TOP = 12;

/**
 * Where the year ends up, from what is already known.
 *
 * The recurring templates say exactly what the fixed side of every remaining month
 * costs; the last six months say what each category typically adds on top. So this
 * needs no budget to be maintained and no input at all — but it is a PROJECTION,
 * and the one thing it must never do is look like a fact. Projected months are
 * drawn dashed, inside a band, and labelled; actual months are solid.
 */
export function ForecastPanel({ year }: { year: number }) {
  const t = useT();
  const maskAmount = useMaskAmount();

  const query = useQuery({
    queryKey: qk.derived.forecast(year),
    queryFn: () => api<Forecast>(`/analysis/forecast?year=${year}`),
    staleTime: 5 * 60_000,
  });

  if (query.isError) return <ErrorState error={query.error} />;
  const d = query.data;
  // Nothing to say about a year that is already complete, or one with no bookings
  // at all: a forecast of nothing is noise on the screen opened every day.
  // A falsy month covers all three ways there is nothing to draw: a complete year
  // (null), a year with no bookings, and a payload that is not the expected shape.
  const months = asList<ForecastMonth>(d?.months);
  if (!d || !d.projectedFromMonth || months.length === 0) return null;
  const rows = asList<ForecastBasisRow>(d.rows).slice(0, 6);
  const height = 220;
  const bottom = height - 26;

  // The band is the range, so the scale has to contain it or the picture would be
  // narrower than the uncertainty it is drawing.
  const scale = linearScale(
    months.flatMap((m) => [
      m.closingBalanceCents,
      m.isProjected ? m.closingBalanceCents - spreadThrough(months, m.month) : m.closingBalanceCents,
      m.isProjected ? m.closingBalanceCents + spreadThrough(months, m.month) : m.closingBalanceCents,
    ]),
    TOP,
    bottom,
  );
  const band = bands(months.length, LEFT, RIGHT);

  const actual = months.filter((m) => !m.isProjected);
  const projected = months.filter((m) => m.isProjected);
  // The projected line starts at the last actual point, or it would float
  // disconnected from the balance it continues.
  const joined = actual.length > 0 ? [actual[actual.length - 1], ...projected] : projected;

  const line = (list: ForecastMonth[]) =>
    list
      .map(
        (m, n) =>
          `${n === 0 ? 'M' : 'L'}${band.centre(m.month - 1).toFixed(1)},${scale
            .y(m.closingBalanceCents)
            .toFixed(1)}`,
      )
      .join(' ');

  const bandPath = (() => {
    if (joined.length < 2) return '';
    const top = joined
      .map(
        (m, n) =>
          `${n === 0 ? 'M' : 'L'}${band.centre(m.month - 1).toFixed(1)},${scale
            .y(m.closingBalanceCents + spreadThrough(months, m.month))
            .toFixed(1)}`,
      )
      .join(' ');
    const back = [...joined]
      .reverse()
      .map(
        (m) =>
          `L${band.centre(m.month - 1).toFixed(1)},${scale
            .y(m.closingBalanceCents - spreadThrough(months, m.month))
            .toFixed(1)}`,
      )
      .join(' ');
    return `${top} ${back} Z`;
  })();

  return (
    <section className="panel panel--pad forecast" style={{ marginBottom: '1rem' }}>
      <header className="forecast__header">
        <h2>{t('forecast.title')}</h2>
        <p className="footnote">
          {t('forecast.basis', {
            months: d.historyMonths,
            templates: d.dueTemplateCount,
            from: months.find((m) => m.month === d.projectedFromMonth)?.monthName ?? '—',
          })}
        </p>
      </header>

      <div className="forecast__figures">
        <div>
          <span className="kpi__label">{t('forecast.closing')}</span>
          <span className="kpi__value">
            <FlowMoney flowCents={d.projectedClosingBalanceCents} />
          </span>
          <span className="kpi__scope">
            {t('forecast.range', {
              low: maskAmount(formatEuro(d.projectedClosingLowCents)),
              high: maskAmount(formatEuro(d.projectedClosingHighCents)),
            })}
          </span>
        </div>
        <div>
          <span className="kpi__label">{t('forecast.actualSoFar')}</span>
          <span className="kpi__value">
            <FlowMoney flowCents={d.actualBalanceCents} />
          </span>
          <span className="kpi__scope">
            {t('forecast.throughMonth', {
              month: months.find((m) => m.month === d.actualThroughMonth)?.monthName ?? '—',
            })}
          </span>
        </div>
        <div>
          <span className="kpi__label">{t('forecast.remaining')}</span>
          <span className="kpi__value">
            <FlowMoney flowCents={d.projectedBalanceCents} />
          </span>
          <span className="kpi__scope">{t('forecast.projectedLabel')}</span>
        </div>
      </div>

      <ChartFrame
        title={t('forecast.chartTitle', { year })}
        note={t('forecast.chartNote')}
        columns={[t('months.cumulative')]}
        valueBasis="flow"
        data={months.map((m) => ({
          label: (
            <>
              <DataLabel>{m.monthName}</DataLabel>
              {m.isProjected && <span className="footnote"> · {t('forecast.projectedShort')}</span>}
            </>
          ),
          values: [m.closingBalanceCents],
        }))}
        height={height}
      >
        <Gridlines
          ticks={scale.ticks}
          y={scale.y}
          left={LEFT}
          right={RIGHT}
          format={formatEuroCompact}
        />
        {bandPath && <path d={bandPath} className="forecast__band" />}
        {actual.length > 1 && <path d={line(actual)} className="chart__line" />}
        {joined.length > 1 && <path d={line(joined)} className="chart__line forecast__line" />}
        {months.map((m) => (
          <circle
            key={m.month}
            cx={band.centre(m.month - 1)}
            cy={scale.y(m.closingBalanceCents)}
            r={3}
            className={m.isProjected ? 'chart__dot forecast__dot' : 'chart__dot'}
          />
        ))}
        {months.map((m) => (
          <text
            key={m.month}
            x={band.centre(m.month - 1)}
            y={bottom + 15}
            className={m.isProjected ? 'chart__tick chart__tick--dim' : 'chart__tick'}
            textAnchor="middle"
          >
            {monthShort(m.month)}
          </text>
        ))}
      </ChartFrame>

      {rows.length > 0 && (
        <div className="table-wrap" style={{ marginTop: '.75rem' }}>
          <table className="data-table">
            <caption className="footnote">{t('forecast.rowsCaption')}</caption>
            <thead>
              <tr>
                <th>{t('bookings.category')}</th>
                <th className="num">{t('forecast.median')}</th>
                <th className="num">{t('forecast.projectedTotal')}</th>
                <th>{t('forecast.source')}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.categoryName}>
                  <td>
                    <DataLabel>{r.categoryName}</DataLabel>
                  </td>
                  {/* Cost convention, like the other two tables on this screen:
                      positive is what it costs. */}
                  <td className="num">
                    <Money cents={r.medianCents} basis="net" tone="auto" />
                  </td>
                  <td className="num">
                    <Money cents={r.projectedTotalCents} basis="net" tone="auto" />
                  </td>
                  <td>{t(sourceKey(r.source))}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

/** Three fixed keys, so the dictionary stays typed and a typo is a build error. */
function sourceKey(source: string): 'forecast.sourceTemplate' | 'forecast.sourceMedian' | 'forecast.sourceMixed' {
  if (source === 'template') return 'forecast.sourceTemplate';
  if (source === 'mixed') return 'forecast.sourceMixed';
  return 'forecast.sourceMedian';
}

/**
 * Uncertainty accumulates: December's balance carries every projected month's
 * spread before it, not just its own.
 */
function spreadThrough(months: ForecastMonth[], month: number): number {
  return months
    .filter((m) => m.isProjected && m.month <= month)
    .reduce((sum, m) => sum + m.spreadCents, 0);
}
