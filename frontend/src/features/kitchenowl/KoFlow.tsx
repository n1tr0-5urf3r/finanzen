import { useEffect, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useSearchParams } from 'react-router-dom';

import { SankeyFlow } from '../../charts/SankeyFlow';
import { Money } from '../../components/Money';
import { Banner, EmptyState, ErrorState, Kpi, LoadingState } from '../../components/ui';
import { api, asList } from '../../lib/api';
import { monthName } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import { buildKoFlow, type KoFlowDetail } from '../../lib/sankey';
import type { MessageKey } from '../../lib/messages/de';
import type { KoCategoryAnalysis, KoCategoryAnalysisRow, KoPayerShare } from '../../lib/types';

const PARAM_MONTH = 'monat';

/** The month, or the whole year, read from the URL by both halves of this view. */
function useKoMonth(): [number | null, (next: number | null) => void] {
  const [params, setParams] = useSearchParams();
  const raw = Number(params.get(PARAM_MONTH));
  const month = raw >= 1 && raw <= 12 ? raw : null;

  const select = (next: number | null) =>
    setParams(
      (prev) => {
        const nextParams = new URLSearchParams(prev);
        if (next === null) nextParams.delete(PARAM_MONTH);
        else nextParams.set(PARAM_MONTH, String(next));
        return nextParams;
      },
      { replace: true },
    );

  return [month, select];
}

/** Which months the mirror has anything in, for this year. */
function useKoMonths(year: number): number[] {
  const query = useQuery({
    queryKey: qk.kitchenowl.analysis(year),
    queryFn: () => api<KoCategoryAnalysis>(`/kitchenowl/analysis/categories?year=${year}`),
  });
  return useMemo(() => {
    const rows = asList<KoCategoryAnalysisRow>(query.data?.rows);
    const seen: number[] = [];
    for (let m = 1; m <= 12; m += 1) {
      if (rows.some((r) => (r.monthlyAmountCents?.[m - 1] ?? 0) !== 0)) seen.push(m);
    }
    return seen;
  }, [query.data]);
}

/**
 * Year or month, in the household screen's own filter bar beside the year — the
 * same place the personal money flow puts it, rather than in a panel of its own.
 */
export function KoFlowPeriodPicker({ year }: { year: number }) {
  const t = useT();
  const [month, selectMonth] = useKoMonth();
  const months = useKoMonths(year);

  return (
    <div className="field">
      <label htmlFor="ko-flow-month">{t('flow.period')}</label>
      <select
        id="ko-flow-month"
        className="select"
        value={month ?? ''}
        onChange={(event) =>
          selectMonth(event.target.value === '' ? null : Number(event.target.value))
        }
      >
        <option value="">{t('flow.wholeYear', { year })}</option>
        {months.map((m) => (
          <option key={m} value={m}>
            {monthName(m)}
          </option>
        ))}
      </select>
    </div>
  );
}

/**
 * The household's money flow: who paid it, and what it was for.
 *
 * The personal diagram has an income side. This one has something the personal
 * ledger cannot: a second true fact about the same euro — who fronted it. So the
 * left column is the payers, and both sides are the same total seen twice. There
 * is no balance and no "left over", because a household ledger has neither; a
 * figure that looked like one would invite adding it to the personal balance,
 * which is the one thing these two ledgers must never do.
 */
export function KoFlow({ year }: { year: number }) {
  const t = useT();
  const [detail, setDetail] = useState<KoFlowDetail>('categories');
  const [month, selectMonth] = useKoMonth();

  // The same key the year view uses: switching between them must not refetch.
  const query = useQuery({
    queryKey: qk.kitchenowl.analysis(year),
    queryFn: () => api<KoCategoryAnalysis>(`/kitchenowl/analysis/categories?year=${year}`),
  });

  const rows = useMemo(
    () => asList<KoCategoryAnalysisRow>(query.data?.rows),
    [query.data],
  );
  const payers = useMemo(() => asList<KoPayerShare>(query.data?.paidBy), [query.data]);

  const months = useKoMonths(year);

  // A month that falls out of the data (a different year, a fresh sync) must not
  // leave the page showing an empty diagram for a month that no longer exists.
  useEffect(() => {
    if (month !== null && months.length > 0 && !months.includes(month)) selectMonth(null);
  }, [months, month]);

  const model = useMemo(
    () =>
      buildKoFlow({
        payers,
        rows,
        month,
        detail,
        labels: {
          hub: t('koFlow.hub'),
          mine: t('koFlow.mine'),
          others: t('koFlow.others'),
          noCategory: t('ko.noCategory'),
          unknownPayer: t('koFlow.unknownPayer'),
        },
      }),
    [payers, rows, month, detail, t],
  );

  const periodLabel = month === null ? String(year) : `${monthName(month)} ${year}`;
  const ownShare = useMemo(
    () =>
      rows.reduce(
        (sum, r) =>
          sum + (month === null ? r.ownShareCents : (r.monthlyOwnShareCents?.[month - 1] ?? 0)),
        0,
      ),
    [rows, month],
  );

  return (
    <>
      {query.isPending && <LoadingState />}
      {query.isError && <ErrorState error={query.error} retry={() => query.refetch()} />}

      {query.data && rows.length === 0 && <EmptyState hint={t('ko.analysisEmpty')} />}

      {query.data && rows.length > 0 && (
        <>
          {model.totalCents === 0 ? (
            <EmptyState hint={t('flow.emptyPeriod', { period: periodLabel })} />
          ) : (
            <>
              {/* The household figure and the user's share, side by side and
                  never added — the rule every KitchenOwl screen follows. */}
              <div className="flow__totals">
                <Kpi label={t('ko.householdTotal')} scope="none">
                  <Money cents={model.totalCents} basis="household" />
                </Kpi>
                <Kpi label={t('ko.myTotal')} scope="none">
                  <Money cents={ownShare} basis="share" />
                </Kpi>
              </div>

              <div className="panel panel--pad">
                <div className="flow__detail">
                  <div className="segmented" role="group" aria-label={t('flow.detail')}>
                    {(
                      [
                        ['categories', 'koFlow.byCategory'],
                        ['shares', 'koFlow.byShare'],
                      ] as [KoFlowDetail, MessageKey][]
                    ).map(([value, labelKey]) => (
                      <button
                        key={value}
                        type="button"
                        aria-pressed={detail === value}
                        onClick={() => setDetail(value)}
                      >
                        {t(labelKey)}
                      </button>
                    ))}
                  </div>
                </div>
                <SankeyFlow
                  title={t('koFlow.chartTitle', { period: periodLabel })}
                  note={t('koFlow.chartNote')}
                  model={model}
                  expanded={null}
                  labels={{
                    amount: t('flow.amount'),
                    expand: t('flow.expand'),
                    collapse: t('flow.collapse'),
                  }}
                />
              </div>

              {query.data.excludedCount > 0 && (
                <Banner tone="info">
                  {t('ko.analysisExcluded', { count: query.data.excludedCount })}
                </Banner>
              )}
            </>
          )}
        </>
      )}
    </>
  );
}
