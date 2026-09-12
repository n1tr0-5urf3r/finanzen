import { useState, type FormEvent } from 'react';
import { useMutation } from '@tanstack/react-query';

import { Button, ErrorState } from '../../components/ui';
import { api, jsonBody } from '../../lib/api';
import { MONTHS_DE, parseEuroInput } from '../../lib/format';
import { useT } from '../../lib/i18n';
import type { BookingKind, Category, Period, RecurringTemplate } from '../../lib/types';
import { useMaskedFieldClass } from '../../lib/privacy';

/**
 * Create or edit a template.
 *
 * The anchor is not asked for. It defaults server-side to `activeFrom`, which is
 * what makes a quarterly template mean "this month, then every third" — the way
 * anybody would describe it — instead of a second date field nobody understands.
 */
export function TemplateForm({
  template,
  categories,
  defaultPeriod,
  onDone,
  onCancel,
}: {
  template: RecurringTemplate | null;
  categories: Category[];
  defaultPeriod: Period;
  onDone: () => void;
  onCancel: () => void;
}) {
  const t = useT();
  const maskedField = useMaskedFieldClass();
  const [name, setName] = useState(template?.name ?? '');
  const [comment, setComment] = useState(template?.comment ?? '');
  const [kind, setKind] = useState<BookingKind>(template?.kind ?? 'expense');
  const [amount, setAmount] = useState(
    template ? (template.amountCents / 100).toFixed(2).replace('.', ',') : '',
  );
  const [estimate, setEstimate] = useState(template?.amountIsEstimate ?? false);
  const [categoryId, setCategoryId] = useState(template?.categoryId ?? '');
  const [taxRelevant, setTaxRelevant] = useState(template?.taxRelevant ?? false);
  const [dayOfMonth, setDayOfMonth] = useState(String(template?.dayOfMonth ?? 1));
  const [intervalMonths, setIntervalMonths] = useState(String(template?.intervalMonths ?? 1));
  const [fromYear, setFromYear] = useState(
    String(template?.activeFrom.year ?? defaultPeriod.year),
  );
  const [fromMonth, setFromMonth] = useState(
    String(template?.activeFrom.month ?? defaultPeriod.month),
  );
  const [active, setActive] = useState(template?.active ?? true);

  const save = useMutation({
    mutationFn: () => {
      const body = {
        name,
        comment,
        kind,
        amountCents: parseEuroInput(amount) ?? 0,
        amountIsEstimate: estimate,
        categoryId: categoryId || null,
        taxRelevant,
        dayOfMonth: Number(dayOfMonth) || 1,
        intervalMonths: Number(intervalMonths) || 1,
        activeFrom: { year: Number(fromYear), month: Number(fromMonth) },
        active,
      };
      return template
        ? api<RecurringTemplate>(`/recurring/${template.id}`, {
            method: 'PUT',
            ...jsonBody(body),
          })
        : api<RecurringTemplate>('/recurring', { method: 'POST', ...jsonBody(body) });
    },
    onSuccess: onDone,
  });

  function submit(event: FormEvent) {
    event.preventDefault();
    save.mutate();
  }

  return (
    <form className="panel panel--pad form-stack" style={{ marginBottom: '1rem' }} onSubmit={submit}>
      <h2 style={{ margin: 0 }}>
        {template ? t('recurring.editTemplate') : t('recurring.newTemplate')}
      </h2>

      {save.isError && <ErrorState error={save.error} />}

      <div style={{ display: 'flex', gap: '.75rem', flexWrap: 'wrap' }}>
        <div className="field" style={{ flex: 1, minWidth: '10rem' }}>
          <label htmlFor="tf-name">{t('recurring.name')}</label>
          <input
            id="tf-name"
            className="input"
            required
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </div>
        <div className="field" style={{ flex: 1, minWidth: '10rem' }}>
          <label htmlFor="tf-comment">{t('bookings.comment')}</label>
          {/* The comment is what the rule table matches on, so it is the booking's
              own words — not a label. */}
          <input
            id="tf-comment"
            className="input"
            required
            lang="de"
            value={comment}
            onChange={(e) => setComment(e.target.value)}
          />
        </div>
      </div>

      <div style={{ display: 'flex', gap: '.75rem', flexWrap: 'wrap' }}>
        <div className="field" style={{ minWidth: '8rem' }}>
          <label htmlFor="tf-kind">{t('bookings.type')}</label>
          <select
            id="tf-kind"
            className="select"
            value={kind}
            onChange={(e) => setKind(e.target.value as BookingKind)}
          >
            <option value="expense">{t('bookings.kind.expense')}</option>
            <option value="income">{t('bookings.kind.income')}</option>
            <option value="transfer">{t('bookings.kind.transfer')}</option>
          </select>
        </div>
        <div className="field" style={{ minWidth: '8rem' }}>
          <label htmlFor="tf-amount">{t('recurring.amount')}</label>
          <input
            id="tf-amount"
            className={`input ${maskedField}`}
            inputMode="decimal"
            required
            value={amount}
            onChange={(e) => setAmount(e.target.value)}
          />
        </div>
        <div className="field" style={{ minWidth: '11rem' }}>
          <label htmlFor="tf-category">{t('bookings.category')}</label>
          <select
            id="tf-category"
            className="select"
            value={categoryId}
            onChange={(e) => setCategoryId(e.target.value)}
          >
            {/* Empty means "let the rule table decide at booking time", which keeps
                a rule change reaching next month's booking. */}
            <option value="">–</option>
            {categories.map((c) => (
              <option key={c.id} value={c.id} lang="de">
                {c.name}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div style={{ display: 'flex', gap: '.75rem', flexWrap: 'wrap' }}>
        <div className="field" style={{ minWidth: '9rem' }}>
          <label htmlFor="tf-interval">{t('recurring.interval')}</label>
          <select
            id="tf-interval"
            className="select"
            value={intervalMonths}
            onChange={(e) => setIntervalMonths(e.target.value)}
          >
            <option value="1">{t('recurring.interval.1')}</option>
            <option value="3">{t('recurring.interval.3')}</option>
            <option value="6">{t('recurring.interval.6')}</option>
            <option value="12">{t('recurring.interval.12')}</option>
          </select>
        </div>
        <div className="field" style={{ minWidth: '7rem' }}>
          <label htmlFor="tf-day">{t('recurring.dayOfMonth')}</label>
          <input
            id="tf-day"
            className="input"
            type="number"
            min={1}
            max={31}
            value={dayOfMonth}
            onChange={(e) => setDayOfMonth(e.target.value)}
          />
        </div>
        <div className="field" style={{ minWidth: '9rem' }}>
          <label htmlFor="tf-from-month">{t('recurring.from')}</label>
          <select
            id="tf-from-month"
            className="select"
            value={fromMonth}
            onChange={(e) => setFromMonth(e.target.value)}
          >
            {MONTHS_DE.map((label, index) => (
              <option key={label} value={index + 1}>
                {label}
              </option>
            ))}
          </select>
        </div>
        <div className="field" style={{ minWidth: '6rem' }}>
          <label htmlFor="tf-from-year">{t('common.year')}</label>
          <input
            id="tf-from-year"
            className="input"
            type="number"
            value={fromYear}
            onChange={(e) => setFromYear(e.target.value)}
          />
        </div>
      </div>

      <label className="chip" style={{ cursor: 'pointer' }} title={t('recurring.estimateHint')}>
        <input
          type="checkbox"
          checked={estimate}
          onChange={(e) => setEstimate(e.target.checked)}
        />
        {t('recurring.estimate')}
      </label>
      <label className="chip" style={{ cursor: 'pointer' }}>
        <input
          type="checkbox"
          checked={taxRelevant}
          onChange={(e) => setTaxRelevant(e.target.checked)}
        />
        {t('bookings.tax')}
      </label>
      <label className="chip" style={{ cursor: 'pointer' }}>
        <input type="checkbox" checked={active} onChange={(e) => setActive(e.target.checked)} />
        {t('recurring.activeLabel')}
      </label>

      <div style={{ display: 'flex', gap: '.5rem' }}>
        <Button type="submit" busy={save.isPending}>
          {t('common.save')}
        </Button>
        <Button type="button" variant="ghost" onClick={onCancel}>
          {t('common.cancel')}
        </Button>
      </div>
    </form>
  );
}
