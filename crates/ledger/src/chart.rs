//! Chart of accounts: the fixed taxonomy every ledger account belongs to.
//!
//! The chart is NORMATIVE (SPEC §6): enum codes, never freeform names. The
//! seed below is mirrored row-for-row by migration `0015_ledger.sql`, and a
//! property test fails if the two drift in either direction. Books are kept
//! from the platform-operator's perspective — the operator is the
//! accounting entity, renters and depositors are counterparties it owes.
//!
//! Rule-7 doctrine (director rulings 2026-10-28, recorded in SPEC D38):
//! Σ payable = custodial-cash debts only (payable.depositor + prepaid.renter);
//! contributed principal lives in claims.contributed OUTSIDE the equation,
//! and the operator fee lands in equity.operator, not a payable.

use crate::error::LedgerError;
use std::str::FromStr;

/// The five classical account types. The type DETERMINES the normal balance
/// (which side increases the account) — see [`normal_balance`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccountType {
    Asset,
    Liability,
    Equity,
    Revenue,
    Expense,
}

impl AccountType {
    /// Stable string form used in the `chart_of_accounts.account_type`
    /// column (and nowhere else — parse/serialize in one place).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Asset => "asset",
            Self::Liability => "liability",
            Self::Equity => "equity",
            Self::Revenue => "revenue",
            Self::Expense => "expense",
        }
    }
}

impl FromStr for AccountType {
    type Err = LedgerError;

    /// Parses the DB column value back into the typed enum. An unknown
    /// string is chart drift and fails loudly (never a fallback type).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "asset" => Ok(Self::Asset),
            "liability" => Ok(Self::Liability),
            "equity" => Ok(Self::Equity),
            "revenue" => Ok(Self::Revenue),
            "expense" => Ok(Self::Expense),
            other => Err(LedgerError::UnknownChartCode(other.to_string())),
        }
    }
}

/// Which side of a T-account increases the balance. The classic mnemonic:
/// assets/expenses live on the left (debit); liabilities/equity/revenue on
/// the right (credit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Debit,
    Credit,
}

impl Direction {
    /// String form for the `ledger_entries.direction` column.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Debit => "debit",
            Self::Credit => "credit",
        }
    }
}

/// The normal balance (the side that makes the account GROW) for a type —
/// the foundation of the "normal balances" invariant: an account sitting on
/// the wrong side is a misposting, and property tests fail loudly on it.
pub const fn normal_balance(account_type: AccountType) -> Direction {
    match account_type {
        AccountType::Asset | AccountType::Expense => Direction::Debit,
        AccountType::Liability | AccountType::Equity | AccountType::Revenue => Direction::Credit,
    }
}

/// Who an account belongs to (the `ledger_accounts.owner_kind` column).
/// Platform-level accounts (cash.escrow, revenue.usage, equity.operator)
/// use [`OwnerKind::Operator`] with owner id `PLATFORM_OWNER_ID`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OwnerKind {
    Renter,
    Depositor,
    Org,
    Operator,
}

impl OwnerKind {
    /// String form for the DB column.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Renter => "renter",
            Self::Depositor => "depositor",
            Self::Org => "org",
            Self::Operator => "operator",
        }
    }
}

/// The single owner id used for operator-level (platform) accounts.
pub const PLATFORM_OWNER_ID: &str = "platform";

/// Every account in the books, as a typed enum — postings reference these,
/// so a typo is a compile error, not a silent new account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccountCode {
    CashEscrow,
    CapacityContributed,
    PrepaidRenter,
    PayableDepositor,
    ClaimsContributed,
    EquityOperator,
    RevenueUsage,
}

impl AccountCode {
    /// String form matching the `chart_of_accounts.code` seed exactly.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CashEscrow => "cash.escrow",
            Self::CapacityContributed => "capacity.contributed",
            Self::PrepaidRenter => "prepaid.renter",
            Self::PayableDepositor => "payable.depositor",
            Self::ClaimsContributed => "claims.contributed",
            Self::EquityOperator => "equity.operator",
            Self::RevenueUsage => "revenue.usage",
        }
    }

    /// The account's type in the normative chart (kept in code so
    /// validation needs no DB round-trip; the seed test pins DB == code).
    pub const fn account_type(self) -> AccountType {
        match self {
            Self::CashEscrow | Self::CapacityContributed => AccountType::Asset,
            Self::PrepaidRenter | Self::PayableDepositor | Self::ClaimsContributed => {
                AccountType::Liability
            }
            Self::EquityOperator => AccountType::Equity,
            Self::RevenueUsage => AccountType::Revenue,
        }
    }
}

impl FromStr for AccountCode {
    type Err = LedgerError;

    /// Parses a `chart_of_accounts.code` value. Unknown ⇒ drift ⇒ loud
    /// error (same doctrine as [`AccountType::from_str`]).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cash.escrow" => Ok(Self::CashEscrow),
            "capacity.contributed" => Ok(Self::CapacityContributed),
            "prepaid.renter" => Ok(Self::PrepaidRenter),
            "payable.depositor" => Ok(Self::PayableDepositor),
            "claims.contributed" => Ok(Self::ClaimsContributed),
            "equity.operator" => Ok(Self::EquityOperator),
            "revenue.usage" => Ok(Self::RevenueUsage),
            other => Err(LedgerError::UnknownChartCode(other.to_string())),
        }
    }
}

/// One normative chart row: an account's definition plus the counterpart
/// accounts it may face inside a posting-rule leg pair. Counterparties are
/// defense in depth — templates are Rust consts, and the chart
/// independently forbids pairs nobody has blessed.
#[derive(Debug, Clone, Copy)]
pub struct ChartRow {
    pub code: AccountCode,
    pub account_type: AccountType,
    pub definition: &'static str,
    pub allowed_counterparties: &'static [AccountCode],
}

/// The normative seed — mirrored exactly by `migrations/0015_ledger.sql`.
/// Drift in either direction fails `properties.rs::chart_seed_matches_code`.
pub const SEED: &[ChartRow] = &[
    ChartRow {
        code: AccountCode::CashEscrow,
        account_type: AccountType::Asset,
        definition: "Custodial cash backing renter prepaid balances and settled \
                     depositor payables. The rule-7 numerator: cash.escrow >= Σ payable + Σ prepaid.",
        allowed_counterparties: &[AccountCode::PrepaidRenter, AccountCode::PayableDepositor],
    },
    ChartRow {
        code: AccountCode::CapacityContributed,
        account_type: AccountType::Asset,
        definition: "Credited capacity value of depositor-contributed provider keys; \
                     backs claims.contributed. Key-death markdown lands with M9.",
        allowed_counterparties: &[AccountCode::ClaimsContributed],
    },
    ChartRow {
        code: AccountCode::PrepaidRenter,
        account_type: AccountType::Liability,
        definition: "Renter custodial prepaid balances — the rule-7 Σ prepaid term. \
                     Consumed by metered usage; never revenue until earned (rule 7).",
        allowed_counterparties: &[AccountCode::CashEscrow, AccountCode::RevenueUsage],
    },
    ChartRow {
        code: AccountCode::PayableDepositor,
        account_type: AccountType::Liability,
        definition: "Settled depositor earnings owed in cash — the rule-7 Σ payable term \
                     (custodial-cash debts only, director ruling 2026-10-28).",
        allowed_counterparties: &[AccountCode::RevenueUsage, AccountCode::CashEscrow],
    },
    ChartRow {
        code: AccountCode::ClaimsContributed,
        account_type: AccountType::Liability,
        definition: "Depositor contributed-principal claims — capacity-backed, NOT cash-backed; \
                     deliberately outside rule 7, governed by the §11 withdrawal liquidity \
                     buffer (director ruling 2026-10-28).",
        allowed_counterparties: &[AccountCode::CapacityContributed],
    },
    ChartRow {
        code: AccountCode::EquityOperator,
        account_type: AccountType::Equity,
        definition: "Operator capital and fees taken. The operator's own claim on the books — \
                     shares are ledger claims, never tokens (rule 9).",
        allowed_counterparties: &[AccountCode::RevenueUsage],
    },
    ChartRow {
        code: AccountCode::RevenueUsage,
        account_type: AccountType::Revenue,
        definition: "Metered pool usage revenue. At settlement splits into the operator fee \
                     (→ equity.operator) and the distributable (→ payable.depositor).",
        allowed_counterparties: &[
            AccountCode::PrepaidRenter,
            AccountCode::EquityOperator,
            AccountCode::PayableDepositor,
        ],
    },
];

/// Looks up a chart row by code (the normative seed, not the DB — the
/// property test pins them equal).
pub fn row(code: AccountCode) -> &'static ChartRow {
    // SEED is a compile-time constant list; a missing row is a bug the
    // seed-completeness unit test catches, so the unwrap here is backed
    // by a test that runs on every CI build.
    SEED.iter().find(|r| r.code == code).expect("seed row")
}

/// Parses the `chart_of_accounts.allowed_counterparties` column: a strict
/// `["a","b"]` string array. Hand-rolled on purpose — the ledger takes no
/// serde dependency, and a strict parser turns malformed seeds into loud
/// errors instead of permissively skipped entries.
pub fn parse_counterparties(raw: &str) -> Result<Vec<AccountCode>, LedgerError> {
    let inner = raw.trim();
    let body = inner
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| {
            LedgerError::UnknownChartCode(format!("allowed_counterparties not a [..] array: {raw}"))
        })?;
    if body.trim().is_empty() {
        return Ok(Vec::new());
    }
    body.split(',')
        .map(|item| {
            let item = item.trim();
            let quoted = item
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .ok_or_else(|| {
                    LedgerError::UnknownChartCode(format!(
                        "counterparty entry not a quoted string: {item}"
                    ))
                })?;
            AccountCode::from_str(quoted)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_balances_follow_the_classical_mapping() {
        assert_eq!(normal_balance(AccountType::Asset), Direction::Debit);
        assert_eq!(normal_balance(AccountType::Expense), Direction::Debit);
        assert_eq!(normal_balance(AccountType::Liability), Direction::Credit);
        assert_eq!(normal_balance(AccountType::Equity), Direction::Credit);
        assert_eq!(normal_balance(AccountType::Revenue), Direction::Credit);
    }

    #[test]
    fn seed_covers_every_account_code_exactly_once() {
        let all = [
            AccountCode::CashEscrow,
            AccountCode::CapacityContributed,
            AccountCode::PrepaidRenter,
            AccountCode::PayableDepositor,
            AccountCode::ClaimsContributed,
            AccountCode::EquityOperator,
            AccountCode::RevenueUsage,
        ];
        for code in all {
            // row() panics if a code is missing from SEED.
            assert_eq!(row(code).code, code);
            // Each account's declared type matches its own enum mapping.
            assert_eq!(row(code).account_type, code.account_type());
        }
        assert_eq!(SEED.len(), all.len());
    }

    #[test]
    fn counterparty_arrays_parse_strictly() {
        let codes = parse_counterparties("[\"cash.escrow\",\"payable.depositor\"]").unwrap();
        assert_eq!(
            codes,
            vec![AccountCode::CashEscrow, AccountCode::PayableDepositor]
        );
        assert!(parse_counterparties("[]").unwrap().is_empty());
        // Malformed inputs must be LOUD, never lenient:
        assert!(parse_counterparties("cash.escrow").is_err());
        assert!(parse_counterparties("[cash.escrow]").is_err());
        assert!(parse_counterparties("[\"no.such.code\"]").is_err());
    }

    #[test]
    fn unknown_chart_codes_fail_loudly() {
        assert!(AccountCode::from_str("cash.pocket").is_err());
        assert!(AccountType::from_str("misc").is_err());
    }
}
