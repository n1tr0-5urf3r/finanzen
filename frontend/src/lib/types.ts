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

export interface RecurringTemplate {
  id: string;
  name: string;
  comment: string;
  kind: BookingKind;
  amountCents: number;
  /** The amount varies, so materialising produces a draft to confirm. */
  amountIsEstimate: boolean;
  categoryId: string | null;
  categoryName: string | null;
  categoryType: string | null;
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
