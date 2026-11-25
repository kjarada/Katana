//! DGN through the one executor and through MCP (docs/dgn.md): IMPORT and
//! EXPORT of a MicroStation V7 design as a person types them, the import as
//! one undo step, the shared scope, seed= and the refusals by name.
//!
//! The export's default seed designs are GDAL's data files (seed_2d.dgn and
//! seed_3d.dgn, where GDAL's own DGN writer finds them), so these cases need
//! GDAL's data folder, as the DXF cases of the GDAL driver once did;
//! `dgn_verbs::gdal_ships_the_seed_designs_the_export_writes_on` says so first
//! when it is missing.

#[path = "../support/geo_fixture.rs"]
pub mod geo_fixture;
#[path = "../support/mcp_client.rs"]
pub mod mcp_client;

mod dgn_verbs;
