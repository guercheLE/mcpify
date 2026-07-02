//! Case-conversion helpers used pervasively across the Python templates
//! (package/module names, class names, env var prefixes). Thin wrappers
//! over `heck` so every template context goes through one place — mirrors
//! `targets::rust::naming`: only the two cases Python identifiers actually
//! use (`snake_case` for modules/functions, `PascalCase` for classes), no
//! camelCase convention to support here either.

use heck::{ToPascalCase, ToShoutySnakeCase, ToSnakeCase};

pub fn snake_case(input: &str) -> String {
    input.to_snake_case()
}

pub fn pascal_case(input: &str) -> String {
    input.to_pascal_case()
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
        assert_eq!(snake_case(title), "bitbucket_data_center_api");
        assert_eq!(pascal_case(title), "BitbucketDataCenterApi");
        assert_eq!(screaming_snake_case(title), "BITBUCKET_DATA_CENTER_API");
    }

    #[test]
    fn handles_already_snake_case_input() {
        assert_eq!(snake_case("my_api_mcp"), "my_api_mcp");
    }

    #[test]
    fn converts_kebab_case_input_to_snake_case() {
        assert_eq!(snake_case("my-api-mcp"), "my_api_mcp");
    }
}
