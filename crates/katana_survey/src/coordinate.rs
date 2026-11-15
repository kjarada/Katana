//! Plane grid position used by every survey calculation.
//!
//! Deliberately not `katana_math::Vec2`: a Vec2 is (x, y) with counter-clockwise
//! angles, survey work is (northing, easting) with clockwise azimuths. Naming
//! the fields keeps the two conventions from being mixed up silently.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Coordinate2 {
    /// Metres.
    pub northing: f64,
    /// Metres.
    pub easting: f64,
}

impl Coordinate2 {
    pub const fn new(northing: f64, easting: f64) -> Self {
        Self { northing, easting }
    }
}
