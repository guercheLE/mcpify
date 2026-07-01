//! Case-conversion helpers used pervasively across the TypeScript templates
//! (project/package names, class names, env var prefixes). Thin wrappers
//! over `heck` so every template context goes through one place.

use heck::{ToKebabCase, ToLowerCamelCase, ToPascalCase, ToShoutySnakeCase};

pub fn kebab_case(input: &str) -> String {
    input.to_kebab_case()
}

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
        assert_eq!(kebab_case(title), "bitbucket-data-center-api");
        assert_eq!(pascal_case(title), "BitbucketDataCenterApi");
        assert_eq!(camel_case(title), "bitbucketDataCenterApi");
        assert_eq!(screaming_snake_case(title), "BITBUCKET_DATA_CENTER_API");
    }

    #[test]
    fn handles_already_kebab_case_input() {
        assert_eq!(kebab_case("my-api-mcp"), "my-api-mcp");
    }
}
