import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { AlertTriangle, WalletMinimal } from 'lucide-react';

import { DataLabel } from '../../components/DataLabel';
import { FlowMoney, Money } from '../../components/Money';
import { StatusPill } from '../../components/ui';
import { api } from '../../lib/api';
import { formatDate, formatDateTime } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import type { KoSummary } from '../../lib/types';

import { KoCategoryChip } from './parts';

/**
 * The dashboard's read-only KitchenOwl tile.
 *
 * Read-only on purpose: the dashboard is the personal ledger's page, and the one
 * thing this tile must never do is look like part of its arithmetic. It is boxed,
 * labelled "getrennt von deinen Buchungen", and shows the household total and the
 * user's own share as two separate figures — never a sum, and never combined with
 * anything on the rest of the page.
 *
 * It reads the local mirror, so it renders instantly and cannot block on HTTP. When
 * KitchenOwl is unreachable the figures stay — they are the last true state — and a
 * visible staleness strip says so, rather than the tile vanishing or spinning.
 */
export function KitchenOwlWidget() {
  const t = useT();
  const query = useQuery({
    queryKey: qk.kitchenowl.summary(),
    queryFn: () => api<KoSummary>('/kitchenowl/summary'),
    // A failure here must not make the dashboard look broken.
    retry: false,
  });

  const summary = query.data;
  if (!summary?.configured) return null;

  return (
    <section className="panel panel--pad ko-widget" aria-labelledby="ko-widget-title">
      <header className="ko-widget__head">
        <h2 id="ko-widget-title">
          <WalletMinimal size={16} aria-hidden="true" /> {t('ko.widgetTitle')}
        </h2>
        <StatusPill tone="neutral">{t('ko.widgetSeparate')}</StatusPill>
      </header>

      {(summary.stale || summary.warning) && (
        <p className="ko-widget__stale" role="status">
          <AlertTriangle size={14} aria-hidden="true" />
          <span>
            {summary.warning ?? t('ko.unreachable')} {t('ko.unreachableHint')}
          </span>
        </p>
      )}

      {!summary.enabled ? (
        <p className="ko-widget__empty">
          {t('ko.notEnabled')} <Link to="/kitchenowl">{t('ko.widgetOpen')}</Link>
        </p>
      ) : (
        <>
          <div className="ko-widget__figures">
            <div>
              <span className="kpi__label">{t('ko.balance')}</span>
              <span className="kpi__value">
                <FlowMoney netCents={summary.myBalanceCents} />
              </span>
              <span className="kpi__scope">
                {summary.myBalanceCents === null || summary.myBalanceCents === 0
                  ? t('ko.balanceSettled')
                  : summary.myBalanceCents < 0
                    ? t('ko.balanceOwed')
                    : t('ko.balanceOwing')}
              </span>
            </div>
            <div>
              <span className="kpi__label">{t('ko.widgetMonth')}</span>
              {/* Two figures, never one. Adding them is meaningless and the
                  markers say which is which even read aloud. */}
              <span className="kpi__value">
                <Money cents={summary.monthAmountCents} basis="household" />
              </span>
              <span className="kpi__scope">
                <Money cents={summary.monthOwnShareCents} basis="share" />
              </span>
            </div>
          </div>

          {summary.recent.length === 0 ? (
            <p className="ko-widget__empty">{t('ko.widgetEmpty')}</p>
          ) : (
            <ul className="ko-widget__list">
              {summary.recent.slice(0, 5).map((expense) => (
                <li key={expense.id}>
                  <span className="ko-widget__name">
                    <DataLabel>{expense.name}</DataLabel>
                    <KoCategoryChip name={expense.koCategoryName} />
                  </span>
                  <span className="ko-widget__meta">{formatDate(expense.date)}</span>
                  <Money cents={expense.amountCents} basis="household" />
                </li>
              ))}
            </ul>
          )}

          <footer className="ko-widget__foot">
            <span>
              {t('ko.lastSynced', {
                when: summary.lastSyncedAt
                  ? formatDateTime(summary.lastSyncedAt)
                  : t('ko.never'),
              })}
            </span>
            <Link to="/kitchenowl">{t('ko.widgetOpen')}</Link>
          </footer>
        </>
      )}
    </section>
  );
}
