//! Double-entry ledger for Tributary V2 (SPEC §6, §11; M7).
//!
//! The ledger is V2's truth spine and ships DARK in M7: nothing in V1
//! reads or writes it, and no request-path behavior changes. It exists so
//! that when real value enters (M10+ renter prepayments, depositor
//! settlements), the accounting is already standing: a fixed chart of
//! accounts, posting-rule-only entry creation, database-enforced
//! append-only history (rule 3), and property-tested invariants.
//!
//! The three invariants this crate refuses to compromise:
//! 1. Every transaction balances (Σdebits = Σcredits) — templates are
//!    balanced by construction; the DB is burglar-tested to agree.
//! 2. Balances are cached derivatives; entries are truth. Drift alarms
//!    ([`Ledger::reconcile`]), never silently re-caches.
//! 3. Money is integer micro-units ([`MicroUsd`]) — never a float.

// The money rule as a compile-time burglar alarm: binary floats cannot
// represent most decimal values exactly, and float drift in a ledger is a
// security finding (AGENTS.md). Any float arithmetic anywhere in this
// crate now fails the build.
#![deny(clippy::float_arithmetic)]

pub mod chart;
pub mod error;
pub mod ledger;
pub mod money;
pub mod posting;

pub use chart::{AccountCode, AccountType, Direction, OwnerKind};
pub use error::LedgerError;
pub use ledger::{AccountBalance, Ledger, PostingReceipt, SolvencyReport, SqlLedger, TrialBalance};
pub use money::MicroUsd;
pub use posting::{PostingRequest, TemplateCode};
