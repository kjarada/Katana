//! Leader (src/katana_cad/tools/annotate_leader.cpp): an arrow line to a
//! feature with a note at its end, as AutoCAD's LEADER draws one.
//!
//! ```text
//!   Specify leader start point or [Arrow/Callout/Style/Paper/SIze/Landing]
//!                                                       the arrow's tip, or an
//!                                                       option for this leader
//!   Specify next point or [Undo]                        the first bend or end
//!   Specify next point or [Annotation/Undo] <Annotation>  more bends, or Enter
//!   Enter the first line of annotation text             typed, line by line;
//!                                                       an empty line finishes
//! ```
//!
//! The leader is ONE entity, a LeaderGeometry (docs/annotation.md, "Leaders
//! and callouts"), added by one command: the arrow, the line, the landing and
//! the note are drawn from it at the scale of the view or sheet looking at
//! it, and move, copy and erase together. It is the LEADER verb's entity.
//! (Before annotation had its own entities the tool drew a polyline, an
//! arrowhead and a text per line, which drifted apart when one was moved.)
//!
//! The options at the first prompt are LEADER's arrow=, callout=, style=,
//! paper=, arrowsize= and landing=: the arrowhead (closed, open, tick, dot or
//! none), a box or circle round the note, the note's text style, its height
//! on paper, the arrowhead's size on paper (SIze - S is Style's) and the
//! landing's length on paper (0 for none; Auto for the rule below). They are
//! for the leader being drawn, as AutoCAD's LEADER options are, so the next
//! takes the dimension style's again. Until SIze and Landing, only the verb
//! could give a leader its own arrowhead or landing, and nothing could after
//! it was drawn. A tip snapped to an entity's end, middle, centre or vertex
//! follows it (tip_ref), as LEADER #id.end ... does.
//!
//! Sizes come from the dimension style the current layer resolves to, as an
//! AutoCAD leader takes DIMASZ and DIMTXT from its dimension style: the arrow
//! is the style's head at arrow_size and the note text_height tall. A
//! leader's sizes are paper millimetres; a paper-sized style's are taken as
//! they are, and a model-unit style's are what its model size is on paper at
//! the document's annotation scale - the leader is made the size it would
//! have been drawn, and from then on keeps its size on paper as the scale
//! changes.
//!
//! The note follows the last segment (annotation::leader_draw): to the right
//! of the end when the line arrives heading right, or straight up or down
//! within the geometric tolerance, to the left when it arrives heading left.
//! When the last segment is more than 15 degrees off horizontal the leader
//! gets a horizontal landing one arrowhead long first, as AutoCAD adds a
//! hook, so the note never hangs off a slope; a leader within 15 degrees of
//! level, or with no note, has none.
//!
//! A SMART leader (docs/annotation.md, "Smart leaders"): a tip put on a
//! point, a line, an arc, a circle, a polyline or a text - within the view's
//! pick aperture of it - is put ON it and follows it, as the command line's
//! #id@x,y does. A tip snapped to one of its points keeps that point instead.
//! The note of such a leader may then name the entity's values in braces, the
//! label template language: "IL {prop.invert:.3f}" is read off the pit every
//! time the leader is drawn. A line with a brace is checked as it is typed
//! ({{ and }} are a brace), and a note that would say nothing about the
//! entity is refused with what the entity lacks. A tip on nothing keeps its
//! note as typed, braces and all.
//!
//! The C++ refuses a note line that is not UTF-8; a Rust line is a `&str`,
//! which always is.

use katana_commands::EntityAttributes;
use katana_entity::anchor::{nearest_anchor, resolve_anchor};
use katana_entity::annotation::{
    DEFAULT_ANNOTATION_SCALE, DEFAULT_TEXT_STYLE_NAME, annotation_paper_size,
};
use katana_entity::entity::to_string;
use katana_entity::leader_values::{check_leader_says_something, check_leader_template};
use katana_entity::{AnchorRef, ArrowHead, CalloutShape, DimensionStyle, Geometry, LeaderGeometry};
use katana_geometry::Point2;
use katana_math::cpp;
use katana_math::numerics::{DEG_TO_RAD, tolerance};

use crate::annotation::leader_draw::code_properties;
use crate::annotation::leader_presets::leader_context;
use crate::interactive_tool::{InteractiveTool, ToolContext, ToolInput, ToolStart, ToolStep};
use crate::selection::{SelectionFilter, pick_entity};
use crate::tool_feedback::ToolFeedback;
use crate::tools::annotate_common::{
    coincident, create_all, default_point, format_number, is_option, new_entity,
    style_for_new_annotation, text_style_named, tip_targets, trimmed, typed_number,
};

const HOOK_ANGLE: f64 = 15.0 * DEG_TO_RAD;

// Whether a typed line of the note has a field in it.
fn has_field(line: &str) -> bool {
    line.contains(['{', '}'])
}

// What the options at the first prompt were told; unset takes the dimension
// style's arrow and height, no callout and the default text style.
#[derive(Clone, Debug, Default, PartialEq)]
struct Options {
    arrow: Option<ArrowHead>,
    callout: Option<CalloutShape>,
    text_style: Option<String>,
    // mm
    paper: Option<f64>,
    // mm
    arrow_size: Option<f64>,
    // mm; 0 for none, None for the rule
    landing: Option<f64>,
}

// The arrowheads and callouts by the words their options offer, in order.
const ARROWS: [(ArrowHead, &str); 5] = [
    (ArrowHead::ClosedFilled, "Closed"),
    (ArrowHead::Open, "Open"),
    (ArrowHead::Tick, "Tick"),
    (ArrowHead::Dot, "Dot"),
    (ArrowHead::None, "None"),
];
const CALLOUTS: [(CalloutShape, &str); 3] = [
    (CalloutShape::None, "None"),
    (CalloutShape::Box, "Box"),
    (CalloutShape::Circle, "Circle"),
];

fn word_for<V: PartialEq + Copy>(words: &[(V, &str)], value: V) -> String {
    for &(each, word) in words {
        if each == value {
            return String::from(word);
        }
    }
    String::new()
}

// "Closed, Open, Tick, Dot, None" as an option list: "Closed/Open/...".
fn option_list<V>(words: &[(V, &str)]) -> String {
    let mut list = String::new();
    for (_, word) in words {
        if !list.is_empty() {
            list.push('/');
        }
        list.push_str(word);
    }
    list
}

// The leader through `vertices` with note `lines`, sized from `style` at
// 1 : `scale` unless `options` say otherwise, its tip following `tip` and its
// note a template when `fields`. `vertices` has at least two points, none
// coincident with the next.
fn leader_for(
    vertices: Vec<Point2>,
    lines: &[String],
    style: &DimensionStyle,
    scale: f64,
    options: &Options,
    tip: AnchorRef,
    fields: bool,
) -> LeaderGeometry {
    let paper = |size: f64| {
        if style.paper_sized {
            size
        } else {
            annotation_paper_size(size, scale)
        }
    };
    let last = vertices[vertices.len() - 1] - vertices[vertices.len() - 2];
    let steep = last.y.abs().atan2(last.x.abs()) > HOOK_ANGLE;
    let mut text = String::new();
    for line in lines {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(line);
    }
    let arrow_size = options.arrow_size.unwrap_or(paper(style.arrow_size));
    let fields = fields && tip.associated() && !text.is_empty();
    LeaderGeometry {
        vertices,
        arrow: options.arrow.unwrap_or(style.arrow_head),
        callout: options.callout.unwrap_or(CalloutShape::None),
        style: options.text_style.clone().unwrap_or_default(),
        arrow_size,
        paper_height: options.paper.unwrap_or(paper(style.text_height)),
        landing: options.landing.unwrap_or(if !lines.is_empty() && steep {
            arrow_size
        } else {
            0.0
        }),
        tip_ref: tip,
        fields,
        text,
        ..LeaderGeometry::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Tip,
    Vertices,
    Annotation,
    Arrow,
    Callout,
    Style,
    Paper,
    ArrowSize,
    Landing,
}

struct LeaderTool {
    pick_tolerance: f64,
    attributes: EntityAttributes,
    style: DimensionStyle,
    scale: f64,

    step: Step,
    options: Options,
    vertices: Vec<Point2>,
    // The entity point the tip was snapped to; unassociated when it was
    // clicked or typed.
    tip_ref: AnchorRef,
    // Set only while anchored_point hands a snapped point to point().
    pending: AnchorRef,
    lines: Vec<String>,
}

impl LeaderTool {
    fn new(cx: &ToolContext<'_>, start: &ToolStart) -> Self {
        Self {
            pick_tolerance: cx.pick_tolerance,
            attributes: start.attributes.clone(),
            style: style_for_new_annotation(cx, &start.attributes),
            scale: cx.document.map_or(DEFAULT_ANNOTATION_SCALE, |document| {
                document.annotation_scale()
            }),
            step: Step::Tip,
            options: Options::default(),
            vertices: Vec::new(),
            tip_ref: AnchorRef::default(),
            pending: AnchorRef::default(),
            lines: Vec::new(),
        }
    }

    fn ask(&mut self, option: Step) -> ToolStep {
        self.step = option;
        ToolStep::next("")
    }

    // The leader through `vertices` with note `lines`, a template when the
    // tip is on an entity and a line names a field.
    fn leader(&self, vertices: Vec<Point2>, lines: &[String]) -> LeaderGeometry {
        let fields = self.tip_ref.associated() && lines.iter().any(|line| has_field(line));
        leader_for(
            vertices,
            lines,
            &self.style,
            self.scale,
            &self.options,
            self.tip_ref,
            fields,
        )
    }

    // Whether the note typed so far is a template.
    fn smart(&self) -> bool {
        self.leader(self.vertices.clone(), &self.lines).fields
    }

    // The entity under a tip at `at` and the place on it nearest `at`: what
    // the view's pick aperture finds (at least the geometric tolerance, for a
    // point typed exactly on a line), of the kinds a tip can be on.
    fn tip_on(&self, cx: &ToolContext<'_>, at: Point2) -> Option<(AnchorRef, Point2)> {
        let document = cx.document?;
        let model = document.model();
        let filter = SelectionFilter {
            types: tip_targets(),
            ..SelectionFilter::default()
        };
        let picked = pick_entity(
            model,
            at,
            cpp::max(self.pick_tolerance, tolerance::GEOMETRIC),
            &filter,
            Some(document.spatial_index()),
        )?;
        let entity = model.entities.find(picked)?;
        let reference = nearest_anchor(entity, at)?;
        let point = resolve_anchor(entity, &reference)?;
        Some((reference, point))
    }

    fn paper_height(&self) -> f64 {
        self.options
            .paper
            .unwrap_or(self.on_paper(self.style.text_height))
    }

    fn arrow_size(&self) -> f64 {
        self.options
            .arrow_size
            .unwrap_or(self.on_paper(self.style.arrow_size))
    }

    // A size of the dimension style on paper, as leader_for takes it.
    fn on_paper(&self, size: f64) -> f64 {
        if self.style.paper_sized {
            size
        } else {
            annotation_paper_size(size, self.scale)
        }
    }

    // The options given so far, after the first prompt: nothing else on
    // screen shows them before the leader is drawn.
    fn settings(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(arrow) = self.options.arrow {
            parts.push(format!("{} arrow", word_for(&ARROWS, arrow)));
        }
        if let Some(callout) = self.options.callout {
            parts.push(format!("{} callout", word_for(&CALLOUTS, callout)));
        }
        if let Some(style) = &self.options.text_style {
            parts.push(format!("style {style}"));
        }
        if let Some(paper) = self.options.paper {
            parts.push(format!("{} mm", format_number(paper)));
        }
        if let Some(size) = self.options.arrow_size {
            parts.push(format!("arrowhead {} mm", format_number(size)));
        }
        if let Some(landing) = self.options.landing {
            parts.push(format!("landing {} mm", format_number(landing)));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!(" ({})", parts.join(", "))
        }
    }

    fn style_value(&mut self, cx: &ToolContext<'_>, typed: &str) -> ToolStep {
        if trimmed(typed).is_empty() {
            return self.enter(cx);
        }
        match text_style_named(cx.document, typed) {
            Err(error) => ToolStep::rejected(error.message),
            Ok(found) => {
                self.options.text_style = Some(found);
                self.step = Step::Tip;
                ToolStep::next("")
            }
        }
    }
}

impl InteractiveTool for LeaderTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        match self.step {
            Step::Tip => format!(
                "Specify leader start point or [Arrow/Callout/Style/Paper/SIze/Landing]{}",
                self.settings()
            ),
            Step::Vertices => String::from(if self.vertices.len() < 2 {
                "Specify next point or [Undo]"
            } else {
                "Specify next point or [Annotation/Undo] <Annotation>"
            }),
            Step::Annotation => String::from(if self.lines.is_empty() {
                "Enter the first line of annotation text, or press Enter for none"
            } else {
                "Enter the next line of annotation text, or press Enter to finish"
            }),
            Step::Arrow => format!(
                "Enter arrowhead [{}] <{}>",
                option_list(&ARROWS),
                word_for(&ARROWS, self.options.arrow.unwrap_or(self.style.arrow_head))
            ),
            Step::Callout => format!(
                "Enter callout [{}] <{}>",
                option_list(&CALLOUTS),
                word_for(
                    &CALLOUTS,
                    self.options.callout.unwrap_or(CalloutShape::None)
                )
            ),
            Step::Style => format!(
                "Enter text style name <{}>",
                self.options
                    .text_style
                    .as_deref()
                    .unwrap_or(DEFAULT_TEXT_STYLE_NAME)
            ),
            Step::Paper => format!(
                "Enter the note's height on paper, mm <{}>",
                format_number(self.paper_height())
            ),
            Step::ArrowSize => format!(
                "Enter the arrowhead's size on paper, mm <{}>",
                format_number(self.arrow_size())
            ),
            Step::Landing => format!(
                "Enter the landing's length on paper, mm, 0 for none, or Auto <{}>",
                match self.options.landing {
                    Some(landing) => format_number(landing),
                    None => String::from("Auto"),
                }
            ),
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.step == Step::Tip || self.step == Step::Vertices {
            ToolInput::Point
        } else {
            ToolInput::Value
        }
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        match self.step {
            Step::Tip => {
                // A snap to one of an entity's points is that point; else a
                // tip within the pick aperture of an entity goes on it, at
                // the place nearest the click; else it is free.
                self.vertices = vec![at];
                self.tip_ref = self.pending;
                if !self.tip_ref.associated()
                    && let Some((reference, point)) = self.tip_on(cx, at)
                {
                    self.vertices = vec![point];
                    self.tip_ref = reference;
                }
                let mut message = String::new();
                if self.tip_ref.associated()
                    && let Some(document) = cx.document
                    && let Some(target) = document.model().entities.find(self.tip_ref.entity)
                {
                    message = format!(
                        "leader on {} {}; its note may name its values in braces, e.g. \
                         {{length:.2f}}",
                        to_string(target.r#type()),
                        target.id
                    );
                }
                self.step = Step::Vertices;
                ToolStep::next(message)
            }
            Step::Vertices => {
                if self
                    .vertices
                    .last()
                    .is_some_and(|&last| coincident(last, at))
                {
                    return ToolStep::rejected(
                        "that point is on the previous one; each leader segment needs a length",
                    );
                }
                self.vertices.push(at);
                ToolStep::next("")
            }
            Step::Annotation
            | Step::Arrow
            | Step::Callout
            | Step::Style
            | Step::Paper
            | Step::ArrowSize
            | Step::Landing => default_point(cx, self.expects(cx), at),
        }
    }

    // Only the tip follows what it was snapped to; the bends are where they
    // were put.
    fn anchored_point(&mut self, cx: &ToolContext<'_>, at: Point2, anchor: &AnchorRef) -> ToolStep {
        self.pending = *anchor;
        let step = self.point(cx, at);
        self.pending = AnchorRef::default();
        step
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        match self.step {
            Step::Tip => {
                if is_option(text, "Arrow", 1) {
                    return self.ask(Step::Arrow);
                }
                if is_option(text, "Callout", 1) {
                    return self.ask(Step::Callout);
                }
                if is_option(text, "Style", 1) {
                    return self.ask(Step::Style);
                }
                if is_option(text, "Paper", 1) {
                    return self.ask(Step::Paper);
                }
                if is_option(text, "Size", 2) {
                    return self.ask(Step::ArrowSize);
                }
                if is_option(text, "Landing", 1) {
                    return self.ask(Step::Landing);
                }
                if is_option(text, "Undo", 1) {
                    return self.undo(cx);
                }
                ToolStep::rejected(
                    "click the point the arrow touches or type it as x,y, or type A, C, S, P, SI \
                     or L for this leader's options",
                )
            }
            Step::Vertices => {
                if is_option(text, "Undo", 1) {
                    return self.undo(cx);
                }
                if is_option(text, "Annotation", 1) {
                    return self.enter(cx);
                }
                ToolStep::rejected(
                    "click the next point, or type A or press Enter for the annotation",
                )
            }
            Step::Annotation => {
                if text.is_empty() {
                    return self.enter(cx);
                }
                if self.tip_ref.associated()
                    && has_field(text)
                    && let Err(error) = check_leader_template(text)
                {
                    return ToolStep::rejected(format!("{}: {}", error.message, error.context));
                }
                self.lines.push(String::from(text));
                ToolStep::next("")
            }
            Step::Arrow => {
                for (head, word) in ARROWS {
                    if is_option(text, word, 1) {
                        self.options.arrow = Some(head);
                        self.step = Step::Tip;
                        return ToolStep::next("");
                    }
                }
                ToolStep::rejected("type C, O, T, D or N: closed, open, tick, dot or none")
            }
            Step::Callout => {
                for (shape, word) in CALLOUTS {
                    if is_option(text, word, 1) {
                        self.options.callout = Some(shape);
                        self.step = Step::Tip;
                        return ToolStep::next("");
                    }
                }
                ToolStep::rejected("type N, B or C: none, a box or a circle")
            }
            Step::Style => self.style_value(cx, text),
            Step::Paper => {
                let Some(height) = typed_number(text).filter(|&height| height > 0.0) else {
                    return ToolStep::rejected(
                        "the note's height on paper is millimetres greater than 0",
                    );
                };
                self.options.paper = Some(height);
                self.step = Step::Tip;
                ToolStep::next("")
            }
            Step::ArrowSize => {
                let Some(size) = typed_number(text).filter(|&size| size > 0.0) else {
                    return ToolStep::rejected(
                        "the arrowhead's size on paper is millimetres greater than 0",
                    );
                };
                self.options.arrow_size = Some(size);
                self.step = Step::Tip;
                ToolStep::next("")
            }
            Step::Landing => {
                if is_option(text, "Auto", 1) {
                    self.options.landing = None;
                    self.step = Step::Tip;
                    return ToolStep::next("");
                }
                let Some(length) = typed_number(text).filter(|&length| !(length < 0.0)) else {
                    return ToolStep::rejected(
                        "the landing is millimetres on paper, 0 for none, or Auto for one \
                         arrowhead where the line slopes",
                    );
                };
                self.options.landing = Some(length);
                self.step = Step::Tip;
                ToolStep::next("")
            }
        }
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Tip => return ToolStep::done(None, "", false),
            Step::Vertices => {
                if self.vertices.len() < 2 {
                    return ToolStep::rejected("a leader needs a second point");
                }
                self.step = Step::Annotation;
                return ToolStep::next("");
            }
            Step::Arrow
            | Step::Callout
            | Step::Style
            | Step::Paper
            | Step::ArrowSize
            | Step::Landing => {
                // What the option shows is kept.
                self.step = Step::Tip;
                return ToolStep::next("");
            }
            Step::Annotation => {}
        }
        let made = self.leader(self.vertices.clone(), &self.lines);
        // As it will be drawn: a note reading a surface or an alignment reads
        // the drawing's.
        if let Some(document) = cx.document
            && let Err(error) = check_leader_says_something(
                document.model(),
                &made,
                code_properties(),
                &leader_context(document),
            )
        {
            return ToolStep::rejected(format!("{} - type U to take back a line", error.message));
        }
        let smart = self.smart();
        let entities = vec![new_entity(Geometry::Leader(made), &self.attributes)];
        let count = self.lines.len();
        let mut message = match count {
            0 => String::from("leader with no text"),
            1 => String::from("leader with 1 line of text"),
            _ => format!("leader with {count} lines of text"),
        };
        if smart {
            message.push_str(&format!(", read off entity {}", self.tip_ref.entity));
        } else if self.tip_ref.associated() {
            message.push_str(&format!(", on entity {}", self.tip_ref.entity));
        }
        ToolStep::done(Some(create_all("CREATE_LEADER", entities)), message, true)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Tip => ToolStep::rejected("nothing to undo: no point has been given"),
            Step::Vertices => {
                self.vertices.pop();
                if self.vertices.is_empty() {
                    self.tip_ref = AnchorRef::default();
                    self.step = Step::Tip;
                }
                ToolStep::next("")
            }
            Step::Annotation => {
                if self.lines.pop().is_none() {
                    self.step = Step::Vertices;
                }
                ToolStep::next("")
            }
            Step::Arrow
            | Step::Callout
            | Step::Style
            | Step::Paper
            | Step::ArrowSize
            | Step::Landing => {
                self.step = Step::Tip;
                ToolStep::next("")
            }
        }
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        match self.step {
            Step::Tip
            | Step::Arrow
            | Step::Callout
            | Step::Style
            | Step::Paper
            | Step::ArrowSize
            | Step::Landing => {}
            Step::Vertices => {
                // The leader as it would be with the cursor as its next
                // point, arrowhead included, so its size is seen before it is
                // placed.
                if let Some(&last) = self.vertices.last() {
                    let mut vertices = self.vertices.clone();
                    if !coincident(last, cursor) {
                        vertices.push(cursor);
                    }
                    if vertices.len() >= 2 {
                        feedback
                            .shapes
                            .push(Geometry::Leader(self.leader(vertices, &[])));
                    }
                    feedback.markers.push(last);
                }
            }
            Step::Annotation => {
                // As it will be: a smart note already read off its entity.
                feedback.shapes.push(Geometry::Leader(
                    self.leader(self.vertices.clone(), &self.lines),
                ));
            }
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.vertices.last().copied()
    }
}

/// The Leader tool.
pub(crate) fn make_leader_tool(
    cx: &ToolContext<'_>,
    start: &ToolStart,
) -> Box<dyn InteractiveTool> {
    Box::new(LeaderTool::new(cx, start))
}
