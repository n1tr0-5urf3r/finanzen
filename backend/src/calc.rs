//! The calculation engine.
//!
//! Pure functions over row structs — no SQL, no I/O — so every acceptance figure is
//! unit-testable without a database. The large-range endpoints have SQL
//! implementations too, and a test asserts the two agree field by field: two
//! independent implementations of the netting rule is the cheapest defence for a
//! layer whose entire value is being correct.
//!
//! Sign convention throughout: `net_cents` is EXPENSE-POSITIVE. A category net is
//! `expenses - income of the same category`, so a category that earned money nets
//! negative. Transfers carry `net_cents = 0`.

use std::collections::BTreeMap;

use crate::locale::div_round_half_up;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Income,
    Expense,
    Transfer,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
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

/// One confirmed ledger row, as `v_ledger` yields it.
#[derive(Debug, Clone)]
pub struct LedgerRow {
    pub period_year: i32,
    pub period_month: u8,
    pub kind: Kind,
    pub amount_cents: i64,
    pub category_id: Option<uuid::Uuid>,
    pub category_name: Option<String>,
    pub type_code: Option<String>,
    pub type_label: Option<String>,
    pub is_income: bool,
    pub is_savings: bool,
    pub in_consumption: bool,
    pub tax_relevant: bool,
}

impl LedgerRow {
    /// Mirrors the generated column in migration 0007 exactly.
    pub fn net_cents(&self) -> i64 {
        match self.kind {
            Kind::Expense => self.amount_cents,
            Kind::Income => -self.amount_cents,
            Kind::Transfer => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Totals {
    pub income_cents: i64,
    pub expense_cents: i64,
    pub saldo_cents: i64,
    pub booking_count: i64,
    pub transfer_count: i64,
    pub uncategorized_count: i64,
    pub uncategorized_net_cents: i64,
}

pub fn totals(rows: &[LedgerRow]) -> Totals {
    let mut t = Totals {
        income_cents: 0,
        expense_cents: 0,
        saldo_cents: 0,
        booking_count: rows.len() as i64,
        transfer_count: 0,
        uncategorized_count: 0,
        uncategorized_net_cents: 0,
    };
    for r in rows {
        match r.kind {
            Kind::Income => t.income_cents += r.amount_cents,
            Kind::Expense => t.expense_cents += r.amount_cents,
            Kind::Transfer => t.transfer_count += 1,
        }
        // A transfer legitimately has no category — it is money moving, not money
        // consumed. Counting it as "uncategorised" would put a permanent non-zero
        // badge on the dashboard that the user can never clear.
        if r.category_id.is_none() && r.kind != Kind::Transfer {
            t.uncategorized_count += 1;
            t.uncategorized_net_cents += r.net_cents();
        }
    }
    t.saldo_cents = t.income_cents - t.expense_cents;

    // The identity that ties the gross view to the netting view. If a fourth `kind`
    // is ever added without updating both sums, this fires immediately rather than
    // producing a plausible-looking wrong number months later.
    debug_assert_eq!(
        t.saldo_cents,
        -rows.iter().map(LedgerRow::net_cents).sum::<i64>(),
        "saldo must equal -sum(net_cents)"
    );
    t
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryNet {
    pub category_id: Option<uuid::Uuid>,
    pub category_name: String,
    pub type_code: Option<String>,
    pub type_label: Option<String>,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub net_cents: i64,
    pub booking_count: i64,
    pub monthly_net_cents: [i64; 12],
}

/// Net per category. Gross legs are returned alongside the net so the UI can show
/// `9.600,00 - 4.800,00 = 4.800,00` — the spreadsheet never showed this, and it is
/// the single most useful thing to add.
pub fn by_category(rows: &[LedgerRow]) -> Vec<CategoryNet> {
    let mut acc: BTreeMap<String, CategoryNet> = BTreeMap::new();
    for r in rows {
        // Transfers have no category meaning and must not appear here at all.
        if r.kind == Kind::Transfer {
            continue;
        }
        let name = r
            .category_name
            .clone()
            .unwrap_or_else(|| "(ohne Kategorie)".to_string());
        let entry = acc.entry(name.clone()).or_insert_with(|| CategoryNet {
            category_id: r.category_id,
            category_name: name,
            type_code: r.type_code.clone(),
            type_label: r.type_label.clone(),
            income_cents: 0,
            expense_cents: 0,
            net_cents: 0,
            booking_count: 0,
            monthly_net_cents: [0; 12],
        });
        match r.kind {
            Kind::Income => entry.income_cents += r.amount_cents,
            Kind::Expense => entry.expense_cents += r.amount_cents,
            Kind::Transfer => unreachable!(),
        }
        entry.net_cents += r.net_cents();
        entry.booking_count += 1;
        if (1..=12).contains(&r.period_month) {
            entry.monthly_net_cents[(r.period_month - 1) as usize] += r.net_cents();
        }
    }
    let mut out: Vec<_> = acc.into_values().collect();
    out.sort_by(|a, b| {
        b.net_cents
            .cmp(&a.net_cents)
            .then(a.category_name.cmp(&b.category_name))
    });
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeNet {
    pub type_code: String,
    pub type_label: String,
    pub net_cents: i64,
    pub booking_count: i64,
}

/// Net per type, aggregated from rows — never by summing rounded category nets.
pub fn by_type(rows: &[LedgerRow]) -> Vec<TypeNet> {
    let mut acc: BTreeMap<String, TypeNet> = BTreeMap::new();
    for r in rows {
        if r.kind == Kind::Transfer {
            continue;
        }
        let Some(code) = r.type_code.clone() else {
            continue;
        };
        let entry = acc.entry(code.clone()).or_insert_with(|| TypeNet {
            type_code: code,
            type_label: r.type_label.clone().unwrap_or_default(),
            net_cents: 0,
            booking_count: 0,
        });
        entry.net_cents += r.net_cents();
        entry.booking_count += 1;
    }
    acc.into_values().collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthRow {
    pub month: u8,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub saldo_cents: i64,
    /// `None` for months with no bookings — mirrors the sheet's `NA()`. Zero-filling
    /// would draw a cliff in the cumulative chart that does not exist.
    pub cumulative_cents: Option<i64>,
    pub fixed_net_cents: i64,
    pub variable_net_cents: i64,
    pub savings_net_cents: i64,
    pub other_net_cents: i64,
    pub booking_count: i64,
}

pub fn by_month(rows: &[LedgerRow]) -> Vec<MonthRow> {
    let mut months: Vec<MonthRow> = (1..=12)
        .map(|m| MonthRow {
            month: m,
            income_cents: 0,
            expense_cents: 0,
            saldo_cents: 0,
            cumulative_cents: None,
            fixed_net_cents: 0,
            variable_net_cents: 0,
            savings_net_cents: 0,
            other_net_cents: 0,
            booking_count: 0,
        })
        .collect();

    for r in rows {
        if !(1..=12).contains(&r.period_month) {
            continue;
        }
        let m = &mut months[(r.period_month - 1) as usize];
        m.booking_count += 1;
        match r.kind {
            Kind::Income => m.income_cents += r.amount_cents,
            Kind::Expense => m.expense_cents += r.amount_cents,
            Kind::Transfer => {}
        }
        match r.type_code.as_deref() {
            Some("fixkosten") => m.fixed_net_cents += r.net_cents(),
            Some("variabel") => m.variable_net_cents += r.net_cents(),
            Some("sparen") => m.savings_net_cents += r.net_cents(),
            Some("sonstiges") => m.other_net_cents += r.net_cents(),
            _ => {}
        }
    }

    // The running balance exists only between the first and last month that carry
    // bookings. Before the first there is nothing to accumulate; after the last
    // there is nothing to say, and repeating September's figure for October,
    // November and December would assert a balance for three months that have not
    // happened — the sheet writes NA() there for exactly that reason.
    //
    // A gap *inside* the range is different: January and March with an empty
    // February is still one continuous balance, so the gap carries the running
    // value forward rather than breaking the line.
    let last_with_data = months.iter().rposition(|m| m.booking_count > 0);

    let mut running = 0i64;
    let mut seen_any = false;
    for (index, m) in months.iter_mut().enumerate() {
        m.saldo_cents = m.income_cents - m.expense_cents;
        if m.booking_count > 0 {
            seen_any = true;
            running += m.saldo_cents;
        }
        m.cumulative_cents = match last_with_data {
            Some(last) if seen_any && index <= last => Some(running),
            _ => None,
        };
    }
    months
}

pub fn months_with_data(rows: &[LedgerRow]) -> i64 {
    let mut seen = std::collections::BTreeSet::new();
    for r in rows {
        seen.insert((r.period_year, r.period_month));
    }
    seen.len() as i64
}

#[derive(Debug, Clone, PartialEq)]
pub struct SavingsRates {
    pub naive_rate: f64,
    pub naive_numerator_cents: i64,
    pub naive_denominator_cents: i64,
    pub consumption_rate: f64,
    pub income_base_cents: i64,
    pub consumption_cents: i64,
    pub savings_amount_cents: i64,
    /// What actually went INTO the savings categories — the standing order, the
    /// deposit, the thing a person means by "meine Sparrate". Net, so a withdrawal
    /// in the same category subtracts from it.
    pub savings_deposit_cents: i64,
    pub savings_deposit_rate: f64,
}

/// Three figures, answering three different questions.
///
/// The DEPOSIT is what was actually paid into the savings categories. It is the
/// literal answer to "what is my Sparrate", it is a decision the user made rather
/// than a residue of one, and it is the only one of the three that can be checked
/// against a bank statement.
///
/// The CONSUMPTION rate is how much of real income was not consumed — which
/// includes money that simply stayed in the account, so it is the larger figure and
/// the honest measure of headroom.
///
/// The NAIVE rate reproduces the spreadsheet (`saldo / gross income`) and is wrong
/// in a specific way worth keeping computable: gross income includes cost-sharing
/// and refunds that are not income at all but negative expenses, which understates
/// it substantially.
pub fn savings_rates(rows: &[LedgerRow]) -> SavingsRates {
    let t = totals(rows);

    let income_base_cents: i64 = rows
        .iter()
        .filter(|r| r.kind == Kind::Income && r.is_income)
        .map(|r| r.amount_cents)
        .sum();
    let consumption_cents: i64 = rows
        .iter()
        .filter(|r| r.in_consumption)
        .map(LedgerRow::net_cents)
        .sum();
    let savings_amount_cents = income_base_cents - consumption_cents;
    // Net, and positive when money moved IN: the stored sign is expense-positive,
    // and a deposit is an expense of the Sparen category.
    let savings_deposit_cents: i64 = rows
        .iter()
        .filter(|r| r.is_savings)
        .map(LedgerRow::net_cents)
        .sum();

    let ratio = |n: i64, d: i64| if d == 0 { 0.0 } else { n as f64 / d as f64 };

    SavingsRates {
        savings_deposit_cents,
        savings_deposit_rate: ratio(savings_deposit_cents, income_base_cents),
        naive_rate: ratio(t.saldo_cents, t.income_cents),
        naive_numerator_cents: t.saldo_cents,
        naive_denominator_cents: t.income_cents,
        consumption_rate: ratio(savings_amount_cents, income_base_cents),
        income_base_cents,
        consumption_cents,
        savings_amount_cents,
    }
}

#[derive(Debug, Clone)]
pub struct YearRaw {
    pub year: i32,
    pub opening_cents: i64,
    pub opening_is_configured: bool,
    pub saldo_cents: i64,
    pub booking_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YearChained {
    pub year: i32,
    pub opening_cents: i64,
    pub opening_is_configured: bool,
    pub saldo_cents: i64,
    pub closing_cents: i64,
    pub booking_count: i64,
    /// Set when a configured opening disagrees with the previous year's close.
    /// Surfaced in the UI rather than silently disagreeing with the spreadsheet
    /// forever — for 2026 this is the 250,00 the legacy rows are short.
    pub chain_gap_cents: Option<i64>,
}

pub fn chain_years(years: &[YearRaw]) -> Vec<YearChained> {
    let mut sorted: Vec<&YearRaw> = years.iter().collect();
    sorted.sort_by_key(|y| y.year);

    let mut out = Vec::with_capacity(sorted.len());
    let mut carry: Option<i64> = None;
    for y in sorted {
        let opening = if y.opening_is_configured {
            y.opening_cents
        } else {
            carry.unwrap_or(y.opening_cents)
        };
        let closing = opening + y.saldo_cents;
        let chain_gap_cents = match (y.opening_is_configured, carry) {
            (true, Some(prev)) if prev != opening => Some(opening - prev),
            _ => None,
        };
        out.push(YearChained {
            year: y.year,
            opening_cents: opening,
            opening_is_configured: y.opening_is_configured,
            saldo_cents: y.saldo_cents,
            closing_cents: closing,
            booking_count: y.booking_count,
            chain_gap_cents,
        });
        carry = Some(closing);
    }
    out
}

pub fn average_expense_per_month(rows: &[LedgerRow]) -> i64 {
    let t = totals(rows);
    div_round_half_up(t.expense_cents, months_with_data(rows).max(1))
}

pub fn fixed_costs_per_month(rows: &[LedgerRow]) -> i64 {
    let fixed: i64 = rows
        .iter()
        .filter(|r| r.type_code.as_deref() == Some("fixkosten"))
        .map(LedgerRow::net_cents)
        .sum();
    div_round_half_up(fixed, months_with_data(rows).max(1))
}

/// Share of total cost. Categories with a negative net (a credit) get no share, and
/// the UI must say why — a blank share cell otherwise reads as a bug.
pub fn share_of_total(net_cents: i64, total_positive_net_cents: i64) -> f64 {
    if total_positive_net_cents <= 0 || net_cents <= 0 {
        return 0.0;
    }
    net_cents as f64 / total_positive_net_cents as f64
}
