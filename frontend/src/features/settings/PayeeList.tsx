import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Trash2 } from 'lucide-react';

import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { Button, EmptyState, ErrorState, LoadingState } from '../../components/ui';
import { api } from '../../lib/api';
import { sortedByName } from '../../lib/categories';
import { formatDate } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import type { Category, StatementPayee } from '../../lib/types';

/**
 * What each payee is called, in your own words.
 *
 * These are written by ticking a switch during a statement review, and until now
 * there was no way to see them again: a rename made once quietly governed every
 * future import of that payee. Renaming is cheap to get wrong — the same shop
 * changes what it sells — so the list is here, editable, and forgettable.
 *
 * Forgetting one is not destructive: the next statement simply asks about that
 * payee again, which is where the answer came from in the first place.
 */
export function PayeeList() {
  const t = useT();
  const client = useQueryClient();
  const [notice, setNotice] = useState<string | null>(null);

  const payees = useQuery({
    queryKey: qk.taxonomy.payees(),
    queryFn: () => api<StatementPayee[]>('/statement-payees'),
  });
  const categories = useQuery({
    queryKey: qk.taxonomy.categories(),
    queryFn: () => api<Category[]>('/categories'),
    staleTime: 30 * 60_000,
  });

  const save = useMutation({
    mutationFn: ({ id, body }: { id: string; body: Record<string, unknown> }) =>
      api<StatementPayee>(`/statement-payees/${id}`, {
        method: 'PATCH',
        body: JSON.stringify(body),
      }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: qk.taxonomy.payees() });
    },
  });

  const forget = useMutation({
    mutationFn: (payee: StatementPayee) =>
      api<void>(`/statement-payees/${payee.id}`, { method: 'DELETE' }),
    onSuccess: (_result, payee) => {
      void client.invalidateQueries({ queryKey: qk.taxonomy.payees() });
      setNotice(t('payees.forgotten', { comment: payee.comment }));
    },
  });

  if (payees.isLoading) return <LoadingState />;
  if (payees.isError) return <ErrorState error={payees.error} retry={() => payees.refetch()} />;

  const rows = payees.data ?? [];
  const options = sortedByName(categories.data ?? []);

  return (
    <section className="settings-section">
      <h2>{t('payees.title')}</h2>
      <p>{t('payees.intro')}</p>

      {save.isError && <ErrorState error={save.error} />}
      {forget.isError && <ErrorState error={forget.error} />}
      {notice && <p className="footnote">{notice}</p>}

      {rows.length === 0 ? (
        <EmptyState hint={t('payees.empty')} />
      ) : (
        <div className="payees">
          {rows.map((payee) => (
            <PayeeRow
              key={payee.id}
              payee={payee}
              categories={options}
              busy={save.isPending || forget.isPending}
              onSave={(body) => save.mutate({ id: payee.id, body })}
              onForget={() => forget.mutate(payee)}
            />
          ))}
        </div>
      )}
    </section>
  );
}

function PayeeRow({
  payee,
  categories,
  busy,
  onSave,
  onForget,
}: {
  payee: StatementPayee;
  categories: Category[];
  busy: boolean;
  onSave: (body: Record<string, unknown>) => void;
  onForget: () => void;
}) {
  const t = useT();
  const [comment, setComment] = useState(payee.comment);
  const dirty = comment.trim() !== payee.comment.trim();

  return (
    <article className="payee">
      {/* The bank's own spelling, which is the key this is stored under and the
          only thing a future statement will match on. */}
      <p className="payee__bank">
        <DataLabel>{payee.payee}</DataLabel>
      </p>

      <div className="payee__fields">
        <div className="field">
          <label htmlFor={`payee-comment-${payee.id}`}>{t('payees.calledThis')}</label>
          <input
            id={`payee-comment-${payee.id}`}
            className="input"
            value={comment}
            onChange={(e) => setComment(e.target.value)}
            onBlur={() => dirty && onSave({ comment: comment.trim() })}
          />
        </div>
        <div className="field">
          <label htmlFor={`payee-category-${payee.id}`}>{t('bookings.category')}</label>
          <select
            id={`payee-category-${payee.id}`}
            className="select"
            value={payee.categoryId ?? ''}
            onChange={(e) =>
              onSave(
                e.target.value
                  ? { categoryId: e.target.value }
                  : // An omitted field is null too, so "no category" has to say so.
                    { clearCategory: true },
              )
            }
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

      <footer className="payee__foot">
        <span className="footnote">
          {payee.categoryName && (
            <CategoryChip
              name={payee.categoryName}
              typeLabel={null}
              fallback={t('bookings.sourceNone')}
            />
          )}{' '}
          {t('payees.used', { count: payee.hits, date: formatDate(payee.updatedAt) })}
        </span>
        <Button type="button" variant="ghost" busy={busy} onClick={onForget}>
          <Trash2 size={15} aria-hidden="true" /> {t('payees.forget')}
        </Button>
      </footer>
    </article>
  );
}
