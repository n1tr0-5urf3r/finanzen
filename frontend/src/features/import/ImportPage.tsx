import { useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useSearchParams } from 'react-router-dom';
import { AlertTriangle, Upload } from 'lucide-react';

import { DataLabel } from '../../components/DataLabel';
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
import { api, apiUpload, asList } from '../../lib/api';
import { formatDateTime } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { invalidateAfterBookingChange, invalidateAfterTaxonomyChange, qk } from '../../lib/queryKeys';
import type { MessageKey } from '../../lib/messages/de';
import type {
  Category,
  CommitResult,
  ImportBatchSummary,
  ImportPreview,
} from '../../lib/types';

import { ReviewQueue } from './ReviewQueue';
import { StatementReview } from './StatementReview';
import { sortedByName } from '../../lib/categories';

type Tab = 'import' | 'pruefliste';

export function ImportPage() {
  const t = useT();
  const [params, setParams] = useSearchParams();
  const tab: Tab = params.get('ansicht') === 'pruefliste' ? 'pruefliste' : 'import';
  const batchParam = params.get('stapel');

  const batches = useQuery({
    queryKey: qk.imports.list(),
    queryFn: () => api<ImportBatchSummary[]>('/imports'),
  });
  const categories = useQuery({
    queryKey: qk.taxonomy.categories(),
    queryFn: () => api<Category[]>('/categories'),
  });

  const batchId = batchParam ?? batches.data?.[0]?.id ?? null;
  const selectedBatch = asList<ImportBatchSummary>(batches.data).find((b) => b.id === batchId);

  function setParam(key: string, value: string | null) {
    setParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        if (value === null) next.delete(key);
        else next.set(key, value);
        return next;
      },
      { replace: true },
    );
  }

  return (
    <>
      <PageHeader title={t('import.title')} subtitle={t('import.intro')} />

      <div className="segmented tabs" role="tablist">
        {(
          [
            ['import', 'import.tabImport'],
            [
              'pruefliste',
              selectedBatch?.source === 'csv_ing' ? 'import.tabStatement' : 'import.tabReview',
            ],
          ] as [Tab, MessageKey][]
        ).map(([value, labelKey]) => (
          <button
            key={value}
            type="button"
            role="tab"
            aria-pressed={tab === value}
            aria-selected={tab === value}
            onClick={() => setParam('ansicht', value === 'import' ? null : value)}
          >
            {t(labelKey)}
          </button>
        ))}
      </div>

      {tab === 'import' ? (
        <ImportTab
          batches={asList(batches.data)}
          loading={batches.isLoading}
          selected={batchId}
          onSelect={(id) => setParam('stapel', id)}
          onReview={(id) => {
            setParams(
              (prev) => {
                const next = new URLSearchParams(prev);
                next.set('stapel', id);
                next.set('ansicht', 'pruefliste');
                return next;
              },
              { replace: true },
            );
          }}
        />
      ) : batchId && categories.data ? (
        // A statement is reviewed line by line and a workbook comment by comment.
        // Which screen you get is a property of the batch, not a second tab to
        // choose between: the file already decided.
        selectedBatch?.source === 'csv_ing' ? (
          <StatementReview
            batchId={batchId}
            categories={categories.data}
            applied={selectedBatch.status === 'applied'}
          />
        ) : (
          <ReviewQueue batchId={batchId} categories={sortedByName(categories.data)} />
        )
      ) : categories.isLoading || batches.isLoading ? (
        <LoadingState />
      ) : (
        <EmptyState hint={t('import.reviewNoBatch')} />
      )}
    </>
  );
}

function ImportTab({
  batches,
  loading,
  selected,
  onSelect,
  onReview,
}: {
  batches: ImportBatchSummary[];
  loading: boolean;
  selected: string | null;
  onSelect: (id: string) => void;
  /** A statement lands on its own review: uploading it books nothing, and the
      work — a comment and a category per line — is all on that screen. Dropping
      the user on a preview with a commit button would be offering the one action
      that does nothing yet. */
  onReview: (id: string) => void;
}) {
  const t = useT();
  const client = useQueryClient();
  const input = useRef<HTMLInputElement>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [over, setOver] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const upload = useMutation({
    mutationFn: (file: File) =>
      apiUpload<ImportPreview>('/imports', file, (fraction) => setProgress(fraction)),
    onSuccess: (preview) => {
      setProgress(null);
      client.invalidateQueries({ queryKey: qk.imports.list() });
      client.setQueryData(qk.imports.one(preview.id), preview);
      if (preview.source === 'csv_ing') onReview(preview.id);
      else onSelect(preview.id);
      setNotice(null);
    },
    onError: () => setProgress(null),
  });

  const preview = useQuery({
    queryKey: qk.imports.one(selected ?? ''),
    queryFn: () => api<ImportPreview>(`/imports/${selected}`),
    enabled: !!selected,
  });

  const commit = useMutation({
    mutationFn: (id: string) => api<CommitResult>(`/imports/${id}/commit`, { method: 'POST' }),
    onSuccess: (result) => {
      invalidateAfterBookingChange(client, result.yearsTouched);
      invalidateAfterTaxonomyChange(client);
      client.invalidateQueries({ queryKey: qk.imports.root });
      setNotice(t('import.committed', { count: result.inserted }));
    },
  });

  function pick(file: File | null | undefined) {
    if (file) upload.mutate(file);
  }

  return (
    <>
      {notice && <Banner tone="info">{notice}</Banner>}

      <div
        className={`dropzone ${over ? 'dropzone--over' : ''}`}
        style={{ marginBottom: '1rem' }}
        onDragOver={(e) => {
          e.preventDefault();
          setOver(true);
        }}
        onDragLeave={() => setOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setOver(false);
          pick(e.dataTransfer.files?.[0]);
        }}
      >
        <Upload size={22} aria-hidden="true" />
        <p>{t('import.dropHint')}</p>
        <input
          ref={input}
          id="import-file"
          type="file"
          accept=".xlsx,.ods,.csv"
          className="sr-only"
          onChange={(e) => {
            pick(e.target.files?.[0]);
            e.target.value = '';
          }}
        />
        <Button
          type="button"
          busy={upload.isPending}
          onClick={() => input.current?.click()}
        >
          {t('import.choose')}
        </Button>
        {progress !== null && (
          <>
            <div className="progress">
              <span style={{ width: `${Math.round(progress * 100)}%` }} />
            </div>
            <span className="kpi__scope">
              {t('import.uploading', { percent: Math.round(progress * 100) })}
            </span>
          </>
        )}
      </div>

      {upload.isError && <ErrorState error={upload.error} />}
      {commit.isError && <ErrorState error={commit.error} />}

      {selected && preview.isLoading && <LoadingState />}
      {preview.isError && <ErrorState error={preview.error} retry={() => preview.refetch()} />}
      {preview.data && (
        <PreviewPanel
          preview={preview.data}
          busy={commit.isPending}
          onCommit={() => commit.mutate(preview.data!.id)}
        />
      )}

      <section style={{ marginTop: '1.5rem' }}>
        <h2 style={{ marginBottom: '.5rem' }}>{t('import.previousImports')}</h2>
        {loading && <LoadingState />}
        {!loading && batches.length === 0 && <EmptyState hint={t('import.noPreviousImports')} />}
        {batches.length > 0 && (
          <div className="panel table-wrap">
            <table className="data-table">
              <thead>
                <tr>
                  <th>{t('import.title')}</th>
                  <th className="num">{t('import.counts')}</th>
                  <th>{t('common.total')}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {batches.map((batch) => (
                  <tr key={batch.id}>
                    <td>
                      <DataLabel>{batch.fileName}</DataLabel>
                      <div className="rule-row__key">{formatDateTime(batch.createdAt)}</div>
                    </td>
                    <td className="num">{batch.rowCount}</td>
                    <td>
                      <StatusPill tone={batch.status === 'applied' ? 'good' : 'info'}>
                        {t(
                          batch.status === 'applied'
                            ? 'import.statusApplied'
                            : 'import.statusPreview',
                        )}
                      </StatusPill>
                    </td>
                    <td>
                      <Button
                        variant="ghost"
                        onClick={() => onSelect(batch.id)}
                        disabled={batch.id === selected}
                      >
                        {t('import.preview')}
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </>
  );
}

function PreviewPanel({
  preview,
  busy,
  onCommit,
}: {
  preview: ImportPreview;
  busy: boolean;
  onCommit: () => void;
}) {
  const t = useT();
  const applied = preview.status === 'applied';
  const gap =
    preview.markerTotalCents !== null && preview.rowTotalCents !== null
      ? preview.markerTotalCents - preview.rowTotalCents
      : null;
  const mismatched = preview.blocks.filter((b) => b.deltaCents !== null);

  return (
    <section>
      <h2 style={{ marginBottom: '.5rem' }}>
        {t('import.preview')} — <DataLabel>{preview.fileName}</DataLabel>
      </h2>

      {/* The importer reports what disagrees instead of adjusting rows to balance. */}
      {preview.warnings.map((warning) => (
        <Banner key={warning} tone="warn">
          <AlertTriangle size={15} aria-hidden="true" /> {warning}
        </Banner>
      ))}
      {applied && <Banner tone="info">{t('import.alreadyApplied')}</Banner>}

      <div className="grid grid--kpi" style={{ marginBottom: '1rem' }}>
        <Count labelKey="import.rows" value={preview.counts.dataRows} />
        <Count labelKey="import.willInsert" value={preview.counts.newBookings} />
        <Count labelKey="import.duplicates" value={preview.counts.duplicates} />
        <Count
          labelKey="import.uncategorized"
          value={preview.counts.uncategorized}
          warn={preview.counts.uncategorized > 0}
        />
        <Count labelKey="import.categorized" value={preview.counts.categorized} />
        <Count labelKey="import.transfers" value={preview.counts.transfer} />
        <Count labelKey="import.taxRelevant" value={preview.counts.taxRelevant} />
        <Count
          labelKey="import.openReview"
          value={preview.counts.openReviewItems}
          warn={preview.counts.openReviewItems > 0}
        />
      </div>

      <div className="panel table-wrap" style={{ marginBottom: '1rem' }}>
        <table className="data-table">
          <caption>{t('import.yearTotals')}</caption>
          <thead>
            <tr>
              <th>{t('common.year')}</th>
              <th className="num">{t('import.monthCount')}</th>
              <th className="num">{t('bookings.income')}</th>
              <th className="num">{t('bookings.expense')}</th>
              <th className="num">{t('months.balance')}</th>
              <th className="num">{t('import.rows')}</th>
            </tr>
          </thead>
          <tbody>
            {preview.yearTotals.map((y) => (
              <tr key={y.year}>
                <th scope="row">{y.year}</th>
                <td className="num">{y.months}</td>
                <td className="num">
                  <Money cents={y.incomeCents} tone="income" />
                </td>
                <td className="num">
                  <Money cents={y.expenseCents} tone="expense" />
                </td>
                <td className="num">
                  <FlowMoney flowCents={y.balanceCents} />
                </td>
                <td className="num">{y.bookingCount}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {preview.blocks.length > 0 && (
        <section style={{ marginBottom: '1rem' }}>
          <h3 style={{ marginBottom: '.35rem' }}>
            {t('import.blocks')}{' '}
            <StatusPill tone={mismatched.length > 0 ? 'warn' : 'good'}>
              {preview.blocks.length}
            </StatusPill>
          </h3>
          <p className="footnote" style={{ marginBottom: '.5rem' }}>
            {t('import.blocksIntro')}
          </p>

          {gap !== null && (
            <div className="grid grid--kpi" style={{ marginBottom: '.75rem' }}>
              <div className="kpi">
                <span className="kpi__label">{t('import.markerTotal')}</span>
                <span className="kpi__value">
                  <Money cents={preview.markerTotalCents} />
                </span>
              </div>
              <div className="kpi">
                <span className="kpi__label">{t('import.rowTotal')}</span>
                <span className="kpi__value">
                  <Money cents={preview.rowTotalCents} />
                </span>
              </div>
              <div className={`kpi ${gap !== 0 ? 'kpi--warn' : ''}`}>
                <span className="kpi__label">{t('import.gap')}</span>
                <span className="kpi__value">
                  <Money cents={gap} basis="signed" />
                </span>
                <span className="kpi__scope">{t('import.gapHint')}</span>
              </div>
            </div>
          )}

          <div className="panel table-wrap">
            <table className="data-table">
              <thead>
                <tr>
                  <th className="num">#</th>
                  <th>{t('common.month')}</th>
                  <th>{t('import.blockLabel')}</th>
                  <th className="num">{t('import.blockRows')}</th>
                  <th className="num">{t('import.blockMarker')}</th>
                  <th className="num">{t('import.blockComputed')}</th>
                  <th className="num">{t('import.blockDelta')}</th>
                </tr>
              </thead>
              <tbody>
                {preview.blocks.map((block) => (
                  <tr
                    key={block.index}
                    className={block.deltaCents !== null ? 'row--uncategorized' : undefined}
                  >
                    <td className="num">{block.index}</td>
                    <td>
                      {block.month}/{block.year}
                    </td>
                    <td>
                      {block.rawLabel ? (
                        <DataLabel>{block.rawLabel}</DataLabel>
                      ) : (
                        <span className="kpi__scope">{block.labelSource}</span>
                      )}
                    </td>
                    <td className="num">{block.rowCount}</td>
                    <td className="num">
                      <Money cents={block.markerCents} />
                    </td>
                    <td className="num">
                      <Money cents={block.computedCents} />
                    </td>
                    <td className="num">
                      {block.deltaCents === null ? (
                        <span className="money money--empty">–</span>
                      ) : (
                        <>
                          <Money cents={block.deltaCents} basis="signed" />{' '}
                          <StatusPill tone="warn">{t('import.blockMismatch')}</StatusPill>
                        </>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}

      <Button
        busy={busy}
        disabled={applied || preview.counts.newBookings === 0}
        onClick={onCommit}
      >
        {t('import.commit', { count: preview.counts.newBookings })}
      </Button>
    </section>
  );
}

function Count({
  labelKey,
  value,
  warn,
}: {
  labelKey: MessageKey;
  value: number;
  warn?: boolean;
}) {
  const t = useT();
  return (
    <div className={`kpi ${warn ? 'kpi--warn' : ''}`}>
      <span className="kpi__label">{t(labelKey)}</span>
      <span className="kpi__value">{value}</span>
    </div>
  );
}
