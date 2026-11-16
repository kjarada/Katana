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
use katana_window::drive::{self, Choice, Harness, LineOptions};
use katana_window::theme::palette;
use katana_window::theme::style::apply;

/// Every shape of `shapes`, nested ones too, as painted.
fn flat(shapes: &[ClippedShape]) -> Vec<Shape> {
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
fn inside(shapes: &[Shape], area: Rect) -> Vec<Shape> {
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
