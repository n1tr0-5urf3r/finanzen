import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useSearchParams } from 'react-router-dom';
import { AlertTriangle, Link2, Link2Off, RefreshCw, Send } from 'lucide-react';

import { DataLabel } from '../../components/DataLabel';
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
import { api, asList } from '../../lib/api';
import { formatDate, formatDateTime, formatPercent } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { invalidateAfterKitchenOwlChange, qk } from '../../lib/queryKeys';
import type { MessageKey } from '../../lib/messages/de';
import type {
  KoDraft,
  KoDraftPage,
  KoExpensePage,
  KoPushIntent,
  KoStatus,
  KoSyncResult,
} from '../../lib/types';

import { KoAmounts, KoCategoryChip, KoLinkState, KoSplit } from './parts';

type Tab = 'ledger' | 'review' | 'push';

/**
 * The KitchenOwl screen: the mirror as a browsable ledger, the link suggestions,
 * and the push queue.
 *
 * The page opens with the separateness stated outright rather than implied by
 * layout. Everything below it belongs to the household's ledger; nothing on this
 * page is ever added to a booking, and an expense with no link is normal rather
 * than an outstanding task. That framing is load-bearing: the alternative reading —
 * "these two lists should agree and the difference is a problem" — is exactly the
 * one the design rejected, and a badge counting unlinked expenses would reintroduce
 * it by itself.
 */
export function KitchenOwlPage() {
  const t = useT();
  const client = useQueryClient();
  const [params, setParams] = useSearchParams();
  const [notice, setNotice] = useState<string | null>(null);

  const tab = (params.get('ansicht') as Tab) ?? 'ledger';
  const linked = params.get('verknuepft');
  const search = params.get('suche') ?? '';
  const includeArchived = params.get('geloescht') === '1';

  const status = useQuery({
    queryKey: qk.kitchenowl.status(),
    queryFn: () => api<KoStatus>('/kitchenowl/status'),
  });

  const sync = useMutation({
    mutationFn: () => api<KoSyncResult>('/kitchenowl/sync', { method: 'POST' }),
    onSuccess: (result) => {
      invalidateAfterKitchenOwlChange(client);
      if (!result.started) {
        setNotice(t('ko.syncBusy'));
        return;
      }
      const run = result.expenses;
      setNotice(
        result.error ??
          t('ko.synced', {
            created: run?.createdCount ?? 0,
            updated: run?.updatedCount ?? 0,
            archived: run?.archivedCount ?? 0,
          }),
      );
    },
  });

  function setParam(key: string, value: string | null) {
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

  if (status.isLoading) return <LoadingState />;
  if (status.isError) return <ErrorState error={status.error} retry={() => status.refetch()} />;
  const s = status.data!;

  if (!s.configured) {
    return (
      <>
        <PageHeader title={t('ko.title')} subtitle={t('ko.subtitle')} />
        <EmptyState title={t('ko.notConfigured')} hint={t('ko.notConfiguredHint')} />
      </>
    );
  }

  return (
    <>
      <PageHeader
        title={t('ko.title')}
        subtitle={s.householdName ?? t('ko.subtitle')}
        actions={
          <Button onClick={() => sync.mutate()} busy={sync.isPending || s.running}>
            <RefreshCw size={15} aria-hidden="true" />
            {sync.isPending || s.running ? t('ko.syncing') : t('ko.syncNow')}
          </Button>
        }
      />

      {/* Stated, not implied. */}
      <Banner tone="info">{t('ko.separateLedger')}</Banner>

      {/* A failure is visible here and nowhere else in the app is affected. */}
      {s.reachable === false && (
        <Banner tone="warn">
          <AlertTriangle size={15} aria-hidden="true" /> {t('ko.unreachable')}{' '}
          {t('ko.unreachableHint')}
          {s.lastExpenseRun?.error && (
            <>
              {' '}
              <code className="ko-error">{s.lastExpenseRun.error}</code>
            </>
          )}
        </Banner>
      )}
      {notice && <Banner tone="info">{notice}</Banner>}
      {sync.isError && <ErrorState error={sync.error} />}

      <SyncStrip status={s} />

      {!s.enabled ? (
        <EmptyState title={t('ko.notEnabled')} hint={t('ko.notEnabledHint')} />
      ) : (
        <>
          <div className="segmented ko-tabs" role="tablist">
            {(
              [
                ['ledger', 'ko.tabLedger'],
                ['review', 'ko.tabReview'],
                ['push', 'ko.tabPush'],
              ] as [Tab, MessageKey][]
            ).map(([value, label]) => (
              <button
                key={value}
                type="button"
                role="tab"
                aria-pressed={tab === value}
                aria-selected={tab === value}
                onClick={() => setParam('ansicht', value === 'ledger' ? null : value)}
              >
                {t(label)}
                {value === 'review' && s.likelyDuplicateCount > 0 && (
                  <span className="ko-tabs__count">{s.likelyDuplicateCount}</span>
                )}
                {value === 'push' && s.failedPushCount > 0 && (
                  <span className="ko-tabs__count ko-tabs__count--warn">
                    {s.failedPushCount}
                  </span>
                )}
              </button>
            ))}
          </div>

          {tab === 'ledger' && (
            <Ledger
              linked={linked}
              search={search}
              includeArchived={includeArchived}
              onParam={setParam}
              onNotice={setNotice}
            />
          )}
          {tab === 'review' && <Review onNotice={setNotice} />}
          {tab === 'push' && <PushQueue onNotice={setNotice} />}
        </>
      )}
    </>
  );
}

/** Last synced, next sync, and the counts — the three things a sync screen owes. */
function SyncStrip({ status }: { status: KoStatus }) {
  const t = useT();
  const last = status.lastExpenseRun;
  return (
    <div className="ko-strip">
      <span>
        {t('ko.lastSynced', {
          when: last?.finishedAt ? formatDateTime(last.finishedAt) : t('ko.never'),
        })}
      </span>
      <span>
        {status.pollSeconds === 0
          ? t('ko.nextSyncManual')
          : t('ko.nextSync', {
              when: status.nextRunAt ? formatDateTime(status.nextRunAt) : t('ko.never'),
            })}
      </span>
      <span className="ko-strip__counts">
        <StatusPill tone="neutral">
          {t('ko.countMirrored')} {status.mirroredCount}
        </StatusPill>
        <StatusPill tone="neutral">
          {t('ko.countLinked')} {status.linkedCount}
        </StatusPill>
        {status.openDraftCount + status.likelyDuplicateCount > 0 && (
          <StatusPill tone="info">
            {t('ko.countOpen')} {status.openDraftCount + status.likelyDuplicateCount}
          </StatusPill>
        )}
      </span>
    </div>
  );
}

function Ledger({
  linked,
  search,
  includeArchived,
  onParam,
  onNotice,
}: {
  linked: string | null;
  search: string;
  includeArchived: boolean;
  onParam: (key: string, value: string | null) => void;
  onNotice: (message: string) => void;
}) {
  const t = useT();
  const client = useQueryClient();
  const filters = { linked, search, includeArchived };

  const query = useQuery({
    queryKey: qk.kitchenowl.expenses(filters),
    queryFn: () => {
      const qs = new URLSearchParams({ pageSize: '100' });
      if (linked) qs.set('linked', linked);
      if (search) qs.set('search', search);
      if (includeArchived) qs.set('includeArchived', 'true');
      return api<KoExpensePage>(`/kitchenowl/expenses?${qs}`);
    },
  });

  const unlink = useMutation({
    mutationFn: (id: string) =>
      api<void>(`/kitchenowl/expenses/${id}/link`, { method: 'DELETE' }),
    onSuccess: () => {
      invalidateAfterKitchenOwlChange(client);
      onNotice(t('ko.unlinked'));
    },
  });

  if (query.isLoading) return <LoadingState />;
  if (query.isError) return <ErrorState error={query.error} retry={() => query.refetch()} />;
  const page = query.data!;

  return (
    <div className="panel panel--pad">
      <div className="ko-filters">
        <input
          className="input"
          placeholder={t('ko.searchPlaceholder')}
          defaultValue={search}
          onChange={(e) => onParam('suche', e.target.value || null)}
          aria-label={t('ko.searchPlaceholder')}
        />
        <div className="segmented">
          {(
            [
              [null, 'ko.filterAll'],
              ['true', 'ko.filterLinked'],
              ['false', 'ko.filterUnlinked'],
            ] as [string | null, MessageKey][]
          ).map(([value, label]) => (
            <button
              key={label}
              type="button"
              aria-pressed={linked === value}
              onClick={() => onParam('verknuepft', value)}
            >
              {t(label)}
            </button>
          ))}
        </div>
        <label className="ko-filters__check">
          <input
            type="checkbox"
            checked={includeArchived}
            onChange={(e) => onParam('geloescht', e.target.checked ? '1' : null)}
          />
          {t('ko.showArchived')}
        </label>
      </div>

      {page.items.length === 0 ? (
        <EmptyState title={t('ko.emptyLedger')} />
      ) : (
        <div className="table-wrap">
          <table className="data-table ko-table">
            <thead>
              <tr>
                <th>{t('ko.expense')}</th>
                <th>{t('ko.date')}</th>
                <th>{t('ko.paidBy')}</th>
                {/* The two columns are named in full. "Betrag" alone would be the
                    single most expensive word on this page. */}
                <th className="num">{t('ko.amountHousehold')}</th>
                <th className="num">{t('ko.amountShare')}</th>
                <th>{t('ko.linked')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {page.items.map((expense) => (
                <tr key={expense.id} className={expense.archivedAt ? 'ko-row--archived' : ''}>
                  <td>
                    <div className="ko-cell__name">
                      <DataLabel>{expense.name}</DataLabel>
                      <KoCategoryChip name={expense.koCategoryName} />
                    </div>
                    {expense.archivedAt && (
                      <StatusPill tone="neutral">{t('ko.archived')}</StatusPill>
                    )}
                    {expense.excludeFromStatistics && (
                      <StatusPill tone="neutral">{t('ko.excluded')}</StatusPill>
                    )}
                  </td>
                  <td>{formatDate(expense.date)}</td>
                  <td>
                    <DataLabel>{expense.paidByName ?? '—'}</DataLabel>
                    <KoSplit expense={expense} />
                  </td>
                  <KoAmounts expense={expense} />
                  <td>
                    <KoLinkState expense={expense} />
                  </td>
                  <td className="num">
                    {expense.linkedBookingId && (
                      <button
                        type="button"
                        className="icon-button"
                        title={t('ko.unlink')}
                        aria-label={`${t('ko.unlink')}: ${expense.name}`}
                        onClick={() => unlink.mutate(expense.id)}
                      >
                        <Link2Off size={15} aria-hidden="true" />
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
            <tfoot>
              <tr>
                <td colSpan={3}>{t('common.total')}</td>
                {/* Two totals, never one. */}
                <td className="num">
                  <Money cents={page.sumAmountCents} basis="household" />
                </td>
                <td className="num">
                  <Money cents={page.sumOwnShareCents} basis="share" />
                </td>
                <td colSpan={2}>
                  {t('ko.countLinked')} {page.linkedCount} {t('common.of')} {page.total}
                </td>
              </tr>
            </tfoot>
          </table>
        </div>
      )}
    </div>
  );
}

function Review({ onNotice }: { onNotice: (message: string) => void }) {
  const t = useT();
  const client = useQueryClient();
  const query = useQuery({
    queryKey: qk.kitchenowl.drafts('open'),
    queryFn: () => api<KoDraftPage>('/kitchenowl/drafts?pageSize=100'),
  });

  const link = useMutation({
    mutationFn: ({ draftId, bookingId }: { draftId: string; bookingId: string }) =>
      api<KoDraft>(`/kitchenowl/drafts/${draftId}/link`, {
        method: 'POST',
        body: JSON.stringify({ bookingId }),
      }),
    onSuccess: () => {
      invalidateAfterKitchenOwlChange(client);
      onNotice(t('ko.linkDone'));
    },
  });

  const dismiss = useMutation({
    mutationFn: (draftId: string) =>
      api<KoDraft>(`/kitchenowl/drafts/${draftId}/dismiss`, { method: 'POST' }),
    onSuccess: () => {
      invalidateAfterKitchenOwlChange(client);
      onNotice(t('ko.dismissed'));
    },
  });

  if (query.isLoading) return <LoadingState />;
  if (query.isError) return <ErrorState error={query.error} retry={() => query.refetch()} />;
  const page = query.data!;

  return (
    <div className="panel panel--pad">
      <p className="ko-intro">{t('ko.reviewIntro')}</p>
      {(link.isError || dismiss.isError) && (
        <ErrorState error={link.error ?? dismiss.error} />
      )}
      {page.items.length === 0 ? (
        <EmptyState title={t('ko.reviewEmpty')} />
      ) : (
        <ul className="ko-drafts">
          {page.items.map((draft) => (
            <li
              key={draft.id}
              className={
                draft.suggestedAction === 'link' ? 'ko-draft ko-draft--likely' : 'ko-draft'
              }
            >
              <div className="ko-draft__head">
                <span className="ko-cell__name">
                  <DataLabel>{draft.expense.name}</DataLabel>
                  <KoCategoryChip name={draft.expense.koCategoryName} />
                </span>
                <span className="ko-draft__amounts">
                  <Money cents={draft.expense.amountCents} basis="household" />
                  <Money cents={draft.expense.ownShareCents} basis="share" />
                </span>
                <span className="ko-draft__date">{formatDate(draft.expense.date)}</span>
              </div>

              {draft.candidates.length === 0 ? (
                <p className="ko-draft__none">
                  {t('ko.candidateNone')} — {t('ko.notLinkedHint')}
                </p>
              ) : (
                <ul className="ko-candidates">
                  {draft.candidates.map((candidate) => (
                    <li key={candidate.bookingId}>
                      <span className="ko-candidate__what">
                        <DataLabel>{candidate.comment}</DataLabel>
                        <span className="ko-candidate__why">
                          {t(`ko.basis.${candidate.basis}` as MessageKey)} ·{' '}
                          {t('ko.matchScore', {
                            score: formatPercent(Math.min(candidate.score, 1)),
                          })}
                        </span>
                      </span>
                      <Money cents={candidate.amountCents} />
                      <span className="ko-candidate__when">
                        <DataLabel>{candidate.monthName}</DataLabel> {candidate.year}
                      </span>
                      {/* "Verknüpfen", never "Buchung anlegen". Creating a booking
                          from a pulled expense is what would double-post. */}
                      <Button
                        variant={draft.suggestedAction === 'link' ? 'primary' : 'secondary'}
                        busy={link.isPending}
                        onClick={() =>
                          link.mutate({
                            draftId: draft.id,
                            bookingId: candidate.bookingId,
                          })
                        }
                      >
                        <Link2 size={14} aria-hidden="true" /> {t('ko.link')}
                      </Button>
                    </li>
                  ))}
                </ul>
              )}

              <div className="ko-draft__actions">
                {draft.suggestedAction === 'link' && (
                  <StatusPill tone="info">{t('ko.suggestedLink')}</StatusPill>
                )}
                <Button variant="ghost" onClick={() => dismiss.mutate(draft.id)}>
                  {t('ko.dismiss')}
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function PushQueue({ onNotice }: { onNotice: (message: string) => void }) {
  const t = useT();
  const client = useQueryClient();
  const query = useQuery({
    queryKey: qk.kitchenowl.push(),
    queryFn: () => api<KoPushIntent[]>('/kitchenowl/push'),
    // A queued push is drained in the background, so the list has to move on its
    // own or it looks stuck.
    refetchInterval: (q) =>
      asList<KoPushIntent>(q.state.data).some((i) => i.state === 'queued' || i.state === 'sending')
        ? 3000
        : false,
  });

  const retry = useMutation({
    mutationFn: (bookingId: string) =>
      api<KoPushIntent>(`/kitchenowl/push/${bookingId}/retry`, { method: 'POST' }),
    onSuccess: () => {
      invalidateAfterKitchenOwlChange(client);
      onNotice(t('ko.pushRetrySafe'));
    },
  });

  const retract = useMutation({
    mutationFn: (bookingId: string) =>
      api<void>(`/kitchenowl/push/${bookingId}`, { method: 'DELETE' }),
    onSuccess: () => {
      invalidateAfterKitchenOwlChange(client);
      onNotice(t('ko.pushRetracted'));
    },
  });

  if (query.isLoading) return <LoadingState />;
  if (query.isError) return <ErrorState error={query.error} retry={() => query.refetch()} />;
  const items = query.data!;

  const tone = (state: KoPushIntent['state']) =>
    state === 'pushed'
      ? 'good'
      : state === 'failed' || state === 'abandoned'
        ? 'danger'
        : 'neutral';

  return (
    <div className="panel panel--pad">
      {items.length === 0 ? (
        <EmptyState title={t('ko.pushEmpty')} />
      ) : (
        <div className="table-wrap">
          <table className="data-table">
            <thead>
              <tr>
                <th>{t('ko.expense')}</th>
                <th>{t('ko.date')}</th>
                <th className="num">{t('ko.amountHousehold')}</th>
                <th>{t('common.total')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {items.map((intent) => (
                <tr key={intent.bookingId}>
                  <td>
                    <DataLabel>{intent.name}</DataLabel>
                    <div className="ko-push__marker">
                      <code>{intent.marker}</code>
                    </div>
                  </td>
                  <td>{formatDate(intent.date)}</td>
                  <td className="num">
                    <Money cents={intent.amountCents} basis="household" />
                  </td>
                  <td>
                    <StatusPill tone={tone(intent.state)}>
                      {t(`ko.pushState.${intent.state}` as MessageKey)}
                    </StatusPill>
                    {intent.attempts > 1 && (
                      <span className="ko-push__attempts">
                        {t('ko.pushAttempts', { count: intent.attempts })}
                      </span>
                    )}
                    {/* Never swallowed: the last error stays on the row in every
                        state, because a push that failed quietly is a booking the
                        user believes is in KitchenOwl and is not. */}
                    {intent.lastError && (
                      <div className="ko-error" role="alert">
                        {intent.lastError}
                      </div>
                    )}
                    {intent.nextAttemptAt &&
                      (intent.state === 'failed' || intent.state === 'queued') && (
                        <div className="ko-push__next">
                          {t('ko.pushNextAttempt', {
                            when: formatDateTime(intent.nextAttemptAt),
                          })}
                        </div>
                      )}
                  </td>
                  <td className="num ko-push__row-actions">
                    {(intent.state === 'failed' ||
                      intent.state === 'abandoned' ||
                      intent.state === 'retracted') && (
                      <Button
                        variant="secondary"
                        busy={retry.isPending}
                        title={t('ko.pushRetrySafe')}
                        onClick={() => retry.mutate(intent.bookingId)}
                      >
                        <Send size={14} aria-hidden="true" /> {t('ko.pushRetry')}
                      </Button>
                    )}
                    {(intent.state === 'queued' || intent.state === 'failed') && (
                      <Button
                        variant="ghost"
                        onClick={() => retract.mutate(intent.bookingId)}
                      >
                        {t('ko.pushRetract')}
                      </Button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
