import { useQuery } from '@tanstack/react-query';
import { useSearchParams } from 'react-router-dom';

import { CategoryChip } from '../../components/DataLabel';
import { FlowMoney, Money, NetBreakdown, ScopeNote } from '../../components/Money';
import { ErrorState, LoadingState, PageHeader } from '../../components/ui';
import { api } from '../../lib/api';
import { formatEuro, formatPercent } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import type { Dashboard } from '../../lib/types';
import type { MessageKey } from '../../lib/messages/de';
import { KitchenOwlWidget } from '../kitchenowl/KitchenOwlWidget';
import { useMaskAmount } from '../../lib/privacy';

function Kpi({
  labelKey,
  children,
  hint,
  transfersIncluded,
  tone,
}: {
  labelKey: MessageKey;
  children: React.ReactNode;
  hint?: MessageKey;
  /** Required: a figure must declare whether transfers are inside it. */
  transfersIncluded: boolean;
  tone?: 'warn' | 'accent';
}) {
  const t = useT();
  return (
    <div className={`kpi ${tone ? `kpi--${tone}` : ''}`} title={hint ? t(hint) : undefined}>
      <span className="kpi__label">{t(labelKey)}</span>
      <span className="kpi__value">{children}</span>
      <ScopeNote transfersIncluded={transfersIncluded} />
    </div>
  );
}

export function DashboardPage() {
  const t = useT();
  const maskAmount = useMaskAmount();
  const [params] = useSearchParams();
  const year = Number(params.get('jahr')) || new Date().getFullYear();

  const query = useQuery({
    queryKey: qk.derived.dashboard(year),
    queryFn: () => api<Dashboard>(`/dashboard?year=${year}`),
  });

  if (query.isLoading) return <LoadingState />;
  if (query.isError) return <ErrorState error={query.error} retry={() => query.refetch()} />;
  const d = query.data!;

  return (
    <>
      <PageHeader title={t('dashboard.title', { year })} />

      {/* The carryover gap is not reported here any more. It is a permanent,
          known property of the legacy import — the 2026 opening balance is a
          CONFIGURED figure and the 2023–2025 rows do not add up to it — so a
          warning banner on the screen opened every day was reporting a settled
          fact as if it were news. It is still computed, still in the API, and
          still shown in Einstellungen, which is where the opening balance is set
          and therefore the only place it is actionable. */}
      <div className="grid grid--kpi" style={{ marginBottom: '1rem' }}>
        <Kpi labelKey="dashboard.income" transfersIncluded={false}>
          <Money cents={d.incomeCents} tone="income" />
        </Kpi>
        <Kpi labelKey="dashboard.expense" transfersIncluded={false}>
          <Money cents={d.expenseCents} tone="expense" />
        </Kpi>
        <Kpi labelKey="dashboard.balance" transfersIncluded={false}>
          <FlowMoney flowCents={d.balanceCents} />
        </Kpi>
        {/* Deliberately adjacent to the balance, and deliberately a different
            scope — that contrast is what ScopeNote exists to explain. */}
        <Kpi labelKey="dashboard.closingBalance" transfersIncluded tone="accent">
          <Money cents={d.closingBalanceCents} />
        </Kpi>
      </div>

      <div className="grid grid--kpi" style={{ marginBottom: '1rem' }}>
        {/* The Sparrate leads, because it is the only one of the three that is a
            decision rather than a residue: it is what left the account for Sparen
            & Anlage, and the one figure here that can be checked against a bank
            statement. Per month, which is how a standing order is thought of. */}
        <Kpi
          labelKey="dashboard.savingsDeposit"
          hint="dashboard.savingsDepositHint"
          transfersIncluded={false}
          tone="accent"
        >
          <Money cents={d.savingsDepositPerMonthCents} />
          <span className="kpi__sub">
            {t('dashboard.savingsDepositYear', {
              total: maskAmount(formatEuro(d.savingsDepositCents)),
              percent: formatPercent(d.savingsDepositRate),
            })}
          </span>
        </Kpi>
        <Kpi
          labelKey="dashboard.savingsRateConsumption"
          hint="dashboard.savingsRateConsumptionHint"
          transfersIncluded={false}
        >
          {formatPercent(d.savingsRateConsumption)}
          {/* The spreadsheet's own rate, kept because a figure that was on every
              screen for years should not silently disappear — but as a footnote to
              the one that supersedes it, not as a tile of equal weight. */}
          <span className="kpi__sub">
            {t('dashboard.savingsRateNaiveNote', { percent: formatPercent(d.savingsRateNaive) })}
          </span>
        </Kpi>
        <Kpi labelKey="dashboard.averageExpense" transfersIncluded={false}>
          <Money cents={d.averageExpensePerMonthCents} />
        </Kpi>
        <Kpi labelKey="dashboard.fixedCosts" transfersIncluded={false}>
          <Money cents={d.fixedCostsPerMonthCents} basis="net" />
        </Kpi>
      </div>

      <div className="grid grid--kpi" style={{ marginBottom: '1.5rem' }}>
        <Kpi labelKey="dashboard.carryover" transfersIncluded>
          <Money cents={d.openingBalanceCents} />
        </Kpi>
        <Kpi labelKey="dashboard.bookings" transfersIncluded>
          {d.bookingCount}
        </Kpi>
        <Kpi labelKey="dashboard.taxRelevant" transfersIncluded={false}>
          {d.taxRelevantCount}
        </Kpi>
        <Kpi
          labelKey="dashboard.uncategorized"
          transfersIncluded={false}
          tone={d.uncategorizedCount > 0 ? 'warn' : undefined}
        >
          {d.uncategorizedCount}
        </Kpi>
      </div>

      <div className="panel panel--pad" style={{ marginBottom: '1rem' }}>
        <h2 style={{ marginBottom: '.6rem' }}>{t('dashboard.byType')}</h2>
        <ScopeNote transfersIncluded={false} />
        <div className="table-wrap">
          <table className="data-table">
            <tbody>
              {d.byType
                .filter((row) => row.typeCode !== 'einkommen')
                .map((row) => (
                  <tr key={row.typeCode}>
                    <td>
                      <CategoryChip name={row.label} typeLabel={row.label} fallback="—" />
                    </td>
                    <td className="num">
                      <Money cents={row.netCents} basis="net" tone="auto" />
                    </td>
                  </tr>
                ))}
            </tbody>
          </table>
        </div>
      </div>

      {/* Below the personal ledger's own figures, boxed and labelled, because the
          one thing it must never look like is part of the arithmetic above it. */}
      <div style={{ marginBottom: '1rem' }}>
        <KitchenOwlWidget />
      </div>

      <div className="panel panel--pad">
        <h2 style={{ marginBottom: '.6rem' }}>{t('dashboard.topCategories')}</h2>
        <p style={{ fontSize: '.8rem', color: 'var(--muted)', marginBottom: '.5rem' }}>
          {t('analysis.intro')}
        </p>
        <div className="table-wrap">
          <table className="data-table">
            <thead>
              <tr>
                <th>{t('bookings.category')}</th>
                <th className="num">{t('bookings.expense')}</th>
                <th className="num">{t('bookings.income')}</th>
                <th className="num">{t('bookings.net')}</th>
              </tr>
            </thead>
            <tbody>
              {d.topCategories.map((row) => (
                <tr key={row.categoryName}>
                  <td>
                    <CategoryChip
                      name={row.categoryName}
                      typeLabel={row.categoryType}
                      fallback={t('bookings.sourceNone')}
                    />
                    {row.incomeCents > 0 && (
                      <span className="pill pill--info" style={{ marginLeft: '.4rem' }}>
                        {t('money.containsRefunds')}
                      </span>
                    )}
                  </td>
                  <td className="num">
                    <Money cents={row.expenseCents} tone="expense" />
                  </td>
                  <td className="num">
                    <Money cents={row.incomeCents} tone="income" />
                  </td>
                  <td className="num">
                    <NetBreakdown
                      incomeCents={row.incomeCents}
                      expenseCents={row.expenseCents}
                      netCents={row.netCents}
                      bookingCount={row.bookingCount}
                    />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </>
  );
}
