import { useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Pencil, Play, Trash2 } from 'lucide-react';

import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { Banner, Button, EmptyState, ErrorState, LoadingState, StatusPill } from '../../components/ui';
import { api, jsonBody, asList } from '../../lib/api';
import { useT } from '../../lib/i18n';
import { invalidateAfterTaxonomyChange, qk } from '../../lib/queryKeys';
import type { ApplyRulesResult, BookingKind, Category, Rule } from '../../lib/types';

import { CategoryPicker } from './CategoryPicker';

/**
 * The rule table — 192 rows in the real data, which is why this is a search box
 * and not a scroll.
 *
 * Two things are stated on every write rather than left to be discovered: a rule
 * change **recategorises history**, and a category set by hand is never touched by
 * it. A silent retroactive edit to last year's tax report would be unacceptable;
 * a counted one is fine.
 */
export function RulesTab({
  categories,
  onNotice,
  creating,
  onCreatingDone,
}: {
  categories: Category[];
  onNotice: (message: string) => void;
  /** The trigger lives in `PageHeader` now; this tab still owns the form. */
  creating: boolean;
  onCreatingDone: () => void;
}) {
  const t = useT();
  const client = useQueryClient();
  const [search, setSearch] = useState('');
  const [onlyUnused, setOnlyUnused] = useState(false);
  const [editing, setEditing] = useState<Rule | 'new' | null>(null);
  const [failure, setFailure] = useState<unknown>(null);

  const rules = useQuery({
    queryKey: qk.taxonomy.rules(),
    queryFn: () => api<Rule[]>('/rules'),
  });

  const save = useMutation({
    mutationFn: ({
      id,
      comment,
      categoryId,
      kindOverride,
    }: {
      id?: string;
      comment: string;
      categoryId: string | null;
      kindOverride: BookingKind | null;
    }) =>
      api<Rule>(id ? `/rules/${id}` : '/rules', {
        method: id ? 'PUT' : 'POST',
        ...jsonBody({ comment, categoryId, kindOverride }),
      }),
    onSuccess: (rule) => {
      invalidateAfterTaxonomyChange(client);
      setEditing(null);
      onCreatingDone();
      setFailure(null);
      // `matchCount` is how many confirmed bookings carry this comment — the
      // bookings the rule now claims. Manual overrides keep their own category,
      // which is why the note below says so rather than implying otherwise.
      onNotice(
        rule.matchCount === 0
          ? t('categories.ruleRetroactiveNone')
          : `${t('categories.ruleRetroactive', {
              count: rule.matchCount,
              category: rule.categoryName ?? t('categories.ruleNoCategory'),
            })} ${t('categories.ruleManualUntouched')}`,
      );
    },
    onError: setFailure,
  });

  const remove = useMutation({
    mutationFn: (rule: Rule) => api<void>(`/rules/${rule.id}`, { method: 'DELETE' }),
    onSuccess: () => {
      invalidateAfterTaxonomyChange(client);
      setFailure(null);
      onNotice(t('categories.ruleDeleted'));
    },
    onError: setFailure,
  });

  const apply = useMutation({
    mutationFn: (dryRun: boolean) =>
      api<ApplyRulesResult>(`/rules/apply?dryRun=${dryRun}`, { method: 'POST' }),
    onSuccess: (result) => {
      if (!result.dryRun) invalidateAfterTaxonomyChange(client);
      setFailure(null);
      onNotice(
        result.dryRun
          ? t('categories.applyPreviewResult', {
              count: result.recategorized,
              examined: result.examined,
            })
          : t('categories.applyResult', {
              count: result.recategorized,
              open: result.stillUncategorized,
            }),
      );
    },
    onError: setFailure,
  });

  const filtered = useMemo(() => {
    const needle = search.trim().toLowerCase();
    return asList<Rule>(rules.data).filter((r) => {
      if (onlyUnused && r.matchCount > 0) return false;
      if (!needle) return true;
      return (
        r.normalizedComment.includes(needle) ||
        r.comment.toLowerCase().includes(needle) ||
        (r.categoryName ?? '').toLowerCase().includes(needle)
      );
    });
  }, [rules.data, search, onlyUnused]);

  if (rules.isLoading) return <LoadingState />;
  if (rules.isError) return <ErrorState error={rules.error} retry={() => rules.refetch()} />;

  const all = asList<Rule>(rules.data);
  const unused = all.filter((r) => r.matchCount === 0).length;

  return (
    <>
      <Banner tone="warn">{t('categories.retroWarning')}</Banner>

      <div className="panel panel--pad filter-bar">
        <div className="field" style={{ flex: 1, minWidth: '14rem' }}>
          <label htmlFor="rule-search">{t('categories.ruleSearch')}</label>
          <input
            id="rule-search"
            className="input"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <label className="chip" style={{ cursor: 'pointer', minHeight: 'var(--tap)' }}>
          <input
            type="checkbox"
            checked={onlyUnused}
            onChange={(e) => setOnlyUnused(e.target.checked)}
          />
          {t('categories.ruleOnlyUnused')}
        </label>
        <Button
          variant="secondary"
          busy={apply.isPending}
          onClick={() => apply.mutate(true)}
        >
          {t('categories.applyPreview')}
        </Button>
        <Button
          variant="secondary"
          busy={apply.isPending}
          onClick={() => apply.mutate(false)}
        >
          <Play size={15} aria-hidden="true" /> {t('categories.applyRules')}
        </Button>
      </div>

      <p className="footnote" style={{ marginBottom: '.6rem' }}>
        {t('categories.rulesSummary', { rules: all.length, unused })}
      </p>

      {failure != null && <ErrorState error={failure} />}

      {(editing || creating) && (
        <RuleForm
          rule={editing && editing !== 'new' ? editing : null}
          categories={categories}
          busy={save.isPending}
          onCancel={() => {
            setEditing(null);
            onCreatingDone();
          }}
          onSubmit={(comment, categoryId, kindOverride) =>
            save.mutate({
              id: editing && editing !== 'new' ? editing.id : undefined,
              comment,
              categoryId,
              kindOverride,
            })
          }
        />
      )}

      {filtered.length === 0 ? (
        <EmptyState hint={t('categories.ruleEmpty')} />
      ) : (
        <div className="panel table-wrap">
          <table className="data-table">
            <thead>
              <tr>
                <th>{t('categories.ruleComment')}</th>
                <th>{t('categories.ruleTarget')}</th>
                <th className="num">{t('categories.ruleMatches')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {filtered.map((rule) => (
                <tr key={rule.id}>
                  <td>
                    <DataLabel>{rule.comment}</DataLabel>
                    {/* Matching compares the normalised key, so it is shown where
                        it differs — that is what makes `Essen`/`essen` explicable. */}
                    {rule.normalizedComment !== rule.comment && (
                      <div className="rule-row__key">
                        {t('categories.ruleNormalized', { key: rule.normalizedComment })}
                      </div>
                    )}
                  </td>
                  <td>
                    <CategoryChip
                      name={rule.categoryName}
                      fallback={t('categories.ruleNoCategory')}
                    />
                    {rule.kindOverride && (
                      <StatusPill tone="neutral">
                        {t(`bookings.kind.${rule.kindOverride}` as const)}
                      </StatusPill>
                    )}
                  </td>
                  <td className="num">
                    {rule.matchCount === 0 ? (
                      <StatusPill tone="warn">{t('categories.ruleUnused')}</StatusPill>
                    ) : (
                      rule.matchCount
                    )}
                  </td>
                  <td>
                    <div className="cat-row__actions">
                      <button
                        type="button"
                        className="icon-button"
                        aria-label={`${t('common.edit')} — ${rule.comment}`}
                        onClick={() => setEditing(rule)}
                      >
                        <Pencil size={15} aria-hidden="true" />
                      </button>
                      <button
                        type="button"
                        className="icon-button"
                        aria-label={`${t('common.delete')} — ${rule.comment}`}
                        onClick={() => {
                          if (
                            window.confirm(
                              t('categories.ruleDeleteConfirm', { comment: rule.comment }),
                            )
                          ) {
                            remove.mutate(rule);
                          }
                        }}
                      >
                        <Trash2 size={15} aria-hidden="true" />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}

function RuleForm({
  rule,
  categories,
  busy,
  onCancel,
  onSubmit,
}: {
  rule: Rule | null;
  categories: Category[];
  busy: boolean;
  onCancel: () => void;
  onSubmit: (comment: string, categoryId: string | null, kind: BookingKind | null) => void;
}) {
  const t = useT();
  const [comment, setComment] = useState(rule?.comment ?? '');
  const [categoryId, setCategoryId] = useState<string | null>(rule?.categoryId ?? null);
  const [kind, setKind] = useState<BookingKind | ''>(rule?.kindOverride ?? '');

  return (
    <form
      className="panel panel--pad"
      style={{ marginBottom: '1rem' }}
      onSubmit={(e) => {
        e.preventDefault();
        if (comment.trim()) onSubmit(comment.trim(), categoryId, kind === '' ? null : kind);
      }}
    >
      <h2 style={{ marginBottom: '.75rem' }}>
        {t(rule ? 'categories.editRule' : 'categories.newRule')}
      </h2>
      <div className="settings-form">
        <div className="field">
          <label htmlFor="rule-comment">{t('categories.ruleComment')}</label>
          <input
            id="rule-comment"
            className="input"
            lang="de"
            value={comment}
            onChange={(e) => setComment(e.target.value)}
            required
          />
          <small>{t('categories.ruleNormalized', { key: comment.trim().toLowerCase() })}</small>
        </div>
        <div className="field">
          <label htmlFor="rule-category">{t('categories.ruleTarget')}</label>
          <CategoryPicker
            id="rule-category"
            categories={categories}
            value={categoryId}
            onChange={setCategoryId}
            allowEmpty
            emptyLabel={t('categories.ruleNoCategory')}
          />
        </div>
        <div className="field">
          <label htmlFor="rule-kind">{t('categories.ruleKind')}</label>
          <select
            id="rule-kind"
            className="select"
            value={kind}
            onChange={(e) => setKind(e.target.value as BookingKind | '')}
          >
            <option value="">{t('categories.ruleKindNone')}</option>
            <option value="transfer">{t('bookings.kind.transfer')}</option>
            <option value="income">{t('bookings.kind.income')}</option>
            <option value="expense">{t('bookings.kind.expense')}</option>
          </select>
        </div>
        <p className="footnote">{t('categories.ruleManualUntouched')}</p>
        <div style={{ display: 'flex', gap: '.5rem' }}>
          <Button type="submit" busy={busy}>
            {t('common.save')}
          </Button>
          <Button type="button" variant="ghost" onClick={onCancel}>
            {t('common.cancel')}
          </Button>
        </div>
      </div>
    </form>
  );
}
