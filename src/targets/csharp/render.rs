use anyhow::{Context, Result};
use rust_embed::RustEmbed;
use tera::Tera;

/// Every `.tera` file under `templates/` compiles into the binary, so
/// generation never depends on the filesystem at runtime — required for
/// `cargo install mcpify` to remain a single, dependency-free binary.
/// Independent template set from `targets::python::render::PyTemplates`,
/// `targets::rust::render::RsTemplates`, and
/// `targets::typescript::render::TsTemplates` (architecture.md §3: a
/// fourth target is a fourth, independent template tree, not a shared
/// one).
#[derive(RustEmbed)]
#[folder = "src/targets/csharp/templates/"]
struct CsTemplates;

/// Builds one `Tera` instance holding every embedded template, keyed by its
/// path relative to `templates/` (e.g. `"Program.cs.tera"`).
pub fn render_engine() -> Result<Tera> {
    let mut tera = Tera::default();

    for path in CsTemplates::iter() {
        let file = CsTemplates::get(&path).with_context(|| {
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

    // Doesn't assert on a specific embedded template name (unlike the other
    // three targets' equivalent test) since C1 stands up the render/emit
    // pair before any real `.tera` files exist under `templates/` — C2 adds
    // `Program.cs.tera` and onward. Once C2 lands, tighten this the same
    // way `targets::python::render`'s test does.
    #[test]
    fn loads_every_embedded_template_without_error() {
        assert!(render_engine().is_ok());
    }
}
