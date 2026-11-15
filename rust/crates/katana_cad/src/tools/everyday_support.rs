//! What the everyday families share (src/katana_cad/tools/everyday_support.hpp,
//! namespace tools::everyday) - Inquiry, Divide and Measure, Lengthen and
//! Reverse, Match Properties, Select Similar and Quick Select: numbers and
//! directions as the survey reports print them, the survey point under a
//! pick, a selection step, and an object measured along its length. In a
//! module of its own, as modify_edit's helpers are, because other families
//! are written at the same time and may well have helpers of the same names.
//!
//! The C++ SelectionStep keeps the `const Document*` of the tool's context;
//! here, as every tool is lent its document per call (interactive_tool.rs,
//! "The Rust shape of a tool"), its methods are handed the document instead.

use katana_entity::{Entity, EntityId, Geometry};
use katana_geometry::{Point2, Vec2};
use katana_math::cpp;
use katana_math::numerics::PI;
use katana_survey::angles::{format_bearing, format_dms, normalize_azimuth};

use crate::document::Document;
use crate::interactive_tool::ToolStep;
use crate::layer_overrides::NO_LAYER_OVERRIDES;
use crate::selection::is_selectable;
use crate::survey_tools::{
    SURVEY_SECONDS_DECIMALS, SurveyPosition, c_fixed, is_metre_unit, position_of_point,
};

// ---- words and numbers --------------------------------------------------------------
//
// The survey reports' conventions (survey_tools.rs): lengths and coordinates
// to 3 decimals, angles in degrees, minutes and seconds to
// SURVEY_SECONDS_DECIMALS, directions as azimuths clockwise from grid north
// and as quadrant bearings, "E ... N ..." easting first.

/// `value` to `decimals` places (the C++ default is 3), with no "-0.000".
pub fn fixed(value: f64, decimals: i32) -> String {
    let mut text = c_fixed(value, decimals);
    // -0.0004 prints as "-0.000", which reads as a sign that means something.
    if text.starts_with('-') && text.bytes().all(|byte| matches!(byte, b'-' | b'0' | b'.')) {
        text.remove(0);
    }
    text
}

/// An angle in D°MM'SS.ss".
pub fn degrees(radians: f64) -> String {
    // The survey formatter fails only for a non-finite angle, which nothing
    // here makes; if one ever came, the report says so in place rather than
    // printing a plausible angle (survey_tools does the same).
    match format_dms(radians, SURVEY_SECONDS_DECIMALS) {
        Ok(text) => text,
        Err(error) => format!("({})", error.message),
    }
}

/// A quadrant bearing, N 36°52'11.63" E, of an azimuth.
pub fn bearing(azimuth: f64) -> String {
    match format_bearing(azimuth, SURVEY_SECONDS_DECIMALS) {
        Ok(text) => text,
        Err(error) => format!("({})", error.message),
    }
}

/// The azimuth of `direction` in the drawing (x east, y north): clockwise
/// from north, in [0, 2*pi). Zero for a zero vector, which callers refuse
/// first.
pub fn azimuth_of(direction: Vec2) -> f64 {
    if direction.x == 0.0 && direction.y == 0.0 {
        return 0.0;
    }
    // atan2(east, north): clockwise from north, as survey's angles measure.
    normalize_azimuth(direction.x.atan2(direction.y))
}

/// "E 1000.000 N 2000.000".
pub fn coordinates(point: Point2) -> String {
    format!("E {} N {}", fixed(point.x, 3), fixed(point.y, 3))
}

/// The length unit reports print: "m" in a metre drawing, otherwise the
/// project's own name for its unit.
pub fn length_unit(document: &Document) -> String {
    let unit = &document.metadata().linear_unit;
    if is_metre_unit(unit) {
        String::from("m")
    } else {
        unit.clone()
    }
}

/// "12.000 m² (0.0012 ha)" in a metre drawing; "12.000 square feet"
/// otherwise, where a hectare means nothing (format_area_report's rule).
pub fn area_text(document: &Document, area: f64) -> String {
    // One hectare is 10 000 m^2 by definition (the SI Brochure, 9th edition,
    // table 8), and means nothing in a drawing in any other unit.
    const SQUARE_METRES_PER_HECTARE: f64 = 10_000.0;
    let unit = &document.metadata().linear_unit;
    if is_metre_unit(unit) {
        return format!(
            "{} m\u{b2} ({} ha)",
            fixed(area, 3),
            fixed(area / SQUARE_METRES_PER_HECTARE, 4)
        );
    }
    format!("{} square {unit}", fixed(area, 3))
}

/// "1 entity", "3 entities": a count and its noun.
pub fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

// ---- the drawing -------------------------------------------------------------------

/// How near a point entity a pick must be to BE that point in a report: a
/// thousandth of a drawing unit, a millimetre in a drawing in metres. A snap
/// lands on the point exactly, and coordinates typed from a report printed
/// to three decimals are within half a thousandth on each axis (0.0007 in
/// all). Not the view's pick aperture: at a zoom where the aperture is ten
/// metres, a click that missed a point was reported as the point - its
/// number, its coordinates, its height - which a surveyor would take for a
/// measurement to the mark (seen on a real survey import).
pub const POINT_COINCIDENCE: f64 = 0.001;

/// Where a pick landed, for a survey report: the point entity there - its
/// number and its height, so an inverse between two survey points reports
/// their height difference - or the bare coordinates. A point entity counts
/// within POINT_COINCIDENCE of `at`; the nearest wins, the lower id on a tie.
pub fn position_at(document: Option<&Document>, at: Point2) -> SurveyPosition {
    let bare = SurveyPosition {
        point: at,
        ..SurveyPosition::default()
    };
    let Some(document) = document else {
        return bare;
    };
    let radius = POINT_COINCIDENCE;
    let mut near = Vec::new();
    document.spatial_index().query_radius(at, radius, &mut near);
    let mut best: Option<EntityId> = None;
    let mut best_distance = f64::INFINITY;
    for id in near {
        let Some(entity) = document.model().entities.find(id) else {
            continue;
        };
        let Geometry::Point(point) = &entity.geometry else {
            continue;
        };
        let distance = point.position.distance_to(at);
        if distance <= radius
            && (distance < best_distance
                || (distance == best_distance && best.is_some_and(|best| id < best)))
        {
            best = Some(id);
            best_distance = distance;
        }
    }
    let Some(best) = best else {
        return bare;
    };
    position_of_point(document, best).unwrap_or(bare)
}

/// The selection an asking tool ends with (List, Reverse, Select Similar,
/// the targets of Match Properties): single picks the view handed to
/// entity(), plus the document's selection when Enter is pressed - the view
/// selects as the Select tool does while a tool asks for a selection - or
/// everything selectable after "All". Ascending, without duplicates or
/// deleted ids. Each call is handed the tool's document (module doc).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionStep {
    picked: Vec<EntityId>,
    history: Vec<Vec<EntityId>>,
}

impl SelectionStep {
    pub fn new() -> Self {
        Self::default()
    }

    /// A pick: Rejected for an id not in the drawing or picked already.
    pub fn pick(&mut self, document: Option<&Document>, id: EntityId) -> ToolStep {
        if !document.is_some_and(|document| document.model().entities.contains(id)) {
            return ToolStep::rejected("that entity is not in the drawing");
        }
        if self.picked.contains(&id) {
            return ToolStep::rejected("that entity is already selected");
        }
        self.history.push(self.picked.clone());
        self.picked.push(id);
        ToolStep::next("")
    }

    /// "All": every entity the document lets be selected.
    pub fn all(&mut self, document: Option<&Document>) -> ToolStep {
        let Some(document) = document else {
            return ToolStep::rejected("there is no drawing to select from");
        };
        let model = document.model();
        let mut every = Vec::new();
        // The document's rule alone: the context's view is not consulted, so a
        // layer hidden in one view only is not left out (as SelectionTool's
        // All in modify_transform, and outstanding there too).
        model.entities.for_each(|candidate: &Entity| {
            if is_selectable(model, candidate, &NO_LAYER_OVERRIDES) {
                every.push(candidate.id);
            }
        });
        self.history.push(std::mem::take(&mut self.picked));
        self.picked = every;
        ToolStep::next(format!(
            "{} selected",
            counted(self.picked.len(), "entity", "entities")
        ))
    }

    /// Takes back the last pick or All; false when there is none.
    pub fn undo(&mut self) -> bool {
        match self.history.pop() {
            Some(previous) => {
                self.picked = previous;
                true
            }
            None => false,
        }
    }

    /// What Enter takes.
    pub fn chosen(&self, document: Option<&Document>) -> Vec<EntityId> {
        let mut out = self.picked.clone();
        if let Some(document) = document {
            out.extend(document.selection().ids());
            out.retain(|&id| document.model().entities.contains(id));
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

// ---- along an object ----------------------------------------------------------------

/// A line, an arc, a circle or a polyline measured from its start - a circle
/// from its east point, counter-clockwise, as it is drawn. None (from of())
/// for a kind with no length (a point, a text, a dimension).
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    geometry: Geometry,
    length: f64,
    closed: bool,
    /// Polyline vertex stations, closing vertex included.
    stations: Vec<f64>,
}

impl Path {
    pub fn of(geometry: &Geometry) -> Option<Path> {
        let mut path = Path {
            geometry: geometry.clone(),
            length: 0.0,
            closed: false,
            stations: Vec::new(),
        };
        match geometry {
            Geometry::Line(line) => path.length = line.start.distance_to(line.end),
            Geometry::Arc(arc) => path.length = arc.radius * arc.sweep.abs(),
            Geometry::Circle(circle) => {
                path.length = 2.0 * PI * circle.radius;
                path.closed = true;
            }
            Geometry::Polyline(polyline) => {
                let v = &polyline.vertices;
                if v.len() < 2 {
                    return None;
                }
                path.closed = polyline.closed;
                path.stations.push(0.0);
                let count = if polyline.closed {
                    v.len() + 1
                } else {
                    v.len()
                };
                for i in 1..count {
                    let previous = path.stations[path.stations.len() - 1];
                    path.stations
                        .push(previous + v[i - 1].distance_to(v[i % v.len()]));
                }
                path.length = path.stations[path.stations.len() - 1];
            }
            _ => return None,
        }
        Some(path)
    }

    pub fn length(&self) -> f64 {
        self.length
    }

    pub fn closed(&self) -> bool {
        self.closed
    }

    /// The point `station` along, clamped to [0, length].
    pub fn point_at(&self, station: f64) -> Point2 {
        let s = cpp::clamp(station, 0.0, self.length);
        match &self.geometry {
            Geometry::Line(line) => {
                if self.length <= 0.0 {
                    return line.start;
                }
                line.start + (line.end - line.start) * (s / self.length)
            }
            Geometry::Arc(arc) => {
                if self.length <= 0.0 {
                    arc.start_point()
                } else {
                    arc.point_at(s / self.length)
                }
            }
            Geometry::Circle(circle) => {
                let angle = if circle.radius > 0.0 {
                    s / circle.radius
                } else {
                    0.0
                };
                circle.center + Vec2::new(angle.cos(), angle.sin()) * circle.radius
            }
            Geometry::Polyline(polyline) => {
                let v = &polyline.vertices;
                let stations = &self.stations;
                // The segment holding s: the last station not after it
                // (std::upper_bound).
                let upper = stations.partition_point(|&station| station <= s);
                let i = upper.clamp(1, stations.len() - 1);
                let a = v[(i - 1) % v.len()];
                let b = v[i % v.len()];
                let span = stations[i] - stations[i - 1];
                if span <= 0.0 {
                    return a;
                }
                a + (b - a) * ((s - stations[i - 1]) / span)
            }
            // of() makes a path of no other kind.
            _ => Point2::default(),
        }
    }
}
