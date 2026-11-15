//! Reference ellipsoid parameters.

/// Ellipsoid of revolution. `inverse_flattening == 0` denotes a sphere (the
/// usual EPSG / PROJ convention, since 1/f is infinite there).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ellipsoid {
    /// a, metres.
    pub semi_major_axis: f64,
    /// 1/f, dimensionless; 0 for a sphere.
    pub inverse_flattening: f64,
}

impl Ellipsoid {
    pub const fn new(semi_major_axis: f64, inverse_flattening: f64) -> Self {
        Self {
            semi_major_axis,
            inverse_flattening,
        }
    }

    pub const fn flattening(&self) -> f64 {
        if self.inverse_flattening == 0.0 {
            0.0
        } else {
            1.0 / self.inverse_flattening
        }
    }

    pub const fn semi_minor_axis(&self) -> f64 {
        self.semi_major_axis * (1.0 - self.flattening())
    }

    /// First eccentricity squared, e^2 = f (2 - f).
    pub const fn eccentricity_squared(&self) -> f64 {
        self.flattening() * (2.0 - self.flattening())
    }

    /// Defining constants as published by the respective authorities.
    pub const fn wgs84() -> Self {
        Self::new(6378137.0, 298.257223563)
    }

    pub const fn grs80() -> Self {
        Self::new(6378137.0, 298.257222101)
    }
}
