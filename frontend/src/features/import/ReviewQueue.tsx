import { useCallback, useEffect, useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, SkipForward } from 'lucide-react';

import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { Money } from '../../components/Money';
import { Banner, Button, EmptyState, ErrorState, LoadingState, StatusPill } from '../../components/ui';
import { api, jsonBody } from '../../lib/api';
import { formatPercent } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { invalidateAfterTaxonomyChange, qk } from '../../lib/queryKeys';
import type {
  Category,
  Resolution,
  ResolveResult,
  ReviewItem,
  ReviewSuggestion,
} from '../../lib/types';

/**
 * The review queue, and the reason the import screen exists.
 *
 * ~238 distinct comments covering ~358 legacy bookings, keyed by comment rather
 * than by row and sorted by frequency, so the work that clears the most bookings
 * comes first. Roughly 35% of the rows get a suggestion at all — the rest are
 * merchants the rule table has never seen — so this is built for fast manual
 * assignment, not for admiring the matcher: one decision per screen, digits pick a
 * suggestion, Enter confirms, Esc skips, and the next item is already there.
 *
 * "Als Regel merken" is checked by default. That is the single thing that makes
 * the queue finishable: without it the same merchant is asked about again in the
 * next import.
 */
export function ReviewQueue({
  batchId,
  categories,
}: {
  batchId: string;
  categories: Category[];
}) {
  const t = useT();
  const client = useQueryClient();
  const [done, setDone] = useState<Record<string, 'resolved' | 'skipped'>>({});
  const [cursor, setCursor] = useState(0);
  const [selected, setSelected] = useState<string | null>(null);
  const [createRule, setCreateRule] = useState(true);
  const [search, setSearch] = useState('');
  const [notice, setNotice] = useState<string | null>(null);

  const query = useQuery({
    queryKey: qk.imports.review(batchId),
    // The whole queue in one request: it is ~240 small rows, and paging it would
    // put a network round trip inside a keyboard loop.
    queryFn: () => api<ReviewItem[]>(`/imports/${batchId}/review?limit=1000`),
  });

  const items = useMemo(() => query.data ?? [], [query.data]);
  const pending = useMemo(() => items.filter((i) => !done[i.id]), [items, done]);
  const current = pending[Math.min(cursor, Math.max(0, pending.length - 1))] as
    | ReviewItem
    | undefined;

  const resolve = useMutation({
    mutationFn: (resolutions: Resolution[]) =>
      api<ResolveResult>(`/imports/${batchId}/review`, {
        method: 'POST',
        ...jsonBody({ resolutions }),
      }),
    onSuccess: (result, resolutions) => {
      setDone((prev) => {
        const next = { ...prev };
        for (const r of resolutions) next[r.itemId] = r.skip ? 'skipped' : 'resolved';
        return next;
      });
      // A resolution writes a rule and recategorises history, so every year's
      // aggregates move — not just the imported one.
      invalidateAfterTaxonomyChange(client);
      client.invalidateQueries({ queryKey: qk.imports.one(batchId) });
      setNotice(
        t('import.reviewResolved', {
          resolved: result.resolved,
          rules: result.rulesCreated,
          open: result.remainingOpen,
        }),
      );
    },
  });

  // A fresh item starts with no choice made beyond the strong suggestion, and the
  // hints are deliberately never preselected.
  useEffect(() => {
    const strong = current?.suggestions.find((s) => s.isSuggestion);
    setSelected(current && !current.ambiguous ? (strong?.categoryId ?? null) : null);
    setSearch('');
  }, [current]);

  const mutate = resolve.mutate;
  const confirmCurrent = useCallback(() => {
    if (!current || !selected) return;
    mutate([{ itemId: current.id, categoryId: selected, createRule }]);
  }, [current, selected, createRule, mutate]);
  const skipCurrent = useCallback(() => {
    if (!current) return;
    mutate([{ itemId: current.id, skip: true }]);
  }, [current, mutate]);

  /**
   * Keyboard first, at the window rather than on a focused element: the point of
   * the screen is 238 decisions without the hand leaving the keyboard, and that
   * cannot depend on the right thing still having focus after a re-render.
   */
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      if (event.key === 'Enter') {
        event.preventDefault();
        confirmCurrent();
        return;
      }
      if (event.key === 'Escape') {
        event.preventDefault();
        skipCurrent();
        return;
      }
      const target = event.target as HTMLElement | null;
      const typing =
        target instanceof HTMLInputElement ||
        target instanceof HTMLSelectElement ||
        target instanceof HTMLTextAreaElement;
      if (typing) return;
      if (/^[1-9]$/.test(event.key)) {
        const all = [...(current?.suggestions ?? []), ...(current?.weakHints ?? [])];
        const pick = all[Number(event.key) - 1];
        if (pick) {
          event.preventDefault();
          setSelected(pick.categoryId);
        }
      }
    }
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [current, confirmCurrent, skipCurrent]);

  /** Containment matches only — never the edit-distance hints. */
  const bulk = useMemo(
    () =>
      pending
        .filter((i) => !i.ambiguous)
        .map((i) => ({ item: i, suggestion: i.suggestions.find((s) => s.isSuggestion) }))
        .filter((x): x is { item: ReviewItem; suggestion: ReviewSuggestion } => !!x.suggestion),
    [pending],
  );

  if (query.isLoading) return <LoadingState />;
  if (query.isError) return <ErrorState error={query.error} retry={() => query.refetch()} />;

  const resolvedCount = Object.keys(done).length;

  return (
    <>
      <p className="footnote" style={{ marginBottom: '.5rem' }}>
        {t('import.reviewIntro')}
      </p>
      {notice && <Banner tone="info">{notice}</Banner>}
      {resolve.isError && <ErrorState error={resolve.error} />}

      <div
        className="panel panel--pad"
        style={{ marginBottom: '1rem', display: 'flex', gap: '.75rem', flexWrap: 'wrap', alignItems: 'center' }}
      >
        <strong>
          {t('import.reviewProgress', { done: resolvedCount, total: items.length })}
        </strong>
        <StatusPill tone={pending.length > 0 ? 'warn' : 'good'}>
          {t('import.reviewOpenCount', { count: pending.length })}
        </StatusPill>
        <div style={{ marginLeft: 'auto', display: 'flex', gap: '.5rem', alignItems: 'center' }}>
          <span className="kpi__scope">{t('import.reviewBulkHint')}</span>
          <Button
            variant="secondary"
            disabled={bulk.length === 0}
            busy={resolve.isPending}
            onClick={() =>
              resolve.mutate(
                bulk.map(({ item, suggestion }) => ({
                  itemId: item.id,
                  categoryId: suggestion.categoryId,
                  createRule: true,
                })),
              )
            }
          >
            {t('import.reviewBulk', { count: bulk.length })}
          </Button>
        </div>
      </div>

      {pending.length === 0 || !current ? (
        <EmptyState hint={t('import.reviewDone')} />
      ) : (
        <div className="review">
          <div className="panel panel--pad review-card">
            <div>
              <span className="kpi__scope">
                {t('import.reviewOf', {
                  index: resolvedCount + 1,
                  total: items.length,
                })}
              </span>
              <div className="review-card__comment">
                <DataLabel>{current.comment}</DataLabel>
              </div>
              <div className="review-card__meta">
                <span>{t('import.reviewAffects', { count: current.rowCount })}</span>
                {current.expenseCents > 0 && (
                  <span>
                    {t('bookings.expense')}: <Money cents={current.expenseCents} tone="expense" />
                  </span>
                )}
                {current.incomeCents > 0 && (
                  <span>
                    {t('bookings.income')}: <Money cents={current.incomeCents} tone="income" />
                  </span>
                )}
                {/* The resolve endpoint takes a category and nothing else, so a
                    transfer hint is advice, not an action. Saying so beats a
                    control that silently does not do what it looks like. */}
                {current.suggestedKind === 'transfer' && (
                  <StatusPill tone="neutral" >
                    <span title={t('import.reviewKindHint')}>
                      {t('bookings.kind.transfer')}
                    </span>
                  </StatusPill>
                )}
              </div>
            </div>

            {current.ambiguous && (
              <Banner tone="warn">{t('import.reviewAmbiguous')}</Banner>
            )}

            <div>
              <h3 style={{ marginBottom: '.35rem' }}>{t('import.reviewSuggestions')}</h3>
              {/* A weak hint is explicitly NOT a suggestion, so an item carrying
                  only hints still says "no suggestion" — otherwise `Malve → Sport`
                  reads as the matcher having an answer. */}
              {current.suggestions.length === 0 && (
                <p className="footnote">{t('import.reviewNoSuggestion')}</p>
              )}
              {current.suggestions.length === 0 && current.weakHints.length === 0 ? null : (
                <div className="suggestions">
                  {current.suggestions.map((s, i) => (
                    <SuggestionButton
                      key={`${s.categoryId}-${i}`}
                      suggestion={s}
                      index={i + 1}
                      selected={selected === s.categoryId}
                      onPick={() => setSelected(s.categoryId)}
                    />
                  ))}
                  {current.weakHints.length > 0 && (
                    <span className="kpi__scope" style={{ alignSelf: 'center' }}>
                      {t('import.reviewSimilar')}:
                    </span>
                  )}
                  {current.weakHints.map((s, i) => (
                    <SuggestionButton
                      key={`hint-${s.categoryId}-${i}`}
                      suggestion={s}
                      index={current.suggestions.length + i + 1}
                      selected={selected === s.categoryId}
                      hint
                      onPick={() => setSelected(s.categoryId)}
                    />
                  ))}
                </div>
              )}
            </div>

            <CategoryFilterPicker
              categories={categories}
              search={search}
              onSearch={setSearch}
              selected={selected}
              onSelect={setSelected}
            />

            <label className="chip" style={{ cursor: 'pointer', minHeight: 'var(--tap)' }}>
              <input
                type="checkbox"
                checked={createRule}
                onChange={(e) => setCreateRule(e.target.checked)}
              />
              {t('import.reviewCreateRule')}
            </label>
            <p className="footnote" style={{ marginTop: 0 }}>
              {t('import.reviewCreateRuleHint')}
            </p>

            <div style={{ display: 'flex', gap: '.5rem', flexWrap: 'wrap' }}>
              <Button
                disabled={!selected}
                busy={resolve.isPending}
                onClick={confirmCurrent}
              >
                <Check size={15} aria-hidden="true" /> {t('import.reviewConfirm')}
              </Button>
              <Button variant="ghost" onClick={skipCurrent}>
                <SkipForward size={15} aria-hidden="true" /> {t('import.reviewSkip')}
              </Button>
              <span className="kpi__scope" style={{ alignSelf: 'center' }}>
                {t('import.reviewKeys')}
              </span>
            </div>
          </div>

          <div className="panel panel--pad">
            <h3 style={{ marginBottom: '.4rem' }}>{t('import.reviewQueue')}</h3>
            {/* Sorted by frequency, exactly as the server returns it: the item that
                clears eleven bookings must never sit below one that clears one. */}
            <div className="queue">
              {pending.slice(0, 60).map((item, i) => (
                <button
                  key={item.id}
                  type="button"
                  className="queue__item"
                  aria-current={item.id === current.id}
                  onClick={() => setCursor(i)}
                >
                  <DataLabel>{item.comment}</DataLabel>
                  <span className="queue__count">{item.rowCount}</span>
                </button>
              ))}
            </div>
          </div>
        </div>
      )}
    </>
  );
}

function SuggestionButton({
  suggestion,
  index,
  selected,
  hint,
  onPick,
}: {
  suggestion: ReviewSuggestion;
  index: number;
  selected: boolean;
  hint?: boolean;
  onPick: () => void;
}) {
  const t = useT();
  return (
    <button
      type="button"
      className={`suggestion ${hint ? 'suggestion--hint' : ''}`}
      aria-pressed={selected}
      onClick={onPick}
      title={`${t('import.reviewMatched', { rule: suggestion.matchedRule })} · ${t(
        'import.reviewConfidence',
        { percent: formatPercent(suggestion.confidence) },
      )}`}
    >
      {index <= 9 && <span className="suggestion__key">{index}</span>}
      <DataLabel>{suggestion.categoryName}</DataLabel>
    </button>
  );
}

/** Type-ahead over all categories, for the ~65% of items with no suggestion. */
function CategoryFilterPicker({
  categories,
  search,
  onSearch,
  selected,
  onSelect,
}: {
  categories: Category[];
  search: string;
  onSearch: (value: string) => void;
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  const t = useT();
  const needle = search.trim().toLowerCase();
  const matches = categories.filter(
    (c) =>
      !needle ||
      c.name.toLowerCase().includes(needle) ||
      c.typeLabel.toLowerCase().includes(needle),
  );
  const chosen = categories.find((c) => c.id === selected);

  return (
    <div className="picker">
      <div className="field">
        <label htmlFor="review-category">{t('import.reviewPick')}</label>
        <input
          id="review-category"
          className="input"
          lang="de"
          placeholder={t('import.reviewSearch')}
          value={search}
          onChange={(e) => onSearch(e.target.value)}
          onKeyDown={(e) => {
            // Enter in the filter takes the single remaining match, which is what
            // "type three letters and carry on" needs to mean.
            if (e.key === 'Enter' && matches.length > 0 && needle) {
              e.preventDefault();
              e.stopPropagation();
              onSelect(matches[0].id);
              onSearch('');
            }
          }}
        />
      </div>
      {chosen && (
        <p className="footnote" style={{ marginTop: '.35rem' }}>
          {t('import.reviewSelected')}{' '}
          <CategoryChip name={chosen.name} typeLabel={chosen.typeLabel} fallback="—" />
        </p>
      )}
      {needle && (
        <div className="picker__list">
          {matches.slice(0, 40).map((c) => (
            <button
              key={c.id}
              type="button"
              className="picker__option"
              aria-selected={c.id === selected}
              onClick={() => {
                onSelect(c.id);
                onSearch('');
              }}
            >
              <DataLabel>{c.name}</DataLabel>
              <span className="kpi__scope">
                <DataLabel>{c.typeLabel}</DataLabel>
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
