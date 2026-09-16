import { describe, expect, it } from 'vitest';

import { buildFlow, buildKoFlow, monthsWithActivity, netInPeriod } from './sankey';
import type {
  CategoryAnalysisRow,
  CategoryTypeSummary,
  KoCategoryAnalysisRow,
  KoPayerShare,
} from './types';

const TYPES: CategoryTypeSummary[] = [
  { typeCode: 'einkommen', label: 'Einkommen', netCents: 0, bookingCount: 0 },
  { typeCode: 'fixkosten', label: 'Fixkosten', netCents: 0, bookingCount: 0 },
  { typeCode: 'variabel', label: 'Variable Kosten', netCents: 0, bookingCount: 0 },
  { typeCode: 'sparen', label: 'Sparen', netCents: 0, bookingCount: 0 },
  { typeCode: 'sonstiges', label: 'Sonstiges', netCents: 0, bookingCount: 0 },
];

const LABELS = {
  hub: 'Einnahmen',
  surplus: 'Übrig',
  deficit: 'Aus dem Bestand',
  noType: 'Ohne Typ',
};

function row(
  name: string,
  type: string | null,
  netCents: number,
  monthly: Partial<Record<number, number>> = {},
): CategoryAnalysisRow {
  return {
    categoryId: name.toLowerCase(),
    categoryName: name,
    categoryType: type,
    incomeCents: netCents < 0 ? -netCents : 0,
    expenseCents: netCents > 0 ? netCents : 0,
    netCents,
    netIsNegative: netCents < 0,
    shareOfTotal: 0,
    averagePerMonthCents: 0,
    bookingCount: 1,
    monthlyNetCents: Array.from({ length: 12 }, (_, i) => monthly[i + 1] ?? 0),
  };
}

/** The verified 2026 figures, in the stored expense-positive convention. */
const YEAR_2026: CategoryAnalysisRow[] = [
  row('Gehalt', 'Einkommen', -2200000),
  row('Freelancing', 'Einkommen', -600000),
  row('Miete', 'Fixkosten', 480000),
  row('Versicherungen', 'Fixkosten', 120000),
  row('Sparen & Anlage', 'Sparen', 648000),
  row('Reisen & Urlaub', 'Variable Kosten', 240000),
  row('Bargeld', 'Sonstiges', 37000),
];

describe('buildFlow', () => {
  it('splits by the sign of the net, not by the category type', () => {
    const flow = buildFlow({ rows: YEAR_2026, types: TYPES, labels: LABELS });

    expect(flow.sources.map((n) => n.label)).toEqual(['Gehalt', 'Freelancing']);
    expect(flow.inflowCents).toBe(2200000 + 600000);
    expect(flow.outflowCents).toBe(480000 + 120000 + 648000 + 240000 + 37000);
    expect(flow.saldoCents).toBe(flow.inflowCents - flow.outflowCents);
  });

  it('balances: what came in either went out or was left over', () => {
    const flow = buildFlow({ rows: YEAR_2026, types: TYPES, labels: LABELS });

    const left = flow.sources.reduce((sum, n) => sum + n.amountCents, 0);
    const right = flow.targets.reduce((sum, n) => sum + n.amountCents, 0);
    expect(left).toBe(right);
    expect(flow.totalCents).toBe(left);
    // and every link is attached to a node that exists
    const keys = new Set([...flow.sources, ...flow.targets, flow.hub].map((n) => n.key));
    for (const link of flow.links) {
      expect(keys.has(link.from)).toBe(true);
      expect(keys.has(link.to)).toBe(true);
    }
  });

  it('groups the outflows into their types, in a fixed order', () => {
    const flow = buildFlow({ rows: YEAR_2026, types: TYPES, labels: LABELS });

    expect(flow.targets.map((n) => n.label)).toEqual([
      'Fixkosten',
      'Variable Kosten',
      'Sparen',
      'Sonstiges',
      'Übrig',
    ]);
    expect(flow.targets[0]?.amountCents).toBe(480000 + 120000);
  });

  it('shows a reimbursement month as an inflow rather than a negative bar', () => {
    // Juni 2026: Dienstreisen nets −300,00 after the NetSoft refund.
    const rows = [
      row('Gehalt', 'Einkommen', -300000, { 6: -300000 }),
      row('Dienstreisen', 'Variable Kosten', -30000, { 6: -30000 }),
      row('Miete', 'Fixkosten', 110000, { 6: 110000 }),
    ];
    const flow = buildFlow({ rows, types: TYPES, month: 6, labels: LABELS });

    const credit = flow.sources.find((n) => n.label === 'Dienstreisen');
    expect(credit?.amountCents).toBe(30000);
    expect(credit?.credit).toBe(true);
    expect(flow.creditCount).toBe(1);
    // ...and it is NOT also sitting in Variable Kosten on the right
    expect(flow.targets.map((n) => n.label)).toEqual(['Fixkosten', 'Übrig']);
    expect(flow.saldoCents).toBe(300000 + 30000 - 110000);
  });

  it('funds a deficit period from the balance so both sides still add up', () => {
    const rows = [
      row('Gehalt', 'Einkommen', -100000, { 4: -100000 }),
      row('Reisen & Urlaub', 'Variable Kosten', 250000, { 4: 250000 }),
    ];
    const flow = buildFlow({ rows, types: TYPES, month: 4, labels: LABELS });

    expect(flow.saldoCents).toBe(-150000);
    expect(flow.sources[0]).toMatchObject({ key: 'deficit', amountCents: 150000 });
    expect(flow.targets.some((n) => n.key === 'surplus')).toBe(false);
    const left = flow.sources.reduce((sum, n) => sum + n.amountCents, 0);
    const right = flow.targets.reduce((sum, n) => sum + n.amountCents, 0);
    expect(left).toBe(right);
  });

  it('breaks one type into its categories without changing the total', () => {
    const flow = buildFlow({
      rows: YEAR_2026,
      types: TYPES,
      expanded: 'fixkosten',
      labels: LABELS,
    });

    expect(flow.targets.map((n) => n.label)).toEqual([
      'Miete',
      'Versicherungen',
      'Variable Kosten',
      'Sparen',
      'Sonstiges',
      'Übrig',
    ]);
    expect(flow.targets.filter((n) => n.nested).map((n) => n.amountCents)).toEqual([
      480000, 120000,
    ]);
    expect(flow.totalCents).toBe(buildFlow({ rows: YEAR_2026, types: TYPES, labels: LABELS }).totalCents);
  });

  it('fans every type out into a fourth column without changing a figure', () => {
    const plain = buildFlow({ rows: YEAR_2026, types: TYPES, labels: LABELS });
    const flow = buildFlow({ rows: YEAR_2026, types: TYPES, fanOut: true, labels: LABELS });

    // The types keep their own column and their own subtotals...
    expect(flow.targets.map((n) => n.label)).toEqual(plain.targets.map((n) => n.label));
    expect(flow.targets.map((n) => n.amountCents)).toEqual(plain.targets.map((n) => n.amountCents));
    expect(flow.totalCents).toBe(plain.totalCents);

    // ...and the categories hang off them, in the same order.
    expect(flow.leaves.map((n) => n.label)).toEqual([
      'Miete',
      'Versicherungen',
      'Reisen & Urlaub',
      'Sparen & Anlage',
      'Bargeld',
      // What is left over has no categories, so it passes straight through.
      'Übrig',
    ]);
    // Every leaf hangs off a target, and each type's leaves sum to it.
    const parent = new Map(flow.links.map((l) => [l.to, l.from]));
    for (const target of flow.targets) {
      const mine = flow.leaves.filter((leaf) => parent.get(leaf.key) === target.key);
      expect(mine.reduce((sum, n) => sum + n.amountCents, 0)).toBe(target.amountCents);
    }
    // ...so the fourth column still adds up to the same total as the first.
    expect(flow.leaves.reduce((sum, n) => sum + n.amountCents, 0)).toBe(flow.totalCents);
  });

  it('draws no fourth column unless it was asked for', () => {
    expect(buildFlow({ rows: YEAR_2026, types: TYPES, labels: LABELS }).leaves).toEqual([]);
    // ...and the per-type drill-down stands down while it is on, so the two ways
    // of showing the same categories cannot both be active.
    const fanned = buildFlow({ rows: YEAR_2026, types: TYPES, fanOut: true, labels: LABELS });
    expect(fanned.targets.every((n) => !n.expandable)).toBe(true);
  });

  it('keeps a category with no type visible instead of folding it into Sonstiges', () => {
    const flow = buildFlow({
      rows: [row('Gehalt', 'Einkommen', -100000), row('Ohne Kategorie', null, 5000)],
      types: TYPES,
      labels: LABELS,
    });

    const none = flow.targets.find((n) => n.label === 'Ohne Typ');
    expect(none?.amountCents).toBe(5000);
    expect(none?.expandable).toBeFalsy();
  });

  it('drops categories that did nothing in the period', () => {
    const rows = [
      row('Gehalt', 'Einkommen', -100000, { 1: -100000 }),
      row('Miete', 'Fixkosten', 110000, { 2: 110000 }),
    ];
    expect(monthsWithActivity(rows)).toEqual([1, 2]);
    expect(netInPeriod(rows[1] as CategoryAnalysisRow, 1)).toBe(0);

    const january = buildFlow({ rows, types: TYPES, month: 1, labels: LABELS });
    expect(january.targets.map((n) => n.label)).toEqual(['Übrig']);
    expect(january.outflowCents).toBe(0);
  });
});

describe('buildKoFlow', () => {
  const KO_LABELS = {
    hub: 'Haushaltsausgaben',
    mine: 'Mein Anteil',
    others: 'Anteil der anderen',
    noCategory: 'Ohne Kategorie',
    unknownPayer: 'Unbekannt',
  };

  function koRow(
    name: string | null,
    amount: number,
    own: number,
    monthlyAmount: number[] = [],
    monthlyOwn: number[] = [],
  ): KoCategoryAnalysisRow {
    return {
      koCategoryId: name === null ? null : name.length,
      koCategoryName: name,
      amountCents: amount,
      ownShareCents: own,
      expenseCount: 1,
      shareOfTotal: 0,
      averagePerMonthCents: 0,
      averageOwnSharePerMonthCents: 0,
      monthlyAmountCents: Array.from({ length: 12 }, (_, i) => monthlyAmount[i] ?? 0),
      monthlyOwnShareCents: Array.from({ length: 12 }, (_, i) => monthlyOwn[i] ?? 0),
    };
  }

  const PAYERS: KoPayerShare[] = [
    { memberId: 2, name: 'Ante', amountCents: 6000, expenseCount: 3, monthlyAmountCents: [4000, 2000] },
    { memberId: 1, name: 'Fabi', amountCents: 4000, expenseCount: 2, monthlyAmountCents: [1000, 3000] },
  ];
  const KO_ROWS = [
    koRow('Wocheneinkauf', 7000, 3500, [3500, 3500], [1750, 1750]),
    koRow(null, 3000, 1500, [1500, 1500], [750, 750]),
  ];

  it('reads as who fronted it on the left and what for on the right', () => {
    const flow = buildKoFlow({ payers: PAYERS, rows: KO_ROWS, labels: KO_LABELS });

    expect(flow.sources.map((n) => n.label)).toEqual(['Ante', 'Fabi']);
    expect(flow.targets.map((n) => n.label)).toEqual(['Wocheneinkauf', 'Ohne Kategorie']);
    expect(flow.inflowCents).toBe(10000);
    expect(flow.outflowCents).toBe(10000);
    // A household ledger has no balance, and must not appear to have one.
    expect(flow.saldoCents).toBe(0);
    expect(flow.targets.some((n) => n.key === 'surplus')).toBe(false);
  });

  it('splits the same total into the shares instead, on request', () => {
    const flow = buildKoFlow({
      payers: PAYERS,
      rows: KO_ROWS,
      detail: 'shares',
      labels: KO_LABELS,
    });

    expect(flow.targets.map((n) => [n.label, n.amountCents])).toEqual([
      ['Mein Anteil', 5000],
      ['Anteil der anderen', 5000],
    ]);
    // ...and it is still the same total as the payer side.
    expect(flow.outflowCents).toBe(flow.inflowCents);
  });

  it('follows one month through both columns', () => {
    const flow = buildKoFlow({ payers: PAYERS, rows: KO_ROWS, month: 2, labels: KO_LABELS });

    expect(flow.sources.map((n) => [n.label, n.amountCents])).toEqual([
      ['Fabi', 3000],
      ['Ante', 2000],
    ]);
    expect(flow.totalCents).toBe(5000);
    expect(flow.outflowCents).toBe(5000);
  });
});
