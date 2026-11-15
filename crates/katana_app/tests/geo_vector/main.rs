//! The GIS vector verbs (step 6, wave 2, agent V), one module per C++ test
//! file, each verb driven through the one executor as the window and the
//! session drive it:
//!
//! ```text
//! buffer_and_dissolve  tests/geo/test_buffer_and_dissolve.cpp  GisBuffer, GisDissolve, GisBufferSession
//! check_verbs          tests/geo/test_check_verbs.cpp          GisCheck, GisCheckSession
//! hull_and_clip        tests/geo/test_hull_and_clip.cpp        GisHull, GisClip
//! overlay_verb         tests/geo/test_overlay_verb.cpp         GisOverlay
//! sql_verb             tests/geo/test_sql_verb.cpp             GisSql
//! drawing_curves       tests/geo/test_drawing_curves.cpp       DrawingCurves (6 of its 14)
//! ```
//!
//! The files' other suites are elsewhere: GeoLineWord is G1's
//! (tests/geo_frame), the *Contract suites step 5's (katana_gis), GisQueryMcp
//! the MCP server's (C), and the eight DrawingCurves cases of the one
//! conversion and the surface input step 5's (katana_interop). The six
//! DrawingCurves cases here drive DRAPE, RASTER ZONAL, EXPORT and INFO as well
//! as GIS CLIP and GIS DISSOLVE: they pass when step 6's wave 2 is integrated
//! as one. Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects.
//!
//! libtest runs these cases on parallel threads of one process, where ctest
//! ran each in its own: every scratch folder is named for its case and the
//! process, and the cases that run GDAL on a file of tests/geo/data take the
//! target's one lock (geo_fixture's `shared_data`), so GDAL's .aux.xml
//! sidecars never race. No input or expected value is changed for it.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/geo_fixture.rs"]
pub mod geo_fixture;

mod buffer_and_dissolve;
mod check_verbs;
mod drawing_curves;
mod hull_and_clip;
mod overlay_verb;
mod sql_verb;

use katana_app::geo::replies::{Record, parse_records};
use katana_app::{Captured, Session};
use katana_entity::entity::property_to_string;
use katana_entity::{Entity, PropertyMap, PropertyValue};

use crate::geo_fixture::GeoFixture;

/// The C++'s VectorFixture: its scratch folder named after the test.
pub fn fixture(name: &str) -> GeoFixture {
    GeoFixture::new("katana-geo-vector-", name)
}

/// A property map from its pairs, as the C++'s PropertyMap{{...}} lists them.
pub fn props<const N: usize>(pairs: [(&str, PropertyValue); N]) -> PropertyMap {
    pairs
        .into_iter()
        .map(|(key, value)| (String::from(key), value))
        .collect()
}

/// The C++ files' `text(entity, key)`: a property as it reads, or "<none>".
pub fn text(entity: &Entity, key: &str) -> String {
    entity
        .properties
        .get(key)
        .map_or_else(|| String::from("<none>"), property_to_string)
}

/// The records of `kind` in a reply, in order: the C++ fixtures' rows() and
/// problems().
pub fn records_of(reply: &str, kind: &str) -> Vec<Record> {
    parse_records(reply)
        .into_iter()
        .filter(|record| record.kind == kind)
        .collect()
}

/// EXPECT_DOUBLE_EQ: within 4 units in the last place.
#[track_caller]
pub fn double_eq(actual: f64, expected: f64) {
    fn biased(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1 << 63) != 0 {
            (!bits).wrapping_add(1)
        } else {
            bits | (1 << 63)
        }
    }
    assert!(
        !actual.is_nan() && !expected.is_nan() && biased(actual).abs_diff(biased(expected)) <= 4,
        "actual={actual:?}, expected={expected:?}"
    );
}

/// The C++'s `katana::app::Session session(nullptr)`, with the capture each
/// line is run with: katana_cli's and katana_mcp's path, the session asking
/// the executor.
pub struct CliSession {
    pub session: Session,
    pub console: Captured,
}

impl CliSession {
    pub fn start() -> Self {
        let mut console = Captured::default();
        let session = Session::new(None, &mut console);
        Self { session, console }
    }

    /// `session.run(line)`: whether the line worked.
    pub fn run(&mut self, line: &str) -> bool {
        self.session.run(line, &mut self.console)
    }
}
