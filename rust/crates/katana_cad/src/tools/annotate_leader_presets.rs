//! The preset leaders by hand (src/katana_cad/tools/annotate_leader_presets.cpp;
//! docs/annotation.md, "Presets"; docs/tools.md): Level Leader, Surface
//! Leader, Cut/Fill Leader, Point Name Leader, Code Leader and Station
//! Leader, one click each on the Annotate menu and toolbar - the owner's
//! request of 2026-10-15 for "a leader to display a point's height, a leader
//! to get a point's difference to a TIN, a leader to display names".
//!
//! ```text
//!   Pick what the note reads [Surface]           a click on a point, a line
//!                                                 or what else the tip goes
//!                                                 on - or a snap to its end,
//!                                                 middle, vertex
//!   Specify where the note goes <up and right>   a click; Enter puts it
//!                                                 where LEADER FOR does
//!   Enter the surface name <ground>              after S (A for a station's
//!                                                 alignment)
//! ```
//!
//! Each makes exactly the leader "LEADER #id@x,y x,y preset=level" makes -
//! through the same door, annotation::leader_presets::resolve_leader_preset
//! and annotation::leader_edit::new_leader - as ONE command, and starts again
//! for the next point, so a row of spot levels is a click and a click each.
//! The note is a smart leader's: read live off the point and the surface,
//! however either changes. A point that has nothing to say - no level, off
//! the surface - is refused at the first click with why, before a note's
//! place is asked for.
//!
//! The surface (or the alignment) offered is the one the drawing's newest
//! leader of the same preset reads - remembered by the drawing, not the
//! program, as Label Objects offers the newest label's style - else none,
//! which reads the first visible surface (the first alignment) and says so.

use katana_commands::EntityAttributes;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::anchor::nearest_anchor;
use katana_entity::annotation::{DEFAULT_ANNOTATION_SCALE, annotation_model_size};
use katana_entity::entity::to_string;
use katana_entity::{AnchorRef, Geometry, LeaderGeometry};
use katana_geometry::{Point2, Vec2};

use crate::annotation::dimension_build::{AnchoredPoint, anchored_point};
use crate::annotation::leader_edit::{
    LeaderChange, LeaderNote, LeaderNoteKind, LeadersForOptions, leader_says, new_leader,
};
use crate::annotation::leader_presets::{
    LeaderPreset, PresetReads, ResolvedPreset, leader_context, preset_info, recognise_preset,
    resolve_leader_preset,
};
use crate::document::Document;
use crate::interactive_tool::{
    InteractiveTool, ToolContext, ToolInput, ToolStart, ToolStep, pick_under,
};
use crate::tool_feedback::ToolFeedback;
use crate::tools::annotate_common::{
    coincident, create_all, default_point, is_option, new_entity, tip_targets, trimmed,
};

// The surface or alignment the newest leader made with `preset` reads, if
// the drawing still has it; empty for none.
fn newest_reads(document: &Document, preset: LeaderPreset) -> String {
    let mut reads = String::new();
    document.model().entities.for_each(|entity| {
        let Geometry::Leader(leader) = &entity.geometry else {
            return;
        };
        if !leader.fields {
            return;
        }
        if let Some(recognised) = recognise_preset(&leader.text)
            && recognised.preset == preset
        {
            reads = recognised.reads;
        }
    });
    let surface = preset_info(preset).reads == PresetReads::Surface;
    let there = if surface {
        document.surfaces().find(&reads).is_some()
    } else {
        document.model().alignments.contains(&reads)
    };
    if there { reads } else { String::new() }
}

// The drawing a preset reads. The C++ reads its document without asking -
// the tip's pick needs one, so a tool past its first step has one - and here
// the missing one is said rather than assumed.
fn document_of<'a>(cx: &ToolContext<'a>) -> Result<&'a Document> {
    cx.document
        .ok_or_else(|| Error::new(ErrorCode::InvalidState, "there is no drawing to read"))
}

// A note's lines on one line of the command log.
fn one_line(text: String) -> String {
    text.replace('\n', " ")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Tip,
    Note,
    Name,
}

struct PresetLeaderTool {
    attributes: EntityAttributes,
    preset: LeaderPreset,
    step: Step,
    // The surface or alignment chosen; empty for the preset's own choice.
    reads: String,
    tip: AnchoredPoint,
}

impl PresetLeaderTool {
    fn new(cx: &ToolContext<'_>, start: &ToolStart, preset: LeaderPreset) -> Self {
        let mut reads = String::new();
        if let Some(document) = cx.document
            && preset_info(preset).reads != PresetReads::Target
        {
            reads = newest_reads(document, preset);
        }
        Self {
            attributes: start.attributes.clone(),
            preset,
            step: Step::Tip,
            reads,
            tip: AnchoredPoint::default(),
        }
    }

    fn reads(&self) -> bool {
        preset_info(self.preset).reads != PresetReads::Target
    }

    fn surface_reader(&self) -> bool {
        preset_info(self.preset).reads == PresetReads::Surface
    }

    fn noun(&self) -> &'static str {
        if self.surface_reader() {
            "surface"
        } else {
            "alignment"
        }
    }

    fn first_what(&self) -> &'static str {
        if self.surface_reader() {
            "visible surface"
        } else {
            "alignment"
        }
    }

    fn option_word(&self) -> &'static str {
        if self.surface_reader() {
            "Surface"
        } else {
            "Alignment"
        }
    }

    fn option(&self) -> String {
        if self.reads() {
            format!(" [{}]", self.option_word())
        } else {
            String::new()
        }
    }

    // What is read, after the prompt: nothing else on screen says.
    fn reading(&self) -> String {
        if !self.reads() {
            return String::new();
        }
        format!(" ({}: {})", self.noun(), self.reads_said())
    }

    // The surface or alignment chosen, or the one the preset takes itself.
    fn reads_said(&self) -> String {
        if self.reads.is_empty() {
            format!("the first {}", self.first_what())
        } else {
            self.reads.clone()
        }
    }

    fn scale(&self, cx: &ToolContext<'_>) -> f64 {
        cx.document.map_or(DEFAULT_ANNOTATION_SCALE, |document| {
            document.annotation_scale()
        })
    }

    // The preset for the drawing as it is: the surface or alignment chosen,
    // or the preset's own choice.
    fn resolve(&self, cx: &ToolContext<'_>) -> Result<ResolvedPreset> {
        let document = document_of(cx)?;
        let chosen = (!self.reads.is_empty()).then_some(self.reads.as_str());
        let (surface, alignment) = if self.surface_reader() {
            (chosen, None)
        } else {
            (None, chosen)
        };
        resolve_leader_preset(document, self.preset, surface, alignment)
    }

    // The leader from the tip to `note`, as LEADER makes it: new_leader with
    // the preset's template. Refused as LEADER refuses it.
    fn make(&self, cx: &ToolContext<'_>, note: Point2) -> Result<LeaderGeometry> {
        let resolved = self.resolve(cx)?;
        let document = document_of(cx)?;
        let change = LeaderChange {
            note: Some(LeaderNote {
                kind: LeaderNoteKind::Template,
                text: resolved.template_text,
            }),
            ..LeaderChange::default()
        };
        new_leader(
            document.model(),
            &[
                self.tip,
                AnchoredPoint {
                    point: note,
                    r#ref: AnchorRef::default(),
                },
            ],
            &change,
            false,
            &leader_context(document),
        )
    }

    // The tip put on what `reference` names, checked for having something to
    // say there before the note's place is asked.
    fn take_tip(&mut self, cx: &ToolContext<'_>, reference: &AnchorRef) -> ToolStep {
        if !reference.associated() {
            return ToolStep::rejected(
                "pick the point (or the line) the note reads: nothing is under the cursor",
            );
        }
        let document = match document_of(cx) {
            Ok(document) => document,
            Err(error) => return ToolStep::rejected(error.message),
        };
        match anchored_point(document.model(), reference) {
            Ok(anchored) => self.tip = anchored,
            Err(error) => return ToolStep::rejected(error.message),
        }
        // Any second point will do to ask: what the note says depends on
        // the tip alone.
        let trial = match self.make(cx, self.tip.point + Vec2::new(1.0, 1.0)) {
            Ok(trial) => trial,
            Err(error) => {
                let context = if error.context.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", error.context)
                };
                return ToolStep::rejected(format!("{}{context}", error.message));
            }
        };
        self.step = Step::Note;
        let on = match document.model().entities.find(reference.entity) {
            Some(target) => format!("{} {}", to_string(target.r#type()), target.id),
            None => String::from("it"),
        };
        ToolStep::next(format!(
            "{} leader on {on}: {}",
            preset_info(self.preset).title,
            one_line(leader_says(
                document.model(),
                &trial,
                &leader_context(document)
            ))
        ))
    }

    fn place(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if coincident(self.tip.point, at) {
            return ToolStep::rejected("that is the tip itself; the note goes somewhere off it");
        }
        let leader = match self.make(cx, at) {
            Ok(leader) => leader,
            Err(error) => return ToolStep::rejected(error.message),
        };
        let document = match document_of(cx) {
            Ok(document) => document,
            Err(error) => return ToolStep::rejected(error.message),
        };
        let says = one_line(leader_says(
            document.model(),
            &leader,
            &leader_context(document),
        ));
        let mut message = format!("leader {}: {says}", preset_info(self.preset).word);
        if let Ok(resolved) = self.resolve(cx)
            && !resolved.chosen.is_empty()
        {
            message.push_str(&format!(
                " (reads {}, the {})",
                resolved.reads, resolved.chosen
            ));
        }
        let entities = vec![new_entity(Geometry::Leader(leader), &self.attributes)];
        ToolStep::done(Some(create_all("CREATE_LEADER", entities)), message, true)
    }
}

impl InteractiveTool for PresetLeaderTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        match self.step {
            Step::Tip => format!(
                "Pick what the note reads{}{}",
                self.option(),
                self.reading()
            ),
            Step::Note => String::from(
                "Specify where the note goes, or press Enter for up and to the right [Undo]",
            ),
            Step::Name => format!("Enter the {} name <{}>", self.noun(), self.reads_said()),
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.step == Step::Name {
            ToolInput::Value
        } else {
            ToolInput::Point
        }
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        match self.step {
            Step::Tip => {
                let Some(document) = cx.document else {
                    return ToolStep::rejected("there is no drawing to read");
                };
                let reference = pick_under(cx, at, &tip_targets(), None)
                    .and_then(|picked| document.model().entities.find(picked))
                    .and_then(|entity| nearest_anchor(entity, at))
                    .unwrap_or_default();
                self.take_tip(cx, &reference)
            }
            Step::Note => self.place(cx, at),
            Step::Name => default_point(cx, self.expects(cx), at),
        }
    }

    // A snap to a point of an entity - its end, its middle, a vertex - is
    // that point, as #id.end on the command line is.
    fn anchored_point(&mut self, cx: &ToolContext<'_>, at: Point2, anchor: &AnchorRef) -> ToolStep {
        if self.step == Step::Tip && anchor.associated() {
            return self.take_tip(cx, anchor);
        }
        self.point(cx, at)
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        match self.step {
            Step::Tip => {
                if self.reads() && is_option(text, self.option_word(), 1) {
                    self.step = Step::Name;
                    return ToolStep::next("");
                }
                let choose = if self.reads() {
                    format!(
                        ", or type {} to choose the {}",
                        &self.option_word()[..1],
                        self.noun()
                    )
                } else {
                    String::new()
                };
                ToolStep::rejected(format!(
                    "click the point (or the line) the note reads{choose}"
                ))
            }
            Step::Note => {
                if is_option(text, "Undo", 1) {
                    return self.undo(cx);
                }
                ToolStep::rejected("click where the note goes, or press Enter")
            }
            Step::Name => {
                let name = trimmed(text);
                if name.is_empty() {
                    return self.enter(cx);
                }
                let Some(document) = cx.document else {
                    return ToolStep::rejected("there is no drawing to read");
                };
                let (surface, alignment) = if self.surface_reader() {
                    (Some(name), None)
                } else {
                    (None, Some(name))
                };
                match resolve_leader_preset(document, self.preset, surface, alignment) {
                    Err(error) => {
                        ToolStep::rejected(format!("{}: {}", error.message, error.context))
                    }
                    Ok(resolved) => {
                        self.reads = resolved.reads;
                        self.step = Step::Tip;
                        ToolStep::next("")
                    }
                }
            }
        }
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Tip => ToolStep::done(None, "", false),
            Step::Note => {
                // LEADER FOR's place for a note: its own direction and
                // distance on paper, from their one source, at the drawing's
                // scale.
                let defaults = LeadersForOptions::default();
                let reach = annotation_model_size(defaults.length, self.scale(cx));
                let at =
                    self.tip.point + Vec2::new(defaults.angle.cos(), defaults.angle.sin()) * reach;
                self.place(cx, at)
            }
            Step::Name => {
                self.step = Step::Tip;
                ToolStep::next("")
            }
        }
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.step == Step::Note || self.step == Step::Name {
            self.step = Step::Tip;
            return ToolStep::next("");
        }
        ToolStep::rejected("nothing to undo: no point has been picked")
    }

    fn preview(&self, cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        if self.step != Step::Note {
            return feedback;
        }
        // The leader as it will be, its note already read off the point.
        if !coincident(self.tip.point, cursor)
            && let Ok(leader) = self.make(cx, cursor)
        {
            feedback.shapes.push(Geometry::Leader(leader));
        }
        feedback.markers.push(self.tip.point);
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        (self.step == Step::Note).then_some(self.tip.point)
    }
}

/// A preset leader tool: LEADER preset= by hand.
pub(crate) fn make_preset_leader_tool(
    cx: &ToolContext<'_>,
    start: &ToolStart,
    preset: LeaderPreset,
) -> Box<dyn InteractiveTool> {
    Box::new(PresetLeaderTool::new(cx, start, preset))
}
