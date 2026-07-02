//! Case-conversion helpers used pervasively across the Go templates
//! (exported type/function names, unexported field/local names, env var
//! prefixes). Thin wrappers over `heck` so every template context goes
//! through one place — mirrors `targets::csharp::naming`: Go is also a
//! two-case system, `PascalCase` for identifiers Go's compiler exports
//! across package boundaries (an uppercase first letter is what makes an
//! identifier exported in Go, not a keyword or annotation) and
//! `camelCase` for everything unexported (locals, struct fields not meant
//! for external packages, parameters). Go's own `internal/` package
//! convention (used pervasively by this target, see G2) is a second,
//! orthogonal visibility mechanism at the package level, layered on top
//! of this identifier-level rule rather than replacing it.

use heck::{ToLowerCamelCase, ToPascalCase, ToShoutySnakeCase};

pub fn pascal_case(input: &str) -> String {
    input.to_pascal_case()
}

pub fn camel_case(input: &str) -> String {
    input.to_lower_camel_case()
}

pub fn screaming_snake_case(input: &str) -> String {
    input.to_shouty_snake_case()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_a_human_title_to_every_case() {
        let title = "Bitbucket Data Center API";
        assert_eq!(pascal_case(title), "BitbucketDataCenterApi");
        assert_eq!(camel_case(title), "bitbucketDataCenterApi");
        assert_eq!(screaming_snake_case(title), "BITBUCKET_DATA_CENTER_API");
    }

    #[test]
    fn handles_already_pascal_case_input() {
        assert_eq!(pascal_case("MyApiMcp"), "MyApiMcp");
    }

    #[test]
    fn converts_kebab_case_input_to_pascal_and_camel_case() {
        assert_eq!(pascal_case("my-api-mcp"), "MyApiMcp");
        assert_eq!(camel_case("my-api-mcp"), "myApiMcp");
    }
}
