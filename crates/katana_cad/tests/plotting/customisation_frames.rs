// The frames a customisation offers beside the default one (docs/plotting.md,
// "A customisation's title frames"): the NSW built-in's frame compiled in from
// resources/plot_frames/nsw_a3_utility_cross_section.json, which frames are on
// offer, and the default standing in, said, for a frame that is not.
//
// The counts are the owner's title block file's, counted in it without this
// program: 99 `string super` blocks of which 14 carry `symbol_data` (85 lines,
// 14 symbols), 80 `string text` blocks, 26 `solid_fill` blocks; its info block
// gives the A3 Landscape sheet and the margins 23, 10, 10, 35. A text's
// position is the file's x + offset and y + raise, added by hand. The texts
// of the branding are compared with the committed file's own, so that no
// organisation's name is written in this test (CLAUDE.md section 9).

use crate::near;
use katana_cad::PaperSize;
use katana_cad::plotting::frame::{
    BUILT_IN_FRAME_ID, DEFAULT_USER_SLOTS, built_in_frame, compiled_in_frame,
    compiled_in_frame_names, frame_for, frames_not_compiled_in, frames_offered, parse_frame,
    resolve_sheet_frame,
};
use katana_cad::plotting::{Frame, FrameRole, FrameText, HorizontalJustify, VerticalJustify};
use katana_core::ErrorCode;
use katana_core::json::Json;
use katana_entity::{Color, CustomisationPlotFrame};

const CUSTOMISATION_FRAME: &str = "nsw_a3_utility_cross_section";
static CUSTOMISATION_FRAME_JSON: &str =
    include_str!("../../../../resources/plot_frames/nsw_a3_utility_cross_section.json");

fn the_customisation_frame() -> &'static Frame {
    match compiled_in_frame(CUSTOMISATION_FRAME) {
        Some(Ok(frame)) => frame,
        Some(Err(error)) => panic!("{}", error.describe()),
        None => panic!("{CUSTOMISATION_FRAME} is not compiled in"),
    }
}

fn the_default_frame() -> &'static Frame {
    match built_in_frame() {
        Ok(frame) => frame,
        Err(error) => panic!("{}", error.describe()),
    }
}

fn offered(names: &[&str]) -> Vec<CustomisationPlotFrame> {
    names
        .iter()
        .map(|name| CustomisationPlotFrame {
            name: String::from(*name),
        })
        .collect()
}

// The raw text of the committed file's entity `index`: what the frame must
// carry, as its owner wrote it.
fn committed_text(index: usize) -> String {
    let root = Json::parse(CUSTOMISATION_FRAME_JSON)
        .unwrap_or_else(|error| panic!("the committed frame is JSON: {}", error.what));
    let entities = root.at("entities").expect("entities");
    for entity in entities.iter() {
        if entity.at("index").and_then(|at| at.get::<usize>()).ok() == Some(index) {
            return entity
                .at("raw_text")
                .and_then(|text| text.get::<String>())
                .unwrap_or_else(|_| panic!("entity {index} is a text"));
        }
    }
    panic!("no entity {index}");
}

fn text_near(frame: &Frame, x: f64, y: f64) -> Option<&FrameText> {
    frame
        .texts
        .iter()
        .find(|text| (text.anchor.x - x).abs() < 1e-9 && (text.anchor.y - y).abs() < 1e-9)
}

mod customisation_frames {
    use super::*;

    #[test]
    fn the_customisation_frame_is_compiled_in_beside_the_default() {
        assert_eq!(
            compiled_in_frame_names(),
            vec![BUILT_IN_FRAME_ID, CUSTOMISATION_FRAME]
        );
        assert_eq!(the_customisation_frame().id, CUSTOMISATION_FRAME);
        assert_eq!(the_default_frame().id, BUILT_IN_FRAME_ID);
        assert!(compiled_in_frame("no_such_frame").is_none());
    }

    #[test]
    fn the_customisation_frame_has_every_line_text_and_symbol_of_its_title_block_file() {
        let frame = the_customisation_frame();
        assert_eq!(frame.polylines.len(), 85);
        // No slot texts are added: the frame prints its own organisation and
        // notes.
        assert_eq!(frame.texts.len(), 80);
        assert_eq!(frame.symbols.len(), 14);
        assert_eq!(
            frame.polylines.iter().filter(|line| line.fill.is_some()).count(),
            26
        );
    }

    #[test]
    fn the_customisation_frame_is_a3_landscape_with_the_files_margins() {
        let frame = the_customisation_frame();
        assert_eq!(frame.width_mm, 420.0);
        assert_eq!(frame.height_mm, 297.0);
        assert_eq!(frame.margins.left, 23.0);
        assert_eq!(frame.margins.right, 10.0);
        assert_eq!(frame.margins.top, 10.0);
        assert_eq!(frame.margins.bottom, 35.0);
        // The viewport: the paper inside the margins, 23..410 x 35..287.
        crate::expect_box(&frame.drawing_area, 23.0, 35.0, 410.0, 287.0);
        // The border and the title block are the file's first two closed
        // rectangles, as the default frame's are.
        crate::expect_box(&frame.border, 21.824523, 34.625155, 411.309003, 289.265155);
        crate::expect_box(&frame.title_block, 21.824523, 8.945155, 411.309003, 34.625155);
    }

    #[test]
    fn a_frame_that_draws_its_own_branding_has_no_user_slots() {
        let frame = the_customisation_frame();
        assert!(frame.user_slots.is_empty());
        assert!(!frame.has_user_slot("logo"));
        assert!(frame.texts.iter().all(|text| text.role != FrameRole::Slot));
        // The default frame has all three, as it always had.
        let default = the_default_frame();
        assert_eq!(default.user_slots, DEFAULT_USER_SLOTS.map(String::from).to_vec());
        assert_eq!(
            default
                .texts
                .iter()
                .filter(|text| text.role == FrameRole::Slot)
                .count(),
            2
        );
    }

    #[test]
    fn the_heading_is_printed_in_the_organisation_cell_as_the_owner_wrote_it() {
        // Entity 37: x 310.28262832, y 29.99630564, no offset or raise; the
        // capital 3.73764973 mm, condensed 0.82, bottom-left.
        let frame = the_customisation_frame();
        let heading = text_near(frame, 310.28262832, 29.99630564).expect("the heading");
        assert_eq!(heading.content, committed_text(37));
        assert!(!heading.content.is_empty());
        near(heading.cap_height_mm, 3.73764973, 1e-12);
        near(heading.x_factor, 0.82, 1e-12);
        assert_eq!(heading.horizontal, HorizontalJustify::Left);
        assert_eq!(heading.vertical, VerticalJustify::Bottom);
        assert!(heading.fields.is_empty());
        let cell = frame.cell("organisation").expect("the organisation cell");
        assert!(cell.rect.contains(heading.anchor));
        // The default frame prints nothing of its own there.
        assert!(text_near(the_default_frame(), 310.28262832, 29.99630564).is_none());
    }

    #[test]
    fn the_logo_text_is_printed_beside_its_filled_mark() {
        // Entities 146 and 147: x 181.81756054, y 31.8613632 and 28.23712081,
        // the capital 2.73 mm, middle-left; the first bold (ttf_weight 800).
        let frame = the_customisation_frame();
        let first = text_near(frame, 181.81756054, 31.8613632).expect("the logo's first line");
        let second = text_near(frame, 181.81756054, 28.23712081).expect("the logo's second line");
        assert_eq!(first.content, committed_text(146));
        assert_eq!(second.content, committed_text(147));
        assert!(first.bold);
        assert!(!second.bold);
        near(first.cap_height_mm, 2.73, 1e-12);
        assert_eq!(first.horizontal, HorizontalJustify::Left);
        assert_eq!(first.vertical, VerticalJustify::Middle);
        // Its mark is drawn in filled red shapes: "acad 010", which the
        // measured palette of the default frame plots #FF0000.
        let red = Color::new(255, 0, 0, 255);
        let fills: Vec<_> = frame.polylines.iter().filter_map(|line| line.fill).collect();
        assert!(fills.iter().all(|fill| *fill == red), "{fills:?}");
        let logo = frame.cell("logo").expect("the logo cell").rect;
        assert!(
            frame
                .polylines
                .iter()
                .filter(|line| line.fill.is_some())
                .all(|line| line.points.iter().all(|point| logo.contains(*point)))
        );
    }

    #[test]
    fn the_disclaimer_is_printed_from_its_offset_anchor_in_a_narrow_face() {
        // Entity 160: x 229.29435856 - 8.37632763 = 220.91803093 and
        // y 28.14999227 + 4.8114272 = 32.96141947; the capital 0.85 mm, the
        // face "Arial Narrow" at full width.
        let frame = the_customisation_frame();
        let disclaimer =
            text_near(frame, 220.91803093, 32.96141947).expect("the disclaimer");
        assert_eq!(disclaimer.content, committed_text(160));
        assert_eq!(disclaimer.font_face, "Arial Narrow");
        near(disclaimer.cap_height_mm, 0.85, 1e-12);
        near(disclaimer.x_factor, 1.0, 1e-12);
        near(disclaimer.line_spacing_mm, 1.5 * 0.85, 1e-12);
        assert!(disclaimer.content.contains('\n'));
    }

    // The default frame was measured from the same file with the branding
    // taken out and its numbers rounded to the thousandth: every one of its
    // lines and texts is in the customisation's frame, within that rounding.
    #[test]
    fn the_customisation_frame_is_the_default_frame_with_the_branding_present() {
        let full = the_customisation_frame();
        let close = |a: f64, b: f64| (a - b).abs() <= 0.0006;
        for line in &the_default_frame().polylines {
            assert!(
                full.polylines.iter().any(|other| {
                    other.points.len() == line.points.len()
                        && other.closed == line.closed
                        && other.role == line.role
                        && other.colour == line.colour
                        && close(other.weight_mm, line.weight_mm)
                        && other
                            .points
                            .iter()
                            .zip(&line.points)
                            .all(|(p, q)| close(p.x, q.x) && close(p.y, q.y))
                }),
                "{line:?}"
            );
        }
        for text in the_default_frame()
            .texts
            .iter()
            .filter(|text| text.role != FrameRole::Slot)
        {
            assert!(
                full.texts.iter().any(|other| {
                    other.content == text.content
                        && other.role == text.role
                        && close(other.anchor.x, text.anchor.x)
                        && close(other.anchor.y, text.anchor.y)
                        && close(other.cap_height_mm, text.cap_height_mm)
                }),
                "{text:?}"
            );
        }
        // And its fields are the same fields: a sheet fills it as it fills the
        // default.
        for field in ["sheet_number", "sheet_count", "scale", "approver_date", "plot_date"] {
            assert_eq!(
                full.texts_showing(field).len(),
                the_default_frame().texts_showing(field).len(),
                "{field}"
            );
        }
    }

    #[test]
    fn the_customisation_frame_scales_to_a1_as_the_default_does() {
        // A1 landscape is 841 x 594: the factor is min(841 / 420, 594 / 297) = 2.
        let a1 = frame_for(CUSTOMISATION_FRAME, PaperSize::A1, true)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        near(a1.scale, 2.0, 1e-12);
        let heading = text_near(&a1, 2.0 * 310.28262832, 2.0 * 29.99630564)
            .map(|text| text.cap_height_mm);
        near(heading.expect("the heading, doubled"), 2.0 * 3.73764973, 1e-9);
        assert_eq!(a1.polylines.iter().filter(|line| line.fill.is_some()).count(), 26);
    }

    #[test]
    fn the_frames_on_offer_are_the_default_then_the_customisations_compiled_in_ones() {
        assert_eq!(frames_offered(&[]), vec![String::from(BUILT_IN_FRAME_ID)]);
        assert_eq!(
            frames_offered(&offered(&[CUSTOMISATION_FRAME, "no_such_frame", CUSTOMISATION_FRAME])),
            vec![
                String::from(BUILT_IN_FRAME_ID),
                String::from(CUSTOMISATION_FRAME)
            ]
        );
        // A customisation offering the default names nothing new.
        assert_eq!(
            frames_offered(&offered(&[BUILT_IN_FRAME_ID])),
            vec![String::from(BUILT_IN_FRAME_ID)]
        );
        assert_eq!(
            frames_not_compiled_in(&offered(&[CUSTOMISATION_FRAME, "no_such_frame"])),
            vec![String::from("no_such_frame")]
        );
        assert!(frames_not_compiled_in(&[]).is_empty());
    }

    #[test]
    fn a_frame_on_offer_is_drawn_as_itself() {
        let nsw = offered(&[CUSTOMISATION_FRAME]);
        let drawn = resolve_sheet_frame(CUSTOMISATION_FRAME, Some(nsw.as_slice()), PaperSize::A3, true)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(drawn.frame.id, CUSTOMISATION_FRAME);
        assert!(drawn.fallback.is_empty());
        // When what is offered is not known, a compiled-in frame is drawn.
        let unknown = resolve_sheet_frame(CUSTOMISATION_FRAME, None, PaperSize::A3, true)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(unknown.frame.id, CUSTOMISATION_FRAME);
        assert!(unknown.fallback.is_empty());
        // The default is always itself, whatever is offered.
        let default = resolve_sheet_frame(BUILT_IN_FRAME_ID, Some(&[][..]), PaperSize::A3, true)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(default.frame.id, BUILT_IN_FRAME_ID);
        assert!(default.fallback.is_empty());
    }

    #[test]
    fn a_frame_the_customisation_does_not_offer_falls_back_to_the_default_and_says_so() {
        let drawn = resolve_sheet_frame(CUSTOMISATION_FRAME, Some(&[][..]), PaperSize::A3, true)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(drawn.frame.id, BUILT_IN_FRAME_ID);
        assert_eq!(drawn.frame, *the_default_frame());
        assert!(drawn.fallback.contains(CUSTOMISATION_FRAME), "{}", drawn.fallback);
        assert!(drawn.fallback.contains("not one the customisation"), "{}", drawn.fallback);
        assert!(drawn.fallback.contains(BUILT_IN_FRAME_ID), "{}", drawn.fallback);

        // A name Katana has not got at all, even one a customisation offers.
        let missing = offered(&["no_such_frame"]);
        let absent = resolve_sheet_frame("no_such_frame", Some(missing.as_slice()), PaperSize::A1, true)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(absent.frame.id, BUILT_IN_FRAME_ID);
        near(absent.frame.scale, 2.0, 1e-12);
        assert!(absent.fallback.contains("no plot frame no_such_frame"), "{}", absent.fallback);
    }

    #[test]
    fn a_portrait_sheet_has_no_frame_whatever_it_names() {
        for name in [BUILT_IN_FRAME_ID, CUSTOMISATION_FRAME, "no_such_frame"] {
            assert_eq!(
                resolve_sheet_frame(name, None, PaperSize::A3, false)
                    .expect_err("a portrait sheet has no frame")
                    .code,
                ErrorCode::InvalidArgument,
                "{name}"
            );
        }
    }

    // A frame description may say which user slots it has; one that says none
    // gets no slot texts, and a slot that is not one is refused.
    #[test]
    fn a_frame_description_lists_only_the_user_slots_it_has() {
        let described = |slots: &str| {
            format!(
                r#"{{"user_slots": {slots}, "sheet": {{"width_mm": 420, "height_mm": 297,
                    "margins_mm": {{"left": 23, "right": 10, "top": 10, "bottom": 35}},
                    "viewport_tbf": {{"x0": 23, "y0": 35, "x1": 410, "y1": 287}},
                    "drawing_border_tbf": {{"x0": 21, "y0": 34, "x1": 411, "y1": 289}},
                    "title_block_tbf": {{"x0": 21, "y0": 9, "x1": 411, "y1": 34}},
                    "sheet_edge_tbf": {{"x0": 0, "y0": 0, "x1": 420, "y1": 297}}}},
                    "cells": [{{"id": "organisation", "tbf": [297, 28, 401, 34]}},
                              {{"id": "disclaimer", "tbf": [220, 12, 297, 34]}}],
                    "entities": []}}"#
            )
        };
        let none = parse_frame(&described("[]"))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(none.user_slots.is_empty());
        assert!(none.texts.is_empty());

        let organisation = parse_frame(&described(r#"["organisation"]"#))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(organisation.texts.len(), 1);
        assert_eq!(organisation.texts[0].fields, vec![String::from("organisation")]);

        let refused = parse_frame(&described(r#"["signature"]"#)).expect_err("not a slot");
        assert_eq!(refused.code, ErrorCode::ParseFailure);
    }

    // A line of the description may be filled; its fill is read with it, and
    // a fill that is not a colour is refused.
    #[test]
    fn a_filled_line_keeps_its_fill_and_a_line_without_one_has_none() {
        let described = |fill: &str| {
            format!(
                r##"{{"user_slots": [], "sheet": {{"width_mm": 420, "height_mm": 297,
                    "margins_mm": {{"left": 23, "right": 10, "top": 10, "bottom": 35}},
                    "viewport_tbf": {{"x0": 23, "y0": 35, "x1": 410, "y1": 287}},
                    "drawing_border_tbf": {{"x0": 21, "y0": 34, "x1": 411, "y1": 289}},
                    "title_block_tbf": {{"x0": 21, "y0": 9, "x1": 411, "y1": 34}},
                    "sheet_edge_tbf": {{"x0": 0, "y0": 0, "x1": 420, "y1": 297}}}},
                    "cells": [],
                    "entities": [
                      {{"kind": "poly", "role": "border", "closed": false, "weight_mm": 0.18,
                        "colour_hex": "#ff0000", "dash_mm": null{fill},
                        "points_tbf": [[1, 1], [2, 1], [1.5, 2], [1, 1]]}},
                      {{"kind": "poly", "role": "border", "closed": true, "weight_mm": 0.5,
                        "colour_hex": "#000000", "dash_mm": null,
                        "points_tbf": [[0, 0], [10, 0], [10, 10]]}}]}}"##
            )
        };
        let frame = parse_frame(&described(r##", "fill_hex": "#112233""##))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(frame.polylines[0].fill, Some(Color::new(0x11, 0x22, 0x33, 255)));
        assert!(!frame.polylines[0].closed);
        assert_eq!(frame.polylines[1].fill, None);

        assert!(parse_frame(&described(r#", "fill_hex": "red""#)).is_err());
    }
}
