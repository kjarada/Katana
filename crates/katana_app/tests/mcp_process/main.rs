//! The MCP server's cases that change or read what the process shares (step
//! 6, wave 2, agent C), from tests/app/test_mcp_server.cpp:
//!
//! ```text
//! mcp_server   McpServer: the 3 cases of a program's session (the customisation
//!              variables), the one that unsets KATANA_UPDATE_TOKEN, the one that
//!              moves the working directory, and the one that reads the real
//!              standard streams
//! ```
//!
//! gtest ran each case in a process of its own; Rust runs a binary's tests on
//! parallel threads of one, so each case here holds start_environment's
//! process lock for its whole length, and what it changed is put back before
//! it lets go. The file's other cases are in the target tests/mcp.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/app_support.rs"]
pub mod app_support;
#[path = "../support/geo_fixture.rs"]
pub mod geo_fixture;
#[path = "../support/mcp_client.rs"]
pub mod mcp_client;
#[path = "../support/start_environment.rs"]
pub mod start_environment;

mod mcp_server;
