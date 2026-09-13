import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';

import { DataLabel } from '../../components/DataLabel';
import { Money } from '../../components/Money';
import { Banner, EmptyState, ErrorState, LoadingState, StatusPill } from '../../components/ui';
import { api, asList } from '../../lib/api';
import { formatPercent } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import { KoTrailingChart } from './KoTrailingChart';
import type {
  KoCompareRow,
  KoComparePayer,
  KoTrailingWindow,
  KoYearComparison,
} from '../../lib/types';

/** Which pair of figures a row is read through. */
type Basis = 'comparable' | 'raw';

function pick(row: KoCompareRow, basis: Basis) {
  return basis === 'comparable'
    ? {
        amount: row.comparableAmountCents,
        previousAmount: row.comparablePreviousAmountCents,
        share: row.comparableOwnShareCents,
        previousShare: row.comparablePreviousOwnShareCents,
        delta: row.comparableDeltaAmountCents,
        shareDelta: row.comparableDeltaOwnShareCents,
        ratio: row.comparableDeltaRatio,
      }
    : {
        amount: row.amountCents,
        previousAmount: row.previousAmountCents,
        share: row.ownShareCents,
        previousShare: row.previousOwnShareCents,
        delta: row.deltaAmountCents,
        shareDelta: row.deltaOwnShareCents,
        ratio: row.deltaRatio,
      };
}

/**
 * The household's year against the one before it.
 *
 * A deliberate twin of the personal `/vergleich` screen, and a deliberate
 * stranger in the same two ways the single-year view is: every figure is a PAIR —
 * what the household spent and the slice the user carries — and nothing here is
 * ever netted, flipped or added to a booking.
 *
 * Built around the one way a comparison lies. The mirror's first expense is
 * 2024-12-22, so 2024 holds a single month: comparing its total with 2025's twelve
 * would report a rise of over a thousand percent that is entirely the calendar.
 * When the two years cover different months the restricted figures lead, the raw
 * ones are one click away, and the banner says which is which.
 */
export function KoCompare({ year }: { year: number }) {
  const t = useT();
  const [basisOverride, setBasisOverride] = useState<Basis | null>(null);

  const query = useQuery({
    queryKey: qk.kitchenowl.compare(year),
    queryFn: () => api<KoYearComparison>(`/kitchenowl/analysis/compare?year=${year}`),
  });

  // The window ends where the year's data ends: a rolling twelve months that ran to
  // December of a year still in progress would be months of nothing.
  const endMonth = query.data?.current.lastMonthWithData ?? 12;
  const trailing = useQuery({
    queryKey: qk.kitchenowl.trailing(year, endMonth),
    queryFn: () =>
      api<KoTrailingWindow>(`/kitchenowl/analysis/trailing?year=${year}&month=${endMonth}`),
    enabled: query.isSuccess,
  });

  const data = query.data;
  const fully = data?.fullyComparable ?? true;
  const basis: Basis = basisOverride ?? (fully ? 'raw' : 'comparable');

  const rows = useMemo(() => asList<KoCompareRow>(data?.rows), [data]);
  const payers = asList<KoComparePayer>(data?.paidBy);

  const totals = data
    ? basis === 'comparable'
      ? {
          amount: data.current.comparableAmountCents,
          previousAmount: data.previous.comparableAmountCents,
          share: data.current.comparableOwnShareCents,
          previousShare: data.previous.comparableOwnShareCents,
        }
      : {
          amount: data.current.amountCents,
          previousAmount: data.previous.amountCents,
          share: data.current.ownShareCents,
          previousShare: data.previous.ownShareCents,
        }
    : null;

  if (query.isPending) return <LoadingState />;
  if (query.isError) return <ErrorState error={query.error} retry={() => query.refetch()} />;
  if (!data) return null;

  if (!data.previousYearHasData) {
    return (
      <EmptyState
        title={t('ko.compareNoPrevious', { year: data.previousYear })}
        hint={t('ko.compareNoPreviousHint')}
      />
    );
  }

  return (
    <div className="ko-compare">
      <p className="footnote" style={{ marginTop: 0 }}>
        {t('ko.compareIntro', { year: data.year, previousYear: data.previousYear })}
      </p>

      {!fully && (
        <Banner tone="warn">
          {t('ko.comparePartYear', {
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
            {t(basis === 'comparable' ? 'ko.compareShowRaw' : 'ko.compareShowComparable')}
          </button>
        </Banner>
      )}

      {totals && (
        <div className="ko-analysis__totals panel panel--pad">
          <div>
            <span className="kpi__label">{t('ko.compareHouseholdChange')}</span>
            <span className="kpi__value">
              <Money
                cents={totals.amount - totals.previousAmount}
                basis="household"
                signed
                tone={totals.amount >= totals.previousAmount ? 'expense' : 'income'}
              />
            </span>
          </div>
          <div>
            <span className="kpi__label">{t('ko.compareShareChange')}</span>
            <span className="kpi__value">
              <Money
                cents={totals.share - totals.previousShare}
                basis="share"
                signed
                tone={totals.share >= totals.previousShare ? 'expense' : 'income'}
              />
            </span>
          </div>
          <div>
            <span className="kpi__label">{t('ko.compareBasis')}</span>
            <span className="kpi__value" style={{ fontSize: '1rem' }}>
              {basis === 'comparable'
                ? t('ko.compareBasisComparable', { count: data.comparableMonths.length })
                : t('ko.compareBasisRaw')}
            </span>
            <span className="kpi__scope">
              {t('ko.compareMonthsCovered', {
                current: data.current.monthsWithData,
                previous: data.previous.monthsWithData,
              })}
            </span>
          </div>
          <div>
            <span className="kpi__label">{t('ko.comparePayers')}</span>
            <span className="ko-analysis__payers">
              {payers.map((p) => (
                <span key={p.name}>
                  <DataLabel>{p.name}</DataLabel>{' '}
                  <Money
                    cents={p.deltaCents}
                    basis="household"
                    signed
                    tone={p.deltaCents >= 0 ? 'expense' : 'income'}
                  />
                </span>
              ))}
            </span>
          </div>
        </div>
      )}

      {trailing.data && (
        <div className="panel panel--pad" style={{ marginBottom: '1rem' }}>
          <KoTrailingChart months={asList(trailing.data.months)} />
        </div>
      )}

      <div className="panel table-wrap screen-table">
        <table className="data-table">
          <thead>
            <tr>
              <th>{t('ko.category')}</th>
              <th className="num">{t('ko.compareThisYear', { year: data.year })}</th>
              <th className="num">{t('ko.compareLastYear', { year: data.previousYear })}</th>
              <th className="num">{t('ko.compareChange')}</th>
              <th className="num">{t('ko.compareChangePercent')}</th>
              <th className="num">{t('ko.compareShareThisYear', { year: data.year })}</th>
              <th className="num">{t('ko.compareShareLastYear', { year: data.previousYear })}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => {
              const f = pick(row, basis);
              return (
                <tr key={row.koCategoryId ?? 'none'}>
                  <th scope="row">
                    {row.koCategoryName ? (
                      <DataLabel>{row.koCategoryName}</DataLabel>
                    ) : (
                      t('ko.noCategory')
                    )}
                    {row.isNew && <StatusPill tone="info">{t('ko.compareNew')}</StatusPill>}
                    {row.isGone && <StatusPill tone="warn">{t('ko.compareGone')}</StatusPill>}
                  </th>
                  <td className="num">
                    <Money cents={f.amount} basis="household" />
                  </td>
                  <td className="num">
                    <Money cents={f.previousAmount} basis="household" />
                  </td>
                  <td className="num">
                    {/* Spending more is the direction that costs, so it carries the
                        expense tone — there is no netting here and no credit case. */}
                    <Money
                      cents={f.delta}
                      basis="household"
                      signed
                      tone={f.delta >= 0 ? 'expense' : 'income'}
                    />
                  </td>
                  <td className="num">
                    {f.ratio === null ? (
                      <abbr title={t('ko.compareNoRatio')}>—</abbr>
                    ) : (
                      formatPercent(f.ratio)
                    )}
                  </td>
                  <td className="num">
                    <Money cents={f.share} basis="share" />
                  </td>
                  <td className="num">
                    <Money cents={f.previousShare} basis="share" />
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
            <article key={row.koCategoryId ?? 'none'} className="mcard">
              <header>
                <strong>
                  {row.koCategoryName ? (
                    <DataLabel>{row.koCategoryName}</DataLabel>
                  ) : (
                    t('ko.noCategory')
                  )}
                </strong>
                <span className="kpi__scope">
                  {f.ratio === null ? '—' : formatPercent(f.ratio)}
                </span>
              </header>
              {(row.isNew || row.isGone) && (
                <p style={{ marginBottom: '.4rem' }}>
                  {row.isNew && <StatusPill tone="info">{t('ko.compareNew')}</StatusPill>}
                  {row.isGone && <StatusPill tone="warn">{t('ko.compareGone')}</StatusPill>}
                </p>
              )}
              <dl className="mcard__grid">
                <div>
                  <dt>{t('ko.compareThisYear', { year: data.year })}</dt>
                  <dd>
                    <Money cents={f.amount} basis="household" />
                  </dd>
                </div>
                <div>
                  <dt>{t('ko.compareLastYear', { year: data.previousYear })}</dt>
                  <dd>
                    <Money cents={f.previousAmount} basis="household" />
                  </dd>
                </div>
                <div>
                  <dt>{t('ko.compareChange')}</dt>
                  <dd>
                    <Money
                      cents={f.delta}
                      basis="household"
                      signed
                      tone={f.delta >= 0 ? 'expense' : 'income'}
                    />
                  </dd>
                </div>
                <div>
                  <dt>{t('ko.myShare')}</dt>
                  <dd>
                    <Money cents={f.share} basis="share" />
                  </dd>
                </div>
              </dl>
            </article>
          );
        })}
      </div>

      <p className="footnote">{t('ko.compareNote')}</p>
    </div>
  );
}
