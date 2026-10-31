import { Link } from 'react-router-dom';
import { Users } from 'lucide-react';

import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { FlowMoney, Money } from '../../components/Money';
import { Banner } from '../../components/ui';
import { formatDate } from '../../lib/format';
import { useT } from '../../lib/i18n';
import type { HouseholdShareItem } from '../../lib/types';

/**
 * My shares of what the flatmates paid, as rows of a drilldown table under its
 * bookings.
 *
 * They count in the figures above, so a list that left them out would not add up
 * — but they are not bookings: no money of mine moved, the booking list cannot
 * show them, and nothing here can edit them (they change in KitchenOwl, their
 * category through the KitchenOwl page's category map). So they sit in the same
 * columns as the bookings, which keeps the list one list, under a heading row that
 * says whose they are, with who paid and the whole purchase as a quiet second line (the month is in
 * its column already; the exact date is the line's tooltip).
 */
export function HouseholdRows({ items }: { items: HouseholdShareItem[] }) {
  const t = useT();
  if (items.length === 0) return null;
  return (
    <tbody className="ko-household-items">
      <tr className="ko-household-items__head">
        <th colSpan={4} scope="rowgroup">
          <span className="ko-household-items__title">
            <Users size={13} aria-hidden="true" />
            {t('analysis.householdItems', { count: items.length })}
          </span>
        </th>
      </tr>
      {items.map((item) => (
        <tr key={item.id} className="ko-household-items__row">
          <td>
            <DataLabel>{item.monthName}</DataLabel>
          </td>
          <td>
            <DataLabel>{item.name}</DataLabel>
            <span className="ko-household-items__meta" title={formatDate(item.date)}>
              {item.paidByName
                ? t('analysis.householdPaidBy', { name: item.paidByName })
                : t('analysis.householdPaidByUnknown')}{' '}
              · <Money cents={item.amountCents} basis="household" />
            </span>
          </td>
          <td>
            <CategoryChip
              name={item.categoryName}
              typeLabel={item.categoryType}
              fallback={t('bookings.sourceNone')}
            />
          </td>
          <td className="num">
            <FlowMoney netCents={item.ownShareCents} />
          </td>
        </tr>
      ))}
    </tbody>
  );
}

/**
 * Household shares counted without a category, because their KitchenOwl category
 * is not mapped yet. Pointed at the map rather than at the booking list, which
 * cannot show them.
 */
export function HouseholdUncategorizedBanner({ count }: { count: number | undefined }) {
  const t = useT();
  if (!count) return null;
  return (
    <Banner tone="warn">
      <Link to="/kitchenowl?ansicht=categories">
        {t('analysis.householdUncategorized', { count })}
      </Link>
    </Banner>
  );
}
