//! Per-step generation logic for the Python target, one submodule per
//! `McpServerTargetGenerator` method — mirrors `targets::rust::steps` /
//! `targets::typescript::steps`.

pub mod auth;
pub mod bootstrap;
pub mod enterprise;
pub mod run_tests;
pub mod setup_and_tests;
pub mod tools;
pub mod transports;
