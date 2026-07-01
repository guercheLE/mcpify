//! Auth scheme discovery and profiling (architecture.md §1, step 3).
//! Full classification/prompt-fallback logic lands in Story 4; for now this
//! module only defines the shared descriptor type `GeneratorContext` depends on.

#[derive(Debug, Clone)]
#[allow(dead_code)] // ponytail: unused until Story 4 (classify.rs) constructs these
pub struct AuthSchemeDescriptor {
    pub name: String,
}
