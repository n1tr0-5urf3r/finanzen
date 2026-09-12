import { useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { DataLabel } from '../../components/DataLabel';
import { Button } from '../../components/ui';
import { api, errorMessage, jsonBody } from '../../lib/api';
import { formatEuro, MONTHS_DE, parseEuroInput } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { invalidateAfterBookingChange, qk } from '../../lib/queryKeys';
import type { Booking, BookingKind, Category } from '../../lib/types';

/**
 * Editing one booking.
 *
 * The category select is the reason this exists: a rule assigns a category to
 * every booking sharing a comment, but sometimes one of them is different — the
 * spreadsheet did exactly this three times, sending two hotels and a train to
 * Dienstreisen while their comments said otherwise. Choosing here sets a manual
 * override, which survives every later rule change; clearing it hands the booking
 * back to the rule table.
 */
export function BookingEditor({
  booking,
  onClose,
}: {
  booking: Booking;
  onClose: () => void;
}) {
  const t = useT();
  const client = useQueryClient();

  const [comment, setComment] = useState(booking.comment);
  const [amount, setAmount] = useState(formatEuro(booking.amountCents).replace(/[^\d,.-]/g, ''));
  const [kind, setKind] = useState<BookingKind>(booking.kind);
  const [month, setMonth] = useState(booking.month);
  const [year, setYear] = useState(booking.year);
  const [taxRelevant, setTaxRelevant] = useState(booking.taxRelevant);
  // `''` means "let the rule decide"; a value is an explicit manual override.
  const [categoryId, setCategoryId] = useState(
    booking.categorySource === 'manual' ? (booking.categoryId ?? '') : '',
  );
  const [error, setError] = useState<unknown>(null);

  const categories = useQuery({
    queryKey: qk.taxonomy.categories(),
    queryFn: () => api<Category[]>('/categories'),
    staleTime: 30 * 60_000,
  });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const save = useMutation({
    mutationFn: () => {
      const cents = parseEuroInput(amount);
      if (!cents || cents <= 0) throw new Error(t('bookings.amountInvalid'));
      return api<Booking>(`/bookings/${booking.id}`, {
        method: 'PUT',
        ...jsonBody({
          year,
          month,
          kind,
          amountCents: cents,
          comment: comment.trim(),
          taxRelevant,
          ...(categoryId
            ? { categoryId }
            : // Explicitly hand it back to the rule table rather than leaving a
              // stale manual override in place.
              { clearCategoryOverride: true }),
        }),
      });
    },
    onSuccess: (updated) => {
      invalidateAfterBookingChange(client, [booking.year, updated.year]);
      client.invalidateQueries({ queryKey: qk.bookings.comments() });
      onClose();
    },
    onError: (e) => setError(e),
  });

  const remove = useMutation({
    mutationFn: () => api<void>(`/bookings/${booking.id}`, { method: 'DELETE' }),
    onSuccess: () => {
      invalidateAfterBookingChange(client, [booking.year]);
      onClose();
    },
    onError: (e) => setError(e),
  });

  return (
    <>
      <div className="sheet-scrim" onClick={onClose} aria-hidden="true" />
      <div
        className="dialog__panel booking-editor"
        role="dialog"
        aria-modal="true"
        aria-label={t('bookings.edit')}
      >
        <header>
          <h2>{t('bookings.edit')}</h2>
        </header>

        <div className="form-stack">
          <div className="field">
            <label htmlFor="be-comment">{t('bookings.comment')}</label>
            <input
              id="be-comment"
              className="input"
              value={comment}
              onChange={(e) => setComment(e.target.value)}
            />
          </div>

          <div className="booking-editor__row">
            <div className="field">
              <label htmlFor="be-amount">{t('bookings.amount')}</label>
              <input
                id="be-amount"
                className="input"
                inputMode="decimal"
                value={amount}
                onChange={(e) => setAmount(e.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor="be-kind">{t('bookings.kindLabel')}</label>
              <select
                id="be-kind"
                className="select"
                value={kind}
                onChange={(e) => setKind(e.target.value as BookingKind)}
              >
                <option value="expense">{t('bookings.kind.expense')}</option>
                <option value="income">{t('bookings.kind.income')}</option>
                <option value="transfer">{t('bookings.kind.transfer')}</option>
              </select>
            </div>
          </div>

          <div className="booking-editor__row">
            <div className="field">
              <label htmlFor="be-month">{t('common.month')}</label>
              <select
                id="be-month"
                className="select"
                value={month}
                onChange={(e) => setMonth(Number(e.target.value))}
              >
                {MONTHS_DE.map((name, index) => (
                  <option key={name} value={index + 1}>
                    {name}
                  </option>
                ))}
              </select>
            </div>
            <div className="field">
              <label htmlFor="be-year">{t('common.year')}</label>
              <input
                id="be-year"
                className="input"
                type="number"
                value={year}
                onChange={(e) => setYear(Number(e.target.value))}
              />
            </div>
          </div>

          <div className="field">
            <label htmlFor="be-category">{t('bookings.category')}</label>
            <select
              id="be-category"
              className="select"
              value={categoryId}
              onChange={(e) => setCategoryId(e.target.value)}
            >
              <option value="">{t('bookings.categoryFromRule')}</option>
              {/* Defensive: a failed or in-flight categories fetch must leave the
                  dialog usable rather than crashing it — everything else in the
                  form still works without the list. */}
              {(Array.isArray(categories.data) ? categories.data : []).map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name} · {c.typeLabel}
                </option>
              ))}
            </select>
            <small>
              {categoryId
                ? t('bookings.categoryManualHint')
                : booking.categoryName
                  ? t('bookings.categoryRuleHint', { category: booking.categoryName })
                  : t('bookings.categoryNoneHint')}
            </small>
          </div>

          <label className="chip booking-editor__tax">
            <input
              type="checkbox"
              checked={taxRelevant}
              onChange={(e) => setTaxRelevant(e.target.checked)}
            />
            {t('bookings.taxRelevant')}
          </label>

          {booking.externalSource && (
            <p className="footnote">
              <DataLabel>{booking.externalSource}</DataLabel> · {t('bookings.linked')}
            </p>
          )}

          {error != null && (
            <p role="alert" style={{ color: 'var(--danger)', fontSize: '.85rem' }}>
              {errorMessage(error)}
            </p>
          )}
        </div>

        <footer>
          <Button
            variant="danger"
            busy={remove.isPending}
            onClick={() => {
              if (window.confirm(t('bookings.deleteConfirm'))) remove.mutate();
            }}
          >
            {t('common.delete')}
          </Button>
          <div style={{ flex: 1 }} />
          <Button variant="secondary" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button busy={save.isPending} onClick={() => save.mutate()}>
            {t('common.save')}
          </Button>
        </footer>
      </div>
    </>
  );
}
