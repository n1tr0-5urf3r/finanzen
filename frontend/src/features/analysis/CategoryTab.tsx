import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';

import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { FlowMoney, Money, NetBreakdown } from '../../components/Money';
import { Banner, EmptyState, ErrorState, LoadingState, StatusPill } from '../../components/ui';
import { api, asList } from '../../lib/api';
import { formatPercent, monthShort } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import { SeriesChart, SERIES_ANCHOR, useSeriesSelection } from './SeriesChart';
import type { MessageKey } from '../../lib/messages/de';
import type { Category, CategoryAnalysis, CategoryAnalysisRow } from '../../lib/types';
import { sortedByName } from '../../lib/categories';

type SortKey = 'name' | 'type' | 'income' | 'expense' | 'net' | 'share' | 'average' | 'count';

const SORTERS: Record<SortKey, (a: CategoryAnalysisRow, b: CategoryAnalysisRow) => number> = {
  name: (a, b) => a.categoryName.localeCompare(b.categoryName, 'de'),
  type: (a, b) => (a.categoryType ?? '').localeCompare(b.categoryType ?? '', 'de'),
  income: (a, b) => a.incomeCents - b.incomeCents,
  expense: (a, b) => a.expenseCents - b.expenseCents,
  net: (a, b) => a.netCents - b.netCents,
  share: (a, b) => a.shareOfTotal - b.shareOfTotal,
  average: (a, b) => a.averagePerMonthCents - b.averagePerMonthCents,
  count: (a, b) => a.bookingCount - b.bookingCount,
};

const COLUMNS: { key: SortKey; labelKey: MessageKey; numeric: boolean }[] = [
  { key: 'name', labelKey: 'bookings.category', numeric: false },
  { key: 'type', labelKey: 'bookings.type', numeric: false },
  { key: 'income', labelKey: 'bookings.income', numeric: true },
  { key: 'expense', labelKey: 'bookings.expense', numeric: true },
  { key: 'net', labelKey: 'bookings.net', numeric: true },
  { key: 'share', labelKey: 'analysis.share', numeric: true },
  { key: 'average', labelKey: 'analysis.perMonth', numeric: true },
  { key: 'count', labelKey: 'analysis.count', numeric: true },
];

/**
 * What each category cost this year, and the months behind any one of them.
 *
 * The year and the page furniture belong to `AnalysisPage`; this is only the tab.
 */
export function CategoryTab({ year }: { year: number }) {
  const t = useT();
  const [sort, setSort] = useState<{ key: SortKey; desc: boolean }>({ key: 'net', desc: true });

  const query = useQuery({
    queryKey: qk.derived.categories(year),
    queryFn: () => api<CategoryAnalysis>(`/analysis/categories?year=${year}`),
  });
  // The picker needs the real category list, not the analysis rows: a category
  // with no bookings this year is still a legitimate thing to chart.
  const categories = useQuery({
    queryKey: qk.taxonomy.categories(),
    queryFn: () => api<Category[]>('/categories'),
    staleTime: 30 * 60_000,
  });

  const rows = useMemo(() => {
    const source = query.data?.rows ?? [];
    const sorted = [...source].sort(SORTERS[sort.key]);
    return sort.desc ? sorted.reverse() : sorted;
  }, [query.data, sort]);

  function toggle(key: SortKey) {
    // Numbers open large-first, names A→Z: the useful direction for each.
    setSort((prev) =>
      prev.key === key
        ? { key, desc: !prev.desc }
        : { key, desc: key !== 'name' && key !== 'type' },
    );
  }

  const anyCredit = rows.some((r) => r.netIsNegative);

  // Clicking a category in the table charts it. The chart sits above the table,
  // so a click at row 25 would otherwise change something the user cannot see.
  const { mode: seriesMode, subject: seriesSubject, select: selectSeries } = useSeriesSelection();
  function chartCategory(categoryId: string) {
    selectSeries('category', categoryId);
    // Optional call: jsdom has no scrollIntoView, and a missing scroll must not
    // take the selection down with it.
    document.getElementById(SERIES_ANCHOR)?.scrollIntoView?.({ behavior: 'smooth', block: 'start' });
  }
  const charted = (categoryId: string | null) =>
    seriesMode === 'category' && categoryId !== null && categoryId === seriesSubject;

  return (
    <>
      {query.isLoading && <LoadingState />}
      {query.isError && <ErrorState error={query.error} retry={() => query.refetch()} />}

      {query.data && rows.length === 0 && <EmptyState hint={t('analysis.empty')} />}

      {query.data && rows.length > 0 && (
        <>
          {query.data.excludedTransferCount > 0 && (
            <Banner tone="info">
              {t('analysis.transfersExcluded', { count: query.data.excludedTransferCount })}
            </Banner>
          )}
          {query.data.uncategorizedCount > 0 && (
            <Banner tone="warn">
              <Link to={`/buchungen?jahr=${year}&ohneKategorie=1`}>
                {t('analysis.uncategorized', { count: query.data.uncategorizedCount })}
              </Link>
            </Banner>
          )}

          {/* Above the table, because "how much do I spend on tanken" is the
              question people come here with; the year's totals are what they
              scroll to afterwards. */}
          <SeriesChart year={year} categories={sortedByName(asList<Category>(categories.data))} />

          <div className="panel table-wrap screen-table">
            <table className="data-table">
              <thead>
                <tr>
                  {COLUMNS.map((col) => (
                    <th
                      key={col.key}
                      className={col.numeric ? 'num' : undefined}
                      aria-sort={
                        sort.key === col.key
                          ? sort.desc
                            ? 'descending'
                            : 'ascending'
                          : 'none'
                      }
                    >
                      <button
                        type="button"
                        className="th-sort"
                        onClick={() => toggle(col.key)}
                        title={t('analysis.sortBy', { column: t(col.labelKey) })}
                      >
                        {t(col.labelKey)}
                        {sort.key === col.key && (
                          <span className="th-sort__arrow" aria-hidden="true">
                            {sort.desc ? '▼' : '▲'}
                          </span>
                        )}
                      </button>
                    </th>
                  ))}
                  {Array.from({ length: 12 }, (_, i) => (
                    <th key={i} className="num month-cell">
                      <DataLabel>{monthShort(i + 1)}</DataLabel>
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => (
                  <tr
                    key={row.categoryId ?? row.categoryName}
                    className={charted(row.categoryId) ? 'is-charted' : undefined}
                  >
                    <th scope="row">
                      {row.categoryId ? (
                        <button
                          type="button"
                          className="linkish"
                          aria-pressed={charted(row.categoryId)}
                          title={t('analysis.chartThis')}
                          onClick={() => chartCategory(row.categoryId as string)}
                        >
                          <CategoryChip
                            name={row.categoryName}
                            typeLabel={row.categoryType}
                            fallback={t('bookings.sourceNone')}
                          />
                        </button>
                      ) : (
                        <CategoryChip
                          name={row.categoryName}
                          typeLabel={row.categoryType}
                          fallback={t('bookings.sourceNone')}
                        />
                      )}
                      {row.incomeCents > 0 && (
                        <StatusPill tone="info">{t('money.containsRefunds')}</StatusPill>
                      )}
                      {/* The single most misreadable cell in the app, said outright. */}
                      {row.netIsNegative && (
                        <StatusPill tone="good">{t('money.credit')}</StatusPill>
                      )}
                    </th>
                    <td>
                      {row.categoryType ? <DataLabel>{row.categoryType}</DataLabel> : null}
                    </td>
                    <td className="num">
                      <Money cents={row.incomeCents} tone="income" />
                    </td>
                    <td className="num">
                      <Money cents={row.expenseCents} tone="expense" />
                    </td>
                    <td className="num">
                      <NetBreakdown
                        incomeCents={row.incomeCents}
                        expenseCents={row.expenseCents}
                        netCents={row.netCents}
                        bookingCount={row.bookingCount}
                      />
                    </td>
                    <td className="num">
                      {/* Deliberately 0 for a credit, and marked rather than blank,
                          so it reads as an answer instead of a missing number. */}
                      {row.netIsNegative ? (
                        <abbr title={t('analysis.shareNote')}>—</abbr>
                      ) : (
                        formatPercent(row.shareOfTotal)
                      )}
                    </td>
                    <td className="num">
                      <FlowMoney netCents={row.averagePerMonthCents} />
                    </td>
                    <td className="num">{row.bookingCount}</td>
                    {row.monthlyNetCents.map((cents, i) => (
                      <td key={i} className="num">
                        {cents === 0 ? (
                          <span className="money money--empty">–</span>
                        ) : (
                          <FlowMoney netCents={cents} />
                        )}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
              <tfoot>
                <tr>
                  <th scope="row">{t('analysis.totalRow')}</th>
                  <td />
                  <td className="num">
                    <Money cents={rows.reduce((s, r) => s + r.incomeCents, 0)} tone="income" />
                  </td>
                  <td className="num">
                    <Money cents={rows.reduce((s, r) => s + r.expenseCents, 0)} tone="expense" />
                  </td>
                  <td className="num">
                    <FlowMoney netCents={query.data.totalNetCents} />
                  </td>
                  <td />
                  <td />
                  <td className="num">{rows.reduce((s, r) => s + r.bookingCount, 0)}</td>
                  {Array.from({ length: 12 }, (_, i) => (
                    <td key={i} className="num">
                      <FlowMoney netCents={rows.reduce((s, r) => s + (r.monthlyNetCents[i] ?? 0), 0)} />
                    </td>
                  ))}
                </tr>
              </tfoot>
            </table>
          </div>

          <div className="screen-cards">
            {rows.map((row) => (
              <article key={row.categoryId ?? row.categoryName} className="mcard">
                <header>
                  <strong>
                    {row.categoryId ? (
                      <button
                        type="button"
                        className="linkish"
                        aria-pressed={charted(row.categoryId)}
                        title={t('analysis.chartThis')}
                        onClick={() => chartCategory(row.categoryId as string)}
                      >
                        <CategoryChip
                          name={row.categoryName}
                          typeLabel={row.categoryType}
                          fallback={t('bookings.sourceNone')}
                        />
                      </button>
                    ) : (
                      <CategoryChip
                        name={row.categoryName}
                        typeLabel={row.categoryType}
                        fallback={t('bookings.sourceNone')}
                      />
                    )}
                  </strong>
                  <span className="kpi__scope">
                    {row.bookingCount} {t('analysis.count')}
                  </span>
                </header>
                {(row.incomeCents > 0 || row.netIsNegative) && (
                  <p style={{ marginBottom: '.4rem', display: 'flex', gap: '.3rem', flexWrap: 'wrap' }}>
                    {row.incomeCents > 0 && (
                      <StatusPill tone="info">{t('money.containsRefunds')}</StatusPill>
                    )}
                    {row.netIsNegative && <StatusPill tone="good">{t('money.credit')}</StatusPill>}
                  </p>
                )}
                <dl className="mcard__grid">
                  <div>
                    <dt>{t('bookings.expense')}</dt>
                    <dd>
                      <Money cents={row.expenseCents} tone="expense" />
                    </dd>
                  </div>
                  <div>
                    <dt>{t('bookings.income')}</dt>
                    <dd>
                      <Money cents={row.incomeCents} tone="income" />
                    </dd>
                  </div>
                  <div>
                    <dt>{t('bookings.net')}</dt>
                    <dd>
                      <FlowMoney netCents={row.netCents} />
                    </dd>
                  </div>
                  <div>
                    <dt>{t('analysis.perMonth')}</dt>
                    <dd>
                      <FlowMoney netCents={row.averagePerMonthCents} />
                    </dd>
                  </div>
                  <div>
                    <dt>{t('analysis.share')}</dt>
                    <dd>
                      {row.netIsNegative ? '—' : formatPercent(row.shareOfTotal)}
                    </dd>
                  </div>
                </dl>
              </article>
            ))}
          </div>

          <p className="footnote">{t('analysis.shareNote')}</p>
          {anyCredit && <p className="footnote">{t('analysis.creditNote')}</p>}
          <p className="footnote">{t('analysis.monthlyColumns')}</p>
        </>
      )}
    </>
  );
}
