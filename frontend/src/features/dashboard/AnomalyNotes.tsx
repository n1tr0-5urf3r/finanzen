import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';

import { CategoryChip } from '../../components/DataLabel';
import { Money } from '../../components/Money';
import { api, asList } from '../../lib/api';
import { formatPercent } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import type { Anomaly, AnomalyReport } from '../../lib/types';

/**
 * Categories a long way from their own median, and nothing else.
 *
 * The point is what it does NOT do. No budgets to maintain, no thresholds to
 * configure, no permanent badge counting things that are fine — and when nothing
 * is unusual, which is most months, it renders nothing at all rather than an empty
 * panel saying so. A notice that appears every day is one nobody reads on the day
 * it matters.
 */
export function AnomalyNotes({ year, month }: { year: number; month: number }) {
  const t = useT();

  const query = useQuery({
    queryKey: qk.derived.anomalies(year, month),
    queryFn: () => api<AnomalyReport>(`/analysis/anomalies?year=${year}&month=${month}`),
    staleTime: 5 * 60_000,
  });

  const items = asList<Anomaly>(query.data?.items);
  // Errors are swallowed on purpose: this is a nicety beside the figures, and a
  // red box where a quiet note would have been is worse than no note.
  if (query.isError || items.length === 0) return null;

  return (
    <section className="panel panel--pad anomalies" style={{ marginBottom: '1rem' }}>
      <header>
        <h2>{t('anomalies.title')}</h2>
        <p className="footnote">
          {t('anomalies.basis', {
            month: query.data?.monthName ?? '',
            months: query.data?.comparedMonths ?? 0,
          })}
        </p>
      </header>

      <ul className="anomalies__list">
        {items.map((a) => (
          <li key={a.categoryName}>
            <CategoryChip
              name={a.categoryName}
              typeLabel={a.categoryType}
              fallback={t('bookings.sourceNone')}
            />
            <span className="anomalies__figures">
              {/* Cost convention, as everywhere on this screen: positive is what
                  it cost. The word says the direction, so the colour never has to
                  carry it alone. */}
              <Money cents={a.currentCents} basis="net" tone="auto" />
              <span className="footnote">
                {t(a.direction === 'above' ? 'anomalies.above' : 'anomalies.below', {
                  percent: formatPercent(Math.abs(a.ratio - 1)),
                })}
              </span>
            </span>
          </li>
        ))}
      </ul>

      <p className="footnote">
        <Link to={`/auswertung?jahr=${year}`}>{t('anomalies.openAnalysis')}</Link>
      </p>
    </section>
  );
}
