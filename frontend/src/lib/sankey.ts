import type {
  CategoryAnalysisRow,
  CategoryTypeCode,
  CategoryTypeSummary,
  KoCategoryAnalysisRow,
  KoPayerShare,
} from './types';

/**
 * Where the money came from and where it went, as one balanced flow.
 *
 * The arithmetic is the netting rule and nothing else: a category's stored net is
 * positive when it cost money and negative when it brought money in, so the sign
 * — not the category's type — decides which side of the diagram a node lands on.
 * That is what makes the picture honest in June, where `Dienstreisen` nets −300,00
 * because of a reimbursement: it shows up as an inflow that month, because that is
 * what it was.
 *
 * Consequence worth saying out loud in the UI: the inflow total here is NETTED
 * income (2026: 28.800,00 €), not the dashboard's gross `Einnahmen` (36.000,00 €).
 * The difference is the flatmate's rent share, the refunds and the business-trip
 * reimbursements, which are already deducted inside their own category. Adding
 * them on the left AND leaving them inside the category on the right would count
 * them twice and inflate both sides of the diagram.
 *
 * The two sides balance by construction:
 *
 *     Σ inflow + deficit  ==  Σ outflow + surplus
 *
 * with exactly one of `deficit`/`surplus` non-zero — that is the period's saldo,
 * the same figure the dashboard shows.
 */

export type FlowDetail = 'types' | 'categories';

export type FlowTone = 'income' | 'credit' | 'surplus' | CategoryTypeCode | 'none';

export interface FlowNode {
  /** Stable across renders and periods, so React keys and selection survive. */
  key: string;
  label: string;
  /** Always positive: the magnitude of what passes through the node. */
  amountCents: number;
  tone: FlowTone;
  column: 'source' | 'hub' | 'target';
  /** Present for a node that stands for one category. */
  categoryId?: string | null;
  /** Present for a type node; the handle the drill-down is keyed on. */
  typeCode?: CategoryTypeCode;
  /** A type node with more than one category behind it can be broken out. */
  expandable?: boolean;
  /** True for a category node shown because its type is expanded. */
  nested?: boolean;
  /** A credit: an expense category that brought more in than it cost. */
  credit?: boolean;
}

export interface FlowLink {
  from: string;
  to: string;
  amountCents: number;
  tone: FlowTone;
}

export interface FlowModel {
  sources: FlowNode[];
  hub: FlowNode;
  targets: FlowNode[];
  links: FlowLink[];
  /** Money in, netted. */
  inflowCents: number;
  /** Money out, netted. */
  outflowCents: number;
  /** `inflow − outflow`: positive is what was left over. */
  saldoCents: number;
  /** What the hub passes through — the height the diagram is scaled to. */
  totalCents: number;
  /** How many expense categories turned out to be credits this period. */
  creditCount: number;
}

export interface FlowLabels {
  hub: string;
  surplus: string;
  deficit: string;
  noType: string;
}

/** The order the cost buckets are stacked in — cheapest to reason about when it
    never moves between periods. Income sits first because a category of type
    `einkommen` that somehow COST money belongs at the top of the outflows. */
const TYPE_ORDER: CategoryTypeCode[] = [
  'einkommen',
  'fixkosten',
  'variabel',
  'sparen',
  'sonstiges',
];

/** The label the backend puts on an analysis row → the type's code. Analysis rows
    carry the German label only, so the live type list is the lookup table and the
    literals are a fallback for a row whose type was deleted since. */
export function typeCodeIndex(types: CategoryTypeSummary[]): Map<string, CategoryTypeCode> {
  const index = new Map<string, CategoryTypeCode>([
    ['einkommen', 'einkommen'],
    ['fixkosten', 'fixkosten'],
    ['variabel', 'variabel'],
    ['variable kosten', 'variabel'],
    ['sparen', 'sparen'],
    ['sparen & anlage', 'sparen'],
    ['sonstiges', 'sonstiges'],
  ]);
  for (const type of types) index.set(type.label.toLowerCase(), type.typeCode);
  return index;
}

/** The net of one category in the selected period: the year, or one month of it. */
export function netInPeriod(row: CategoryAnalysisRow, month: number | null): number {
  if (month === null) return row.netCents;
  return row.monthlyNetCents[month - 1] ?? 0;
}

/** The months that have any activity at all, so the picker offers only those. */
export function monthsWithActivity(rows: CategoryAnalysisRow[]): number[] {
  const months: number[] = [];
  for (let m = 1; m <= 12; m += 1) {
    if (rows.some((row) => netInPeriod(row, m) !== 0)) months.push(m);
  }
  return months;
}

export function buildFlow({
  rows,
  types,
  month = null,
  detail = 'types',
  expanded = null,
  labels,
}: {
  rows: CategoryAnalysisRow[];
  types: CategoryTypeSummary[];
  month?: number | null;
  /**
   * How fine the outflow column is:
   *
   * - `types` — the five buckets. Answers "how much of it was fixed", which is
   *   the question the shape of the diagram is good at.
   * - `categories` — every category that cost something, still ordered and
   *   coloured by its type. Answers "what exactly", at the price of a long column
   *   of thin bands.
   *
   * The totals are identical either way; only the grouping changes.
   */
  detail?: FlowDetail;
  /** One type broken out inside the `types` view, or `null`. */
  expanded?: CategoryTypeCode | null;
  labels: FlowLabels;
}): FlowModel {
  const codeOf = typeCodeIndex(types);
  const labelOf = new Map<CategoryTypeCode, string>();
  for (const type of types) labelOf.set(type.typeCode, type.label);

  interface Entry {
    row: CategoryAnalysisRow;
    code: CategoryTypeCode | null;
    net: number;
  }

  const entries: Entry[] = rows
    .map((row) => ({
      row,
      code: row.categoryType ? (codeOf.get(row.categoryType.toLowerCase()) ?? null) : null,
      net: netInPeriod(row, month),
    }))
    .filter((e) => e.net !== 0);

  const sources: FlowNode[] = [];
  const targets: FlowNode[] = [];
  const links: FlowLink[] = [];

  // ── left: everything that brought money in ────────────────────────────────
  const inflows = entries.filter((e) => e.net < 0).sort((a, b) => a.net - b.net);
  let creditCount = 0;
  for (const entry of inflows) {
    // A negative net in an income category is income. A negative net anywhere
    // else is a refund or a shared cost coming back, and says so.
    const credit = entry.code !== 'einkommen';
    if (credit) creditCount += 1;
    const key = `src:${entry.row.categoryId ?? entry.row.categoryName}`;
    sources.push({
      key,
      label: entry.row.categoryName,
      amountCents: -entry.net,
      tone: credit ? 'credit' : 'income',
      column: 'source',
      categoryId: entry.row.categoryId,
      credit,
    });
    links.push({ from: key, to: 'hub', amountCents: -entry.net, tone: credit ? 'credit' : 'income' });
  }

  const inflowCents = sources.reduce((sum, n) => sum + n.amountCents, 0);

  // ── right: everything that cost money, grouped by type ────────────────────
  const outflows = entries.filter((e) => e.net > 0);
  const byType = new Map<CategoryTypeCode | 'none', Entry[]>();
  for (const entry of outflows) {
    const key = entry.code ?? 'none';
    const bucket = byType.get(key);
    if (bucket) bucket.push(entry);
    else byType.set(key, [entry]);
  }

  const outflowCents = outflows.reduce((sum, e) => sum + e.net, 0);
  const saldoCents = inflowCents - outflowCents;

  const orderedTypes: (CategoryTypeCode | 'none')[] = [
    ...TYPE_ORDER.filter((code) => byType.has(code)),
    ...(byType.has('none') ? (['none'] as const) : []),
  ];

  for (const code of orderedTypes) {
    const bucket = (byType.get(code) ?? []).sort((a, b) => b.net - a.net);
    const total = bucket.reduce((sum, e) => sum + e.net, 0);
    const tone: FlowTone = code === 'none' ? 'none' : code;
    const typeKey = `type:${code}`;

    if (detail === 'categories' || (expanded !== null && code === expanded)) {
      // Broken out: the hub feeds the categories directly, so the column still
      // sums to the same figure and no flow is drawn twice.
      for (const entry of bucket) {
        const key = `cat:${entry.row.categoryId ?? entry.row.categoryName}`;
        targets.push({
          key,
          label: entry.row.categoryName,
          amountCents: entry.net,
          tone,
          column: 'target',
          categoryId: entry.row.categoryId,
          typeCode: code === 'none' ? undefined : code,
          nested: true,
        });
        links.push({ from: 'hub', to: key, amountCents: entry.net, tone });
      }
      continue;
    }

    targets.push({
      key: typeKey,
      label: code === 'none' ? labels.noType : (labelOf.get(code as CategoryTypeCode) ?? code),
      amountCents: total,
      tone,
      column: 'target',
      typeCode: code === 'none' ? undefined : code,
      expandable: code !== 'none' && bucket.length > 1,
    });
    links.push({ from: 'hub', to: typeKey, amountCents: total, tone });
  }

  // ── the saldo, on whichever side makes both sides add up ──────────────────
  if (saldoCents > 0) {
    targets.push({
      key: 'surplus',
      label: labels.surplus,
      amountCents: saldoCents,
      tone: 'surplus',
      column: 'target',
    });
    links.push({ from: 'hub', to: 'surplus', amountCents: saldoCents, tone: 'surplus' });
  } else if (saldoCents < 0) {
    // A deficit month is funded from the balance, not from income. Drawing it as
    // a source is the only way the diagram can stay balanced without hiding it.
    sources.unshift({
      key: 'deficit',
      label: labels.deficit,
      amountCents: -saldoCents,
      tone: 'credit',
      column: 'source',
    });
    links.unshift({ from: 'deficit', to: 'hub', amountCents: -saldoCents, tone: 'credit' });
  }

  const totalCents = Math.max(inflowCents, outflowCents);

  return {
    sources,
    hub: {
      key: 'hub',
      label: labels.hub,
      amountCents: totalCents,
      tone: 'income',
      column: 'hub',
    },
    targets,
    links,
    inflowCents,
    outflowCents,
    saldoCents,
    totalCents,
    creditCount,
  };
}

// ────────────────────────────────────────────────────────────── the household

/**
 * The same diagram for the household ledger, which is a different shape of
 * question.
 *
 * There is no income here and nothing nets: KitchenOwl records what the household
 * spent, full stop. What it does have, and the personal ledger cannot, is a
 * SECOND fact about the same euro — who fronted it — so the left column is the
 * payers rather than sources of income. Both sides are the same total seen twice:
 * who paid it, and what it was for.
 *
 * The right column can instead show the split, which is the household's other
 * question: of everything spent, how much is mine to carry. The two never mix —
 * `categories` and `shares` are two decompositions of one total, never summed,
 * which is the rule that governs every figure on the household screens.
 */
export type KoFlowDetail = 'categories' | 'shares';

export interface KoFlowLabels {
  hub: string;
  mine: string;
  others: string;
  noCategory: string;
  unknownPayer: string;
}

export function buildKoFlow({
  payers,
  rows,
  month = null,
  detail = 'categories',
  labels,
}: {
  payers: KoPayerShare[];
  rows: KoCategoryAnalysisRow[];
  month?: number | null;
  detail?: KoFlowDetail;
  labels: KoFlowLabels;
}): FlowModel {
  const amountOf = (monthly: number[], total: number) =>
    month === null ? total : (monthly[month - 1] ?? 0);

  const sources: FlowNode[] = [];
  const targets: FlowNode[] = [];
  const links: FlowLink[] = [];

  // Household colours carry no income/expense meaning — nothing here is income —
  // so the payers are told apart by the two neutral node tones instead.
  const payerTones: FlowTone[] = ['income', 'credit', 'sonstiges', 'none'];

  payers
    .map((payer) => ({
      payer,
      amountCents: amountOf(payer.monthlyAmountCents ?? [], payer.amountCents),
    }))
    .filter((p) => p.amountCents > 0)
    .sort((a, b) => b.amountCents - a.amountCents)
    .forEach(({ payer, amountCents }, i) => {
      const key = `payer:${payer.memberId ?? 'unknown'}`;
      const tone = payerTones[i % payerTones.length] as FlowTone;
      sources.push({
        key,
        label: payer.name || labels.unknownPayer,
        amountCents,
        tone,
        column: 'source',
      });
      links.push({ from: key, to: 'hub', amountCents, tone });
    });

  const inflowCents = sources.reduce((sum, n) => sum + n.amountCents, 0);

  if (detail === 'shares') {
    const mine = rows.reduce(
      (sum, r) => sum + amountOf(r.monthlyOwnShareCents ?? [], r.ownShareCents),
      0,
    );
    const total = rows.reduce((sum, r) => sum + amountOf(r.monthlyAmountCents ?? [], r.amountCents), 0);
    for (const [key, label, amountCents, tone] of [
      ['share:mine', labels.mine, mine, 'variabel'],
      ['share:others', labels.others, total - mine, 'sonstiges'],
    ] as [string, string, number, FlowTone][]) {
      if (amountCents <= 0) continue;
      targets.push({ key, label, amountCents, tone, column: 'target' });
      links.push({ from: 'hub', to: key, amountCents, tone });
    }
  } else {
    // One band per KitchenOwl category. They have no types, so the colour only
    // has to tell them apart; the uncategorised one is deliberately the grey.
    const tones: FlowTone[] = ['fixkosten', 'variabel', 'sparen', 'sonstiges', 'credit', 'income'];
    rows
      .map((row) => ({
        row,
        amountCents: amountOf(row.monthlyAmountCents ?? [], row.amountCents),
      }))
      .filter((r) => r.amountCents > 0)
      .sort((a, b) => b.amountCents - a.amountCents)
      .forEach(({ row, amountCents }, i) => {
        const key = `koCat:${row.koCategoryId ?? 'none'}`;
        const tone: FlowTone = row.koCategoryName ? (tones[i % tones.length] as FlowTone) : 'none';
        targets.push({
          key,
          label: row.koCategoryName ?? labels.noCategory,
          amountCents,
          tone,
          column: 'target',
        });
        links.push({ from: 'hub', to: key, amountCents, tone });
      });
  }

  const outflowCents = targets.reduce((sum, n) => sum + n.amountCents, 0);

  return {
    sources,
    hub: {
      key: 'hub',
      label: labels.hub,
      amountCents: Math.max(inflowCents, outflowCents),
      tone: 'income',
      column: 'hub',
    },
    targets,
    links,
    inflowCents,
    outflowCents,
    // Both sides are the same total by construction; a household ledger has no
    // saldo, and pretending it has one would invite comparing it with the
    // personal balance.
    saldoCents: 0,
    totalCents: Math.max(inflowCents, outflowCents),
    creditCount: 0,
  };
}
