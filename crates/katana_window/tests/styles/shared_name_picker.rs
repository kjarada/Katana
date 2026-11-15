//! The twins of tests/qt_widgets/customisation/test_shared_name_picker.cpp: the linetype and
//! symbol picker every manager uses, and the rule it exists for - a picker never drops or rewrites
//! the name it was given (decision D3, audit QT-02).
//!
//! What the C++ did to Qt's widgets is done to the picker's own state: `type_text` is the field
//! cleared and typed into, `highlight_completion` the arrow onto a completion, `press_enter` and
//! `close_completions` the keys, `leave` a click into another field; a choice the C++ heard
//! through `onNameChosen` is what those answer.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use katana_cad::Document;
use katana_cad::style_catalogue::{DefinitionSource, NameRole, missing_names};
use katana_commands::{create_linetype, create_style};
use katana_entity::{
    LineStyle, Linetype, LinetypeElement, Stroke, StrokeOp, Style, StyleLibrary, StyleUnits,
};
use katana_geometry::Point2;
use katana_window::customisation::definition_thumbnails::DefinitionThumbnails;
use katana_window::customisation::style_painter::CanvasRasterBackend;
use katana_window::customisation::{CustomisationContext, NamePicker, UNDEFINED_NAME_COLOUR};

fn paper_linestyle(name: &str) -> LineStyle {
    LineStyle {
        name: String::from(name),
        units: StyleUnits::Paper,
        length: 4.0,
        strokes: vec![
            Stroke {
                op: StrokeOp::Move,
                point: Point2::new(0.0, 0.0),
                ..Stroke::default()
            },
            Stroke {
                op: StrokeOp::Draw,
                point: Point2::new(1.5, 0.0),
                ..Stroke::default()
            },
        ],
        ..LineStyle::default()
    }
}

/// A library of two definitions written for this test: a paper linestyle (not `mode vertex`, so a
/// linestyle - D2) and a vertex symbol (a symbol - D3). Their strokes are irrelevant here; they
/// only have to be valid.
fn test_library() -> StyleLibrary {
    let mut library = StyleLibrary::new();
    library
        .add(paper_linestyle("TEST Dashed Kerb"))
        .expect("the kerb is added");
    let manhole = LineStyle {
        name: String::from("TEST Manhole"),
        at_vertices: true,
        strokes: vec![Stroke {
            op: StrokeOp::Circle,
            radius: 0.5,
            ..Stroke::default()
        }],
        ..LineStyle::default()
    };
    library.add(manhole).expect("the manhole is added");
    library
}

/// A library of paper linestyles with these names and nothing else: the case tests need names
/// that differ only in case, which test_library has not.
fn linestyles_named(names: &[&str]) -> StyleLibrary {
    let mut library = StyleLibrary::new();
    for name in names {
        library
            .add(paper_linestyle(name))
            .expect("the linestyle is added");
    }
    library
}

struct PickerFixture {
    document: Document,
    context: CustomisationContext,
}

impl PickerFixture {
    fn new() -> Self {
        let mut document = Document::new(None);
        document.set_style_library(test_library());
        let dashed = Linetype {
            name: String::from("DASHED"),
            pattern: vec![
                LinetypeElement { length: 1.0 },
                LinetypeElement { length: -0.5 },
            ],
            ..Linetype::default()
        };
        document
            .execute(create_linetype(dashed))
            .expect("DASHED is made");
        let thumbnails = Rc::new(RefCell::new(DefinitionThumbnails::new(Arc::new(
            CanvasRasterBackend,
        ))));
        Self {
            document,
            context: CustomisationContext::default().sharing(thumbnails),
        }
    }

    fn picker(&self, role: NameRole, offer_by_layer: bool) -> NamePicker {
        NamePicker::new(&self.context, &self.document, role, offer_by_layer)
    }
}

/// The row showing `text`.
fn row_of(picker: &NamePicker, text: &str) -> Option<usize> {
    picker.find_text(text)
}

mod name_picker {
    use super::*;

    #[test]
    fn a_name_defined_nowhere_survives_set_and_current_name_byte_for_byte() {
        let fixture = PickerFixture::new();
        let mut picker = fixture.picker(NameRole::Symbol, false);
        // A library name as a library in another office might spell it: two spaces, a trailing
        // space and UTF-8 "e acute". The C++ name also holds a byte (0xFF) that is not UTF-8 at
        // all; a name here is a String, which cannot hold one, so that byte is left out.
        let name = "Pit  Grated \u{e9} (SW) ";
        picker.set_current_name(&fixture.document, name);
        assert_eq!(picker.current_name(), name);
        assert!(!picker.current_is_defined());
        // Kept first in the list, not dropped.
        let names = picker.names();
        assert!(!names.is_empty());
        assert_eq!(names[0], name);
        // Its tooltip says what it draws as meanwhile: entity's built_in_symbol_for finds "pit" in
        // the name, so a manhole.
        assert!(picker.items()[0].tool_tip.contains("manhole"));
        assert_eq!(picker.items()[0].foreground, Some(UNDEFINED_NAME_COLOUR));
    }

    #[test]
    fn the_decorated_label_is_what_is_shown_but_never_what_current_name_returns() {
        let fixture = PickerFixture::new();
        let mut picker = fixture.picker(NameRole::Linetype, true);
        picker.set_current_name(&fixture.document, "Old Kerb");
        assert_eq!(picker.current_text(), "Old Kerb (not defined)");
        assert_eq!(picker.current_name(), "Old Kerb");

        // A name that IS listed carries no decoration and no mark.
        picker.set_current_name(&fixture.document, "TEST Dashed Kerb");
        assert_eq!(picker.current_text(), "TEST Dashed Kerb");
        assert!(picker.current_is_defined());
        // And the previous kept name is gone: it belonged to the other value.
        assert_eq!(row_of(&picker, "Old Kerb (not defined)"), None);

        // Names are case-sensitive: a different case is a different, undefined name, kept as given
        // rather than "corrected" to the listed one.
        picker.set_current_name(&fixture.document, "test dashed kerb");
        assert_eq!(picker.current_name(), "test dashed kerb");
        assert!(!picker.current_is_defined());
    }

    #[test]
    fn the_list_is_in_sections_and_only_a_styles_linetype_offers_by_layer() {
        let fixture = PickerFixture::new();
        let style = fixture.picker(NameRole::Linetype, true);
        assert!(style.count() > 0);
        assert_eq!(style.item_text(0), "ByLayer");
        let drawing = row_of(&style, "Drawing linetypes").expect("the drawing's header");
        let library = row_of(&style, "Library linestyles").expect("the library's header");
        let dashed = row_of(&style, "DASHED").expect("DASHED is listed");
        assert!(drawing < dashed);
        assert!(dashed < library);
        assert!(library < row_of(&style, "TEST Dashed Kerb").expect("the kerb is listed"));
        // A header cannot be chosen.
        assert!(style.items()[drawing].name.is_none());
        // A vertex symbol is not a linestyle (D2).
        assert_eq!(row_of(&style, "TEST Manhole"), None);

        let layer = fixture.picker(NameRole::Linetype, false);
        assert_eq!(row_of(&layer, "ByLayer"), None);

        let symbol = fixture.picker(NameRole::Symbol, false);
        let built_in = row_of(&symbol, "Built-in symbols").expect("the built-in header");
        let library_symbols = row_of(&symbol, "Library symbols").expect("the library's header");
        assert!(library_symbols > built_in);
        let manhole = row_of(&symbol, "TEST Manhole").expect("the manhole is listed");
        assert!(manhole > library_symbols);
        let circle = row_of(&symbol, "circle").expect("circle is listed");
        assert!(circle > built_in);
        assert!(circle < library_symbols);
        // Every entry has a picture, the shared cache's.
        assert!(symbol.items()[manhole].icon.is_some());
        let kerb = row_of(&style, "TEST Dashed Kerb").expect("the kerb is listed");
        assert!(style.items()[kerb].icon.is_some());
        assert!(style.items()[dashed].icon.is_some());
    }

    #[test]
    fn typing_a_fragment_in_any_case_completes_and_choosing_it_reports_the_exact_name() {
        let fixture = PickerFixture::new();
        let mut picker = fixture.picker(NameRole::Linetype, true);
        let mut chosen: Vec<String> = Vec::new();

        // "dASHED k": the middle of "TEST Dashed Kerb", in neither of its cases. "DASHED" contains
        // "dashed" but not "dashed k", so one completion.
        picker.type_text("dASHED k");
        assert_eq!(picker.completion_count(), 1);
        assert_eq!(picker.completions()[0], "TEST Dashed Kerb");

        // Chosen from the completions as a person would: the arrow onto it, then Enter.
        assert!(picker.completions_open());
        picker.highlight_completion(0);
        chosen.extend(picker.press_enter());
        assert_eq!(picker.current_name(), "TEST Dashed Kerb");
        assert_eq!(chosen, ["TEST Dashed Kerb"]);

        // "dash" alone is in both names.
        picker.type_text("dash");
        assert_eq!(picker.completion_count(), 2);
    }

    #[test]
    fn setting_and_refreshing_never_fire_on_name_chosen_and_typing_does() {
        let mut fixture = PickerFixture::new();
        let mut picker = fixture.picker(NameRole::Linetype, true);
        // Setting and refreshing answer no choice at all: they have none to answer.
        picker.set_current_name(&fixture.document, "DASHED");
        picker.set_current_name(&fixture.document, "Unknown Name");
        picker.refresh(&fixture.document);
        fixture.document.set_style_library(StyleLibrary::new());
        picker.refresh(&fixture.document);
        let mut fired: Vec<String> = Vec::new();

        // A person typing a name and pressing Enter is a choice.
        picker.type_text("DASHED");
        fired.extend(picker.press_enter());
        assert_eq!(fired.len(), 1);
        assert_eq!(fired.last().map(String::as_str), Some("DASHED"));
        // Enter again with nothing changed is not another choice.
        fired.extend(picker.press_enter());
        assert_eq!(fired.len(), 1);
    }

    #[test]
    fn a_library_reload_keeps_the_current_name_marked_while_it_is_undefined() {
        let mut fixture = PickerFixture::new();
        let mut picker = fixture.picker(NameRole::Linetype, true);
        picker.set_current_name(&fixture.document, "TEST Dashed Kerb");
        assert!(picker.current_is_defined());

        // The library goes: the name stays, marked.
        fixture.document.set_style_library(StyleLibrary::new());
        picker.refresh(&fixture.document);
        assert_eq!(picker.current_name(), "TEST Dashed Kerb");
        assert!(!picker.current_is_defined());
        assert_eq!(picker.current_text(), "TEST Dashed Kerb (not defined)");

        // And comes back: defined again, listed once.
        fixture.document.set_style_library(test_library());
        picker.refresh(&fixture.document);
        assert_eq!(picker.current_name(), "TEST Dashed Kerb");
        assert!(picker.current_is_defined());
        let names = picker.names();
        assert_eq!(
            names
                .iter()
                .filter(|name| *name == "TEST Dashed Kerb")
                .count(),
            1
        );
    }

    // Review of 2026-09-24: the choice box finished an edit with its own lookup before the picker
    // heard of it, folding case whenever the completions' search did, so it jumped to the first
    // listed name in ANY case and reported it as chosen - the rewriting of a name D3 forbids.

    #[test]
    fn enter_on_an_exactly_typed_name_chooses_that_name_not_another_case_of_it() {
        let mut fixture = PickerFixture::new();
        fixture
            .document
            .set_style_library(linestyles_named(&["Kerb", "KERB"]));
        let mut picker = fixture.picker(NameRole::Linetype, true);
        // Sorted with case folded, ties by bytes: "KERB" (0x45) before "Kerb" (0x65), so a
        // case-folded search meets "KERB" first.
        assert!(row_of(&picker, "KERB").expect("KERB") < row_of(&picker, "Kerb").expect("Kerb"));
        picker.set_current_name(&fixture.document, "ByLayer");
        let mut chosen: Vec<String> = Vec::new();

        picker.type_text("Kerb");
        // Both names contain "kerb" folded, so the completions offer both, and nothing in them is
        // highlighted: Enter finishes what was typed.
        assert_eq!(picker.completion_count(), 2);
        chosen.extend(picker.press_enter());
        assert_eq!(chosen, ["Kerb"]);
        assert_eq!(picker.current_name(), "Kerb");
        assert!(picker.current_is_defined());
    }

    #[test]
    fn enter_on_a_name_listed_only_in_another_case_keeps_the_typed_case() {
        let mut fixture = PickerFixture::new();
        fixture
            .document
            .set_style_library(linestyles_named(&["Kerb"]));
        let mut picker = fixture.picker(NameRole::Linetype, true);
        picker.set_current_name(&fixture.document, "ByLayer");
        let mut chosen: Vec<String> = Vec::new();

        // "KERB": a name from a library that is not loaded. The completions offer "Kerb"; the
        // person closes them and presses Enter on what they typed.
        picker.type_text("KERB");
        assert!(picker.completions_open());
        picker.close_completions();
        assert!(!picker.completions_open());
        chosen.extend(picker.press_enter());
        assert_eq!(chosen, ["KERB"]);
        assert_eq!(picker.current_name(), "KERB");
        assert_eq!(picker.current_text(), "KERB");
        // Names are case-sensitive: "KERB" is not the listed "Kerb".
        assert!(!picker.current_is_defined());
    }

    #[test]
    fn leaving_the_field_keeps_the_typed_case_of_a_name_listed_in_another() {
        let mut fixture = PickerFixture::new();
        fixture
            .document
            .set_style_library(linestyles_named(&["Kerb"]));
        let mut picker = fixture.picker(NameRole::Linetype, true);
        picker.set_current_name(&fixture.document, "ByLayer");
        let mut chosen: Vec<String> = Vec::new();

        picker.type_text("kerb");
        chosen.extend(picker.leave());
        assert_eq!(chosen, ["kerb"]);
        assert_eq!(picker.current_text(), "kerb");
        assert_eq!(picker.current_name(), "kerb");
    }

    // The same review: the picker ignored the field's finished edit whenever the completions were
    // open, trusting them to report the choice - but only a HIGHLIGHTED completion is one, and the
    // field does not finish again on leaving when nothing has changed since. A new name that is
    // part of a listed one was dropped without a word.
    #[test]
    fn enter_on_a_typed_name_with_nothing_highlighted_in_the_popup_chooses_it() {
        let fixture = PickerFixture::new();
        let mut picker = fixture.picker(NameRole::Linetype, true);
        picker.set_current_name(&fixture.document, "ByLayer");
        let mut chosen: Vec<String> = Vec::new();

        // "Kerb", a new library name, is part of "TEST Dashed Kerb" and of nothing else listed
        // (ByLayer, DASHED), so the completions open on that one name.
        picker.type_text("Kerb");
        assert!(picker.completions_open());
        assert_eq!(picker.completion_count(), 1);
        assert!(!picker.completion_highlighted());

        chosen.extend(picker.press_enter());
        assert_eq!(chosen, ["Kerb"]);
        assert_eq!(picker.current_name(), "Kerb");
        // Leaving the field afterwards is not a second choice.
        chosen.extend(picker.leave());
        assert_eq!(chosen, ["Kerb"]);
    }

    // The same review: the picker marked every linetype name it did not list as "(not defined)",
    // including the names cad's missing_names deliberately never reports - the plain continuous
    // line ("1", "0", "continuous" in any case; D4) and ByLayer in another spelling. An archive
    // import names "1" for every plain string, so the style manager would have marked every one of
    // them in amber while its Missing chip, counted by missing_names, said none was missing.
    #[test]
    fn plain_line_names_are_neither_marked_nor_missing_and_a_missing_name_is_both() {
        let mut fixture = PickerFixture::new();
        // Styles naming each plain spelling, one naming a genuinely missing library linestyle, and
        // one naming the fixture's listed library linestyle.
        let styles = [
            ("plain one", "1"),
            ("plain zero", "0"),
            ("dxf", "Continuous"),
            ("shouted", "CONTINUOUS"),
            ("inherit", "BYLAYER"),
            ("old", "Old Survey Kerb"),
            ("kerb", "TEST Dashed Kerb"),
        ];
        for (name, linetype) in styles {
            let style = Style {
                name: String::from(name),
                linetype: String::from(linetype),
                ..Style::default()
            };
            fixture
                .document
                .execute(create_style(style))
                .expect("the style is made");
        }
        // The cad side, worked out from missing_names' rule (style_catalogue): of these only "Old
        // Survey Kerb" is in no table and no library and is not a plain-line name.
        let missing = missing_names(&fixture.document);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].name, "Old Survey Kerb");

        let mut picker = fixture.picker(NameRole::Linetype, true);
        for (_, linetype) in styles {
            picker.set_current_name(&fixture.document, linetype);
            let reported = linetype == "Old Survey Kerb";
            assert_eq!(picker.current_is_defined(), !reported, "{linetype}");
            // Kept as given either way: "BYLAYER" is not rewritten to the listed "ByLayer", nor
            // "Continuous" to anything.
            assert_eq!(picker.current_name(), linetype);
            let row = picker.current_index().expect("a row is shown");
            let marked = picker.items()[row].source == Some(DefinitionSource::Undefined);
            assert_eq!(marked, reported, "{linetype}");
            assert_eq!(
                picker.current_text().ends_with(" (not defined)"),
                reported,
                "{linetype}"
            );
            assert_eq!(picker.field_is_marked(), reported, "{linetype}");
        }

        // And each tooltip says what is drawn: the plain continuous line, or - for ByLayer in any
        // spelling on a Style - the layer's linetype, not a solid line.
        picker.set_current_name(&fixture.document, "1");
        assert!(picker.tool_tip().contains("the plain continuous line"));
        picker.set_current_name(&fixture.document, "BYLAYER");
        assert!(picker.tool_tip().contains("layer's linetype"));
        assert!(!picker.tool_tip().contains("solid line"));
    }
}
