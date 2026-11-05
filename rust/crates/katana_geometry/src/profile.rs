use katana_core::error::{Error, ErrorCode, Result};
use katana_core::text::format_exact_real;
use katana_math::numerics::tolerance;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProfilePvi {
    pub station: f64,
    pub elevation: f64,
    pub curve_length: f64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VerticalAlignment {
    pub pvis: Vec<ProfilePvi>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProfileElementKind {
    #[default]
    Tangent,
    Curve,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProfileElement {
    pub kind: ProfileElementKind,
    pub start_station: f64,
    pub length: f64,
    pub start_elevation: f64,
    pub start_grade: f64,
    pub end_grade: f64,
    pub pvi: usize,
}
impl ProfileElement {
    pub fn elevation_at(&self, distance: f64) -> f64 {
        if !(self.length > 0.0) {
            return self.start_elevation;
        }
        self.start_elevation
            + self.start_grade * distance
            + (self.end_grade - self.start_grade) * distance * distance / (2.0 * self.length)
    }
    pub fn grade_at(&self, distance: f64) -> f64 {
        if !(self.length > 0.0) {
            self.start_grade
        } else {
            self.start_grade + (self.end_grade - self.start_grade) * (distance / self.length)
        }
    }
    pub fn end_elevation(&self) -> f64 {
        self.elevation_at(self.length)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProfileExtremum {
    pub station: f64,
    pub elevation: f64,
    pub high: bool,
    pub pvi: usize,
}
#[derive(Clone, Debug, Default)]
pub struct SolvedProfile {
    elements: Vec<ProfileElement>,
    start_station: f64,
    length: f64,
}
impl SolvedProfile {
    pub fn elements(&self) -> &[ProfileElement] {
        &self.elements
    }
    pub fn start_station(&self) -> f64 {
        self.start_station
    }
    pub fn end_station(&self) -> f64 {
        self.start_station + self.length
    }
    pub fn length(&self) -> f64 {
        self.length
    }
    pub fn contains_station(&self, station: f64) -> bool {
        station >= self.start_station && station <= self.end_station()
    }
    pub fn element_at(&self, station: f64) -> Option<&ProfileElement> {
        if self.elements.is_empty() || !self.contains_station(station) {
            return None;
        }
        let after = self
            .elements
            .partition_point(|element| !(station < element.start_station));
        Some(&self.elements[after.saturating_sub(1)])
    }
    pub fn elevation_at(&self, station: f64) -> Option<f64> {
        self.element_at(station)
            .map(|element| element.elevation_at(station - element.start_station))
    }
    pub fn grade_at(&self, station: f64) -> Option<f64> {
        self.element_at(station)
            .map(|element| element.grade_at(station - element.start_station))
    }
    pub fn key_stations(&self) -> Vec<f64> {
        let mut stations = vec![self.start_station];
        stations.extend(
            self.elements
                .iter()
                .map(|element| element.start_station + element.length),
        );
        stations
    }
    pub fn high_low_points(&self) -> Vec<ProfileExtremum> {
        let mut points = Vec::new();
        for element in &self.elements {
            if element.kind != ProfileElementKind::Curve
                || !(element.start_grade * element.end_grade < 0.0)
            {
                continue;
            }
            let distance =
                element.start_grade * element.length / (element.start_grade - element.end_grade);
            points.push(ProfileExtremum {
                station: element.start_station + distance,
                elevation: element.elevation_at(distance),
                high: element.start_grade > 0.0,
                pvi: element.pvi,
            });
        }
        points
    }
}

pub fn solve_profile(definition: &VerticalAlignment) -> Result<SolvedProfile> {
    let pvis = &definition.pvis;
    let count = pvis.len();
    if count < 2 {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "a profile needs at least two PVIs",
        ));
    }
    for (index, pvi) in pvis.iter().enumerate() {
        if !pvi.station.is_finite() || !pvi.elevation.is_finite() || !pvi.curve_length.is_finite() {
            return Err(
                Error::new(ErrorCode::InvalidArgument, "a PVI has a non-finite value")
                    .with_context(format!("PVI {index}")),
            );
        }
        if pvi.curve_length < 0.0 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a curve length cannot be negative",
            )
            .with_context(format!("PVI {index}")));
        }
        if index > 0 && pvi.station - pvis[index - 1].station < tolerance::COORDINATE {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "PVI stations must strictly increase",
            )
            .with_context(format!("PVI {} and PVI {index}", index - 1)));
        }
    }
    if pvis[0].curve_length > 0.0 || pvis[count - 1].curve_length > 0.0 {
        let index = if pvis[0].curve_length > 0.0 {
            0
        } else {
            count - 1
        };
        return Err(Error::new(ErrorCode::InvalidGeometry, "the first and last PVI cannot carry a curve: there is no grade on the far side to blend into").with_context(format!("PVI {index}")));
    }
    for index in 0..count - 1 {
        let available = (pvis[index + 1].station - pvis[index].station)
            - 0.5 * pvis[index].curve_length
            - 0.5 * pvis[index + 1].curve_length;
        if available < -tolerance::COORDINATE {
            return Err(Error::new(ErrorCode::InvalidGeometry, "the vertical curves overlap: their half-lengths exceed the distance between the PVIs").with_context(format!("PVI {index} and PVI {}, short by {} m", index + 1, format_exact_real(-available))));
        }
    }
    let grades: Vec<f64> = pvis
        .windows(2)
        .map(|pair| (pair[1].elevation - pair[0].elevation) / (pair[1].station - pair[0].station))
        .collect();
    let mut solved = SolvedProfile {
        start_station: pvis[0].station,
        ..Default::default()
    };
    let mut station = pvis[0].station;
    let mut elevation = pvis[0].elevation;
    for index in 1..count {
        let grade = grades[index - 1];
        let curve = index + 1 < count && pvis[index].curve_length > 0.0;
        let half = if curve {
            0.5 * pvis[index].curve_length
        } else {
            0.0
        };
        let tangent_end = pvis[index].station - half;
        let tangent_length = tangent_end - station;
        if tangent_length > tolerance::GEOMETRIC {
            solved.elements.push(ProfileElement {
                kind: ProfileElementKind::Tangent,
                start_station: station,
                length: tangent_length,
                start_elevation: elevation,
                start_grade: grade,
                end_grade: grade,
                pvi: index,
            });
            elevation += grade * tangent_length;
            station = tangent_end;
        }
        if curve {
            let element = ProfileElement {
                kind: ProfileElementKind::Curve,
                start_station: station,
                length: pvis[index].curve_length,
                start_elevation: elevation,
                start_grade: grade,
                end_grade: grades[index],
                pvi: index,
            };
            solved.elements.push(element);
            elevation = element.end_elevation();
            station += pvis[index].curve_length;
        }
    }
    solved.length = station - solved.start_station;
    Ok(solved)
}
