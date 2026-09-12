import { useQuery } from '@tanstack/react-query';
import { useSearchParams } from 'react-router-dom';

import { CumulativeLine, MonthlyBars, TypeBreakdown, type MonthPoint } from '../../charts/MonthlyCharts';
import { DataLabel } from '../../components/DataLabel';
import { Money, ScopeNote } from '../../components/Money';
import { EmptyState, ErrorState, LoadingState, PageHeader } from '../../components/ui';
import { api } from '../../lib/api';
import { formatPercent, monthShort } from '../../lib/format';
import { YearPicker } from '../../components/YearPicker';
import { useT } from '../../lib/i18n';
import { qk } from '../../lib/queryKeys';
import type { CategoryTypeSummary, MonthlyOverview, MonthlyRow } from '../../lib/types';

/**
 * The four per-type columns, in the spreadsheet's order.
 *
 * The labels are NOT in here: "Fixkosten" and "Variable Kosten" are type names
 * from the database, which makes them data rather than interface, and a test
 * fails if one ever lands in the message catalogue. They come from
 * `/category-types` and are wrapped in `<DataLabel>` like every other data string.
 */
const TYPE_COLUMNS: { code: string; pick: (m: MonthlyRow) => number }[] = [
  { code: 'fixkosten', pick: (m) => m.fixedCostsNetCents },
  { code: 'variabel', pick: (m) => m.variableCostsNetCents },
  { code: 'sparen', pick: (m) => m.savingsNetCents },
  { code: 'sonstiges', pick: (m) => m.otherNetCents },
];

export function MonthsPage() {
  const t = useT();
  const [params, setParams] = useSearchParams();
  const year = Number(params.get('jahr')) || new Date().getFullYear();

  const overview = useQuery({
    queryKey: qk.derived.months(year),
    queryFn: () => api<MonthlyOverview>(`/overview/months?year=${year}`),
  });
  const types = useQuery({
    queryKey: qk.taxonomy.types(),
    queryFn: () => api<CategoryTypeSummary[]>('/category-types'),
    staleTime: 5 * 60_000,
  });

  const label = (code: string) => types.data?.find((x) => x.typeCode === code)?.label ?? code;

  return (
    <>
      <PageHeader title={t('months.title', { year })} subtitle={t('months.intro')} />

      <div
        className="panel panel--pad"
        style={{ marginBottom: '1rem', display: 'flex', gap: '.75rem', flexWrap: 'wrap', alignItems: 'flex-end' }}
      >
        <div style={{ minWidth: '8rem' }}>
          <YearPicker id="months-year" value={year} onChange={(next) =>
            setParams(
              (prev) => {
                const p = new URLSearchParams(prev);
                p.set('jahr', String(next));
                return p;
              },
              { replace: true },
            )} />
        </div>
        <p className="kpi__scope" style={{ margin: 0, maxWidth: '38rem' }}>
          {t('months.perTypeNote')}
        </p>
      </div>

      {(overview.isLoading || types.isLoading) && <LoadingState />}
      {overview.isError && (
        <ErrorState error={overview.error} retry={() => overview.refetch()} />
      )}
      {types.isError && <ErrorState error={types.error} retry={() => types.refetch()} />}

      {overview.data && overview.data.total.bookingCount === 0 && (
        <EmptyState hint={t('months.empty')} />
      )}
      {overview.data && types.data && overview.data.total.bookingCount > 0 && (
        <MonthsBody data={overview.data} label={label} />
      )}
    </>
  );
}

function MonthsBody({
  data,
  label,
}: {
  data: MonthlyOverview;
  label: (code: string) => string;
}) {
  const t = useT();

  /**
   * The last month that actually has bookings.
   *
   * The wire carries a cumulative for the months AFTER it too — the server keeps
   * the running value alive once data has been seen, which is right for a gap
   * inside the year and wrong for the tail of it. Left alone, a year with data
   * through September would report the same balance for October, November and
   * December as if those months had happened and changed nothing. So the tail is
   * cut here, and `null` from the server (the months before the first booking) is
   * rendered as absent either way.
   */
  const lastWithData = data.months.reduce((acc, m, i) => (m.bookingCount > 0 ? i : acc), -1);
  const cumulative = (index: number) =>
    index <= lastWithData ? data.months[index].cumulativeCents : null;

  const points: MonthPoint[] = data.months.map((m) => ({
    monthName: m.monthName,
    short: monthShort(m.month),
    incomeCents: m.incomeCents,
    expenseCents: m.expenseCents,
    cumulativeCents: m.cumulativeCents,
    hasData: m.bookingCount > 0,
  }));

  return (
    <>
      <div className="chart-grid chart-grid--pair" style={{ marginBottom: '1rem' }}>
          <div className="panel panel--pad">
            <MonthlyBars points={points} />
            <ScopeNote transfersIncluded={false} />
          </div>
          <div className="panel panel--pad">
            <TypeBreakdown
              slices={TYPE_COLUMNS.map((c) => ({
                label: label(c.code),
                typeCode: c.code,
                netCents: c.pick(data.total),
              }))}
            />
            <ScopeNote transfersIncluded={false} />
          </div>
      </div>
      <div className="panel panel--pad" style={{ marginBottom: '1rem' }}>
        <CumulativeLine points={points} />
        <ScopeNote transfersIncluded />
      </div>

      <div className="panel table-wrap screen-table">
        <table className="data-table">
          <caption>{t('months.emptyRowNote')}</caption>
          <thead>
            <tr>
              <th>{t('common.month')}</th>
              <th className="num">{t('bookings.income')}</th>
              <th className="num">{t('bookings.expense')}</th>
              <th className="num">{t('months.balance')}</th>
              <th className="num" title={t('months.cumulativeHint')}>
                {t('months.cumulative')}
              </th>
              <th className="num" title={t('months.savingsRateHint')}>
                {t('months.savingsRate')}
              </th>
              {TYPE_COLUMNS.map((c) => (
                <th key={c.code} className="num">
                  <DataLabel>{label(c.code)}</DataLabel>
                </th>
              ))}
              <th className="num">{t('months.bookings')}</th>
            </tr>
          </thead>
          <tbody>
            {data.months.map((m, index) => {
              const empty = m.bookingCount === 0;
              return (
                <tr key={m.month} className={empty ? 'row--empty' : undefined}>
                  <th scope="row" style={{ fontWeight: 600 }}>
                    <DataLabel>{m.monthName}</DataLabel>
                    {empty && (
                      <span className="kpi__scope" style={{ marginLeft: '.4rem' }}>
                        {t('months.noBookings')}
                      </span>
                    )}
                  </th>
                  {/* A month with no bookings is not a month that came to zero, so
                      every figure in it is absent rather than 0,00. */}
                  <td className="num">
                    <Money cents={empty ? null : m.incomeCents} tone="income" />
                  </td>
                  <td className="num">
                    <Money cents={empty ? null : m.expenseCents} tone="expense" />
                  </td>
                  <td className="num">
                    <Money cents={empty ? null : m.balanceCents} basis="signed" tone="auto" />
                  </td>
                  {/* Absent where nothing is booked. Never zero-filled: a zero
                      here would draw a cliff in the running balance. */}
                  <td className="num">
                    <Money cents={cumulative(index)} />
                  </td>
                  <td className="num">
                    {m.savingsRate === null ? (
                      <span className="money money--empty">–</span>
                    ) : (
                      formatPercent(m.savingsRate)
                    )}
                  </td>
                  {TYPE_COLUMNS.map((c) => (
                    <td key={c.code} className="num">
                      <Money cents={empty ? null : c.pick(m)} basis="net" tone="auto" />
                    </td>
                  ))}
                  <td className="num">{m.bookingCount}</td>
                </tr>
              );
            })}
          </tbody>
          <tfoot>
            <tr>
              <th scope="row">{t('common.total')}</th>
              <td className="num">
                <Money cents={data.total.incomeCents} tone="income" />
              </td>
              <td className="num">
                <Money cents={data.total.expenseCents} tone="expense" />
              </td>
              <td className="num">
                <Money cents={data.total.balanceCents} basis="signed" tone="auto" />
              </td>
              <td className="num">
                {/* The year's close is the balance, and the server sends no
                    cumulative for the total row rather than repeating it. */}
                <Money cents={data.total.cumulativeCents} />
              </td>
              <td className="num">
                {data.total.savingsRate === null ? (
                  <span className="money money--empty">–</span>
                ) : (
                  formatPercent(data.total.savingsRate)
                )}
              </td>
              {TYPE_COLUMNS.map((c) => (
                <td key={c.code} className="num">
                  <Money cents={c.pick(data.total)} basis="net" tone="auto" />
                </td>
              ))}
              <td className="num">{data.total.bookingCount}</td>
            </tr>
          </tfoot>
        </table>
      </div>

      {/* Eleven columns cannot work one-handed, so the phone gets cards. */}
      <div className="screen-cards">
        {data.months.map((m, index) => {
          const empty = m.bookingCount === 0;
          return (
            <article key={m.month} className={`mcard ${empty ? 'mcard--empty' : ''}`}>
              <header>
                <strong>
                  <DataLabel>{m.monthName}</DataLabel>
                </strong>
                <span className="kpi__scope">
                  {empty ? t('months.noBookings') : `${m.bookingCount} ${t('months.bookings')}`}
                </span>
              </header>
              {!empty && (
                <dl className="mcard__grid">
                  <div>
                    <dt>{t('bookings.income')}</dt>
                    <dd>
                      <Money cents={m.incomeCents} tone="income" />
                    </dd>
                  </div>
                  <div>
                    <dt>{t('bookings.expense')}</dt>
                    <dd>
                      <Money cents={m.expenseCents} tone="expense" />
                    </dd>
                  </div>
                  <div>
                    <dt>{t('months.balance')}</dt>
                    <dd>
                      <Money cents={m.balanceCents} basis="signed" tone="auto" />
                    </dd>
                  </div>
                  <div>
                    <dt>{t('months.cumulative')}</dt>
                    <dd>
                      <Money cents={cumulative(index)} />
                    </dd>
                  </div>
                  {TYPE_COLUMNS.map((c) => (
                    <div key={c.code}>
                      <dt>
                        <DataLabel>{label(c.code)}</DataLabel>
                      </dt>
                      <dd>
                        <Money cents={c.pick(m)} basis="net" tone="auto" />
                      </dd>
                    </div>
                  ))}
                </dl>
              )}
            </article>
          );
        })}
      </div>
    </>
  );
}
