import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';

import { ChartFrame } from '../../charts/ChartFrame';
import { bands, niceTicks } from '../../charts/scales';
import { Money } from '../../components/Money';
import { api, asList } from '../../lib/api';
import { formatEuroCompact, monthShort } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import type { Category, MonthlySeries, SeriesSubject } from '../../lib/types';

const LEFT = 62;
const RIGHT = 710;

/**
 * One subject across twelve months.
 *
 * The category table answers "what did Auto & Parken cost this year". This
 * answers the question that actually gets asked over dinner — "how much do we
 * spend on tanken, and is it getting worse" — which is what the spreadsheet's
 * Filter tab was for.
 */
export function SeriesChart({ year, categories }: { year: number; categories: Category[] }) {
  const t = useT();
  const [mode, setMode] = useState<'comment' | 'category'>('comment');
  const [subject, setSubject] = useState<string>('');

  const subjects = useQuery({
    queryKey: qk.derived.seriesSubjects(year),
    queryFn: () => api<SeriesSubject[]>(`/analysis/series/subjects?year=${year}`),
    staleTime: 5 * 60_000,
  });

  // Default to the most-used comment, so the chart shows something the moment the
  // section is opened rather than an empty frame and a picker.
  const suggested = asList<SeriesSubject>(subjects.data);
  const active =
    subject ||
    (mode === 'comment' ? (suggested[0]?.comment ?? '') : (categories[0]?.id ?? ''));

  // The picker always offers whatever is being charted, even when the suggestion
  // list failed to load — otherwise a failed subjects request leaves a chart with
  // no way to choose anything and no explanation.
  const options =
    mode === 'comment'
      ? (suggested.length > 0 || !active
          ? suggested
          : [{ comment: active, bookingCount: 0, netCents: 0, categoryName: null }])
      : [];

  const series = useQuery({
    queryKey: qk.derived.series(year, mode, active),
    queryFn: () =>
      api<MonthlySeries>(
        mode === 'comment'
          ? `/analysis/series?year=${year}&comment=${encodeURIComponent(active)}`
          : `/analysis/series?year=${year}&categoryId=${active}`,
      ),
    enabled: Boolean(active),
  });

  const bars = useMemo(() => {
    const months = series.data?.months ?? [];
    const ticks = niceTicks(
      Math.min(0, ...months.map((m) => m.netCents)),
      Math.max(0, ...months.map((m) => m.netCents)),
    );
    return { months, ticks };
  }, [series.data]);

  const height = 240;
  const lo = bars.ticks[0] ?? 0;
  const hi = bars.ticks[bars.ticks.length - 1] ?? 1;
  const span = hi - lo || 1;
  const y = (v: number) => 14 + (1 - (v - lo) / span) * (height - 52);
  const band = bands(12, LEFT, RIGHT);
  const zeroY = y(0);

  return (
    <section className="panel panel--pad series">
      <header className="series__header">
        <h2>{t('analysis.seriesTitle')}</h2>
        <p className="footnote">{t('analysis.seriesHint')}</p>
      </header>

      <div className="series__controls">
        <div className="segmented" role="group" aria-label={t('analysis.seriesTitle')}>
          <button
            type="button"
            aria-pressed={mode === 'comment'}
            onClick={() => {
              setMode('comment');
              setSubject('');
            }}
          >
            {t('analysis.byComment')}
          </button>
          <button
            type="button"
            aria-pressed={mode === 'category'}
            onClick={() => {
              setMode('category');
              setSubject('');
            }}
          >
            {t('analysis.byCategory')}
          </button>
        </div>

        <div className="field series__picker">
          <label htmlFor="series-subject" className="sr-only">
            {mode === 'comment' ? t('bookings.comment') : t('bookings.category')}
          </label>
          <select
            id="series-subject"
            className="select"
            value={active}
            onChange={(e) => setSubject(e.target.value)}
          >
            {mode === 'comment'
              ? options.map((s) => (
                  <option key={s.comment} value={s.comment}>
                    {s.comment} ({s.bookingCount})
                  </option>
                ))
              : categories.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name}
                  </option>
                ))}
          </select>
        </div>
      </div>

      {series.data && (
        <>
          <div className="series__stats">
            <div>
              <span className="kpi__label">{t('common.total')}</span>
              <Money cents={series.data.netCents} basis="net" tone="auto" />
            </div>
            <div>
              <span className="kpi__label">{t('analysis.perActiveMonth')}</span>
              <Money cents={series.data.averagePerActiveMonthCents} basis="net" tone="auto" />
            </div>
            <div>
              <span className="kpi__label">{t('analysis.count')}</span>
              <span className="num">{series.data.bookingCount}</span>
            </div>
            <div>
              <span className="kpi__label">{t('months.withData')}</span>
              <span className="num">{series.data.monthsWithData}</span>
            </div>
          </div>

          <ChartFrame
            title={`${series.data.subject} · ${year}`}
            columns={[t('bookings.net')]}
            data={bars.months.map((m) => ({
              label: m.monthName,
              values: [m.netCents],
            }))}
            height={height}
          >
            {bars.ticks.map((tick) => (
              <g key={tick}>
                <line x1={LEFT} x2={RIGHT} y1={y(tick)} y2={y(tick)} className="chart__grid" />
                <text x={LEFT - 8} y={y(tick) + 4} className="chart__tick" textAnchor="end">
                  {formatEuroCompact(tick)}
                </text>
              </g>
            ))}
            {bars.months.map((m, i) => {
              const value = m.netCents;
              const top = Math.min(y(value), zeroY);
              const barHeight = Math.max(1, Math.abs(y(value) - zeroY));
              return (
                <g key={m.month}>
                  <rect
                    x={band.start(i) + band.width * 0.15}
                    y={top}
                    width={band.width * 0.7}
                    height={barHeight}
                    className={`chart__bar ${value < 0 ? 'chart__bar--credit' : 'chart__bar--expense'}`}
                  />
                  <text
                    x={band.centre(i)}
                    y={height - 18}
                    className={`chart__tick ${m.bookingCount === 0 ? 'chart__tick--dim' : ''}`}
                    textAnchor="middle"
                  >
                    {monthShort(m.month)}
                  </text>
                </g>
              );
            })}
            <line x1={LEFT} x2={RIGHT} y1={zeroY} y2={zeroY} className="chart__baseline" />
          </ChartFrame>
        </>
      )}
    </section>
  );
}
