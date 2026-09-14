import { useState } from 'react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { Link, useSearchParams } from 'react-router-dom';
import { ArrowLeftRight, Download, FileJson, Link2, Send } from 'lucide-react';

import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { FlowMoney, Money } from '../../components/Money';
import {
  Banner,
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  StatusPill,
} from '../../components/ui';
import { api, asList, downloadFile } from '../../lib/api';
import { formatEuro } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { YearPicker } from '../../components/YearPicker';
import { qk } from '../../lib/queryKeys';
import { BookingCard } from './BookingCard';
import { BookingEditor } from './BookingEditor';
import type { Booking, BookingPage, Category, KoStatus } from '../../lib/types';

import { PushDialog } from '../kitchenowl/PushDialog';
import { useMaskAmount } from '../../lib/privacy';
import { sortedByName } from '../../lib/categories';

export function BookingsPage() {
  const t = useT();
  const maskAmount = useMaskAmount();
  const [params, setParams] = useSearchParams();

  const year = Number(params.get('jahr')) || new Date().getFullYear();
  const search = params.get('suche') ?? '';
  const uncategorizedOnly = params.get('ohneKategorie') === '1';

  const [downloaded, setDownloaded] = useState<string | null>(null);
  // The push dialogue lives here because a push is an action on a booking, not a
  // KitchenOwl browsing task. The column only appears when the server actually has
  // KitchenOwl configured.
  const [pushing, setPushing] = useState<Booking | null>(null);
  const [pushNotice, setPushNotice] = useState<string | null>(null);
  const koStatus = useQuery({
    queryKey: qk.kitchenowl.status(),
    queryFn: () => api<KoStatus>('/kitchenowl/status'),
    retry: false,
  });
  const koReady = koStatus.data?.configured === true;
  const download = useMutation({
    mutationFn: (path: string) => downloadFile(path),
    onSuccess: setDownloaded,
  });

  const categoryId = params.get('kategorie') ?? '';
  const page = Number(params.get('seite')) || 0;
  const direction = params.get('richtung') === 'asc' ? 'asc' : 'desc';
  const [editing, setEditing] = useState<Booking | null>(null);
  // 474 bookings in one page is a long scroll and 200 of them silently truncated
  // the year at August; a hundred at a time with an explicit pager is honest.
  const PAGE_SIZE = 100;

  const filters = { year, search, uncategorized: uncategorizedOnly, categoryId, page, direction };
  const categories = useQuery({
    queryKey: qk.taxonomy.categories(),
    queryFn: () => api<Category[]>('/categories'),
    staleTime: 30 * 60_000,
  });

  const query = useQuery({
    queryKey: qk.bookings.list(filters),
    queryFn: () => {
      const qs = new URLSearchParams({
        year: String(year),
        pageSize: String(PAGE_SIZE),
        page: String(page),
        direction,
      });
      if (search) qs.set('search', search);
      if (categoryId) qs.set('categoryId', categoryId);
      if (uncategorizedOnly) qs.set('uncategorized', 'true');
      return api<BookingPage>(`/bookings?${qs}`);
    },
  });

  function update(key: string, value: string | null) {
    setParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        if (value === null || value === '') next.delete(key);
        else next.set(key, value);
        // Narrowing the filter while on page 4 of the old result lands on an
        // empty page that looks like "no bookings" rather than a paging artefact.
        if (key !== 'seite') next.delete('seite');
        return next;
      },
      { replace: true },
    );
  }

  return (
    <>
      <PageHeader
        title={t('bookings.title')}
        actions={
          <>
            {/* The JSON keeps integer cents and is what /exports/restore reads back;
                the CSV is de-DE formatted because its reader is Excel. */}
            <Button
              variant="secondary"
              busy={download.isPending}
              title={t('export.jsonHint')}
              onClick={() => download.mutate(`/exports/bookings.json?year=${year}`)}
            >
              <FileJson size={15} aria-hidden="true" /> {t('export.json')}
            </Button>
            <Button
              variant="secondary"
              busy={download.isPending}
              title={t('export.csvHint')}
              onClick={() => download.mutate(`/exports/bookings.csv?year=${year}`)}
            >
              <Download size={15} aria-hidden="true" /> {t('export.csv')}
            </Button>
          </>
        }
      />

      {downloaded && <Banner tone="info">{t('export.downloaded', { filename: downloaded })}</Banner>}
      {download.isError && <ErrorState error={download.error} />}

      <div className="panel panel--pad filter-bar">
        <div style={{ minWidth: '8rem' }}>
          <YearPicker id="f-year" value={year} onChange={(next) => update('jahr', String(next))} />
        </div>
        <div className="field" style={{ flex: 1, minWidth: '12rem' }}>
          <label htmlFor="f-search">{t('bookings.search')}</label>
          <input
            id="f-search" className="input" value={search}
            onChange={(e) => update('suche', e.target.value)}
          />
          {/* This box searches the year on screen. The same phrase across every
              year is one link away, which is where someone goes the moment the
              answer is not in this one. */}
          {search.trim().length > 0 && (
            <Link className="footnote" to={`/suche?q=${encodeURIComponent(search.trim())}`}>
              {t('search.fromBookings')}
            </Link>
          )}
        </div>
        <div className="field" style={{ minWidth: '11rem' }}>
          <label htmlFor="f-category">{t('bookings.category')}</label>
          <select
            id="f-category"
            className="select"
            value={categoryId}
            onChange={(e) => update('kategorie', e.target.value || null)}
          >
            <option value="">{t('bookings.filterAll')}</option>
            {sortedByName(asList<Category>(categories.data)).map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        </div>

        <label className="chip" style={{ cursor: 'pointer', minHeight: 'var(--tap)' }}>
          <input
            type="checkbox" checked={uncategorizedOnly}
            onChange={(e) => update('ohneKategorie', e.target.checked ? '1' : null)}
          />
          {t('bookings.filterUncategorized')}
        </label>
      </div>

      {query.isLoading && <LoadingState />}
      {query.isError && <ErrorState error={query.error} retry={() => query.refetch()} />}

      {query.data && (
        <>
          <div className="bookings-toolbar">
            <div className="segmented" role="group" aria-label={t('bookings.newest')}>
              <button
                type="button"
                aria-pressed={direction === 'desc'}
                onClick={() => update('richtung', null)}
              >
                {t('bookings.newest')}
              </button>
              <button
                type="button"
                aria-pressed={direction === 'asc'}
                onClick={() => update('richtung', 'asc')}
              >
                {t('bookings.oldest')}
              </button>
            </div>
            <span className="footnote">{t('bookings.editHint')}</span>
          </div>

          <p style={{ fontSize: '.85rem', color: 'var(--muted)', marginBottom: '.6rem' }}>
            {t('bookings.summary', {
              count: query.data.total,
              income: maskAmount(formatEuro(query.data.sumIncomeCents)),
              expense: maskAmount(formatEuro(query.data.sumExpenseCents)),
              // The per-category convention is expense-positive, which is right
              // for "what did Miete cost me". Summed over a whole filter it is
              // the balance, and showing +9.000,00 gained as "-9.000,00" reads
              // as a loss — so the sign is flipped and shown explicitly here.
              net: maskAmount(formatEuro(-query.data.sumNetCents, { showSign: true })),
            })}
            {query.data.uncategorizedCount > 0 && (
              <>
                {' · '}
                <Link to="?ohneKategorie=1" style={{ color: 'var(--uncategorized)' }}>
                  {t('bookings.summaryUncategorized', { count: query.data.uncategorizedCount })}
                </Link>
              </>
            )}
          </p>

          {query.data.items.length === 0 ? (
            <EmptyState hint={t('bookings.empty')} />
          ) : (
            <div className="panel table-wrap screen-table">
              <table className="data-table">
                <thead>
                  <tr>
                    <th>{t('common.month')}</th>
                    <th>{t('bookings.comment')}</th>
                    <th>{t('bookings.category')}</th>
                    <th className="num">{t('bookings.income')}</th>
                    <th className="num">{t('bookings.expense')}</th>
                    <th className="num">{t('bookings.net')}</th>
                    <th>{t('bookings.tax')}</th>
                    {koReady && <th />}
                  </tr>
                </thead>
                <tbody>
                  {query.data.items.map((b) => (
                    <tr
                      key={b.id}
                      className={
                        b.categorySource === 'unresolved' && b.kind !== 'transfer'
                          ? 'row--uncategorized'
                          : b.kind === 'transfer'
                            ? 'row--transfer'
                            : undefined
                      }
                    >
                      <td
                        className="booking-row__open"
                        onClick={() => setEditing(b)}
                        role="button"
                        tabIndex={0}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter' || e.key === ' ') {
                            e.preventDefault();
                            setEditing(b);
                          }
                        }}
                      >
                        {/* The month is data; imported history has no day at all. */}
                        <DataLabel>{b.monthName}</DataLabel>
                        {!b.bookedOn && (
                          <span className="kpi__scope" title={t('bookings.noDay')}> ·</span>
                        )}
                      </td>
                      <td className="booking-row__open" onClick={() => setEditing(b)}>
                        <DataLabel>{b.comment}</DataLabel>
                        {b.kind === 'transfer' && (
                          <ArrowLeftRight
                            size={13}
                            aria-label={t('bookings.kind.transfer')}
                            style={{ display: 'inline', marginLeft: '.3rem', color: 'var(--transfer)' }}
                          />
                        )}
                      </td>
                      <td>
                        <CategoryChip
                          name={b.categoryName}
                          typeLabel={b.categoryType}
                          fallback={t('bookings.sourceNone')}
                        />
                        {b.categorySource === 'manual' && (
                          <StatusPill tone="info">{t('bookings.sourceManual')}</StatusPill>
                        )}
                      </td>
                      <td className="num">
                        {b.kind === 'income' ? <Money cents={b.amountCents} tone="income" /> : null}
                      </td>
                      <td className="num">
                        {b.kind === 'expense' ? <Money cents={b.amountCents} tone="expense" /> : null}
                      </td>
                      <td className="num">
                        <FlowMoney netCents={b.netCents} />
                      </td>
                      <td>{b.taxRelevant ? <StatusPill tone="danger">×</StatusPill> : null}</td>
                      {koReady && (
                        <td className="num">
                          {b.externalSource === 'kitchenowl' ? (
                            // Already in KitchenOwl. Shown, not offered again — the
                            // server refuses a second push, and a button that only
                            // produces an error is worse than no button.
                            <span className="ko-link" title={t('ko.linked')}>
                              <Link2 size={14} aria-hidden="true" />
                              <span className="sr-only">{t('ko.linked')}</span>
                            </span>
                          ) : b.kind === 'transfer' || b.status !== 'confirmed' ? null : (
                            <button
                              type="button"
                              className="icon-button"
                              title={t('ko.pushTitle')}
                              aria-label={`${t('ko.pushTitle')}: ${b.comment}`}
                              onClick={() => setPushing(b)}
                            >
                              <Send size={15} aria-hidden="true" />
                            </button>
                          )}
                        </td>
                      )}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}

          {/* Eight columns need ~730px; a phone has 375. Scrolling two
              screen-widths sideways per row is not reading, so below the sidebar
              breakpoint the table is replaced by a card per booking. Both are in
              the DOM and CSS picks, which keeps the markup testable. */}
          {query.data.items.length > 0 && (
            <div className="screen-cards">
              {query.data.items.map((b) => (
                <BookingCard
                  key={b.id}
                  booking={b}
                  onOpen={setEditing}
                  onPush={koReady ? setPushing : undefined}
                />
              ))}
            </div>
          )}

          {/* 474 bookings do not fit one request. Without a pager the year simply
              stopped in August with nothing saying so. */}
          {query.data.total > PAGE_SIZE && (
            <nav className="pager" aria-label={t('bookings.title')}>
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
                  pages: Math.ceil(query.data.total / PAGE_SIZE),
                })}
              </span>
              <Button
                variant="secondary"
                disabled={(page + 1) * PAGE_SIZE >= query.data.total}
                onClick={() => update('seite', String(page + 1))}
              >
                →
              </Button>
            </nav>
          )}
        </>
      )}

      {editing && <BookingEditor booking={editing} onClose={() => setEditing(null)} />}

      {pushNotice && <Banner tone="info">{pushNotice}</Banner>}

      {pushing && (
        <div className="dialog-shim">
          <PushDialog
            booking={pushing}
            onClose={(message) => {
              setPushing(null);
              if (message) setPushNotice(message);
            }}
          />
        </div>
      )}
    </>
  );
}
