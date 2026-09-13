import { useId, useState, type ReactNode } from 'react';

import { formatEuro } from '../lib/format';
import { useT } from '../lib/i18n';
import { MASKED_AMOUNT, usePrivacy } from '../lib/privacy';
import type { MessageKey } from '../lib/messages/de';
import type { BookingKind } from '../lib/types';

/**
 * `gross` is a plain amount. `net` is expenses minus income of the same category —
 * Miete reads 4.800,00 because a flatmate pays half of the 9.600,00 that actually
 * left the account. `signed` shows an explicit +/−.
 *
 * `household` and `share` belong to the KitchenOwl ledger and exist because they are
 * the most confusable pair of numbers in the application: the same purchase has a
 * full amount the household spent and a slice the user owes, they sit next to each
 * other in every row, and neither is ever added to the other or to a booking. Both
 * carry a visible marker and say which they are in their accessible name, so the
 * distinction survives being read aloud, screenshotted or copied out of context.
 */
export type Basis = 'gross' | 'net' | 'signed' | 'household' | 'share';

/** Which bases wear a visible marker, and what it says. */
const MARKERS: Partial<Record<Basis, { abbr: MessageKey; title: MessageKey }>> = {
  net: { abbr: 'money.netAbbr', title: 'money.netExplainer' },
  household: { abbr: 'money.householdAbbr', title: 'money.householdExplainer' },
  share: { abbr: 'money.shareAbbr', title: 'money.shareExplainer' },
};

interface MoneyProps {
  cents: number | null | undefined;
  basis?: Basis;
  tone?: BookingKind | 'credit' | 'auto';
  /** Force an explicit +/− while keeping the basis marker. See `FlowMoney`. */
  signed?: boolean;
  /**
   * Whether this is money the category brought IN. Defaults to the sign of a net
   * figure, which is only correct while the stored expense-positive sign is the
   * one being displayed — `FlowMoney` shows the opposite sign and says so here.
   */
  credit?: boolean;
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
export function Money({ cents, basis = 'gross', tone, signed, credit, className }: MoneyProps) {
  const t = useT();
  const { hidden } = usePrivacy();
  if (cents === null || cents === undefined) {
    return (
      <span className="money money--empty" aria-label={t('common.noValue')}>
        –
      </span>
    );
  }

  const resolved = tone === 'auto' ? (cents < 0 ? 'income' : 'expense') : tone;
  const isCredit = credit ?? (basis === 'net' && cents < 0);
  // Privacy mode replaces the figure and its accessible name, and nothing else:
  // the tone, the credit styling and the netto marker all describe what KIND of
  // number this is, which is the part that stays useful with the amount gone.
  const text = hidden
    ? MASKED_AMOUNT
    : formatEuro(cents, { showSign: signed || basis === 'signed' });
  const basisWord = t(`money.basis.${basis}` as const);
  const marker = MARKERS[basis];

  return (
    <span
      className={[
        'money',
        resolved ? `money--${resolved}` : '',
        isCredit ? 'money--credit' : '',
        basis === 'share' ? 'money--share' : '',
        className ?? '',
      ]
        .filter(Boolean)
        .join(' ')}
      aria-label={
        hidden
          ? t('privacy.hiddenValue')
          : basisWord
            ? `${text} ${basisWord}`
            : text
      }
    >
      {text}
      {marker && (
        <abbr className="money__basis" title={t(marker.title)} aria-hidden="true">
          {t(marker.abbr)}
        </abbr>
      )}
    </span>
  );
}

/**
 * The same net figure, oriented the way a bank statement orients it: POSITIVE
 * means money came in, negative means it went out.
 *
 * The stored convention is the opposite — `net_cents` is expenses minus income,
 * so a category that earned money is negative. That is right for "what did Miete
 * cost me" and actively misleading as soon as income categories sit in the same
 * column: Gehalt reading −22.000,00 says "lost" to every reader, when it is the
 * one row that is unambiguously a gain.
 *
 * So the arithmetic keeps its sign and the DISPLAY flips it, in one place. The
 * tone is taken from the stored value, not the flipped one, so the colour still
 * says income or expense rather than following the minus sign around.
 */
export function FlowMoney({
  netCents,
  flowCents,
  className,
}: {
  /** A stored net: expenses minus income, so a category that earned money is negative. */
  netCents?: number | null;
  /** Already oriented as a flow — a balance, a running total — so it is shown as it is. */
  flowCents?: number | null;
  className?: string;
}) {
  const flow = flowCents !== undefined ? flowCents : netCents == null ? null : -netCents;
  if (flow === null || flow === undefined) return <Money cents={null} />;
  // Stated, not inferred. `Money` derives "is this a credit" from a NEGATIVE net,
  // which is the stored convention; handed the flipped value it marks every
  // ordinary expense as a credit and colours the whole column backwards.
  const credit = flow > 0;
  return (
    <Money
      cents={flow}
      basis="net"
      signed
      credit={credit}
      tone={credit ? 'income' : 'expense'}
      className={className}
    />
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
  orientation = 'flow',
  children,
}: {
  incomeCents: number;
  expenseCents: number;
  netCents: number;
  bookingCount?: number;
  /**
   * `flow` — positive is money in. Right wherever the column can hold either
   * direction, which is most places.
   *
   * `cost` — positive is what it cost, the stored sign. Right in a ranking of
   * costs, where no income category can appear and a column of minus signs would
   * be noise. The popover follows the headline either way: a total that disagrees
   * with the rows above it is worse than either convention.
   */
  orientation?: 'flow' | 'cost';
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
        {orientation === 'cost' ? (
          <Money cents={netCents} basis="net" tone="auto" />
        ) : (
          <FlowMoney netCents={netCents} />
        )}
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
            {/* Read in the order money moves: what came in, what went out, what
                is left — so the signs add up on the page, which the old "expense
                minus income" ordering only did if you already knew the
                convention. Under `cost` the same three lines are stated the way
                the column states them. */}
            {orientation === 'cost' ? (
              <>
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
                  <Money cents={netCents} basis="net" tone="auto" />
                </span>
              </>
            ) : (
              <>
                <span className="net-breakdown__row">
                  <span>{t('bookings.income')}</span>
                  <Money cents={incomeCents} basis="signed" tone="income" />
                </span>
                <span className="net-breakdown__row">
                  <span>{t('bookings.expense')}</span>
                  <Money cents={-expenseCents} basis="signed" tone="expense" />
                </span>
                <span className="net-breakdown__row net-breakdown__row--total">
                  <span>{t('bookings.net')}</span>
                  <FlowMoney netCents={netCents} />
                </span>
              </>
            )}
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
