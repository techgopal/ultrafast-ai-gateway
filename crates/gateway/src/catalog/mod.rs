//! The model catalog: what each provider offers and who may call it.

pub mod sync;

/// Longest model name, in characters.
pub const MAX_MODEL_NAME_CHARS: usize = 200;

/// A model name is the provider's model id verbatim: 1 to 200 characters,
/// no whitespace and no control character. `/`, `:`, `.`, `-`, `_` and `@`
/// are ordinary characters.
pub fn validate_model_name(name: &str) -> Result<(), &'static str> {
    let chars = name.chars().count();
    if chars == 0 || chars > MAX_MODEL_NAME_CHARS {
        return Err("name must be 1 to 200 characters");
    }
    if name.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("name must not contain whitespace or control characters");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_ids_are_valid_names() {
        for good in [
            "gpt-4o-2024-08-06",
            "models/gemini-2.0-flash",
            "meta-llama/Llama-3.3-70B",
            "llama3.2:3b",
            "claude-3-5-sonnet@20241022",
            "x",
        ] {
            assert!(validate_model_name(good).is_ok(), "{good}");
        }
        assert!(validate_model_name(&"é".repeat(200)).is_ok());
    }

    #[test]
    fn whitespace_control_and_length_are_refused() {
        for bad in [
            "",
            " a",
            "a b",
            "a\tb",
            "a\nb",
            "a\u{0}",
            "a\u{a0}b",
            &"x".repeat(201),
        ] {
            assert!(validate_model_name(bad).is_err(), "{bad:?}");
        }
    }
}
