use anyhow::{Context, Result};
use rust_embed::RustEmbed;
use tera::Tera;

/// Every `.tera` file under `templates/` compiles into the binary, so
/// generation never depends on the filesystem at runtime — required for
/// `cargo install mcpify` to remain a single, dependency-free binary.
/// Independent template set from `targets::rust::render::RsTemplates` and
/// `targets::typescript::render::TsTemplates` (architecture.md §3: a third
/// target is a third, independent template tree, not a shared one).
#[derive(RustEmbed)]
#[folder = "src/targets/python/templates/"]
struct PyTemplates;

/// Builds one `Tera` instance holding every embedded template, keyed by its
/// path relative to `templates/` (e.g. `"pyproject.toml.tera"`).
pub fn render_engine() -> Result<Tera> {
    let mut tera = Tera::default();

    for path in PyTemplates::iter() {
        if path.ends_with(".gitkeep") {
            continue;
        }
        let file = PyTemplates::get(&path).with_context(|| {
            format!("embedded template '{path}' vanished between iter() and get()")
        })?;
        let contents = std::str::from_utf8(&file.data)
            .with_context(|| format!("embedded template '{path}' is not valid UTF-8"))?;
        tera.add_raw_template(&path, contents)
            .with_context(|| format!("failed to parse template '{path}'"))?;
    }

    Ok(tera)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_without_error_even_with_no_real_templates_yet() {
        // Story P1 only stands up the engine; real `.tera` files land in
        // P2+. This just proves the embed+parse pipeline itself works.
        render_engine().unwrap();
    }
}
