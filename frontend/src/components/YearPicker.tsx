import { useQuery } from '@tanstack/react-query';

import { api } from '../lib/api';
import { useT } from '../lib/i18n';
import { qk } from '../lib/queryKeys';
import type { Year } from '../lib/types';

/**
 * The year to look at.
 *
 * A free-text number field invites 1823 and answers with an empty screen that
 * looks like data loss. The years that actually hold bookings are known — the
 * app maintains them — so this offers exactly those, newest first.
 *
 * The current year is included even when it has no bookings yet, because it is
 * where the next one will land.
 */
export function YearPicker({
  value,
  onChange,
  id = 'year-picker',
}: {
  value: number;
  onChange: (year: number) => void;
  id?: string;
}) {
  const t = useT();
  const years = useQuery({
    queryKey: qk.years(),
    queryFn: () => api<Year[]>('/years'),
    staleTime: 5 * 60_000,
  });

  const options = (() => {
    const known = Array.isArray(years.data) ? years.data.map((y) => y.year) : [];
    const set = new Set<number>(known);
    set.add(new Date().getFullYear());
    // Whatever is selected must be offered, or the select would silently show
    // something other than what the page is displaying.
    set.add(value);
    return [...set].sort((a, b) => b - a);
  })();

  return (
    <div className="field">
      <label htmlFor={id}>{t('common.year')}</label>
      <select
        id={id}
        className="select"
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
      >
        {options.map((year) => (
          <option key={year} value={year}>
            {year}
          </option>
        ))}
      </select>
    </div>
  );
}
