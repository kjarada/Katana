//! Field to finish in ONE step: a survey import that also codes what it drew
//! and strings it (include/katana/cad/survey_finish.hpp,
//! src/katana_cad/customisation/survey_finish.cpp).
//!
//! Importing a coded field file is three steps - import, Apply Survey Codes,
//! Process Linework - which as separate commands would each be an undo step,
//! the second replacing the selection and the third reachable from one dialog
//! only. `with_survey_finish` makes them one command: it OWNS the command that
//! draws the points, runs it, and then
//!
//! ```text
//! 1. codes the points that command created (apply_survey_codes);
//! 2. draws the strings the file NUMBERED itself (draw_survey_features, only
//!    the features that have a name, name a point of this import and that a
//!    rule makes a line);
//! 3. strings the other created points by their codes (process_linework, by
//!    the same rule).
//! ```
//!
//! The decisions the rest of this module builds on:
//!
//! ```text
//! 1. EACH STEP IS PLANNED WHEN IT RUNS, as the styling of linework's lines
//!    always was: apply_survey_codes and process_linework read the drawing,
//!    and the points are not in it until the import has run;
//!    draw_survey_features fixes the layers it creates when it is planned,
//!    and creating a layer that exists is refused, so it must be planned
//!    after the coding step has made its layers. Undo and redo replay what
//!    was planned, so ids, layers and styles come back exactly.
//!
//! 2. IT ACTS ON WHAT THE IMPORT CREATED AND NOTHING ELSE. An empty id list
//!    means "every entity" to apply_survey_codes and "every point" to
//!    process_linework, so neither is ever called with one: an import that
//!    drew nothing codes nothing. A string of the file none of whose points
//!    this import created is not drawn (a longer copy of a file, topped up,
//!    does not draw the first copy's strings again). It never deletes a point.
//!
//! 3. ONLY A NAMED FEATURE IS A STRING OF THE FILE'S OWN. A field file that
//!    keeps a string number beside the code gives each (code, number) a
//!    feature named by the number: the file has said which points are one
//!    line, and that is drawn as it stands. A feature with NO name says only
//!    what its points' codes already say - and, from a reader whose format
//!    has no string numbers, in pieces: one feature per run of consecutive
//!    shots, so a kerb shot in sections across a road is many runs of one
//!    point and none of them is the kerb. Such a feature is not drawn; its
//!    points are strung by their codes, as Process Linework strings them.
//!
//! 4. A POINT IS IN AT MOST ONE AUTOMATIC LINE OF ITS OWN STRING. A point a
//!    named feature OF ITS OWN CODE holds is that string's - the test
//!    name_survey_strings names it by - and the feature has answered for it,
//!    with a line or with the reason there is none, so it is not looked at
//!    again by its code. A named feature of ANOTHER code (a line keyed
//!    between two kerb points and coded as a boundary) is drawn or not on its
//!    own and leaves its ends in their own code's string.
//!
//! 5. A LINE RUNS THROUGH ITS POINTS WHERE THEY STAND. A vertex is taken from
//!    the point's entity when this import, or the job being re-adjusted, has
//!    one in the drawing - so a point the person moved, and a re-adjustment
//!    keeps where they put it, is still on its line. A point with no such
//!    entity is, for a string of the file's own, where the file puts it: the
//!    drawing's own control, a point a policy skipped, and one the person
//!    deleted - deleting a mark does not take a vertex out of a kerb the file
//!    strung, and the report counts the vertices that are at such a point. A
//!    string strung by CODE has only the entities to read, and passes a
//!    deleted point by.
//!
//! 6. A STEP THAT HAS NOTHING TO DO SAYS WHY AND IS NOT AN ERROR. No survey
//!    codes loaded, no code in the file, no rule for any code: no layer,
//!    style or line is made and no point is moved, and the report gives the
//!    reason (SurveyFinishSkip). What IS an error - a rule that cannot become
//!    a layer or a style, control codes spelled ambiguously - fails the whole
//!    import, points included, with that error: an import that asked for
//!    codes and silently got none would look like one whose codes matched
//!    nothing.
//! ```
//!
//! How a step reaches the drawing in Rust: the command runs inside the
//! Document's execute, undo or redo, when the Document's state (its survey
//! map and library) is lent to the StateLink the command captured
//! (document.rs). Each step is PLANNED inside StateLink::with, over the model
//! the step is handed, and RUN after the `with` has returned - the linework
//! step's styling command reaches the state itself when it runs, and
//! StateLink::with does not nest.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use katana_commands::{Command, CommandContext, CommandPtr, delete_entities};
use katana_core::error::make_error;
use katana_core::text::trimmed;
use katana_core::{Error, ErrorCode, Status};
use katana_entity::entity::{INVALID_ENTITY_ID, heights_of, property_to_string};
use katana_entity::{Entity, EntityId, EnumName, Geometry, Model, validate};
use katana_survey::data_model::{SurveyFeature, SurveyPoint, SurveyProject};

use crate::document::{Document, StateLink};
use crate::linework::{
    LineworkCodes, LineworkOptions, LineworkOrder, LineworkReport, SurveyFeatureOptions,
    UnplacedFeature, UnplacedFeatureReason, UnplacedReason, draw_survey_features_to,
    parse_field_code, process_linework_to,
};
use crate::survey_coding::{
    CodingSource, SURVEY_STRING_PROPERTY, SurveyCodingOptions, SurveyCodingReport,
    apply_survey_codes_to, survey_code_of,
};
use crate::survey_import::SurveyImportOptions;

/// A lock that a panic elsewhere does not poison for good: the reports are
/// plain values, whole after any write.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Clone, Debug, Default)]
pub struct SurveyFinishOptions {
    /// Code the points the import creates. Both options are OFF by default,
    /// so a caller that says nothing gets the import it always got. The front
    /// ends turn them on from the customisation.
    pub codes: bool,
    /// String them: the file's features, then the rest by code.
    pub linework: bool,
    /// How points and lines are coded: `colour_of`, `create_layers`,
    /// `create_styles`, `set_attributes`. `property` and `ids` are filled when
    /// the step runs (the import's code property; what the import created).
    pub coding: SurveyCodingOptions,
    /// The control codes a point's code may carry, and how a string is
    /// ordered (LineworkOptions::codes and ::order).
    pub controls: LineworkCodes,
    pub order: LineworkOrder,
}

impl SurveyFinishOptions {
    pub fn any(&self) -> bool {
        self.codes || self.linework
    }
}

/// Why a step did not run. `None`: it ran.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SurveyFinishSkip {
    #[default]
    None,
    /// Its option is off.
    NotAsked,
    /// The import drew no point, so there was nothing to act on.
    NoPoints,
    /// No survey codes are loaded (the drawing's map is empty).
    NoSurveyCodes,
    /// None of the points, and no feature, carries a code.
    NoCodesInFile,
    /// No rule answers any of those codes.
    NoRuleMatches,
}

/// A word for a reply line: the C++ `toString(SurveyFinishSkip)`.
impl EnumName for SurveyFinishSkip {
    fn name(self) -> &'static str {
        match self {
            Self::None => "ran",
            Self::NotAsked => "not-asked",
            Self::NoPoints => "no-points",
            Self::NoSurveyCodes => "no-survey-codes",
            Self::NoCodesInFile => "no-codes-in-file",
            Self::NoRuleMatches => "no-rule-matches",
        }
    }
}

/// What the finish did, filled when the command runs (empty before, and left
/// as the run filled it by an undo).
#[derive(Clone, Debug)]
pub struct SurveyFinishReport {
    /// Point entities the wrapped command created: an import's count of
    /// points, whatever was then drawn through them.
    pub points: usize,

    // ---- codes ----
    pub why_not_coded: SurveyFinishSkip,
    /// What apply_survey_codes did to those points. When the step did not run
    /// for want of a code or a rule, only what was SEEN is here - `property`,
    /// `coded`, `fallback_only` and the two lists of codes with no rule - and
    /// nothing of what a run would have done.
    pub coding: SurveyCodingReport,

    // ---- linework ----
    pub why_not_strung: SurveyFinishSkip,
    /// The file's own strings: one LineworkString per NAMED feature drawn, and
    /// the named features not drawn with why (draw_survey_features under
    /// only_ruled_lines; UnplacedFeature::index is the feature's place in the
    /// `strings` handed to with_survey_finish). A feature with no name, and
    /// one that names no point of this import, is in neither: nothing was
    /// asked of it.
    pub features: LineworkReport,
    pub unplaced_features: Vec<UnplacedFeature>,
    /// Points left to those strings: each is held by a named feature of its
    /// own code, which has answered for it - a line, or the reason there is
    /// none - so it is not strung again by its code.
    pub points_in_features: usize,
    /// The rest, strung by code (process_linework under only_ruled_lines):
    /// its lines, and each point not in one with why.
    pub strung: LineworkReport,
    /// Every line CREATED, features first, in creation order.
    pub lines: Vec<EntityId>,
    /// A re-adjusted job only (SurveyFinishEarlier), of the lines its earlier
    /// run drew. Redrawn: strung again by this run, and kept as the SAME
    /// entities - with their ids, layers and styles, and whatever follows them -
    /// at this run's vertices (a line that already ran there is among them,
    /// untouched), in the order strung.
    pub earlier_lines_redrawn: Vec<EntityId>,
    /// No longer strung by this run, and deleted.
    pub earlier_lines_removed: Vec<EntityId>,
    /// How many were left exactly as that run drew them, because the linework
    /// step did not run.
    pub earlier_lines_kept: usize,
    /// ... and how many vertices of the lines of the file's own strings are
    /// at a point the person deleted (SurveyFinishEarlier::deleted_by_hand):
    /// there the line runs through where this run puts the point.
    pub vertices_at_deleted_points: usize,
}

impl Default for SurveyFinishReport {
    fn default() -> Self {
        Self {
            points: 0,
            why_not_coded: SurveyFinishSkip::NotAsked,
            coding: SurveyCodingReport::default(),
            why_not_strung: SurveyFinishSkip::NotAsked,
            features: LineworkReport::default(),
            unplaced_features: Vec::new(),
            points_in_features: 0,
            strung: LineworkReport::default(),
            lines: Vec::new(),
            earlier_lines_redrawn: Vec::new(),
            earlier_lines_removed: Vec::new(),
            earlier_lines_kept: 0,
            vertices_at_deleted_points: 0,
        }
    }
}

impl SurveyFinishReport {
    pub fn codes_ran(&self) -> bool {
        self.why_not_coded == SurveyFinishSkip::None
    }

    pub fn linework_ran(&self) -> bool {
        self.why_not_strung == SurveyFinishSkip::None
    }

    /// Codes the points carry that no rule more specific than "*" answers: the
    /// unmatched and the fallback-only together, in name order.
    pub fn codes_with_no_rule(&self) -> Vec<String> {
        let mut codes = BTreeSet::new();
        add_distinct(&mut codes, &self.coding.unmatched_codes);
        add_distinct(&mut codes, &self.coding.fallback_only_codes);
        codes.into_iter().collect()
    }

    /// Layers created by all three steps, distinct, in name order.
    pub fn layers_created(&self) -> Vec<String> {
        let mut layers = BTreeSet::new();
        add_distinct(&mut layers, &self.coding.layers_created);
        for step in [&self.features, &self.strung] {
            add_distinct(&mut layers, &step.layers_created);
            if let Some(styling) = step.styling_report() {
                add_distinct(&mut layers, &styling.layers_created);
            }
        }
        layers.into_iter().collect()
    }

    /// Styles created by all three steps, distinct, in name order.
    pub fn styles_created(&self) -> Vec<String> {
        let mut styles = BTreeSet::new();
        add_distinct(&mut styles, &self.coding.styles_created);
        for step in [&self.features, &self.strung] {
            if let Some(styling) = step.styling_report() {
                add_distinct(&mut styles, &styling.styles_created);
            }
        }
        styles.into_iter().collect()
    }

    /// Strings of the file and points in no line: unplaced_features plus
    /// strung.unplaced. Most are no fault - a survey mark is a point - so see
    /// the reasons before calling it a problem (finish_warnings does).
    pub fn unplaced(&self) -> usize {
        self.unplaced_features.len() + self.strung.unplaced.len()
    }
}

/// The C++ `std::shared_ptr<SurveyFinishReport>` the finish fills when it
/// runs, shared with whoever asked for the report.
pub type SharedFinishReport = Arc<Mutex<SurveyFinishReport>>;

/// What a RE-ADJUSTED job hands over beside its command: the points it drew
/// before, which are strung again with the new ones (and never coded again -
/// they were, or the person has changed them since), and the lines its
/// earlier run drew. When the linework step runs, each of those that this run
/// strings again is redrawn in place and the others are deleted; when it does
/// not, they are left alone and counted. Empty for an import.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurveyFinishEarlier {
    pub points: Vec<EntityId>,
    pub lines: Vec<EntityId>,
    /// The ids of the job's points the person deleted from the drawing and
    /// the re-adjustment leaves deleted. Nothing is done about them - a string
    /// of the file's runs through where the run puts each, a string by code
    /// passes it by - but the report counts the vertices that are at one.
    pub deleted_by_hand: Vec<String>,
}

// How many codes a sentence names before it says "and N more": enough to
// recognise the file, few enough to stay one line (survey_points names point
// ids by the same limit).
const NAMED_CODES: usize = 12;

fn named_codes(codes: &[String]) -> String {
    let mut text = String::new();
    for (i, code) in codes.iter().take(NAMED_CODES).enumerate() {
        if i != 0 {
            text.push_str(", ");
        }
        text.push_str(code);
    }
    if codes.len() > NAMED_CODES {
        text.push_str(&format!(" and {} more", codes.len() - NAMED_CODES));
    }
    text
}

fn is_point(entity: Option<&Entity>) -> bool {
    entity.is_some_and(|entity| matches!(entity.geometry, Geometry::Point(_)))
}

// A failure of one step, naming it: the message stays the step's own, which
// says what is wrong, and the context says where in the import it happened.
fn failed(error: &Error, step: &str) -> Error {
    let context = if error.context.is_empty() {
        format!("finishing the survey import: {step}")
    } else {
        format!("finishing the survey import: {step} {}", error.context)
    };
    make_error(error.code, error.message.clone(), context)
}

// The lines a planned step redraws in place, noted, and taken off the job's
// earlier lines still to be accounted for.
fn note_redrawn(
    planned: &LineworkReport,
    report: &mut SurveyFinishReport,
    earlier_lines: &mut Vec<EntityId>,
) {
    for line in &planned.strings {
        if line.redrawn != INVALID_ENTITY_ID {
            report.earlier_lines_redrawn.push(line.redrawn);
            earlier_lines.retain(|&id| id != line.redrawn);
        }
    }
}

struct SurveyFinishCommand {
    link: StateLink,
    points: CommandPtr,
    // What the steps are planned from; emptied once they have run.
    strings: SurveyProject,
    import: SurveyImportOptions,
    options: SurveyFinishOptions,
    report: Option<SharedFinishReport>,
    earlier: SurveyFinishEarlier,
    // The steps that ran, in order - coding, features, linework, the removal
    // of a job's earlier lines - each as it was planned, for undo and redo.
    steps: Vec<CommandPtr>,
}

impl Command for SurveyFinishCommand {
    // The import it is: one step in the history, under the import's name.
    fn name(&self) -> &str {
        self.points.name()
    }

    fn is_destructive(&self) -> bool {
        self.points.is_destructive()
    }

    fn validate(&self, context: &CommandContext<'_>) -> Status {
        // Every step is planned against the Document; run on another model it
        // would code whatever that drawing's entities of the same ids are.
        // Run anywhere but through that Document, the state is not lent.
        if self.link.with(|_| ()).is_err() {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "the survey import was planned for another document",
            ));
        }
        // Refused before anything is drawn rather than after the points are.
        if self.options.linework {
            validate(&self.options.controls)?;
        }
        self.points.validate(context)
    }

    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.points.execute(context)?;
        self.steps.clear();
        let mut report = SurveyFinishReport::default();
        // Taken out for the run, which reads them beside the steps it keeps.
        let mut strings = std::mem::take(&mut self.strings);
        let earlier = std::mem::take(&mut self.earlier);
        if let Err(error) = self.finish(context, &mut strings, &earlier, &mut report) {
            // All or nothing: an import that asked for codes and got none
            // looks like one whose codes matched nothing.
            for step in self.steps.iter_mut().rev() {
                let _ = step.undo(context);
            }
            self.steps.clear();
            let _ = self.points.undo(context);
            self.strings = strings;
            self.earlier = earlier;
            return Err(error);
        }
        if let Some(shared) = &self.report {
            *lock(shared) = report;
        }
        // Redo replays the steps and never plans again, so what they were
        // planned from is let go here: this command lives on in the undo
        // history, and the reduced survey there - every point with its
        // metadata and its source record, every feature - would be dead
        // weight. (The job commands let their raw project go for the same
        // reason.)
        Ok(())
    }

    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        for step in self.steps.iter_mut().rev() {
            step.undo(context)?;
        }
        self.points.undo(context)
    }

    // Replays what was planned when it ran, never plans again: the ids, the
    // layers and the styles come back exactly.
    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.points.redo(context)?;
        for step in &mut self.steps {
            step.redo(context)?;
        }
        Ok(())
    }

    // The points, then the lines.
    fn created_entities(&self) -> Vec<EntityId> {
        let mut created = self.points.created_entities();
        for step in &self.steps {
            created.extend(step.created_entities());
        }
        created
    }
}

impl SurveyFinishCommand {
    fn run(
        steps: &mut Vec<CommandPtr>,
        context: &mut CommandContext<'_>,
        mut command: CommandPtr,
        step: &str,
    ) -> Status {
        let mut status = command.validate(context);
        if status.is_ok() {
            status = command.execute(context);
        }
        if let Err(error) = status {
            return Err(failed(&error, step));
        }
        steps.push(command);
        Ok(())
    }

    // A step that draws lines: run, and the lines it created listed.
    fn run_lines(
        &mut self,
        context: &mut CommandContext<'_>,
        command: CommandPtr,
        step: &str,
        report: &mut SurveyFinishReport,
    ) -> Status {
        Self::run(&mut self.steps, context, command, step)?;
        if let Some(last) = self.steps.last() {
            report.lines.extend(last.created_entities());
        }
        Ok(())
    }

    // The point number the import wrote on `entity`, as the job pairs its
    // points by it; empty when it wrote none.
    fn number_of(&self, entity: &Entity) -> String {
        if self.import.point_number_property.is_empty() {
            return String::new();
        }
        entity
            .properties
            .get(&self.import.point_number_property)
            .map(property_to_string)
            .unwrap_or_default()
    }

    // The code `entity` is of: the first token of its code field, which is
    // how a point's code is read everywhere (parse_field_code).
    fn code_of(&self, entity: &Entity) -> String {
        if self.import.code_property.is_empty() {
            return String::new();
        }
        survey_code_of(entity, &self.import.code_property)
            .map(|text| parse_field_code(text, &self.options.controls).name)
            .unwrap_or_default()
    }

    fn finish(
        &mut self,
        context: &mut CommandContext<'_>,
        strings: &mut SurveyProject,
        earlier: &SurveyFinishEarlier,
        report: &mut SurveyFinishReport,
    ) -> Status {
        // What the import drew, and only that. The lists handed on below are
        // checked for being empty where they are made: to apply_survey_codes
        // an empty list is every entity, to process_linework every point.
        let created: Vec<EntityId> = self
            .points
            .created_entities()
            .into_iter()
            .filter(|&id| is_point(context.model.entities.find(id)))
            .collect();
        report.points = created.len();
        let no_codes = self.link.with(|state| state.survey_map.empty())?;

        if self.options.codes {
            if created.is_empty() {
                report.why_not_coded = SurveyFinishSkip::NoPoints;
            } else if no_codes {
                report.why_not_coded = SurveyFinishSkip::NoSurveyCodes;
            } else {
                self.apply_codes(context, &created, report)?;
            }
        }

        if self.options.linework {
            // A re-adjusted job's earlier points that still stand, then the
            // new ones: all of them are the job's to string.
            let mut subject: Vec<EntityId> = earlier
                .points
                .iter()
                .copied()
                .filter(|&id| is_point(context.model.entities.find(id)))
                .collect();
            subject.extend_from_slice(&created);
            // ... and the lines its earlier run drew that are still there.
            let mut standing: Vec<EntityId> = earlier
                .lines
                .iter()
                .copied()
                .filter(|&id| context.model.entities.contains(id))
                .collect();
            if subject.is_empty() {
                report.why_not_strung = SurveyFinishSkip::NoPoints;
            } else if no_codes {
                // With no map nothing says which codes are lines, and a line
                // by control code alone is not the customisation at work.
                report.why_not_strung = SurveyFinishSkip::NoSurveyCodes;
            } else {
                self.draw_lines(
                    context,
                    strings,
                    earlier,
                    &subject,
                    standing.clone(),
                    report,
                )?;
            }

            if !standing.is_empty() {
                if report.why_not_strung != SurveyFinishSkip::None {
                    // A step with nothing to do leaves the drawing as it
                    // found it, and the report says those lines are the
                    // earlier run's. (None was redrawn: no line was planned.)
                    report.earlier_lines_kept = standing.len();
                } else {
                    // What this run strings again it has redrawn in place;
                    // what is left is a line of a string it no longer makes.
                    let redrawn: HashSet<EntityId> =
                        report.earlier_lines_redrawn.iter().copied().collect();
                    standing.retain(|id| !redrawn.contains(id));
                    if !standing.is_empty() {
                        Self::run(
                            &mut self.steps,
                            context,
                            delete_entities(standing.clone()),
                            "earlier lines",
                        )?;
                        report.earlier_lines_removed = standing;
                    }
                }
            }
        }
        Ok(())
    }

    fn apply_codes(
        &mut self,
        context: &mut CommandContext<'_>,
        created: &[EntityId],
        report: &mut SurveyFinishReport,
    ) -> Status {
        let mut coding = self.options.coding.clone();
        coding.property.clone_from(&self.import.code_property);
        coding.ids = created.to_vec(); // not empty: finish() checked
        let mut tally = SurveyCodingReport::default();
        let planned = {
            let model: &Model = &*context.model;
            self.link.with(|state| {
                apply_survey_codes_to(
                    CodingSource {
                        model,
                        survey_map: &state.survey_map,
                        library: &state.library,
                    },
                    &coding,
                    Some(&mut tally),
                )
            })?
        };
        let planned = match planned {
            Ok(planned) => planned,
            Err(error) => return Err(failed(&error, "codes")),
        };
        if tally.coded == 0 || tally.matched == 0 {
            // Nothing the customisation knows is in this file, so the plan is
            // dropped whole - also the fallback rule's attributes, which a
            // CODE run would attach - and the points stay exactly as the
            // import drew them. Only what was SEEN is reported: what the plan
            // would have created was not.
            report.why_not_coded = if tally.coded == 0 {
                SurveyFinishSkip::NoCodesInFile
            } else {
                SurveyFinishSkip::NoRuleMatches
            };
            report.coding.property = std::mem::take(&mut tally.property);
            report.coding.coded = tally.coded;
            report.coding.fallback_only = tally.fallback_only;
            report.coding.unmatched_codes = std::mem::take(&mut tally.unmatched_codes);
            report.coding.fallback_only_codes = std::mem::take(&mut tally.fallback_only_codes);
            return Ok(());
        }
        report.why_not_coded = SurveyFinishSkip::None;
        report.coding = tally;
        // None: every point already has what its rule gives.
        match planned {
            Some(planned) => Self::run(&mut self.steps, context, planned, "codes"),
            None => Ok(()),
        }
    }

    // `subject`: the points to string, none of them missing from the drawing
    // and the list not empty. `earlier_lines`: a re-adjusted job's lines that
    // are still there; the ones a line planned here is drawn AS are listed in
    // the report, and the caller deletes the rest.
    fn draw_lines(
        &mut self,
        context: &mut CommandContext<'_>,
        strings: &mut SurveyProject,
        earlier: &SurveyFinishEarlier,
        subject: &[EntityId],
        mut earlier_lines: Vec<EntityId>,
        report: &mut SurveyFinishReport,
    ) -> Status {
        // The points by the number the import wrote on each: which features
        // name them, and where each stands. The first of a number is kept.
        let mut by_number: HashMap<String, EntityId> = HashMap::with_capacity(subject.len());
        for &id in subject {
            let Some(entity) = context.model.entities.find(id) else {
                continue;
            };
            let number = self.number_of(entity);
            if !number.is_empty() {
                by_number.entry(number).or_insert(id);
            }
        }

        // 1. The strings the file numbered itself: the features that have a
        //    name. One with none says only what its points' codes say - from
        //    some readers in pieces, a feature per run of consecutive shots -
        //    and is left to step 2. One that names no point of this import is
        //    another import's string, and is not drawn a second time by this
        //    one; an import that wrote no point number cannot tell, and takes
        //    them all.
        let numbered = !self.import.point_number_property.is_empty();
        let mut ours = vec![false; strings.features.len()];
        // Point id -> the code of each such feature that holds it, and those
        // codes on their own (borrowed from the features, which nothing below
        // changes).
        let mut held_by: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut held_codes: HashSet<&str> = HashSet::new();
        let mut any = false;
        let mut codes = false; // something here carries a code
        let mut rules = false; // ... and a rule more specific than "*" answers one
        for (i, feature) in strings.features.iter().enumerate() {
            if trimmed(feature.name.as_bytes()).is_empty() {
                continue;
            }
            if numbered
                && !feature
                    .point_ids
                    .iter()
                    .any(|id| by_number.contains_key(id.as_str()))
            {
                continue;
            }
            ours[i] = true;
            any = true;
            if !feature.code.is_empty() {
                codes = true;
                held_codes.insert(feature.code.as_str());
                for id in &feature.point_ids {
                    held_by
                        .entry(id.as_str())
                        .or_default()
                        .push(feature.code.as_str());
                }
            }
        }
        if any {
            // A line runs through its points where they STAND: a point the
            // person has moved, and a re-adjustment keeps where they put it,
            // is still on its line. A point with no entity among these - the
            // drawing's control, one a policy skipped, one deleted by hand -
            // is where the file puts it.
            for point in strings.points.iter_mut() {
                let Some(&id) = by_number.get(point.id.as_str()) else {
                    continue;
                };
                let Some(entity) = context.model.entities.find(id) else {
                    continue;
                };
                let Geometry::Point(at) = &entity.geometry else {
                    continue;
                };
                point.easting = at.position.x;
                point.northing = at.position.y;
                point.elevation = heights_of(&entity.properties, 1).first().copied().flatten();
            }

            // Planned now: the coding step has run, so the layers it made are
            // in the model and are not created twice.
            let options = SurveyFeatureOptions {
                import: self.import.clone(),
                coding: self.options.coding.clone(),
                only_ruled_lines: true,
                consider: ours,
                earlier_lines: earlier_lines.clone(),
            };
            let planned = {
                let model: &Model = &*context.model;
                let project: &SurveyProject = &*strings;
                let step_link = self.link.clone();
                self.link.with(|state| {
                    draw_survey_features_to(
                        CodingSource {
                            model,
                            survey_map: &state.survey_map,
                            library: &state.library,
                        },
                        step_link,
                        project,
                        &options,
                    )
                })?
            };
            let planned = match planned {
                Ok(planned) => planned,
                Err(error) => return Err(failed(&error, "features")),
            };
            rules = !planned.report.strings.is_empty()
                || planned
                    .unplaced
                    .iter()
                    .any(|feature| feature.reason != UnplacedFeatureReason::NoRule);
            note_redrawn(&planned.report, report, &mut earlier_lines);
            // A point the person deleted, and the re-adjustment keeps deleted,
            // has no entity to stand anywhere: the file's string still runs
            // through where the run puts it, and that is said, because a
            // string strung by code simply passes such a point by.
            if !earlier.deleted_by_hand.is_empty() {
                let deleted: HashSet<&str> =
                    earlier.deleted_by_hand.iter().map(String::as_str).collect();
                for line in &planned.report.strings {
                    report.vertices_at_deleted_points += line
                        .point_numbers
                        .iter()
                        .filter(|id| deleted.contains(id.as_str()))
                        .count();
                }
            }
            let command = planned.command;
            report.features = planned.report;
            report.unplaced_features = planned.unplaced;
            if let Some(command) = command {
                self.run_lines(context, command, "features", report)?;
            }
        }

        // 2. The other points, by their codes - planned after the features
        //    ran, for the same reason. A point a numbered string OF ITS OWN
        //    CODE holds is that string's, and step 1 has answered for it with
        //    a line or with the reason there is none: strung again by its
        //    code it would be in two lines of one string, or be refused twice
        //    for one reason. A point only a string of ANOTHER code holds is
        //    still a point of its own code's string.
        let mut rest: Vec<EntityId> = Vec::new();
        for &id in subject {
            let mut held = false;
            if any && let Some(entity) = context.model.entities.find(id) {
                let number = self.number_of(entity);
                let code = self.code_of(entity);
                held = if number.is_empty() {
                    // No number to find it by, so it cannot be told from the
                    // points the file strung under its code: it is left to
                    // them when the file has a numbered string of that code.
                    held_codes.contains(code.as_str())
                } else {
                    held_by
                        .get(number.as_str())
                        .is_some_and(|holders| holders.contains(&code.as_str()))
                };
            }
            if held {
                report.points_in_features += 1;
            } else {
                rest.push(id);
            }
        }
        if !rest.is_empty() {
            // Never an empty list: it means every point in the drawing.
            let linework = LineworkOptions {
                property: self.import.code_property.clone(),
                point_number_property: self.import.point_number_property.clone(),
                order: self.options.order,
                ids: rest,
                keep_points: true, // an import never deletes a point
                codes: self.options.controls.clone(),
                coding: self.options.coding.clone(),
                only_ruled_lines: true,
                earlier_lines: earlier_lines.clone(), // those step 1 did not redraw
                ..LineworkOptions::default()
            };
            let planned = {
                let model: &Model = &*context.model;
                let step_link = self.link.clone();
                self.link.with(|state| {
                    process_linework_to(
                        CodingSource {
                            model,
                            survey_map: &state.survey_map,
                            library: &state.library,
                        },
                        step_link,
                        &linework,
                    )
                })?
            };
            let planned = match planned {
                Ok(planned) => planned,
                Err(error) => return Err(failed(&error, "linework")),
            };
            let mut no_code = 0_usize;
            let mut no_rule = 0_usize;
            for point in &planned.report.unplaced {
                no_code += usize::from(point.reason == UnplacedReason::NoCode);
                no_rule += usize::from(point.reason == UnplacedReason::NoRule);
            }
            codes = codes || planned.report.considered > no_code;
            rules = rules || planned.report.considered > no_code + no_rule;
            note_redrawn(&planned.report, report, &mut earlier_lines);
            let command = planned.command;
            report.strung = planned.report;
            if let Some(command) = command {
                self.run_lines(context, command, "linework", report)?;
            }
        }

        // Under only_ruled_lines nothing is a line without a rule, so "no
        // code" and "no rule" both mean nothing was drawn and nothing changed.
        report.why_not_strung = if !codes {
            SurveyFinishSkip::NoCodesInFile
        } else if !rules {
            SurveyFinishSkip::NoRuleMatches
        } else {
            SurveyFinishSkip::None
        };
        Ok(())
    }
}

fn add_distinct(into: &mut BTreeSet<String>, names: &[String]) {
    into.extend(names.iter().cloned());
}

fn because(skip: SurveyFinishSkip) -> &'static str {
    match skip {
        SurveyFinishSkip::None | SurveyFinishSkip::NotAsked => "it was not asked for",
        SurveyFinishSkip::NoPoints => "there are no points to string",
        SurveyFinishSkip::NoSurveyCodes => "no survey codes are loaded",
        SurveyFinishSkip::NoCodesInFile => "none of the points carries a code",
        SurveyFinishSkip::NoRuleMatches => "no rule matches any of their codes",
    }
}

// How many were left out for each reason, keyed by the reason's place in its
// enumeration so the reasons are said in the order they are declared:
// "no rule for its code: 1; its code is a point code: 2".
type ReasonCounts = BTreeMap<usize, (&'static str, usize)>;

fn count_reason(counts: &mut ReasonCounts, place: usize, name: &'static str) {
    counts.entry(place).or_insert((name, 0)).1 += 1;
}

fn by_reason(counts: &ReasonCounts) -> String {
    let mut text = String::new();
    for (name, count) in counts.values() {
        if !text.is_empty() {
            text.push_str("; ");
        }
        text.push_str(&format!("{name}: {count}"));
    }
    text
}

// One sentence of the report, and whether it is something a person may have
// to act on rather than an account of what was done (finish_warnings).
struct Sentence {
    text: String,
    warning: bool,
}

fn sentences_of(report: &SurveyFinishReport) -> Vec<Sentence> {
    let mut sentences = Vec::new();
    let mut say = |text: String, warning: bool| sentences.push(Sentence { text, warning });

    match report.why_not_coded {
        SurveyFinishSkip::None => {
            say(
                format!(
                    "Survey codes were applied to the {} point(s) drawn: {} carry a code and {} \
                     of those have a rule; {} layer(s) and {} style(s) were created.",
                    report.points,
                    report.coding.coded,
                    report.coding.matched,
                    report.coding.layers_created.len(),
                    report.coding.styles_created.len()
                ),
                false,
            );
            let unknown = report.codes_with_no_rule();
            if !unknown.is_empty() {
                say(
                    format!("No rule for the code(s): {}.", named_codes(&unknown)),
                    true,
                );
            }
        }
        SurveyFinishSkip::NoPoints => {
            // Also every re-adjustment that draws no new point: the points it
            // moved keep the codes they have. Nothing to act on.
            say(
                String::from("No new points were drawn, so no survey codes were applied."),
                false,
            );
        }
        SurveyFinishSkip::NotAsked => {}
        skip => {
            let mut text = format!("Survey codes were not applied: {}", because(skip));
            let unknown = report.codes_with_no_rule();
            if !unknown.is_empty() {
                text.push_str(&format!(" ({})", named_codes(&unknown)));
            }
            text.push('.');
            say(text, true);
        }
    }

    if report.why_not_strung == SurveyFinishSkip::None {
        let mut text = format!("Linework: {} line(s) drawn", report.lines.len());
        if !report.earlier_lines_redrawn.is_empty() {
            text.push_str(&format!(
                " and {} of the job's line(s) redrawn where this adjustment puts them",
                report.earlier_lines_redrawn.len()
            ));
        }
        text.push('.');
        say(text, false);
        if !report.earlier_lines_removed.is_empty() {
            say(
                format!(
                    "{} of the job's line(s) are no longer strung by this adjustment and were \
                     deleted, with anything attached to them.",
                    report.earlier_lines_removed.len()
                ),
                true,
            );
        }
        if report.vertices_at_deleted_points != 0 {
            say(
                format!(
                    "{} vertex(es) of the job's lines are at a point deleted from the drawing by \
                     hand: a string the file numbered still runs through where this adjustment \
                     puts it.",
                    report.vertices_at_deleted_points
                ),
                true,
            );
        }
        if !report.unplaced_features.is_empty() {
            let mut reasons = ReasonCounts::new();
            let mut fault = false;
            for feature in &report.unplaced_features {
                count_reason(&mut reasons, feature.reason as usize, feature.reason.name());
                // A string of a point code is in no line because it is none.
                fault = fault || feature.reason != UnplacedFeatureReason::PointCode;
            }
            say(
                format!(
                    "{} string(s) of the file are in no line ({}).",
                    report.unplaced_features.len(),
                    by_reason(&reasons)
                ),
                fault,
            );
        }
        if !report.strung.unplaced.is_empty() {
            let mut reasons = ReasonCounts::new();
            let mut fault = false;
            for point in &report.strung.unplaced {
                count_reason(&mut reasons, point.reason as usize, point.reason.name());
                // Likewise a point of a point code, and one with no code.
                fault = fault
                    || (point.reason != UnplacedReason::PointCode
                        && point.reason != UnplacedReason::NoCode);
            }
            say(
                format!(
                    "{} point(s) are in no line ({}).",
                    report.strung.unplaced.len(),
                    by_reason(&reasons)
                ),
                fault,
            );
        }
    } else if report.why_not_strung != SurveyFinishSkip::NotAsked {
        let mut text = format!("No linework was drawn: {}.", because(report.why_not_strung));
        if report.earlier_lines_kept != 0 {
            text.push_str(&format!(
                " The job's {} line(s) are as the earlier adjustment drew them.",
                report.earlier_lines_kept
            ));
        }
        // No points to string is nothing to act on - unless lines were left
        // standing that this adjustment no longer accounts for.
        let warning =
            report.why_not_strung != SurveyFinishSkip::NoPoints || report.earlier_lines_kept != 0;
        say(text, warning);
    }
    sentences
}

/// The report as sentences, for a log: per step that was asked for, what ran
/// with its counts or why it did not, and then what was left over - the codes
/// with no rule, the strings and points in no line by reason, a job's lines
/// deleted or left as an earlier adjustment drew them.
pub fn describe(report: &SurveyFinishReport) -> Vec<String> {
    sentences_of(report)
        .into_iter()
        .map(|sentence| sentence.text)
        .collect()
}

/// Those of `describe`'s sentences a person may have to act on, and none of
/// the ones that only say what was done:
///
/// ```text
/// * a step that was asked for and had nothing to go on (no survey codes
///   loaded, no code in the file, no rule for any code);
/// * codes no rule answers;
/// * a string or a point left out of every line for a reason other than
///   being what it is (a point code, or a point with no code);
/// * a job's lines deleted, left as an earlier adjustment drew them, or run
///   through a point that was deleted by hand.
/// ```
///
/// Empty for a finish that did all it was asked. This is what a survey job
/// adds to its report's warnings; the rest is in its finish report.
pub fn finish_warnings(report: &SurveyFinishReport) -> Vec<String> {
    sentences_of(report)
        .into_iter()
        .filter(|sentence| sentence.warning)
        .map(|sentence| sentence.text)
        .collect()
}

/// Gives each of `points` that a NAMED feature of its own code holds that
/// name as metadata SURVEY_STRING_PROPERTY, which the import then writes as a
/// property like any other metadata key. For the field files that keep a
/// string number apart from the code: the builder they share leaves the code
/// alone on the point and the number only in the feature's name. The FIRST
/// such feature, in the project's order, names the point; a feature of
/// another code does not (a point shot again under a second code is still of
/// its first), and a point that already has the key keeps it.
///
/// Call it on the project before planning its import, and only for an import
/// that is to be finished INTO A DRAWING THAT HAS SURVEY CODES, as a survey
/// job does: without a finish, or with no codes loaded, the points are then
/// drawn exactly as they always were, to the last property. (With codes
/// loaded a point of a numbered string carries its number whether or not a
/// rule then answers it - the one thing a finish with nothing to do leaves
/// behind.)
pub fn name_survey_strings(points: &mut [SurveyPoint], features: &[SurveyFeature]) {
    if features.is_empty() {
        return;
    }
    // By id, the first of an id kept; nothing below adds or removes a point.
    let mut by_id: HashMap<String, usize> = HashMap::with_capacity(points.len());
    for (index, point) in points.iter().enumerate() {
        by_id.entry(point.id.clone()).or_insert(index);
    }
    for feature in features {
        // Blanks alone are no name, as the finish reads a feature and as the
        // number is read back off the point (survey_string_of).
        if trimmed(feature.name.as_bytes()).is_empty() || feature.code.is_empty() {
            continue;
        }
        for id in &feature.point_ids {
            // Not drawn: an unpositioned point, or the drawing's control.
            let Some(&index) = by_id.get(id.as_str()) else {
                continue;
            };
            let point = &mut points[index];
            // Of its own code, read as the point is coded: by its first token.
            if parse_field_code(&point.code, &LineworkCodes::default()).name != feature.code {
                continue;
            }
            point
                .metadata
                .entry(String::from(SURVEY_STRING_PROPERTY))
                .or_insert_with(|| feature.name.clone());
        }
    }
}

/// ONE command that owns `points`, runs it, and then codes and strings what it
/// created, as `options` say. `points` is the unexecuted command of
/// import_survey_points or import_survey_project (or a job's own); `import`
/// the options it was planned with.
///
/// `strings` is what the file strung: the project's points, unpositioned
/// points and features (all the points, also one that was not drawn - a line
/// runs through where the file puts it). Pass an empty project for a file
/// with no features - a delimited point list - and its points are strung by
/// their codes alone. It is let go once the command has run: redo replays,
/// and the undo history holds no copy of the survey.
///
/// Returns `points` itself, unwrapped, when neither option is on - the caller
/// then has exactly the command it had - and None for a None `points`
/// (nothing to import is still nothing to do). Otherwise the composite, named
/// as `points` is: one undo takes back the import with its codes and its
/// lines, and its created_entities() are the points and then the lines.
///
/// It must be executed on THIS document, as process_linework's command. Its
/// validate() refuses another, and control codes spelled ambiguously
/// (entity's validate), before anything is drawn.
///
/// `report` is filled when it runs; for a None or unwrapped `points` it is
/// filled at once, with NoPoints or NotAsked. The C++ defaults are no report
/// and an empty SurveyFinishEarlier.
pub fn with_survey_finish(
    document: &Document,
    points: Option<CommandPtr>,
    strings: SurveyProject,
    import: &SurveyImportOptions,
    options: SurveyFinishOptions,
    report: Option<SharedFinishReport>,
    earlier: SurveyFinishEarlier,
) -> Option<CommandPtr> {
    with_survey_finish_to(
        document.state_link(),
        points,
        strings,
        import,
        options,
        report,
        earlier,
    )
}

/// with_survey_finish for the link to the document's state, which is all the
/// finish keeps of its Document: what a survey job command calls while it
/// plans inside its own step.
pub(crate) fn with_survey_finish_to(
    link: StateLink,
    points: Option<CommandPtr>,
    strings: SurveyProject,
    import: &SurveyImportOptions,
    options: SurveyFinishOptions,
    report: Option<SharedFinishReport>,
    earlier: SurveyFinishEarlier,
) -> Option<CommandPtr> {
    let points = match points {
        Some(points) if options.any() => points,
        points => {
            if let Some(shared) = &report {
                let mut report = lock(shared);
                *report = SurveyFinishReport::default();
                if points.is_none() {
                    let why = |asked: bool| {
                        if asked {
                            SurveyFinishSkip::NoPoints
                        } else {
                            SurveyFinishSkip::NotAsked
                        }
                    };
                    report.why_not_coded = why(options.codes);
                    report.why_not_strung = why(options.linework);
                }
            }
            return points;
        }
    };
    Some(Box::new(SurveyFinishCommand {
        link,
        points,
        strings,
        import: import.clone(),
        options,
        report,
        earlier,
        steps: Vec::new(),
    }))
}
