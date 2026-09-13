import { Link2, Users2 } from 'lucide-react';

import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { Money } from '../../components/Money';
import { StatusPill } from '../../components/ui';
import { useT } from '../../lib/i18n';
import type { Booking } from '../../lib/types';

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
