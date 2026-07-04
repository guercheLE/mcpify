//! Per-step generation logic for the Rust target, one submodule per
//! `McpServerTargetGenerator` method — mirrors
//! `targets::typescript::steps`.

pub mod auth;
pub mod bootstrap;
pub mod enterprise;
pub mod run_tests;
pub mod setup_and_tests;
pub mod tools;
pub mod transports;
pub mod versions;
