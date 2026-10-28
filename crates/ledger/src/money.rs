//! Money type: integer micro-units, always.
//!
//! A float used for money is a security finding (AGENTS.md) — binary
//! floating point cannot represent 0.1 exactly, and rounding drift in a
//! ledger compounds into missing cash. `MicroUsd` therefore wraps a plain
//! `i64` count of micro-dollars (10^-6 USD) and exposes NO conversion from
//! or to floats. The crate-level `#![deny(clippy::float_arithmetic)]`
//! (lib.rs) makes any future float arithmetic a hard compile error.

use crate::error::LedgerError;
use std::fmt;

/// An amount of money in integer micro-units (micro-USD). 1_000_000 = $1.
///
/// `Copy` (cheap 8-byte pass-by-value) and `Eq`/`Ord` (exact integer
/// comparisons — money never needs epsilon).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MicroUsd(i64);

impl MicroUsd {
    /// Wraps a raw micro-unit count. Accepts any i64 (zero and negatives
    /// are legal intermediate states for signed balances; POSTINGS validate
    /// positivity separately).
    pub const fn new(micros: i64) -> Self {
        Self(micros)
    }

    /// The raw micro-unit count.
    pub const fn micros(self) -> i64 {
        self.0
    }

    /// Checked addition — overflow is a programming error, surfaced as an
    /// error value instead of a panic in a server process.
    pub const fn checked_add(self, rhs: Self) -> Result<Self, LedgerError> {
        match self.0.checked_add(rhs.0) {
            Some(sum) => Ok(Self(sum)),
            None => Err(LedgerError::MoneyOverflow),
        }
    }

    /// Checked subtraction (see [`MicroUsd::checked_add`]).
    pub const fn checked_sub(self, rhs: Self) -> Result<Self, LedgerError> {
        match self.0.checked_sub(rhs.0) {
            Some(diff) => Ok(Self(diff)),
            None => Err(LedgerError::MoneyOverflow),
        }
    }
}

/// Renders as `12.345678 USD` using pure integer math — no float ever
/// touches the value (the obvious `micros as f64 / 1e6` would be the exact
/// bug this type exists to prevent).
impl fmt::Display for MicroUsd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let negative = self.0 < 0;
        let magnitude = self.0.unsigned_abs(); // u64: |i64::MIN| fits
        let whole = magnitude / 1_000_000;
        let frac = magnitude % 1_000_000;
        let sign = if negative { "-" } else { "" };
        write!(f, "{sign}{whole}.{frac:06} USD")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_uses_integer_math_only() {
        assert_eq!(MicroUsd::new(1_234_567).to_string(), "1.234567 USD");
        assert_eq!(MicroUsd::new(1_000_000).to_string(), "1.000000 USD");
        assert_eq!(MicroUsd::new(5).to_string(), "0.000005 USD");
        assert_eq!(MicroUsd::new(0).to_string(), "0.000000 USD");
        assert_eq!(MicroUsd::new(-250_000).to_string(), "-0.250000 USD");
        assert_eq!(
            MicroUsd::new(i64::MIN).to_string(),
            "-9223372036854.775808 USD"
        );
    }

    #[test]
    fn checked_ops_report_overflow_as_errors() {
        // matches! (pattern assert) instead of ==: LedgerError wraps
        // sqlx::Error in one variant and so has no PartialEq — errors are
        // for matching, not comparing.
        assert!(matches!(
            MicroUsd::new(i64::MAX).checked_add(MicroUsd::new(1)),
            Err(LedgerError::MoneyOverflow)
        ));
        assert!(matches!(
            MicroUsd::new(i64::MIN).checked_sub(MicroUsd::new(1)),
            Err(LedgerError::MoneyOverflow)
        ));
        assert_eq!(
            MicroUsd::new(10).checked_sub(MicroUsd::new(4)).unwrap(),
            MicroUsd::new(6)
        );
    }
}
