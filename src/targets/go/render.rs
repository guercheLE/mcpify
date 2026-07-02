use anyhow::{Context, Result};
use rust_embed::RustEmbed;
use tera::Tera;

/// Every `.tera` file under `templates/` compiles into the binary, so
/// generation never depends on the filesystem at runtime — required for
/// `cargo install mcpify` to remain a single, dependency-free binary.
/// Independent template set from `targets::csharp::render::CsTemplates` and
/// every other target's equivalent (architecture.md §3: a fifth target is a
/// fifth, independent template tree, not a shared one).
#[derive(RustEmbed)]
#[folder = "src/targets/go/templates/"]
struct GoTemplates;

/// Builds one `Tera` instance holding every embedded template, keyed by its
/// path relative to `templates/` (e.g. `"go.mod.tera"`).
pub fn render_engine() -> Result<Tera> {
    let mut tera = Tera::default();

    for path in GoTemplates::iter() {
        let file = GoTemplates::get(&path).with_context(|| {
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
    // four targets' equivalent test) since G1 stands up the render/emit
    // pair before any real `.tera` files exist under `templates/` — G2 adds
    // `go.mod.tera` and onward. Once G2 lands, tighten this the same way
    // `targets::csharp::render`'s test does.
    #[test]
    fn loads_every_embedded_template_without_error() {
        assert!(render_engine().is_ok());
    }
}
