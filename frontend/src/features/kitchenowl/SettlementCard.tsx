import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowLeftRight, Check } from 'lucide-react';

import { DataLabel } from '../../components/DataLabel';
import { Money } from '../../components/Money';
import { Button, ErrorState } from '../../components/ui';
import { api, jsonBody } from '../../lib/api';
import { formatDateTime } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { invalidateAfterSettlement, qk } from '../../lib/queryKeys';
import type { KoSettlement } from '../../lib/types';

/**
 * Settling up — the one place the two ledgers legitimately touch.
 *
 * The direction word comes from the server, beside the figure it describes, so the
 * sentence and the amount cannot disagree; KitchenOwl's balance is signed from the
 * user's side and negative means they owe, and that has been rendered backwards
 * once already.
 *
 * The reasoning is on the card rather than in a commit message, because the one
 * thing a user will otherwise report as a bug is that booking a settlement leaves
 * the year's balance untouched. It does, and it should: the household's purchases
 * are already in the personal ledger at full value, so counting the settlement as
 * an expense would count them twice.
 */
export function SettlementCard({ onNotice }: { onNotice: (text: string) => void }) {
  const t = useT();
  const client = useQueryClient();

  const settlement = useQuery({
    queryKey: qk.kitchenowl.settlement(),
    queryFn: () => api<KoSettlement>('/kitchenowl/settlement'),
  });

  const book = useMutation({
    mutationFn: () =>
      api<KoSettlement>('/kitchenowl/settlement', { method: 'POST', ...jsonBody({}) }),
    onSuccess: (result) => {
      invalidateAfterSettlement(client, result.period.year);
      onNotice(t('ko.settleDone', { comment: result.suggestedComment }));
    },
  });

  const s = settlement.data;
  if (!s) return null;

  // "Nothing to settle" is a state worth showing rather than hiding: it is the
  // answer to the question the card exists for.
  const nothingToSettle = s.direction === 'settled' || s.direction === 'unknown';

  return (
    <section className="panel panel--pad ko-settle">
      <header className="ko-settle__head">
        <h2>{t('ko.settleTitle')}</h2>
        <p className="footnote">{t('ko.settleWhy')}</p>
      </header>

      {book.isError && <ErrorState error={book.error} />}

      <div className="ko-settle__body">
        <div className="ko-settle__figure">
          <span className="kpi__label">{t('ko.balance')}</span>
          <span className="kpi__value">
            {/* The sign stands exactly as KitchenOwl reports it. */}
            <Money
              cents={s.balanceCents}
              basis="signed"
              tone={(s.balanceCents ?? 0) < 0 ? 'expense' : 'income'}
            />
          </span>
          <span className="kpi__scope">
            {t(
              s.direction === 'i_owe'
                ? 'ko.balanceOwed'
                : s.direction === 'household_owes_me'
                  ? 'ko.balanceOwing'
                  : s.direction === 'settled'
                    ? 'ko.balanceSettled'
                    : 'ko.settleNoBalance',
            )}
          </span>
        </div>

        {s.alreadySettled ? (
          <div className="ko-settle__done">
            <StatusLine
              text={t('ko.settleAlready', {
                comment: s.booking?.comment ?? s.suggestedComment,
                when: s.settledAt ? formatDateTime(s.settledAt) : '',
              })}
            />
            {s.settledBalanceCents != null && (
              <span className="footnote">
                {t('ko.settleBasedOn')}{' '}
                <Money cents={s.settledBalanceCents} basis="signed" />
              </span>
            )}
          </div>
        ) : (
          <div className="ko-settle__action">
            {!nothingToSettle && (
              <p className="ko-settle__suggestion">
                {t('ko.settleSuggestion')}{' '}
                <DataLabel>{s.suggestedComment}</DataLabel> ·{' '}
                <Money cents={s.amountCents} /> · {t('ko.settleAsTransfer')}
              </p>
            )}
            <Button
              onClick={() => book.mutate()}
              busy={book.isPending}
              disabled={nothingToSettle}
            >
              <ArrowLeftRight size={15} aria-hidden="true" />
              {t('ko.settleAction')}
            </Button>
          </div>
        )}
      </div>

      <p className="footnote">{t('ko.settleNoBalanceChange')}</p>
    </section>
  );
}

function StatusLine({ text }: { text: string }) {
  return (
    <span className="ko-settle__status">
      <Check size={15} aria-hidden="true" />
      {text}
    </span>
  );
}
