//! Shared by the katana_interop modules, never public
//! (src/katana_interop/table_reprojection.hpp): a feature table moved from
//! one coordinate system to another, for IMPORT's crs=project and EXPORT's
//! crs=<code> alike (docs/interop.md, "Import options", "Export options"), so
//! the two directions move a table the one way.

use katana_core::Status;
use katana_gis::gdal_adapter::VectorFeature;
use katana_gis::processing::FeatureTable;
use katana_gis::reproject::reproject_features;

/// Every part of every feature of `table` from `from` to `to`, by the one
/// reprojection (reproject_features), heights untouched. An arc's three
/// points move with the rest; a conformal projection keeps an arc that small
/// round to far below a millimetre. The table's own CRS is left for the
/// caller to set: it may want the text it was given or its WKT.
pub(crate) fn reproject_table(table: &mut FeatureTable, from: &str, to: &str) -> Status {
    let mut geometries = Vec::new();
    for feature in &table.features {
        for part in &feature.parts {
            geometries.push(VectorFeature {
                geometry: part.clone(),
                ..VectorFeature::default()
            });
        }
    }
    if geometries.is_empty() {
        return Ok(());
    }
    let mut moved = reproject_features(geometries, from, to)?.into_iter();
    for feature in &mut table.features {
        for part in &mut feature.parts {
            // One moved geometry was made for each part, in this order.
            if let Some(next) = moved.next() {
                *part = next.geometry;
            }
        }
    }
    Ok(())
}
