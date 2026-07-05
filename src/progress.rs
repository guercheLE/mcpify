use std::sync::OnceLock;

static ENABLED: OnceLock<bool> = OnceLock::new();

/// Turns on progress output. Called once, at the very top of `main()` —
/// every other entry point (unit tests, the `tests/*.rs` integration
/// binaries, and any future library consumer) never calls this, so
/// `enabled()` defaults to `false` and mcpify stays exactly as quiet as it
/// is today unless run as the actual CLI binary.
pub fn init(enabled: bool) {
    let _ = ENABLED.set(enabled);
}

pub fn enabled() -> bool {
    ENABLED.get().copied().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_disabled_since_nothing_in_the_test_binary_calls_init() {
        assert!(!enabled());
    }
}
