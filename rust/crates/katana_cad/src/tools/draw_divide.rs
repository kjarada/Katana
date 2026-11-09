//! Divide and Measure (src/katana_cad/tools/draw_divide.cpp; see
//! families.rs): points along an object - N equal parts, or one every so far -
//! as AutoCAD's DIVIDE and MEASURE place them, for setting-out marks, fence
//! posts, chainage points or light poles.
//!
//! The points are ordinary point entities on the current layer. [Style] puts
//! them in a named style instead, and a style carries a point symbol (Format >
//! Styles), which is how a symbol goes along an object here: the symbol is
//! the style's, not a block's, so it is drawn and plotted as every styled
//! point is. On a 3D string each point gets the height interpolated along it
//! (modify_edit_support's height_at), so points divided along a surveyed kerb
//! are levelled points, and a point where the string has no height gets none -
//! absent is not zero.
//!
//! Where they go, measured along the object from its start (a circle from its
//! east point, anticlockwise, as it is drawn):
//!
//! ```text
//!   Divide into N: an open object gets N - 1 points, its ends being marked
//!   already; a closed one gets N, since each of its N parts ends at a point.
//!   Measure every d: at d, 2d, ... short of the far end. An open object is
//!   measured from the end nearer the pick, as AutoCAD measures; a closed one
//!   from its start.
//! ```
//!
//! All the points are ONE command, so one undo takes them all.

use katana_commands::{EntityAttributes, create_entities};
use katana_core::text::{equals_ignoring_case, parse_integer, trimmed};
use katana_entity::entity::set_heights;
use katana_entity::{Entity, EntityId, PointGeometry};
use katana_geometry::Point2;
use katana_math::numerics::tolerance;

use crate::interactive_tool::{
    InteractiveTool, ToolCatalog, ToolContext, ToolInfo, ToolInput, ToolStart, ToolStep,
};
use crate::tool_feedback::ToolFeedback;
use crate::tools::everyday_support::{Path, fixed};
use crate::tools::families::Report;
use crate::tools::modify_edit_support::{
    ToolPtr, base_entity, base_value, format_number, height_at, is_option, kind_name, parse_number,
    tool_factory, trimmed_text, with_article,
};

// The array command's guard against a runaway typo (entity_commands.rs), kept
// here for the same reason: a length typed in millimetres into a drawing in
// metres asks for a million points.
const MOST_POINTS: i64 = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Divide,
    Measure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Object,
    Amount,
    Style,
}

struct AlongTool {
    attributes: EntityAttributes,
    kind: Kind,
    step: Step,
    source: Entity,
    path: Option<Path>,
    from_end: bool,
}

impl AlongTool {
    fn new(start: &ToolStart, kind: Kind) -> Self {
        Self {
            attributes: start.attributes.clone(),
            kind,
            step: Step::Object,
            source: Entity::default(),
            path: None,
            from_end: false,
        }
    }

    fn choose_style(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        let name = trimmed_text(text);
        if equals_ignoring_case(name.as_bytes(), b"ByLayer") {
            self.attributes.style.clear();
        } else if !cx
            .document
            .is_some_and(|document| document.model().styles.contains(name))
        {
            return ToolStep::rejected(format!(
                "there is no style '{name}' in the drawing (Format > Styles makes one)"
            ));
        } else {
            self.attributes.style = String::from(name);
        }
        self.step = Step::Amount;
        ToolStep::next("")
    }

    fn divide(&self, text: &str) -> ToolStep {
        let parts = parse_integer(trimmed(text.as_bytes()));
        let Some(parts) = parts.filter(|&parts| parts >= 2) else {
            return ToolStep::rejected("the number of segments is a whole number of 2 or more");
        };
        if parts > MOST_POINTS {
            return ToolStep::rejected(format!(
                "that would make {parts} points; the most is {MOST_POINTS}"
            ));
        }
        let Some(path) = &self.path else {
            return ToolStep::rejected("type the number of segments");
        };
        let n = parts as usize;
        let length = path.length();
        // k * L / n rather than a running sum, so the last station is as
        // exact as the first and no error accumulates along the object.
        let first = if path.closed() { 0 } else { 1 };
        let stations: Vec<f64> = (first..n).map(|k| length * k as f64 / n as f64).collect();
        self.place(path, &stations, &format!("Divided into {n} parts"))
    }

    fn measure(&self, text: &str) -> ToolStep {
        // A typed number is finite, so "more than" is "not at most".
        let Some(every) = parse_number(text).filter(|&every| every > tolerance::GEOMETRIC) else {
            return ToolStep::rejected("the length of a segment is a number more than zero");
        };
        let Some(path) = &self.path else {
            return ToolStep::rejected("type the length of a segment");
        };
        let length = path.length();
        // Short of the far end by more than the tolerance, so a length that
        // divides the object exactly does not put a point on its end.
        let count = ((length - tolerance::GEOMETRIC) / every).floor();
        if count < 1.0 {
            return ToolStep::rejected(format!(
                "the object is {} long, shorter than one segment",
                fixed(length, 3)
            ));
        }
        if count > MOST_POINTS as f64 {
            return ToolStep::rejected(format!(
                "that would make {} points; the most is {MOST_POINTS}",
                fixed(count, 0)
            ));
        }
        let stations: Vec<f64> = (1..=count as usize)
            .map(|k| {
                let along = every * k as f64;
                if self.from_end { length - along } else { along }
            })
            .collect();
        self.place(
            path,
            &stations,
            &format!("Measured every {}", format_number(every)),
        )
    }

    fn place(&self, path: &Path, stations: &[f64], what: &str) -> ToolStep {
        let mut points = Vec::with_capacity(stations.len());
        for &station in stations {
            let position = path.point_at(station);
            let mut point = Entity {
                geometry: katana_entity::Geometry::Point(PointGeometry { position }),
                layer: self.attributes.layer.clone(),
                style: self.attributes.style.clone(),
                color: self.attributes.color,
                ..Entity::default()
            };
            if let Some(height) = height_at(&self.source, position) {
                set_heights(&mut point.properties, &[Some(height)]);
            }
            points.push(point);
        }
        let count = points.len();
        ToolStep::done(
            Some(create_entities(points)),
            format!(
                "{what}: {count}{}",
                if count == 1 { " point" } else { " points" }
            ),
            true,
        )
    }
}

impl InteractiveTool for AlongTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        match self.step {
            Step::Object => String::from(match self.kind {
                Kind::Divide => "Select the object to divide",
                Kind::Measure => "Select the object to measure, near the end to measure from",
            }),
            Step::Amount => String::from(match self.kind {
                Kind::Divide => "Enter the number of segments or [Style]",
                Kind::Measure => "Specify length of segment or [Style]",
            }),
            Step::Style => format!(
                "Style for the points, or ByLayer <{}>",
                if self.attributes.style.is_empty() {
                    "ByLayer"
                } else {
                    self.attributes.style.as_str()
                }
            ),
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.step == Step::Object {
            ToolInput::Entity
        } else {
            ToolInput::Value
        }
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if self.step != Step::Object {
            return base_entity(cx, self.expects(cx), id, at);
        }
        let Some(entity) = cx
            .document
            .and_then(|document| document.model().entities.find(id))
        else {
            return ToolStep::rejected("that entity is not in the drawing");
        };
        let Some(path) = Path::of(&entity.geometry) else {
            return ToolStep::rejected(format!(
                "{} has no length to place points along",
                with_article(kind_name(&entity.geometry))
            ));
        };
        let length = path.length();
        if length <= tolerance::GEOMETRIC {
            return ToolStep::rejected(format!(
                "that {} has no length",
                kind_name(&entity.geometry)
            ));
        }
        // From the nearer end, for an open object only: a closed one has no
        // end, and is measured from its start.
        self.from_end = !path.closed()
            && at.distance_to(path.point_at(length)) < at.distance_to(path.point_at(0.0));
        self.source = entity.clone();
        self.path = Some(path);
        self.step = Step::Amount;
        ToolStep::next(format!("Length {}", fixed(length, 3)))
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if self.step == Step::Style {
            return self.choose_style(cx, text);
        }
        if self.step != Step::Amount {
            return base_value(cx, text);
        }
        if is_option(text, "Style", "S") {
            self.step = Step::Style;
            return ToolStep::next("");
        }
        match self.kind {
            Kind::Divide => self.divide(text),
            Kind::Measure => self.measure(text),
        }
    }

    fn enter(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Object => ToolStep::done(None, "", false),
            Step::Amount => ToolStep::rejected(match self.kind {
                Kind::Divide => "type the number of segments",
                Kind::Measure => "type the length of a segment",
            }),
            Step::Style => {
                self.step = Step::Amount; // the style stays as it is
                ToolStep::next("")
            }
        }
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Object => ToolStep::rejected("nothing to undo; no object has been picked yet"),
            Step::Amount => {
                self.step = Step::Object;
                self.path = None;
                ToolStep::next("")
            }
            Step::Style => {
                self.step = Step::Amount;
                ToolStep::next("")
            }
        }
    }

    fn preview(&self, _cx: &ToolContext<'_>, _cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        if let Some(path) = &self.path {
            // Where the stations are counted from.
            feedback
                .markers
                .push(path.point_at(if self.from_end { path.length() } else { 0.0 }));
        }
        feedback
    }
}

fn divide_tool(_cx: &ToolContext<'_>, start: &ToolStart) -> ToolPtr {
    Box::new(AlongTool::new(start, Kind::Divide))
}

fn measure_tool(_cx: &ToolContext<'_>, start: &ToolStart) -> ToolPtr {
    Box::new(AlongTool::new(start, Kind::Measure))
}

fn along(
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
        category: String::from("Draw"),
        group: String::from("Points"),
        order,
        aliases: aliases.iter().map(|&alias| String::from(alias)).collect(),
        tip: String::from(tip),
        make: Some(tool_factory(make)),
        ..ToolInfo::default()
    }
}

pub fn add_draw_divide_tools(catalog: &mut ToolCatalog, report: &mut Report<'_>) {
    report(catalog.add(along(
        "draw.divide",
        "Divide",
        10,
        &["DIVIDE", "DIV"],
        "Places points dividing a line, arc, circle or polyline into equal parts; S puts them \
         in a style, whose symbol they then show.",
        divide_tool,
    )));
    report(catalog.add(along(
        "draw.measure",
        "Measure",
        20,
        &["MEASURE", "ME"],
        "Places a point every given length along a line, arc, circle or polyline, from the end \
         nearer the pick; S puts them in a style.",
        measure_tool,
    )));
}
