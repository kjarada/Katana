//! The shared controls' look (drive::controls, the look of 2026-11-16): a one-line field's text
//! centred in its height and set in as a choice's is, an editable choice one framed control with
//! its list's arrow inside it, a checked box and a chosen radio in the accent, and a tree's
//! expander egui's triangle rather than a glyph the window's fonts lack.
//!
//! Read from what egui was asked to paint - the shapes, every renderer's input - in the window's
//! theme (theme::style::apply); the harness's own style is egui's.

use std::cell::Cell;

use egui::epaint::{ClippedShape, ColorMode};
use egui::{Modifiers, Order, PointerButton, Pos2, Rect, Shape, Ui};
use katana_window::drive::{self, Choice, Harness, LineOptions, SpinRange};
use katana_window::theme::palette;
use katana_window::theme::style::apply;

/// Every shape of `shapes`, nested ones too, as painted.
pub(crate) fn flat(shapes: &[ClippedShape]) -> Vec<Shape> {
    fn add(shape: &Shape, into: &mut Vec<Shape>) {
        match shape {
            Shape::Vec(inner) => {
                for each in inner {
                    add(each, into);
                }
            }
            other => into.push(other.clone()),
        }
    }
    let mut all = Vec::new();
    for clipped in shapes {
        add(&clipped.shape, &mut all);
    }
    all
}

/// The shapes drawn inside `area`, give or take half a point.
pub(crate) fn inside(shapes: &[Shape], area: Rect) -> Vec<Shape> {
    let room = area.expand(0.5);
    shapes
        .iter()
        .filter(|shape| room.contains_rect(shape.visual_bounding_rect()))
        .cloned()
        .collect()
}

/// The layout box of the first line of text drawn inside `area`: where its row of text sits,
/// whatever its glyphs.
fn text_row(shapes: &[Shape], area: Rect) -> Rect {
    inside(shapes, area)
        .iter()
        .find_map(|shape| match shape {
            Shape::Text(text) if !text.galley.is_empty() => {
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text drawn in {area:?}"))
}

/// Within half a point: egui rounds a widget's rectangle to the pixel grid.
const HALF_A_POINT: f32 = 0.5;

mod control_look {
    use super::*;

    #[test]
    fn a_fields_text_is_centred_in_its_height_and_set_in_as_a_choices_is() {
        // The audit of 2026-11-16 (T4): a field's text sat 4 to 5 px above its middle and 4 in
        // from its edge, beside a choice's centred 8 in, in every form.
        let mut harness = Harness::new();
        apply(harness.context());
        let mut value = String::from("Ag 120");
        let items = vec![String::from("Ag 120")];
        let mut chosen = Choice::at(0);
        let mut draw = |ui: &mut Ui| {
            ui.spacing_mut().text_edit_width = 200.0;
            ui.spacing_mut().combo_width = 200.0;
            drive::line_with(ui, "field", &mut value, LineOptions::default());
            drive::choice(ui, "list", &items, &mut chosen);
        };
        harness.show(&mut draw).expect("drawn");
        let output = harness.frame(&mut draw);
        let shapes = flat(&output.shapes);
        output.drop_without_applying_deltas();
        let field = harness.control("field").expect("the field").rect;
        let list = harness.control("list").expect("the choice").rect;
        let field_text = text_row(&shapes, field);
        let list_text = text_row(&shapes, list);
        // The premise: both are a control high.
        assert!((field.height() - list.height()).abs() <= HALF_A_POINT, "{field:?} {list:?}");
        assert!(
            (field_text.center().y - field.center().y).abs() <= 1.0,
            "the field's text {field_text:?} is not centred in {field:?}"
        );
        assert!(
            ((field_text.left() - field.left()) - (list_text.left() - list.left())).abs() <= 1.0,
            "the field's text starts {} in, the choice's {}",
            field_text.left() - field.left(),
            list_text.left() - list.left()
        );
    }

    #[test]
    fn a_number_box_is_a_fields_well_with_its_text_where_a_fields_is() {
        // The second look audit (N4): egui draws a DragValue as a button, the raised grey of a
        // key with its text centred across it, so every number box of every form (Layers' Line
        // weight, Contours' Major every, Text Styles') read as a button beside the fields round
        // it. A number box is a field's well, a control high, its text centred in that height
        // and set in as a field's is - a decimal box and a whole one alike.
        let mut harness = Harness::new();
        apply(harness.context());
        let mut value = String::from("2.50 mm");
        let mut size = 2.5;
        let mut count: i64 = 5;
        let mut draw = |ui: &mut Ui| {
            ui.spacing_mut().text_edit_width = 200.0;
            drive::line_with(ui, "field", &mut value, LineOptions::default());
            drive::spin(
                ui,
                "decimal",
                &mut size,
                SpinRange::new(0.0, 100.0, 0.1, 2).with_suffix(" mm"),
            );
            drive::whole(ui, "count", &mut count, 0..=99);
        };
        harness.show(&mut draw).expect("drawn");
        let output = harness.frame(&mut draw);
        let shapes = flat(&output.shapes);
        output.drop_without_applying_deltas();
        let field = harness.control("field").expect("the field").rect;
        let field_text = text_row(&shapes, field);
        let input = palette::input().to_egui();
        let raised = palette::raised().to_egui();
        for name in ["decimal", "count"] {
            let control = harness.control(name).expect("the number box").rect;
            assert!(
                (control.height() - field.height()).abs() <= HALF_A_POINT,
                "{name} is {} high beside a field {} high",
                control.height(),
                field.height()
            );
            let drawn = inside(&shapes, control);
            let wells: Vec<Rect> = drawn
                .iter()
                .filter_map(|shape| match shape {
                    Shape::Rect(rect) if rect.fill == input => Some(rect.rect),
                    _ => None,
                })
                .collect();
            assert_eq!(wells.len(), 1, "{name}: one well: {wells:?}");
            assert!(
                (wells[0].width() - control.width()).abs() <= HALF_A_POINT,
                "{name}: the well {:?} spans the box {control:?}",
                wells[0]
            );
            assert!(
                !drawn
                    .iter()
                    .any(|shape| matches!(shape, Shape::Rect(rect) if rect.fill == raised)),
                "{name}: not a button's raised grey"
            );
            let text = text_row(&shapes, control);
            assert!(
                (text.center().y - control.center().y).abs() <= 1.0,
                "{name}: the text {text:?} is not centred in {control:?}"
            );
            assert!(
                ((text.left() - control.left()) - (field_text.left() - field.left())).abs() <= 1.0,
                "{name}: the text starts {} in, a field's {}",
                text.left() - control.left(),
                field_text.left() - field.left()
            );
        }
        // What it holds and reports is as it was.
        assert_eq!(harness.control("decimal").expect("drawn").value, "2.50");
        assert_eq!(harness.control("count").expect("drawn").value, "5");
    }

    #[test]
    fn an_editable_choice_is_one_framed_control_with_its_arrow_inside_its_end() {
        // It was a field and a separate 38 px ComboBox button beside it (T4): two frames, two
        // controls to the eye. Now one frame, a field's well, round the whole of what the
        // control records, as wide as a field alone; a click inside its right end opens the
        // list under it, as wide as it.
        let mut harness = Harness::new();
        apply(harness.context());
        let items = vec![String::from("alpha"), String::from("beta")];
        let mut chosen = Choice::editable("alpha");
        let mut draw = |ui: &mut Ui| {
            ui.spacing_mut().text_edit_width = 200.0;
            drive::choice(ui, "editable", &items, &mut chosen);
        };
        harness.show(&mut draw).expect("drawn");
        let output = harness.frame(&mut draw);
        let shapes = flat(&output.shapes);
        output.drop_without_applying_deltas();
        let control = harness.control("editable").expect("the choice").rect;
        assert!((control.width() - 200.0).abs() <= HALF_A_POINT, "{control:?}");
        let input = palette::input().to_egui();
        let frames: Vec<Rect> = inside(&shapes, control)
            .iter()
            .filter_map(|shape| match shape {
                Shape::Rect(rect) if rect.fill == input => Some(rect.rect),
                _ => None,
            })
            .collect();
        assert_eq!(frames.len(), 1, "one well, not two: {frames:?}");
        assert!(
            (frames[0].left() - control.left()).abs() <= HALF_A_POINT
                && (frames[0].right() - control.right()).abs() <= HALF_A_POINT,
            "the well {:?} spans the control {control:?}",
            frames[0]
        );
        let raised = palette::raised().to_egui();
        assert!(
            !inside(&shapes, control)
                .iter()
                .any(|shape| matches!(shape, Shape::Rect(rect) if rect.fill == raised)),
            "no button of its own beside the field"
        );

        // The arrow's end: a click there opens the list under the control.
        let arrow = Pos2::new(control.right() - 6.0, control.center().y);
        harness
            .pointer_move(arrow)
            .pointer_button(arrow, PointerButton::Primary, true, Modifiers::NONE);
        harness.frame(&mut draw).drop_without_applying_deltas();
        harness.pointer_button(arrow, PointerButton::Primary, false, Modifiers::NONE);
        harness.frame(&mut draw).drop_without_applying_deltas();
        harness.run(&mut draw).expect("the open list settles");
        let ctx = harness.context().clone();
        let list = ctx
            .memory(|memory| {
                memory
                    .areas()
                    .top_layer_id(Order::Foreground)
                    .and_then(|layer| memory.area_rect(layer.id))
            })
            .expect("the list is open");
        assert!(list.top() >= control.bottom() - HALF_A_POINT, "{list:?} under {control:?}");
        assert!(list.width() >= control.width() - 1.0, "{list:?} as wide as {control:?}");
        // What it holds and what a fill does are as they were.
        assert_eq!(harness.control("editable").expect("the choice").items, items);
    }

    #[test]
    fn a_checked_box_and_a_chosen_radio_are_drawn_in_the_accent_and_their_opposites_are_not() {
        // T17: egui drew a checked box the raised grey of an unchecked one with a hairline tick,
        // and a chosen radio a 5 px dot in a grey disc: on read as off at a glance.
        let mut harness = Harness::new();
        apply(harness.context());
        let mut on = true;
        let mut off = false;
        let mut draw = |ui: &mut Ui| {
            drive::check(ui, "on", "On", &mut on);
            drive::check(ui, "off", "Off", &mut off);
            drive::radio(ui, "chosen", "Chosen", true);
            drive::radio(ui, "other", "Other", false);
        };
        harness.show(&mut draw).expect("drawn");
        let output = harness.frame(&mut draw);
        let shapes = flat(&output.shapes);
        output.drop_without_applying_deltas();
        let accent = palette::accent().to_egui();
        let on_accent = palette::accent_text().to_egui();
        let accent_box = |name: &str| {
            let rect = harness.control(name).expect("drawn").rect;
            inside(&shapes, rect)
                .iter()
                .any(|shape| matches!(shape, Shape::Rect(rect) if rect.fill == accent))
        };
        assert!(accent_box("on"), "a checked box is filled in the accent");
        assert!(!accent_box("off"), "an unchecked one is not");
        let check = harness.control("on").expect("drawn").rect;
        let tick = inside(&shapes, check).into_iter().find_map(|shape| match shape {
            Shape::Path(path) if path.stroke.color == ColorMode::Solid(on_accent) => {
                Some(path.stroke.width)
            }
            _ => None,
        });
        assert!(
            tick.is_some_and(|width| width >= 2.0),
            "the tick is drawn on the accent, two pixels wide: {tick:?}"
        );
        let discs = |name: &str| -> Vec<(egui::Color32, f32)> {
            let rect = harness.control(name).expect("drawn").rect;
            inside(&shapes, rect)
                .iter()
                .filter_map(|shape| match shape {
                    Shape::Circle(circle) => Some((circle.fill, circle.radius)),
                    _ => None,
                })
                .collect()
        };
        let chosen = discs("chosen");
        assert!(chosen.iter().any(|(fill, _)| *fill == accent), "{chosen:?}");
        assert!(
            chosen
                .iter()
                .any(|(fill, radius)| *fill == on_accent && *radius >= 3.5),
            "a dot 7 px across at least: {chosen:?}"
        );
        let other = discs("other");
        assert!(!other.iter().any(|(fill, _)| *fill == accent), "{other:?}");
    }

    /// The colour `text` is painted in: its own, or the fallback a placeholder colour takes.
    fn ink(text: &egui::epaint::TextShape) -> egui::Color32 {
        let own = text
            .galley
            .job
            .sections
            .first()
            .map_or(egui::Color32::PLACEHOLDER, |section| section.format.color);
        if own == egui::Color32::PLACEHOLDER {
            text.fallback_color
        } else {
            own
        }
    }

    /// The text shapes painted inside `area`: their words and their colour.
    fn words_in(shapes: &[Shape], area: Rect) -> Vec<(String, egui::Color32)> {
        inside(shapes, area)
            .iter()
            .filter_map(|shape| match shape {
                Shape::Text(text) if !text.galley.is_empty() => {
                    Some((String::from(text.galley.text()), ink(text)))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_empty_choice_says_what_is_missing_muted_while_its_value_reads_empty() {
        // The third look audit (I): an empty choice was an empty well (Thin's Surface, Global
        // Modify's Layer and Style, Properties' Style with nothing selected). It says NO_CHOICE,
        // or the caller's words, muted - and records "", as the tests and drivers read it.
        let mut harness = Harness::new();
        apply(harness.context());
        let items = vec![String::from("alpha"), String::from("beta")];
        let none: Vec<String> = Vec::new();
        let mut empty = Choice::default();
        let mut surface = Choice::default();
        let mut typed = Choice::editable("");
        let mut chosen = Choice::at(0);
        let mut draw = |ui: &mut Ui| {
            ui.spacing_mut().text_edit_width = 200.0;
            ui.spacing_mut().combo_width = 200.0;
            drive::choice(ui, "empty", &none, &mut empty);
            drive::controls::choice_with_hint(ui, "surface", &items, &mut surface, "No surface");
            drive::choice(ui, "typed", &items, &mut typed);
            drive::controls::choice_with_hint(ui, "chosen", &items, &mut chosen, "No surface");
        };
        harness.show(&mut draw).expect("drawn");
        let output = harness.frame(&mut draw);
        let shapes = flat(&output.shapes);
        output.drop_without_applying_deltas();
        let muted = palette::text_muted().to_egui();
        for (name, hint) in [
            ("empty", drive::controls::NO_CHOICE),
            ("surface", "No surface"),
            ("typed", drive::controls::NO_CHOICE),
        ] {
            let control = harness.control(name).expect("drawn");
            assert_eq!(control.value, "", "{name} reads empty");
            let words = words_in(&shapes, control.rect);
            assert!(
                words.contains(&(String::from(hint), muted)),
                "{name} says \"{hint}\" muted: {words:?}"
            );
        }
        // A choice made shows it, and not the hint.
        let made = harness.control("chosen").expect("drawn");
        assert_eq!(made.value, "alpha");
        let words = words_in(&shapes, made.rect);
        assert!(words.iter().any(|(text, _)| text == "alpha"), "{words:?}");
        assert!(!words.iter().any(|(text, _)| text == "No surface"), "{words:?}");
        // The hint is not an item: a fill of its words is refused, and the choice stays empty.
        harness.fill("surface", "No surface");
        assert!(harness.run(&mut draw).is_err(), "a fill of the hint is refused");
        assert_eq!(harness.control("surface").expect("drawn").value, "");
    }

    #[test]
    fn a_lists_own_empty_item_shows_the_one_empty_word_muted_and_keeps_its_value() {
        // The fourth look audit (3): empty was spelled six ways, mixed in one form - Corridors'
        // "None" and "(none)", Symbol Library's "No style" and "(no symbol)". An item in brackets
        // that IS the empty value shows the window's word for it, muted as an empty choice's hint;
        // what the choice records and takes in a fill is still the item.
        let mut harness = Harness::new();
        apply(harness.context());
        let styles = vec![String::from("(none)"), String::from("Standard")];
        let symbols = vec![String::from("(no symbol)"), String::from("TREE")];
        // DEM Tools' Resolution: its first item, "", is the verb's default, unsaid.
        let resolutions = vec![String::new(), String::from("10")];
        let mut style = Choice::at(0);
        let mut symbol = Choice::at(0);
        let mut resolution = Choice::at(0);
        let mut draw = |ui: &mut Ui| {
            ui.spacing_mut().combo_width = 200.0;
            drive::choice(ui, "style", &styles, &mut style);
            drive::choice(ui, "symbol", &symbols, &mut symbol);
            drive::choice(ui, "resolution", &resolutions, &mut resolution);
        };
        harness.show(&mut draw).expect("drawn");
        let output = harness.frame(&mut draw);
        let shapes = flat(&output.shapes);
        output.drop_without_applying_deltas();
        let muted = palette::text_muted().to_egui();
        for (name, item, shown) in [
            ("style", "(none)", drive::controls::NO_CHOICE),
            ("symbol", "(no symbol)", "No symbol"),
            ("resolution", "", drive::controls::NO_CHOICE),
        ] {
            let control = harness.control(name).expect("drawn");
            assert_eq!(control.value, item, "{name} records its item");
            let words = words_in(&shapes, control.rect);
            assert!(
                words.contains(&(String::from(shown), muted)),
                "{name} shows \"{shown}\" muted: {words:?}"
            );
            assert!(
                item.is_empty() || !words.iter().any(|(text, _)| text == item),
                "{name} does not show {item}: {words:?}"
            );
        }
        // The item's own text fills it as before, and so do the words it shows.
        harness.fill("style", "Standard");
        harness.run(&mut draw).expect("an item is chosen");
        harness.fill("style", "(none)");
        harness.run(&mut draw).expect("the empty item by its text");
        assert_eq!(harness.control("style").expect("drawn").value, "(none)");
        harness.fill("symbol", "TREE");
        harness.run(&mut draw).expect("an item is chosen");
        harness.fill("symbol", "No symbol");
        harness
            .run(&mut draw)
            .expect("the empty item by the words it shows");
        assert_eq!(
            harness.control("symbol").expect("drawn").value,
            "(no symbol)"
        );
    }

    #[test]
    fn a_verbs_tokens_show_in_sentence_case_and_a_fill_takes_either_case() {
        // The fourth look audit ("For 10"): choice values in two cases, "both", "round", "auto"
        // beside "Landscape", "Colour". A choice of a verb's tokens shows them in sentence case;
        // it records the token, and a fill of it in either case chooses it. A choice of names (a
        // layer, a surface) shows them as written.
        let mut harness = Harness::new();
        apply(harness.context());
        let tokens = vec![
            String::from("both"),
            String::from("round"),
            String::from("none"),
        ];
        let names = vec![String::from("ground"), String::from("design")];
        let mut token = Choice::at(1);
        let mut name = Choice::at(0);
        let mut draw = |ui: &mut Ui| {
            ui.spacing_mut().combo_width = 200.0;
            drive::controls::token_choice(ui, "cap", &tokens, &mut token);
            drive::choice(ui, "surface", &names, &mut name);
        };
        harness.show(&mut draw).expect("drawn");
        let output = harness.frame(&mut draw);
        let shapes = flat(&output.shapes);
        output.drop_without_applying_deltas();
        let cap = harness.control("cap").expect("drawn");
        assert_eq!(cap.value, "round", "the token is recorded");
        assert_eq!(cap.items, tokens, "the items are the tokens");
        let words = words_in(&shapes, cap.rect);
        assert!(words.iter().any(|(text, _)| text == "Round"), "{words:?}");
        assert!(!words.iter().any(|(text, _)| text == "round"), "{words:?}");
        let surface = harness.control("surface").expect("drawn");
        let words = words_in(&shapes, surface.rect);
        assert!(
            words.iter().any(|(text, _)| text == "ground"),
            "a name as written: {words:?}"
        );

        harness.fill("cap", "Both");
        harness
            .run(&mut draw)
            .expect("the token in sentence case is taken");
        assert_eq!(harness.control("cap").expect("drawn").value, "both");
        harness.fill("cap", "NONE");
        harness.run(&mut draw).expect("in any case");
        assert_eq!(harness.control("cap").expect("drawn").value, "none");
        harness.fill("cap", "square");
        assert!(harness.run(&mut draw).is_err(), "a token it lacks is refused");
        assert_eq!(drive::controls::no_value_hint("linetype"), "No linetype");
        assert_eq!(drive::controls::SEARCH_HINT, "Search...");
    }

    #[test]
    fn a_fill_in_another_case_is_refused_where_two_items_read_it() {
        // Which item "Plan" means only the items' own spelling can tell: neither is chosen.
        let mut harness = Harness::new();
        let items = vec![String::from("plan"), String::from("PLAN")];
        let mut chosen = Choice::default();
        let mut draw = |ui: &mut Ui| {
            drive::choice(ui, "twins", &items, &mut chosen);
        };
        harness.show(&mut draw).expect("drawn");
        harness.fill("twins", "Plan");
        assert!(harness.run(&mut draw).is_err());
        harness.fill("twins", "PLAN");
        harness.run(&mut draw).expect("its own spelling is taken");
        assert_eq!(harness.control("twins").expect("drawn").value, "PLAN");
    }

    #[test]
    fn a_colour_written_as_text_shows_its_swatch_in_a_table_a_field_and_a_value() {
        // The third look audit (H): Layers, Surface Styles and Text Styles showed a colour as six
        // digits. A table's cell (dialog_layout::table_cell, every table's), a one-line field
        // with the room, and a colour value each paint the colour's swatch; a word does not.
        let mut harness = Harness::new();
        apply(harness.context());
        let green = egui::Color32::from_rgb(0x3C, 0xA0, 0x3C);
        let red = egui::Color32::from_rgb(0xC8, 0x3C, 0x3C);
        let blue = egui::Color32::from_rgb(0x4F, 0xC3, 0xF7);
        let mut field = String::from("#C83C3C");
        let mut narrow = String::from("#C83C3C");
        let mut word = String::from("ByLayer");
        let cells = Cell::new([Rect::NOTHING; 2]);
        let mut draw = |ui: &mut Ui| {
            let colour = katana_window::dialog_layout::table_cell(ui, "#3CA03C").rect;
            let words = katana_window::dialog_layout::table_cell(ui, "ByLayer").rect;
            cells.set([colour, words]);
            for (name, value, width) in [
                ("field", &mut field, 110.0),
                ("narrow", &mut narrow, 60.0),
                ("word", &mut word, 110.0),
            ] {
                drive::line_with(
                    ui,
                    name,
                    value,
                    LineOptions {
                        width: Some(width),
                        ..LineOptions::default()
                    },
                );
            }
            drive::controls::colour_value(ui, "value", "#4FC3F7");
        };
        harness.show(&mut draw).expect("drawn");
        let output = harness.frame(&mut draw);
        let shapes = flat(&output.shapes);
        output.drop_without_applying_deltas();
        let swatch = |area: Rect, colour: egui::Color32| -> Option<Rect> {
            inside(&shapes, area).iter().find_map(|shape| match shape {
                Shape::Rect(rect) if rect.fill == colour => Some(rect.rect),
                _ => None,
            })
        };
        let [colour_cell, word_cell] = cells.get();
        let cell = swatch(colour_cell, green).expect("the colour cell has its swatch");
        assert!(
            (cell.width() - drive::controls::SWATCH_SIZE).abs() <= HALF_A_POINT,
            "{cell:?}"
        );
        assert!(
            inside(&shapes, word_cell)
                .iter()
                .all(|shape| !matches!(shape, Shape::Rect(_))),
            "a word's cell has none"
        );
        let field_rect = harness.control("field").expect("drawn").rect;
        let in_field = swatch(field_rect, red).expect("the field shows its colour");
        assert!(
            in_field.center().x > field_rect.center().x,
            "at the field's right end: {in_field:?} in {field_rect:?}"
        );
        assert_eq!(harness.control("field").expect("drawn").value, "#C83C3C");
        let narrow_rect = harness.control("narrow").expect("drawn").rect;
        assert!(swatch(narrow_rect, red).is_none(), "no room beside its text: no swatch");
        let value = harness.control("value").expect("drawn");
        assert_eq!(value.kind, drive::ControlKind::Label);
        assert_eq!(harness.report("value").expect("read"), "#4FC3F7");
        let value_swatch = shapes
            .iter()
            .find_map(|shape| match shape {
                Shape::Rect(rect) if rect.fill == blue => Some(rect.rect),
                _ => None,
            })
            .expect("the value has its swatch");
        assert!(value_swatch.right() <= value.rect.left(), "before its text");
    }

    #[test]
    fn a_primary_drawn_disabled_is_greyed_not_lit() {
        // The third look audit (A): a primary lit with nothing to run invited the click. Drawn in
        // a disabled Ui (add_enabled_ui(can_run, ..)) it is any disabled button - the fill a
        // plain button disabled beside it has, which egui fades with the Ui - and a press of it
        // is refused. (Compared with that button, not with the accent: a disabled Ui fades every
        // colour it paints, so a lit primary greyed by egui alone is not the accent either.)
        let mut harness = Harness::new();
        apply(harness.context());
        let mut draw = |ui: &mut Ui| {
            drive::primary(ui, "lit", "Run");
            ui.add_enabled_ui(false, |ui| {
                drive::primary(ui, "greyed", "Measure");
                drive::button(ui, "plain", "Close");
            });
        };
        harness.show(&mut draw).expect("drawn");
        let output = harness.frame(&mut draw);
        let shapes = flat(&output.shapes);
        output.drop_without_applying_deltas();
        let fills = |name: &str| -> Vec<egui::Color32> {
            let rect = harness.control(name).expect("drawn").rect;
            inside(&shapes, rect)
                .iter()
                .filter_map(|shape| match shape {
                    Shape::Rect(rect) if rect.fill != egui::Color32::TRANSPARENT => {
                        Some(rect.fill)
                    }
                    _ => None,
                })
                .collect()
        };
        let accent = palette::accent().to_egui();
        assert!(
            fills("lit").contains(&accent),
            "the premise: an enabled primary is in the accent: {:?}",
            fills("lit")
        );
        let plain = fills("plain");
        assert!(!plain.is_empty(), "the premise: a disabled button has a fill");
        assert_eq!(fills("greyed"), plain, "a disabled primary is a disabled button");
        assert!(!harness.control("greyed").expect("drawn").enabled);
        harness.press("greyed");
        assert!(harness.run(&mut draw).is_err(), "a press of it is refused");
    }

    #[test]
    fn a_number_typed_as_text_is_a_number_boxs_width_not_its_rows() {
        // The third look audit (C): Contours' Interval filled its row beside a short Major every.
        // number_line keeps a typed number to a number box's width, wider only for its text.
        let mut harness = Harness::new();
        apply(harness.context());
        let mut short = String::from("0.5");
        let mut empty = String::new();
        let mut long = String::from("123456789012.345678");
        let mut size = 2.5;
        let mut draw = |ui: &mut Ui| {
            ui.spacing_mut().text_edit_width = 300.0;
            drive::controls::number_line(ui, "short", &mut short, "");
            drive::controls::number_line(ui, "empty", &mut empty, "auto");
            drive::controls::number_line(ui, "long", &mut long, "");
            drive::spin(ui, "box", &mut size, SpinRange::new(0.0, 100.0, 0.1, 2));
        };
        harness.show(&mut draw).expect("drawn");
        let width = |name: &str| harness.control(name).expect("drawn").rect.width();
        assert!(
            (width("short") - drive::controls::NUMBER_FIELD_WIDTH).abs() <= HALF_A_POINT,
            "a short number is a box's least width: {}",
            width("short")
        );
        assert!((width("short") - width("box")).abs() <= HALF_A_POINT, "as wide as a box");
        assert!((width("empty") - width("short")).abs() <= HALF_A_POINT);
        assert!(
            width("long") > drive::controls::NUMBER_FIELD_WIDTH && width("long") < 300.0,
            "a long one is as wide as its text: {}",
            width("long")
        );
        assert_eq!(
            harness.control("short").expect("drawn").kind,
            drive::ControlKind::Line
        );
    }

    #[test]
    fn a_tree_expander_is_a_triangle_right_while_closed_and_down_while_open() {
        // T2: the trees drew their expanders as the text ▸ and ▾ (U+25B8, U+25BE), which no font
        // the window carries has: a box each. A leaf's cell is as wide and empty, so a level's
        // names line up.
        let mut harness = Harness::new();
        apply(harness.context());
        let drawn = Cell::new([Rect::NOTHING; 3]);
        let clicked = Cell::new(false);
        let mut draw = |ui: &mut Ui| {
            let closed = drive::tree_expander(ui, Some(false));
            clicked.set(clicked.get() || closed.clicked());
            let open = drive::tree_expander(ui, Some(true)).rect;
            let leaf = drive::tree_expander(ui, None).rect;
            drawn.set([closed.rect, open, leaf]);
        };
        harness.show(&mut draw).expect("drawn");
        let output = harness.frame(&mut draw);
        let shapes = flat(&output.shapes);
        output.drop_without_applying_deltas();
        let cells = drawn.get();
        let triangle = |cell: Rect| -> Vec<Pos2> {
            let drawn = inside(&shapes, cell);
            assert!(
                !drawn.iter().any(|shape| matches!(shape, Shape::Text(_))),
                "no glyph in {cell:?}"
            );
            drawn
                .into_iter()
                .find_map(|shape| match shape {
                    Shape::Path(path) if path.closed && path.points.len() == 3 => Some(path.points),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no triangle in {cell:?}"))
        };
        // Closed, its base is upright (two corners share the least x) and it points right; open,
        // its base lies flat at the top (two share the least y) and it points down.
        let closed = triangle(cells[0]);
        let left = closed.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        assert_eq!(closed.iter().filter(|p| (p.x - left).abs() < 0.01).count(), 2, "{closed:?}");
        let open = triangle(cells[1]);
        let top = open.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        assert_eq!(open.iter().filter(|p| (p.y - top).abs() < 0.01).count(), 2, "{open:?}");
        assert!(inside(&shapes, cells[2]).is_empty(), "a leaf's cell is empty");
        assert_eq!(cells[2].width(), cells[0].width());

        let at = cells[0].center();
        harness
            .pointer_move(at)
            .pointer_button(at, PointerButton::Primary, true, Modifiers::NONE);
        harness.frame(&mut draw).drop_without_applying_deltas();
        harness.pointer_button(at, PointerButton::Primary, false, Modifiers::NONE);
        harness.frame(&mut draw).drop_without_applying_deltas();
        assert!(clicked.get(), "a click on the expander is its click");
    }
}
