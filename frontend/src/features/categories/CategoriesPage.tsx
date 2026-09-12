import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useSearchParams } from 'react-router-dom';
import { Pencil, Plus, Trash2 } from 'lucide-react';

import { DataLabel, TypeChip } from '../../components/DataLabel';
import { FlowMoney } from '../../components/Money';
import {
  Banner,
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
} from '../../components/ui';
import { ApiError, api, jsonBody } from '../../lib/api';
import { useT } from '../../lib/i18n';
import { invalidateAfterTaxonomyChange, qk } from '../../lib/queryKeys';
import type { MessageKey } from '../../lib/messages/de';
import type { Category, CategoryTypeSummary } from '../../lib/types';

import { CategoryPicker } from './CategoryPicker';
import { RulesTab } from './RulesTab';

type Tab = 'kategorien' | 'regeln';

export function CategoriesPage() {
  const t = useT();
  const [params, setParams] = useSearchParams();
  const tab: Tab = params.get('ansicht') === 'regeln' ? 'regeln' : 'kategorien';
  const [notice, setNotice] = useState<string | null>(null);

  const categories = useQuery({
    queryKey: qk.taxonomy.categories(),
    queryFn: () => api<Category[]>('/categories'),
  });
  const types = useQuery({
    queryKey: qk.taxonomy.types(),
    queryFn: () => api<CategoryTypeSummary[]>('/category-types'),
    staleTime: 5 * 60_000,
  });

  function setTab(next: Tab) {
    setNotice(null);
    setParams(
      (prev) => {
        const p = new URLSearchParams(prev);
        if (next === 'kategorien') p.delete('ansicht');
        else p.set('ansicht', next);
        return p;
      },
      { replace: true },
    );
  }

  return (
    <>
      <PageHeader title={t('categories.title')} subtitle={t('categories.intro')} />

      <div className="segmented tabs" role="tablist">
        {(
          [
            ['kategorien', 'categories.tabCategories'],
            ['regeln', 'categories.tabRules'],
          ] as [Tab, MessageKey][]
        ).map(([value, labelKey]) => (
          <button
            key={value}
            type="button"
            role="tab"
            aria-pressed={tab === value}
            aria-selected={tab === value}
            onClick={() => setTab(value)}
          >
            {t(labelKey)}
          </button>
        ))}
      </div>

      {notice && <Banner tone="info">{notice}</Banner>}

      {(categories.isLoading || types.isLoading) && <LoadingState />}
      {categories.isError && (
        <ErrorState error={categories.error} retry={() => categories.refetch()} />
      )}

      {categories.data &&
        types.data &&
        (tab === 'kategorien' ? (
          <CategoriesTab
            categories={categories.data}
            types={types.data}
            onNotice={setNotice}
          />
        ) : (
          <RulesTab categories={categories.data} onNotice={setNotice} />
        ))}
    </>
  );
}

function CategoriesTab({
  categories,
  types,
  onNotice,
}: {
  categories: Category[];
  types: CategoryTypeSummary[];
  onNotice: (message: string) => void;
}) {
  const t = useT();
  const client = useQueryClient();
  const [editing, setEditing] = useState<Category | 'new' | null>(null);
  /** Set when the server refuses a delete because bookings still point at it. */
  const [blocked, setBlocked] = useState<{ category: Category; message: string } | null>(null);
  const [reassignTo, setReassignTo] = useState<string | null>(null);
  const [failure, setFailure] = useState<unknown>(null);

  const save = useMutation({
    mutationFn: ({ id, name, typeCode }: { id?: string; name: string; typeCode: string }) =>
      api<Category>(id ? `/categories/${id}` : '/categories', {
        method: id ? 'PUT' : 'POST',
        ...jsonBody({ name, typeCode }),
      }),
    onSuccess: (_, vars) => {
      invalidateAfterTaxonomyChange(client);
      setEditing(null);
      setFailure(null);
      onNotice(t(vars.id ? 'categories.saved' : 'categories.created'));
    },
    onError: setFailure,
  });

  const remove = useMutation({
    mutationFn: ({ category, target }: { category: Category; target?: string }) =>
      api<void>(
        `/categories/${category.id}${target ? `?reassignTo=${target}` : ''}`,
        { method: 'DELETE' },
      ),
    onSuccess: (_, vars) => {
      invalidateAfterTaxonomyChange(client);
      setBlocked(null);
      setReassignTo(null);
      setFailure(null);
      onNotice(t(vars.target ? 'categories.reassigned' : 'categories.deleted'));
    },
    onError: (error, vars) => {
      // 409 is not a failure to report — it is the reassign flow's entry point.
      // The server puts the booking count in the message, so it is shown verbatim.
      if (error instanceof ApiError && error.status === 409) {
        setBlocked({ category: vars.category, message: error.message });
        setReassignTo(null);
        return;
      }
      setFailure(error);
    },
  });

  return (
    <>
      <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: '.75rem' }}>
        <Button onClick={() => setEditing('new')}>
          <Plus size={15} aria-hidden="true" /> {t('categories.newCategory')}
        </Button>
      </div>

      {failure != null && <ErrorState error={failure} />}

      {editing && (
        <CategoryForm
          category={editing === 'new' ? null : editing}
          types={types}
          busy={save.isPending}
          onCancel={() => setEditing(null)}
          onSubmit={(name, typeCode) =>
            save.mutate({ id: editing === 'new' ? undefined : editing.id, name, typeCode })
          }
        />
      )}

      {blocked && (
        <div className="panel panel--pad" style={{ marginBottom: '1rem' }}>
          <Banner tone="warn">{blocked.message}</Banner>
          <p className="footnote" style={{ marginBottom: '.6rem' }}>
            {t('categories.reassignHint')}
          </p>
          <div className="field" style={{ maxWidth: '22rem' }}>
            <label htmlFor="reassign-target">{t('categories.reassignTo')}</label>
            <CategoryPicker
              id="reassign-target"
              categories={categories}
              value={reassignTo}
              onChange={setReassignTo}
              allowEmpty
              emptyLabel={t('common.none')}
              exclude={blocked.category.id}
            />
          </div>
          <div style={{ display: 'flex', gap: '.5rem', marginTop: '.75rem' }}>
            <Button
              variant="danger"
              disabled={!reassignTo}
              busy={remove.isPending}
              onClick={() =>
                reassignTo && remove.mutate({ category: blocked.category, target: reassignTo })
              }
            >
              {t('categories.reassignConfirm')}
            </Button>
            <Button variant="ghost" onClick={() => setBlocked(null)}>
              {t('common.cancel')}
            </Button>
          </div>
        </div>
      )}

      {categories.length === 0 && <EmptyState hint={t('categories.empty')} />}

      {types.map((type) => {
        const rows = categories.filter((c) => c.typeCode === type.typeCode);
        if (rows.length === 0) return null;
        return (
          <section key={type.typeCode} className="cat-group">
            <h2>
              <TypeChip label={type.label} />
            </h2>
            <div className="panel table-wrap">
              <table className="data-table">
                <thead>
                  <tr>
                    <th>{t('categories.name')}</th>
                    <th className="num">{t('categories.bookings')}</th>
                    <th className="num">{t('categories.net')}</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {rows.map((c) => (
                    <tr key={c.id}>
                      <td>
                        <DataLabel>{c.name}</DataLabel>
                      </td>
                      <td className="num">{c.bookingCount ?? 0}</td>
                      <td className="num">
                        <FlowMoney netCents={c.netCents} />
                      </td>
                      <td>
                        <div className="cat-row__actions">
                          <button
                            type="button"
                            className="icon-button"
                            aria-label={`${t('common.edit')} — ${c.name}`}
                            onClick={() => setEditing(c)}
                          >
                            <Pencil size={15} aria-hidden="true" />
                          </button>
                          <button
                            type="button"
                            className="icon-button"
                            aria-label={`${t('common.delete')} — ${c.name}`}
                            onClick={() => {
                              if (window.confirm(t('categories.deleteConfirm', { name: c.name }))) {
                                remove.mutate({ category: c });
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
          </section>
        );
      })}
    </>
  );
}

function CategoryForm({
  category,
  types,
  busy,
  onCancel,
  onSubmit,
}: {
  category: Category | null;
  types: CategoryTypeSummary[];
  busy: boolean;
  onCancel: () => void;
  onSubmit: (name: string, typeCode: string) => void;
}) {
  const t = useT();
  const [name, setName] = useState(category?.name ?? '');
  const [typeCode, setTypeCode] = useState<string>(category?.typeCode ?? types[0]?.typeCode ?? '');

  return (
    <form
      className="panel panel--pad"
      style={{ marginBottom: '1rem' }}
      onSubmit={(e) => {
        e.preventDefault();
        if (name.trim()) onSubmit(name.trim(), typeCode);
      }}
    >
      <h2 style={{ marginBottom: '.75rem' }}>
        {t(category ? 'categories.editCategory' : 'categories.newCategory')}
      </h2>
      <div className="settings-form">
        <div className="field">
          <label htmlFor="cat-name">{t('categories.name')}</label>
          {/* A category name is data the user is authoring, so the input is
              marked German like everything else that ends up in the database. */}
          <input
            id="cat-name"
            className="input"
            lang="de"
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
          />
        </div>
        <div className="field">
          <label htmlFor="cat-type">{t('categories.type')}</label>
          <select
            id="cat-type"
            className="select"
            lang="de"
            value={typeCode}
            onChange={(e) => setTypeCode(e.target.value)}
          >
            {types.map((type) => (
              <option key={type.typeCode} value={type.typeCode}>
                {type.label}
              </option>
            ))}
          </select>
        </div>
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
