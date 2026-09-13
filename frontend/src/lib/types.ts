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
