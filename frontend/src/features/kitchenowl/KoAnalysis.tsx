import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';

import { ChartFrame } from '../../charts/ChartFrame';
import { bands, niceTicks } from '../../charts/scales';
import { DataLabel } from '../../components/DataLabel';
import { Money } from '../../components/Money';
import { YearPicker } from '../../components/YearPicker';
import { Banner, Button, EmptyState, ErrorState, LoadingState } from '../../components/ui';
import { api, asList } from '../../lib/api';
import { formatEuroCompact, formatPercent, monthShort } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { useMaskAmount } from '../../lib/privacy';
import { qk } from '../../lib/queryKeys';
import type {
  KoCategoryAnalysis,
  KoExpense,
  KoCategoryAnalysisRow,
  KoExpensePage,
  KoMonthlySeries,
  KoSeriesSubject,
} from '../../lib/types';

const LEFT = 62;
const RIGHT = 710;

type Subject = { mode: 'category' | 'name' | 'uncategorized'; value: string };

/**
 * The household's own analysis — the same questions the personal Auswertung
 * answers, asked of the mirror.
 *
 * It is a deliberate twin and a deliberate stranger. Twin, because "what does
 * Wocheneinkauf cost us, and which months" is the same question in both ledgers
 * and should not need a second way of reading. Stranger, because the household's
 * figures are not the user's: every amount here is a PAIR — what the household
 * spent and the slice the user carries — and the two are never added, to each
 * other or to anything in the personal ledger. No figure on this screen may be
 * compared with a booking.
 */
export function KoAnalysis() {
  const t = useT();
  const maskAmount = useMaskAmount();
  const [year, setYear] = useState(() => new Date().getFullYear());
  const [subject, setSubject] = useState<Subject | null>(null);

  const analysis = useQuery({
    queryKey: qk.kitchenowl.analysis(year),
    queryFn: () => api<KoCategoryAnalysis>(`/kitchenowl/analysis/categories?year=${year}`),
  });
  const subjects = useQuery({
    queryKey: qk.kitchenowl.seriesSubjects(year),
    queryFn: () => api<KoSeriesSubject[]>(`/kitchenowl/analysis/series/subjects?year=${year}`),
    staleTime: 5 * 60_000,
  });

  const rows = asList<KoCategoryAnalysisRow>(analysis.data?.rows);
  const names = asList<KoSeriesSubject>(subjects.data);

  // Opens on the biggest category, so the chart answers something before it is
  // touched — the same choice the personal analysis makes.
  const active: Subject | null =
    subject ??
    (rows.length > 0
      ? rows[0].koCategoryId === null
        ? { mode: 'uncategorized', value: '' }
        : { mode: 'category', value: String(rows[0].koCategoryId) }
      : null);

  return (
    <div className="ko-analysis">
      <div className="panel panel--pad ko-analysis__controls">
        <div style={{ minWidth: '8rem' }}>
          <YearPicker
            id="ko-analysis-year"
            value={year}
            years={analysis.data?.years}
            onChange={(next) => {
              setYear(next);
              setSubject(null);
            }}
          />
        </div>
        <p className="footnote" style={{ margin: 0 }}>
          {t('ko.analysisScope')}
        </p>
      </div>

      {analysis.isPending && <LoadingState />}
      {analysis.isError && <ErrorState error={analysis.error} retry={() => analysis.refetch()} />}

      {analysis.data && rows.length === 0 && <EmptyState hint={t('ko.analysisEmpty')} />}

      {analysis.data && rows.length > 0 && (
        <>
          {analysis.data.excludedCount > 0 && (
            <Banner tone="info">
              {t('ko.analysisExcluded', { count: analysis.data.excludedCount })}
            </Banner>
          )}

          <div className="ko-analysis__totals panel panel--pad">
            <div>
              <span className="kpi__label">{t('ko.householdTotal')}</span>
              <span className="kpi__value">
                <Money cents={analysis.data.totalAmountCents} basis="household" />
              </span>
            </div>
            <div>
              <span className="kpi__label">{t('ko.myTotal')}</span>
              <span className="kpi__value">
                <Money cents={analysis.data.totalOwnShareCents} basis="share" />
              </span>
            </div>
            <div>
              <span className="kpi__label">{t('ko.expenseCount')}</span>
              <span className="kpi__value num">{analysis.data.expenseCount}</span>
            </div>
            <div>
              <span className="kpi__label">{t('ko.paidBy')}</span>
              <span className="ko-analysis__payers">
                {analysis.data.paidBy.map((p) => (
                  <span key={p.name}>
                    <DataLabel>{p.name}</DataLabel>{' '}
                    <Money cents={p.amountCents} basis="household" />
                  </span>
                ))}
              </span>
            </div>
          </div>

          <KoSeries
            year={year}
            active={active}
            rows={rows}
            names={names}
            onSelect={setSubject}
            maskAmount={maskAmount}
          />

          <div className="panel table-wrap screen-table">
            <table className="data-table">
              <thead>
                <tr>
                  <th>{t('ko.category')}</th>
                  <th className="num">{t('ko.household')}</th>
                  <th className="num">{t('ko.myShare')}</th>
                  <th className="num">{t('analysis.share')}</th>
                  <th className="num">{t('analysis.perMonth')}</th>
                  <th className="num">{t('analysis.count')}</th>
                  {Array.from({ length: 12 }, (_, i) => (
                    <th key={i} className="num month-cell">
                      <DataLabel>{monthShort(i + 1)}</DataLabel>
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => {
                  const target: Subject =
                    row.koCategoryId === null
                      ? { mode: 'uncategorized', value: '' }
                      : { mode: 'category', value: String(row.koCategoryId) };
                  const isActive =
                    active?.mode === target.mode && active?.value === target.value;
                  return (
                    <tr
                      key={row.koCategoryId ?? 'none'}
                      className={isActive ? 'is-charted' : undefined}
                    >
                      <th scope="row">
                        <button
                          type="button"
                          className="linkish"
                          aria-pressed={isActive}
                          title={t('ko.chartThis')}
                          onClick={() => setSubject(target)}
                        >
                          {row.koCategoryName ? (
                            <DataLabel>{row.koCategoryName}</DataLabel>
                          ) : (
                            t('ko.noCategory')
                          )}
                        </button>
                      </th>
                      <td className="num">
                        <Money cents={row.amountCents} basis="household" />
                      </td>
                      <td className="num">
                        <Money cents={row.ownShareCents} basis="share" />
                      </td>
                      <td className="num">{formatPercent(row.shareOfTotal)}</td>
                      <td className="num">
                        <Money cents={row.averagePerMonthCents} basis="household" />
                      </td>
                      <td className="num">{row.expenseCount}</td>
                      {row.monthlyAmountCents.map((cents, i) => (
                        <td key={i} className="num">
                          {cents === 0 ? (
                            <span className="money money--empty">–</span>
                          ) : (
                            <Money cents={cents} basis="household" />
                          )}
                        </td>
                      ))}
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>

          <div className="screen-cards">
            {rows.map((row) => {
              const target: Subject =
                row.koCategoryId === null
                  ? { mode: 'uncategorized', value: '' }
                  : { mode: 'category', value: String(row.koCategoryId) };
              return (
                <article key={row.koCategoryId ?? 'none'} className="mcard">
                  <header>
                    <strong>
                      <button
                        type="button"
                        className="linkish"
                        title={t('ko.chartThis')}
                        onClick={() => setSubject(target)}
                      >
                        {row.koCategoryName ? (
                          <DataLabel>{row.koCategoryName}</DataLabel>
                        ) : (
                          t('ko.noCategory')
                        )}
                      </button>
                    </strong>
                    <span className="kpi__scope">
                      {row.expenseCount} {t('analysis.count')}
                    </span>
                  </header>
                  <dl className="mcard__grid">
                    <div>
                      <dt>{t('ko.household')}</dt>
                      <dd>
                        <Money cents={row.amountCents} basis="household" />
                      </dd>
                    </div>
                    <div>
                      <dt>{t('ko.myShare')}</dt>
                      <dd>
                        <Money cents={row.ownShareCents} basis="share" />
                      </dd>
                    </div>
                    <div>
                      <dt>{t('analysis.perMonth')}</dt>
                      <dd>
                        <Money cents={row.averagePerMonthCents} basis="household" />
                      </dd>
                    </div>
                    <div>
                      <dt>{t('analysis.share')}</dt>
                      <dd>{formatPercent(row.shareOfTotal)}</dd>
                    </div>
                  </dl>
                </article>
              );
            })}
          </div>

          <p className="footnote">{t('ko.analysisNote')}</p>
        </>
      )}
    </div>
  );
}

/**
 * Twelve months for one category or one recurring name.
 *
 * Each month draws the household's bar with the user's share INSIDE it rather
 * than beside it. Side by side invites reading two comparable costs; nested says
 * what is true — one is a slice of the other.
 */
function KoSeries({
  year,
  active,
  rows,
  names,
  onSelect,
  maskAmount,
}: {
  year: number;
  active: Subject | null;
  rows: KoCategoryAnalysisRow[];
  names: KoSeriesSubject[];
  onSelect: (s: Subject) => void;
  maskAmount: (text: string) => string;
}) {
  const t = useT();
  const [showExpenses, setShowExpenses] = useState(false);

  const path = (() => {
    if (!active) return null;
    const base = `/kitchenowl/analysis/series?year=${year}`;
    if (active.mode === 'category') return `${base}&koCategoryId=${active.value}`;
    if (active.mode === 'uncategorized') return `${base}&uncategorized=true`;
    return `${base}&name=${encodeURIComponent(active.value)}`;
  })();

  const series = useQuery({
    queryKey: qk.kitchenowl.series(year, active?.mode ?? '', active?.value ?? ''),
    queryFn: () => api<KoMonthlySeries>(path as string),
    enabled: Boolean(path),
  });

  const expensesPath = (() => {
    if (!active) return null;
    const qs = new URLSearchParams({ year: String(year), pageSize: '50' });
    if (active.mode === 'category') qs.set('koCategoryId', active.value);
    if (active.mode === 'uncategorized') qs.set('uncategorized', 'true');
    if (active.mode === 'name') qs.set('search', active.value);
    return `/kitchenowl/expenses?${qs.toString()}`;
  })();

  const expenses = useQuery({
    queryKey: qk.kitchenowl.expenses({ year, scope: 'series', ...active }),
    queryFn: () => api<KoExpensePage>(expensesPath as string),
    enabled: showExpenses && Boolean(expensesPath),
  });

  const chart = useMemo(() => {
    const months = series.data?.months ?? [];
    const ticks = niceTicks(0, Math.max(1, ...months.map((m) => m.amountCents)));
    return { months, ticks };
  }, [series.data]);

  const height = 240;
  const hi = chart.ticks[chart.ticks.length - 1] ?? 1;
  const y = (v: number) => 14 + (1 - v / (hi || 1)) * (height - 52);
  const band = bands(12, LEFT, RIGHT);
  const baseline = y(0);

  return (
    <section className="panel panel--pad series">
      <header className="series__header">
        <h2>{t('ko.seriesTitle')}</h2>
        <p className="footnote">{t('ko.seriesHint')}</p>
      </header>

      <div className="series__controls">
        <div className="field series__picker">
          <label htmlFor="ko-series-subject">{t('ko.category')}</label>
          <select
            id="ko-series-subject"
            className="select"
            value={active ? `${active.mode}:${active.value}` : ''}
            onChange={(e) => {
              const [mode, ...rest] = e.target.value.split(':');
              onSelect({ mode: mode as Subject['mode'], value: rest.join(':') });
            }}
          >
            <optgroup label={t('ko.category')}>
              {rows.map((row) =>
                row.koCategoryId === null ? (
                  <option key="none" value="uncategorized:">
                    {t('ko.noCategory')} ({row.expenseCount})
                  </option>
                ) : (
                  <option key={row.koCategoryId} value={`category:${row.koCategoryId}`}>
                    {row.koCategoryName} ({row.expenseCount})
                  </option>
                ),
              )}
            </optgroup>
            {names.length > 0 && (
              <optgroup label={t('ko.byName')}>
                {names.map((n) => (
                  <option key={n.name} value={`name:${n.name}`}>
                    {n.name} ({n.expenseCount})
                  </option>
                ))}
              </optgroup>
            )}
          </select>
        </div>
      </div>

      {series.isError && <ErrorState error={series.error} />}
      {series.data && (
        <>
          <div className="series__stats">
            <div>
              <span className="kpi__label">{t('ko.household')}</span>
              <Money cents={series.data.amountCents} basis="household" />
            </div>
            <div>
              <span className="kpi__label">{t('ko.myShare')}</span>
              <Money cents={series.data.ownShareCents} basis="share" />
            </div>
            <div>
              <span className="kpi__label">{t('analysis.perActiveMonth')}</span>
              <Money cents={series.data.averagePerActiveMonthCents} basis="household" />
            </div>
            <div>
              <span className="kpi__label">{t('analysis.count')}</span>
              <span className="num">{series.data.expenseCount}</span>
            </div>
          </div>

          <p className="series__drilldown">
            <Button
              variant="ghost"
              onClick={() => setShowExpenses((open) => !open)}
              aria-expanded={showExpenses}
            >
              {showExpenses
                ? t('ko.hideExpenses')
                : t('ko.showExpenses', { count: series.data.expenseCount })}
            </Button>
          </p>

          {showExpenses && (
            <div className="series__bookings">
              {expenses.isPending && <LoadingState />}
              {expenses.isError && <ErrorState error={expenses.error} />}
              {expenses.data && (
                <table className="table table--compact">
                  <thead>
                    <tr>
                      <th>{t('ko.expenseName')}</th>
                      <th>{t('ko.paidByShort')}</th>
                      <th className="num">{t('ko.household')}</th>
                      <th className="num">{t('ko.myShare')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {asList<KoExpense>(expenses.data.items).map((e) => (
                      <tr key={e.id}>
                        <td>
                          <DataLabel>{e.name}</DataLabel>
                        </td>
                        <td>
                          {e.paidByName ? <DataLabel>{e.paidByName}</DataLabel> : '—'}
                        </td>
                        <td className="num">
                          <Money cents={e.amountCents} basis="household" />
                        </td>
                        <td className="num">
                          <Money cents={e.ownShareCents} basis="share" />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          )}

          <ChartFrame
            title={`${series.data.subject || t('ko.noCategory')} · ${year}`}
            columns={[t('ko.household'), t('ko.myShare')]}
            valueBasis="gross"
            note={t('ko.seriesOrientation')}
            data={chart.months.map((m) => ({
              label: m.monthName,
              values: [m.amountCents, m.ownShareCents],
            }))}
            height={height}
          >
            {chart.ticks.map((tick) => (
              <g key={tick}>
                <line x1={LEFT} x2={RIGHT} y1={y(tick)} y2={y(tick)} className="chart__grid" />
                <text x={LEFT - 8} y={y(tick) + 4} className="chart__tick" textAnchor="end">
                  {maskAmount(formatEuroCompact(tick))}
                </text>
              </g>
            ))}
            {chart.months.map((m, i) => (
              <g key={m.month}>
                {/* The household's bar, with the user's slice drawn inside it. */}
                <rect
                  x={band.start(i) + band.width * 0.15}
                  y={y(m.amountCents)}
                  width={band.width * 0.7}
                  height={Math.max(0, baseline - y(m.amountCents))}
                  className="chart__bar chart__bar--household"
                />
                <rect
                  x={band.start(i) + band.width * 0.3}
                  y={y(m.ownShareCents)}
                  width={band.width * 0.4}
                  height={Math.max(0, baseline - y(m.ownShareCents))}
                  className="chart__bar chart__bar--share"
                />
                <text
                  x={band.centre(i)}
                  y={height - 18}
                  className={`chart__tick ${m.expenseCount === 0 ? 'chart__tick--dim' : ''}`}
                  textAnchor="middle"
                >
                  {monthShort(m.month)}
                </text>
              </g>
            ))}
            <line x1={LEFT} x2={RIGHT} y1={baseline} y2={baseline} className="chart__baseline" />
          </ChartFrame>
        </>
      )}
    </section>
  );
}
