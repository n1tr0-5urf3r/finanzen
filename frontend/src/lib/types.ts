export interface ApiErrorBody {
  code?: string;
  message?: string;
  details?: unknown;
}

export type BookingKind = 'income' | 'expense' | 'transfer';
export type CategorySource = 'unresolved' | 'rule' | 'manual' | 'imported';
export type CategoryTypeCode = 'einkommen' | 'fixkosten' | 'variabel' | 'sparen' | 'sonstiges';

export interface User {
  id: string;
  username: string;
  displayName: string;
  isAdmin: boolean;
}

export interface SetupStatus {
  setupRequired: boolean;
  provider: string;
  registrationOpen: boolean;
}

export interface Booking {
  id: string;
  year: number;
  month: number;
  /** Server-rendered German month name; data, not chrome. */
  monthName: string;
  bookedOn: string | null;
  kind: BookingKind;
  /** Always positive; direction lives in `kind`. */
  amountCents: number;
  /** expense − income, so a category that earned money is negative. */
  netCents: number;
  comment: string;
  taxRelevant: boolean;
  categoryId: string | null;
  categoryName: string | null;
  categoryType: string | null;
  categorySource: CategorySource;
  shared: boolean;
  externalSource: string | null;
  externalId: string | null;
  hasReceipt: boolean;
  status: string;
  origin: string;
}

/** One year of whatever a search matched — the point of searching across years. */
export interface SearchYearSummary {
  year: number;
  bookingCount: number;
  incomeCents: number;
  expenseCents: number;
  /** Stored convention: expenses minus income, so a year that earned is negative. */
  netCents: number;
}

/** A distinct spelling the search matched, folded case-insensitively. */
export interface SearchComment {
  comment: string;
  bookingCount: number;
  netCents: number;
  categoryName: string | null;
}

export interface SearchResult {
  query: string;
  items: Booking[];
  total: number;
  page: number;
  pageSize: number;
  sumIncomeCents: number;
  sumExpenseCents: number;
  sumNetCents: number;
  byYear: SearchYearSummary[];
  comments: SearchComment[];
}

export interface BookingPage {
  items: Booking[];
  total: number;
  page: number;
  pageSize: number;
  sumIncomeCents: number;
  sumExpenseCents: number;
  sumNetCents: number;
  uncategorizedCount: number;
}

export interface Category {
  id: string;
  name: string;
  typeCode: CategoryTypeCode;
  typeLabel: string;
  sortOrder: number;
  archived: boolean;
  bookingCount: number | null;
  netCents: number | null;
}

export interface Rule {
  id: string;
  comment: string;
  normalizedComment: string;
  categoryId: string | null;
  categoryName: string | null;
  kindOverride: BookingKind | null;
  source: string;
  matchCount: number;
}

export interface CategoryTypeSummary {
  typeCode: CategoryTypeCode;
  label: string;
  netCents: number;
  bookingCount: number;
}

export interface CategoryAnalysisRow {
  categoryId: string | null;
  categoryName: string;
  categoryType: string | null;
  incomeCents: number;
  expenseCents: number;
  netCents: number;
  netIsNegative: boolean;
  shareOfTotal: number;
  averagePerMonthCents: number;
  bookingCount: number;
  monthlyNetCents: number[];
}

export interface Dashboard {
  year: number;
  incomeCents: number;
  expenseCents: number;
  balanceCents: number;
  openingBalanceCents: number;
  closingBalanceCents: number;
  carryoverGapCents: number | null;
  averageExpensePerMonthCents: number;
  fixedCostsPerMonthCents: number;
  monthsWithData: number;
  savingsRateNaive: number;
  savingsRateConsumption: number;
  savingsAmountCents: number;
  /** What was actually paid into the savings categories — the Sparrate. */
  savingsDepositCents: number;
  savingsDepositPerMonthCents: number;
  savingsDepositRate: number;
  bookingCount: number;
  taxRelevantCount: number;
  uncategorizedCount: number;
  uncategorizedNetCents: number;
  byType: CategoryTypeSummary[];
  topCategories: CategoryAnalysisRow[];
}

/**
 * One category, this year against last.
 *
 * Every figure is in the stored expense-positive convention, so a positive
 * `deltaCents` means it cost MORE than last year. The display flips that sign like
 * everywhere else: `<FlowMoney netCents={deltaCents}>` reads as the direction money
 * moved, which is the same for a cost that fell and an income that rose.
 */
export interface CompareRow {
  categoryId: string | null;
  categoryName: string;
  categoryType: string | null;
  netCents: number;
  previousNetCents: number;
  deltaCents: number;
  /** Null when there was nothing last year to be a share of. */
  deltaRatio: number | null;
  comparableNetCents: number;
  comparablePreviousNetCents: number;
  comparableDeltaCents: number;
  comparableDeltaRatio: number | null;
  monthlyNetCents: number[];
  previousMonthlyNetCents: number[];
  bookingCount: number;
  previousBookingCount: number;
  isNew: boolean;
  isGone: boolean;
}

export interface CompareTypeRow {
  typeCode: string;
  label: string;
  netCents: number;
  previousNetCents: number;
  deltaCents: number;
  deltaRatio: number | null;
  comparableNetCents: number;
  comparablePreviousNetCents: number;
  comparableDeltaCents: number;
}

export interface CompareTotals {
  year: number;
  incomeCents: number;
  expenseCents: number;
  saldoCents: number;
  bookingCount: number;
  monthsWithData: number;
  /** The last month of the year that holds a booking; null for an empty year. */
  lastMonthWithData: number | null;
  comparableIncomeCents: number;
  comparableExpenseCents: number;
  comparableSaldoCents: number;
}

export interface YearComparison {
  year: number;
  previousYear: number;
  current: CompareTotals;
  previous: CompareTotals;
  /** Months 1..12 that BOTH years carry bookings in. */
  comparableMonths: number[];
  /** True when both years cover the same months, so the raw figures are fair. */
  fullyComparable: boolean;
  rows: CompareRow[];
  byType: CompareTypeRow[];
  previousYearHasData: boolean;
}

export interface TrailingMonth {
  year: number;
  month: number;
  monthName: string;
  incomeCents: number;
  expenseCents: number;
  saldoCents: number;
  netCents: number;
  bookingCount: number;
}

export interface TrailingCategory {
  categoryId: string | null;
  categoryName: string;
  categoryType: string | null;
  netCents: number;
  averagePerMonthCents: number;
  bookingCount: number;
  /** Twelve entries, oldest first, aligned with `months`. */
  monthlyNetCents: number[];
}

export interface TrailingWindow {
  year: number;
  month: number;
  fromYear: number;
  fromMonth: number;
  months: TrailingMonth[];
  incomeCents: number;
  expenseCents: number;
  saldoCents: number;
  bookingCount: number;
  monthsWithData: number;
  rows: TrailingCategory[];
}

export interface MonthlyRow {
  month: number;
  monthName: string;
  incomeCents: number;
  expenseCents: number;
  balanceCents: number;
  cumulativeCents: number | null;
  savingsRate: number | null;
  fixedCostsNetCents: number;
  variableCostsNetCents: number;
  savingsNetCents: number;
  otherNetCents: number;
  bookingCount: number;
  uncategorizedCount: number;
}

export interface MonthlyOverview {
  year: number;
  months: MonthlyRow[];
  total: MonthlyRow;
}

export interface CategoryAnalysis {
  year: number;
  rows: CategoryAnalysisRow[];
  totalNetCents: number;
  monthsWithData: number;
  uncategorizedCount: number;
  excludedTransferCount: number;
}

export interface Year {
  year: number;
  openingBalanceCents: number;
  openingSource: string;
  locked: boolean;
  bookingCount: number;
  incomeCents: number;
  expenseCents: number;
  balanceCents: number;
  closingBalanceCents: number;
  carryoverGapCents: number | null;
}

export interface CommentSummary {
  comment: string;
  count: number;
  categoryName: string | null;
  isUncategorized: boolean;
}

/**
 * A year/month pair. `periodOrd` is an internal encoding on the server; the wire
 * carries the two fields so nothing here has to reimplement the arithmetic.
 */
export interface Period {
  year: number;
  month: number;
}

/**
 * A known annual or quarterly lump, accrued monthly. A fund books nothing — it is
 * an expectation, measured against the ordinary bookings in its category.
 */
export interface SinkingFund {
  id: string;
  name: string;
  categoryId: string | null;
  categoryName: string | null;
  annualCents: number;
  /** 1..12, the month the bill actually arrives. */
  dueMonth: number;
  dueMonthName: string;
  note: string | null;
  active: boolean;
  sortOrder: number;
}

export interface FundStatus {
  fund: SinkingFund;
  /** What to set aside each month — an approximation, by construction. */
  monthlyAccrualCents: number;
  /** What should be aside by the asked-about month. Exact at twelve. */
  accruedByMonthCents: number;
  /** Stored sign: positive is cost. */
  spentCents: number;
  remainingCents: number;
  /** Accrued minus spent. Positive is a cushion, negative is catching up. */
  overUnderCents: number;
  duePassed: boolean;
}

export interface FundOverview {
  year: number;
  month: number;
  funds: FundStatus[];
  monthlyAccrualCents: number;
  accruedByMonthCents: number;
  spentCents: number;
  owedToTheFutureCents: number;
}

export interface FundSuggestion {
  categoryId: string;
  categoryName: string;
  annualCents: number;
  dueMonth: number;
  dueMonthName: string;
  monthsWithSpending: number;
  bookingCount: number;
  year: number;
}

export interface RecurringTemplate {
  id: string;
  name: string;
  comment: string;
  kind: BookingKind;
  amountCents: number;
  /** The amount varies, so materialising produces a draft to confirm. */
  amountIsEstimate: boolean;
  categoryId: string | null;
  /** The override's category, or the one the rule table gives the comment. */
  categoryName: string | null;
  categoryType: string | null;
  /** True when the name above comes from the rule table rather than an override. */
  categoryFromRule: boolean;
  taxRelevant: boolean;
  dayOfMonth: number | null;
  /** 1 monthly, 3 quarterly, 12 annual. */
  intervalMonths: number;
  anchor: Period;
  activeFrom: Period;
  activeTo: Period | null;
  active: boolean;
  sortOrder: number;
  /** Both null unless the request named a period. */
  dueInPeriod: boolean | null;
  bookedInPeriod: boolean | null;
  lastBooked: Period | null;
  bookingCount: number;
}

export interface MaterializedItem {
  templateId: string;
  templateName: string;
  comment: string;
  amountCents: number;
  kind: BookingKind;
  status: string;
  bookingId: string | null;
  skippedReason: string | null;
}

export interface MaterializeResult {
  year: number;
  month: number;
  monthName: string;
  created: number;
  skipped: number;
  drafts: number;
  dryRun: boolean;
  items: MaterializedItem[];
}

export interface Receipt {
  id: string;
  bookingId: string | null;
  filename: string;
  contentType: string;
  byteSize: number;
  sha256: string;
  uploadedAt: string;
}

export interface TaxEntry {
  bookingId: string;
  index: number;
  month: number;
  monthName: string;
  comment: string;
  categoryName: string | null;
  incomeCents: number;
  expenseCents: number;
  hasReceipt: boolean;
}

export interface TaxCategorySummary {
  categoryName: string;
  expenseCents: number;
  incomeCents: number;
  netCents: number;
  count: number;
}

export interface TaxReport {
  year: number;
  totalExpenseCents: number;
  totalIncomeCents: number;
  totalNetCents: number;
  bookingCount: number;
  receiptsPresent: number;
  entries: TaxEntry[];
  byCategory: TaxCategorySummary[];
}

/* ── KitchenOwl ───────────────────────────────────────────────────────────────
   A SEPARATE, PARALLEL LEDGER. None of these numbers is ever added to a booking
   figure, and every view showing both says which is which. `amountCents` is what
   the household spent; `ownShareCents` is the user's slice of it. */

export interface KoMember {
  memberId: number;
  name: string;
  username: string | null;
  /** Negative means the user owes the household. The only balance KitchenOwl has. */
  balanceCents: number;
  isMe: boolean;
  isOwner: boolean;
  isAdmin: boolean;
  fetchedAt: string;
}

/**
 * KitchenOwl's own taxonomy — seven household labels with no relationship to the
 * app's 32 categories and five types. Never auto-mapped, and deliberately not
 * styled like an app category.
 */
export interface KoCategory {
  categoryId: number;
  name: string;
  colorArgb: number | null;
  budgetCents: number | null;
  fetchedAt: string;
}

export interface KoMetadata {
  members: KoMember[];
  categories: KoCategory[];
  fetchedAt: string | null;
  /** Served anyway, with a warning: the push dialogue must open during an outage. */
  stale: boolean;
  warning: string | null;
}

export interface KoShare {
  memberId: number;
  name: string | null;
  /** An integer WEIGHT, not a percentage. */
  factor: number;
  shareCents: number;
}

export interface KoExpense {
  id: string;
  externalId: number;
  name: string;
  description: string | null;
  date: string;
  /** The full shared amount — what left somebody's account. */
  amountCents: number;
  /** The user's slice. Never summed with the amount or with a booking. */
  ownShareCents: number;
  paidById: number | null;
  paidByName: string | null;
  paidFor: KoShare[];
  koCategoryId: number | null;
  koCategoryName: string | null;
  excludeFromStatistics: boolean;
  archivedAt: string | null;
  linkedBookingId: string | null;
  linkedBookingComment: string | null;
  linkedBookingAmountCents: number | null;
  updatedAt: string;
}

export interface KoExpensePage {
  items: KoExpense[];
  total: number;
  page: number;
  pageSize: number;
  sumAmountCents: number;
  sumOwnShareCents: number;
  linkedCount: number;
}

/**
 * The household ledger's own analysis. Two figures everywhere: `amountCents` is
 * what the household spent, `ownShareCents` is the user's slice. They are never
 * added — not to each other and not to anything from the personal ledger.
 */
export interface KoCategoryAnalysisRow {
  koCategoryId: number | null;
  koCategoryName: string | null;
  amountCents: number;
  ownShareCents: number;
  expenseCount: number;
  shareOfTotal: number;
  averagePerMonthCents: number;
  averageOwnSharePerMonthCents: number;
  monthlyAmountCents: number[];
  monthlyOwnShareCents: number[];
}

export interface KoPayerShare {
  memberId: number | null;
  name: string;
  amountCents: number;
  expenseCount: number;
}

export interface KoCategoryAnalysis {
  year: number;
  rows: KoCategoryAnalysisRow[];
  totalAmountCents: number;
  totalOwnShareCents: number;
  expenseCount: number;
  monthsWithData: number;
  uncategorizedCount: number;
  /** What KitchenOwl itself keeps out of its statistics, so this does too. */
  excludedCount: number;
  paidBy: KoPayerShare[];
  years: number[];
}

export interface KoSeriesMonth {
  month: number;
  monthName: string;
  amountCents: number;
  ownShareCents: number;
  expenseCount: number;
}

export interface KoMonthlySeries {
  year: number;
  mode: 'category' | 'name' | 'uncategorized';
  subject: string;
  koCategoryId: number | null;
  months: KoSeriesMonth[];
  amountCents: number;
  ownShareCents: number;
  expenseCount: number;
  averagePerActiveMonthCents: number;
  averageOwnSharePerActiveMonthCents: number;
  monthsWithData: number;
}

export interface KoSeriesSubject {
  name: string;
  expenseCount: number;
  amountCents: number;
  ownShareCents: number;
  koCategoryName: string | null;
}

/** One KitchenOwl category, this year against last. Both figures, both deltas. */
export interface KoCompareRow {
  koCategoryId: number | null;
  koCategoryName: string | null;
  amountCents: number;
  ownShareCents: number;
  expenseCount: number;
  previousAmountCents: number;
  previousOwnShareCents: number;
  previousExpenseCount: number;
  deltaAmountCents: number;
  deltaOwnShareCents: number;
  /** Null when there was no previous year to be a share of. */
  deltaRatio: number | null;
  comparableAmountCents: number;
  comparableOwnShareCents: number;
  comparablePreviousAmountCents: number;
  comparablePreviousOwnShareCents: number;
  comparableDeltaAmountCents: number;
  comparableDeltaOwnShareCents: number;
  comparableDeltaRatio: number | null;
  monthlyAmountCents: number[];
  monthlyOwnShareCents: number[];
  previousMonthlyAmountCents: number[];
  previousMonthlyOwnShareCents: number[];
  isNew: boolean;
  isGone: boolean;
}

export interface KoCompareTotals {
  year: number;
  amountCents: number;
  ownShareCents: number;
  expenseCount: number;
  monthsWithData: number;
  lastMonthWithData: number | null;
  comparableAmountCents: number;
  comparableOwnShareCents: number;
  comparableExpenseCount: number;
  excludedCount: number;
}

export interface KoComparePayer {
  memberId: number | null;
  name: string;
  amountCents: number;
  previousAmountCents: number;
  deltaCents: number;
  expenseCount: number;
  previousExpenseCount: number;
}

export interface KoYearComparison {
  year: number;
  previousYear: number;
  current: KoCompareTotals;
  previous: KoCompareTotals;
  comparableMonths: number[];
  fullyComparable: boolean;
  previousYearHasData: boolean;
  rows: KoCompareRow[];
  paidBy: KoComparePayer[];
  years: number[];
}

export interface KoTrailingMonth {
  year: number;
  month: number;
  monthName: string;
  amountCents: number;
  ownShareCents: number;
  expenseCount: number;
}

export interface KoTrailingCategory {
  koCategoryId: number | null;
  koCategoryName: string | null;
  amountCents: number;
  ownShareCents: number;
  expenseCount: number;
  averagePerMonthCents: number;
  averageOwnSharePerMonthCents: number;
  monthlyAmountCents: number[];
  monthlyOwnShareCents: number[];
}

export interface KoTrailingWindow {
  year: number;
  month: number;
  fromYear: number;
  fromMonth: number;
  amountCents: number;
  ownShareCents: number;
  expenseCount: number;
  monthsWithData: number;
  months: KoTrailingMonth[];
  rows: KoTrailingCategory[];
}

export interface KoMatchCandidate {
  bookingId: string;
  comment: string;
  amountCents: number;
  year: number;
  month: number;
  monthName: string;
  score: number;
  basis: 'fullAmount' | 'ownShare' | 'fullAmountNear' | 'ownShareNear';
}

export type KoDraftStatus =
  | 'open'
  | 'likely_duplicate'
  | 'possible_duplicate'
  | 'ignored_by_default'
  | 'confirmed'
  | 'discarded';

export interface KoDraft {
  id: string;
  status: KoDraftStatus;
  expense: KoExpense;
  candidates: KoMatchCandidate[];
  /** `link` or `none`. Never `create`: a pull writes no booking. */
  suggestedAction: 'link' | 'none';
  createdAt: string;
}

export interface KoDraftPage {
  items: KoDraft[];
  total: number;
  openCount: number;
  likelyCount: number;
}

export interface KoSyncRun {
  id: string;
  kind: string;
  status: 'running' | 'success' | 'partial' | 'failed';
  startedAt: string;
  finishedAt: string | null;
  createdCount: number;
  updatedCount: number;
  archivedCount: number;
  failedCount: number;
  error: string | null;
}

export interface KoStatus {
  configured: boolean;
  enabled: boolean;
  reachable: boolean | null;
  running: boolean;
  householdId: number | null;
  householdName: string | null;
  lastExpenseRun: KoSyncRun | null;
  lastMetadataRun: KoSyncRun | null;
  nextRunAt: string | null;
  /** 0 means only "jetzt synchronisieren" moves anything. */
  pollSeconds: number;
  mirroredCount: number;
  archivedCount: number;
  linkedCount: number;
  openDraftCount: number;
  likelyDuplicateCount: number;
  pendingPushCount: number;
  failedPushCount: number;
  metadataFetchedAt: string | null;
  metadataStale: boolean;
  lastError: string | null;
}

export interface KoSummary {
  configured: boolean;
  enabled: boolean;
  householdName: string | null;
  members: KoMember[];
  myBalanceCents: number | null;
  recent: KoExpense[];
  month: Period;
  monthAmountCents: number;
  monthOwnShareCents: number;
  monthCount: number;
  lastSyncedAt: string | null;
  stale: boolean;
  warning: string | null;
}

/**
 * Settling up with the household.
 *
 * `balanceCents` is KitchenOwl's own signed figure, unflipped: negative means the
 * user owes the household. `direction` is decided on the server beside it, so the
 * sentence and the sign cannot drift apart.
 */
/**
 * One untagged name in the filing queue.
 *
 * Grouped by name because that is the shape of the decision: sixteen Kaufland
 * receipts are one judgement about Kaufland, not sixteen.
 */
export interface KoUntaggedGroup {
  name: string;
  /** The folded key the group was built on; what `apply` matches against. */
  matchKey: string;
  expenseCount: number;
  /** Household total and the user's share. Two figures, never added. */
  amountCents: number;
  ownShareCents: number;
  firstDate: string;
  lastDate: string;
  suggestion: KoTagSuggestion | null;
}

/** A suggested category and, always, the evidence behind it. */
export interface KoTagSuggestion {
  koCategoryId: number;
  koCategoryName: string;
  /** `override` (a standing correction) · `precedent` (the mirror's own history) · `rule`. */
  source: 'override' | 'precedent' | 'rule';
  /** How many times this name already carries that category. `precedent` only. */
  timesSeen: number;
}

export interface KoTagFailure {
  externalId: number;
  name: string;
  error: string;
}

export interface KoTagResult {
  koCategoryId: number;
  koCategoryName: string;
  requested: number;
  tagged: number;
  skipped: number;
  failed: number;
  failures: KoTagFailure[];
}

export interface KoSettlement {
  balanceCents: number | null;
  direction: 'i_owe' | 'household_owes_me' | 'settled' | 'unknown';
  /**
   * What settling books: an expense when the user owes, an income when the
   * household does. Never a transfer — the money goes to another person, so it
   * moves the balance.
   */
  kind: BookingKind | null;
  categoryId: string | null;
  categoryName: string | null;
  /** True when `Haushaltsausgleich` is missing, so the rule table decides instead. */
  categoryIsFallback: boolean;
  /** Always positive: what would change hands. */
  amountCents: number;
  period: Period;
  suggestedComment: string;
  alreadySettled: boolean;
  /** A personal-ledger booking. Never added to any figure above it. */
  booking: Booking | null;
  settledBalanceCents: number | null;
  settledAt: string | null;
}

export type KoPushState =
  | 'queued'
  | 'sending'
  | 'pushed'
  | 'failed'
  | 'abandoned'
  | 'retracted';

export interface KoPushIntent {
  bookingId: string;
  state: KoPushState;
  bookingComment: string | null;
  amountCents: number;
  date: string;
  name: string;
  marker: string;
  attempts: number;
  lastError: string | null;
  externalId: number | null;
  nextAttemptAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface KoSyncResult {
  started: boolean;
  expenses: KoSyncRun | null;
  metadata: KoSyncRun | null;
  error: string | null;
}

/* ── Import ───────────────────────────────────────────────────────────────────
   The dry-run preview, the commit result, and the review queue. Nothing here has
   touched the ledger until `/imports/{id}/commit` returns. */

export interface ImportCounts {
  dataRows: number;
  newBookings: number;
  duplicates: number;
  income: number;
  expense: number;
  transfer: number;
  categorized: number;
  uncategorized: number;
  taxRelevant: number;
  openReviewItems: number;
}

export interface ImportYearTotals {
  year: number;
  months: number;
  incomeCents: number;
  expenseCents: number;
  balanceCents: number;
  bookingCount: number;
}

/**
 * One inferred month block of the legacy sheet. `deltaCents` is non-null where the
 * block's own saldo marker disagrees with its rows — recorded, never adjusted.
 */
export interface MonthBlockInfo {
  index: number;
  year: number;
  month: number;
  rowCount: number;
  labelSource: string;
  rawLabel: string | null;
  markerCents: number | null;
  computedCents: number;
  deltaCents: number | null;
}

export interface ImportPreview {
  id: string;
  fileName: string;
  sheet: string;
  source: string;
  /** `preview` until committed, then `applied`. */
  status: string;
  counts: ImportCounts;
  yearTotals: ImportYearTotals[];
  blocks: MonthBlockInfo[];
  /** The legacy sheet's own month-end markers; the authoritative carry-over. */
  markerTotalCents: number | null;
  rowTotalCents: number | null;
  warnings: string[];
}

export interface ImportBatchSummary {
  id: string;
  fileName: string;
  sheet: string | null;
  source: string;
  status: string;
  rowCount: number;
  createdAt: string;
  appliedAt: string | null;
}

export interface CommitResult {
  inserted: number;
  skipped: number;
  yearsTouched: number[];
  uncategorizedRemaining: number;
}

/**
 * A suggested category for an unknown comment.
 *
 * `isSuggestion` is the whole distinction: containment matches are precise enough
 * to preselect, edit-distance hints are not and are the source of every
 * confidently wrong answer (`trinken` → `tanken`, `Malve` → `Mafit`). The weak
 * ones arrive in `weakHints` and must stay greyed and unselected.
 */
export interface ReviewSuggestion {
  categoryId: string;
  categoryName: string;
  matchedRule: string;
  confidence: number;
  tier: string;
  isSuggestion: boolean;
}

/** Keyed by DISTINCT comment, sorted by frequency: ~240 decisions, not ~362 rows. */
export interface ReviewItem {
  id: string;
  comment: string;
  normalizedComment: string;
  rowCount: number;
  expenseCents: number;
  incomeCents: number;
  suggestions: ReviewSuggestion[];
  weakHints: ReviewSuggestion[];
  /** Two rules claim this comment equally — a human has to choose. */
  ambiguous: boolean;
  suggestedKind: BookingKind | null;
  status: string;
}

export interface Resolution {
  itemId: string;
  categoryId?: string | null;
  /** Default true on the wire, and default checked in the UI. */
  createRule?: boolean;
  skip?: boolean;
}

export interface ResolveResult {
  resolved: number;
  skipped: number;
  rulesCreated: number;
  remainingOpen: number;
}

export interface ApplyRulesResult {
  examined: number;
  recategorized: number;
  stillUncategorized: number;
  dryRun: boolean;
}

export interface SeriesMonth {
  month: number;
  monthName: string;
  incomeCents: number;
  expenseCents: number;
  netCents: number;
  bookingCount: number;
}

export interface MonthlySeries {
  year: number;
  mode: 'category' | 'comment';
  /** The category name or the comment, verbatim — data, never translated. */
  subject: string;
  categoryId: string | null;
  months: SeriesMonth[];
  incomeCents: number;
  expenseCents: number;
  netCents: number;
  /** Divided by the months carrying a booking for this subject, not by twelve. */
  averagePerActiveMonthCents: number;
  bookingCount: number;
  monthsWithData: number;
}

export interface SeriesSubject {
  comment: string;
  bookingCount: number;
  netCents: number;
  categoryName: string | null;
}

/**
 * The rest of the year, projected from the recurring templates and the median of
 * the last six months. A projection is never an actual: every month says which it
 * is, and the two are summed separately.
 */
export interface ForecastMonth {
  month: number;
  monthName: string;
  /** Expense-positive, like every stored net. */
  netCents: number;
  fixedCents: number;
  variableCents: number;
  spreadCents: number;
  isProjected: boolean;
  bookingCount: number;
  closingBalanceCents: number;
}

export interface ForecastBasisRow {
  categoryName: string;
  categoryType: string | null;
  monthsOfHistory: number;
  medianCents: number;
  projectedTotalCents: number;
  /** `template`, `median` or `mixed`. */
  source: string;
}

export interface Forecast {
  year: number;
  openingBalanceCents: number;
  actualThroughMonth: number | null;
  projectedFromMonth: number | null;
  months: ForecastMonth[];
  actualBalanceCents: number;
  projectedBalanceCents: number;
  projectedClosingBalanceCents: number;
  projectedClosingLowCents: number;
  projectedClosingHighCents: number;
  dueTemplateCount: number;
  historyMonths: number;
  method: string;
  rows: ForecastBasisRow[];
}

/** A category a long way from its own median. An empty list is the normal case. */
export interface Anomaly {
  categoryName: string;
  categoryType: string | null;
  currentCents: number;
  medianCents: number;
  deltaCents: number;
  ratio: number;
  direction: 'above' | 'below';
  monthsOfHistory: number;
}

export interface AnomalyReport {
  year: number;
  month: number;
  monthName: string;
  items: Anomaly[];
  comparedMonths: number;
  minRatio: number;
  minDeltaCents: number;
}
