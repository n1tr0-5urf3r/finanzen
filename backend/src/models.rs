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
