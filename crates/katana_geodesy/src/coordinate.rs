//! Coordinate value types of the geodesy layer.
//!
//! AXIS-ORDER CONVENTION (the classic PROJ / EPSG pitfall, fixed here once)
//!
//! * Prefer the two NAMED types. A latitude can never be mistaken for a
//!   longitude, or a northing for an easting, when the field has a name:
//!
//!   ```text
//!   GeographicCoordinate { latitude, longitude, height }   angles in DEGREES
//!   ProjectedCoordinate { easting, northing, height }      CRS axis unit
//!   ```
//!
//!   Degrees are decimal degrees, north and east positive. Eastings and
//!   northings are in the linear unit of the CRS (metres, feet, US survey
//!   feet ... see `CoordinateReferenceSystem::horizontal_unit_name`); they
//!   are NOT silently converted to metres.
//!
//! * `Coordinate { x, y, z, t }` is the generic escape hatch (geocentric XYZ,
//!   bulk arrays). Its order is ALWAYS "traditional GIS order":
//!
//!   ```text
//!   geographic CRS : x = longitude, y = latitude   (CRS angular unit)
//!   projected  CRS : x = easting,   y = northing   (CRS linear unit)
//!   geocentric CRS : x = X, y = Y, z = Z           (metres)
//!   ```
//!
//!   regardless of the axis order the authority declares (EPSG:4326 is
//!   officially latitude-first; Katana still uses x = longitude). This is
//!   obtained from PROJ with proj_normalize_for_visualization(). The
//!   normalisation reorders axes; it never flips their direction: for the
//!   rare westing/southing systems (e.g. South African Lo grids) x carries
//!   the westing and y the southing, positive as the CRS defines them.
//!
//! * Heights travel in `height` / `z` in the unit of the CRS vertical axis and
//!   are only transformed when both CRSs are three-dimensional; with 2D CRSs
//!   they pass through unchanged.

/// Value of `Coordinate::t` meaning "no coordinate epoch given". PROJ treats an
/// infinite time as unspecified; a literal 0 would be the year 0 and would make
/// a time-dependent (plate-motion) transformation drift by two millennia.
pub const UNSPECIFIED_EPOCH: f64 = f64::INFINITY;

/// Position on an ellipsoid. Decimal degrees, north/east positive.
///
/// `repr(C)`: the batch transformation hands PROJ interior pointers into a
/// slice of these, with the size of one as the stride.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct GeographicCoordinate {
    pub latitude: f64,
    pub longitude: f64,
    /// Above the ellipsoid (3D CRS) or passed through (2D CRS).
    pub height: f64,
}

impl GeographicCoordinate {
    pub const fn new(latitude: f64, longitude: f64, height: f64) -> Self {
        Self {
            latitude,
            longitude,
            height,
        }
    }
}

/// Position on a map grid, in the linear unit of the projected CRS.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct ProjectedCoordinate {
    pub easting: f64,
    pub northing: f64,
    pub height: f64,
}

impl ProjectedCoordinate {
    pub const fn new(easting: f64, northing: f64, height: f64) -> Self {
        Self {
            easting,
            northing,
            height,
        }
    }
}

/// Generic coordinate tuple in traditional GIS order (see the module comment).
/// `t` is the coordinate epoch in decimal years, used only by time-dependent
/// datum transformations.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Coordinate {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub t: f64,
}

impl Coordinate {
    /// A coordinate with no epoch (`t` is `UNSPECIFIED_EPOCH`).
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self {
            x,
            y,
            z,
            t: UNSPECIFIED_EPOCH,
        }
    }
}

impl Default for Coordinate {
    fn default() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }
}

// The batch transformation hands interior pointers to PROJ with
// size_of::<Coordinate>() as the stride.
const _: () = assert!(
    std::mem::size_of::<Coordinate>() == 4 * std::mem::size_of::<f64>(),
    "Coordinate must be four packed doubles"
);

/// Outcome of a batch transformation. A point that PROJ cannot transform is
/// NOT an error of the whole batch: its x, y and z are set to quiet NaN (so
/// that it can never be mistaken for a position) and it is counted here.
/// Callers must inspect `failed`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BatchReport {
    pub succeeded: usize,
    pub failed: usize,
    /// Meaningful only when `failed > 0`.
    pub first_failed_index: usize,
}

impl BatchReport {
    pub const fn all_succeeded(&self) -> bool {
        self.failed == 0
    }
}
