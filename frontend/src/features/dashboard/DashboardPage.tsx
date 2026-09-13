import { useQuery } from '@tanstack/react-query';
import { useSearchParams } from 'react-router-dom';

import { CategoryChip } from '../../components/DataLabel';
import { FlowMoney, Money, NetBreakdown, ScopeNote } from '../../components/Money';
import { ErrorState, Kpi, LoadingState, PageHeader } from '../../components/ui';
import { YearPicker } from '../../components/YearPicker';
import { api } from '../../lib/api';
import { formatEuro, formatPercent } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import type { Dashboard } from '../../lib/types';
import { KitchenOwlWidget } from '../kitchenowl/KitchenOwlWidget';
import { useMaskAmount } from '../../lib/privacy';
import { AnomalyNotes } from './AnomalyNotes';
import { ForecastPanel } from './ForecastPanel';

export function DashboardPage() {
  const t = useT();
  const maskAmount = useMaskAmount();
  const [params, setParams] = useSearchParams();
  const year = Number(params.get('jahr')) || new Date().getFullYear();
  // The current month in the current year, the last month otherwise: asking a past
  // year about "this month" would compare December with nothing.
  const now = new Date();
  const anomalyMonth = year === now.getFullYear() ? now.getMonth() + 1 : 12;

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

      {/* The dashboard read `jahr` from the URL and offered no way to set it, so
          its own year was reachable only by typing one. Same picker, same place,
          as every other screen. */}
      <div className="panel panel--pad filter-bar">
        <YearPicker
          id="dashboard-year"
          value={year}
          onChange={(next) =>
            setParams(
              (prev) => {
                const p = new URLSearchParams(prev);
                p.set('jahr', String(next));
                return p;
              },
              { replace: true },
            )
          }
        />
      </div>

      {/* The carryover gap is not reported here any more. It is a permanent,
          known property of the legacy import — the 2026 opening balance is a
          CONFIGURED figure and the 2023–2025 rows do not add up to it — so a
          warning banner on the screen opened every day was reporting a settled
          fact as if it were news. It is still computed, still in the API, and
          still shown in Einstellungen, which is where the opening balance is set
          and therefore the only place it is actionable. */}
      <div className="grid grid--kpi" style={{ marginBottom: '1rem' }}>
        <Kpi label={t('dashboard.income')} scope="without">
          <Money cents={d.incomeCents} tone="income" />
        </Kpi>
        <Kpi label={t('dashboard.expense')} scope="without">
          <Money cents={d.expenseCents} tone="expense" />
        </Kpi>
        <Kpi label={t('dashboard.balance')} scope="without">
          <FlowMoney flowCents={d.balanceCents} />
        </Kpi>
        {/* Deliberately adjacent to the balance, and deliberately a different
            scope — that contrast is what ScopeNote exists to explain. */}
        <Kpi label={t('dashboard.closingBalance')} scope="with" tone="accent">
          <Money cents={d.closingBalanceCents} />
        </Kpi>
      </div>

      <div className="grid grid--kpi" style={{ marginBottom: '1rem' }}>
        {/* The Sparrate leads, because it is the only one of the three that is a
            decision rather than a residue: it is what left the account for Sparen
            & Anlage, and the one figure here that can be checked against a bank
            statement. Per month, which is how a standing order is thought of. */}
        <Kpi
          label={t('dashboard.savingsDeposit')}
          hint={t('dashboard.savingsDepositHint')}
          scope="without"
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
          label={t('dashboard.savingsRateConsumption')}
          hint={t('dashboard.savingsRateConsumptionHint')}
          scope="without"
        >
          {formatPercent(d.savingsRateConsumption)}
          {/* The spreadsheet's own rate, kept because a figure that was on every
              screen for years should not silently disappear — but as a footnote to
              the one that supersedes it, not as a tile of equal weight. */}
          <span className="kpi__sub">
            {t('dashboard.savingsRateNaiveNote', { percent: formatPercent(d.savingsRateNaive) })}
          </span>
        </Kpi>
        <Kpi label={t('dashboard.averageExpense')} scope="without">
          <Money cents={d.averageExpensePerMonthCents} />
        </Kpi>
        <Kpi label={t('dashboard.fixedCosts')} scope="without">
          <Money cents={d.fixedCostsPerMonthCents} basis="net" />
        </Kpi>
      </div>

      <div className="grid grid--kpi" style={{ marginBottom: '1.5rem' }}>
        <Kpi label={t('dashboard.carryover')} scope="with">
          <Money cents={d.openingBalanceCents} />
        </Kpi>
        <Kpi label={t('dashboard.bookings')} scope="with">
          {d.bookingCount}
        </Kpi>
        <Kpi label={t('dashboard.taxRelevant')} scope="without">
          {d.taxRelevantCount}
        </Kpi>
        <Kpi
          label={t('dashboard.uncategorized')}
          scope="without"
          tone={d.uncategorizedCount > 0 ? 'warn' : undefined}
        >
          {d.uncategorizedCount}
        </Kpi>
      </div>

      <AnomalyNotes year={year} month={anomalyMonth} />
      <ForecastPanel year={year} />

      <div className="panel panel--pad" style={{ marginBottom: '1rem' }}>
        <h2>{t('dashboard.byType')}</h2>
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
        <h2>{t('dashboard.topCategories')}</h2>
        <p style={{ fontSize: '.8rem', color: 'var(--muted)', marginBottom: '.5rem' }}>
          {t('dashboard.costNote')}
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
                      orientation="cost"
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
