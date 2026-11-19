// The title frames a customisation offers, on the command line
// (docs/plotting.md, "A customisation's title frames"; sheet_verbs.rs):
// SHEETS FRAMES lists them, SHEETS PAGESETUP frame= chooses one for the set
// and puts it on every framed sheet as one step, frame= on a sheet takes one
// on offer, the choice is kept in the set's JSON, and a set naming a frame the
// customisation in use does not offer is drawn with the default and said so.
//
// The customisations are invented ("Probe" and "Plain"); the frame they offer
// is the one Katana compiles in beside the default, by its name.

use katana_cad::CustomisationOrigin;
use katana_cad::plotting::frame::BUILT_IN_FRAME_ID;
use katana_cad::plotting::preflight::findings_from_json;
use katana_cad::plotting::sheet_json::{sheet_set_from_json, sheet_set_to_json};
use katana_cad::plotting::{Finding, Severity};
use katana_core::ErrorCode;
use katana_entity::{Customisation, CustomisationPlotFrame};

use crate::{Session, contains};

const FRAME: &str = "nsw_a3_utility_cross_section";

// A session whose customisation offers `frames`, installed as a load is.
fn offering(frames: &[&str]) -> Session {
    let mut session = Session::default();
    install(&mut session, "Probe", frames);
    session
}

fn install(session: &mut Session, name: &str, frames: &[&str]) {
    let customisation = Customisation {
        name: String::from(name),
        plot_frames: frames
            .iter()
            .map(|frame| CustomisationPlotFrame {
                name: String::from(*frame),
            })
            .collect(),
        ..Customisation::default()
    };
    session
        .document
        .install_customisation(customisation, CustomisationOrigin::Loaded, false)
        .unwrap_or_else(|error| panic!("{}", error.describe()));
}

fn lines(reply: &str) -> Vec<&str> {
    reply.lines().collect()
}

mod sheet_frame_verbs {
    use super::*;

    #[test]
    fn an_install_gives_the_session_the_frames_the_customisation_offers() {
        let session = offering(&[FRAME]);
        let state = session.document.customisation_state();
        assert_eq!(state.plot_frames.len(), 1);
        assert_eq!(state.plot_frames[0].name, FRAME);
        // And the session as one customisation - what KEEP writes - offers it.
        assert_eq!(session.document.customisation().plot_frames, state.plot_frames);
    }

    #[test]
    fn sheets_frames_lists_the_default_alone_when_the_customisation_offers_none() {
        let mut s = Session::default();
        assert_eq!(
            lines(&s.ok("SHEETS FRAMES")),
            vec![
                "frames count=1 set=a3_landscape customisation=\"\"",
                "frame name=a3_landscape from=default"
            ]
        );
    }

    #[test]
    fn sheets_frames_lists_the_default_then_the_customisations_frames() {
        let mut s = offering(&[FRAME, "no_such_frame"]);
        assert_eq!(
            lines(&s.ok("SHEETS FRAMES")),
            vec![
                "frames count=2 set=a3_landscape customisation=\"Probe\"",
                "frame name=a3_landscape from=default",
                "frame name=nsw_a3_utility_cross_section from=customisation",
                "problem the customisation \"Probe\" offers the frame no_such_frame, which Katana \
                 does not have",
            ]
        );
        assert_eq!(s.refused("SHEETS FRAMES extra").code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn the_page_setup_frame_goes_on_every_framed_sheet_in_one_step() {
        let mut s = offering(&[FRAME]);
        s.ok("SHEET NEW PLAN");
        s.ok("SHEET NEW BARE frame=off");
        s.ok("SHEET NEW SECTIONS");
        let steps = s.steps();
        let reply = s.ok(&format!("SHEETS PAGESETUP frame={FRAME}"));
        assert!(reply.ends_with(&format!(" frame={FRAME}")), "{reply}");
        assert!(reply.starts_with("pagesetup style=colour"), "{reply}");
        assert_eq!(s.steps(), steps + 1);
        assert_eq!(s.document.history().undo_name(), "PAGE_SETUP");
        let set = s.set();
        assert_eq!(set.page_setup.frame, FRAME);
        assert_eq!(set.sheets[0].frame, FRAME);
        // A sheet whose frame is off keeps it off.
        assert!(set.sheets[1].frame.is_empty());
        assert_eq!(set.sheets[2].frame, FRAME);

        // The same again is nothing at all.
        s.ok(&format!("SHEETS PAGESETUP frame={FRAME}"));
        assert_eq!(s.steps(), steps + 1);

        // Undo takes the frame back off the page setup and the sheets together.
        s.ok("UNDO");
        let set = s.set();
        assert_eq!(set.page_setup.frame, BUILT_IN_FRAME_ID);
        assert_eq!(set.sheets[0].frame, BUILT_IN_FRAME_ID);
        assert_eq!(set.sheets[2].frame, BUILT_IN_FRAME_ID);
    }

    #[test]
    fn a_page_setup_without_frame_leaves_the_sheets_frames_alone() {
        let mut s = offering(&[FRAME]);
        s.ok("SHEET NEW PLAN");
        s.ok(&format!("SHEET SET 1 frame={FRAME}"));
        // The colours changed, no frame named: the sheet keeps the frame it was given.
        s.ok("SHEETS PAGESETUP style=grey");
        assert_eq!(s.set().sheets[0].frame, FRAME);
        assert_eq!(s.set().page_setup.frame, BUILT_IN_FRAME_ID);
    }

    #[test]
    fn a_new_sheet_and_frame_on_take_the_sets_frame() {
        let mut s = offering(&[FRAME]);
        s.ok(&format!("SHEETS PAGESETUP frame={FRAME}"));
        let added = s.ok("SHEET NEW PLAN");
        assert!(contains(&added, &format!("frame={FRAME}")), "{added}");
        s.ok("SHEET SET 1 frame=off");
        assert!(s.set().sheets[0].frame.is_empty());
        let on = s.ok("SHEET SET 1 frame=on");
        assert!(contains(&on, &format!("frame={FRAME}")), "{on}");
        // A frame on offer by its name, in any case; the default by its own.
        let default = s.ok("SHEET SET 1 frame=A3_LANDSCAPE");
        assert!(contains(&default, "frame=a3_landscape"), "{default}");
        let named = s.ok(&format!("SHEET SET 1 frame={}", FRAME.to_uppercase()));
        assert!(contains(&named, &format!("frame={FRAME}")), "{named}");
    }

    #[test]
    fn a_frame_the_customisation_does_not_offer_is_refused_and_changes_nothing() {
        let mut s = Session::default();
        s.ok("SHEET NEW PLAN");
        let before = sheet_set_to_json(&s.set()).expect("the set's JSON");
        let steps = s.steps();
        for line in [
            format!("SHEETS PAGESETUP frame={FRAME}"),
            format!("SHEET SET 1 frame={FRAME}"),
            format!("SHEET NEW OTHER frame={FRAME}"),
            String::from("SHEETS PAGESETUP frame=off"),
        ] {
            let error = s.refused(&line);
            assert_eq!(error.code, ErrorCode::NotFound, "{line}");
            assert!(contains(&error.message, "no plot frame of that name"), "{line}");
            // It says what is on offer.
            assert!(contains(&error.message, "a3_landscape"), "{line}");
        }
        assert_eq!(s.steps(), steps);
        assert_eq!(sheet_set_to_json(&s.set()).expect("the set's JSON"), before);
    }

    #[test]
    fn the_chosen_frame_is_kept_in_the_sets_json_and_read_back() {
        let mut s = offering(&[FRAME]);
        s.ok("SHEET NEW PLAN");
        let default_json = s.ok("SHEETS JSON");
        // At the default frame the page setup says nothing of it.
        assert!(!contains(&default_json, "page_setup"), "{default_json}");
        s.ok(&format!("SHEETS PAGESETUP frame={FRAME}"));
        let json = s.ok("SHEETS JSON");
        assert!(
            contains(&json, &format!("\"page_setup\":{{\"frame\":\"{FRAME}\"}}")),
            "{json}"
        );
        let read = sheet_set_from_json(&json).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(read.page_setup.frame, FRAME);
        assert_eq!(read.sheets[0].frame, FRAME);
        assert!(read == *s.set());
    }

    // A set made under a customisation that offered the frame, opened under
    // one that does not: drawn with the default, and said so - by SHEETS CHECK
    // (frame.unknown, naming the frame and the reason) and by SHEETS FRAMES.
    #[test]
    fn a_set_naming_a_frame_no_longer_offered_is_reported_not_silently_redrawn() {
        let mut s = offering(&[FRAME]);
        s.ok("SHEET NEW PLAN");
        s.ok(&format!("SHEETS PAGESETUP frame={FRAME}"));
        let clean: Vec<Finding> = findings_from_json(&s.ok("SHEETS CHECK json"))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(clean.iter().all(|finding| finding.code != "frame.unknown"));

        install(&mut s, "Plain", &[]);
        let findings: Vec<Finding> = findings_from_json(&s.ok("SHEETS CHECK json"))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let fallback: Vec<&Finding> = findings
            .iter()
            .filter(|finding| finding.code == "frame.unknown")
            .collect();
        assert_eq!(fallback.len(), 1, "{findings:?}");
        assert_eq!(fallback[0].severity, Severity::Error);
        assert_eq!(fallback[0].subject, FRAME);
        assert!(contains(&fallback[0].message, "not one the customisation"), "{fallback:?}");
        assert!(contains(&fallback[0].message, "a3_landscape"), "{fallback:?}");

        let frames = s.ok("SHEETS FRAMES");
        assert!(
            contains(
                &frames,
                &format!("problem the set's frame {FRAME} is not on offer")
            ),
            "{frames}"
        );
        // The set itself is not changed behind the person's back.
        assert_eq!(s.set().sheets[0].frame, FRAME);
    }
}
