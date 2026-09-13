import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link, useSearchParams } from 'react-router-dom';
import { Search } from 'lucide-react';

import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { FlowMoney, Money } from '../../components/Money';
import {
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
} from '../../components/ui';
import { api, asList } from '../../lib/api';
import { formatEuro } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { useMaskAmount } from '../../lib/privacy';
import { qk } from '../../lib/queryKeys';
import { BookingCard } from '../bookings/BookingCard';
import { BookingEditor } from '../bookings/BookingEditor';
import type {
  Booking,
  Category,
  SearchComment,
  SearchResult,
  SearchYearSummary,
} from '../../lib/types';

const PAGE_SIZE = 100;

/**
 * One phrase, every year at once.
 *
 * The rest of the app is deliberately year-scoped — a ledger is read a year at a
 * time — which turns "what have I ever paid this merchant" into four page loads
 * and a mental addition. The per-year summary at the top is the answer; the rows
 * underneath are the evidence for it, and are editable in place because a search
 * is usually how a miscategorised booking gets found.
 */
export function SearchPage() {
  const t = useT();
  const maskAmount = useMaskAmount();
  const [params, setParams] = useSearchParams();

  const query = params.get('q') ?? '';
  const categoryId = params.get('kategorie') ?? '';
  const page = Number(params.get('seite')) || 0;

  // The field is local so typing does not re-query on every keystroke; the URL is
  // only written on submit, which also makes a search a link worth sending.
  const [draft, setDraft] = useState(query);
  useEffect(() => setDraft(query), [query]);

  const [editing, setEditing] = useState<Booking | null>(null);

  const categories = useQuery({
    queryKey: qk.taxonomy.categories(),
    queryFn: () => api<Category[]>('/categories'),
    staleTime: 30 * 60_000,
  });

  const results = useQuery({
    queryKey: qk.bookings.search(query, categoryId, page),
    queryFn: () => {
      const qs = new URLSearchParams({ q: query, pageSize: String(PAGE_SIZE) });
      if (categoryId) qs.set('categoryId', categoryId);
      if (page > 0) qs.set('page', String(page));
      return api<SearchResult>(`/bookings/search?${qs.toString()}`);
    },
    enabled: query.trim().length > 0,
  });

  function update(key: string, value: string | null) {
    setParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        if (value) next.set(key, value);
        else next.delete(key);
        // Any new search starts at the first page; keeping the old one silently
        // shows page four of a different result.
        if (key !== 'seite') next.delete('seite');
        return next;
      },
      { replace: true },
    );
  }

  const years = asList<SearchYearSummary>(results.data?.byYear);
  const comments = asList<SearchComment>(results.data?.comments);
  const items = asList<Booking>(results.data?.items);
  // Bars are drawn against the largest year, so the shape is comparable within one
  // search and never pretends to a scale it does not have.
  const widest = Math.max(1, ...years.map((y) => Math.abs(y.netCents)));

  return (
    <>
      <PageHeader title={t('search.title')} subtitle={t('search.intro')} />

      <form
        className="panel panel--pad search-bar"
        onSubmit={(e) => {
          e.preventDefault();
          update('q', draft.trim() || null);
        }}
      >
        <div className="field" style={{ flex: 1, minWidth: '12rem' }}>
          <label htmlFor="search-q">{t('search.field')}</label>
          <input
            id="search-q"
            className="input"
            value={draft}
            autoComplete="off"
            placeholder={t('search.placeholder')}
            onChange={(e) => setDraft(e.target.value)}
          />
        </div>
        <div className="field" style={{ minWidth: '11rem' }}>
          <label htmlFor="search-category">{t('bookings.category')}</label>
          <select
            id="search-category"
            className="select"
            value={categoryId}
            onChange={(e) => update('kategorie', e.target.value || null)}
          >
            <option value="">{t('bookings.filterAll')}</option>
            {asList<Category>(categories.data).map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        </div>
        <Button type="submit">
          <Search size={15} aria-hidden="true" />
          {t('search.submit')}
        </Button>
      </form>

      {!query.trim() && <EmptyState hint={t('search.empty')} />}

      {query.trim().length > 0 && (
        <>
          {results.isPending && <LoadingState />}
          {results.isError && (
            <ErrorState error={results.error} retry={() => results.refetch()} />
          )}

          {results.data && results.data.total === 0 && (
            <EmptyState hint={t('search.noHits', { query: results.data.query })} />
          )}

          {results.data && results.data.total > 0 && (
            <>
              <p className="footnote" style={{ marginBottom: '.75rem' }}>
                {t('search.summary', {
                  count: results.data.total,
                  years: years.length,
                  income: maskAmount(formatEuro(results.data.sumIncomeCents)),
                  expense: maskAmount(formatEuro(results.data.sumExpenseCents)),
                  // The stored sign is expense-positive; a balance reads the other
                  // way round, so it is flipped here exactly as on the bookings
                  // screen.
                  net: maskAmount(
                    formatEuro(-results.data.sumNetCents, { showSign: true }),
                  ),
                })}
              </p>

              {/* The reason this screen exists, so it comes first. */}
              <div className="panel panel--pad" style={{ marginBottom: '1rem' }}>
                <h2 style={{ margin: '0 0 .6rem' }}>{t('search.byYear')}</h2>
                <table className="data-table search-years">
                  <thead>
                    <tr>
                      <th>{t('common.year')}</th>
                      <th className="num">{t('analysis.count')}</th>
                      <th className="num">{t('bookings.income')}</th>
                      <th className="num">{t('bookings.expense')}</th>
                      <th className="num">{t('bookings.net')}</th>
                      <th className="search-years__bar" />
                    </tr>
                  </thead>
                  <tbody>
                    {years.map((y) => (
                      <tr key={y.year}>
                        <th scope="row">
                          <Link to={`/buchungen?jahr=${y.year}&suche=${encodeURIComponent(results.data.query)}`}>
                            {y.year}
                          </Link>
                        </th>
                        <td className="num">{y.bookingCount}</td>
                        <td className="num">
                          <Money cents={y.incomeCents} tone="income" />
                        </td>
                        <td className="num">
                          <Money cents={y.expenseCents} tone="expense" />
                        </td>
                        <td className="num">
                          <FlowMoney netCents={y.netCents} />
                        </td>
                        <td className="search-years__bar">
                          {/* Proportion only: no axis, no ticks, nothing that
                              claims to be a chart with a scale. */}
                          <span
                            className={`search-years__fill ${
                              y.netCents < 0 ? 'search-years__fill--in' : ''
                            }`}
                            style={{ width: `${(Math.abs(y.netCents) / widest) * 100}%` }}
                            aria-hidden="true"
                          />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              {comments.length > 1 && (
                <div className="panel panel--pad" style={{ marginBottom: '1rem' }}>
                  <h2 style={{ margin: '0 0 .5rem' }}>{t('search.spellings')}</h2>
                  {/* Hand-typed comments drift. Saying which spellings were folded
                      together is the difference between a total you trust and one
                      you re-check by hand. */}
                  <div className="search-spellings">
                    {comments.map((c) => (
                      <span key={c.comment} className="chip">
                        <DataLabel>{c.comment}</DataLabel>
                        <span className="kpi__scope"> ×{c.bookingCount}</span>
                        {c.categoryName && (
                          <CategoryChip
                            name={c.categoryName}
                            typeLabel={null}
                            fallback={t('bookings.sourceNone')}
                          />
                        )}
                      </span>
                    ))}
                  </div>
                </div>
              )}

              <h2 style={{ margin: '0 0 .5rem' }}>{t('search.allBookings')}</h2>
              {/* Cards at every width, not only on a phone: a search result is a
                  handful of rows from different years, and the year belongs on the
                  row rather than in a column header that is no longer true. */}
              <div className="search-results">
                {items.map((b) => (
                  <BookingCard key={b.id} booking={b} onOpen={setEditing} showYear />
                ))}
              </div>

              {results.data.total > PAGE_SIZE && (
                <nav className="pager" aria-label={t('search.title')}>
                  <Button
                    variant="secondary"
                    disabled={page === 0}
                    onClick={() => update('seite', page > 1 ? String(page - 1) : null)}
                  >
                    ←
                  </Button>
                  <span>
                    {t('bookings.page', {
                      page: page + 1,
                      pages: Math.ceil(results.data.total / PAGE_SIZE),
                    })}
                  </span>
                  <Button
                    variant="secondary"
                    disabled={(page + 1) * PAGE_SIZE >= results.data.total}
                    onClick={() => update('seite', String(page + 1))}
                  >
                    →
                  </Button>
                </nav>
              )}
            </>
          )}
        </>
      )}

      {editing && <BookingEditor booking={editing} onClose={() => setEditing(null)} />}
    </>
  );
}
