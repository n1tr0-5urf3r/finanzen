//! Every wire type in one file, camelCase on the wire, `ToSchema` for the OpenAPI
//! document. No ORM entities: rows are read positionally and mapped by hand.
//!
//! Money is **integer cents** on the wire, and every such field name ends in `Cents`.
//! de-DE rendering happens in the frontend and in the CSV/PDF exports, never here.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

// ------------------------------------------------------------------- enums

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum BookingKind {
    Income,
    Expense,
    Transfer,
}

impl BookingKind {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Income => "income",
            Self::Expense => "expense",
            Self::Transfer => "transfer",
        }
    }
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "income" => Some(Self::Income),
            "expense" => Some(Self::Expense),
            "transfer" => Some(Self::Transfer),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum CategorySource {
    Unresolved,
    Rule,
    Manual,
    Imported,
}

impl CategorySource {
    pub fn parse(raw: &str) -> Self {
        match raw {
            "rule" => Self::Rule,
            "manual" => Self::Manual,
            "imported" => Self::Imported,
            _ => Self::Unresolved,
        }
    }
}

// -------------------------------------------------------------------- auth

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub is_admin: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    pub setup_required: bool,
    /// Which provider the login page should render for. Lets the frontend stay
    /// ignorant of whether local auth or OIDC is configured.
    pub provider: String,
    pub registration_open: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetupRequest {
    pub username: String,
    pub display_name: String,
    pub password: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub username: String,
    pub display_name: String,
    pub password: String,
    #[serde(default)]
    pub is_admin: bool,
}

// ---------------------------------------------------------------- bookings

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Booking {
    pub id: Uuid,
    pub year: i32,
    pub month: u8,
    /// Server-rendered German month name. Always derivable, but including it removes
    /// a client-side lookup from every list render.
    pub month_name: String,
    /// Absent for imported history, which genuinely has no day.
    pub booked_on: Option<NaiveDate>,
    pub kind: BookingKind,
    /// Always positive; the direction is carried by `kind`.
    pub amount_cents: i64,
    /// `expense - income`, so a category that earned money nets negative.
    pub net_cents: i64,
    pub comment: String,
    pub tax_relevant: bool,
    /// `null` means no rule matched and no override was set. Never silently folded
    /// into the real `Sonstiges` category.
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    pub category_type: Option<String>,
    pub category_source: CategorySource,
    pub shared: bool,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub has_receipt: bool,
    pub status: String,
    pub origin: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookingInput {
    pub year: i32,
    pub month: u8,
    pub booked_on: Option<NaiveDate>,
    pub kind: BookingKind,
    pub amount_cents: i64,
    pub comment: String,
    #[serde(default)]
    pub tax_relevant: bool,
    /// Sets a manual override. Omit to let the rule table decide.
    pub category_id: Option<Uuid>,
    #[serde(default)]
    pub clear_category_override: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookingPage {
    pub items: Vec<Booking>,
    pub total: i64,
    pub page: u32,
    pub page_size: u32,
    /// Sums for the CURRENT filter, so the UI never re-derives them client-side.
    pub sum_income_cents: i64,
    pub sum_expense_cents: i64,
    pub sum_net_cents: i64,
    pub uncategorized_count: i64,
}

/// One year's worth of whatever a search matched.
///
/// The point of a cross-year search is not the list — it is this: what a merchant
/// has cost per year since 2023, on one screen, without four page loads.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchYearSummary {
    pub year: i32,
    pub booking_count: i64,
    pub income_cents: i64,
    pub expense_cents: i64,
    /// Stored convention: expenses minus income, so a year that earned is negative.
    pub net_cents: i64,
}

/// A distinct comment the search matched, with how often it occurs.
///
/// Hand-typed comments drift — `Kaufland`, `kaufland`, `Kaufland Berlin` — and a
/// search for one of them should say which spellings it actually caught.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchComment {
    /// The spelling used most recently, which is the one worth showing.
    pub comment: String,
    pub booking_count: i64,
    pub net_cents: i64,
    pub category_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    /// Echoed back, trimmed, so the UI never has to guess what was asked.
    pub query: String,
    pub items: Vec<Booking>,
    pub total: i64,
    pub page: u32,
    pub page_size: u32,
    pub sum_income_cents: i64,
    pub sum_expense_cents: i64,
    pub sum_net_cents: i64,
    /// Newest year first, and only years that actually matched.
    pub by_year: Vec<SearchYearSummary>,
    pub comments: Vec<SearchComment>,
}

// -------------------------------------------------------- categories & rules

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: Uuid,
    pub name: String,
    pub type_code: String,
    pub type_label: String,
    pub sort_order: i16,
    pub archived: bool,
    pub booking_count: Option<i64>,
    pub net_cents: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CategoryInput {
    pub name: String,
    pub type_code: String,
    pub sort_order: Option<i16>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: Uuid,
    pub comment: String,
    /// `lower(trim(comment))` — what matching actually compares.
    pub normalized_comment: String,
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    /// How `to ING` and `from Volksbank` become transfers: an ordinary editable rule,
    /// not a hard-coded list.
    pub kind_override: Option<BookingKind>,
    pub source: String,
    pub match_count: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuleInput {
    pub comment: String,
    pub category_id: Option<Uuid>,
    pub kind_override: Option<BookingKind>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRulesResult {
    pub examined: i64,
    pub recategorized: i64,
    pub still_uncategorized: i64,
    pub dry_run: bool,
}

// ---------------------------------------------------------------- analysis

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CategoryTypeSummary {
    pub type_code: String,
    pub label: String,
    pub net_cents: i64,
    pub booking_count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    pub year: i32,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub balance_cents: i64,
    pub opening_balance_cents: i64,
    pub closing_balance_cents: i64,
    /// Set when a configured opening disagrees with the previous year's close.
    pub carryover_gap_cents: Option<i64>,
    pub average_expense_per_month_cents: i64,
    pub fixed_costs_per_month_cents: i64,
    pub months_with_data: i64,
    /// `balance / income` — reproduces the spreadsheet, and is misleading on its own
    /// because gross income includes cost-sharing that is really negative expense.
    pub savings_rate_naive: f64,
    /// Excludes Sparen (retained wealth) and transfers.
    pub savings_rate_consumption: f64,
    pub savings_amount_cents: i64,
    /// What was actually paid into the savings categories this year — the decision,
    /// not the residue. This is what a person means by "meine Sparrate".
    pub savings_deposit_cents: i64,
    /// The same, per month with data, which is the form it is usually said in.
    pub savings_deposit_per_month_cents: i64,
    /// Of real income, so it is comparable with `savingsRateConsumption`.
    pub savings_deposit_rate: f64,
    pub booking_count: i64,
    pub tax_relevant_count: i64,
    pub uncategorized_count: i64,
    pub uncategorized_net_cents: i64,
    pub by_type: Vec<CategoryTypeSummary>,
    pub top_categories: Vec<CategoryAnalysisRow>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CategoryAnalysisRow {
    pub category_id: Option<Uuid>,
    pub category_name: String,
    pub category_type: Option<String>,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub net_cents: i64,
    /// True when the category earned more than it cost. The UI must render this as a
    /// credit, not as a bare negative cost.
    pub net_is_negative: bool,
    /// Zero for negative-net categories — they have no share of the costs.
    pub share_of_total: f64,
    pub average_per_month_cents: i64,
    pub booking_count: i64,
    pub monthly_net_cents: Vec<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CategoryAnalysis {
    pub year: i32,
    pub rows: Vec<CategoryAnalysisRow>,
    pub total_net_cents: i64,
    pub months_with_data: i64,
    pub uncategorized_count: i64,
    /// Transfers are excluded from this analysis; the count lets the UI say so.
    pub excluded_transfer_count: i64,
}

// ---------------------------------------------------------------- year on year

/// One category, this year against last.
///
/// Every figure is in the stored expense-positive convention: a positive net is what
/// something cost, and a positive `deltaCents` means it cost MORE than last year (or,
/// for an income category, that less came in). The display flips that sign, exactly
/// as it does everywhere else.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompareRow {
    pub category_id: Option<Uuid>,
    pub category_name: String,
    pub category_type: Option<String>,
    pub net_cents: i64,
    pub previous_net_cents: i64,
    pub delta_cents: i64,
    /// `delta / |previous|`. `None` when there was nothing last year to be a share
    /// of — a percentage against zero is not a large number, it is not a number.
    pub delta_ratio: Option<f64>,
    /// The same four figures over the months BOTH years actually have. This is the
    /// pair that may be compared; the ones above are the full years as they stand.
    pub comparable_net_cents: i64,
    pub comparable_previous_net_cents: i64,
    pub comparable_delta_cents: i64,
    pub comparable_delta_ratio: Option<f64>,
    pub monthly_net_cents: Vec<i64>,
    pub previous_monthly_net_cents: Vec<i64>,
    pub booking_count: i64,
    pub previous_booking_count: i64,
    /// Nothing in this category last year, or nothing this year. Either makes the
    /// percentage meaningless and both are worth saying out loud.
    pub is_new: bool,
    pub is_gone: bool,
}

/// One category type, this year against last.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompareTypeRow {
    pub type_code: String,
    pub label: String,
    pub net_cents: i64,
    pub previous_net_cents: i64,
    pub delta_cents: i64,
    pub delta_ratio: Option<f64>,
    pub comparable_net_cents: i64,
    pub comparable_previous_net_cents: i64,
    pub comparable_delta_cents: i64,
}

/// One year's headline figures, both as it stands and restricted to the months the
/// other year also has.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompareTotals {
    pub year: i32,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub saldo_cents: i64,
    pub booking_count: i64,
    /// Months carrying at least one booking. A nine-month year and a twelve-month
    /// year are not comparable, and this is what says so.
    pub months_with_data: i64,
    /// The last month of this year that holds a booking, or `None` for an empty
    /// year. How far the year has actually got — which is where a trailing window
    /// should end, and not something the client should have to infer.
    pub last_month_with_data: Option<u8>,
    pub comparable_income_cents: i64,
    pub comparable_expense_cents: i64,
    pub comparable_saldo_cents: i64,
}

/// A year against its predecessor.
///
/// The trap this endpoint exists to avoid: 2026 holds nine months and 2025 holds
/// twelve, so the raw totals make this year look thrifty by three months' worth of
/// spending. Every figure therefore comes twice — as it stands, and restricted to
/// `comparableMonths`, the months both years actually have — and the UI leads with
/// the restricted pair whenever the two years differ in length.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct YearComparison {
    pub year: i32,
    pub previous_year: i32,
    pub current: CompareTotals,
    pub previous: CompareTotals,
    /// Month numbers 1..12 that BOTH years carry bookings in, ascending.
    pub comparable_months: Vec<u8>,
    /// True when the two years cover the same months, so the raw figures are already
    /// a fair comparison.
    pub fully_comparable: bool,
    pub rows: Vec<CompareRow>,
    pub by_type: Vec<CompareTypeRow>,
    /// Empty when the previous year holds nothing at all — the UI then says so
    /// rather than rendering a table of "+100 %".
    pub previous_year_has_data: bool,
}

/// One month inside a trailing window. Carries its year, because the window crosses
/// the year boundary and a bare "Januar" would be ambiguous in exactly the place this
/// view exists to look at.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TrailingMonth {
    pub year: i32,
    pub month: u8,
    pub month_name: String,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub saldo_cents: i64,
    pub net_cents: i64,
    pub booking_count: i64,
}

/// One category over a trailing window, with its twelve values in window order.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TrailingCategory {
    pub category_id: Option<Uuid>,
    pub category_name: String,
    pub category_type: Option<String>,
    pub net_cents: i64,
    pub average_per_month_cents: i64,
    pub booking_count: i64,
    /// Twelve entries, oldest first, aligned with `months`.
    pub monthly_net_cents: Vec<i64>,
}

/// Twelve months ending at a given period, whatever years they fall in.
///
/// A calendar year is an accounting convention, not a unit of behaviour: in January
/// the year-to-date view has one month in it. This is the window that always has
/// twelve, which is what makes a trend visible.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TrailingWindow {
    pub year: i32,
    pub month: u8,
    /// The first month of the window, which is eleven months before the last.
    pub from_year: i32,
    pub from_month: u8,
    pub months: Vec<TrailingMonth>,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub saldo_cents: i64,
    pub booking_count: i64,
    pub months_with_data: i64,
    pub rows: Vec<TrailingCategory>,
}

/// A single subject's twelve months — one category, or one comment.
///
/// The spreadsheet's `Filter` tab did exactly this and it is the question a
/// household actually asks: not "what did Auto & Parken cost" but "how much do I
/// spend on tanken, and is it getting worse".
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonthlySeries {
    pub year: i32,
    /// `category` or `comment` — which kind of subject was asked for.
    pub mode: String,
    /// The category name or the comment, verbatim. Data, so never translated.
    pub subject: String,
    pub category_id: Option<Uuid>,
    /// Twelve entries, Januar first. Gross legs and the net, so the UI can show
    /// the netting rather than assert it.
    pub months: Vec<SeriesMonth>,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub net_cents: i64,
    /// Divided by the months that carry a booking for THIS subject, not by twelve
    /// and not by the year's months — an expense that only happens in summer
    /// should not look small.
    pub average_per_active_month_cents: i64,
    pub booking_count: i64,
    pub months_with_data: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SeriesMonth {
    pub month: u8,
    pub month_name: String,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub net_cents: i64,
    pub booking_count: i64,
}

/// What the picker offers: every comment that actually occurs, most used first.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SeriesSubject {
    pub comment: String,
    pub booking_count: i64,
    pub net_cents: i64,
    pub category_name: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyRow {
    pub month: u8,
    pub month_name: String,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub balance_cents: i64,
    /// `null` for months with no bookings — the sheet's `NA()`. Zero-filling would
    /// draw a cliff in the cumulative chart that does not exist.
    pub cumulative_cents: Option<i64>,
    pub savings_rate: Option<f64>,
    pub fixed_costs_net_cents: i64,
    pub variable_costs_net_cents: i64,
    pub savings_net_cents: i64,
    pub other_net_cents: i64,
    pub booking_count: i64,
    pub uncategorized_count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyOverview {
    pub year: i32,
    pub months: Vec<MonthlyRow>,
    pub total: MonthlyRow,
}

// ------------------------------------------------------------------- years

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Year {
    pub year: i32,
    pub opening_balance_cents: i64,
    /// `configured` for 2026 (40.000,00 entered by hand) versus `derived` from the
    /// previous year's close.
    pub opening_source: String,
    pub locked: bool,
    pub booking_count: i64,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub balance_cents: i64,
    pub closing_balance_cents: i64,
    pub carryover_gap_cents: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct YearInput {
    pub year: i32,
    pub opening_balance_cents: i64,
    #[serde(default)]
    pub locked: bool,
}

// ------------------------------------------------------------------ shared

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub status: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaxEntry {
    pub booking_id: Uuid,
    pub index: i64,
    pub month: u8,
    pub month_name: String,
    pub comment: String,
    pub category_name: Option<String>,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub has_receipt: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaxCategorySummary {
    pub category_name: String,
    pub expense_cents: i64,
    pub income_cents: i64,
    pub net_cents: i64,
    pub count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaxReport {
    pub year: i32,
    pub total_expense_cents: i64,
    pub total_income_cents: i64,
    pub total_net_cents: i64,
    pub booking_count: i64,
    pub receipts_present: i64,
    pub entries: Vec<TaxEntry>,
    pub by_category: Vec<TaxCategorySummary>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DateTimeWrapper(pub DateTime<Utc>);

// -------------------------------------------------------------- recurring

/// A year/month pair on the wire. `period_ord` is an internal encoding —
/// `year*12 + month - 1` — and leaking it into the API would force every client to
/// reimplement the arithmetic to show a month name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Period {
    pub year: i32,
    pub month: u8,
}

impl Period {
    pub fn from_ord(ord: i32) -> Self {
        let (year, month) = crate::locale::ord_to_year_month(ord);
        Self { year, month }
    }
    pub fn ord(self) -> i32 {
        crate::locale::period_ord(self.year, self.month)
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecurringTemplate {
    pub id: Uuid,
    pub name: String,
    pub comment: String,
    pub kind: BookingKind,
    pub amount_cents: i64,
    /// The amount varies month to month (the gym is 29,00 / 31,50 / 34,50), so
    /// materialising must produce a **draft** the user confirms with the real figure.
    pub amount_is_estimate: bool,
    pub category_id: Option<Uuid>,
    /// The category this template books into: the override if it has one, otherwise
    /// the one the rule table gives its comment. An empty cell in the list read as
    /// "uncategorised", which was never true.
    pub category_name: Option<String>,
    pub category_type: Option<String>,
    /// True when the name above comes from the rule table rather than from an
    /// override — so a rule change will move it, and that is worth saying.
    pub category_from_rule: bool,
    pub tax_relevant: bool,
    pub day_of_month: Option<u8>,
    /// 1 = monthly, 3 = quarterly, 12 = annual.
    pub interval_months: u8,
    /// The month the cycle is measured from; due months are `anchor + n*interval`.
    pub anchor: Period,
    pub active_from: Period,
    pub active_to: Option<Period>,
    pub active: bool,
    pub sort_order: i16,
    /// Whether the template falls due in the period the request asked about, and
    /// whether a booking for it already exists there. Both are `null` when the
    /// request named no period.
    pub due_in_period: Option<bool>,
    pub booked_in_period: Option<bool>,
    pub last_booked: Option<Period>,
    pub booking_count: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecurringTemplateInput {
    pub name: String,
    pub comment: String,
    pub kind: BookingKind,
    pub amount_cents: i64,
    #[serde(default)]
    pub amount_is_estimate: bool,
    /// A manual override, exactly as on a booking. Omit to let the rule table decide
    /// at materialisation time, so a rule change still reaches future bookings.
    pub category_id: Option<Uuid>,
    #[serde(default)]
    pub tax_relevant: bool,
    pub day_of_month: Option<u8>,
    #[serde(default = "one")]
    pub interval_months: u8,
    /// Defaults to `active_from`, which is what makes a plain monthly template a
    /// two-field affair.
    pub anchor: Option<Period>,
    pub active_from: Period,
    pub active_to: Option<Period>,
    #[serde(default = "yes")]
    pub active: bool,
    #[serde(default)]
    pub sort_order: i16,
}

fn one() -> u8 {
    1
}
fn one_share() -> i64 {
    1
}
fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MaterializeRequest {
    pub year: i32,
    pub month: u8,
    /// Restricts the run to a subset — the month checklist's individual ticks.
    /// Omitted means every template due in that month.
    pub template_ids: Option<Vec<Uuid>>,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MaterializedItem {
    pub template_id: Uuid,
    pub template_name: String,
    pub comment: String,
    pub amount_cents: i64,
    pub kind: BookingKind,
    /// `draft` for estimate templates, `confirmed` otherwise.
    pub status: String,
    pub booking_id: Option<Uuid>,
    /// Set when nothing was created. `alreadyBooked` is the normal, expected case.
    pub skipped_reason: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MaterializeResult {
    pub year: i32,
    pub month: u8,
    pub month_name: String,
    pub created: i64,
    pub skipped: i64,
    pub drafts: i64,
    pub dry_run: bool,
    pub items: Vec<MaterializedItem>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmBookingInput {
    /// Confirming is also where an estimate's real amount is entered, because that
    /// is the only reason the draft existed.
    pub amount_cents: Option<i64>,
}

// ---------------------------------------------------------- sinking funds

/// A known annual or quarterly lump, stated once so it can be accrued monthly.
///
/// A fund books nothing. It is an EXPECTATION, compared against the ordinary
/// bookings in its category — a fund that created bookings would double-count the
/// very spending it exists to anticipate.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SinkingFund {
    pub id: Uuid,
    pub name: String,
    /// Optional: a fund can be a plain reminder before the user decides which
    /// category it belongs to. Without one there is nothing to compare against, and
    /// `spentCents` stays 0 rather than guessing.
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    pub annual_cents: i64,
    /// 1..12 — the month the bill actually arrives.
    pub due_month: u8,
    pub due_month_name: String,
    pub note: Option<String>,
    pub active: bool,
    pub sort_order: i16,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SinkingFundInput {
    pub name: String,
    pub category_id: Option<Uuid>,
    pub annual_cents: i64,
    pub due_month: u8,
    pub note: Option<String>,
    #[serde(default = "yes")]
    pub active: bool,
    #[serde(default)]
    pub sort_order: i16,
}

/// One fund, measured against what has actually been spent this year.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FundStatus {
    pub fund: SinkingFund,
    /// What to set aside each month. An approximation by construction — a year does
    /// not always divide into twelve equal cents — which is why it is never the
    /// figure the cumulative one is built from.
    pub monthly_accrual_cents: i64,
    /// What should be aside by the end of the asked-about month. Proportional to the
    /// ANNUAL amount, so twelve months come back to it exactly.
    pub accrued_by_month_cents: i64,
    /// Net spending in the fund's category this year, stored sign: positive is cost.
    pub spent_cents: i64,
    /// What is still expected to leave the account this year. Never negative: a bill
    /// that came in cheaper does not become money owed to you.
    pub remaining_cents: i64,
    /// Accrued minus spent. Positive is a cushion; negative means the bill landed
    /// before the fund had caught up with it.
    pub over_under_cents: i64,
    /// Whether the due month is in the past relative to the asked-about month.
    pub due_passed: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FundOverview {
    pub year: i32,
    pub month: u8,
    pub funds: Vec<FundStatus>,
    pub monthly_accrual_cents: i64,
    pub accrued_by_month_cents: i64,
    pub spent_cents: i64,
    /// The headline: what these known lumps will still take out of the account this
    /// year. The number the monthly saldo does not tell you.
    pub owed_to_the_future_cents: i64,
}

/// A fund the history argues for. Suggested, never created: the app does not get to
/// decide that a holiday is a recurring obligation.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FundSuggestion {
    pub category_id: Uuid,
    pub category_name: String,
    pub annual_cents: i64,
    pub due_month: u8,
    pub due_month_name: String,
    /// How many months of the year the category was spent in at all. The point of
    /// the suggestion is the clustering, so the evidence is returned with it.
    pub months_with_spending: i64,
    pub booking_count: i64,
    pub year: i32,
}

// --------------------------------------------------------------- receipts

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub id: Uuid,
    pub booking_id: Option<Uuid>,
    /// The user's own filename, kept as metadata only. It is never a path component.
    pub filename: String,
    pub content_type: String,
    pub byte_size: i64,
    pub sha256: String,
    pub uploaded_at: DateTime<Utc>,
}

// ---------------------------------------------------------------- exports

/// The full-export document.
///
/// Money here stays **integer cents**, because this file is the account's backup and
/// the input to `POST /exports/restore`. The CSV sibling is the one place de-DE
/// formatting is applied to exported money, and only because its reader is Excel.
///
/// Nothing references a uuid across the document except where it is internal to it:
/// categories, rules and templates are joined by NAME, because ids are per-user and
/// a restore into a different account must still resolve them.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportDocument {
    /// Bumped when the shape changes incompatibly; `restore` refuses what it cannot
    /// read rather than guessing.
    pub format_version: i32,
    pub exported_at: DateTime<Utc>,
    pub app: String,
    /// `null` for a whole-account export.
    pub year: Option<i32>,
    pub category_types: Vec<ExportCategoryType>,
    pub categories: Vec<ExportCategory>,
    pub rules: Vec<ExportRule>,
    pub years: Vec<ExportYear>,
    pub recurring_templates: Vec<ExportTemplate>,
    pub bookings: Vec<ExportBooking>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportCategoryType {
    pub code: String,
    pub label: String,
    pub sort_order: i16,
    pub is_income: bool,
    pub is_savings: bool,
    pub in_consumption: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportCategory {
    pub name: String,
    pub type_code: String,
    pub sort_order: i16,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportRule {
    pub comment: String,
    pub category_name: Option<String>,
    pub kind_override: Option<BookingKind>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportYear {
    pub year: i32,
    pub opening_balance_cents: i64,
    pub opening_source: String,
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportTemplate {
    pub name: String,
    pub comment: String,
    pub kind: BookingKind,
    pub amount_cents: i64,
    pub amount_is_estimate: bool,
    pub category_name: Option<String>,
    pub tax_relevant: bool,
    pub day_of_month: Option<u8>,
    pub interval_months: u8,
    pub anchor: Period,
    pub active_from: Period,
    pub active_to: Option<Period>,
    pub active: bool,
    pub sort_order: i16,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportBooking {
    pub year: i32,
    pub month: u8,
    pub booked_on: Option<NaiveDate>,
    pub kind: BookingKind,
    pub amount_cents: i64,
    pub comment: String,
    pub tax_relevant: bool,
    pub category_name: Option<String>,
    pub category_source: CategorySource,
    pub status: String,
    pub origin: String,
    pub shared: bool,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    /// Carried so a restore of an imported account stays idempotent against a later
    /// re-import of the same workbook.
    pub import_fingerprint: Option<String>,
    pub template_name: Option<String>,
    pub has_receipt: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub categories_created: i64,
    pub rules_created: i64,
    pub years_created: i64,
    pub templates_created: i64,
    pub bookings_created: i64,
    /// Bookings whose export said `rule` but whose rule is absent from the document.
    /// They are restored as manual overrides so the figure never moves; the count
    /// makes that visible instead of silent.
    pub rule_links_downgraded: i64,
    pub warnings: Vec<String>,
}

// ------------------------------------------------------------- kitchenowl
//
// KitchenOwl is a SEPARATE, PARALLEL LEDGER. Nothing in this section may be added
// to a personal-booking figure, and every DTO that carries both the shared amount
// and the user's own share names them so they cannot be confused:
// `amountCents` is what the household spent, `ownShareCents` is the user's slice.

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoMember {
    pub member_id: i64,
    pub name: String,
    pub username: Option<String>,
    /// From `member[].expense_balance`, the only balance KitchenOwl exposes.
    /// Negative means the user owes the household.
    pub balance_cents: i64,
    pub is_me: bool,
    pub is_owner: bool,
    pub is_admin: bool,
    pub fetched_at: DateTime<Utc>,
}

/// A KitchenOwl expense category. A **different taxonomy** from the app's 32
/// categories: seven household labels with no relationship to Fixkosten/Variable
/// Kosten. Never auto-mapped, and the UI must not style it like an app category.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoCategory {
    pub category_id: i64,
    pub name: String,
    /// Packed ARGB as KitchenOwl stores it, not a CSS colour.
    pub color_argb: Option<i64>,
    pub budget_cents: Option<i64>,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoMetadata {
    pub members: Vec<KoMember>,
    pub categories: Vec<KoCategory>,
    pub fetched_at: Option<DateTime<Utc>>,
    /// Older than `KITCHENOWL_METADATA_STALE_SECONDS`. Served anyway: a push
    /// dialogue that will not open because KitchenOwl is down is worse than one
    /// that opens with yesterday's member list and says so.
    pub stale: bool,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoShare {
    pub member_id: i64,
    pub name: Option<String>,
    /// An integer WEIGHT, not a percentage: the share is
    /// `amount * factor / sum(factors)`, allocated by largest remainder so the
    /// shares sum to the amount exactly.
    pub factor: i64,
    pub share_cents: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoExpense {
    pub id: Uuid,
    pub external_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub date: NaiveDate,
    /// The FULL shared amount — what left somebody's account.
    pub amount_cents: i64,
    /// The user's slice of it. These two are the most confusable pair of numbers in
    /// the feature and are never summed with each other or with a booking.
    pub own_share_cents: i64,
    pub paid_by_id: Option<i64>,
    pub paid_by_name: Option<String>,
    pub paid_for: Vec<KoShare>,
    pub ko_category_id: Option<i64>,
    pub ko_category_name: Option<String>,
    pub exclude_from_statistics: bool,
    /// Set when the expense vanished from KitchenOwl. Kept rather than deleted,
    /// because the row may carry a link the user confirmed by hand.
    pub archived_at: Option<DateTime<Utc>>,
    pub linked_booking_id: Option<Uuid>,
    pub linked_booking_comment: Option<String>,
    pub linked_booking_amount_cents: Option<i64>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoExpensePage {
    pub items: Vec<KoExpense>,
    pub total: i64,
    pub page: u32,
    pub page_size: u32,
    /// Sums for the current filter. Both are reported, always, because showing only
    /// one of them is how the two get confused.
    pub sum_amount_cents: i64,
    pub sum_own_share_cents: i64,
    pub linked_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoMatchCandidate {
    pub booking_id: Uuid,
    pub comment: String,
    pub amount_cents: i64,
    pub year: i32,
    pub month: u8,
    pub month_name: String,
    pub score: f64,
    /// `fullAmount` | `ownShare` | `fullAmountNear` | `ownShareNear` — so the UI can
    /// say why rather than showing a bare number.
    pub basis: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoDraft {
    pub id: Uuid,
    pub status: String,
    pub expense: KoExpense,
    pub candidates: Vec<KoMatchCandidate>,
    /// `link` when a candidate cleared the threshold, otherwise `none`. Never
    /// `create`: a pull never writes a booking, and the 63-of-211 overlap is why.
    pub suggested_action: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoDraftPage {
    pub items: Vec<KoDraft>,
    pub total: i64,
    pub open_count: i64,
    pub likely_count: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoSyncRun {
    pub id: Uuid,
    pub kind: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_count: i64,
    pub updated_count: i64,
    pub archived_count: i64,
    pub failed_count: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoStatus {
    /// The server has a URL and a token.
    pub configured: bool,
    /// This account opted in by running a sync at least once. The credentials are
    /// process-global, so mirroring into every account would copy one household
    /// into other people's books.
    pub enabled: bool,
    /// Derived from the last sync run, never from a live probe — this endpoint must
    /// not block on HTTP.
    pub reachable: Option<bool>,
    pub running: bool,
    pub household_id: Option<i64>,
    pub household_name: Option<String>,
    pub last_expense_run: Option<KoSyncRun>,
    pub last_metadata_run: Option<KoSyncRun>,
    pub next_run_at: Option<DateTime<Utc>>,
    /// `0` means the periodic pull is disabled and only "jetzt synchronisieren"
    /// moves anything.
    pub poll_seconds: u64,
    pub mirrored_count: i64,
    pub archived_count: i64,
    pub linked_count: i64,
    pub open_draft_count: i64,
    pub likely_duplicate_count: i64,
    pub pending_push_count: i64,
    pub failed_push_count: i64,
    pub metadata_fetched_at: Option<DateTime<Utc>>,
    pub metadata_stale: bool,
    pub last_error: Option<String>,
}

/// Settling up with the household — the one place the two ledgers touch.
///
/// A settlement is money moving between the user and the household, not money
/// consumed: the household's purchases are already in the personal ledger at full
/// value, so booking the settlement as an expense would count them twice. It is
/// therefore a `transfer`, whose `net_cents` is 0 by construction, and it belongs
/// to no category.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoSettlement {
    /// KitchenOwl's own signed balance, passed through UNFLIPPED: negative means
    /// the user owes the household. `null` when the mirror holds no member row for
    /// the user yet.
    pub balance_cents: Option<i64>,
    /// `i_owe` | `household_owes_me` | `settled` | `unknown`. The word the UI reads
    /// out, decided here so that the figure and the sentence beside it cannot
    /// disagree — they have done exactly that once already.
    pub direction: String,
    /// What settling would book: `expense` when the user owes, `income` when the
    /// household does, `null` when there is nothing to settle. NEVER `transfer` —
    /// this money goes to another person, so it moves the balance.
    pub kind: Option<BookingKind>,
    /// Where it would land — `Haushaltsausgleich`, which is where the user's own
    /// thirteen historical settlements live.
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    /// True when that category does not exist here, so the rule table decides and
    /// the booking may come out uncategorised. Said out loud rather than discovered.
    pub category_is_fallback: bool,
    /// Always positive: the sum that would change hands. `0` when settled.
    pub amount_cents: i64,
    pub period: Period,
    /// `Ausgleich <Monat>` — the spelling the spreadsheet used. Data, not chrome.
    pub suggested_comment: String,
    /// True once this period carries a settlement booking, so the button is offered
    /// exactly once.
    pub already_settled: bool,
    /// The settlement booking, when one exists. A personal-ledger figure, never
    /// added to any KitchenOwl figure above it.
    pub booking: Option<Booking>,
    /// The balance that settlement was based on, which is not necessarily today's:
    /// expenses booked afterwards move the balance again.
    pub settled_balance_cents: Option<i64>,
    pub settled_at: Option<DateTime<Utc>>,
}

/// The dashboard widget. Reads the **local mirror only** and never blocks on HTTP,
/// so KitchenOwl being down costs a staleness warning rather than a spinner.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoSummary {
    pub configured: bool,
    pub enabled: bool,
    pub household_name: Option<String>,
    pub members: Vec<KoMember>,
    pub my_balance_cents: Option<i64>,
    pub recent: Vec<KoExpense>,
    pub month: Period,
    /// The household's spend this month, and the user's share of it. Two figures,
    /// always both, never added to anything from the personal ledger.
    pub month_amount_cents: i64,
    pub month_own_share_cents: i64,
    pub month_count: i64,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub stale: bool,
    pub warning: Option<String>,
}

/// One KitchenOwl category over a year.
///
/// Both figures, always: `amountCents` is what the household spent and
/// `ownShareCents` is the user's slice of it. They are different numbers with
/// different meanings and adding them is meaningless.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoCategoryAnalysisRow {
    /// Null for expenses with no KitchenOwl category — the normal case for a third
    /// of the live corpus, never an error.
    pub ko_category_id: Option<i64>,
    pub ko_category_name: Option<String>,
    pub amount_cents: i64,
    pub own_share_cents: i64,
    pub expense_count: i64,
    /// Of the household's total, not of the user's share.
    pub share_of_total: f64,
    pub average_per_month_cents: i64,
    pub average_own_share_per_month_cents: i64,
    pub monthly_amount_cents: Vec<i64>,
    pub monthly_own_share_cents: Vec<i64>,
}

/// Who paid, which only a shared ledger can ask.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoPayerShare {
    pub member_id: Option<i64>,
    pub name: String,
    pub amount_cents: i64,
    pub expense_count: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoCategoryAnalysis {
    pub year: i32,
    pub rows: Vec<KoCategoryAnalysisRow>,
    pub total_amount_cents: i64,
    pub total_own_share_cents: i64,
    pub expense_count: i64,
    pub months_with_data: i64,
    pub uncategorized_count: i64,
    /// Expenses KitchenOwl itself keeps out of its statistics, so this analysis
    /// does too. Reported rather than hidden.
    pub excluded_count: i64,
    pub paid_by: Vec<KoPayerShare>,
    /// The years the mirror holds anything for, newest first — the year picker
    /// cannot borrow the personal ledger's years, which start earlier.
    pub years: Vec<i32>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoSeriesMonth {
    pub month: u8,
    /// German month name; data, not chrome.
    pub month_name: String,
    pub amount_cents: i64,
    pub own_share_cents: i64,
    pub expense_count: i64,
}

/// One KitchenOwl category or one recurring name across twelve months.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoMonthlySeries {
    pub year: i32,
    /// `category`, `name` or `uncategorized`.
    pub mode: String,
    pub subject: String,
    pub ko_category_id: Option<i64>,
    pub months: Vec<KoSeriesMonth>,
    pub amount_cents: i64,
    pub own_share_cents: i64,
    pub expense_count: i64,
    /// Divided by the months that hold THIS subject, never by twelve.
    pub average_per_active_month_cents: i64,
    pub average_own_share_per_active_month_cents: i64,
    pub months_with_data: i64,
}

/// One KitchenOwl category, this year against last.
///
/// Both figures are carried through the comparison, and so are both deltas. A
/// household that spent 200 € more of which the user carries 100 € more is two
/// facts, and collapsing them to one would be the same mistake the single-year
/// view is built to avoid.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoCompareRow {
    pub ko_category_id: Option<i64>,
    pub ko_category_name: Option<String>,
    pub amount_cents: i64,
    pub own_share_cents: i64,
    pub expense_count: i64,
    pub previous_amount_cents: i64,
    pub previous_own_share_cents: i64,
    pub previous_expense_count: i64,
    pub delta_amount_cents: i64,
    pub delta_own_share_cents: i64,
    /// `delta / previous`, absent when there is no previous to be a share of —
    /// "+∞ %" for a category that did not exist last year is worse than nothing,
    /// and `isNew` says what actually happened.
    pub delta_ratio: Option<f64>,
    /// The same figures restricted to the months both years carry. The mirror
    /// started on 2024-12-22, so 2024 against 2025 is a part year against a full
    /// one and the raw totals would be a statement about the calendar.
    pub comparable_amount_cents: i64,
    pub comparable_own_share_cents: i64,
    pub comparable_previous_amount_cents: i64,
    pub comparable_previous_own_share_cents: i64,
    pub comparable_delta_amount_cents: i64,
    pub comparable_delta_own_share_cents: i64,
    pub comparable_delta_ratio: Option<f64>,
    pub monthly_amount_cents: Vec<i64>,
    pub monthly_own_share_cents: Vec<i64>,
    pub previous_monthly_amount_cents: Vec<i64>,
    pub previous_monthly_own_share_cents: Vec<i64>,
    pub is_new: bool,
    pub is_gone: bool,
}

/// One year's side of the comparison.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoCompareTotals {
    pub year: i32,
    pub amount_cents: i64,
    pub own_share_cents: i64,
    pub expense_count: i64,
    pub months_with_data: i64,
    /// Where the year's data stops, so a trailing window can end there instead of
    /// running into months that have not happened.
    pub last_month_with_data: Option<u8>,
    pub comparable_amount_cents: i64,
    pub comparable_own_share_cents: i64,
    pub comparable_expense_count: i64,
    /// What KitchenOwl itself keeps out of its statistics, per year.
    pub excluded_count: i64,
}

/// Who paid, this year against last.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoComparePayer {
    pub member_id: Option<i64>,
    pub name: String,
    pub amount_cents: i64,
    pub previous_amount_cents: i64,
    pub delta_cents: i64,
    pub expense_count: i64,
    pub previous_expense_count: i64,
}

/// The household's year against the one before it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoYearComparison {
    pub year: i32,
    pub previous_year: i32,
    pub current: KoCompareTotals,
    pub previous: KoCompareTotals,
    /// The months both years actually carry expenses in.
    pub comparable_months: Vec<u8>,
    /// False when the two years cover different months, which is when the raw
    /// totals stop being a comparison.
    pub fully_comparable: bool,
    pub previous_year_has_data: bool,
    pub rows: Vec<KoCompareRow>,
    pub paid_by: Vec<KoComparePayer>,
    /// Every year the mirror holds, newest first — the picker's options.
    pub years: Vec<i32>,
}

/// One month of a trailing window, in window order.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoTrailingMonth {
    pub year: i32,
    pub month: u8,
    pub month_name: String,
    pub amount_cents: i64,
    pub own_share_cents: i64,
    pub expense_count: i64,
}

/// One category over a trailing window, its twelve values in window order.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoTrailingCategory {
    pub ko_category_id: Option<i64>,
    pub ko_category_name: Option<String>,
    pub amount_cents: i64,
    pub own_share_cents: i64,
    pub expense_count: i64,
    pub average_per_month_cents: i64,
    pub average_own_share_per_month_cents: i64,
    pub monthly_amount_cents: Vec<i64>,
    pub monthly_own_share_cents: Vec<i64>,
}

/// Twelve months ending wherever asked, ignoring the year boundary.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoTrailingWindow {
    pub year: i32,
    pub month: u8,
    pub from_year: i32,
    pub from_month: u8,
    pub amount_cents: i64,
    pub own_share_cents: i64,
    pub expense_count: i64,
    pub months_with_data: i64,
    pub months: Vec<KoTrailingMonth>,
    pub rows: Vec<KoTrailingCategory>,
}

/// A name worth charting, as the picker offers it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoSeriesSubject {
    pub name: String,
    pub expense_count: i64,
    pub amount_cents: i64,
    pub own_share_cents: i64,
    pub ko_category_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoPushIntent {
    pub booking_id: Uuid,
    /// `queued` | `sending` | `pushed` | `failed` | `abandoned` | `retracted`.
    pub state: String,
    pub booking_comment: Option<String>,
    pub amount_cents: i64,
    pub date: NaiveDate,
    pub name: String,
    pub marker: String,
    pub attempts: i32,
    /// Kept in every state. A queue item that failed silently is a booking the user
    /// believes is in KitchenOwl and is not.
    pub last_error: Option<String>,
    pub external_id: Option<i64>,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoPushShareInput {
    pub member_id: i64,
    /// Integer weight. `1` each is an even split; `{12, 7}` occurs in the real data.
    #[serde(default = "one_share")]
    pub factor: i64,
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoPushRequest {
    /// Defaults to the booking's comment.
    pub name: Option<String>,
    pub description: Option<String>,
    /// Defaults to the booking's own amount, untouched.
    pub amount_cents: Option<i64>,
    pub date: Option<NaiveDate>,
    /// KitchenOwl's own taxonomy. Never derived from the app's category.
    pub ko_category_id: Option<i64>,
    pub paid_by_id: Option<i64>,
    #[serde(default)]
    pub paid_for: Vec<KoPushShareInput>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoLinkRequest {
    pub booking_id: Uuid,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KoSyncResult {
    pub started: bool,
    pub expenses: Option<KoSyncRun>,
    pub metadata: Option<KoSyncRun>,
    /// Set when the run could not finish. The rest of the app keeps working.
    pub error: Option<String>,
}
