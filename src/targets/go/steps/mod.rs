//! Per-step generation logic for the Go target, one submodule per
//! `McpServerTargetGenerator` method — mirrors `targets::csharp::steps` /
//! `targets::python::steps`. Stubbed out in Story G1; each submodule gets a
//! real implementation in its own story (G2-G8).

pub mod auth;
pub mod bootstrap;
pub mod enterprise;
pub mod run_tests;
pub mod setup_and_tests;
pub mod tools;
pub mod transports;
