//! Shared types, errors, config.
//! Placeholder crate — grows as the other crates extract common code.

/// Returns the crate name, proving the workspace member builds and its tests run.
pub fn crate_name() -> &'static str {
    "tributary-common"
}

#[cfg(test)]
mod tests {
    use super::crate_name;

    #[test]
    fn hello_world() {
        assert_eq!(crate_name(), "tributary-common");
    }
}
