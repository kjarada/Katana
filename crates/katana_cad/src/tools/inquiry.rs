//! Distance, Area, ID Point, Angle, List (src/katana_cad/tools/inquiry.cpp;
//! see families.rs): the tools that measure and report. None changes the
//! drawing - each finishes with no command and its answer as the step's
//! message, which the window writes to the command log - so they are safe to
//! run on any drawing, locked layers included.
//!
//! The answers are a surveyor's, not a drafter's: coordinates easting first,
//! directions as azimuths and quadrant bearings in degrees, minutes and
//! seconds, heights kept apart from lengths and never invented. Distance and
//! an object's Area are the survey tools' own computation and wording
//! (survey_tools.rs: compute_inverse, compute_area and their formatters), so
//! the Survey menu's Inverse and this tool cannot describe one pair of points
//! two ways. A pick ON a survey point (a snap puts it exactly there; see
//! everyday_support's POINT_COINCIDENCE) speaks for that point: its number,
//! and its height, so a distance between two levelled points reports their
//! height difference. A pick merely near one is only its coordinates.
//!
//! Distance, Area, ID Point and Angle start again when they have answered, as
//! AutoCAD's MEASUREGEOM does, so a run of measurements is a run of picks; Esc
//! ends them. List ends, since its next use wants a fresh selection.

use katana_entity::entity::{
    ELEVATION_PROPERTY, ELEVATIONS_PROPERTY, heights_of, property_to_string, to_string,
};
use katana_entity::{Entity, EntityId, Geometry};
use katana_geometry::{Point2, Polyline2, Segment2};
use katana_math::cpp;
use katana_math::numerics::{PI, tolerance};
use katana_survey::angles::normalize_azimuth;
use katana_survey::cogo::{polygon_area, polygon_perimeter};
use katana_survey::coordinate::Coordinate2;

use crate::document::Document;
use crate::interactive_tool::{
    InteractiveTool, ToolCatalog, ToolContext, ToolInfo, ToolInput, ToolStart, ToolStep,
};
use crate::survey_import::SurveyImportOptions;
use crate::survey_tools::{
    compute_area, compute_inverse, format_area_report, format_inverse_report,
};
use crate::tool_feedback::ToolFeedback;
use crate::tools::everyday_support::{
    Path, SelectionStep, area_text, azimuth_of, bearing, coordinates, counted, degrees, fixed,
    length_unit, position_at,
};
use crate::tools::families::Report;
use crate::tools::modify_edit_support::{
    ToolPtr, as_sentence, base_entity, base_point, is_option, tool_factory,
};

const CATEGORY: &str = "Tools";
const GROUP: &str = "Inquiry";

fn coincident(a: Point2, b: Point2) -> bool {
    a.distance_to(b) <= tolerance::GEOMETRIC
}

// The rubber band from `from` to the cursor, with `from` marked.
fn add_band(feedback: &mut ToolFeedback, from: Point2, cursor: Point2) {
    if !coincident(from, cursor) {
        feedback
            .shapes
            .push(Geometry::Line(Segment2::new(from, cursor)));
    }
    feedback.markers.push(from);
}

// ---- Distance ----------------------------------------------------------------------
//
// Two points, then the inverse between them: the differences in easting and
// northing, the horizontal distance, the azimuth both ways and the bearing,
// and - when both picks are survey points with heights - the height
// difference, slope distance and grade.

#[derive(Default)]
struct DistanceTool {
    first: Option<Point2>,
}

impl InteractiveTool for DistanceTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        String::from(if self.first.is_some() {
            "Specify second point"
        } else {
            "Specify first point"
        })
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        ToolInput::Point
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        let Some(first) = self.first else {
            self.first = Some(at);
            return ToolStep::next("");
        };
        match compute_inverse(
            &position_at(cx.document, first),
            &position_at(cx.document, at),
        ) {
            Ok(inverse) => ToolStep::done(None, format_inverse_report(&inverse), true),
            Err(error) => ToolStep::rejected(as_sentence(&error.message)),
        }
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.first.take().is_none() {
            return ToolStep::rejected("nothing to undo; no point has been given yet");
        }
        ToolStep::next("")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        if let Some(first) = self.first {
            add_band(&mut feedback, first, cursor);
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.first
    }
}

// ---- Area --------------------------------------------------------------------------
//
// Corners picked in order, closed back to the first, and Enter for the area
// and perimeter; or [Object] and a closed polyline or circle, measured by the
// survey tools' own area (compute_area). Started with a selection, Enter at
// the first corner measures what is selected, as the command line's AREA
// does.

struct AreaTool {
    selection: Vec<EntityId>,
    corners: Vec<Point2>,
    object: bool,
}

impl AreaTool {
    fn measure(cx: &ToolContext<'_>, ids: &[EntityId], restart: bool) -> ToolStep {
        let Some(document) = cx.document else {
            return ToolStep::rejected("there is no drawing to measure");
        };
        match compute_area(document, ids) {
            Ok(area) => ToolStep::done(None, format_area_report(&area), restart),
            Err(error) => ToolStep::rejected(as_sentence(&error.message)),
        }
    }

    fn picked_area(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        // The C++ reads its document here unchecked; a context with none is
        // refused as measure() refuses it.
        let Some(document) = cx.document else {
            return ToolStep::rejected("there is no drawing to measure");
        };
        // The survey library's polygon area and perimeter, which work from
        // the first vertex so coordinates of MGA size keep their precision.
        let ring: Vec<Coordinate2> = self
            .corners
            .iter()
            .map(|corner| Coordinate2::new(corner.y, corner.x)) // northing, easting
            .collect();
        let area = polygon_area(&ring);
        let perimeter = polygon_perimeter(&ring);
        let (area, perimeter) = match (area, perimeter) {
            (Ok(area), Ok(perimeter)) => (area, perimeter),
            (Err(error), _) | (Ok(_), Err(error)) => {
                return ToolStep::rejected(as_sentence(&error.message));
            }
        };
        let mut text = format!(
            "Area of {} - drawing unit '{}' (project settings)\n",
            counted(self.corners.len(), "picked corner", "picked corners"),
            document.metadata().linear_unit
        );
        text.push_str(&format!(
            "  area {}   perimeter {} {}",
            area_text(document, area),
            fixed(perimeter, 3),
            length_unit(document)
        ));
        self.corners.clear();
        ToolStep::done(None, text, true)
    }
}

impl InteractiveTool for AreaTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        if self.object {
            return String::from("Select a closed polyline or circle or [Points]");
        }
        if self.corners.is_empty() {
            return String::from(if self.selection.is_empty() {
                "Specify first corner point or [Object]"
            } else {
                "Specify first corner point or [Object] <Selection>"
            });
        }
        String::from(if self.corners.len() < 3 {
            "Specify next corner point or [Undo]"
        } else {
            "Specify next corner point or [Undo], or press Enter for the area"
        })
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.object {
            ToolInput::Entity
        } else {
            ToolInput::Point
        }
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if self.object {
            return base_point(cx, self.expects(cx), at);
        }
        if let Some(&last) = self.corners.last()
            && coincident(at, last)
        {
            return ToolStep::rejected("the corner is where the last one is");
        }
        self.corners.push(at);
        ToolStep::next("")
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if !self.object {
            return base_entity(cx, self.expects(cx), id, at);
        }
        Self::measure(cx, &[id], true)
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if !self.object && self.corners.is_empty() && is_option(text, "Object", "O") {
            self.object = true;
            return ToolStep::next("");
        }
        if self.object && is_option(text, "Points", "P") {
            self.object = false;
            return ToolStep::next("");
        }
        if !self.object && is_option(text, "Undo", "U") {
            return self.undo(cx);
        }
        ToolStep::rejected(format!("'{text}' is not an option here"))
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        if self.object {
            return ToolStep::done(None, "", false);
        }
        if self.corners.is_empty() {
            // The selection, once - a restart would measure it again.
            if self.selection.is_empty() {
                return ToolStep::done(None, "", false);
            }
            return Self::measure(cx, &self.selection, false);
        }
        if self.corners.len() < 3 {
            return ToolStep::rejected("an area needs at least three corners");
        }
        self.picked_area(cx)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.object || self.corners.pop().is_none() {
            return ToolStep::rejected("nothing to undo; no corner has been given yet");
        }
        ToolStep::next("")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        let Some(&last) = self.corners.last() else {
            return feedback;
        };
        if self.object {
            return feedback;
        }
        let mut outline = Polyline2::new(self.corners.clone(), false);
        if !coincident(cursor, last) {
            outline.vertices.push(cursor);
        }
        outline.closed = outline.vertices.len() >= 3;
        if outline.vertices.len() >= 2 {
            feedback.shapes.push(Geometry::Polyline(outline));
        }
        feedback.markers = self.corners.clone();
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.corners.last().copied()
    }
}

// ---- ID Point ----------------------------------------------------------------------
//
// One point and its coordinates. On a survey point, the point's number, its
// height and its code as well - what a surveyor clicks a point to find out.

struct IdPointTool;

impl InteractiveTool for IdPointTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        String::from("Specify a point")
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        ToolInput::Point
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        let position = position_at(cx.document, at);
        let (Some(id), Some(document)) = (position.entity, cx.document) else {
            return ToolStep::done(None, coordinates(at), true);
        };
        let mut text = format!("{}  {}", position.label(), coordinates(position.point));
        match position.elevation {
            Some(elevation) => text.push_str(&format!("  Z {}", fixed(elevation, 3))),
            None => text.push_str("  no elevation"),
        }
        // The code as the survey import writes it (SurveyImportOptions).
        let code_key = SurveyImportOptions::default().code_property;
        if let Some(entity) = document.model().entities.find(id)
            && let Some(code) = entity.properties.get(&code_key)
        {
            text.push_str(&format!("  code {}", property_to_string(code)));
        }
        ToolStep::done(None, text, true)
    }
}

// ---- Angle -------------------------------------------------------------------------
//
// A vertex, a point on the first arm and a point on the second: the included
// angle, and the angle turned clockwise from the first arm to the second as a
// surveyor turns it from a backsight to a foresight, with both arms'
// directions.

#[derive(Default)]
struct AngleTool {
    /// The vertex, then the first arm's point.
    points: Vec<Point2>,
}

impl InteractiveTool for AngleTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        String::from(match self.points.len() {
            0 => "Specify the vertex of the angle",
            1 => "Specify a point on the first arm (the backsight)",
            _ => "Specify a point on the second arm (the foresight)",
        })
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        ToolInput::Point
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if let Some(&vertex) = self.points.first()
            && coincident(at, vertex)
        {
            return ToolStep::rejected("an arm needs a point away from the vertex");
        }
        if self.points.len() < 2 {
            self.points.push(at);
            return ToolStep::next("");
        }
        let vertex = self.points[0];
        let first = azimuth_of(self.points[1] - vertex);
        let second = azimuth_of(at - vertex);
        let clockwise = normalize_azimuth(second - first);
        let other = if clockwise > 0.0 {
            2.0 * PI - clockwise
        } else {
            0.0
        };
        let mut text = format!("Angle at {}\n", position_at(cx.document, vertex).label());
        text.push_str(&format!(
            "  Included angle {}\n",
            degrees(cpp::min(clockwise, other))
        ));
        text.push_str(&format!(
            "  Turned clockwise from the first arm to the second {}   the other way {}\n",
            degrees(clockwise),
            degrees(other)
        ));
        text.push_str(&format!(
            "  First arm  azimuth {}   bearing {}\n",
            degrees(first),
            bearing(first)
        ));
        text.push_str(&format!(
            "  Second arm azimuth {}   bearing {}",
            degrees(second),
            bearing(second)
        ));
        self.points.clear();
        ToolStep::done(None, text, true)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.points.pop().is_none() {
            return ToolStep::rejected("nothing to undo; no point has been given yet");
        }
        ToolStep::next("")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        if self.points.len() == 2 {
            feedback.shapes.push(Geometry::Line(Segment2::new(
                self.points[0],
                self.points[1],
            )));
        }
        if let Some(&vertex) = self.points.first() {
            add_band(&mut feedback, vertex, cursor);
            feedback.markers = self.points.clone();
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.points.last().copied()
    }
}

// ---- List --------------------------------------------------------------------------
//
// What each selected entity is, where, and on what: its layer, style and
// colour, its geometry in survey terms, its heights and its properties.

// A thousand lines in the log is more than anyone reads; a selection of a
// whole survey is listed by its first entities and a count of the rest.
const MOST_LISTED: usize = 200;
// A polyline's vertices beyond this are counted, not listed.
const MOST_VERTICES: usize = 50;

fn z_text(height: Option<f64>) -> String {
    match height {
        Some(height) => format!("  Z {}", fixed(height, 3)),
        None => String::new(),
    }
}

fn describe(document: &Document, entity: &Entity) -> String {
    let unit = length_unit(document);
    let metres = format!(" {unit}");
    let mut text = format!(
        "{}  {}  layer {}  style {}  colour {}",
        entity.id,
        to_string(entity.r#type()),
        entity.layer,
        if entity.style.is_empty() {
            "ByLayer"
        } else {
            entity.style.as_str()
        },
        match entity.color {
            Some(color) => color.to_hex(),
            None => String::from("ByLayer"),
        }
    );
    let heights = |count: usize| heights_of(&entity.properties, count);
    match &entity.geometry {
        Geometry::Point(g) => {
            text.push_str(&format!(
                "\n    {}{}",
                coordinates(g.position),
                z_text(heights(1)[0])
            ));
        }
        Geometry::Line(g) => {
            let z = heights(2);
            text.push_str(&format!(
                "\n    from {}{}  to {}{}",
                coordinates(g.start),
                z_text(z[0]),
                coordinates(g.end),
                z_text(z[1])
            ));
            text.push_str(&format!(
                "\n    length {}{metres}",
                fixed(g.start.distance_to(g.end), 3)
            ));
            if !coincident(g.start, g.end) {
                text.push_str(&format!(
                    "   bearing {}",
                    bearing(azimuth_of(g.end - g.start))
                ));
            }
        }
        Geometry::Arc(g) => {
            text.push_str(&format!(
                "\n    centre {}  radius {}{metres}",
                coordinates(g.center),
                fixed(g.radius, 3)
            ));
            text.push_str(&format!(
                "\n    from {}  to {}{}",
                coordinates(g.start_point()),
                coordinates(g.end_point()),
                if g.sweep >= 0.0 {
                    "  anticlockwise"
                } else {
                    "  clockwise"
                }
            ));
            text.push_str(&format!(
                "\n    length {}{metres}   delta {}   chord {}{metres}",
                fixed(g.radius * g.sweep.abs(), 3),
                degrees(g.sweep.abs()),
                fixed(g.start_point().distance_to(g.end_point()), 3)
            ));
        }
        Geometry::Circle(g) => {
            text.push_str(&format!(
                "\n    centre {}{}  radius {}{metres}",
                coordinates(g.center),
                z_text(heights(1)[0]),
                fixed(g.radius, 3)
            ));
            text.push_str(&format!(
                "\n    circumference {}{metres}   area {}",
                fixed(2.0 * PI * g.radius, 3),
                area_text(document, PI * g.radius * g.radius)
            ));
        }
        Geometry::Polyline(g) => {
            let path = Path::of(&entity.geometry);
            text.push_str(&format!(
                "\n    {}, {}, length {}{metres}",
                if g.closed { "closed" } else { "open" },
                counted(g.vertices.len(), "vertex", "vertices"),
                fixed(path.map_or(0.0, |path| path.length()), 3)
            ));
            if g.closed && g.vertices.len() >= 3 {
                let ring: Vec<Coordinate2> = g
                    .vertices
                    .iter()
                    .map(|v| Coordinate2::new(v.y, v.x))
                    .collect();
                if let Ok(area) = polygon_area(&ring) {
                    text.push_str(&format!("   area {}", area_text(document, area)));
                }
            }
            let z = heights(g.vertices.len());
            let shown = g.vertices.len().min(MOST_VERTICES);
            for (number, (vertex, height)) in (1..).zip(g.vertices.iter().zip(&z).take(shown)) {
                text.push_str(&format!(
                    "\n    {number}  {}{}",
                    coordinates(*vertex),
                    z_text(*height)
                ));
            }
            if shown < g.vertices.len() {
                text.push_str(&format!(
                    "\n    ... and {} more vertices",
                    g.vertices.len() - shown
                ));
            }
        }
        Geometry::Text(g) => {
            text.push_str(&format!(
                "\n    \"{}\"  height {}{metres}  at {}  rotation {}",
                g.text,
                fixed(g.height, 3),
                coordinates(g.position),
                degrees(g.rotation)
            ));
        }
        Geometry::Dimension(g) => {
            text.push_str(&format!(
                "\n    measures {}{metres}  from {}  to {}",
                fixed(g.measurement(), 3),
                coordinates(g.start),
                coordinates(g.end)
            ));
            if !g.text_override.is_empty() {
                text.push_str(&format!("  shown as \"{}\"", g.text_override));
            }
        }
        _ => {}
    }
    for (key, value) in &entity.properties {
        // The heights are shown with the coordinates they belong to.
        if key == ELEVATION_PROPERTY || key == ELEVATIONS_PROPERTY {
            continue;
        }
        text.push_str(&format!("\n    {key} = {}", property_to_string(value)));
    }
    text
}

#[derive(Default)]
struct ListTool {
    selection: SelectionStep,
}

impl InteractiveTool for ListTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        String::from("Select entities to list or [All], then press Enter")
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        ToolInput::Selection
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, _at: Point2) -> ToolStep {
        self.selection.pick(cx.document, id)
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if is_option(text, "All", "A") {
            return self.selection.all(cx.document);
        }
        ToolStep::rejected(format!(
            "'{text}' is not an option here: pick entities, or type All"
        ))
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        let ids = self.selection.chosen(cx.document);
        let Some(document) = cx.document.filter(|_| !ids.is_empty()) else {
            return ToolStep::rejected(
                "nothing is selected: select the entities to list, then press Enter",
            );
        };
        let mut text = format!("{} listed", counted(ids.len(), "entity", "entities"));
        let shown = ids.len().min(MOST_LISTED);
        for &id in &ids[..shown] {
            if let Some(entity) = document.model().entities.find(id) {
                text.push('\n');
                text.push_str(&describe(document, entity));
            }
        }
        if shown < ids.len() {
            text.push_str(&format!(
                "\n... and {} more, not listed; select fewer to see them",
                ids.len() - shown
            ));
        }
        ToolStep::done(None, text, false)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.selection.undo() {
            ToolStep::next("")
        } else {
            ToolStep::rejected("nothing to undo in this tool")
        }
    }
}

fn distance_tool(_cx: &ToolContext<'_>, _start: &ToolStart) -> ToolPtr {
    Box::new(DistanceTool::default())
}

fn area_tool(_cx: &ToolContext<'_>, start: &ToolStart) -> ToolPtr {
    Box::new(AreaTool {
        selection: start.selection.clone(),
        corners: Vec::new(),
        object: false,
    })
}

fn id_point_tool(_cx: &ToolContext<'_>, _start: &ToolStart) -> ToolPtr {
    Box::new(IdPointTool)
}

fn angle_tool(_cx: &ToolContext<'_>, _start: &ToolStart) -> ToolPtr {
    Box::new(AngleTool::default())
}

fn list_tool(_cx: &ToolContext<'_>, _start: &ToolStart) -> ToolPtr {
    Box::new(ListTool::default())
}

fn inquiry(
    id: &str,
    name: &str,
    order: i32,
    aliases: &[&str],
    tip: &str,
    make: fn(&ToolContext<'_>, &ToolStart) -> ToolPtr,
) -> ToolInfo {
    ToolInfo {
        id: String::from(id),
        name: String::from(name),
        category: String::from(CATEGORY),
        group: String::from(GROUP),
        order,
        aliases: aliases.iter().map(|&alias| String::from(alias)).collect(),
        tip: String::from(tip),
        make: Some(tool_factory(make)),
        ..ToolInfo::default()
    }
}

pub fn add_inquiry_tools(catalog: &mut ToolCatalog, report: &mut Report<'_>) {
    // DIST, AA, ID and LI are AutoCAD's own short forms. The bare words AREA
    // and LIST stay the command line's (area of the selection; every entity
    // listed), which scripts and the headless checks type.
    report(catalog.add(inquiry(
        "inquiry.distance",
        "Distance",
        10,
        &["DIST", "DI"],
        "Measures between two points: dE and dN, the horizontal distance, the azimuth and \
         bearing, and the height difference when both are survey points with heights.",
        distance_tool,
    )));
    report(catalog.add(inquiry(
        "inquiry.area",
        "Area",
        20,
        &["AA", "MEASUREAREA"],
        "Measures the area and perimeter of corners picked in turn (Enter to total), or of a \
         closed polyline or circle (O for Object).",
        area_tool,
    )));
    report(catalog.add(inquiry(
        "inquiry.id",
        "ID Point",
        30,
        &["ID"],
        "Reports a point's easting and northing; on a survey point, its number, height and \
         code too.",
        id_point_tool,
    )));
    report(catalog.add(inquiry(
        "inquiry.angle",
        "Angle",
        40,
        &["ANGLE", "MEASUREANGLE"],
        "Measures the angle at a vertex between two arms, as included and as turned clockwise \
         from the first arm to the second, with each arm's bearing.",
        angle_tool,
    )));
    report(catalog.add(inquiry(
        "inquiry.list",
        "List",
        50,
        &["LI", "LS"],
        "Lists the selected entities: layer, style and colour, coordinates, lengths, bearings, \
         areas, heights and properties.",
        list_tool,
    )));
}
