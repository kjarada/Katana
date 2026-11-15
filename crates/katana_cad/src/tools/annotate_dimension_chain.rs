//! Baseline and continued dimensions
//! (src/katana_cad/tools/annotate_dimension_chain.cpp), as AutoCAD's
//! DIMBASELINE and DIMCONTINUE take them:
//!
//! ```text
//!   Specify next extension line origin (from dimension 7) or
//!   [Select/Spacing/Undo] <done>          each point one more dimension; Enter
//!                                         (or Esc) makes them all, and Enter
//!                                         with none placed ends the tool
//!   Select base dimension                 after Select, or when the drawing has
//!                                         no linear or aligned dimension yet
//! ```
//!
//! Baseline measures every point from the base's first origin, each dimension
//! line a spacing further out; Continue chains them end to end from the
//! base's second origin, every line on the base's. The chain is made by
//! annotation::dimension_build::dimension_chain - DIM BASELINE's and DIM
//! CONTINUE's own door - as ONE command, on the base's layer and in its
//! style: "DIM BASELINE 7 p p spacing=5" typed and this tool given the same
//! points make the same dimensions. Baseline's spacing is a text height and a
//! half of the base's style unless Spacing says otherwise (Continue has no
//! Spacing).
//!
//! The base is the drawing's newest linear or aligned dimension until Select
//! picks another: the one the last dimension tool made, as AutoCAD goes on
//! from the last dimension drawn. The drawing, not the program, remembers it:
//! after a chain its newest is the chain's last, so the tool, starting again,
//! goes on from there - Enter, Enter makes a chain and ends, as in AutoCAD. A
//! point snapped to an entity's end, middle, centre or vertex follows it, as
//! DIM BASELINE's #id points do.

use katana_core::{Error, ErrorCode, Result};
use katana_entity::annotation::DEFAULT_ANNOTATION_SCALE;
use katana_entity::{
    AnchorRef, DimensionGeometry, DimensionKind, Entity, EntityId, Geometry, Model,
};
use katana_geometry::Point2;

use crate::annotation::dimension_build::{
    AnchoredPoint, DimensionChain, baseline_dimensions, baseline_spacing, continued_dimensions,
    dimension_chain,
};
use crate::interactive_tool::{InteractiveTool, ToolContext, ToolInput, ToolStart, ToolStep};
use crate::tool_feedback::ToolFeedback;
use crate::tools::annotate_common::{default_entity, format_number, is_option, typed_number};

// The dimension geometry a chain can go on from: a linear or aligned one.
fn chainable(entity: Option<&Entity>) -> Option<&DimensionGeometry> {
    match &entity?.geometry {
        Geometry::Dimension(dimension)
            if dimension.kind == DimensionKind::Aligned
                || dimension.kind == DimensionKind::Linear =>
        {
            Some(dimension)
        }
        _ => None,
    }
}

// The newest linear or aligned dimension in the drawing; 0 when there is
// none.
fn newest_chainable(model: &Model) -> EntityId {
    let mut newest = 0;
    model.entities.for_each(|entity| {
        if chainable(Some(entity)).is_some() {
            newest = entity.id;
        }
    });
    newest
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Base,
    Next,
    Spacing,
}

struct ChainTool {
    kind: DimensionChain,
    scale: f64,
    step: Step,
    base: EntityId,
    // Select was typed: Undo at the base prompt goes back to the base it had.
    chose: bool,
    points: Vec<AnchoredPoint>,
    spacing: Option<f64>,
}

impl ChainTool {
    fn new(cx: &ToolContext<'_>, kind: DimensionChain) -> Self {
        let base = cx
            .document
            .map_or(0, |document| newest_chainable(document.model()));
        Self {
            kind,
            scale: cx.document.map_or(DEFAULT_ANNOTATION_SCALE, |document| {
                document.annotation_scale()
            }),
            step: if base != 0 { Step::Next } else { Step::Base },
            base,
            chose: false,
            points: Vec::new(),
            spacing: None,
        }
    }

    fn baseline(&self) -> bool {
        self.kind == DimensionChain::Baseline
    }

    fn base_entity<'a>(&self, cx: &ToolContext<'a>) -> Option<&'a Entity> {
        if self.base == 0 {
            return None;
        }
        cx.document?.model().entities.find(self.base)
    }

    fn base_geometry<'a>(&self, cx: &ToolContext<'a>) -> Option<&'a DimensionGeometry> {
        chainable(self.base_entity(cx))
    }

    // Spacing's value, else what a chain from the base takes by default.
    fn spacing(&self, cx: &ToolContext<'_>) -> f64 {
        if let Some(spacing) = self.spacing {
            return spacing;
        }
        match (cx.document, self.base_entity(cx)) {
            (Some(document), Some(base)) => baseline_spacing(document.model(), base, self.scale),
            _ => 0.0,
        }
    }

    fn chain_through(
        &self,
        cx: &ToolContext<'_>,
        points: &[AnchoredPoint],
    ) -> Result<Vec<DimensionGeometry>> {
        let Some(base) = self.base_geometry(cx) else {
            return Err(Error::new(
                ErrorCode::NotFound,
                "the base dimension is no longer in the drawing",
            ));
        };
        if self.baseline() {
            baseline_dimensions(base, points, self.spacing(cx))
        } else {
            continued_dimensions(base, points)
        }
    }

    fn place(&mut self, cx: &ToolContext<'_>, at: Point2, anchor: AnchorRef) -> ToolStep {
        if self.step == Step::Base {
            return ToolStep::rejected("pick the linear or aligned dimension to go on from");
        }
        if self.step == Step::Spacing {
            return ToolStep::rejected("type the spacing, or press Enter to keep it");
        }
        let mut points = self.points.clone();
        points.push(AnchoredPoint {
            point: at,
            r#ref: anchor,
        });
        // Each point is tried as it is given, so one that would make a
        // dimension of nothing is refused here and not at the end.
        if let Err(error) = self.chain_through(cx, &points) {
            return ToolStep::rejected(error.message);
        }
        self.points = points;
        ToolStep::next("")
    }

    fn finish(&self, cx: &ToolContext<'_>, restart: bool) -> ToolStep {
        if self.points.is_empty() {
            return ToolStep::done(None, "", false);
        }
        let Some(document) = cx.document else {
            return ToolStep::rejected("there is no drawing to dimension");
        };
        let built = match dimension_chain(
            document.model(),
            self.base,
            &self.points,
            self.kind,
            self.spacing,
            self.scale,
        ) {
            Ok(built) => built,
            Err(error) => return ToolStep::rejected(error.message),
        };
        let count = self.points.len();
        ToolStep::done(
            Some(built),
            format!(
                "{count}{}{}",
                if self.baseline() {
                    " baseline"
                } else {
                    " continued"
                },
                if count == 1 {
                    " dimension"
                } else {
                    " dimensions"
                }
            ),
            restart,
        )
    }
}

impl InteractiveTool for ChainTool {
    fn prompt(&self, cx: &ToolContext<'_>) -> String {
        match self.step {
            Step::Base => String::from("Select base dimension"),
            Step::Next => format!(
                "Specify next extension line origin (from dimension {}) or {}",
                self.base,
                if self.baseline() {
                    "[Select/Spacing/Undo] <done>"
                } else {
                    "[Select/Undo] <done>"
                }
            ),
            Step::Spacing => format!(
                "Specify baseline spacing <{}>",
                format_number(self.spacing(cx))
            ),
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        match self.step {
            Step::Base => ToolInput::Entity,
            Step::Next => ToolInput::Point,
            Step::Spacing => ToolInput::Value,
        }
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if self.step != Step::Base {
            return default_entity(cx, self.expects(cx), id, at);
        }
        let picked = cx
            .document
            .and_then(|document| document.model().entities.find(id));
        if chainable(picked).is_none() {
            return ToolStep::rejected("pick a linear or aligned dimension to go on from");
        }
        self.base = id;
        self.step = Step::Next;
        ToolStep::next("")
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        self.place(cx, at, AnchorRef::default())
    }

    fn anchored_point(&mut self, cx: &ToolContext<'_>, at: Point2, anchor: &AnchorRef) -> ToolStep {
        self.place(cx, at, *anchor)
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        match self.step {
            Step::Base => ToolStep::rejected("pick the linear or aligned dimension to go on from"),
            Step::Next => {
                if is_option(text, "Undo", 1) {
                    return self.undo(cx);
                }
                if is_option(text, "Select", 1) {
                    if !self.points.is_empty() {
                        return ToolStep::rejected(
                            "press Enter to make the dimensions placed first, then choose \
                             another base",
                        );
                    }
                    self.chose = true;
                    self.step = Step::Base;
                    return ToolStep::next("");
                }
                if self.baseline() && is_option(text, "Spacing", 2) {
                    self.step = Step::Spacing;
                    return ToolStep::next("");
                }
                ToolStep::rejected(if self.baseline() {
                    "click the next origin, type SP for the spacing or S to choose another base, \
                     or press Enter to finish"
                } else {
                    "click the next origin, type S to choose another base, or press Enter to \
                     finish"
                })
            }
            Step::Spacing => {
                if text.is_empty() {
                    return self.enter(cx);
                }
                let Some(spacing) = typed_number(text).filter(|&spacing| spacing > 0.0) else {
                    return ToolStep::rejected("the spacing is a distance greater than 0");
                };
                self.spacing = Some(spacing);
                self.step = Step::Next;
                ToolStep::next("")
            }
        }
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            // As AutoCAD's Select prompt: Enter ends the tool.
            Step::Base => ToolStep::done(None, "", false),
            Step::Next => self.finish(cx, true),
            Step::Spacing => {
                self.step = Step::Next;
                ToolStep::next("")
            }
        }
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Base => {
                if self.chose && self.base != 0 {
                    self.step = Step::Next; // back to the base it had
                    return ToolStep::next("");
                }
                ToolStep::rejected("nothing to undo: no dimension has been chosen")
            }
            Step::Next => {
                if self.points.pop().is_none() {
                    return ToolStep::rejected("nothing to undo: no origin has been given");
                }
                ToolStep::next("")
            }
            Step::Spacing => {
                self.step = Step::Next;
                ToolStep::next("")
            }
        }
    }

    // Esc keeps the dimensions already placed, as a LINE keeps its segments.
    fn cancel(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        if self.points.is_empty() {
            return ToolStep::done(None, "", false);
        }
        self.finish(cx, false)
    }

    fn preview(&self, cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        if self.step != Step::Next {
            return feedback;
        }
        let mut points = self.points.clone();
        points.push(AnchoredPoint {
            point: cursor,
            r#ref: AnchorRef::default(),
        });
        if let Ok(chain) = self.chain_through(cx, &points) {
            for dimension in chain {
                feedback.shapes.push(Geometry::Dimension(dimension));
            }
        }
        for point in &self.points {
            feedback.markers.push(point.point);
        }
        feedback
    }

    fn last_point(&self, cx: &ToolContext<'_>) -> Option<Point2> {
        if let Some(last) = self.points.last() {
            return Some(last.point);
        }
        self.base_geometry(cx).map(|base| base.end)
    }
}

/// The Baseline Dimension tool.
pub(crate) fn make_baseline_dimension_tool(
    cx: &ToolContext<'_>,
    _start: &ToolStart,
) -> Box<dyn InteractiveTool> {
    Box::new(ChainTool::new(cx, DimensionChain::Baseline))
}

/// The Continue Dimension tool.
pub(crate) fn make_continue_dimension_tool(
    cx: &ToolContext<'_>,
    _start: &ToolStart,
) -> Box<dyn InteractiveTool> {
    Box::new(ChainTool::new(cx, DimensionChain::Continued))
}
