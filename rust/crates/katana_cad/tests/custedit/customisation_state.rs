// The customisation's session state on the Document (customisation_state.rs;
// tests/cad/customisation/test_customisation_state.cpp): what a default
// Document has, what installing one sets and reports, what the raw setters and
// the three part-setters do to its origin, and the record a project keeps of
// it - worked out by the open, written by the save.
//
// Every customisation here is built in code or written as text in the test;
// nothing reads a file that is not the test's own.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use katana_cad::customisation_merge::{LoadMode, merge_customisation};
use katana_cad::customisation_state::customisation_faults;
use katana_cad::{
    CustomisationOrigin, CustomisationSource, CustomisationState, Document, DocumentChange,
    ListenerHandle,
};
use katana_core::{ErrorCode, Status};
use katana_entity::customisation::{customisation_from_json, customisation_to_json};
use katana_entity::entity::to_string;
use katana_entity::{
    Color, ColourTable, Customisation, CustomisationAutomation, CustomisationBase,
    CustomisationWriteOptions, LineStyle, LineworkCodes, Model, Stroke, StrokeOp, StyleLibrary,
    SurveyMap, SurveyRule,
};
use katana_geometry::Point2;
use katana_storage::{ProjectMetadata, ProjectStore, capture_model};

/// The parts of every notification, in order.
struct Recorder {
    calls: Arc<Mutex<Vec<u32>>>,
    _handle: ListenerHandle,
}

impl Recorder {
    fn new(document: &Document) -> Self {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&calls);
        let handle = document.add_change_listener(move |change| {
            sink.lock().expect("the recorder").push(change.parts);
        });
        Self {
            calls,
            _handle: handle,
        }
    }

    fn calls(&self) -> Vec<u32> {
        self.calls.lock().expect("the recorder").clone()
    }
}

fn definition(name: &str, source: &str, symbol: bool) -> LineStyle {
    LineStyle {
        name: String::from(name),
        source: String::from(source),
        symbol,
        strokes: vec![Stroke {
            op: StrokeOp::Draw,
            point: Point2::new(1.0, 0.0),
            ..Stroke::default()
        }],
        ..LineStyle::default()
    }
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| String::from(*name)).collect()
}

fn source(name: &str, definitions: bool, rules: bool, notice: &[&str]) -> CustomisationSource {
    CustomisationSource {
        name: String::from(name),
        definitions,
        rules,
        notice: strings(notice),
    }
}

fn automation(codes: bool, linework: bool) -> CustomisationAutomation {
    CustomisationAutomation {
        codes_on_survey_import: codes,
        linework_on_survey_import: linework,
    }
}

/// A customisation that says everything one can: two definitions (one listed
/// as a symbol), one rule, one colour of its own, its own spelling of a
/// control code and one automation switch off.
fn site() -> Customisation {
    let mut made = Customisation {
        name: String::from("Site"),
        description: String::from("Site standards"),
        notice: strings(&["For this site only."]),
        based_on: Some(CustomisationBase {
            name: String::from("NSW"),
            digest: String::from("0123456789abcdef"),
        }),
        linework: Some(LineworkCodes {
            start: String::from("S"),
            ..LineworkCodes::default()
        }),
        automation: Some(automation(false, true)),
        ..Customisation::default()
    };
    assert!(
        made.colours
            .add("sui gas", Color::new(1, 2, 3, 255))
            .is_ok()
    );
    assert!(
        made.library
            .add(definition("SITE Fence", "Site", false))
            .is_ok()
    );
    assert!(
        made.library
            .add(definition("SITE Peg", "Site", true))
            .is_ok()
    );
    let fence = SurveyRule {
        key: String::from("FE*"),
        model: String::from("FENCES"),
        colour: String::from("sui gas"),
        linestyle: String::from("SITE Fence"),
        ..SurveyRule::default()
    };
    assert!(made.map.add(fence).is_ok());
    made
}

/// One rule and nothing else: a customisation of survey codes alone.
fn codes_only(name: &str) -> Customisation {
    let mut made = Customisation {
        name: String::from(name),
        ..Customisation::default()
    };
    let rule = SurveyRule {
        key: String::from("ZZ*"),
        model: String::from("OTHER"),
        ..SurveyRule::default()
    };
    assert!(made.map.add(rule).is_ok());
    made
}

/// A fresh directory for one test's projects, removed when the test ends.
/// Named for the test and the process: the cases of one binary run at once on
/// threads of one process, where the C++'s ran a case at a time, so a folder
/// shared by the whole suite would be removed under another case.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "katana-cad-tests-customisation-state-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        if let Err(error) = std::fs::create_dir_all(&root) {
            panic!("{}: {error}", root.display());
        }
        Self { root }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A project whose record holds `names`, made through the store itself: a
/// Document's save writes the record of ITS session, so it cannot make one
/// that was recorded by a session of another time.
fn project_recording(directory: &Path, names: &[&str]) {
    let metadata = ProjectMetadata {
        customisation: strings(names),
        ..ProjectMetadata::default()
    };
    let mut store = match ProjectStore::create(directory, &metadata) {
        Ok(store) => store,
        Err(error) => panic!("{}", error.describe()),
    };
    if let Err(error) = store.save(&capture_model(&Model::default(), metadata, Vec::new())) {
        panic!("{}", error.describe());
    }
}

const INSTALLED: u32 =
    DocumentChange::STYLE_LIBRARY | DocumentChange::SURVEY_MAP | DocumentChange::CUSTOMISATION;

/// Why `status` is a refusal; that it is one is checked here, so that a call
/// which wrongly succeeded fails the test.
#[track_caller]
fn refusal_of(status: Status) -> ErrorCode {
    status.expect_err("it was not refused").code
}

#[track_caller]
fn must(status: Status) {
    if let Err(error) = status {
        panic!("{}", error.describe());
    }
}

mod customisation_state {
    use super::*;

    // ---- nothing, until something is installed --------------------------------------

    #[test]
    fn a_default_document_has_no_customisation_and_installs_none_by_itself() {
        let document = Document::default();
        let state = document.customisation_state();
        assert!(*state == CustomisationState::default());
        assert!(state.name.is_empty());
        assert_eq!(state.origin, CustomisationOrigin::None);
        assert!(!state.kept);
        assert!(state.sources.is_empty());
        assert!(state.colours.empty());
        assert!(state.missing_at_open.is_empty());
        assert!(state.built_in.is_empty());
        assert!(state.based_on.is_none());
        // The control codes and the switches have defaults until something
        // says otherwise: the seven spellings, and both switches on.
        assert_eq!(state.linework.start, "ST");
        assert_eq!(state.linework.end, "END");
        assert_eq!(state.linework.close, "CL");
        assert_eq!(state.linework.arc_start, "BC");
        assert_eq!(state.linework.arc_end, "EC");
        assert_eq!(state.linework.join, "JPN");
        assert_eq!(state.linework.rectangle, "RECT");
        assert!(state.automation.codes_on_survey_import);
        assert!(state.automation.linework_on_survey_import);
        // And no definitions or rules: a Document never installs a
        // customisation itself, whatever is compiled into the program.
        assert!(document.style_library().empty());
        assert!(document.survey_map().empty());
        assert_eq!(document.customisation_generation(), 0);
    }

    #[test]
    fn an_origin_is_named_by_one_of_five_words() {
        assert_eq!(to_string(CustomisationOrigin::None), "none");
        assert_eq!(to_string(CustomisationOrigin::BuiltIn), "builtIn");
        assert_eq!(to_string(CustomisationOrigin::Kept), "kept");
        assert_eq!(to_string(CustomisationOrigin::Loaded), "loaded");
        assert_eq!(to_string(CustomisationOrigin::Edited), "edited");
    }

    // ---- installing a whole customisation -------------------------------------------

    #[test]
    fn installing_sets_every_part_and_reports_the_three_of_them_in_one_notification() {
        let mut document = Document::default();
        let recorder = Recorder::new(&document);

        must(document.install_customisation(site(), CustomisationOrigin::Loaded, false));

        let calls = recorder.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], INSTALLED);
        assert!(DocumentChange::of(calls[0]).changes_drawing());
        // Each generation once, from 0.
        assert_eq!(document.library_generation(), 1);
        assert_eq!(document.survey_map_generation(), 1);
        assert_eq!(document.customisation_generation(), 1);

        let state = document.customisation_state();
        assert_eq!(state.name, "Site");
        assert_eq!(state.origin, CustomisationOrigin::Loaded);
        assert!(!state.kept);
        assert_eq!(state.description, "Site standards");
        assert_eq!(state.notice, strings(&["For this site only."]));
        let based_on = state.based_on.as_ref().expect("a basedOn");
        assert_eq!(based_on.name, "NSW");
        assert_eq!(based_on.digest, "0123456789abcdef");
        assert_eq!(state.colours.size(), 1);
        assert_eq!(
            state.colours.find("sui gas"),
            Some(Color::new(1, 2, 3, 255))
        );
        assert_eq!(state.linework.start, "S");
        assert_eq!(
            state.linework.end, "END",
            "the rest of what it says, which is the defaults"
        );
        assert!(!state.automation.codes_on_survey_import);
        assert!(state.automation.linework_on_survey_import);
        // It lists no sources, so it is its own one source, bringing both
        // kinds.
        assert_eq!(state.sources, vec![source("Site", true, true, &[])]);
        assert!(state.missing_at_open.is_empty());

        assert_eq!(document.style_library().size(), 2);
        assert!(
            document
                .definition_for("SITE Peg")
                .expect("SITE Peg")
                .symbol
        );
        assert!(
            !document
                .definition_for("SITE Fence")
                .expect("SITE Fence")
                .symbol
        );
        assert_eq!(document.survey_map().size(), 1);
        assert_eq!(
            document.survey_map().lookup("FE01").resolved.model,
            "FENCES"
        );
    }

    #[test]
    fn a_customisation_that_lists_its_sources_is_installed_with_them() {
        // A session kept after a second customisation was merged into it lists
        // both; read back, it must be the session it was.
        let mut merged = site();
        merged.sources = vec![
            source("NSW", true, true, &["The first notice."]),
            source("Site", true, false, &["For this site only."]),
        ];
        let mut document = Document::default();
        must(document.install_customisation(merged.clone(), CustomisationOrigin::Kept, true));
        assert_eq!(document.customisation_state().sources, merged.sources);
        assert_eq!(
            document.customisation_state().origin,
            CustomisationOrigin::Kept
        );
        assert!(document.customisation_state().kept);
    }

    #[test]
    fn installing_is_not_an_edit_of_the_session_or_of_the_drawing() {
        let mut document = Document::default();
        must(document.install_customisation(site(), CustomisationOrigin::BuiltIn, true));
        // The library and the map were set, and the origin is still what was
        // said: the raw setters would have made it Edited.
        assert_eq!(
            document.customisation_state().origin,
            CustomisationOrigin::BuiltIn
        );
        assert!(document.customisation_state().kept);
        // Not undoable and not a change to the drawing.
        assert!(!document.is_modified());
        assert_eq!(document.history().undo_count(), 0);
        assert_eq!(document.model_revision(), 0);
    }

    // This test pinned the opposite when it was written, earlier in this
    // change: that a customisation silent about the control codes and the
    // switches left the SESSION's standing. That is the merge's rule - a load
    // that says nothing changes nothing (MergeCustomisation.LineworkCodesAnd-
    // AutomationAreTakenOnlyFromACustomisationThatSaysThem) - and it was wrong
    // for an install, which takes the place of the whole session: installed
    // `kept`, the session must be the one the next start gives, and that start
    // has no earlier session whose spellings could be left behind. The
    // expectation below is the format's own defaults (entity's LineworkCodes:
    // start "ST"; both switches on).
    #[test]
    fn a_customisation_silent_about_the_control_codes_and_the_switches_installs_the_defaults() {
        let mut document = Document::default();
        must(document.install_customisation(site(), CustomisationOrigin::Loaded, false));
        // The session spells start "S" and has coding off. A customisation
        // that says neither is installed over it: neither stays.
        must(document.install_customisation(
            codes_only("Other"),
            CustomisationOrigin::Loaded,
            false,
        ));
        let state = document.customisation_state();
        assert_eq!(state.name, "Other");
        assert_eq!(state.linework.start, "ST");
        assert!(state.linework == LineworkCodes::default());
        assert!(state.automation.codes_on_survey_import);
        assert!(state.automation.linework_on_survey_import);
        // What it does not hold of the rest is gone: an install takes the
        // place of the session, it is not a merge.
        assert!(document.style_library().empty());
        assert!(state.colours.empty());
        assert!(state.description.is_empty());
        assert!(state.based_on.is_none());
        assert_eq!(state.sources, vec![source("Other", false, true, &[])]);
    }

    #[test]
    fn a_reset_to_a_silent_built_in_leaves_the_session_a_fresh_start_with_it_would_give() {
        // What a reset to the built-in does, by hand. The session's control
        // codes and switches were changed, and the built-in - which says
        // neither - is installed as what the next start would give. That start
        // is a new Document installing the same customisation, so the two
        // sessions are equal whatever the first held before: start "ST" again,
        // both switches on again.
        let built_in = codes_only("Built In");
        let mut edited = Document::default();
        must(edited.set_linework_codes(LineworkCodes {
            start: String::from("BEG"),
            ..LineworkCodes::default()
        }));
        edited.set_automation(automation(false, false));
        must(edited.install_customisation(built_in.clone(), CustomisationOrigin::BuiltIn, true));

        let mut fresh = Document::default();
        must(fresh.install_customisation(built_in, CustomisationOrigin::BuiltIn, true));

        assert!(edited.customisation_state() == fresh.customisation_state());
        assert_eq!(edited.customisation_state().linework.start, "ST");
        assert!(
            edited
                .customisation_state()
                .automation
                .codes_on_survey_import
        );
        assert!(
            edited
                .customisation_state()
                .automation
                .linework_on_survey_import
        );
        assert!(edited.customisation_state().kept);
        assert_eq!(
            edited.customisation_state().origin,
            CustomisationOrigin::BuiltIn
        );
    }

    #[test]
    fn a_load_merged_into_the_session_still_leaves_the_control_codes_it_does_not_say() {
        // The other half of the rule: a LOAD that is silent leaves the
        // session's alone. It does so through the two calls a load makes - the
        // session as a customisation, merged, and the result installed -
        // because the session always says its own, and the merge carries them
        // forward.
        let mut document = Document::default();
        must(document.set_linework_codes(LineworkCodes {
            start: String::from("BEG"),
            ..LineworkCodes::default()
        }));
        document.set_automation(automation(false, true));

        let loaded = vec![codes_only("Other")];
        let merge = merge_customisation(&document.customisation(), &loaded, LoadMode::Merge);
        assert!(merge.ok());
        must(document.install_customisation(merge.merged, CustomisationOrigin::Loaded, false));

        assert_eq!(document.survey_map().size(), 1, "the load did come in");
        assert_eq!(document.customisation_state().linework.start, "BEG");
        assert!(
            !document
                .customisation_state()
                .automation
                .codes_on_survey_import
        );
        assert!(
            document
                .customisation_state()
                .automation
                .linework_on_survey_import
        );
    }

    #[test]
    fn the_session_as_one_customisation_installs_back_as_the_same_session() {
        let mut first = Document::default();
        must(first.install_customisation(site(), CustomisationOrigin::Loaded, false));
        let session = first.customisation();

        // Everything the session has, the control codes and switches said.
        assert_eq!(session.name, "Site");
        assert_eq!(session.description, "Site standards");
        assert_eq!(session.notice, strings(&["For this site only."]));
        assert_eq!(session.sources, vec![source("Site", true, true, &[])]);
        assert_eq!(session.linework.as_ref().expect("linework").start, "S");
        assert!(
            !session
                .automation
                .expect("automation")
                .codes_on_survey_import
        );
        assert_eq!(session.library.all(), first.style_library().all());
        assert_eq!(session.map, *first.survey_map());
        assert_eq!(session.colours, first.customisation_state().colours);

        let mut second = Document::default();
        must(second.install_customisation(session.clone(), CustomisationOrigin::Loaded, false));
        assert!(second.customisation_state() == first.customisation_state());
        assert!(second.customisation() == session);

        // And through the file it would be kept in.
        let written = match customisation_to_json(&session, &CustomisationWriteOptions::default()) {
            Ok(written) => written,
            Err(error) => panic!("{}", error.describe()),
        };
        let read = match customisation_from_json(written.as_bytes()) {
            Ok(read) => read,
            Err(error) => panic!("{}", error.describe()),
        };
        assert!(read == session);
    }

    #[test]
    fn install_refuses_what_could_not_be_kept_or_recorded_and_then_changes_nothing() {
        let mut document = Document::default();
        must(document.install_customisation(site(), CustomisationOrigin::Loaded, false));
        let before = document.customisation_state().clone();
        let recorder = Recorder::new(&document);

        // "none" is where nothing came from.
        assert_eq!(
            refusal_of(document.install_customisation(
                codes_only("Other"),
                CustomisationOrigin::None,
                false
            )),
            ErrorCode::InvalidArgument
        );
        // A name a project's record could not hold: none, a path, two lines.
        for name in ["", "roads/2026", "a\\b", "two\nlines"] {
            assert_eq!(
                refusal_of(document.install_customisation(
                    codes_only(name),
                    CustomisationOrigin::Loaded,
                    false
                )),
                ErrorCode::InvalidArgument,
                "\"{name}\""
            );
        }
        // The same of a source's name.
        let mut bad_source = codes_only("Other");
        bad_source.sources = vec![
            source("fine", false, true, &[]),
            source("not/fine", true, false, &[]),
        ];
        assert_eq!(
            refusal_of(document.install_customisation(
                bad_source,
                CustomisationOrigin::Loaded,
                false
            )),
            ErrorCode::InvalidArgument
        );
        // Control codes that spell two controls alike.
        let mut bad_codes = codes_only("Other");
        bad_codes.linework = Some(LineworkCodes {
            start: String::from("X"),
            close: String::from("x"),
            ..LineworkCodes::default()
        });
        assert_eq!(
            refusal_of(document.install_customisation(
                bad_codes,
                CustomisationOrigin::Loaded,
                false
            )),
            ErrorCode::InvalidArgument
        );
        // A definition whose source is a path. A project records each
        // definition's source beside the customisation's own name, and the
        // store refuses a record holding a path: installed, EVERY save would
        // then fail.
        let mut bad_definition = codes_only("Other");
        assert!(
            bad_definition
                .library
                .add(definition("ROAD Kerb", "lib/roads", false))
                .is_ok()
        );
        let path_as_source =
            document.install_customisation(bad_definition, CustomisationOrigin::Loaded, false);
        let refusal = path_as_source.expect_err("it was not refused");
        assert_eq!(refusal.code, ErrorCode::InvalidArgument);
        assert!(
            refusal.message.contains("\"ROAD Kerb\""),
            "{}",
            refusal.describe()
        );
        // What it says it is based on, with a digest that is not sixteen
        // hexadecimal digits, or a name that is a path: the kept file's writer
        // refuses both, so a KEEP of the session would fail.
        let mut bad_digest = codes_only("Other");
        bad_digest.based_on = Some(CustomisationBase {
            name: String::from("NSW"),
            digest: String::from("0123"),
        });
        assert_eq!(
            refusal_of(document.install_customisation(
                bad_digest,
                CustomisationOrigin::Loaded,
                false
            )),
            ErrorCode::InvalidArgument
        );
        let mut bad_base = codes_only("Other");
        bad_base.based_on = Some(CustomisationBase {
            name: String::from("a/b"),
            digest: String::from("0123456789abcdef"),
        });
        assert_eq!(
            refusal_of(document.install_customisation(
                bad_base,
                CustomisationOrigin::Loaded,
                false
            )),
            ErrorCode::InvalidArgument
        );

        assert!(*document.customisation_state() == before);
        assert_eq!(document.style_library().size(), 2);
        assert_eq!(document.survey_map().size(), 1);
        assert_eq!(document.customisation_generation(), 1);
        assert!(recorder.calls().is_empty());
    }

    #[test]
    fn every_part_that_could_not_be_kept_or_recorded_is_a_fault_that_names_the_part() {
        // Nothing wrong: site(), and a definition made in a session, which has
        // no source and needs none.
        let mut sound = site();
        assert!(sound.library.add(definition("MINE Mark", "", true)).is_ok());
        assert!(customisation_faults(&sound).is_empty());
        let mut document = Document::default();
        assert!(
            document
                .install_customisation(sound, CustomisationOrigin::Loaded, false)
                .is_ok()
        );

        // One of each planted, so five faults, in the order they are looked
        // for: the name, a source's name, a definition's source, the control
        // codes, what it is based on. Each says which part it is about first.
        let mut faulty = site();
        faulty.name = String::from("roads/2026");
        faulty.sources = vec![
            source("fine", true, true, &[]),
            source("not\\fine", true, false, &[]),
        ];
        assert!(
            faulty
                .library
                .add(definition("ROAD Kerb", "lib/roads", false))
                .is_ok()
        );
        faulty.linework = Some(LineworkCodes {
            start: String::from("X"),
            close: String::from("x"),
            ..LineworkCodes::default()
        });
        faulty.based_on = Some(CustomisationBase {
            name: String::from("NSW"),
            digest: String::from("0123"),
        });

        let faults = customisation_faults(&faulty);
        let parts = [
            "its name: ",
            "the name of one of its sources: ",
            "the source of its definition \"ROAD Kerb\": ",
            "its linework codes: ",
            "its basedOn: ",
        ];
        assert_eq!(faults.len(), parts.len());
        for (i, (fault, part)) in faults.iter().zip(parts).enumerate() {
            assert_eq!(fault.code, ErrorCode::InvalidArgument, "{i}");
            assert!(fault.message.starts_with(part), "{}", fault.describe());
        }
    }

    // ---- the change bit -------------------------------------------------------------

    #[test]
    #[allow(
        clippy::assertions_on_constants,
        reason = "The C++ checks the constants at run time, as its twin does."
    )]
    fn its_change_bit_is_the_eighteenth_and_is_no_part_of_what_a_drawing_is_drawn_from() {
        assert_eq!(DocumentChange::CUSTOMISATION, 1_u32 << 18);
        assert_eq!(DocumentChange::DRAWING & DocumentChange::CUSTOMISATION, 0);
        // Alone - a switch, a name - it draws nothing.
        assert!(!DocumentChange::of(DocumentChange::CUSTOMISATION).changes_drawing());
        // And no other part shares the bit.
        for other in [
            DocumentChange::ENTITIES,
            DocumentChange::SELECTION,
            DocumentChange::TABLES,
            DocumentChange::CURRENT_ATTRIBUTES,
            DocumentChange::HISTORY,
            DocumentChange::SAVED,
            DocumentChange::METADATA,
            DocumentChange::STYLE_LIBRARY,
            DocumentChange::SURVEY_MAP,
            DocumentChange::REPLACED,
            DocumentChange::DRAFTING,
        ] {
            assert_eq!(other & DocumentChange::CUSTOMISATION, 0, "{other}");
        }
    }

    #[test]
    fn a_new_drawing_keeps_the_customisation_and_does_not_report_it() {
        let mut document = Document::default();
        must(document.install_customisation(site(), CustomisationOrigin::BuiltIn, true));
        let before = document.customisation_state().clone();
        let recorder = Recorder::new(&document);

        document.new_document();

        let calls = recorder.calls();
        assert_eq!(calls.len(), 1);
        let change = DocumentChange::of(calls[0]);
        assert!(change.has(DocumentChange::REPLACED));
        assert!(!change.has(DocumentChange::CUSTOMISATION));
        assert!(!change.has(DocumentChange::STYLE_LIBRARY));
        assert!(*document.customisation_state() == before);
        assert_eq!(document.style_library().size(), 2);
        assert_eq!(document.customisation_generation(), 1);
    }

    // ---- the raw setters, which an editor calls -------------------------------------

    #[test]
    fn the_raw_setters_mark_the_session_edited_and_report_their_own_part_alone() {
        let mut document = Document::default();
        must(document.install_customisation(site(), CustomisationOrigin::BuiltIn, true));
        let recorder = Recorder::new(&document);

        let mut library = StyleLibrary::default();
        assert!(library.add(definition("MINE Mark", "", true)).is_ok());
        document.set_style_library(library);

        // Exactly what it has always reported, in one notification.
        let calls = recorder.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], DocumentChange::STYLE_LIBRARY);
        assert_eq!(
            document.customisation_state().origin,
            CustomisationOrigin::Edited
        );
        assert!(!document.customisation_state().kept);
        // The generation is how a panel showing the origin sees it: 1 for the
        // install, 2 for the edit.
        assert_eq!(document.customisation_generation(), 2);
        // An edit keeps the name: it is still that customisation, changed.
        assert_eq!(document.customisation_state().name, "Site");

        document.set_survey_map(SurveyMap::default());
        let calls = recorder.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1], DocumentChange::SURVEY_MAP);
        assert_eq!(
            document.customisation_state().origin,
            CustomisationOrigin::Edited
        );
        assert_eq!(
            document.customisation_generation(),
            2,
            "already edited: nothing of it changed"
        );
    }

    #[test]
    fn either_raw_setter_makes_an_empty_session_an_edited_one() {
        let mut by_library = Document::default();
        by_library.set_style_library(StyleLibrary::default());
        assert_eq!(
            by_library.customisation_state().origin,
            CustomisationOrigin::Edited
        );
        assert!(!by_library.customisation_state().kept);

        let mut by_map = Document::default();
        by_map.set_survey_map(SurveyMap::default());
        assert_eq!(
            by_map.customisation_state().origin,
            CustomisationOrigin::Edited
        );
        assert_eq!(by_map.customisation_generation(), 1);
    }

    // ---- the parts set on their own -------------------------------------------------

    #[test]
    fn a_colour_table_also_moves_the_library_generation_and_reports_the_library() {
        let mut document = Document::default();
        must(document.install_customisation(site(), CustomisationOrigin::BuiltIn, true));
        let recorder = Recorder::new(&document);

        let mut colours = ColourTable::default();
        assert!(colours.add("sui gas", Color::new(200, 100, 0, 255)).is_ok());
        document.set_colour_table(colours.clone());

        let calls = recorder.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            DocumentChange::CUSTOMISATION | DocumentChange::STYLE_LIBRARY
        );
        assert!(
            DocumentChange::of(calls[0]).changes_drawing(),
            "a pen inside a definition is drawn in another colour now"
        );
        // What is baked from the library - a sprite, a thumbnail - holds the
        // old colour, and is dropped only when this moves.
        assert_eq!(document.library_generation(), 2);
        assert_eq!(document.customisation_generation(), 2);
        assert_eq!(document.survey_map_generation(), 1);
        assert_eq!(document.customisation_state().colours, colours);
        assert_eq!(
            document.customisation_state().origin,
            CustomisationOrigin::Edited
        );
        assert!(!document.customisation_state().kept);

        // The table it already has is no change at all.
        document.set_colour_table(colours);
        assert_eq!(recorder.calls().len(), 1);
        assert_eq!(document.library_generation(), 2);
        assert_eq!(document.customisation_generation(), 2);
    }

    #[test]
    fn the_control_codes_are_checked_and_reported_as_the_customisation_alone() {
        let mut document = Document::default();
        let recorder = Recorder::new(&document);

        let codes = LineworkCodes {
            start: String::from("BEGIN"),
            rectangle: String::new(), // an empty spelling switches the control off
            ..LineworkCodes::default()
        };
        must(document.set_linework_codes(codes.clone()));
        let calls = recorder.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], DocumentChange::CUSTOMISATION);
        assert_eq!(document.customisation_state().linework, codes);
        assert_eq!(
            document.customisation_state().origin,
            CustomisationOrigin::Edited
        );
        assert_eq!(document.customisation_generation(), 1);
        assert_eq!(document.library_generation(), 0);

        // A spelling with a blank could never be matched: refused, nothing
        // moved.
        let blank = LineworkCodes {
            end: String::from("THE END"),
            ..codes.clone()
        };
        assert_eq!(
            refusal_of(document.set_linework_codes(blank)),
            ErrorCode::InvalidArgument
        );
        // Two controls spelled alike, whatever the case.
        let alike = LineworkCodes {
            close: String::from("begin"),
            ..codes.clone()
        };
        assert_eq!(
            refusal_of(document.set_linework_codes(alike)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(document.customisation_state().linework, codes);
        // The codes it already has are no change.
        must(document.set_linework_codes(codes));
        assert_eq!(recorder.calls().len(), 1);
        assert_eq!(document.customisation_generation(), 1);
    }

    #[test]
    fn the_switches_and_whether_it_is_kept_are_reported_as_the_customisation_alone() {
        let mut document = Document::default();
        must(document.install_customisation(site(), CustomisationOrigin::BuiltIn, true));
        let recorder = Recorder::new(&document);

        // site() has coding off and linework on; turn linework off too.
        document.set_automation(automation(false, false));
        let calls = recorder.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], DocumentChange::CUSTOMISATION);
        assert!(
            !document
                .customisation_state()
                .automation
                .linework_on_survey_import
        );
        assert_eq!(
            document.customisation_state().origin,
            CustomisationOrigin::Edited
        );
        assert!(
            !document.customisation_state().kept,
            "it differs from what was kept"
        );
        document.set_automation(automation(false, false));
        assert_eq!(recorder.calls().len(), 1, "the switches it already has");

        // Written to the kept file: kept again, and still an edited built-in.
        document.set_customisation_kept(true);
        let calls = recorder.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1], DocumentChange::CUSTOMISATION);
        assert!(document.customisation_state().kept);
        assert_eq!(
            document.customisation_state().origin,
            CustomisationOrigin::Edited
        );
        document.set_customisation_kept(true);
        assert_eq!(recorder.calls().len(), 2);
        // 1 install, 2 switches, 3 kept.
        assert_eq!(document.customisation_generation(), 3);
    }

    // ---- what a project records -----------------------------------------------------

    #[test]
    fn a_save_writes_the_record_without_the_drawing_ever_being_modified_by_it() {
        let scratch = Scratch::new("save-writes-the-record");
        // Scoped: the project is opened again below, by another Document.
        {
            let mut document = Document::default();
            must(document.install_customisation(site(), CustomisationOrigin::Loaded, false));
            assert!(
                !document.is_modified(),
                "a customisation is not an edit of the drawing"
            );
            assert!(
                document.metadata().customisation.is_empty(),
                "nothing is recorded until a save"
            );

            // A save with nowhere to go: refused, and the drawing is still
            // untouched. (The record was once written through set_metadata
            // before the save, which left exactly this drawing asking to be
            // saved.)
            assert_eq!(refusal_of(document.save()), ErrorCode::InvalidState);
            assert!(!document.is_modified());
            assert!(document.metadata().customisation.is_empty());

            // Somewhere to go: the record is the one name the session is
            // drawn with.
            let recorder = Recorder::new(&document);
            must(document.save_as(&scratch.root.join("site.katana")));
            assert!(!document.is_modified());
            assert_eq!(document.metadata().customisation, strings(&["Site"]));

            // Saved again, the record is the same and only the save is
            // reported.
            must(document.save());
            assert_eq!(
                recorder.calls().last().copied(),
                Some(DocumentChange::SAVED)
            );
            assert!(!document.is_modified());
        }

        // And it is IN the project, not only in this Document.
        let mut reopened = Document::default();
        must(reopened.open(&scratch.root.join("site.katana")));
        assert_eq!(reopened.metadata().customisation, strings(&["Site"]));
    }

    #[test]
    fn a_save_that_fails_part_way_leaves_the_record_and_the_drawing_as_they_were() {
        // The record is written INTO what is saved and becomes the metadata's
        // only once the save has succeeded, so a save the project store
        // refuses leaves no trace. (Written through set_metadata before the
        // save, as it once was, it stayed, and the untouched drawing asked to
        // be saved.) The refusal is arranged with the one thing a store
        // refuses of a record - a name that is a path - as a definition's
        // source, given through the raw setter, which checks nothing: an
        // install would have refused it.
        let scratch = Scratch::new("save-fails-part-way");
        let project = scratch.root.join("site.katana");
        let mut unrecordable = StyleLibrary::default();
        assert!(
            unrecordable
                .add(definition("ROAD Kerb", "lib/roads", false))
                .is_ok()
        );
        {
            let mut document = Document::default();
            must(document.install_customisation(site(), CustomisationOrigin::Loaded, false));
            must(document.save_as(&project));
            assert_eq!(document.metadata().customisation, strings(&["Site"]));

            document.set_style_library(unrecordable.clone());
            assert!(
                !document.is_modified(),
                "the library is session data, not the drawing"
            );
            let before = document.metadata().clone();
            let recorder = Recorder::new(&document);

            // By hand: the record would be "Site" (a source that brought
            // rules) and then "lib/roads" (a definition's source no load
            // accounts for), and the store takes no name with a '/'.
            let refused = document.save().expect_err("it was not refused");
            assert_eq!(refused.code, ErrorCode::InvalidArgument);
            assert!(
                refused.context.contains("lib/roads"),
                "{}",
                refused.describe()
            );
            assert!(
                *document.metadata() == before,
                "nothing of the save is left in it"
            );
            assert_eq!(document.metadata().customisation, strings(&["Site"]));
            assert!(!document.is_modified());
            assert!(
                recorder.calls().is_empty(),
                "nothing was saved, so nothing is reported"
            );
        }
        {
            // The same of a first save, which has no project yet - and has
            // none after it.
            let mut unsaved = Document::default();
            unsaved.set_style_library(unrecordable);
            assert_eq!(
                refusal_of(unsaved.save_as(&scratch.root.join("never.katana"))),
                ErrorCode::InvalidArgument
            );
            assert!(unsaved.metadata().customisation.is_empty());
            assert!(!unsaved.is_modified());
            assert!(!unsaved.has_project());
        }

        // The project still holds what the save that worked wrote.
        let mut reopened = Document::default();
        must(reopened.open(&project));
        assert_eq!(reopened.metadata().customisation, strings(&["Site"]));
    }

    #[test]
    fn an_open_works_out_what_is_not_loaded_and_a_new_drawing_forgets_it() {
        let scratch = Scratch::new("open-works-out-missing");
        let project = scratch.root.join("recorded.katana");
        project_recording(&project, &["Site", "old symbols.4d"]);

        {
            // Nothing loaded: both recorded names are missing, in the
            // project's order, and the open says the customisation's state
            // changed.
            let mut empty = Document::default();
            let recorder = Recorder::new(&empty);
            must(empty.open(&project));
            assert_eq!(
                empty.customisation_state().missing_at_open,
                strings(&["Site", "old symbols.4d"])
            );
            let calls = recorder.calls();
            assert_eq!(calls.len(), 1);
            assert!(DocumentChange::of(calls[0]).has(DocumentChange::REPLACED));
            assert!(DocumentChange::of(calls[0]).has(DocumentChange::CUSTOMISATION));
            assert_eq!(empty.customisation_generation(), 1);

            // A new drawing recorded nothing, so it is missing nothing.
            empty.new_document();
            assert!(empty.customisation_state().missing_at_open.is_empty());
            let calls = recorder.calls();
            assert_eq!(calls.len(), 2);
            assert!(DocumentChange::of(calls[1]).has(DocumentChange::CUSTOMISATION));
            assert_eq!(empty.customisation_generation(), 2);
            // And a second new drawing has nothing to forget.
            empty.new_document();
            let calls = recorder.calls();
            assert!(!DocumentChange::of(calls[2]).has(DocumentChange::CUSTOMISATION));
            assert_eq!(empty.customisation_generation(), 2);
        }
        {
            // "Site" loaded: only the other name is missing.
            let mut with_site = Document::default();
            must(with_site.install_customisation(site(), CustomisationOrigin::Loaded, false));
            must(with_site.open(&project));
            assert_eq!(
                with_site.customisation_state().missing_at_open,
                strings(&["old symbols.4d"])
            );
            // The customisation itself is kept across the open.
            assert_eq!(with_site.customisation_state().name, "Site");
            assert_eq!(with_site.style_library().size(), 2);
        }
    }

    #[test]
    fn a_save_keeps_the_names_the_open_found_missing_until_they_are_loaded() {
        let scratch = Scratch::new("save-keeps-missing");
        let project = scratch.root.join("site.katana");
        {
            let mut author = Document::default();
            must(author.install_customisation(site(), CustomisationOrigin::Loaded, false));
            must(author.save_as(&project));
        }

        // Another session, with other codes and without "Site". By hand: the
        // open finds "Site" missing; an unedited save records what this
        // session is drawn with, "Other", and keeps the name it could not
        // judge, after it.
        let mut other = Document::default();
        must(other.install_customisation(codes_only("Other"), CustomisationOrigin::Loaded, false));
        must(other.open(&project));
        assert_eq!(
            other.customisation_state().missing_at_open,
            strings(&["Site"])
        );
        {
            let recorder = Recorder::new(&other);
            must(other.save());
            assert_eq!(other.metadata().customisation, strings(&["Other", "Site"]));
            // The record the save wrote is the metadata's now, and is
            // reported.
            let calls = recorder.calls();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0], DocumentChange::SAVED | DocumentChange::METADATA);
            assert!(!other.is_modified());
        }

        // "Site" installed in the place of "Other": it is loaded now, so it is
        // no longer missing; and "Other", which this session loaded and then
        // replaced, it has judged - the record lets it go.
        must(other.install_customisation(site(), CustomisationOrigin::Loaded, false));
        assert!(other.customisation_state().missing_at_open.is_empty());
        must(other.save());
        assert_eq!(other.metadata().customisation, strings(&["Site"]));
    }

    // The four files a very old project may record were the NSW customisation's
    // (customisation_record.rs, EARLIER_FILE_NAMES_BELONG_TO). So NSW answers
    // them, whichever built-in the run has, and no other customisation does:
    // the default of a program is another one, and a project drawn with NSW's
    // linestyles must not be told its customisation is loaded because the
    // default is.
    #[test]
    fn the_earlier_file_names_are_answered_by_nsw_and_by_no_other_built_in() {
        let scratch = Scratch::new("earlier-names");
        let project = scratch.root.join("old.katana");
        let general = [
            "linestyles.4d",
            "survey_codes.mapfile",
            "survey_codes_names.mapfile",
            "symbols.4d",
        ];
        project_recording(&project, &general);

        {
            // The same customisation loaded, but no host has said it is the
            // built-in: the four names are four files this session does not
            // have.
            let mut untold = Document::default();
            must(untold.install_customisation(site(), CustomisationOrigin::Loaded, false));
            must(untold.open(&project));
            assert_eq!(
                untold.customisation_state().missing_at_open,
                strings(&general)
            );
        }
        {
            // Told that the built-in of the run is called "Site": it is that of
            // the run (CustomisationState::built_in), and it is not NSW, whose
            // files the four were, so it does not answer them: they stay
            // missing, and the save keeps them in the record beside the one it
            // draws with.
            let mut run = Document::default();
            must(run.install_customisation(site(), CustomisationOrigin::BuiltIn, true));
            let recorder = Recorder::new(&run);
            run.set_built_in_customisation_name(String::from("Site"));
            let calls = recorder.calls();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0], DocumentChange::CUSTOMISATION);
            assert_eq!(run.customisation_state().built_in, "Site");

            must(run.open(&project));
            assert_eq!(run.customisation_state().missing_at_open, strings(&general));
            // To a copy: the project above is read again by the part after
            // this one.
            must(run.save_as(&scratch.root.join("resaved.katana")));
            assert_eq!(
                run.metadata().customisation.first().map(String::as_str),
                Some("Site")
            );
            for name in general {
                assert!(
                    run.metadata()
                        .customisation
                        .iter()
                        .any(|recorded| recorded == name),
                    "{name} is still recorded"
                );
            }

            // Then NSW is made the session, as CUSTOMISE BUILTIN NSW does, on
            // the run whose built-in is still "Site": the four files are
            // loaded now, by the customisation that was made of them.
            must(run.install_customisation(codes_only("NSW"), CustomisationOrigin::BuiltIn, false));
            assert!(run.customisation_state().missing_at_open.is_empty());
            assert_eq!(run.customisation_state().built_in, "Site");
        }
        {
            // NSW as the built-in of the run answers them from the start:
            // nothing is missing, and the save records the one name the
            // drawing is drawn with now.
            let mut told = Document::default();
            must(told.install_customisation(codes_only("NSW"), CustomisationOrigin::BuiltIn, true));
            told.set_built_in_customisation_name(String::from("NSW"));
            must(told.open(&project));
            assert!(told.customisation_state().missing_at_open.is_empty());
            must(told.save());
            assert_eq!(told.metadata().customisation, strings(&["NSW"]));
        }
    }

    // ---- a load installed through the raw setters -----------------------------------

    #[test]
    fn a_load_recorded_by_its_sources_joins_the_sessions_and_comes_off_what_is_missing() {
        let scratch = Scratch::new("record-a-load");
        let project = scratch.root.join("two.katana");
        project_recording(&project, &["a.4d", "b.mapfile"]);

        let mut document = Document::default();
        must(document.open(&project));
        assert_eq!(
            document.customisation_state().missing_at_open,
            strings(&["a.4d", "b.mapfile"])
        );
        let recorder = Recorder::new(&document);

        // What a front end that read the files itself does: the two raw
        // setters, then the sources of what it read.
        document.set_style_library(StyleLibrary::default());
        document.record_customisation_load(&[source("a.4d", true, false, &[])], false, false);

        let calls = recorder.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1], DocumentChange::CUSTOMISATION);
        assert_eq!(
            document.customisation_state().sources,
            vec![source("a.4d", true, false, &[])]
        );
        // A load, not an edit - the setter before it had said "edited".
        assert_eq!(
            document.customisation_state().origin,
            CustomisationOrigin::Loaded
        );
        assert!(!document.customisation_state().kept);
        assert_eq!(
            document.customisation_state().missing_at_open,
            strings(&["b.mapfile"])
        );

        // A Replace by survey codes takes no library's place.
        document.record_customisation_load(&[source("c.mapfile", false, true, &[])], false, true);
        assert_eq!(
            document.customisation_state().sources,
            vec![
                source("a.4d", true, false, &[]),
                source("c.mapfile", false, true, &[]),
            ]
        );
    }
}
