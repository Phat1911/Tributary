//! Ledger error taxonomy. Every variant is a LOUD failure by design: a
//! ledger that "warns and continues" is how books drift silently.

/// Anything the ledger refuses to do quietly.
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    /// No `ledger_accounts` row matches the requested chart code + owner.
    /// The account must be opened (via the trait) before postings touch it.
    #[error("ledger account not found: {code} for {owner_kind}:{owner_id}")]
    AccountNotFound {
        code: String,
        owner_kind: String,
        owner_id: String,
    },
    /// The database chart contains a code this build does not know — the
    /// normative seed (migration) and the `AccountCode` enum drifted.
    /// Failing loudly is the point; entries must never post against a
    /// half-understood chart.
    #[error("chart drift: code {0:?} exists in the DB but not in this build's AccountCode enum")]
    UnknownChartCode(String),
    /// A posting named a template code that has no definition.
    #[error("posting-rule template unknown: {0}")]
    UnknownTemplate(String),
    /// The template needs an owner slot (renter/depositor) the request did
    /// not supply, or supplied one it does not use — both are caller bugs.
    #[error("posting {template} owner-slot mismatch: {problem}")]
    OwnerSlot { template: String, problem: String },
    /// A leg would push an account past its normal balance (overdraw a
    /// prepaid, distribute unearned revenue, …). The conditional cache
    /// update refused the row; the transaction rolled back.
    #[error(
        "insufficient normal-side balance on {account}: need {needed} micros, have {available} micros"
    )]
    Insufficient {
        account: String,
        needed: i64,
        available: i64,
    },
    /// The balances cache disagrees with the entries (the truth). The
    /// account names are carried so the nightly job can page with specifics.
    #[error(
        "balance cache DRIFT on accounts {accounts:?} — entries are truth; recompute before any further posting"
    )]
    BalanceDrift { accounts: Vec<String> },
    /// Entries themselves do not balance (Σdebits ≠ Σcredits). With
    /// posting-rule-only creation this can only mean corruption or a raw
    /// INSERT bypass — both are High-priority events.
    #[error(
        "TRIAL BALANCE VIOLATION: Σdebits {debits} != Σcredits {credits} — entries are corrupt or were bypassed"
    )]
    TrialBalanceViolation { debits: i64, credits: i64 },
    /// Rule 7 (SPEC §11): cash:escrow no longer covers custodial debts.
    /// A violation is a High finding by definition — never a warning.
    #[error(
        "SOLVENCY VIOLATION (rule 7): cash:escrow {escrow} < Σ payable {payable} + Σ prepaid {prepaid} (short by {short} micros) — High finding (SPEC §11)"
    )]
    SolvencyViolated {
        escrow: i64,
        payable: i64,
        prepaid: i64,
        short: i64,
    },
    /// Integer overflow in money math (i64 micro-units is ~9.2 quadrillion
    /// dollars; hitting this means a bug, not wealth).
    #[error("money overflow: i64 micro-units exceeded")]
    MoneyOverflow,
    /// Underlying database failure (connection, constraint, …).
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}
