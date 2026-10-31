import { Link2, Users2 } from 'lucide-react';

import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { Money } from '../../components/Money';
import { StatusPill } from '../../components/ui';
import { formatEuro } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { useMaskAmount } from '../../lib/privacy';
import type { Booking } from '../../lib/types';

/**
 * What the statistics count of a booking, under it, when that is not the bank amount.
 *
 * "dein Anteil: 30,00 €" under a booking linked to a KitchenOwl expense: the row
 * shows what the bank moved and every statistic counts only my share, so without
 * this a monthly total comes out lower than the rows behind it and nothing on
 * screen says why.
 *
 * "gezählt: 0,00 €" under a household settlement, which counts net of the others'
 * shares of my linked purchases — and "gezählt als Ausgabe: 40,00 €" when those
 * shares outweigh it and it lands on the other side of the totals.
 *
 * Nothing is shown when the counted figure is the booking itself — there is no
 * difference to explain.
 */
export function CountedShare({ booking: b }: { booking: Booking }) {
  const t = useT();
  const mask = useMaskAmount();
  if (b.countedAmountCents == null) return null;
  const kind = b.countedKind ?? b.kind;
  if (b.countedAmountCents === b.amountCents && kind === b.kind) return null;
  const amount = mask(formatEuro(b.countedAmountCents));
  if (b.countedBasis === 'koSettlement') {
    const label =
      kind === b.kind
        ? t('bookings.countedSettlement', { amount })
        : kind === 'expense'
          ? t('bookings.countedSettlementAsExpense', { amount })
          : t('bookings.countedSettlementAsIncome', { amount });
    return (
      <span className="booking-row__share" title={t('bookings.countedSettlementTitle')}>
        {label}
      </span>
    );
  }
  return (
    <span className="booking-row__share" title={t('bookings.countedShareTitle')}>
      {t('bookings.countedShare', { amount })}
    </span>
  );
}

/**
 * One booking as a card.
 *
 * Extracted from the bookings screen rather than copied: it is the rendering the
 * phone has used since the tables became unreadable at 375px, and the search
 * results are the same rows answering a different question. Two copies of this
 * markup would drift the moment one of them grew a pill.
 *
 * `onPush` is optional because sharing a booking is a KitchenOwl action and the
 * column only exists where KitchenOwl is configured. `showYear` likewise: the
 * bookings screen is scoped to one year and would only be repeating itself, while
 * a search result without a year is four possible answers.
 */
export function BookingCard({
  booking: b,
  onOpen,
  onPush,
  showYear = false,
}: {
  booking: Booking;
  onOpen: (booking: Booking) => void;
  onPush?: (booking: Booking) => void;
  showYear?: boolean;
}) {
  const t = useT();

  return (
    <article
      role="button"
      tabIndex={0}
      onClick={() => onOpen(b)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onOpen(b);
        }
      }}
      className={`mcard bcard bcard--clickable ${
        b.categorySource === 'unresolved' && b.kind !== 'transfer'
          ? 'bcard--uncategorized'
          : b.kind === 'transfer'
            ? 'bcard--transfer'
            : ''
      }`}
    >
      <header>
        <strong>
          <DataLabel>{b.comment}</DataLabel>
        </strong>
        {/* The amount is the reason you opened the row, so it leads. */}
        <Money
          cents={b.kind === 'income' ? b.amountCents : -b.amountCents}
          basis="signed"
          tone={b.kind}
        />
      </header>
      <div className="bcard__meta">
        <DataLabel>{showYear ? `${b.monthName} ${b.year}` : b.monthName}</DataLabel>
        {!b.bookedOn && <span title={t('bookings.noDay')}> ·</span>}
        <CategoryChip
          name={b.categoryName}
          typeLabel={b.categoryType}
          fallback={t('bookings.sourceNone')}
        />
        {b.categorySource === 'manual' && (
          <StatusPill tone="info">{t('bookings.sourceManual')}</StatusPill>
        )}
        {b.kind === 'transfer' && (
          <StatusPill tone="neutral">{t('bookings.kind.transfer')}</StatusPill>
        )}
        {b.taxRelevant && <StatusPill tone="danger">{t('bookings.tax')}</StatusPill>}
        <CountedShare booking={b} />
      </div>

      {onPush && (
        <div className="bcard__actions">
          {b.externalSource === 'kitchenowl' ? (
            <span className="ko-link">
              <Link2 size={14} aria-hidden="true" /> {t('ko.linked')}
            </span>
          ) : b.kind === 'transfer' || b.status !== 'confirmed' ? null : (
            <button
              type="button"
              className="button button--secondary"
              aria-label={`${t('ko.pushTitle')}: ${b.comment}`}
              onClick={(e) => {
                // The card itself opens the editor, so this must not bubble or
                // sharing would also start an edit.
                e.stopPropagation();
                onPush(b);
              }}
            >
              <Users2 size={15} aria-hidden="true" /> {t('ko.push')}
            </button>
          )}
        </div>
      )}
    </article>
  );
}
