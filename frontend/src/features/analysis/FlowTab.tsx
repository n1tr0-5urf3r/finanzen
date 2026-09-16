import { useEffect, useMemo, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';

import { SankeyFlow } from '../../charts/SankeyFlow';
import { FlowMoney, Money } from '../../components/Money';
import { Banner, EmptyState, ErrorState, Kpi, LoadingState } from '../../components/ui';
import { api, asList } from '../../lib/api';
import { monthName } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import { buildFlow, monthsWithActivity, type FlowDetail } from '../../lib/sankey';
import type { MessageKey } from '../../lib/messages/de';
import type { CategoryAnalysis, CategoryTypeCode, CategoryTypeSummary } from '../../lib/types';

const PARAM_MONTH = 'monat';

/** The month, or the whole year, read from the URL by everything that needs it. */
function useFlowMonth(): [number | null, (next: number | null) => void] {
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

/**
 * Year or month, sitting in the page's own filter bar beside the year picker
 * rather than in a second panel of its own — one row of controls per screen is
 * the pattern every other page here follows.
 */
export function FlowPeriodPicker({ year }: { year: number }) {
  const t = useT();
  const [month, selectMonth] = useFlowMonth();
  const query = useQuery({
    queryKey: qk.derived.categories(year),
    queryFn: () => api<CategoryAnalysis>(`/analysis/categories?year=${year}`),
  });
  const months = useMemo(() => monthsWithActivity(query.data?.rows ?? []), [query.data]);

  return (
    <div className="field">
      <label htmlFor="flow-month">{t('flow.period')}</label>
      <select
        id="flow-month"
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
 * Where the money came from and where it went, for one year or one month.
 *
 * The category table answers "what did this cost"; this answers the question
 * that comes before it — "of everything that came in, how much survived the
 * fixed costs". A table can carry both figures and still not make the ratio
 * visible, which is the entire reason a flow diagram exists.
 *
 * The month lives in the URL beside the year, so a particular month's flow is a
 * link; the drill-down does not, because it is a way of looking rather than a
 * thing to send someone.
 */
export function FlowTab({ year }: { year: number }) {
  const t = useT();
  const [month] = useFlowMonth();
  const [expanded, setExpanded] = useState<CategoryTypeCode | null>(null);
  const [detail, setDetail] = useState<FlowDetail>('types');
  const [fanOut, setFanOut] = useState(false);

  // Same key as the category tab: switching tabs must not refetch what is
  // already in the cache, and both read the identical figures.
  const query = useQuery({
    queryKey: qk.derived.categories(year),
    queryFn: () => api<CategoryAnalysis>(`/analysis/categories?year=${year}`),
  });
  const types = useQuery({
    queryKey: qk.taxonomy.types(),
    queryFn: () => api<CategoryTypeSummary[]>('/category-types'),
    staleTime: 30 * 60_000,
  });

  const rows = useMemo(() => query.data?.rows ?? [], [query.data]);

  // A type broken out in January is rarely the one worth breaking out in
  // February, and a stale expansion makes the diagram look like it changed shape
  // on its own.
  useEffect(() => setExpanded(null), [month]);

  const model = useMemo(
    () =>
      buildFlow({
        rows,
        types: asList<CategoryTypeSummary>(types.data),
        month,
        detail,
        fanOut: detail === 'types' && fanOut,
        expanded,
        labels: {
          hub: t('flow.hub'),
          surplus: t('flow.surplus'),
          deficit: t('flow.deficit'),
          noType: t('flow.noType'),
        },
      }),
    [rows, types.data, month, detail, fanOut, expanded, t],
  );

  const periodLabel = month === null ? String(year) : `${monthName(month)} ${year}`;

  return (
    <>
      {query.isLoading && <LoadingState />}
      {query.isError && <ErrorState error={query.error} retry={() => query.refetch()} />}

      {query.data && rows.length === 0 && <EmptyState hint={t('analysis.empty')} />}

      {query.data && rows.length > 0 && (
        <>
          {model.totalCents === 0 ? (
            <EmptyState hint={t('flow.emptyPeriod', { period: periodLabel })} />
          ) : (
            <>
              <div className="flow__totals">
                <Kpi label={t('flow.inflow')} scope="without" hint={t('flow.inflowHint')}>
                  <Money cents={model.inflowCents} tone="income" />
                </Kpi>
                <Kpi label={t('flow.outflow')} scope="without">
                  <Money cents={model.outflowCents} tone="expense" />
                </Kpi>
                <Kpi label={t('flow.saldo')} scope="without">
                  <FlowMoney flowCents={model.saldoCents} />
                </Kpi>
              </div>

              <div className="panel panel--pad">
                <div className="flow__detail">
                  <div className="segmented" role="group" aria-label={t('flow.detail')}>
                    {(
                      [
                        ['types', 'flow.detailTypes'],
                        ['categories', 'flow.detailCategories'],
                      ] as [FlowDetail, MessageKey][]
                    ).map(([value, labelKey]) => (
                      <button
                        key={value}
                        type="button"
                        aria-pressed={detail === value}
                        onClick={() => {
                          setDetail(value);
                          setExpanded(null);
                        }}
                      >
                        {t(labelKey)}
                      </button>
                    ))}
                  </div>
                  {detail === 'types' && (
                    <label className="flow__switch">
                      <input
                        type="checkbox"
                        checked={fanOut}
                        onChange={(event) => {
                          setFanOut(event.target.checked);
                          setExpanded(null);
                        }}
                      />
                      {t('flow.fanOut')}
                    </label>
                  )}
                  {expanded && detail === 'types' && !fanOut && (
                    <button type="button" className="linkish" onClick={() => setExpanded(null)}>
                      {t('flow.collapseAll')}
                    </button>
                  )}
                </div>
                <SankeyFlow
                  title={t('flow.chartTitle', { period: periodLabel })}
                  note={t('flow.chartNote')}
                  model={model}
                  expanded={expanded}
                  onToggleType={
                    detail === 'types' && !fanOut
                      ? (code) => setExpanded((prev) => (prev === code ? null : code))
                      : undefined
                  }
                  labels={{
                    amount: t('flow.amount'),
                    expand: t('flow.expand'),
                    collapse: t('flow.collapse'),
                  }}
                />
              </div>

              {/* Both facts the picture cannot state on its own: that its income
                  side is already netted, and that a reimbursement is sitting on
                  the left rather than reducing a bar on the right. */}
              {model.creditCount > 0 && (
                <Banner tone="info">{t('flow.credits', { count: model.creditCount })}</Banner>
              )}
              {query.data.uncategorizedCount > 0 && (
                <Banner tone="warn">
                  <Link to={`/buchungen?jahr=${year}&ohneKategorie=1`}>
                    {t('analysis.uncategorized', { count: query.data.uncategorizedCount })}
                  </Link>
                </Banner>
              )}
              {query.data.excludedTransferCount > 0 && (
                <Banner tone="info">
                  {t('analysis.transfersExcluded', { count: query.data.excludedTransferCount })}
                </Banner>
              )}
            </>
          )}
        </>
      )}
    </>
  );
}
