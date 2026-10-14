//! Data plane: OpenAI-compatible proxy, pool-key auth, streaming, metering.
//! Placeholder crate — implemented from M1 onward.

/// Returns the crate name, proving the workspace member builds and its tests run.
pub fn crate_name() -> &'static str {
    "tributary-gateway"
}

#[cfg(test)]
mod tests {
    use super::crate_name;

    #[test]
    fn hello_world() {
        assert_eq!(crate_name(), "tributary-gateway");
    }
}
