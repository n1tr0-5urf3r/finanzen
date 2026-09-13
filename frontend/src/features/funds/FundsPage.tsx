import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useSearchParams } from 'react-router-dom';
import { Pencil, Plus, Trash2 } from 'lucide-react';

import { DataLabel } from '../../components/DataLabel';
import { FlowMoney, Money } from '../../components/Money';
import { YearPicker } from '../../components/YearPicker';
import {
  Banner,
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  StatusPill,
} from '../../components/ui';
import { api, asList } from '../../lib/api';
import { monthName } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import type { Category, FundOverview, FundStatus, FundSuggestion, SinkingFund } from '../../lib/types';

import { FundForm } from './FundForm';

type Prefill = { name: string; categoryId: string; annualCents: number; dueMonth: number };

/**
 * Rücklagen — the annual lumps, spread over twelve months.
 *
 * The screen answers one question the monthly saldo cannot: how much of what is
 * still coming this year is already known. Nothing here books anything; a fund is
 * an expectation held against the bookings its category already has, which is why
 * every row shows Soll and Ist side by side rather than a single number that could
 * be either.
 */
export function FundsPage() {
  const t = useT();
  const client = useQueryClient();
  const [params, setParams] = useSearchParams();
  const now = new Date();
  const year = Number(params.get('jahr')) || now.getFullYear();
  // Measured up to today in the current year, and to the end of any other: asking
  // "did the funds cover it" about a finished year means all twelve months.
  const month = year === now.getFullYear() ? now.getMonth() + 1 : 12;

  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<SinkingFund | null>(null);
  const [prefill, setPrefill] = useState<Prefill | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const status = useQuery({
    queryKey: qk.funds.status(year, month),
    queryFn: () => api<FundOverview>(`/funds/status?year=${year}&month=${month}`),
  });
  const suggestions = useQuery({
    queryKey: qk.funds.suggestions(year),
    queryFn: () => api<FundSuggestion[]>(`/funds/suggestions?year=${year}`),
  });
  const categories = useQuery({
    queryKey: qk.taxonomy.categories(),
    queryFn: () => api<Category[]>('/categories'),
    staleTime: 30 * 60_000,
  });

  const remove = useMutation({
    mutationFn: (fund: SinkingFund) => api<void>(`/funds/${fund.id}`, { method: 'DELETE' }),
    onSuccess: () => {
      client.invalidateQueries({ queryKey: qk.funds.root });
      setNotice(t('funds.deleted'));
    },
  });

  function openNew() {
    setEditing(null);
    setPrefill(null);
    setFormOpen(true);
  }

  function accept(s: FundSuggestion) {
    setEditing(null);
    setPrefill({
      name: s.categoryName,
      categoryId: s.categoryId,
      annualCents: s.annualCents,
      dueMonth: s.dueMonth,
    });
    setFormOpen(true);
  }

  const rows = asList<FundStatus>(status.data?.funds);

  return (
    <>
      <PageHeader
        title={t('funds.title')}
        subtitle={t('funds.intro')}
        actions={
          <Button onClick={openNew}>
            <Plus size={15} aria-hidden="true" />
            {t('funds.add')}
          </Button>
        }
      />

      {notice && <Banner tone="info">{notice}</Banner>}
      {remove.isError && <ErrorState error={remove.error} />}

      <div
        className="panel panel--pad"
        style={{
          marginBottom: '1rem',
          display: 'flex',
          gap: '.75rem 1.25rem',
          flexWrap: 'wrap',
          alignItems: 'flex-end',
        }}
      >
        <div style={{ minWidth: '8rem' }}>
          <YearPicker
            id="funds-year"
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
        <p className="footnote" style={{ margin: 0 }}>
          {t('funds.asOf', { month: monthName(month), year })}
        </p>
      </div>

      {status.isPending && <LoadingState />}
      {status.isError && <ErrorState error={status.error} retry={() => status.refetch()} />}

      {status.data && (
        <div className="panel panel--pad funds__totals">
          <div>
            <span className="kpi__label">{t('funds.owedToFuture')}</span>
            <span className="kpi__value">
              <Money cents={status.data.owedToTheFutureCents} />
            </span>
            <span className="kpi__scope">{t('funds.owedHint')}</span>
          </div>
          <div>
            <span className="kpi__label">{t('funds.monthlyTotal')}</span>
            <span className="kpi__value">
              <Money cents={status.data.monthlyAccrualCents} />
            </span>
          </div>
          <div>
            <span className="kpi__label">
              {t('funds.accruedTotal', { month: monthName(month) })}
            </span>
            <span className="kpi__value">
              <Money cents={status.data.accruedByMonthCents} />
            </span>
          </div>
          <div>
            <span className="kpi__label">{t('funds.spentTotal')}</span>
            <span className="kpi__value">
              <Money cents={status.data.spentCents} basis="net" tone="auto" />
            </span>
          </div>
        </div>
      )}

      {status.data && rows.length === 0 && (
        <EmptyState title={t('funds.empty')} hint={t('funds.emptyHint')} />
      )}

      {rows.length > 0 && (
        <>
          <div className="panel table-wrap screen-table">
            <table className="data-table">
              <thead>
                <tr>
                  <th>{t('funds.name')}</th>
                  <th>{t('funds.category')}</th>
                  <th>{t('funds.dueMonth')}</th>
                  <th className="num">{t('funds.annual')}</th>
                  <th className="num">{t('funds.monthly')}</th>
                  <th className="num">{t('funds.accrued')}</th>
                  <th className="num">{t('funds.spent')}</th>
                  <th className="num">{t('funds.overUnder')}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => (
                  <tr key={row.fund.id}>
                    <th scope="row">
                      <DataLabel>{row.fund.name}</DataLabel>
                      {!row.fund.active && (
                        <StatusPill tone="warn">{t('funds.active')}</StatusPill>
                      )}
                    </th>
                    <td>
                      {row.fund.categoryName ? (
                        <DataLabel>{row.fund.categoryName}</DataLabel>
                      ) : (
                        <span className="footnote">{t('funds.noCategory')}</span>
                      )}
                    </td>
                    <td>
                      <DataLabel>{row.fund.dueMonthName}</DataLabel>
                    </td>
                    <td className="num">
                      <Money cents={row.fund.annualCents} />
                    </td>
                    <td className="num">
                      <Money cents={row.monthlyAccrualCents} />
                    </td>
                    <td className="num">
                      <Money cents={row.accruedByMonthCents} />
                    </td>
                    <td className="num">
                      <Money cents={row.spentCents} basis="net" tone="auto" />
                    </td>
                    <td className="num">
                      {/* The one figure here that can point either way: a cushion
                          is money still available, catching up is money already
                          gone. `flowCents` keeps that sign as it is. */}
                      <FlowMoney flowCents={row.overUnderCents} />
                      <span className="footnote" style={{ marginLeft: '.35rem' }}>
                        {row.overUnderCents >= 0 ? t('funds.cushion') : t('funds.behind')}
                      </span>
                    </td>
                    <td>
                      <div className="cat-row__actions">
                        <button
                          type="button"
                          className="icon-button"
                          aria-label={`${t('common.edit')} — ${row.fund.name}`}
                          onClick={() => {
                            setPrefill(null);
                            setEditing(row.fund);
                            setFormOpen(true);
                          }}
                        >
                          <Pencil size={15} aria-hidden="true" />
                        </button>
                        <button
                          type="button"
                          className="icon-button"
                          aria-label={`${t('common.delete')} — ${row.fund.name}`}
                          onClick={() => {
                            if (window.confirm(t('funds.deleteConfirm', { name: row.fund.name })))
                              remove.mutate(row.fund);
                          }}
                        >
                          <Trash2 size={15} aria-hidden="true" />
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <div className="screen-cards">
            {rows.map((row) => (
              <article key={row.fund.id} className="mcard">
                <header>
                  <strong>
                    <DataLabel>{row.fund.name}</DataLabel>
                  </strong>
                  <span className="kpi__scope">
                    <DataLabel>{row.fund.dueMonthName}</DataLabel>
                  </span>
                </header>
                <dl className="mcard__grid">
                  <div>
                    <dt>{t('funds.annual')}</dt>
                    <dd>
                      <Money cents={row.fund.annualCents} />
                    </dd>
                  </div>
                  <div>
                    <dt>{t('funds.monthly')}</dt>
                    <dd>
                      <Money cents={row.monthlyAccrualCents} />
                    </dd>
                  </div>
                  <div>
                    <dt>{t('funds.accrued')}</dt>
                    <dd>
                      <Money cents={row.accruedByMonthCents} />
                    </dd>
                  </div>
                  <div>
                    <dt>{t('funds.spent')}</dt>
                    <dd>
                      <Money cents={row.spentCents} basis="net" tone="auto" />
                    </dd>
                  </div>
                  <div>
                    <dt>{t('funds.overUnder')}</dt>
                    <dd>
                      <FlowMoney flowCents={row.overUnderCents} />
                    </dd>
                  </div>
                  <div>
                    <dt>{t('funds.remaining')}</dt>
                    <dd>
                      <Money cents={row.remainingCents} />
                    </dd>
                  </div>
                </dl>
                <div className="cat-row__actions" style={{ marginTop: '.5rem' }}>
                  <Button
                    variant="ghost"
                    onClick={() => {
                      setPrefill(null);
                      setEditing(row.fund);
                      setFormOpen(true);
                    }}
                  >
                    {t('common.edit')}
                  </Button>
                </div>
              </article>
            ))}
          </div>
        </>
      )}

      <section className="panel panel--pad" style={{ marginTop: '1rem' }}>
        <h2 style={{ marginBottom: '.2rem' }}>{t('funds.suggestions', { year })}</h2>
        <p className="footnote">{t('funds.suggestionsHint')}</p>

        {suggestions.isError && <ErrorState error={suggestions.error} />}
        {suggestions.data && asList<FundSuggestion>(suggestions.data).length === 0 && (
          <p className="footnote">{t('funds.noSuggestions')}</p>
        )}

        <div className="funds__suggestions">
          {asList<FundSuggestion>(suggestions.data).map((s) => (
            <article key={s.categoryId} className="mcard">
              <header>
                <strong>
                  <DataLabel>{s.categoryName}</DataLabel>
                </strong>
                <span className="kpi__scope">
                  <DataLabel>{s.dueMonthName}</DataLabel>
                </span>
              </header>
              <p style={{ margin: '0 0 .4rem' }}>
                <Money cents={s.annualCents} />
              </p>
              <p className="footnote" style={{ marginTop: 0 }}>
                {t('funds.suggestionEvidence', {
                  count: s.bookingCount,
                  months: s.monthsWithSpending,
                })}
              </p>
              <Button variant="secondary" onClick={() => accept(s)}>
                {t('funds.accept')}
              </Button>
            </article>
          ))}
        </div>
      </section>

      {formOpen && (
        <FundForm
          fund={editing}
          prefill={prefill}
          categories={asList<Category>(categories.data)}
          onDone={(created) => {
            setFormOpen(false);
            setEditing(null);
            setPrefill(null);
            setNotice(t(created ? 'funds.created' : 'funds.saved'));
            client.invalidateQueries({ queryKey: qk.funds.root });
          }}
          onCancel={() => {
            setFormOpen(false);
            setEditing(null);
            setPrefill(null);
          }}
        />
      )}
    </>
  );
}
