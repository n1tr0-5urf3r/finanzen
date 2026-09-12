import { useCallback, useMemo, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';

import { ChartFrame } from '../../charts/ChartFrame';
import { bands, niceTicks } from '../../charts/scales';
import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { FlowMoney } from '../../components/Money';
import { Button, ErrorState, LoadingState } from '../../components/ui';
import { api, asList } from '../../lib/api';
import { formatEuroCompact, monthShort } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { useMaskAmount } from '../../lib/privacy';
import { qk } from '../../lib/queryKeys';
import type {
  Booking,
  BookingPage,
  Category,
  MonthlySeries,
  SeriesSubject,
} from '../../lib/types';

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
/** The table below scrolls the chart back into view when it changes it. */
export const SERIES_ANCHOR = 'serie';

const PARAM_MODE = 'serieTyp';
const PARAM_SUBJECT = 'serieWert';

/**
 * Which series is charted, held in the URL rather than in component state.
 *
 * Two reasons: the category table below the chart drives the same selection, and
 * lifting it into a shared parent would put the whole picker's state one level
 * away from the picker; and a chart someone wants to show another person is then
 * simply a link.
 */
export function useSeriesSelection() {
  const [params, setParams] = useSearchParams();
  const mode: 'comment' | 'category' =
    params.get(PARAM_MODE) === 'kommentar' ? 'comment' : 'category';
  const subject = params.get(PARAM_SUBJECT) ?? '';

  const select = useCallback(
    (nextMode: 'comment' | 'category', nextSubject: string) => {
      setParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          next.set(PARAM_MODE, nextMode === 'comment' ? 'kommentar' : 'kategorie');
          if (nextSubject) next.set(PARAM_SUBJECT, nextSubject);
          else next.delete(PARAM_SUBJECT);
          return next;
        },
        // Replace: picking a series is a filter, not a place. Twenty clicks
        // should not mean twenty presses of the back button to leave the page.
        { replace: true },
      );
    },
    [setParams],
  );

  return { mode, subject, select };
}

export function SeriesChart({ year, categories }: { year: number; categories: Category[] }) {
  const t = useT();
  const maskAmount = useMaskAmount();
  const { mode, subject, select } = useSeriesSelection();
  const [showBookings, setShowBookings] = useState(false);

  const subjects = useQuery({
    queryKey: qk.derived.seriesSubjects(year),
    queryFn: () => api<SeriesSubject[]>(`/analysis/series/subjects?year=${year}`),
    staleTime: 5 * 60_000,
  });

  // Default to a category, because that is the question this section is usually
  // opened with ("what did Auto & Parken cost this year"); the comment view is the
  // finer-grained follow-up. Either way the chart shows something the moment the
  // section opens, rather than an empty frame and a picker.
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

  // One filter expression, used for both the inline table and the link out, so the
  // two can never disagree about what "these bookings" means.
  const bookingFilter =
    mode === 'comment'
      ? { suche: series.data?.subject ?? active, kategorie: '' }
      : { suche: '', kategorie: active };
  const bookingsHref = `/buchungen?jahr=${year}${
    bookingFilter.suche ? `&suche=${encodeURIComponent(bookingFilter.suche)}` : ''
  }${bookingFilter.kategorie ? `&kategorie=${bookingFilter.kategorie}` : ''}`;

  const bookings = useQuery({
    queryKey: qk.bookings.list({ year, mode, subject: active, scope: 'series' }),
    queryFn: () => {
      const qs = new URLSearchParams({ year: String(year), pageSize: '50', direction: 'asc' });
      if (bookingFilter.suche) qs.set('search', bookingFilter.suche);
      if (bookingFilter.kategorie) qs.set('categoryId', bookingFilter.kategorie);
      return api<BookingPage>(`/bookings?${qs.toString()}`);
    },
    enabled: showBookings && Boolean(active),
  });

  const bars = useMemo(() => {
    const months = series.data?.months ?? [];
    // The stored convention is expense-positive: `net_cents` is expenses minus
    // income, so Gehalt is NEGATIVE and tanken is positive. That is right for
    // "what did Miete cost me" and wrong for a chart, where every reader takes a
    // bar pointing down to mean worse. Drawing is flipped so money out goes down
    // and money in goes up; the figures beside it keep the stored sign, which is
    // what the netto marker and the Gutschrift label are for.
    const plotted = months.map((m) => -m.netCents);
    const ticks = niceTicks(Math.min(0, ...plotted), Math.max(0, ...plotted));
    return { months, plotted, ticks };
  }, [series.data]);

  const height = 240;
  const lo = bars.ticks[0] ?? 0;
  const hi = bars.ticks[bars.ticks.length - 1] ?? 1;
  const span = hi - lo || 1;
  const y = (v: number) => 14 + (1 - (v - lo) / span) * (height - 52);
  const band = bands(12, LEFT, RIGHT);
  const zeroY = y(0);

  return (
    <section className="panel panel--pad series" id={SERIES_ANCHOR}>
      <header className="series__header">
        <h2>{t('analysis.seriesTitle')}</h2>
        <p className="footnote">{t('analysis.seriesHint')}</p>
      </header>

      <div className="series__controls">
        <div className="segmented" role="group" aria-label={t('analysis.seriesTitle')}>
          <button
            type="button"
            aria-pressed={mode === 'category'}
            onClick={() => select('category', '')}
          >
            {t('analysis.byCategory')}
          </button>
          <button
            type="button"
            aria-pressed={mode === 'comment'}
            onClick={() => select('comment', '')}
          >
            {t('analysis.byComment')}
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
            onChange={(e) => select(mode, e.target.value)}
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
              <FlowMoney netCents={series.data.netCents} />
            </div>
            <div>
              <span className="kpi__label">{t('analysis.perActiveMonth')}</span>
              <FlowMoney netCents={series.data.averagePerActiveMonthCents} />
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

          {/* The chart says how much; this says which. Expanding in place keeps the
              chart on screen, so a spike and the booking that caused it are readable
              together; the link is for when the full filter UI is wanted. */}
          <p className="series__drilldown">
            <Button
              variant="ghost"
              onClick={() => setShowBookings((open) => !open)}
              aria-expanded={showBookings}
            >
              {showBookings
                ? t('analysis.hideBookings')
                : t('analysis.showBookings', { count: series.data.bookingCount })}
            </Button>
            <Link to={bookingsHref}>{t('analysis.openInBookings')}</Link>
          </p>

          {showBookings && (
            <div className="series__bookings">
              {bookings.isPending && <LoadingState />}
              {bookings.isError && <ErrorState error={bookings.error} />}
              {bookings.data && (
                <table className="table table--compact">
                  <thead>
                    <tr>
                      <th>{t('common.month')}</th>
                      <th>{t('bookings.comment')}</th>
                      <th>{t('bookings.category')}</th>
                      <th className="num">{t('bookings.net')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {asList<Booking>(bookings.data.items).map((b) => (
                      <tr key={b.id}>
                        <td>
                          <DataLabel>{b.monthName}</DataLabel>
                        </td>
                        <td>
                          <DataLabel>{b.comment}</DataLabel>
                        </td>
                        <td>
                          <CategoryChip
                            name={b.categoryName}
                            typeLabel={b.categoryType}
                            fallback={t('bookings.sourceNone')}
                          />
                        </td>
                        <td className="num">
                          <FlowMoney netCents={b.netCents} />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
              {bookings.data && bookings.data.total > bookings.data.items.length && (
                <p className="footnote">
                  <Link to={bookingsHref}>
                    {t('analysis.moreBookings', {
                      shown: bookings.data.items.length,
                      total: bookings.data.total,
                    })}
                  </Link>
                </p>
              )}
            </div>
          )}

          <ChartFrame
            title={`${series.data.subject} · ${year}`}
            columns={[t('bookings.net')]}
            valueBasis="net"
            note={t('analysis.seriesOrientation')}
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
                  {maskAmount(formatEuroCompact(tick))}
                </text>
              </g>
            ))}
            {bars.months.map((m, i) => {
              // Plotted sign: positive is money in, negative is money out.
              const value = bars.plotted[i];
              const top = Math.min(y(value), zeroY);
              const barHeight = Math.max(1, Math.abs(y(value) - zeroY));
              return (
                <g key={m.month}>
                  <rect
                    x={band.start(i) + band.width * 0.15}
                    y={top}
                    width={band.width * 0.7}
                    height={barHeight}
                    className={`chart__bar ${value > 0 ? 'chart__bar--credit' : 'chart__bar--expense'}`}
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
