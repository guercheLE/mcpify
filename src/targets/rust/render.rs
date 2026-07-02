use anyhow::{Context, Result};
use rust_embed::RustEmbed;
use tera::Tera;

/// Every `.tera` file under `templates/` compiles into the binary, so
/// generation never depends on the filesystem at runtime — required for
/// `cargo install mcpify` to remain a single, dependency-free binary.
/// Independent template set from `targets::typescript::render::TsTemplates`
/// (architecture.md §3: a second target is a second, independent template
/// tree, not a shared one).
#[derive(RustEmbed)]
#[folder = "src/targets/rust/templates/"]
struct RsTemplates;

/// Builds one `Tera` instance holding every embedded template, keyed by its
/// path relative to `templates/` (e.g. `"Cargo.toml.tera"`).
pub fn render_engine() -> Result<Tera> {
    let mut tera = Tera::default();

    for path in RsTemplates::iter() {
        let file = RsTemplates::get(&path).with_context(|| {
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
    fn loads_every_embedded_template_without_error() {
        // No `.tera` templates exist yet (Story R1 only builds the engine;
        // Story R2 adds the first real templates) — this just proves the
        // embed+parse pipeline itself doesn't error on an empty/near-empty
        // template set.
        render_engine().unwrap();
    }
}
