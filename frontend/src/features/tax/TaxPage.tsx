import { useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useSearchParams } from 'react-router-dom';
import { Camera, Download, FileText, Paperclip, Trash2 } from 'lucide-react';

import { CategoryChip, DataLabel } from '../../components/DataLabel';
import { Money, NetBreakdown } from '../../components/Money';
import {
  Banner,
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  StatusPill,
} from '../../components/ui';
import { api, apiUpload, downloadFile } from '../../lib/api';
import { YearPicker } from '../../components/YearPicker';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import type { Receipt, TaxReport } from '../../lib/types';

export function TaxPage() {
  const t = useT();
  const client = useQueryClient();
  const [params, setParams] = useSearchParams();
  const year = Number(params.get('jahr')) || new Date().getFullYear();
  const [notice, setNotice] = useState<string | null>(null);
  const [failure, setFailure] = useState<unknown>(null);

  const query = useQuery({
    queryKey: qk.derived.tax(year),
    queryFn: () => api<TaxReport>(`/tax?year=${year}`),
  });

  const download = useMutation({
    mutationFn: (path: string) => downloadFile(path),
    onSuccess: (filename) => setNotice(t('export.downloaded', { filename })),
    onError: setFailure,
  });

  const upload = useMutation({
    mutationFn: ({ bookingId, file }: { bookingId: string; file: File }) =>
      apiUpload<Receipt>(`/bookings/${bookingId}/receipt`, file),
    onSuccess: () => {
      client.invalidateQueries({ queryKey: qk.derived.tax(year) });
      client.invalidateQueries({ queryKey: qk.bookings.root });
      setNotice(t('tax.receiptAdded'));
      setFailure(null);
    },
    onError: setFailure,
  });

  const removeReceipt = useMutation({
    mutationFn: (bookingId: string) =>
      api<void>(`/bookings/${bookingId}/receipt`, { method: 'DELETE' }),
    onSuccess: () => {
      client.invalidateQueries({ queryKey: qk.derived.tax(year) });
      client.invalidateQueries({ queryKey: qk.bookings.root });
      setNotice(t('tax.receiptRemoved'));
    },
    onError: setFailure,
  });

  return (
    <>
      <PageHeader
        title={t('tax.title', { year })}
        subtitle={t('tax.intro')}
        actions={
          <>
            <Button
              variant="secondary"
              busy={download.isPending}
              onClick={() => download.mutate(`/tax/export.csv?year=${year}`)}
            >
              <Download size={15} aria-hidden="true" /> {t('tax.exportCsv')}
            </Button>
            <Button
              variant="secondary"
              busy={download.isPending}
              onClick={() => download.mutate(`/tax/export.pdf?year=${year}`)}
            >
              <FileText size={15} aria-hidden="true" /> {t('tax.exportPdf')}
            </Button>
          </>
        }
      />

      {notice && <Banner tone="info">{notice}</Banner>}
      {failure != null && <ErrorState error={failure} />}

      <div
        className="panel panel--pad"
        style={{ marginBottom: '1rem', display: 'flex', gap: '.75rem', flexWrap: 'wrap', alignItems: 'flex-end' }}
      >
        <div style={{ minWidth: '7rem' }}>
          <YearPicker id="tax-year" value={year} onChange={(next) =>
            setParams(
              (prev) => {
                const p = new URLSearchParams(prev);
                p.set('jahr', String(next));
                return p;
              },
              { replace: true },
            )} />
        </div>
        <p className="kpi__scope" style={{ margin: 0 }}>
          {t('tax.exportHint')}
        </p>
      </div>

      {query.isLoading && <LoadingState />}
      {query.isError && <ErrorState error={query.error} retry={() => query.refetch()} />}

      {query.data && query.data.entries.length === 0 && <EmptyState hint={t('tax.empty')} />}

      {query.data && query.data.entries.length > 0 && (
        <>
          <div className="grid grid--kpi" style={{ marginBottom: '1rem' }}>
            <div className="kpi">
              <span className="kpi__label">{t('tax.totalExpense')}</span>
              <span className="kpi__value">
                <Money cents={query.data.totalExpenseCents} tone="expense" />
              </span>
            </div>
            <div className="kpi">
              <span className="kpi__label">{t('tax.totalIncome')}</span>
              <span className="kpi__value">
                <Money cents={query.data.totalIncomeCents} tone="income" />
              </span>
            </div>
            <div className="kpi">
              <span className="kpi__label">{t('tax.count')}</span>
              <span className="kpi__value">{query.data.bookingCount}</span>
            </div>
            <div
              className={`kpi ${
                query.data.receiptsPresent < query.data.bookingCount ? 'kpi--warn' : ''
              }`}
            >
              <span className="kpi__label">{t('tax.receipt')}</span>
              <span className="kpi__value">
                {t('tax.receiptSummary', {
                  present: query.data.receiptsPresent,
                  total: query.data.bookingCount,
                })}
              </span>
            </div>
          </div>

          <div className="panel table-wrap screen-table">
            <table className="data-table">
              <thead>
                <tr>
                  <th className="num">#</th>
                  <th>{t('common.month')}</th>
                  <th>{t('bookings.comment')}</th>
                  <th>{t('bookings.category')}</th>
                  <th className="num">{t('bookings.income')}</th>
                  <th className="num">{t('bookings.expense')}</th>
                  <th>{t('tax.receipt')}</th>
                </tr>
              </thead>
              <tbody>
                {query.data.entries.map((entry) => (
                  <tr key={entry.bookingId}>
                    <td className="num">{entry.index}</td>
                    <td>
                      <DataLabel>{entry.monthName}</DataLabel>
                    </td>
                    <td>
                      <DataLabel>{entry.comment}</DataLabel>
                    </td>
                    <td>
                      <CategoryChip
                        name={entry.categoryName}
                        fallback={t('bookings.sourceNone')}
                      />
                    </td>
                    <td className="num">
                      {entry.incomeCents > 0 ? (
                        <Money cents={entry.incomeCents} tone="income" />
                      ) : null}
                    </td>
                    <td className="num">
                      {entry.expenseCents > 0 ? (
                        <Money cents={entry.expenseCents} tone="expense" />
                      ) : null}
                    </td>
                    <td>
                      <ReceiptCell
                        bookingId={entry.bookingId}
                        comment={entry.comment}
                        hasReceipt={entry.hasReceipt}
                        busy={
                          upload.isPending && upload.variables?.bookingId === entry.bookingId
                        }
                        onPick={(file) => upload.mutate({ bookingId: entry.bookingId, file })}
                        onOpen={() =>
                          download.mutate(`/bookings/${entry.bookingId}/receipt`)
                        }
                        onDelete={() => removeReceipt.mutate(entry.bookingId)}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {/* The receipt button is the point of this screen on a phone — it opens
              the camera — so the card keeps it reachable without sideways
              scrolling past six columns to find it. */}
          <div className="screen-cards">
            {query.data.entries.map((entry) => (
              <article key={entry.bookingId} className="mcard bcard">
                <header>
                  <strong>
                    <DataLabel>{entry.comment}</DataLabel>
                  </strong>
                  <Money
                    cents={entry.incomeCents > 0 ? entry.incomeCents : -entry.expenseCents}
                    basis="signed"
                    tone={entry.incomeCents > 0 ? 'income' : 'expense'}
                  />
                </header>
                <div className="bcard__meta">
                  <span className="num">#{entry.index}</span>
                  <DataLabel>{entry.monthName}</DataLabel>
                  <CategoryChip
                    name={entry.categoryName}
                    fallback={t('bookings.sourceNone')}
                  />
                </div>
                <div className="bcard__actions">
                  <ReceiptCell
                    bookingId={entry.bookingId}
                    comment={entry.comment}
                    hasReceipt={entry.hasReceipt}
                    busy={upload.isPending && upload.variables?.bookingId === entry.bookingId}
                    onPick={(file) => upload.mutate({ bookingId: entry.bookingId, file })}
                    onOpen={() => download.mutate(`/bookings/${entry.bookingId}/receipt`)}
                    onDelete={() => removeReceipt.mutate(entry.bookingId)}
                  />
                </div>
              </article>
            ))}
          </div>

          <section style={{ marginTop: '1.5rem' }}>
            <h2>{t('tax.byCategory')}</h2>
            <div className="panel table-wrap">
              <table className="data-table">
                <thead>
                  <tr>
                    <th>{t('bookings.category')}</th>
                    <th className="num">{t('analysis.count')}</th>
                    <th className="num">{t('bookings.income')}</th>
                    <th className="num">{t('bookings.expense')}</th>
                    <th className="num">{t('bookings.net')}</th>
                  </tr>
                </thead>
                <tbody>
                  {query.data.byCategory.map((row) => (
                    <tr key={row.categoryName}>
                      <td>
                        <DataLabel>{row.categoryName}</DataLabel>
                      </td>
                      <td className="num">{row.count}</td>
                      <td className="num">
                        <Money cents={row.incomeCents} tone="income" />
                      </td>
                      <td className="num">
                        <Money cents={row.expenseCents} tone="expense" />
                      </td>
                      <td className="num">
                        <NetBreakdown
                          incomeCents={row.incomeCents}
                          expenseCents={row.expenseCents}
                          netCents={row.netCents}
                          bookingCount={row.count}
                        />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>
        </>
      )}
    </>
  );
}

/**
 * The receipt column.
 *
 * `capture="environment"` is the whole point: on a phone this opens the rear
 * camera directly rather than a file picker, so photographing a receipt while
 * standing in the shop is one tap. On a desktop the attribute is ignored and the
 * same control is an ordinary file dialogue, which is why `accept` still lists
 * PDFs — the desktop case is a downloaded invoice, not a photograph.
 */
function ReceiptCell({
  bookingId,
  comment,
  hasReceipt,
  busy,
  onPick,
  onOpen,
  onDelete,
}: {
  bookingId: string;
  comment: string;
  hasReceipt: boolean;
  busy: boolean;
  onPick: (file: File) => void;
  onOpen: () => void;
  onDelete: () => void;
}) {
  const t = useT();
  const input = useRef<HTMLInputElement>(null);

  if (busy) return <StatusPill tone="info">{t('tax.uploading')}</StatusPill>;

  return (
    <span style={{ display: 'inline-flex', gap: '.25rem', alignItems: 'center' }}>
      <input
        ref={input}
        id={`receipt-${bookingId}`}
        type="file"
        accept="image/*,application/pdf"
        capture="environment"
        className="sr-only"
        onChange={(e) => {
          const file = e.target.files?.[0];
          if (file) onPick(file);
          // Reset, so picking the same file twice still fires a change event.
          e.target.value = '';
        }}
      />
      {hasReceipt ? (
        <>
          <button
            type="button"
            className="icon-button"
            aria-label={`${t('tax.viewReceipt')} — ${comment}`}
            onClick={onOpen}
          >
            <Paperclip size={15} aria-hidden="true" />
          </button>
          <button
            type="button"
            className="icon-button"
            aria-label={`${t('tax.deleteReceipt')} — ${comment}`}
            onClick={() => {
              if (window.confirm(t('tax.receiptDeleteConfirm', { comment }))) onDelete();
            }}
          >
            <Trash2 size={15} aria-hidden="true" />
          </button>
        </>
      ) : (
        <button
          type="button"
          className="icon-button"
          aria-label={`${t('tax.addReceipt')} — ${comment}`}
          onClick={() => input.current?.click()}
        >
          <Camera size={15} aria-hidden="true" />
        </button>
      )}
    </span>
  );
}
