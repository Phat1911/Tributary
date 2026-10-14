//! Control plane: REST admin API, auth, sessions.
//! Placeholder crate — implemented from M2 onward.

/// Returns the crate name, proving the workspace member builds and its tests run.
pub fn crate_name() -> &'static str {
    "tributary-control"
}

#[cfg(test)]
mod tests {
    use super::crate_name;

    #[test]
    fn hello_world() {
        assert_eq!(crate_name(), "tributary-control");
    }
}
