// The built-in plot frame (include/katana/cad/plotting/frame.hpp) and the
// sheet scale ladder (plot.hpp); tests/cad/plotting/test_frame.cpp.
//
// Every expected value is read off resources/plot_frames/a3_landscape.json by
// hand - its counts, a cell's corners, a text's position plus its offset and
// raise - and then scaled or fitted by hand. None is the code's output.

use crate::{expect_box, near};
use katana_cad::PaperSize;
use katana_cad::plot::{SHEET_SCALES, STANDARD_SCALES, sheet_scale_at_least};
use katana_cad::plotting::frame::{
    BUILT_IN_FRAME_ID, built_in_frame, fit_image, frame_for, frame_scale_for, parse_frame,
};
use katana_cad::plotting::{Frame, FrameRole, FrameText, HorizontalJustify, VerticalJustify};
use katana_core::ErrorCode;

fn a3() -> &'static Frame {
    match built_in_frame() {
        Ok(frame) => frame,
        Err(error) => panic!("{}", error.describe()),
    }
}

fn text_at(frame: &Frame, x: f64, y: f64) -> Option<&FrameText> {
    frame
        .texts
        .iter()
        .find(|text| (text.anchor.x - x).abs() < 1e-9 && (text.anchor.y - y).abs() < 1e-9)
}

fn content_at(frame: &Frame, x: f64, y: f64) -> &str {
    &text_at(frame, x, y)
        .unwrap_or_else(|| panic!("no text at {x}, {y}"))
        .content
}

mod plot_frame {
    use super::*;

    #[test]
    fn the_built_in_frame_has_every_item_of_the_committed_data_and_two_user_slots() {
        // The JSON holds 145 entities: 57 "poly", 74 "text", 14 "symbol" (the
        // 34 branding entities were removed before it was committed). The
        // frame adds two texts of its own - the organisation and notes slots -
        // so 76 texts. 16 cells.
        let frame = a3();
        assert_eq!(frame.polylines.len(), 57);
        assert_eq!(frame.texts.len(), 76);
        assert_eq!(frame.symbols.len(), 14);
        assert_eq!(frame.cells.len(), 16);
    }

    #[test]
    fn the_sheet_is_a3_with_the_measured_margins_and_drawing_area() {
        // sheet.width_mm 420, height_mm 297; margins L23 R10 T10 B35; the
        // viewport 23..410 x 35..287 (387 x 252 mm).
        let frame = a3();
        assert_eq!(frame.width_mm, 420.0);
        assert_eq!(frame.height_mm, 297.0);
        assert_eq!(frame.margins.left, 23.0);
        assert_eq!(frame.margins.right, 10.0);
        assert_eq!(frame.margins.top, 10.0);
        assert_eq!(frame.margins.bottom, 35.0);
        expect_box(&frame.drawing_area, 23.0, 35.0, 410.0, 287.0);
        expect_box(&frame.border, 21.825, 34.625, 411.309, 289.265);
        expect_box(&frame.title_block, 21.825, 8.945, 411.309, 34.625);
    }

    #[test]
    fn the_logo_organisation_and_notes_cells_are_the_empty_slots() {
        let frame = a3();
        let logo = frame.cell("logo").expect("a logo cell");
        // 167.265..220.229 x 22.625..34.625: 52.964 x 12.0 mm.
        expect_box(&logo.rect, 167.265, 22.625, 220.229, 34.625);
        near(logo.rect.width(), 52.964, 1e-9);
        near(logo.rect.height(), 12.0, 1e-9);
        let organisation = frame.cell("organisation").expect("an organisation cell");
        expect_box(&organisation.rect, 297.117, 28.865, 401.843, 34.625);
        // The measured app's disclaimer cell is the notes slot here.
        assert!(frame.cell("disclaimer").is_none());
        let notes = frame.cell("notes").expect("a notes cell");
        expect_box(&notes.rect, 220.229, 12.0, 297.117, 34.625);
        // Nothing is drawn in the logo cell: no text anchors inside it.
        for text in &frame.texts {
            assert!(!logo.rect.contains(text.anchor), "{}", text.content);
        }
    }

    #[test]
    fn text_anchors_include_their_offset_and_raise() {
        let frame = a3();
        // "No. of" (#25): x 406.169 + offset -1.349 = 404.820; y 32.970 +
        // raise 0.063 = 33.033.
        let no_of = text_at(frame, 404.82, 33.033).expect("No. of");
        assert_eq!(no_of.content, "No. of");
        assert_eq!(no_of.cap_height_mm, 1.0);
        assert_eq!(no_of.x_factor, 0.82);
        // The scale value (#127): 125.835 + 6.545 = 132.380; 25.777 + 0.219 =
        // 25.996. Cap 3, and centred in the SCALE cell like the app does.
        let scale = text_at(frame, 132.38, 25.996).expect("the scale value");
        assert_eq!(scale.content, " {scale}");
        assert_eq!(scale.cap_height_mm, 3.0);
        assert_eq!(scale.centred_in, "scale");
        // "SHEET No." (#21) has no offset: 403.902, 20.518.
        let sheet_no = text_at(frame, 403.902, 20.518).expect("SHEET No.");
        assert_eq!(sheet_no.content, "SHEET No.");
        assert_eq!(sheet_no.horizontal, HorizontalJustify::Left);
        assert_eq!(sheet_no.vertical, VerticalJustify::Bottom);
        // "Survey Plan Filename:" (#36) runs up the left margin at 90 degrees.
        let filename = text_at(frame, 20.856, 9.459).expect("the file name label");
        assert_eq!(filename.angle_degrees, 90.0);
    }

    #[test]
    fn field_tokens_become_named_fields_and_each_sign_off_has_its_own_date() {
        let frame = a3();
        // #178 " $drawing_number" and #35 " $user_text<1,...>".
        assert_eq!(content_at(frame, 404.018, 13.105), " {sheet_number}");
        assert_eq!(content_at(frame, 404.018, 26.009), " {sheet_count}");
        assert_eq!(
            text_at(frame, 404.018, 26.009)
                .expect("the sheet count")
                .centred_in,
            "sheet_count"
        );
        // The five-line project block (#177), middle-centre at 350.275,
        // 26.447, its region-specific first-line label gone.
        let project = text_at(frame, 350.275, 26.447).expect("the project block");
        assert_eq!(
            project.content,
            "{project_line_1}\n{project_line_2}\n{project_line_3}\n{project_line_4}\nDrawing Set \
             Number: {set_number}"
        );
        assert_eq!(project.horizontal, HorizontalJustify::Centre);
        assert_eq!(project.vertical, VerticalJustify::Middle);
        // Multi-line text steps down by 1.5 x the cap height: 1.5 x 2.5.
        assert_eq!(project.line_spacing_mm, 3.75);
        // The coordinate system and height datum are fields, not one region's
        // text.
        assert_eq!(
            content_at(frame, 107.967, 19.993),
            "CO-ORD SYSTEM: {coordinate_system}"
        );
        assert_eq!(
            content_at(frame, 145.656, 20.118),
            "HEIGHT DATUM: {height_datum}"
        );
        // The six date texts: each sign-off row and the stamp its own field.
        assert_eq!(content_at(frame, 209.758, 20.395), "{locator_date}");
        assert_eq!(content_at(frame, 209.8, 16.952), "{surveyor_date}");
        assert_eq!(content_at(frame, 209.741, 13.439), "{compiler_date}");
        assert_eq!(content_at(frame, 209.787, 9.91), "{reviewer_date}");
        assert_eq!(content_at(frame, 284.657, 10.6), "{approver_date}");
        assert_eq!(
            content_at(frame, 379.813, 1.821),
            "{paper} Border version: 2.0    {plot_date}"
        );
        // The file name prints twice: up the left margin and in its cell.
        assert_eq!(frame.texts_showing("file_name").len(), 2);
        // No raw token survives the parse.
        for text in &frame.texts {
            assert!(!text.content.contains('$'), "{}", text.content);
        }
    }

    #[test]
    fn the_slots_sit_where_the_removed_branding_was() {
        let frame = a3();
        assert_eq!(frame.texts_showing("organisation").len(), 1);
        let organisation = &frame.texts[frame.texts_showing("organisation")[0]];
        // Centred in the organisation cell: (297.117 + 401.843) / 2 = 349.480,
        // (28.865 + 34.625) / 2 = 31.745; the removed header's cap 3.738.
        near(organisation.anchor.x, 349.48, 1e-9);
        near(organisation.anchor.y, 31.745, 1e-9);
        assert_eq!(organisation.cap_height_mm, 3.738);
        assert_eq!(organisation.role, FrameRole::Slot);
        let notes = &frame.texts[frame.texts_showing("notes")[0]];
        // The removed notes' first line at 220.918, 32.961, cap 0.85, and a
        // line pitch of 1.5 x 0.85 = 1.275.
        assert_eq!(notes.anchor.x, 220.918);
        assert_eq!(notes.anchor.y, 32.961);
        near(notes.line_spacing_mm, 1.275, 1e-12);
    }

    #[test]
    fn the_construction_border_is_a_guide_that_does_not_plot() {
        // Two construction rectangles (entities 39 and 41) at the paper edge,
        // in orange (#ff8000) at 0.18 mm; every other line plots.
        let frame = a3();
        let mut guides = 0;
        for line in &frame.polylines {
            if line.role == FrameRole::Construction {
                guides += 1;
                assert!(!line.plots);
                assert_eq!(line.colour.to_hex(), "#FF8000");
            } else {
                assert!(line.plots);
            }
        }
        assert_eq!(guides, 2);
    }

    #[test]
    fn the_dashed_underlines_are_two_point_two_on_one_point_one() {
        // 12 field underlines, "DGN Style 1": dash 2.2 mm, gap 1.1 mm, 0.53 mm.
        let frame = a3();
        let mut dashed = 0;
        for line in &frame.polylines {
            if line.role == FrameRole::Underline {
                dashed += 1;
                assert_eq!(line.dash_mm, 2.2);
                assert_eq!(line.gap_mm, 1.1);
                assert_eq!(line.weight_mm, 0.53);
            }
        }
        assert_eq!(dashed, 12);
    }

    #[test]
    fn a_closed_ring_does_not_repeat_its_first_point() {
        // The border (#0) is five points in the data, the last the first again.
        let frame = a3();
        let border = frame.polylines.first().expect("a border");
        assert!(border.closed);
        assert_eq!(border.points.len(), 4);
    }

    #[test]
    fn other_a_sizes_scale_the_a3_frame_uniformly() {
        // A1: min(841 / 420, 594 / 297) = min(2.00238, 2) = 2 exactly.
        assert_eq!(frame_scale_for(PaperSize::A1, true), 2.0);
        // A4 landscape: min(297 / 420, 210 / 297) = 210 / 297 = 0.70707...
        assert_eq!(frame_scale_for(PaperSize::A4, true), 210.0 / 297.0);
        // A0: min(1189 / 420 = 2.83095, 841 / 297 = 2.83165) = 1189 / 420.
        assert_eq!(frame_scale_for(PaperSize::A0, true), 1189.0 / 420.0);
        // A portrait sheet has no frame.
        assert_eq!(frame_scale_for(PaperSize::A3, false), 0.0);

        let a1 = frame_for(BUILT_IN_FRAME_ID, PaperSize::A1, true)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(a1.width_mm, 841.0);
        assert_eq!(a1.height_mm, 594.0);
        // Everything doubles: the drawing area 46..820 x 70..574, the logo
        // cell 334.530..440.458 x 45.25..69.25, the 0.53 mm border 1.06 mm,
        // "SHEET No." at 807.804, 41.036 with a 2 mm cap.
        expect_box(&a1.drawing_area, 46.0, 70.0, 820.0, 574.0);
        expect_box(
            &a1.cell("logo").expect("a logo cell").rect,
            334.53,
            45.25,
            440.458,
            69.25,
        );
        near(a1.polylines[0].weight_mm, 1.06, 1e-12);
        let sheet_no = text_at(&a1, 807.804, 41.036).expect("SHEET No.");
        assert_eq!(sheet_no.cap_height_mm, 2.0);
        // The millimetre of slack A1 has over twice A3 (841 against 840) goes
        // to the right margin: 841 - 820 = 21; the top is 594 - 574 = 20.
        near(a1.margins.right, 21.0, 1e-9);
        near(a1.margins.top, 20.0, 1e-9);
        assert_eq!(a1.margins.left, 46.0);
        // The construction guide follows the paper's edge, not the scaled
        // frame's.
        for line in &a1.polylines {
            if line.role == FrameRole::Construction {
                let mut right: f64 = 0.0;
                for point in &line.points {
                    if right < point.x {
                        right = point.x;
                    }
                }
                assert!(right >= 841.0, "{right}");
            }
        }
    }

    #[test]
    fn a_portrait_sheet_or_an_unknown_frame_is_refused() {
        assert_eq!(
            frame_for(BUILT_IN_FRAME_ID, PaperSize::A3, false)
                .expect_err("a portrait sheet has no frame")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            frame_for("no such frame", PaperSize::A3, true)
                .expect_err("no such frame")
                .code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn a_logo_is_fitted_into_its_cell_with_its_aspect_kept() {
        // The logo cell less 1 mm all round is 50.964 x 10 mm. A 4:1 image is
        // limited by the height: 40 x 10 mm, centred on the cell's centre
        // (193.747, 28.625).
        let cell = a3().cell("logo").expect("a logo cell").rect;
        let wide = fit_image(&cell, 400.0, 100.0, 1.0);
        expect_box(&wide, 173.747, 23.625, 213.747, 33.625);
        // A 10:1 image is limited by the width: 50.964 x 5.0964 mm.
        let banner = fit_image(&cell, 1000.0, 100.0, 1.0);
        near(banner.width(), 50.964, 1e-9);
        near(banner.height(), 5.0964, 1e-9);
        // An image with no size has nowhere to go.
        assert!(fit_image(&cell, 0.0, 100.0, 1.0).empty());
    }

    #[test]
    fn a_corrupt_frame_is_a_parse_failure_not_a_crash() {
        assert_eq!(
            parse_frame("not json").expect_err("not JSON").code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            parse_frame("{}").expect_err("no members").code,
            ErrorCode::ParseFailure
        );
    }
}

mod sheet_scales {
    use super::*;

    #[test]
    fn the_sheet_ladder_is_finer_than_the_plot_ladder_and_keeps_its_steps() {
        // A section needs 1:10 to 1:75 and the steps between the plot ladder's.
        assert_eq!(SHEET_SCALES[0], 1.0);
        assert_eq!(SHEET_SCALES[SHEET_SCALES.len() - 1], 50000.0);
        assert!(SHEET_SCALES.is_sorted());
        // Every step of the single-page plot's ladder is on the sheet ladder too.
        for scale in STANDARD_SCALES {
            assert!(SHEET_SCALES.contains(&scale), "{scale}");
        }
        let at_least = |needed: f64| {
            sheet_scale_at_least(needed).unwrap_or_else(|error| panic!("{}", error.describe()))
        };
        assert_eq!(at_least(60.0), 75.0);
        assert_eq!(at_least(501.3), 750.0);
        assert_eq!(at_least(1100.0), 1250.0);
        assert_eq!(at_least(500.0), 500.0); // exactly fits
        assert_eq!(at_least(60000.0), 60000.0);
        assert_eq!(
            sheet_scale_at_least(0.0)
                .expect_err("no scale denominator")
                .code,
            ErrorCode::InvalidArgument
        );
    }
}
