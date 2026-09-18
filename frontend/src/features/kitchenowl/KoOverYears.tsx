import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';

import { GroupedMonthBars } from '../../charts/GroupedMonthBars';
import { DataLabel } from '../../components/DataLabel';
import { Money } from '../../components/Money';
import { Banner, EmptyState, ErrorState, Kpi, LoadingState } from '../../components/ui';
import { api } from '../../lib/api';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import { sortedByLabel } from '../../lib/categories';
import type { MessageKey } from '../../lib/messages/de';
import type { KoOverYears as KoOverYearsData, KoOverYearsSeries } from '../../lib/types';

type Grouping = 'categories' | 'payers';

const FULL_YEAR = 12;

/**
 * The household mirror, one column per year.
 *
 * The personal screen's twin, and a deliberate stranger in the same two ways every
 * KitchenOwl screen is: every figure is a PAIR — what the household spent and the
 * slice carried — which are never added to each other or to anything in the
 * personal ledger, and the left-hand question is one the personal ledger cannot
 * ask at all: who fronted the money, year by year.
 *
 * Nothing is averaged per year here either. The mirror starts partway into its
 * first year and the current one is still running.
 */
export function KoOverYears() {
  const t = useT();
  const [grouping, setGrouping] = useState<Grouping>('categories');
  const [charted, setCharted] = useState<string>('');

  const query = useQuery({
    queryKey: qk.kitchenowl.overYears(),
    queryFn: () => api<KoOverYearsData>('/kitchenowl/analysis/over-years'),
  });

  const data = query.data;
  const rows: KoOverYearsSeries[] = useMemo(() => {
    if (!data) return [];
    return grouping === 'payers' ? data.byPayer : data.byCategory;
  }, [data, grouping]);
  const selected = rows.find((r) => r.key === charted) ?? rows[0];
  // Largest first in the table, by name in the picker — the same split the
  // personal screen makes, and the same one every other dropdown here follows.
  const options = useMemo(() => sortedByLabel(rows), [rows]);

  if (query.isPending) return <LoadingState />;
  if (query.isError) return <ErrorState error={query.error} retry={() => query.refetch()} />;
  if (!data || data.years.length === 0) return <EmptyState hint={t('ko.analysisEmpty')} />;

  const years = data.years.map(String);
  const first = data.years[0] as number;
  const last = data.years[data.years.length - 1] as number;
  const totalAmount = data.amountPerYearCents.reduce((sum, v) => sum + v, 0);
  const totalOwn = data.ownSharePerYearCents.reduce((sum, v) => sum + v, 0);
  const partial = data.years.filter((_, i) => (data.monthsPerYear[i] ?? 0) < FULL_YEAR);
  // A payer fronted the whole amount, so the share half of the pair is theirs to
  // settle, not theirs to carry: it is left out rather than drawn as zero.
  const withShare = grouping === 'categories';

  return (
    <>
      <div className="flow__totals">
        <Kpi label={t('overYears.span')} scope="none">
          {`${first}–${last}`}
        </Kpi>
        <Kpi label={t('ko.householdTotal')} scope="none">
          <Money cents={totalAmount} basis="household" />
        </Kpi>
        <Kpi label={t('ko.myTotal')} scope="none">
          <Money cents={totalOwn} basis="share" />
        </Kpi>
      </div>

      {partial.length > 0 && (
        <Banner tone="info">{t('overYears.partialYears', { years: partial.join(', ') })}</Banner>
      )}

      <div className="panel panel--pad">
        <GroupedMonthBars
          title={t('koOverYears.perYear')}
          note={t('koOverYears.perYearNote')}
          labels={years}
          labelCurrent={t('ko.householdTotal')}
          labelPrevious={t('ko.myShare')}
          current={data.amountPerYearCents}
          previous={data.ownSharePerYearCents}
          valueBasis="gross"
        />
      </div>

      <section className="panel panel--pad series">
        <header className="series__header">
          <h2>{t('overYears.subjectTitle')}</h2>
          <p className="footnote">{t('koOverYears.subjectHint')}</p>
        </header>

        <div className="series__controls">
          <div className="segmented" role="group" aria-label={t('overYears.grouping')}>
            {(
              [
                ['categories', 'koOverYears.byCategory'],
                ['payers', 'koOverYears.byPayer'],
              ] as [Grouping, MessageKey][]
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
            <label htmlFor="ko-over-years-subject" className="sr-only">
              {t(grouping === 'payers' ? 'ko.paidBy' : 'ko.category')}
            </label>
            <select
              id="ko-over-years-subject"
              className="select"
              value={selected?.key ?? ''}
              onChange={(event) => setCharted(event.target.value)}
            >
              {options.map((row) => (
                <option key={row.key} value={row.key}>
                  {`${row.label || t('ko.noCategory')} (${row.yearsActive})`}
                </option>
              ))}
            </select>
          </div>
        </div>

        {selected && (
          <GroupedMonthBars
            title={t('overYears.subject', { subject: selected.label || t('ko.noCategory') })}
            note={withShare ? t('koOverYears.perYearNote') : t('koOverYears.payerNote')}
            labels={years}
            labelCurrent={t('ko.householdTotal')}
            labelPrevious={t('ko.myShare')}
            current={selected.perYearAmountCents}
            previous={withShare ? selected.perYearOwnShareCents : []}
            valueBasis="gross"
          />
        )}
      </section>

      <div className="panel table-wrap screen-table">
        <table className="data-table">
          <thead>
            <tr>
              <th>{t(grouping === 'payers' ? 'ko.paidBy' : 'ko.category')}</th>
              <th className="num">{t('overYears.totalColumn')}</th>
              {withShare && <th className="num">{t('ko.myShare')}</th>}
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
                  <DataLabel>{row.label || t('ko.noCategory')}</DataLabel>
                </th>
                <td className="num">
                  <Money cents={row.totalAmountCents} basis="household" />
                </td>
                {withShare && (
                  <td className="num">
                    <Money cents={row.totalOwnShareCents} basis="share" />
                  </td>
                )}
                <td className="num">{row.yearsActive}</td>
                {data.years.map((year, i) => (
                  <td key={year} className="num month-cell">
                    {(row.perYearAmountCents[i] ?? 0) === 0 ? (
                      <span className="money--empty">–</span>
                    ) : (
                      <Money cents={row.perYearAmountCents[i] ?? 0} basis="household" />
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
              <DataLabel>{row.label || t('ko.noCategory')}</DataLabel>
              <Money cents={row.totalAmountCents} basis="household" />
            </header>
            <dl className="mcard__grid">
              {withShare && (
                <div>
                  <dt>{t('ko.myShare')}</dt>
                  <dd>
                    <Money cents={row.totalOwnShareCents} basis="share" />
                  </dd>
                </div>
              )}
              <div>
                <dt>{t('overYears.yearsActive')}</dt>
                <dd>{`${row.yearsActive} / ${data.years.length}`}</dd>
              </div>
              <div>
                <dt>{t('ko.expenseCount')}</dt>
                <dd>{row.expenseCount}</dd>
              </div>
            </dl>
          </article>
        ))}
      </div>

      {data.excludedCount > 0 && (
        <Banner tone="info">{t('ko.analysisExcluded', { count: data.excludedCount })}</Banner>
      )}
    </>
  );
}
