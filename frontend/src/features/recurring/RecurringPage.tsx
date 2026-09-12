import { useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useSearchParams } from 'react-router-dom';
import { CalendarCheck, Check, Pencil, Plus, Trash2 } from 'lucide-react';

import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { Money } from '../../components/Money';
import {
  Banner,
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  StatusPill,
} from '../../components/ui';
import { api, jsonBody, asList } from '../../lib/api';
import { MONTHS_DE, parseEuroInput } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { invalidateAfterMaterialize, qk } from '../../lib/queryKeys';
import type {
  Booking,
  BookingPage,
  Category,
  MaterializeResult,
  RecurringTemplate,
} from '../../lib/types';

import { TemplateForm } from './TemplateForm';

/**
 * The fixed-cost ritual, as a checklist.
 *
 * ~18 entries a month by hand becomes: open the page, glance at what is ticked,
 * tap "alle buchen". Everything due and not yet booked is preselected, so the
 * common case needs no selection at all — and a second tap is harmless, because
 * the server refuses to create the same template's booking twice in one period.
 */
export function RecurringPage() {
  const t = useT();
  const client = useQueryClient();
  const [params, setParams] = useSearchParams();
  const now = new Date();

  const year = Number(params.get('jahr')) || now.getFullYear();
  const month = Number(params.get('monat')) || now.getMonth() + 1;

  const [deselected, setDeselected] = useState<Set<string>>(new Set());
  const [editing, setEditing] = useState<RecurringTemplate | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const templates = useQuery({
    queryKey: qk.recurring.list(year, month),
    queryFn: () =>
      api<RecurringTemplate[]>(`/recurring?year=${year}&month=${month}`),
  });
  const categories = useQuery({
    queryKey: qk.taxonomy.categories(),
    queryFn: () => api<Category[]>('/categories'),
  });
  const drafts = useQuery({
    queryKey: qk.drafts(year, month),
    queryFn: () =>
      api<BookingPage>(`/bookings?year=${year}&month=${month}&status=draft`),
  });

  // Due, not yet booked, and not unticked by hand. Selection is stored as the
  // *exclusions* so that a template becoming due does not silently stay unticked.
  const selectable = useMemo(
    () => asList<RecurringTemplate>(templates.data).filter((x) => x.dueInPeriod && !x.bookedInPeriod),
    [templates.data],
  );
  const selected = selectable.filter((x) => !deselected.has(x.id));

  const materialize = useMutation({
    mutationFn: (ids: string[]) =>
      api<MaterializeResult>(
        '/recurring/materialize',
        { method: 'POST', ...jsonBody({ year, month, templateIds: ids }) },
      ),
    onSuccess: (result) => {
      invalidateAfterMaterialize(client, year);
      setDeselected(new Set());
      setNotice(
        t('recurring.result', { created: result.created, skipped: result.skipped }) +
          (result.drafts > 0
            ? ` — ${t('recurring.resultDrafts', { count: result.drafts })}`
            : ''),
      );
    },
  });

  const remove = useMutation({
    mutationFn: (id: string) => api<void>(`/recurring/${id}`, { method: 'DELETE' }),
    onSuccess: () => client.invalidateQueries({ queryKey: qk.recurring.root }),
  });

  function setPeriod(key: string, value: number) {
    setParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        next.set(key, String(value));
        return next;
      },
      { replace: true },
    );
    setDeselected(new Set());
  }

  function intervalLabel(interval: number): string {
    if (interval === 1 || interval === 3 || interval === 6 || interval === 12) {
      return t(`recurring.interval.${interval}` as const);
    }
    return t('recurring.intervalEvery', { count: interval });
  }

  return (
    <>
      <PageHeader
        title={t('recurring.title')}
        subtitle={t('recurring.intro')}
        actions={
          <Button
            variant="secondary"
            onClick={() => {
              setEditing(null);
              setFormOpen((v) => !v);
            }}
          >
            <Plus size={15} aria-hidden="true" /> {t('recurring.newTemplate')}
          </Button>
        }
      />

      {notice && <Banner tone="info">{notice}</Banner>}
      {materialize.isError && <ErrorState error={materialize.error} />}

      <div
        className="panel panel--pad"
        style={{ marginBottom: '1rem', display: 'flex', gap: '.75rem', flexWrap: 'wrap', alignItems: 'flex-end' }}
      >
        <div className="field" style={{ minWidth: '7rem' }}>
          <label htmlFor="r-year">{t('common.year')}</label>
          <input
            id="r-year"
            className="input"
            type="number"
            value={year}
            onChange={(e) => setPeriod('jahr', Number(e.target.value))}
          />
        </div>
        <div className="field" style={{ minWidth: '9rem' }}>
          <label htmlFor="r-month">{t('common.month')}</label>
          <select
            id="r-month"
            className="select"
            value={month}
            onChange={(e) => setPeriod('monat', Number(e.target.value))}
          >
            {MONTHS_DE.map((name, index) => (
              <option key={name} value={index + 1}>
                {name}
              </option>
            ))}
          </select>
        </div>
        <Button
          onClick={() => materialize.mutate(selected.map((x) => x.id))}
          busy={materialize.isPending}
          disabled={selected.length === 0}
        >
          <CalendarCheck size={15} aria-hidden="true" />
          {selected.length === selectable.length
            ? t('recurring.bookAll')
            : t('recurring.bookSelected', { count: selected.length })}
        </Button>
      </div>

      {formOpen && (
        <TemplateForm
          template={editing}
          categories={asList<Category>(categories.data)}
          defaultPeriod={{ year, month }}
          onDone={() => {
            setFormOpen(false);
            setEditing(null);
            client.invalidateQueries({ queryKey: qk.recurring.root });
          }}
          onCancel={() => {
            setFormOpen(false);
            setEditing(null);
          }}
        />
      )}

      {templates.isLoading && <LoadingState />}
      {templates.isError && (
        <ErrorState error={templates.error} retry={() => templates.refetch()} />
      )}

      {templates.data && templates.data.length === 0 && (
        <EmptyState hint={t('recurring.empty')} />
      )}

      {templates.data && templates.data.length > 0 && (
        <div className="panel table-wrap">
          <table className="data-table">
            <thead>
              <tr>
                <th>
                  <span className="sr-only">{t('recurring.selectAll')}</span>
                </th>
                <th>{t('recurring.name')}</th>
                <th>{t('bookings.category')}</th>
                <th>{t('recurring.interval')}</th>
                <th className="num">{t('recurring.amount')}</th>
                <th>{t('common.total')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {templates.data.map((x) => {
                const due = x.dueInPeriod === true;
                const booked = x.bookedInPeriod === true;
                const checked = due && !booked && !deselected.has(x.id);
                return (
                  <tr key={x.id} className={due ? undefined : 'row--transfer'}>
                    <td>
                      <input
                        type="checkbox"
                        checked={checked}
                        disabled={!due || booked}
                        aria-label={x.name}
                        onChange={(e) =>
                          setDeselected((prev) => {
                            const next = new Set(prev);
                            if (e.target.checked) next.delete(x.id);
                            else next.add(x.id);
                            return next;
                          })
                        }
                      />
                    </td>
                    <td>
                      <DataLabel>{x.name}</DataLabel>
                      {!x.active && (
                        <StatusPill tone="neutral">{t('recurring.inactive')}</StatusPill>
                      )}
                      {x.amountIsEstimate && (
                        <StatusPill tone="warn">{t('recurring.estimate')}</StatusPill>
                      )}
                    </td>
                    <td>
                      <CategoryChip
                        name={x.categoryName}
                        typeLabel={x.categoryType}
                        fallback={t('bookings.sourceNone')}
                      />
                    </td>
                    <td>{intervalLabel(x.intervalMonths)}</td>
                    <td className="num">
                      <Money
                        cents={x.amountCents}
                        tone={x.kind === 'income' ? 'income' : 'expense'}
                      />
                    </td>
                    <td>
                      {booked ? (
                        <StatusPill tone="good">{t('recurring.alreadyBooked')}</StatusPill>
                      ) : due ? (
                        <StatusPill tone="info">{t('recurring.due')}</StatusPill>
                      ) : (
                        <span className="kpi__scope">{t('recurring.notDue')}</span>
                      )}
                    </td>
                    <td>
                      <button
                        type="button"
                        className="icon-button"
                        aria-label={t('recurring.editTemplate')}
                        onClick={() => {
                          setEditing(x);
                          setFormOpen(true);
                        }}
                      >
                        <Pencil size={15} aria-hidden="true" />
                      </button>
                      <button
                        type="button"
                        className="icon-button"
                        aria-label={t('common.delete')}
                        onClick={() => {
                          if (window.confirm(t('recurring.deleteConfirm', { name: x.name }))) {
                            remove.mutate(x.id);
                          }
                        }}
                      >
                        <Trash2 size={15} aria-hidden="true" />
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {selectable.length === 0 && templates.data && templates.data.length > 0 && (
        <p className="kpi__scope" style={{ marginTop: '.6rem' }}>
          {t('recurring.nothingDue')}
        </p>
      )}

      {drafts.data && drafts.data.items.length > 0 && (
        <DraftList year={year} items={drafts.data.items} />
      )}
    </>
  );
}

/**
 * Drafts waiting for their real amount.
 *
 * These are the reason `amountIsEstimate` exists, and they count towards nothing
 * until confirmed — which the copy says outright, because a list of bookings that
 * are invisible in every total is otherwise deeply confusing.
 */
function DraftList({ year, items }: { year: number; items: Booking[] }) {
  const t = useT();
  const client = useQueryClient();
  const [amounts, setAmounts] = useState<Record<string, string>>({});

  const confirm = useMutation({
    mutationFn: ({ id, amountCents }: { id: string; amountCents: number | null }) =>
      api<Booking>(`/bookings/${id}/confirm`, {
        method: 'POST',
        ...jsonBody({ amountCents }),
      }),
    onSuccess: () => invalidateAfterMaterialize(client, year),
  });

  return (
    <section style={{ marginTop: '1.5rem' }}>
      <h2>{t('recurring.drafts')}</h2>
      <p className="kpi__scope">{t('recurring.draftsIntro')}</p>
      <div className="panel table-wrap">
        <table className="data-table">
          <thead>
            <tr>
              <th>{t('bookings.comment')}</th>
              <th>{t('bookings.category')}</th>
              <th className="num">{t('recurring.amount')}</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {items.map((b) => (
              <tr key={b.id}>
                <td>
                  <DataLabel>{b.comment}</DataLabel>
                </td>
                <td>
                  <CategoryChip
                    name={b.categoryName}
                    typeLabel={b.categoryType}
                    fallback={t('bookings.sourceNone')}
                  />
                </td>
                <td className="num">
                  <input
                    className="input"
                    inputMode="decimal"
                    style={{ maxWidth: '7rem', textAlign: 'right' }}
                    aria-label={`${t('recurring.amount')} ${b.comment}`}
                    value={
                      amounts[b.id] ??
                      (b.amountCents / 100).toFixed(2).replace('.', ',')
                    }
                    onChange={(e) =>
                      setAmounts((prev) => ({ ...prev, [b.id]: e.target.value }))
                    }
                  />
                </td>
                <td>
                  <Button
                    variant="secondary"
                    busy={confirm.isPending && confirm.variables?.id === b.id}
                    onClick={() =>
                      confirm.mutate({
                        id: b.id,
                        amountCents: parseEuroInput(amounts[b.id] ?? ''),
                      })
                    }
                  >
                    <Check size={15} aria-hidden="true" /> {t('recurring.confirm')}
                  </Button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}
