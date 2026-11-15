//! What a Document knows of the customisation it is drawn with, beyond the
//! definitions and the survey code rules themselves
//! (include/katana/cad/customisation_state.hpp).
//!
//! The library and the survey map were all a Document held, and each front end
//! kept the rest - which files were loaded, which names a project recorded
//! that were not - in lists of its own. Two lists in two programs had already
//! come to differ (after OPEN then NEW, one still named the old project's
//! files), and a CUSTOMISE verb in the shared interpreter can reach neither.
//! So the session's whole customisation is here, on the Document, beside the
//! library and the map it describes.
//!
//! Like them it is SESSION data: not undoable, not saved in the project, kept
//! across a new or an opened drawing (document.rs).

use katana_core::error::make_error;
use katana_core::{Error, ErrorCode};
use katana_entity::customisation::{customisation_to_json, validate_customisation_name};
use katana_entity::{
    ColourTable, Customisation, CustomisationAutomation, CustomisationBase,
    CustomisationWriteOptions, EnumName, LineworkCodes, validate,
};

use crate::customisation_record::CustomisationSource;

/// Where the customisation in a session came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CustomisationOrigin {
    /// Nothing has been installed: an empty Document.
    #[default]
    None,
    /// The customisation that is part of the program.
    BuiltIn,
    /// The user's own kept customisation, read at start-up.
    Kept,
    /// A customisation loaded in this session.
    Loaded,
    /// Changed in this session: an editor, or one of the setters.
    Edited,
}

/// "none", "builtIn", "kept", "loaded", "edited": the words a reply names an
/// origin by.
impl EnumName for CustomisationOrigin {
    fn name(self) -> &'static str {
        match self {
            CustomisationOrigin::None => "none",
            CustomisationOrigin::BuiltIn => "builtIn",
            CustomisationOrigin::Kept => "kept",
            CustomisationOrigin::Loaded => "loaded",
            CustomisationOrigin::Edited => "edited",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomisationState {
    /// Empty until a customisation is installed. A session whose library or
    /// map was set directly keeps the name it had.
    pub name: String,
    pub origin: CustomisationOrigin,
    pub description: String,
    /// The author's notice of the customisation itself, a line an entry. Each
    /// source keeps its own beside it.
    pub notice: Vec<String>,
    /// What went into the session, in load order: each source's name, whether
    /// it brought definitions, whether it brought rules, and its notice.
    pub sources: Vec<CustomisationSource>,
    /// The customisation an edited or kept copy started from, when it says.
    pub based_on: Option<CustomisationBase>,
    /// The colours the customisation's own names mean. A name is resolved
    /// against this and then the standard names (colour_lookup).
    pub colours: ColourTable,
    /// How the linework control codes are spelled: the defaults until a
    /// customisation, or a setter, says otherwise.
    pub linework: LineworkCodes,
    /// What is applied to survey data without being asked for.
    pub automation: CustomisationAutomation,
    /// The session is what the next start would give: what start-up
    /// installed, or what was last kept. False from the first change after
    /// either.
    pub kept: bool,
    /// The names the open project recorded that this session did not have
    /// when it was opened, less those loaded since (customisation_record.rs).
    /// A save keeps them in the record; a new drawing has none.
    pub missing_at_open: Vec<String>,
    /// The name the host's built-in customisation declares, whether or not it
    /// is what is installed; empty when the host has none. It is the built-in
    /// of the run: what RESET gives and CUSTOMISE BUILTIN marks as the
    /// default.
    pub built_in: String,
    /// What went wrong when the session's customisation was STARTED
    /// (start_customisation, customisation_host), a sentence each: a built-in
    /// that did not read, a kept file that did not read and what started in
    /// its place. A front end says them once, at the start, where its errors
    /// go - which a client of katana_mcp never reads, so that an agent whose
    /// kept file was refused was told `origin: builtIn` and no more. Kept here
    /// they are in the report whenever it is asked for (customisation_json,
    /// "start"). They describe the start: nothing after it changes them, and a
    /// session never started with a host has none.
    pub start_problems: Vec<String>,
    /// The kept customisation the session started with was made from another
    /// built-in than the host has, or another edition of it
    /// (CustomisationStart::kept_from_another_built_in). Of the start, as
    /// above.
    pub kept_from_another_built_in: bool,
}

/// Everything about `customisation` that would make a session of it unfit to
/// be KEPT or RECORDED, each an InvalidArgument whose message begins with the
/// part it is about:
///
/// ```text
///   its name: ...                          validate_customisation_name refuses it
///   the name of one of its sources: ...    the same, of a source it lists
///   the source of its definition "X": ...  the same, of a definition's
///                                          LineStyle::source, when it has one
///   its linework codes: ...                entity's validate refuses them
///   its basedOn: ...                       the format's writer refuses it: a
///                                          name as above, or a digest that is
///                                          not one
/// ```
///
/// Empty: it can be installed. A project records the name, each source's name
/// and each definition's source, a line each, and the project store refuses a
/// name it could not read back - after which EVERY save of the session fails;
/// the kept file is written by the format's writer, which refuses the rest. A
/// customisation read from a file has passed all of this already (entity's
/// customisation_from_json); one built in code has not.
/// Document::install_customisation refuses on the first of them, and
/// merge_customisation lists them all, for each customisation of a load.
pub fn customisation_faults(customisation: &Customisation) -> Vec<Error> {
    let mut faults = Vec::new();
    let mut fault = |part: &str, refusal: &Error| {
        faults.push(make_error(
            ErrorCode::InvalidArgument,
            format!("{part}: {}", refusal.message),
            refusal.context.clone(),
        ));
    };
    if let Err(refusal) = validate_customisation_name(&customisation.name) {
        fault("its name", &refusal);
    }
    for source in &customisation.sources {
        if let Err(refusal) = validate_customisation_name(&source.name) {
            fault("the name of one of its sources", &refusal);
        }
    }
    // A definition's source reaches a project's record exactly as the
    // customisation's own name does (customisation_record); one made in a
    // session has none, and that is no fault.
    customisation.library.for_each(|definition| {
        if definition.source.is_empty() {
            return;
        }
        if let Err(refusal) = validate_customisation_name(&definition.source) {
            fault(
                &format!("the source of its definition \"{}\"", definition.name),
                &refusal,
            );
        }
    });
    if let Some(linework) = &customisation.linework
        && let Err(refusal) = validate(linework)
    {
        fault("its linework codes", &refusal);
    }
    if let Some(based_on) = &customisation.based_on {
        // Judged by the format's own writer, which is what a KEEP hands it to:
        // what a digest looks like is the format's to say, and is said there,
        // once. A customisation of nothing but this basedOn, under a name that
        // is fine, can be refused for nothing else; the refusal already begins
        // "basedOn: ".
        let probe = Customisation {
            name: String::from("basedOn"),
            based_on: Some(based_on.clone()),
            ..Customisation::default()
        };
        if let Err(written) = customisation_to_json(&probe, &CustomisationWriteOptions::default()) {
            faults.push(make_error(
                ErrorCode::InvalidArgument,
                format!("its {}", written.message),
                written.context,
            ));
        }
    }
    faults
}
