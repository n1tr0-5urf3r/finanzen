import { Link } from 'react-router-dom';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowRight, Check } from 'lucide-react';

import { Money } from '../../components/Money';
import { Button, EmptyState, ErrorState, LoadingState } from '../../components/ui';
import { api, asList, jsonBody } from '../../lib/api';
import { sortedByName } from '../../lib/categories';
import { useT } from '../../lib/i18n';
import { invalidateAfterKitchenOwlChange, qk } from '../../lib/queryKeys';
import type { Category, KoCategoryMap, KoCategoryMapping } from '../../lib/types';

import { KoCategoryChip } from './parts';

/**
 * Which finance category each KitchenOwl category's shares count in.
 *
 * What a flatmate paid for the household counts in my statistics at my share, and
 * it needs a category there. KitchenOwl's few household labels are a different
 * taxonomy from the app's, so the user maps them here — one list, KitchenOwl's
 * category on the left, mine on the right, and nothing else to operate.
 *
 * Choosing in the select IS the decision and is saved at once; there is no
 * second "save" step to forget. Nothing is mapped by itself: where a finance
 * category has exactly the same name it is offered as a one-click suggestion
 * beside the select, never preselected into it — a select showing a category
 * that is not actually in effect would say the opposite of what the figures do.
 * Unmapped, the shares count without a category, and the analysis says so.
 */
export function KoCategoryMapTab() {
  const t = useT();
  const client = useQueryClient();

  const map = useQuery({
    queryKey: qk.kitchenowl.categoryMap(),
    queryFn: () => api<KoCategoryMap>('/kitchenowl/category-map'),
  });
  const categories = useQuery({
    queryKey: qk.taxonomy.categories(),
    queryFn: () => api<Category[]>('/categories'),
    staleTime: 30 * 60_000,
  });

  const save = useMutation({
    mutationFn: (vars: { koCategoryId: number; financesCategoryId: string | null }) =>
      api<KoCategoryMapping>(`/kitchenowl/category-map/${vars.koCategoryId}`, {
        method: 'PUT',
        ...jsonBody({ financesCategoryId: vars.financesCategoryId }),
      }),
    onSuccess: (saved) => {
      // The row shows the new mapping at once, before the refetch lands.
      client.setQueryData<KoCategoryMap>(qk.kitchenowl.categoryMap(), (prev) =>
        prev
          ? {
              ...prev,
              categories: prev.categories.map((c) =>
                c.koCategoryId === saved.koCategoryId ? saved : c,
              ),
            }
          : prev,
      );
      // A mapping moves shares between categories in every statistic.
      invalidateAfterKitchenOwlChange(client);
    },
  });

  if (map.isPending) return <LoadingState />;
  if (map.isError) return <ErrorState error={map.error} retry={() => map.refetch()} />;

  const rows = asList<KoCategoryMapping>(map.data.categories);
  const all = sortedByName(asList<Category>(categories.data));
  const nameOf = (id: string | null) => all.find((c) => c.id === id)?.name;
  // Archived categories are not offered — except the one a row is already mapped
  // to, which must stay selectable or the select would silently show another.
  const options = (row: KoCategoryMapping) =>
    all.filter((c) => !c.archived || c.id === row.financesCategoryId);

  const choose = (row: KoCategoryMapping, financesCategoryId: string | null) =>
    save.mutate({ koCategoryId: row.koCategoryId, financesCategoryId });
  const saving = (row: KoCategoryMapping) =>
    save.isPending && save.variables?.koCategoryId === row.koCategoryId;
  const justSaved = (row: KoCategoryMapping) =>
    save.isSuccess && save.variables?.koCategoryId === row.koCategoryId;

  const source = (row: KoCategoryMapping) => (
    <div className="ko-map__source">
      <KoCategoryChip name={row.name} />
      <span className="ko-map__meta">
        {t('ko.mapShareCount', { count: row.shareCount })} ·{' '}
        <Money cents={row.ownShareCents} basis="share" />
      </span>
    </div>
  );

  const target = (row: KoCategoryMapping) => {
    const suggestion =
      !row.financesCategoryId && row.suggestedCategoryId
        ? nameOf(row.suggestedCategoryId)
        : undefined;
    return (
      <div className="ko-map__target">
        <label className="sr-only" htmlFor={`map-${row.koCategoryId}`}>
          {t('ko.mapFinanceCategory')} — {row.name}
        </label>
        <select
          id={`map-${row.koCategoryId}`}
          className="select"
          value={row.financesCategoryId ?? ''}
          disabled={saving(row)}
          onChange={(e) => choose(row, e.target.value || null)}
        >
          <option value="">{t('ko.mapNone')}</option>
          {options(row).map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
        {suggestion && (
          <Button
            variant="ghost"
            busy={saving(row)}
            onClick={() => choose(row, row.suggestedCategoryId)}
          >
            {t('ko.mapSuggestion', { category: suggestion })}
          </Button>
        )}
        {justSaved(row) && (
          <span className="ko-map__saved" role="status">
            <Check size={13} aria-hidden="true" /> {t('ko.mapSaved')}
          </span>
        )}
      </div>
    );
  };

  return (
    <div className="ko-map">
      <p className="ko-intro">{t('ko.mapIntro')}</p>
      {save.isError && <ErrorState error={save.error} />}

      {rows.length === 0 ? (
        <EmptyState title={t('ko.mapEmpty')} hint={t('ko.mapEmptyHint')} />
      ) : (
        <ul className="panel ko-map__list">
          <li className="ko-map__head" aria-hidden="true">
            <span>{t('ko.category')}</span>
            <span />
            <span>{t('ko.mapFinanceCategory')}</span>
          </li>
          {rows.map((row) => (
            <li key={row.koCategoryId} className="ko-map__row" data-ko-category={row.name}>
              {source(row)}
              <ArrowRight size={15} aria-hidden="true" className="ko-map__arrow" />
              {target(row)}
            </li>
          ))}
        </ul>
      )}

      {map.data.untaggedShareCount > 0 && (
        <p className="footnote">
          {t('ko.mapUntagged', { count: map.data.untaggedShareCount })}{' '}
          <Link to="/kitchenowl?ansicht=tag">{t('ko.mapUntaggedLink')}</Link>
        </p>
      )}
    </div>
  );
}
