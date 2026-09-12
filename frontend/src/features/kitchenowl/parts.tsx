import { Link2, Tag } from 'lucide-react';

import { DataLabel } from '../../components/DataLabel';
import { Money } from '../../components/Money';
import { useT } from '../../lib/i18n';
import type { KoExpense } from '../../lib/types';

/**
 * A KitchenOwl category.
 *
 * Deliberately **not** a `<CategoryChip>` and deliberately not in the type palette.
 * KitchenOwl's seven household labels and the app's 32 categories are unrelated
 * taxonomies that are never mapped onto each other, and making them look alike is
 * the fastest way to convince someone otherwise. Outlined, neutral, tagged.
 *
 * 73 of the 211 expenses in the overlapping period carry no category at all, so the
 * absent state is the common one and is drawn as an explicit, quiet label rather
 * than an empty cell or a warning.
 */
export function KoCategoryChip({ name }: { name: string | null | undefined }) {
  const t = useT();
  if (!name) {
    return <span className="ko-chip ko-chip--none">{t('ko.noKoCategory')}</span>;
  }
  return (
    <span className="ko-chip">
      <Tag size={12} aria-hidden="true" />
      <DataLabel>{name}</DataLabel>
    </span>
  );
}

/**
 * The full amount and the user's own share, always together and always labelled.
 *
 * These are the two most confusable numbers in the feature: same purchase, adjacent
 * cells, and adding them produces a plausible-looking figure that means nothing.
 * `<Money basis="household">` and `<Money basis="share">` each carry a visible
 * marker and say which they are in their accessible name.
 */
export function KoAmounts({ expense }: { expense: KoExpense }) {
  return (
    <>
      <td className="num">
        <Money cents={expense.amountCents} basis="household" />
      </td>
      <td className="num">
        <Money cents={expense.ownShareCents} basis="share" />
      </td>
    </>
  );
}

/** The split, as integer weights — never as percentages. */
export function KoSplit({ expense }: { expense: KoExpense }) {
  if (expense.paidFor.length === 0) return null;
  return (
    <span className="ko-split">
      {expense.paidFor.map((share) => (
        <span key={share.memberId} className="ko-split__part">
          <DataLabel>{share.name ?? `#${share.memberId}`}</DataLabel>
          <span className="ko-split__factor" aria-hidden="true">
            ×{share.factor}
          </span>
        </span>
      ))}
    </span>
  );
}

/**
 * Whether this expense is linked to a booking. "Not linked" is the ordinary
 * outcome, so it is stated plainly and never as a warning — the two ledgers are
 * not meant to reconcile.
 */
export function KoLinkState({ expense }: { expense: KoExpense }) {
  const t = useT();
  if (!expense.linkedBookingId) {
    return <span className="ko-link ko-link--none">{t('ko.notLinked')}</span>;
  }
  return (
    <span className="ko-link">
      <Link2 size={13} aria-hidden="true" />
      <DataLabel>{expense.linkedBookingComment ?? ''}</DataLabel>
    </span>
  );
}
