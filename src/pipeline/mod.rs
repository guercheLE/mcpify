pub mod dir_guard;

// Full shared-pipeline orchestration (ingest -> dir_guard -> profile_auth ->
// mcp_store.db assembly) lands in Story 6; each stage is exercised on its
// own until then.
