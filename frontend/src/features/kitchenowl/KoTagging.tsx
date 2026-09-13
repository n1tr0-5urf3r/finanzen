import { useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, Tag } from 'lucide-react';

import { DataLabel } from '../../components/DataLabel';
import { Money } from '../../components/Money';
import { Banner, Button, EmptyState, ErrorState, LoadingState } from '../../components/ui';
import { api, asList, jsonBody } from '../../lib/api';
import { formatDate } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { invalidateAfterKitchenOwlChange, qk } from '../../lib/queryKeys';
import type { KoCategory, KoMetadata, KoTagResult, KoUntaggedGroup } from '../../lib/types';

/**
 * Filing the household's uncategorised expenses.
 *
 * A third of the mirror has no KitchenOwl category, which makes "Ohne Kategorie"
 * the largest and least useful slice of every household analysis. This is the
 * screen that empties it.
 *
 * Three things it is built around:
 *
 * **One row per NAME.** 173 expenses are about 100 names, and the frequent names
 * carry most of the money. Deciding Kaufland once and applying it to sixteen
 * receipts is the whole point; a list of 173 rows would be the same work done
 * sixteen times over.
 *
 * **Every suggestion shows its evidence.** "Wocheneinkauf, because you filed this
 * name that way 39 times" is a different proposition from "Wocheneinkauf, because
 * a rule mentions it", and a preselected dropdown with no explanation is how one
 * wrong guess becomes thirty-nine wrong expenses. Nothing is preselected without a
 * reason attached.
 *
 * **It writes to KitchenOwl.** This is not a local tag: the category lands in the
 * household's own ledger where the other member sees it. The screen says so before
 * anything is pressed, and reports per-expense failures rather than a total.
 */
export function KoTagging() {
  const t = useT();
  const client = useQueryClient();
  const [choice, setChoice] = useState<Record<string, number>>({});
  const [result, setResult] = useState<KoTagResult | null>(null);

  const groups = useQuery({
    queryKey: qk.kitchenowl.untagged(null),
    queryFn: () => api<KoUntaggedGroup[]>('/kitchenowl/untagged'),
  });
  const metadata = useQuery({
    queryKey: qk.kitchenowl.metadata(),
    queryFn: () => api<KoMetadata>('/kitchenowl/metadata'),
    staleTime: 10 * 60_000,
  });

  const rows = asList<KoUntaggedGroup>(groups.data);
  const categories = asList<KoCategory>(metadata.data?.categories);

  const totals = useMemo(
    () =>
      rows.reduce(
        (acc, r) => ({
          expenses: acc.expenses + r.expenseCount,
          amount: acc.amount + r.amountCents,
          share: acc.share + r.ownShareCents,
        }),
        { expenses: 0, amount: 0, share: 0 },
      ),
    [rows],
  );

  const apply = useMutation({
    mutationFn: (vars: { name: string; koCategoryId: number }) =>
      api<KoTagResult>('/kitchenowl/untagged/apply', {
        method: 'POST',
        ...jsonBody(vars),
      }),
    onSuccess: (r) => {
      setResult(r);
      invalidateAfterKitchenOwlChange(client);
    },
  });

  /** The suggestion unless the user has said otherwise. */
  const chosen = (row: KoUntaggedGroup) =>
    choice[row.matchKey] ?? row.suggestion?.koCategoryId ?? 0;

  if (groups.isPending) return <LoadingState />;
  if (groups.isError) return <ErrorState error={groups.error} retry={() => groups.refetch()} />;

  return (
    <div className="ko-tagging">
      <Banner tone="info">{t('ko.tagWritesToKitchenOwl')}</Banner>

      {rows.length === 0 ? (
        <EmptyState title={t('ko.tagNothingLeft')} hint={t('ko.tagNothingLeftHint')} />
      ) : (
        <>
          <div className="ko-analysis__totals panel panel--pad">
            <div>
              <span className="kpi__label">{t('ko.tagNames')}</span>
              <span className="kpi__value num">{rows.length}</span>
            </div>
            <div>
              <span className="kpi__label">{t('ko.expenseCount')}</span>
              <span className="kpi__value num">{totals.expenses}</span>
            </div>
            <div>
              <span className="kpi__label">{t('ko.household')}</span>
              <span className="kpi__value">
                <Money cents={totals.amount} basis="household" />
              </span>
            </div>
            <div>
              <span className="kpi__label">{t('ko.myShare')}</span>
              <span className="kpi__value">
                <Money cents={totals.share} basis="share" />
              </span>
            </div>
          </div>

          {apply.isError && <ErrorState error={apply.error} />}

          {result && (
            <Banner tone={result.failed > 0 ? 'warn' : 'info'}>
              {t('ko.tagApplied', {
                tagged: result.tagged,
                category: result.koCategoryName,
              })}
              {result.failed > 0 && (
                <ul className="ko-tagging__failures">
                  {result.failures.map((f) => (
                    <li key={f.externalId}>
                      <DataLabel>{f.name}</DataLabel> — {f.error}
                    </li>
                  ))}
                </ul>
              )}
            </Banner>
          )}

          <div className="panel table-wrap screen-table">
            <table className="data-table">
              <thead>
                <tr>
                  <th>{t('ko.expenseName')}</th>
                  <th className="num">{t('analysis.count')}</th>
                  <th className="num">{t('ko.household')}</th>
                  <th className="num">{t('ko.myShare')}</th>
                  <th>{t('ko.tagSeen')}</th>
                  <th>{t('ko.category')}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => (
                  <tr key={row.matchKey}>
                    <th scope="row">
                      <DataLabel>{row.name}</DataLabel>
                    </th>
                    <td className="num">{row.expenseCount}</td>
                    <td className="num">
                      <Money cents={row.amountCents} basis="household" />
                    </td>
                    <td className="num">
                      <Money cents={row.ownShareCents} basis="share" />
                    </td>
                    <td className="footnote">
                      {row.firstDate === row.lastDate
                        ? formatDate(row.firstDate)
                        : `${formatDate(row.firstDate)} – ${formatDate(row.lastDate)}`}
                    </td>
                    <td>
                      <CategoryPicker
                        row={row}
                        categories={categories}
                        value={chosen(row)}
                        onChange={(id) => setChoice((p) => ({ ...p, [row.matchKey]: id }))}
                      />
                    </td>
                    <td>
                      <Button
                        variant="ghost"
                        disabled={chosen(row) === 0}
                        busy={apply.isPending && apply.variables?.name === row.name}
                        onClick={() =>
                          apply.mutate({ name: row.name, koCategoryId: chosen(row) })
                        }
                      >
                        <Check size={15} aria-hidden="true" />
                        {t('ko.tagApply', { count: row.expenseCount })}
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {/* Eleven columns do not fit a phone, and this is a queue somebody will
              work through on the sofa. */}
          <div className="screen-cards">
            {rows.map((row) => (
              <article key={row.matchKey} className="mcard">
                <header>
                  <strong>
                    <DataLabel>{row.name}</DataLabel>
                  </strong>
                  <span className="kpi__scope">
                    {row.expenseCount} {t('analysis.count')}
                  </span>
                </header>
                <dl className="mcard__grid">
                  <div>
                    <dt>{t('ko.household')}</dt>
                    <dd>
                      <Money cents={row.amountCents} basis="household" />
                    </dd>
                  </div>
                  <div>
                    <dt>{t('ko.myShare')}</dt>
                    <dd>
                      <Money cents={row.ownShareCents} basis="share" />
                    </dd>
                  </div>
                </dl>
                <div className="ko-tagging__card-actions">
                  <CategoryPicker
                    row={row}
                    categories={categories}
                    value={chosen(row)}
                    onChange={(id) => setChoice((p) => ({ ...p, [row.matchKey]: id }))}
                  />
                  <Button
                    disabled={chosen(row) === 0}
                    busy={apply.isPending && apply.variables?.name === row.name}
                    onClick={() => apply.mutate({ name: row.name, koCategoryId: chosen(row) })}
                  >
                    <Check size={15} aria-hidden="true" />
                    {t('ko.tagApply', { count: row.expenseCount })}
                  </Button>
                </div>
              </article>
            ))}
          </div>

          <p className="footnote">{t('ko.tagSuggestionNote')}</p>
        </>
      )}
    </div>
  );
}

/**
 * The category, and where the preselection came from.
 *
 * The reason sits next to the dropdown rather than in a tooltip: it is the
 * difference between a suggestion worth accepting blind and one worth a glance,
 * and it is the only thing standing between one careless click and thirty-nine
 * miscategorised expenses in somebody else's ledger.
 */
function CategoryPicker({
  row,
  categories,
  value,
  onChange,
}: {
  row: KoUntaggedGroup;
  categories: KoCategory[];
  value: number;
  onChange: (id: number) => void;
}) {
  const t = useT();
  const reason = (() => {
    if (!row.suggestion || value !== row.suggestion.koCategoryId) return null;
    switch (row.suggestion.source) {
      case 'precedent':
        return t('ko.tagWhyPrecedent', { count: row.suggestion.timesSeen });
      case 'override':
        return t('ko.tagWhyOverride');
      default:
        return t('ko.tagWhyRule');
    }
  })();

  return (
    <div className="ko-tagging__pick">
      <label className="sr-only" htmlFor={`tag-${row.matchKey}`}>
        {t('ko.category')} — {row.name}
      </label>
      <select
        id={`tag-${row.matchKey}`}
        className="select"
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
      >
        <option value={0}>{t('ko.tagChoose')}</option>
        {categories.map((c) => (
          <option key={c.categoryId} value={c.categoryId}>
            {c.name}
          </option>
        ))}
      </select>
      {reason && (
        <span className="footnote ko-tagging__why">
          <Tag size={11} aria-hidden="true" /> {reason}
        </span>
      )}
    </div>
  );
}
