import { useEffect, useState, type FormEvent } from 'react';
import { useMutation } from '@tanstack/react-query';

import { Button, ErrorState } from '../../components/ui';
import { api, jsonBody } from '../../lib/api';
import { MONTHS_DE, formatEuro, parseEuroInput } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { useMaskedFieldClass } from '../../lib/privacy';
import type { Category, SinkingFund } from '../../lib/types';

/**
 * A modal, like the booking and template editors. A form that opens at the top of
 * the page answers a click made at the bottom of a list somewhere the user cannot
 * see it.
 */
export function FundForm({
  fund,
  prefill,
  categories,
  onDone,
  onCancel,
}: {
  fund: SinkingFund | null;
  /** A suggestion the user accepted: the same form, already filled in. */
  prefill?: { name: string; categoryId: string; annualCents: number; dueMonth: number } | null;
  categories: Category[];
  onDone: (created: boolean) => void;
  onCancel: () => void;
}) {
  const t = useT();
  const maskedField = useMaskedFieldClass();

  const [name, setName] = useState(fund?.name ?? prefill?.name ?? '');
  const [categoryId, setCategoryId] = useState(fund?.categoryId ?? prefill?.categoryId ?? '');
  const [amount, setAmount] = useState(() => {
    const cents = fund?.annualCents ?? prefill?.annualCents;
    return cents ? formatEuro(cents).replace(/[^\d,.-]/g, '') : '';
  });
  const [dueMonth, setDueMonth] = useState(
    String(fund?.dueMonth ?? prefill?.dueMonth ?? new Date().getMonth() + 1),
  );
  const [note, setNote] = useState(fund?.note ?? '');
  const [active, setActive] = useState(fund?.active ?? true);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onCancel();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onCancel]);

  const save = useMutation({
    mutationFn: () => {
      const cents = parseEuroInput(amount);
      if (!cents || cents <= 0) throw new Error(t('funds.amountInvalid'));
      return api<SinkingFund>(fund ? `/funds/${fund.id}` : '/funds', {
        method: fund ? 'PUT' : 'POST',
        ...jsonBody({
          name: name.trim(),
          categoryId: categoryId || null,
          annualCents: cents,
          dueMonth: Number(dueMonth),
          note: note.trim() || null,
          active,
        }),
      });
    },
    onSuccess: () => onDone(!fund),
  });

  function submit(e: FormEvent) {
    e.preventDefault();
    save.mutate();
  }

  return (
    <>
      <div className="sheet-scrim" onClick={onCancel} aria-hidden="true" />
      <form
        className="dialog__panel booking-editor form-stack"
        role="dialog"
        aria-modal="true"
        aria-label={fund ? t('funds.edit') : t('funds.add')}
        onSubmit={submit}
      >
        <h2 style={{ margin: 0 }}>{fund ? t('funds.edit') : t('funds.add')}</h2>

        {save.isError && <ErrorState error={save.error} />}

        <div className="field">
          <label htmlFor="ff-name">{t('funds.name')}</label>
          <input
            id="ff-name"
            className="input"
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
          />
        </div>

        <div className="field">
          <label htmlFor="ff-category">{t('funds.category')}</label>
          <select
            id="ff-category"
            className="select"
            value={categoryId}
            onChange={(e) => setCategoryId(e.target.value)}
          >
            <option value="">{t('funds.noCategory')}</option>
            {categories.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
          <span className="footnote">{t('funds.noCategoryHint')}</span>
        </div>

        <div className="booking-editor__row">
          <div className="field">
            <label htmlFor="ff-amount">{t('funds.annual')}</label>
            <input
              id="ff-amount"
              className={`input ${maskedField}`}
              inputMode="decimal"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="ff-due">{t('funds.dueMonth')}</label>
            <select
              id="ff-due"
              className="select"
              value={dueMonth}
              onChange={(e) => setDueMonth(e.target.value)}
            >
              {MONTHS_DE.map((label, i) => (
                <option key={label} value={i + 1}>
                  {label}
                </option>
              ))}
            </select>
          </div>
        </div>

        <div className="field">
          <label htmlFor="ff-note">{t('funds.note')}</label>
          <input
            id="ff-note"
            className="input"
            value={note}
            onChange={(e) => setNote(e.target.value)}
          />
        </div>

        <label className="chip" style={{ cursor: 'pointer', minHeight: 'var(--tap)' }}>
          <input type="checkbox" checked={active} onChange={(e) => setActive(e.target.checked)} />
          {t('funds.active')}
        </label>

        <div style={{ display: 'flex', gap: '.5rem', flexWrap: 'wrap' }}>
          <Button type="submit" busy={save.isPending}>
            {t('common.save')}
          </Button>
          <Button type="button" variant="ghost" onClick={onCancel}>
            {t('common.cancel')}
          </Button>
        </div>
      </form>
    </>
  );
}
