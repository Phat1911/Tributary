//! Double-entry ledger: `LedgerTrait`, DB ledger, chart of accounts.
//! Placeholder crate — implemented from M7 onward.

/// Returns the crate name, proving the workspace member builds and its tests run.
pub fn crate_name() -> &'static str {
    "tributary-ledger"
}

#[cfg(test)]
mod tests {
    use super::crate_name;

    #[test]
    fn hello_world() {
        assert_eq!(crate_name(), "tributary-ledger");
    }
}
