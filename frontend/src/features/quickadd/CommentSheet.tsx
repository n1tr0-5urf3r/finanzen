import { useMemo, useState } from 'react';

import { DataLabel } from '../../components/DataLabel';
import { useT } from '../../lib/i18n';
import { useKeyboardInset } from '../../lib/keyboard';
import type { CommentSummary, Rule } from '../../lib/types';

/**
 * Reached only for a comment that is not already on a tile. Searches the cached
 * rule keys and the user's own comment history together, so a rare-but-known
 * comment is recognised rather than retyped.
 */
export function CommentSheet({
  rules,
  comments,
  onPick,
  onClose,
}: {
  rules: Rule[];
  comments: CommentSummary[];
  onPick: (comment: string) => void;
  onClose: () => void;
}) {
  const t = useT();
  useKeyboardInset();
  const [query, setQuery] = useState('');

  const options = useMemo(() => {
    const seen = new Set<string>();
    const merged: { comment: string; category: string | null }[] = [];
    for (const c of comments) {
      const key = c.comment.toLocaleLowerCase('de');
      if (!seen.has(key)) {
        seen.add(key);
        merged.push({ comment: c.comment, category: c.categoryName });
      }
    }
    for (const r of rules) {
      const key = r.normalizedComment;
      if (!seen.has(key)) {
        seen.add(key);
        merged.push({ comment: r.comment, category: r.categoryName });
      }
    }
    const needle = query.trim().toLocaleLowerCase('de');
    if (!needle) return merged.slice(0, 60);
    return merged
      .filter((o) => o.comment.toLocaleLowerCase('de').includes(needle))
      .slice(0, 60);
  }, [comments, rules, query]);

  const typed = query.trim();
  const exactExists = options.some(
    (o) => o.comment.toLocaleLowerCase('de') === typed.toLocaleLowerCase('de'),
  );

  return (
    <>
      <div
        style={{ position: 'fixed', inset: 0, background: 'rgba(14,17,19,.55)', zIndex: 45 }}
        onClick={onClose}
        aria-hidden="true"
      />
      <div className="sheet" role="dialog" aria-modal="true" aria-label={t('bookings.comment')}>
        <div className="sheet__header">
          <input
            className="input"
            autoFocus
            inputMode="text"
            enterKeyHint="done"
            placeholder={t('quick.commentSearch')}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <div className="sheet__list">
          {typed && !exactExists && (
            <button className="sheet__item" onClick={() => onPick(typed)}>
              <strong>{t('quick.useAsNew', { comment: typed })}</strong>
            </button>
          )}
          {options.map((o) => (
            <button key={o.comment} className="sheet__item" onClick={() => onPick(o.comment)}>
              <strong>
                <DataLabel>{o.comment}</DataLabel>
              </strong>
              <span>{o.category ? <DataLabel>{o.category}</DataLabel> : '—'}</span>
            </button>
          ))}
        </div>
      </div>
    </>
  );
}
