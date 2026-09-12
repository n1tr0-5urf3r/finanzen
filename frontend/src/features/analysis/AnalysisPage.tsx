import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link, useSearchParams } from 'react-router-dom';

import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { Money, NetBreakdown, ScopeNote } from '../../components/Money';
import { Banner, EmptyState, ErrorState, LoadingState, PageHeader, StatusPill } from '../../components/ui';
import { api } from '../../lib/api';
import { formatPercent, monthShort } from '../../lib/format';
import { YearPicker } from '../../components/YearPicker';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import type { MessageKey } from '../../lib/messages/de';
import type { CategoryAnalysis, CategoryAnalysisRow } from '../../lib/types';

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

export function AnalysisPage() {
  const t = useT();
  const [params, setParams] = useSearchParams();
  const year = Number(params.get('jahr')) || new Date().getFullYear();
  const [sort, setSort] = useState<{ key: SortKey; desc: boolean }>({ key: 'net', desc: true });

  const query = useQuery({
    queryKey: qk.derived.categories(year),
    queryFn: () => api<CategoryAnalysis>(`/analysis/categories?year=${year}`),
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

  return (
    <>
      <PageHeader title={t('analysis.title')} subtitle={t('analysis.intro')} />

      <div
        className="panel panel--pad"
        style={{ marginBottom: '1rem', display: 'flex', gap: '.75rem', flexWrap: 'wrap', alignItems: 'flex-end' }}
      >
        <div style={{ minWidth: '8rem' }}>
          <YearPicker id="analysis-year" value={year} onChange={(next) =>
            setParams(
              (prev) => {
                const p = new URLSearchParams(prev);
                p.set('jahr', String(next));
                return p;
              },
              { replace: true },
            )} />
        </div>
        <ScopeNote transfersIncluded={false} />
      </div>

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
                  <tr key={row.categoryId ?? row.categoryName}>
                    <th scope="row">
                      <CategoryChip
                        name={row.categoryName}
                        typeLabel={row.categoryType}
                        fallback={t('bookings.sourceNone')}
                      />
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
                      <Money cents={row.averagePerMonthCents} basis="net" tone="auto" />
                    </td>
                    <td className="num">{row.bookingCount}</td>
                    {row.monthlyNetCents.map((cents, i) => (
                      <td key={i} className="num">
                        {cents === 0 ? (
                          <span className="money money--empty">–</span>
                        ) : (
                          <Money cents={cents} basis="net" tone="auto" />
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
                    <Money cents={query.data.totalNetCents} basis="net" tone="auto" />
                  </td>
                  <td />
                  <td />
                  <td className="num">{rows.reduce((s, r) => s + r.bookingCount, 0)}</td>
                  {Array.from({ length: 12 }, (_, i) => (
                    <td key={i} className="num">
                      <Money
                        cents={rows.reduce((s, r) => s + (r.monthlyNetCents[i] ?? 0), 0)}
                        basis="net"
                        tone="auto"
                      />
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
                    <CategoryChip
                      name={row.categoryName}
                      typeLabel={row.categoryType}
                      fallback={t('bookings.sourceNone')}
                    />
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
                      <Money cents={row.netCents} basis="net" tone="auto" />
                    </dd>
                  </div>
                  <div>
                    <dt>{t('analysis.perMonth')}</dt>
                    <dd>
                      <Money cents={row.averagePerMonthCents} basis="net" tone="auto" />
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
