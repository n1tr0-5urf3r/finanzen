import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';

import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { Money } from '../../components/Money';
import { Banner, Button, EmptyState, ErrorState, LoadingState, StatusPill } from '../../components/ui';
import { api } from '../../lib/api';
import { formatDate } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import { sortedByName } from '../../lib/categories';
import type { Category, StatementRow, StatementRowPage } from '../../lib/types';

type Filter = '' | 'pending' | 'accepted' | 'rejected' | 'duplicates';

/**
 * A bank statement, line by line.
 *
 * The workbook review asks about a *comment* — one decision covers every booking
 * that shares it. A statement has no comments: it has `VISA SUPERMARKT SAGT DANKE` and
 * forty characters of card-terminal noise, and no two lines are alike. So this is
 * a row-by-row screen, and nothing is booked until a line has been looked at:
 * the commit takes only the accepted ones.
 *
 * Three things it has to get across without being read:
 *
 * * what the bank actually wrote, because that is the only evidence of what a
 *   line was — the payee, the purpose text and the date, verbatim;
 * * what the app guessed, and how sure it is: an exact rule match and a fuzzy
 *   containment hit look different and must not be confirmed with the same shrug;
 * * whether the ledger already holds this. A statement overlaps whatever you
 *   typed in by hand, and re-importing last month is the normal case.
 */
export function StatementReview({
  batchId,
  categories,
  applied,
}: {
  batchId: string;
  categories: Category[];
  applied: boolean;
}) {
  const t = useT();
  const client = useQueryClient();
  const [filter, setFilter] = useState<Filter>('pending');

  const query = useQuery({
    queryKey: qk.imports.statement(batchId, filter),
    queryFn: () =>
      api<StatementRowPage>(
        `/imports/${batchId}/statement?pageSize=200${filter ? `&filter=${filter}` : ''}`,
      ),
  });

  const patch = useMutation({
    mutationFn: ({ id, body }: { id: string; body: Record<string, unknown> }) =>
      api<StatementRow>(`/imports/${batchId}/statement/${id}`, {
        method: 'PATCH',
        body: JSON.stringify(body),
      }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: qk.imports.root });
    },
  });

  const bulk = useMutation({
    mutationFn: (body: { scope: string; decision: string }) =>
      api<{ affected: number }>(`/imports/${batchId}/statement/bulk`, {
        method: 'POST',
        body: JSON.stringify(body),
      }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: qk.imports.root });
    },
  });

  if (query.isLoading) return <LoadingState />;
  if (query.isError) return <ErrorState error={query.error} retry={() => query.refetch()} />;
  if (!query.data) return null;

  const { items, total, pending, accepted, rejected, duplicates } = query.data;
  const sorted = sortedByName(categories);

  const filters: [Filter, string, number][] = [
    ['pending', t('statement.filterPending'), pending],
    ['accepted', t('statement.filterAccepted'), accepted],
    ['rejected', t('statement.filterRejected'), rejected],
    ['duplicates', t('statement.filterDuplicates'), duplicates],
    ['', t('statement.filterAll'), total],
  ];

  return (
    <section>
      <div className="panel panel--pad statement__bar">
        <p className="statement__progress">
          {t('statement.progress', { accepted, total })}
          {rejected > 0 && ` · ${t('statement.rejectedCount', { count: rejected })}`}
        </p>
        {!applied && (
          <div className="statement__actions">
            <Button
              type="button"
              variant="ghost"
              busy={bulk.isPending}
              disabled={duplicates === 0}
              onClick={() => bulk.mutate({ scope: 'duplicates', decision: 'rejected' })}
            >
              {t('statement.skipDuplicates', { count: duplicates })}
            </Button>
            <Button
              type="button"
              variant="ghost"
              busy={bulk.isPending}
              onClick={() => bulk.mutate({ scope: 'ruleMatches', decision: 'accepted' })}
            >
              {t('statement.acceptRuleMatches')}
            </Button>
          </div>
        )}
      </div>

      {applied && <Banner tone="info">{t('statement.applied')}</Banner>}
      {bulk.isError && <ErrorState error={bulk.error} />}
      {patch.isError && <ErrorState error={patch.error} />}

      <div className="segmented tabs" role="group" aria-label={t('statement.filter')}>
        {filters.map(([value, label, count]) => (
          <button
            key={value || 'all'}
            type="button"
            aria-pressed={filter === value}
            onClick={() => setFilter(value)}
          >
            {`${label} (${count})`}
          </button>
        ))}
      </div>

      {items.length === 0 ? (
        <EmptyState hint={t('statement.empty')} />
      ) : (
        <div className="statement__rows">
          {items.map((row) => (
            <StatementLine
              key={row.id}
              row={row}
              categories={sorted}
              readOnly={applied}
              busy={patch.isPending}
              onChange={(body) => patch.mutate({ id: row.id, body })}
            />
          ))}
        </div>
      )}
    </section>
  );
}

function StatementLine({
  row,
  categories,
  readOnly,
  busy,
  onChange,
}: {
  row: StatementRow;
  categories: Category[];
  readOnly: boolean;
  busy: boolean;
  onChange: (body: Record<string, unknown>) => void;
}) {
  const t = useT();
  const [comment, setComment] = useState(row.comment);
  const [showPurpose, setShowPurpose] = useState(false);

  const category = categories.find((c) => c.id === row.categoryId) ?? null;
  const dirty = comment.trim() !== row.comment.trim();

  return (
    <article
      className={`statement__row statement__row--${row.decision}${
        row.duplicateBookingId ? ' statement__row--duplicate' : ''
      }`}
    >
      <header className="statement__head">
        <span className="statement__date">
          <DataLabel>{formatDate(row.bookedOn)}</DataLabel>
        </span>
        <span className="statement__amount">
          {/* What the bank did to the account, signed the way a statement reads.
              Gross on purpose: a statement line is a payment, not a category's
              net, and the "netto" marker would be claiming something else. */}
          <Money
            cents={row.kind === 'income' ? row.amountCents : -row.amountCents}
            signed
            credit={row.kind === 'income'}
            tone={row.kind === 'income' ? 'income' : 'expense'}
          />
        </span>
        {row.duplicateBookingId && (
          <StatusPill tone="warn">
            {t('statement.duplicateOf', {
              comment: row.duplicateComment ?? '',
              date: row.duplicateBookedOn ? formatDate(row.duplicateBookedOn) : '',
            })}
          </StatusPill>
        )}
        {row.decision === 'accepted' && <StatusPill tone="good">{t('statement.willBook')}</StatusPill>}
        {row.decision === 'rejected' && <StatusPill tone="neutral">{t('statement.wontBook')}</StatusPill>}
      </header>

      {/* What the bank wrote, verbatim. It is the evidence; everything else on
          this card is an interpretation of it. */}
      <p className="statement__party">
        <DataLabel>{row.counterparty ?? ''}</DataLabel>
        {row.purpose && (
          <button
            type="button"
            className="linkish statement__toggle"
            onClick={() => setShowPurpose((v) => !v)}
          >
            {showPurpose ? t('statement.hidePurpose') : t('statement.showPurpose')}
          </button>
        )}
      </p>
      {showPurpose && row.purpose && (
        <p className="statement__purpose">
          <DataLabel>{row.purpose}</DataLabel>
        </p>
      )}

      <div className="statement__fields">
        <div className="field">
          <label htmlFor={`comment-${row.id}`}>{t('bookings.comment')}</label>
          <input
            id={`comment-${row.id}`}
            className="input"
            value={comment}
            disabled={readOnly}
            onChange={(e) => setComment(e.target.value)}
            onBlur={() => dirty && onChange({ comment: comment.trim() })}
          />
        </div>

        <div className="field">
          <label htmlFor={`category-${row.id}`}>
            {t('bookings.category')}
            {row.categorySource === 'rule' && (
              <span className="kpi__scope"> · {t('statement.fromRule')}</span>
            )}
            {row.categorySource === 'suggestion' && (
              <span className="kpi__scope"> · {t('statement.guessed')}</span>
            )}
          </label>
          <select
            id={`category-${row.id}`}
            className="select"
            value={row.categoryId ?? ''}
            disabled={readOnly}
            onChange={(e) => onChange({ categoryId: e.target.value || null })}
          >
            <option value="">{t('statement.noCategory')}</option>
            {categories.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        </div>
      </div>

      {category && (
        <p className="statement__chip">
          <CategoryChip
            name={category.name}
            typeLabel={category.typeLabel}
            fallback={t('bookings.sourceNone')}
          />
        </p>
      )}

      {!readOnly && (
        <footer className="statement__foot">
          <label className="statement__rule">
            <input
              type="checkbox"
              checked={row.createRule}
              onChange={(e) => onChange({ createRule: e.target.checked })}
            />
            {t('statement.rememberRule')}
          </label>
          <div className="statement__decide">
            <Button
              type="button"
              busy={busy}
              onClick={() =>
                onChange({
                  comment: comment.trim(),
                  decision: row.decision === 'accepted' ? 'pending' : 'accepted',
                })
              }
            >
              {row.decision === 'accepted' ? t('statement.undo') : t('statement.accept')}
            </Button>
            <Button
              type="button"
              variant="ghost"
              busy={busy}
              onClick={() =>
                onChange({ decision: row.decision === 'rejected' ? 'pending' : 'rejected' })
              }
            >
              {row.decision === 'rejected' ? t('statement.undo') : t('statement.reject')}
            </Button>
          </div>
        </footer>
      )}

      {row.duplicateBookingId && (
        <p className="footnote">
          <Link to={`/buchungen?suche=${encodeURIComponent(row.duplicateComment ?? '')}`}>
            {t('statement.openDuplicate')}
          </Link>
        </p>
      )}
    </article>
  );
}
