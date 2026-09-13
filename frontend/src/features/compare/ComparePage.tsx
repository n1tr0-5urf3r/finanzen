import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useSearchParams } from 'react-router-dom';

import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { FlowMoney, Money, ScopeNote } from '../../components/Money';
import {
  Banner,
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  StatusPill,
} from '../../components/ui';
import { YearPicker } from '../../components/YearPicker';
import { api, asList } from '../../lib/api';
import { formatPercent } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import { GroupedMonthBars } from '../../charts/GroupedMonthBars';
import { TrailingChart } from './TrailingChart';
import type { MessageKey } from '../../lib/messages/de';
import type {
  CompareRow,
  CompareTypeRow,
  TrailingWindow,
  YearComparison,
} from '../../lib/types';

type SortKey = 'name' | 'type' | 'current' | 'previous' | 'delta' | 'ratio';

/** Which pair of figures a row is being read through. */
type Basis = 'comparable' | 'raw';

function pick(row: CompareRow, basis: Basis) {
  return basis === 'comparable'
    ? {
        net: row.comparableNetCents,
        previous: row.comparablePreviousNetCents,
        delta: row.comparableDeltaCents,
        ratio: row.comparableDeltaRatio,
      }
    : {
        net: row.netCents,
        previous: row.previousNetCents,
        delta: row.deltaCents,
        ratio: row.deltaRatio,
      };
}

const SORTERS: Record<SortKey, (basis: Basis) => (a: CompareRow, b: CompareRow) => number> = {
  name: () => (a, b) => a.categoryName.localeCompare(b.categoryName, 'de'),
  type: () => (a, b) => (a.categoryType ?? '').localeCompare(b.categoryType ?? '', 'de'),
  current: (basis) => (a, b) => pick(a, basis).net - pick(b, basis).net,
  previous: (basis) => (a, b) => pick(a, basis).previous - pick(b, basis).previous,
  // By magnitude: the question is what MOVED, and a big drop is as interesting as
  // a big rise.
  delta: (basis) => (a, b) => Math.abs(pick(a, basis).delta) - Math.abs(pick(b, basis).delta),
  ratio: (basis) => (a, b) =>
    Math.abs(pick(a, basis).ratio ?? 0) - Math.abs(pick(b, basis).ratio ?? 0),
};

const COLUMNS: { key: SortKey; labelKey: MessageKey; numeric: boolean }[] = [
  { key: 'name', labelKey: 'bookings.category', numeric: false },
  { key: 'type', labelKey: 'bookings.type', numeric: false },
  { key: 'current', labelKey: 'compare.thisYear', numeric: true },
  { key: 'previous', labelKey: 'compare.lastYear', numeric: true },
  { key: 'delta', labelKey: 'compare.change', numeric: true },
  { key: 'ratio', labelKey: 'compare.changePercent', numeric: true },
];

/**
 * This year against last.
 *
 * Two thirds of this ledger is history — 1.404 bookings from 2023 to 2025 — and
 * every other screen looks at one calendar year, so until now that history was
 * imported and then invisible.
 *
 * The screen is built around the one way a comparison lies: a part year against a
 * full one. Nine months of spending is 25 % less than twelve months of the same
 * spending, and nothing about that is an improvement. So when the two years cover
 * different months the restricted figures lead, the raw ones are one click away,
 * and the banner says which is which rather than leaving it to be noticed.
 */
export function ComparePage() {
  const t = useT();
  const [params, setParams] = useSearchParams();
  const year = Number(params.get('jahr')) || new Date().getFullYear();
  const [sort, setSort] = useState<{ key: SortKey; desc: boolean }>({ key: 'delta', desc: true });
  const [basisOverride, setBasisOverride] = useState<Basis | null>(null);
  // Which category the month-by-month pair is drawn for. Empty means "the one at
  // the top of the table", so the chart answers something before it is touched.
  const [charted, setCharted] = useState<string>('');

  const query = useQuery({
    queryKey: qk.derived.compare(year),
    queryFn: () => api<YearComparison>(`/analysis/compare?year=${year}`),
  });

  // The window ends where the year's data ends: a trailing twelve months that ran
  // to December of a year still in progress would be four empty months of nothing.
  const endMonth = query.data?.current.lastMonthWithData ?? 12;
  const trailing = useQuery({
    queryKey: qk.derived.trailing(year, endMonth),
    queryFn: () => api<TrailingWindow>(`/analysis/trailing?year=${year}&month=${endMonth}`),
    enabled: query.isSuccess,
  });

  const data = query.data;
  const fully = data?.fullyComparable ?? true;
  const basis: Basis = basisOverride ?? (fully ? 'raw' : 'comparable');

  const rows = useMemo(() => {
    const source = asList<CompareRow>(data?.rows);
    const sorted = [...source].sort(SORTERS[sort.key](basis));
    return sort.desc ? sorted.reverse() : sorted;
  }, [data, sort, basis]);

  function toggle(key: SortKey) {
    setSort((prev) =>
      prev.key === key
        ? { key, desc: !prev.desc }
        : { key, desc: key !== 'name' && key !== 'type' },
    );
  }

  // Falls back to the first row rather than to nothing, and survives a year change
  // that no longer contains the chosen category.
  const chartedRow =
    rows.find((r) => (r.categoryId ?? r.categoryName) === charted) ?? rows[0] ?? null;

  const totals = (() => {
    if (!data) return null;
    return basis === 'comparable'
      ? {
          current: data.current.comparableSaldoCents,
          previous: data.previous.comparableSaldoCents,
          currentExpense: data.current.comparableExpenseCents,
          previousExpense: data.previous.comparableExpenseCents,
          currentIncome: data.current.comparableIncomeCents,
          previousIncome: data.previous.comparableIncomeCents,
        }
      : {
          current: data.current.saldoCents,
          previous: data.previous.saldoCents,
          currentExpense: data.current.expenseCents,
          previousExpense: data.previous.expenseCents,
          currentIncome: data.current.incomeCents,
          previousIncome: data.previous.incomeCents,
        };
  })();

  return (
    <>
      <PageHeader title={t('compare.title')} subtitle={t('compare.intro')} />

      <div
        className="panel panel--pad"
        style={{
          marginBottom: '1rem',
          display: 'flex',
          gap: '.75rem',
          flexWrap: 'wrap',
          alignItems: 'flex-end',
        }}
      >
        <div style={{ minWidth: '8rem' }}>
          <YearPicker
            id="compare-year"
            value={year}
            onChange={(next) =>
              setParams(
                (prev) => {
                  const p = new URLSearchParams(prev);
                  p.set('jahr', String(next));
                  return p;
                },
                { replace: true },
              )
            }
          />
        </div>
        <ScopeNote transfersIncluded={false} />
      </div>

      {query.isLoading && <LoadingState />}
      {query.isError && <ErrorState error={query.error} retry={() => query.refetch()} />}

      {data && !data.previousYearHasData && (
        <EmptyState
          title={t('compare.noPreviousYear', { year: data.previousYear })}
          hint={t('compare.noPreviousYearHint')}
        />
      )}

      {data && data.previousYearHasData && (
        <>
          {!fully && (
            <Banner tone="warn">
              {t('compare.partYear', {
                year: data.year,
                months: data.current.monthsWithData,
                previousYear: data.previousYear,
                previousMonths: data.previous.monthsWithData,
                comparable: data.comparableMonths.length,
              })}{' '}
              <button
                type="button"
                className="linkish"
                onClick={() => setBasisOverride(basis === 'comparable' ? 'raw' : 'comparable')}
              >
                {t(basis === 'comparable' ? 'compare.showRaw' : 'compare.showComparable')}
              </button>
            </Banner>
          )}

          {totals && (
            <div className="grid grid--kpi" style={{ marginBottom: '1rem' }}>
              <div className="kpi">
                <span className="kpi__label">{t('compare.saldoChange')}</span>
                <span className="kpi__value">
                  <FlowMoney flowCents={totals.current - totals.previous} />
                </span>
                <span className="kpi__scope">
                  {t('compare.versus', { year: data.previousYear })}
                </span>
              </div>
              <div className="kpi">
                <span className="kpi__label">{t('compare.expenseChange')}</span>
                <span className="kpi__value">
                  <FlowMoney netCents={totals.currentExpense - totals.previousExpense} />
                </span>
                <span className="kpi__scope">
                  {t('compare.versus', { year: data.previousYear })}
                </span>
              </div>
              <div className="kpi">
                <span className="kpi__label">{t('compare.incomeChange')}</span>
                <span className="kpi__value">
                  <FlowMoney flowCents={totals.currentIncome - totals.previousIncome} />
                </span>
                <span className="kpi__scope">
                  {t('compare.versus', { year: data.previousYear })}
                </span>
              </div>
              <div className="kpi">
                <span className="kpi__label">{t('compare.basis')}</span>
                <span className="kpi__value" style={{ fontSize: '1rem' }}>
                  {basis === 'comparable'
                    ? t('compare.basisComparable', { count: data.comparableMonths.length })
                    : t('compare.basisRaw')}
                </span>
                <span className="kpi__scope">
                  {t('compare.monthsCovered', {
                    current: data.current.monthsWithData,
                    previous: data.previous.monthsWithData,
                  })}
                </span>
              </div>
            </div>
          )}

          {trailing.data && (
            <div className="panel panel--pad" style={{ marginBottom: '1rem' }}>
              <TrailingChart months={asList(trailing.data.months)} />
              <ScopeNote transfersIncluded={false} />
            </div>
          )}

          {/* The table says which year was larger. Only the months say WHEN — a new
              standing cost from March and one holiday in August are the same
              number in a total and nothing alike. */}
          {chartedRow && (
            <div className="panel panel--pad" style={{ marginBottom: '1rem' }}>
              <div className="field" style={{ maxWidth: '22rem', marginBottom: '.6rem' }}>
                <label htmlFor="compare-chart-category">{t('bookings.category')}</label>
                <select
                  id="compare-chart-category"
                  className="select"
                  value={chartedRow.categoryId ?? chartedRow.categoryName}
                  onChange={(e) => setCharted(e.target.value)}
                >
                  {rows.map((row) => (
                    <option
                      key={row.categoryId ?? row.categoryName}
                      value={row.categoryId ?? row.categoryName}
                    >
                      {row.categoryName}
                    </option>
                  ))}
                </select>
              </div>
              <GroupedMonthBars
                title={`${chartedRow.categoryName} · ${year} / ${data.previousYear}`}
                note={t('compare.monthlyNote')}
                labelCurrent={String(year)}
                labelPrevious={String(data.previousYear)}
                current={chartedRow.monthlyNetCents}
                previous={chartedRow.previousMonthlyNetCents}
                valueBasis="net"
              />
              <ScopeNote transfersIncluded={false} />
            </div>
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
                        sort.key === col.key ? (sort.desc ? 'descending' : 'ascending') : 'none'
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
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => {
                  const f = pick(row, basis);
                  return (
                    <tr key={row.categoryId ?? row.categoryName}>
                      <th scope="row">
                        <CategoryChip
                          name={row.categoryName}
                          typeLabel={row.categoryType}
                          fallback={t('bookings.sourceNone')}
                        />
                        {row.isNew && <StatusPill tone="info">{t('compare.new')}</StatusPill>}
                        {row.isGone && <StatusPill tone="warn">{t('compare.gone')}</StatusPill>}
                      </th>
                      <td>
                        {row.categoryType ? <DataLabel>{row.categoryType}</DataLabel> : null}
                      </td>
                      <td className="num">
                        <FlowMoney netCents={f.net} />
                      </td>
                      <td className="num">
                        <FlowMoney netCents={f.previous} />
                      </td>
                      <td className="num">
                        <FlowMoney netCents={f.delta} />
                      </td>
                      <td className="num">
                        {/* Flipped with the money beside it: a cost that fell is a
                            plus here, exactly as the euro figure shows a plus. */}
                        {f.ratio === null ? (
                          <abbr title={t('compare.noRatio')}>—</abbr>
                        ) : (
                          formatPercent(-f.ratio)
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>

          <div className="screen-cards">
            {rows.map((row) => {
              const f = pick(row, basis);
              return (
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
                      {f.ratio === null ? '—' : formatPercent(-f.ratio)}
                    </span>
                  </header>
                  {(row.isNew || row.isGone) && (
                    <p style={{ marginBottom: '.4rem' }}>
                      {row.isNew && <StatusPill tone="info">{t('compare.new')}</StatusPill>}
                      {row.isGone && <StatusPill tone="warn">{t('compare.gone')}</StatusPill>}
                    </p>
                  )}
                  <dl className="mcard__grid">
                    <div>
                      <dt>{t('compare.thisYear')}</dt>
                      <dd>
                        <FlowMoney netCents={f.net} />
                      </dd>
                    </div>
                    <div>
                      <dt>{t('compare.lastYear')}</dt>
                      <dd>
                        <FlowMoney netCents={f.previous} />
                      </dd>
                    </div>
                    <div>
                      <dt>{t('compare.change')}</dt>
                      <dd>
                        <FlowMoney netCents={f.delta} />
                      </dd>
                    </div>
                    <div>
                      <dt>{t('analysis.count')}</dt>
                      <dd>
                        <span className="num">{row.bookingCount}</span>
                      </dd>
                    </div>
                  </dl>
                </article>
              );
            })}
          </div>

          {asList<CompareTypeRow>(data.byType).length > 0 && (
            <div className="panel panel--pad" style={{ marginTop: '1rem' }}>
              <h2 style={{ marginBottom: '.6rem' }}>{t('compare.byType')}</h2>
              <div className="table-wrap">
                <table className="data-table">
                  <tbody>
                    {asList<CompareTypeRow>(data.byType).map((row) => (
                      <tr key={row.typeCode}>
                        <td>
                          <CategoryChip
                            name={row.label}
                            typeLabel={row.label}
                            fallback={t('bookings.sourceNone')}
                          />
                        </td>
                        <td className="num">
                          {/* A type row is a cost line, and the type table on the
                              dashboard states costs the same way. */}
                          <Money
                            cents={
                              basis === 'comparable' ? row.comparableNetCents : row.netCents
                            }
                            basis="net"
                            tone="auto"
                          />
                        </td>
                        <td className="num">
                          <Money
                            cents={
                              basis === 'comparable'
                                ? row.comparablePreviousNetCents
                                : row.previousNetCents
                            }
                            basis="net"
                            tone="auto"
                          />
                        </td>
                        <td className="num">
                          <FlowMoney
                            netCents={
                              basis === 'comparable' ? row.comparableDeltaCents : row.deltaCents
                            }
                          />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}

          <p className="footnote">{t('compare.note')}</p>
        </>
      )}
    </>
  );
}
