import { useId, useState, type ReactNode } from 'react';

import { formatEuro } from '../lib/format';
import { useT } from '../lib/i18n';
import type { BookingKind } from '../lib/types';

/**
 * `gross` is a plain amount. `net` is expenses minus income of the same category —
 * Miete reads 4.800,00 because a flatmate pays half of the 9.600,00 that actually
 * left the account. `signed` shows an explicit +/−.
 */
export type Basis = 'gross' | 'net' | 'signed';

interface MoneyProps {
  cents: number | null | undefined;
  basis?: Basis;
  tone?: BookingKind | 'credit' | 'auto';
  className?: string;
}

/**
 * The single place currency is rendered.
 *
 * No other component may call `formatEuro` directly — that rule is what lets the
 * net/gross distinction be carried in markup instead of depending on every call
 * site remembering it, and it is why a screen reader hears "4.800,00 Euro netto"
 * where a sighted user sees the superscript marker.
 */
export function Money({ cents, basis = 'gross', tone, className }: MoneyProps) {
  const t = useT();
  if (cents === null || cents === undefined) {
    return (
      <span className="money money--empty" aria-label={t('common.noValue')}>
        –
      </span>
    );
  }

  const resolved = tone === 'auto' ? (cents < 0 ? 'income' : 'expense') : tone;
  const isCredit = basis === 'net' && cents < 0;
  const text = formatEuro(cents, { showSign: basis === 'signed' });
  const basisWord = t(`money.basis.${basis}` as const);

  return (
    <span
      className={[
        'money',
        resolved ? `money--${resolved}` : '',
        isCredit ? 'money--credit' : '',
        className ?? '',
      ]
        .filter(Boolean)
        .join(' ')}
      aria-label={basisWord ? `${text} ${basisWord}` : text}
    >
      {text}
      {basis === 'net' && (
        <abbr className="money__basis" title={t('money.netExplainer')} aria-hidden="true">
          {t('money.netAbbr')}
        </abbr>
      )}
    </span>
  );
}

/**
 * A net figure is never a dead end: it opens the three lines that produced it.
 * Without this, "Miete 5.100" is simply wrong-looking to anyone who knows the rent
 * is 1.100 a month.
 */
export function NetBreakdown({
  incomeCents,
  expenseCents,
  netCents,
  bookingCount,
  children,
}: {
  incomeCents: number;
  expenseCents: number;
  netCents: number;
  bookingCount?: number;
  children?: ReactNode;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const id = useId();

  return (
    <span style={{ position: 'relative', display: 'inline-block' }}>
      <button
        type="button"
        className="net-cell"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen((v) => !v)}
      >
        <Money cents={netCents} basis="net" tone="auto" />
      </button>
      {open && (
        <span
          id={id}
          role="dialog"
          className="panel panel--pad"
          style={{
            position: 'absolute',
            right: 0,
            top: 'calc(100% + .25rem)',
            zIndex: 20,
            boxShadow: 'var(--shadow-3)',
          }}
        >
          <span className="net-breakdown">
            <span className="net-breakdown__row">
              <span>{t('bookings.expense')}</span>
              <Money cents={expenseCents} tone="expense" />
            </span>
            <span className="net-breakdown__row">
              <span>{t('bookings.income')}</span>
              <Money cents={-incomeCents} basis="signed" tone="income" />
            </span>
            <span className="net-breakdown__row net-breakdown__row--total">
              <span>{t('bookings.net')}</span>
              <Money cents={netCents} tone="auto" />
            </span>
            {bookingCount !== undefined && (
              <span className="net-breakdown__note">
                {t('import.reviewAffects', { count: bookingCount })}
              </span>
            )}
            {children}
          </span>
        </span>
      )}
    </span>
  );
}

/**
 * Whether transfers are inside a figure. A required prop on every KPI tile, so the
 * question cannot be quietly skipped six months from now.
 */
export function ScopeNote({ transfersIncluded }: { transfersIncluded: boolean }) {
  const t = useT();
  return (
    <span className="kpi__scope">
      {t(transfersIncluded ? 'scope.withTransfers' : 'scope.withoutTransfers')}
    </span>
  );
}
