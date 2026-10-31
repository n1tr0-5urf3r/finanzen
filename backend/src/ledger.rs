//! Which ledger a statistic reads: with the household's shares, or without.
//!
//! Migration 0024 put my shares of what the flatmates paid into `v_ledger` as rows
//! of their own, and nets them out of the settlements. That is the default, and
//! every report that simply reads `v_ledger` gets it. Two readers need the ledger
//! without them: the analysis page when its "Haushaltsanteile einbeziehen" switch is off,
//! and the tax report always — somebody else's purchase is not a receipt of mine.
//! Both read `v_ledger_personal`, which is `v_ledger` with the household rows left
//! out and the settlements at their 0023 figure, under the same column names.
//!
//! The choice is a relation name rather than a predicate, so a query is written
//! once and the switch cannot be half-applied: forgetting the filter on the rows
//! while keeping the household-netted settlement figure would understate every
//! year by exactly what the flatmates paid for me.

/// The two ledgers a statistic can be computed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ledger {
    /// Bookings, plus my shares of the household's purchases paid by others, with
    /// settlements netted of both. What every statistic shows by default.
    WithHousehold,
    /// Bookings only, settlements netted only of my linked purchases' shares.
    Personal,
}

impl Ledger {
    /// From the `household` query parameter, which defaults to on: leaving it out is
    /// how every caller other than the analysis page's switch asks for the default.
    pub fn from_param(household: Option<bool>) -> Self {
        if household.unwrap_or(true) {
            Self::WithHousehold
        } else {
            Self::Personal
        }
    }

    /// The view to select from. Both carry the same columns.
    pub fn view(self) -> &'static str {
        match self {
            Self::WithHousehold => "v_ledger",
            Self::Personal => "v_ledger_personal",
        }
    }

    pub fn includes_household(self) -> bool {
        self == Self::WithHousehold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_household_is_included_unless_switched_off() {
        assert_eq!(Ledger::from_param(None), Ledger::WithHousehold);
        assert_eq!(Ledger::from_param(Some(true)), Ledger::WithHousehold);
        assert_eq!(Ledger::from_param(Some(false)), Ledger::Personal);
        assert_eq!(Ledger::WithHousehold.view(), "v_ledger");
        assert_eq!(Ledger::Personal.view(), "v_ledger_personal");
    }
}
