//! The sheet painter drawing a frame a customisation offers (docs/plotting.md, "A customisation's
//! title frames"): the NSW built-in's frame prints its own heading and its filled logo where the
//! default frame leaves the organisation and logo slots empty, and a sheet naming a frame the
//! customisation in use does not offer is drawn with the default frame and the painter says so.
//!
//! Ink is compared with a baseline painted in the same test - the same sheet with the default
//! frame - never as a count of text pixels: fonts differ between machines (CLAUDE.md, "Testing").
//! The heading's place is the frame's own text anchor and cell, read from the frame, so no
//! organisation's name is written here (CLAUDE.md section 9).

use katana_cad::CustomisationOrigin;
use katana_cad::document::Document;
use katana_cad::plotting::frame::{BUILT_IN_FRAME_ID, built_in_frame, compiled_in_frame};
use katana_cad::plotting::sheet_set::{Sheet, SheetSet};
use katana_entity::{Customisation, CustomisationPlotFrame, Model};
use katana_geometry::Box2;
use katana_window::canvas::Color;
use katana_window::plan_painter::PlanSource;
use katana_window::sheet_painter::SheetSource;

use crate::support::{bx, colour_in, ink_in, painted};

const FRAME: &str = "nsw_a3_utility_cross_section";

fn framed(frame: &str) -> SheetSet {
    SheetSet {
        sheets: vec![Sheet {
            id: String::from("s1"),
            name: String::from("PLAN"),
            frame: String::from(frame),
            ..Sheet::default()
        }],
        ..SheetSet::default()
    }
}

/// A session whose customisation offers `frames`.
fn offering(frames: &[&str]) -> Document {
    let mut document = Document::new(None);
    let customisation = Customisation {
        name: String::from("Probe"),
        plot_frames: frames
            .iter()
            .map(|frame| CustomisationPlotFrame {
                name: String::from(*frame),
            })
            .collect(),
        ..Customisation::default()
    };
    document
        .install_customisation(customisation, CustomisationOrigin::Loaded, false)
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    document
}

fn source_with<'a>(model: &'a Model, document: &'a Document) -> SheetSource<'a> {
    SheetSource {
        plan: PlanSource {
            model: Some(model),
            ..PlanSource::default()
        },
        document: Some(document),
        ..SheetSource::default()
    }
}

/// Where the customisation frame's heading is printed: from its anchor to the right edge of the
/// organisation cell it is in, the capital's height above the baseline - read from the frame.
fn heading_box() -> Box2 {
    let frame = match compiled_in_frame(FRAME) {
        Some(Ok(frame)) => frame,
        _ => panic!("{FRAME} is compiled in and reads"),
    };
    let cell = frame.cell("organisation").expect("the organisation cell").rect;
    let heading = frame
        .texts
        .iter()
        .filter(|text| cell.contains(text.anchor))
        .max_by(|a, b| a.cap_height_mm.total_cmp(&b.cap_height_mm))
        .expect("a text in the organisation cell");
    bx(
        heading.anchor.x + 0.5,
        heading.anchor.y + 0.2,
        cell.max.x - 0.5,
        heading.anchor.y + heading.cap_height_mm - 0.2,
    )
}

/// The default frame's logo cell, which the customisation frame's mark lies in (168.6 to 178.9 by
/// 28.1 to 34.3 mm, inside 167.265 to 220.229 by 22.625 to 34.625).
fn logo_box() -> Box2 {
    built_in_frame()
        .as_ref()
        .expect("the default frame reads")
        .cell("logo")
        .expect("the logo cell")
        .rect
}

mod customisation_frame_painter {
    use super::*;

    #[test]
    fn the_customisation_frame_prints_its_heading_where_the_default_frame_prints_nothing() {
        let model = Model::new();
        let document = offering(&[FRAME]);
        let source = source_with(&model, &document);
        let around = heading_box();

        // The baseline: the same sheet in the default frame, whose organisation slot is empty.
        let (default, _) = painted(&framed(BUILT_IN_FRAME_ID), &source, "frame_default_heading");
        let baseline = ink_in(&default, &around);

        let (own, stats) = painted(&framed(FRAME), &source, "frame_customisation_heading");
        let heading = ink_in(&own, &around);
        assert!(heading > baseline + 100, "heading {heading}, baseline {baseline}");
        assert!(
            stats.problems.iter().all(|problem| !problem.starts_with("frame:")),
            "{:?}",
            stats.problems
        );
        // Every text of the frame is drawn: 80, none of them a slot.
        assert!(stats.frame_texts_drawn > 0);
        assert!(!stats.logo_drawn);
    }

    #[test]
    fn the_customisation_frame_fills_its_logo_in_red_where_the_default_slot_is_empty() {
        let model = Model::new();
        let document = offering(&[FRAME]);
        let source = source_with(&model, &document);
        // "acad 010", plotted #FF0000; antialiased edges are allowed 40 a channel.
        let red = Color::rgb(255, 0, 0);
        let (default, _) = painted(&framed(BUILT_IN_FRAME_ID), &source, "frame_default_logo");
        assert_eq!(colour_in(&default, &logo_box(), red, 40), 0);
        let (own, _) = painted(&framed(FRAME), &source, "frame_customisation_logo");
        // The filled mark, not its outlines: its 13 distinct shapes cover 35.5 mm2 (their
        // vertices, by the shoelace formula), 568 pixels at 16 a square millimetre; a 0.18 mm
        // outline is under a pixel wide and mostly antialiased off pure red. Over 350 is a fill.
        assert!(colour_in(&own, &logo_box(), red, 40) > 350);
    }

    #[test]
    fn a_frame_the_customisation_does_not_offer_is_drawn_as_the_default_and_said() {
        let model = Model::new();
        let plain = offering(&[]);
        let source = source_with(&model, &plain);
        let (fallback, stats) = painted(&framed(FRAME), &source, "frame_not_offered");
        let (default, _) = painted(&framed(BUILT_IN_FRAME_ID), &source, "frame_not_offered_default");
        // Drawn exactly as the default frame is: the same pixels.
        assert!(fallback == default, "the fallback is not the default frame");
        let said: Vec<&String> = stats
            .problems
            .iter()
            .filter(|problem| problem.starts_with("frame: "))
            .collect();
        assert_eq!(said.len(), 1, "{:?}", stats.problems);
        assert!(said[0].contains(FRAME), "{}", said[0]);
        assert!(said[0].contains("not one the customisation"), "{}", said[0]);
        // And the heading is not printed.
        assert!(ink_in(&fallback, &heading_box()) <= ink_in(&default, &heading_box()));
    }

    #[test]
    fn with_no_document_a_compiled_in_frame_is_drawn_as_itself() {
        let model = Model::new();
        let source = SheetSource {
            plan: PlanSource {
                model: Some(&model),
                ..PlanSource::default()
            },
            ..SheetSource::default()
        };
        let (own, stats) = painted(&framed(FRAME), &source, "frame_no_document");
        let (default, _) = painted(&framed(BUILT_IN_FRAME_ID), &source, "frame_no_document_default");
        assert!(ink_in(&own, &heading_box()) > ink_in(&default, &heading_box()) + 100);
        assert!(stats.problems.iter().all(|problem| !problem.starts_with("frame:")));
    }
}
