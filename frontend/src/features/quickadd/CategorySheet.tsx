import { useMemo, useState } from 'react';

import { DataLabel } from '../../components/DataLabel';
import { useT } from '../../lib/i18n';
import { useKeyboardInset } from '../../lib/keyboard';
import type { Category } from '../../lib/types';
import { sortedByName } from '../../lib/categories';

/**
 * Choosing a category outright.
 *
 * Quick Add normally infers one from the comment via the rule table, which is
 * what makes it three taps. But a new account has no rules yet, and some
 * bookings genuinely do not follow one — so there has to be a way to say it
 * rather than only to imply it. Picking here sets a manual override, exactly as
 * the Buchungen screen does, and the override beats the rule.
 */
export function CategorySheet({
  categories,
  selectedId,
  onPick,
  onClose,
}: {
  categories: Category[];
  selectedId: string | null;
  onPick: (picked: { id: string; name: string } | null) => void;
  onClose: () => void;
}) {
  const t = useT();
  useKeyboardInset();
  const [query, setQuery] = useState('');

  // Grouped by type, because 32 categories in one flat list is a scroll and a
  // squint. Within a group they are alphabetical, like every other picker: the
  // app's own sort order is deliberate on the Kategorien screen and invisible
  // here, where you already know the name you are looking for.
  const groups = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase('de');
    const matching = needle
      ? categories.filter((c) => c.name.toLocaleLowerCase('de').includes(needle))
      : categories;

    const byType = new Map<string, Category[]>();
    for (const c of sortedByName(matching)) {
      const list = byType.get(c.typeLabel) ?? [];
      list.push(c);
      byType.set(c.typeLabel, list);
    }
    return [...byType.entries()];
  }, [categories, query]);

  return (
    <>
      <div className="sheet-scrim" onClick={onClose} aria-hidden="true" />
      <div className="sheet" role="dialog" aria-modal="true" aria-label={t('bookings.category')}>
        <div className="sheet__header">
          <input
            className="input"
            autoFocus
            inputMode="text"
            enterKeyHint="done"
            placeholder={t('quick.categorySearch')}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <div className="sheet__list">
          {/* Clearing hands the booking back to the rule table rather than
              pinning a category the user no longer wants. */}
          {selectedId && (
            <button className="sheet__item" onClick={() => onPick(null)}>
              <strong>{t('quick.categoryClear')}</strong>
            </button>
          )}
          {groups.map(([typeLabel, items]) => (
            <div key={typeLabel}>
              <p className="sheet__group">
                <DataLabel>{typeLabel}</DataLabel>
              </p>
              {items.map((c) => (
                <button
                  key={c.id}
                  className="sheet__item"
                  aria-pressed={c.id === selectedId}
                  onClick={() => onPick({ id: c.id, name: c.name })}
                >
                  <strong>
                    <DataLabel>{c.name}</DataLabel>
                  </strong>
                  {c.id === selectedId && <span aria-hidden="true">✓</span>}
                </button>
              ))}
            </div>
          ))}
          {groups.length === 0 && (
            <p className="sheet__group">{t('quick.categoryNoMatch')}</p>
          )}
        </div>
      </div>
    </>
  );
}
