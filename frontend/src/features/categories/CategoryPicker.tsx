import type { Category } from '../../lib/types';

/**
 * A category chooser.
 *
 * Grouped by type, and every visible string — the option labels and the group
 * labels alike — comes from the database. `lang="de"` on the select marks the
 * whole control as data, which is the closest a native `<select>` gets to
 * `<DataLabel>`; the options themselves cannot carry elements.
 */
export function CategoryPicker({
  id,
  categories,
  value,
  onChange,
  allowEmpty,
  emptyLabel,
  exclude,
}: {
  id: string;
  categories: Category[];
  value: string | null;
  onChange: (id: string | null) => void;
  allowEmpty?: boolean;
  emptyLabel?: string;
  exclude?: string;
}) {
  const groups = new Map<string, Category[]>();
  for (const c of categories) {
    if (c.id === exclude) continue;
    const list = groups.get(c.typeLabel) ?? [];
    list.push(c);
    groups.set(c.typeLabel, list);
  }

  return (
    <select
      id={id}
      className="select"
      lang="de"
      value={value ?? ''}
      onChange={(e) => onChange(e.target.value || null)}
    >
      {allowEmpty && <option value="">{emptyLabel ?? '—'}</option>}
      {[...groups.entries()].map(([label, items]) => (
        <optgroup key={label} label={label}>
          {items.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </optgroup>
      ))}
    </select>
  );
}
