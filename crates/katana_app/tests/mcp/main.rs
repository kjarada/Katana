//! The MCP server and its tools (step 6, wave 2, agent C): the cases of these
//! C++ files that drive katana_app::mcp, one module per C++ file:
//!
//! ```text
//! mcp_server    tests/app/test_mcp_server.cpp    McpServer (95 of 101)
//! update_verbs  tests/app/test_update_verbs.cpp  UpdateVerbs (the 3 of check_update_reply)
//! view_scale    tests/app/test_view_scale.cpp    ViewScaleMcp
//! sql_verb      tests/geo/test_sql_verb.cpp      GisQueryMcp
//! dem_verbs     tests/geo/test_dem_verbs.cpp     DemSession
//! grid_verb     tests/geo/test_grid_verb.cpp     GridSession
//! ```
//!
//! The six McpServer cases that set an environment variable, change the
//! working directory or read the real streams are in tests/mcp_process,
//! behind one lock. libtest runs these on parallel threads of one process,
//! where ctest ran each in its own: every scratch folder is named for its
//! case and the process, each case has a session and a server of its own,
//! and the cases that run GDAL on a file of tests/geo/data or samples/gis
//! take the target's one lock (geo_fixture's `shared_data`).

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
#[path = "../../../katana_update/tests/test_key/mod.rs"]
pub mod test_key;
#[path = "../support/update_rig.rs"]
pub mod update_rig;

mod dem_verbs;
mod grid_verb;
mod mcp_server;
mod sql_verb;
mod update_verbs;
mod view_scale;
