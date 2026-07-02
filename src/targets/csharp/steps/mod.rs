//! Per-step generation logic for the C# target, one submodule per
//! `McpServerTargetGenerator` method — mirrors `targets::python::steps` /
//! `targets::rust::steps`. Stubbed out in Story C1; each submodule gets a
//! real implementation in its own story (C2-C8).

pub mod auth;
pub mod bootstrap;
pub mod enterprise;
pub mod run_tests;
pub mod setup_and_tests;
pub mod tools;
pub mod transports;
