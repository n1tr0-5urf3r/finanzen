import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';

import { CumulativeLine, MonthlyBars, type BandPoint } from '../../charts/MonthlyCharts';
import { NetBars, type NetBar } from '../../charts/NetBars';
import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { FlowMoney, Money } from '../../components/Money';
import { Banner, EmptyState, ErrorState, Kpi, LoadingState } from '../../components/ui';
import { api, asList } from '../../lib/api';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import type { OverYears, OverYearsSeries, Year } from '../../lib/types';

type Grouping = 'categories' | 'types';

/** A part year beside full ones is the one way this screen can mislead: twelve
    months of rent against two is not thrift. Anything under this many months is
    called out rather than left to be noticed. */
const FULL_YEAR = 12;

/**
 * The whole ledger, one column per year.
 *
 * The year-on-year screen answers "what changed since last year"; with twelve
 * years of history that stopped being the interesting question. This answers the
 * one only a long ledger can: what a category has been doing all along — rent
 * climbing year after year, a car bought once and never again, a subscription
 * nobody ever cancelled.
 *
 * Nothing here is averaged per year. The first year of this ledger has two months
 * in it and the current one is still running, so an average would quietly compare
 * a part year with a full one; instead the short years are named, every figure is
 * the year's own total, and the month count sits in the table beside it.
 */
export function OverYearsTab() {
  const t = useT();
  const [grouping, setGrouping] = useState<Grouping>('categories');
  const [charted, setCharted] = useState<string>('');

  const query = useQuery({
    queryKey: qk.derived.overYears(),
    queryFn: () => api<OverYears>('/analysis/over-years'),
  });
  // The closing balance per year is the one figure this screen cannot derive: it
  // needs the configured carryover, which the year list already carries.
  const years = useQuery({
    queryKey: qk.years(),
    queryFn: () => api<Year[]>('/years'),
    staleTime: 5 * 60_000,
  });

  const data = query.data;
  const rows: OverYearsSeries[] = useMemo(() => {
    if (!data) return [];
    return grouping === 'types' ? data.byType : data.byCategory;
  }, [data, grouping]);

  const selected = rows.find((r) => r.key === charted) ?? rows[0];

  const points: BandPoint[] = useMemo(() => {
    if (!data) return [];
    const closing = new Map(
      asList<Year>(years.data).map((y) => [y.year, y.closingBalanceCents] as const),
    );
    return data.years.map((year, i) => ({
      label: String(year),
      short: String(year),
      incomeCents: data.incomePerYearCents[i] ?? 0,
      expenseCents: data.expensePerYearCents[i] ?? 0,
      cumulativeCents: closing.get(year) ?? null,
      hasData: true,
    }));
  }, [data, years.data]);

  const bars: NetBar[] = useMemo(() => {
    if (!data || !selected) return [];
    return data.years.map((year, i) => ({
      label: String(year),
      short: String(year),
      netCents: selected.perYearCents[i] ?? 0,
      // A year the subject itself has no booking in, which is not the same as a
      // year it netted to zero.
      hasData: (selected.perYearCents[i] ?? 0) !== 0,
    }));
  }, [data, selected]);

  if (query.isLoading) return <LoadingState />;
  if (query.isError) return <ErrorState error={query.error} retry={() => query.refetch()} />;
  if (!data || data.years.length === 0) return <EmptyState hint={t('analysis.empty')} />;

  const first = data.years[0] as number;
  const last = data.years[data.years.length - 1] as number;
  const total = data.balancePerYearCents.reduce((sum, v) => sum + v, 0);
  const best = data.balancePerYearCents.reduce(
    (acc, v, i) => (v > (data.balancePerYearCents[acc] ?? 0) ? i : acc),
    0,
  );
  const worst = data.balancePerYearCents.reduce(
    (acc, v, i) => (v < (data.balancePerYearCents[acc] ?? 0) ? i : acc),
    0,
  );
  const partial = data.years.filter((_, i) => (data.monthsPerYear[i] ?? 0) < FULL_YEAR);

  return (
    <>
      <div className="flow__totals">
        <Kpi label={t('overYears.span')} scope="none">
          {`${first}–${last}`}
        </Kpi>
        <Kpi label={t('overYears.total')} scope="without" hint={t('overYears.totalHint')}>
          <FlowMoney flowCents={total} />
        </Kpi>
        <Kpi label={t('overYears.best')} scope="without">
          <span>
            <DataLabel>{String(data.years[best])}</DataLabel>{' '}
            <FlowMoney flowCents={data.balancePerYearCents[best] ?? 0} />
          </span>
        </Kpi>
        <Kpi label={t('overYears.worst')} scope="without">
          <span>
            <DataLabel>{String(data.years[worst])}</DataLabel>{' '}
            <FlowMoney flowCents={data.balancePerYearCents[worst] ?? 0} />
          </span>
        </Kpi>
      </div>

      {partial.length > 0 && (
        <Banner tone="info">
          {t('overYears.partialYears', {
            years: partial.join(', '),
          })}
        </Banner>
      )}

      <div className="chart-grid chart-grid--even">
        <div className="panel panel--pad">
          <MonthlyBars points={points} title={t('overYears.perYear')} />
        </div>
        <div className="panel panel--pad">
          <CumulativeLine
            points={points}
            title={t('overYears.closing')}
            note={t('overYears.closingNote')}
          />
        </div>
      </div>

      {/* The same shape as the per-month series: a switch for what is being
          listed and a dropdown for which one of them, rather than a chart that
          can only be aimed from a table row thirty lines further down. */}
      <section className="panel panel--pad series">
        <header className="series__header">
          <h2>{t('overYears.subjectTitle')}</h2>
          <p className="footnote">{t('overYears.subjectHint')}</p>
        </header>

        <div className="series__controls">
          <div className="segmented" role="group" aria-label={t('overYears.grouping')}>
            {(
              [
                ['categories', 'overYears.byCategory'],
                ['types', 'overYears.byType'],
              ] as [Grouping, 'overYears.byCategory' | 'overYears.byType'][]
            ).map(([value, labelKey]) => (
              <button
                key={value}
                type="button"
                aria-pressed={grouping === value}
                onClick={() => {
                  setGrouping(value);
                  setCharted('');
                }}
              >
                {t(labelKey)}
              </button>
            ))}
          </div>

          <div className="field series__picker">
            <label htmlFor="over-years-subject" className="sr-only">
              {t(grouping === 'types' ? 'bookings.type' : 'bookings.category')}
            </label>
            <select
              id="over-years-subject"
              className="select"
              value={selected?.key ?? ''}
              onChange={(event) => setCharted(event.target.value)}
            >
              {rows.map((row) => (
                <option key={row.key} value={row.key}>
                  {`${row.label} (${row.yearsActive})`}
                </option>
              ))}
            </select>
          </div>
        </div>

        {selected && (
          <NetBars
            title={t('overYears.subject', { subject: selected.label })}
            note={t('overYears.subjectNote')}
            bars={bars}
          />
        )}
      </section>

      {/* Wide on a desktop, cards on a phone — the same pair every report screen
          here uses, because a thirteen-column table cannot be made one-handed. */}
      <div className="panel table-wrap screen-table">
        <table className="data-table">
          <thead>
            <tr>
              <th>{t(grouping === 'types' ? 'bookings.type' : 'bookings.category')}</th>
              <th className="num">{t('overYears.totalColumn')}</th>
              <th className="num">{t('overYears.yearsActive')}</th>
              {data.years.map((year) => (
                <th key={year} className="num month-cell">
                  <DataLabel>{String(year)}</DataLabel>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.key} className={row.key === selected?.key ? 'is-charted' : undefined}>
                <th scope="row">
                  {grouping === 'types' ? (
                    <DataLabel>{row.label}</DataLabel>
                  ) : (
                    <CategoryChip
                      name={row.label}
                      typeLabel={row.categoryType}
                      fallback={t('bookings.sourceNone')}
                    />
                  )}
                </th>
                <td className="num">
                  <Money cents={row.totalCents} basis="net" tone="auto" />
                </td>
                <td className="num">{row.yearsActive}</td>
                {data.years.map((year, i) => (
                  <td key={year} className="num month-cell">
                    {(row.perYearCents[i] ?? 0) === 0 ? (
                      <span className="money--empty">–</span>
                    ) : (
                      <Money cents={row.perYearCents[i] ?? 0} basis="net" tone="auto" />
                    )}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="screen-cards">
        {rows.map((row) => (
          <article key={row.key} className="mcard">
            <header>
              {grouping === 'types' ? (
                <DataLabel>{row.label}</DataLabel>
              ) : (
                <CategoryChip
                  name={row.label}
                  typeLabel={row.categoryType}
                  fallback={t('bookings.sourceNone')}
                />
              )}
              <Money cents={row.totalCents} basis="net" tone="auto" />
            </header>
            <dl className="mcard__grid">
              <div>
                <dt>{t('overYears.yearsActive')}</dt>
                <dd>{`${row.yearsActive} / ${data.years.length}`}</dd>
              </div>
              <div>
                <dt>{t('analysis.count')}</dt>
                <dd>{row.bookingCount}</dd>
              </div>
            </dl>
          </article>
        ))}
      </div>
    </>
  );
}
