import { useState } from 'react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { Link, useSearchParams } from 'react-router-dom';
import { ArrowLeftRight, Download, FileJson, Link2, Send } from 'lucide-react';

import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { Money } from '../../components/Money';
import {
  Banner,
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  StatusPill,
} from '../../components/ui';
import { api, downloadFile } from '../../lib/api';
import { formatEuro } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import type { Booking, BookingPage, KoStatus } from '../../lib/types';

import { PushDialog } from '../kitchenowl/PushDialog';

export function BookingsPage() {
  const t = useT();
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

  const filters = { year, search, uncategorized: uncategorizedOnly };
  const query = useQuery({
    queryKey: qk.bookings.list(filters),
    queryFn: () => {
      const qs = new URLSearchParams({ year: String(year), pageSize: '200' });
      if (search) qs.set('search', search);
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

      <div className="panel panel--pad" style={{ marginBottom: '1rem', display: 'flex', gap: '.75rem', flexWrap: 'wrap', alignItems: 'flex-end' }}>
        <div className="field" style={{ minWidth: '8rem' }}>
          <label htmlFor="f-year">{t('common.year')}</label>
          <input
            id="f-year" className="input" type="number" value={year}
            onChange={(e) => update('jahr', e.target.value)}
          />
        </div>
        <div className="field" style={{ flex: 1, minWidth: '12rem' }}>
          <label htmlFor="f-search">{t('bookings.search')}</label>
          <input
            id="f-search" className="input" value={search}
            onChange={(e) => update('suche', e.target.value)}
          />
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
          <p style={{ fontSize: '.85rem', color: 'var(--muted)', marginBottom: '.6rem' }}>
            {t('bookings.summary', {
              count: query.data.total,
              income: formatEuro(query.data.sumIncomeCents),
              expense: formatEuro(query.data.sumExpenseCents),
              net: formatEuro(query.data.sumNetCents),
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
            <div className="panel table-wrap">
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
                      <td>
                        {/* The month is data; imported history has no day at all. */}
                        <DataLabel>{b.monthName}</DataLabel>
                        {!b.bookedOn && (
                          <span className="kpi__scope" title={t('bookings.noDay')}> ·</span>
                        )}
                      </td>
                      <td>
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
                        <Money cents={b.netCents} basis="net" tone="auto" />
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
        </>
      )}

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
