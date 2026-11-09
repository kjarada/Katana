// The words the survey code tools show a rule's parts by, where no other test
// pins them: every section's word in the code table, a rule's `surface`, a
// definition drawn `at vertices`, and a layer that is not a layer path
// (tests/cad/customisation/test_code_words.cpp).
//
// They are the Katana customisation format's words - "feature", "layer",
// "surface", "at vertices" - and each expected text is written out by hand
// from the rules the test builds and the table "The words a rule is shown by"
// in docs/customisation.md, never captured from a run.

use katana_cad::code_table::{
    code_table, explain_code, format_code_explanation, format_code_table, format_coverage,
    lint_survey_rule,
};
use katana_cad::style_catalogue::check_linetype_name;
use katana_cad::style_manager_rows::style_diagnostics;
use katana_cad::survey_coding::customisation_coverage;
use katana_cad::{DefinitionLookup, Document, LintKind, LintSeverity, StyleDiagnosticKind};
use katana_commands::create_style;
use katana_core::ErrorCode;
use katana_entity::style_library::add_or_replace;
use katana_entity::{LineStyle, Style, StyleLibrary, SurveyMap, SurveyRule, SurveySection};

use crate::{done, err, must};

// Two definitions drawn at vertices - symbols, never line patterns - and
// nothing else. Their names, and the one below that nothing defines, are
// invented as the other fixtures' are ("TEST ..."): a name a real
// customisation defines is its author's, and a fixture is committed.
fn marks() -> StyleLibrary {
    let mut library = StyleLibrary::default();
    for name in ["TEST Peg", "TEST Nail"] {
        let style = LineStyle {
            name: String::from(name),
            at_vertices: true,
            ..LineStyle::default()
        };
        assert!(add_or_replace(&mut library, style).is_ok(), "{name}");
    }
    library
}

// A drawing with that library and one style per linetype name given.
fn styled_with(document: &mut Document, styles: &[(&str, &str)]) {
    document.set_style_library(marks());
    for &(name, linetype) in styles {
        let style = Style {
            name: String::from(name),
            linetype: String::from(linetype),
            ..Style::default()
        };
        must(document, create_style(style));
    }
}

mod code_words {
    use super::*;

    #[test]
    fn the_code_table_names_each_of_the_nine_sections_by_its_word() {
        // One key with a rule in every section, none saying anything else: the
        // row lists the sections in the enumeration's order and has no parts.
        let mut map = SurveyMap::new();
        for section in [
            SurveySection::Map,
            SurveySection::VertexSymbol,
            SurveySection::VertexTextStyle,
            SurveySection::Pipe,
            SurveySection::VertexPipe,
            SurveySection::SegmentPipe,
            SurveySection::StringAttribute,
            SurveySection::VertexAttribute,
            SurveySection::Tinable,
        ] {
            let rule = SurveyRule {
                key: String::from("K*"),
                section,
                ..SurveyRule::default()
            };
            done(map.add(rule));
        }
        assert_eq!(
            format_code_table(&code_table(&map), ""),
            "K*  [prefix; 9 rules: feature, symbol, text, pipe, vertexPipe, segmentPipe, \
             attributes, vertexAttributes, surface]\n\
             1 of 1 code\n"
        );
    }

    #[test]
    fn whether_a_code_goes_into_a_surface_is_explained_as_its_surface() {
        // GS, an exact key, says only that its code goes into a surface.
        let mut map = SurveyMap::new();
        let ground = SurveyRule {
            key: String::from("GS"),
            section: SurveySection::Tinable,
            tinable: Some(true),
            ..SurveyRule::default()
        };
        done(map.add(ground));

        let why = explain_code(&map, "GS", None, None);
        assert_eq!(why.fields.len(), 1);
        assert_eq!(why.fields[0].field, "surface");
        assert_eq!(
            format_code_explanation(&why),
            "Code \"GS\": exact match, matched\n  surface: yes  <- rule #0 GS (surface)\n"
        );
    }

    #[test]
    fn a_linestyle_naming_a_definition_drawn_at_vertices_is_said_to_be_a_symbol_at_vertices() {
        // PG* draws its line with TEST Peg, which the library holds as a symbol.
        let library = marks();
        let pegs = SurveyRule {
            key: String::from("PG*"),
            model: String::from("SURVEY MARKS"),
            linestyle: String::from("TEST Peg"),
            ..SurveyRule::default()
        };
        let mut map = SurveyMap::new();
        done(map.add(pegs.clone()));

        // The lint: that one warning, and nothing else is wrong with the rule.
        let issues = lint_survey_rule(&pegs, 0, &library, None, None);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, LintKind::LinestyleIsVertex);
        assert_eq!(
            issues[0].message,
            "linestyle \"TEST Peg\" is a symbol (at vertices), drawn at each vertex rather than \
             along the line"
        );

        // The explanation: the layer and the linestyle from rule #0, then what
        // the linestyle's name resolves to.
        let lookup = |name: &str| library.find(name);
        let definition_of: &DefinitionLookup<'_> = &lookup;
        let why = explain_code(&map, "PG1", Some(definition_of), None);
        assert_eq!(
            format_code_explanation(&why),
            "Code \"PG1\": prefix match, matched\n\
             \x20 layer: SURVEY MARKS  <- rule #0 PG* (feature)\n\
             \x20 linestyle: TEST Peg  <- rule #0 PG* (feature)\n\
             \x20 linestyle \"TEST Peg\": defined, but as a symbol (at vertices)\n"
        );
    }

    #[test]
    fn a_layer_that_is_not_a_layer_path_is_an_error_naming_the_layer_and_never_saying_valid_layer()
    {
        // "A//B" has an empty level. SurveyMap::add refuses such a rule, so the
        // lint meets it only on a rule not yet in a map: an editor's form.
        let rule = SurveyRule {
            key: String::from("WM*"),
            model: String::from("A//B"),
            ..SurveyRule::default()
        };
        let issues = lint_survey_rule(&rule, 0, &StyleLibrary::default(), None, None);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, LintKind::InvalidLayerPath);
        assert_eq!(issues[0].severity, LintSeverity::Error);
        assert!(
            issues[0]
                .message
                .starts_with("layer \"A//B\" is not a layer path: "),
            "{}",
            issues[0].message
        );
        // The code manager shows this message under the Layer field, where "valid
        // layer" is what its two GOOD states say ("A valid layer the drawing
        // has."): the refusal must not read as one of them.
        assert!(
            !issues[0].message.contains("valid layer"),
            "{}",
            issues[0].message
        );
    }

    #[test]
    fn two_linetypes_naming_symbols_at_vertices_are_counted_in_the_plural_by_that_word() {
        // Two styles, each naming a definition the library holds only as a
        // symbol: neither resolves, both name one, and the names come in name
        // order ("TEST Nail" before "TEST Peg").
        let mut document = Document::default();
        styled_with(
            &mut document,
            &[("pegs", "TEST Peg"), ("nails", "TEST Nail")],
        );
        assert_eq!(
            format_coverage(&customisation_coverage(&document)),
            "0 of this drawing's 2 styles are drawn with a loaded definition (2 name one; the \
             rest are plain lines)\n\
             \x20 2 names are loaded as `at vertices` symbols, not linestyles, so a linetype \
             naming one draws solid: \"TEST Nail\", \"TEST Peg\"\n"
        );
    }

    #[test]
    fn the_style_manager_says_a_linetype_naming_a_symbol_is_drawn_solid_and_why() {
        // One style whose linetype is a symbol: drawn as the continuous line, and
        // the reason is that the name is defined - as a symbol at vertices.
        let mut document = Document::default();
        styled_with(&mut document, &[("pegs", "TEST Peg")]);
        let diagnostics = style_diagnostics(&document);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, StyleDiagnosticKind::MissingLinetype);
        assert_eq!(diagnostics[0].name, "TEST Peg");
        assert_eq!(
            diagnostics[0].drawn_as,
            "a solid line (continuous): an `at vertices` symbol, not a linestyle"
        );
    }

    #[test]
    fn giving_a_symbol_at_vertices_as_a_linetype_is_refused_in_the_style_managers_words() {
        // What STYLE SET, LAYER LTYPE and Global Modify ask before taking a
        // linetype name. TEST Peg is defined - as a symbol - so it is refused by
        // what it is, in the words "Drawn as" has for the same state above, and
        // not as a name nothing defines.
        let mut document = Document::default();
        document.set_style_library(marks());
        let asked = err(check_linetype_name(&document, "TEST Peg"));
        assert_eq!(asked.code, ErrorCode::InvalidArgument);
        assert_eq!(
            asked.message,
            "that is an `at vertices` symbol, not a linestyle: give it as the symbol"
        );
        assert_eq!(asked.context, "TEST Peg");
        // A name nothing defines is the other refusal, and says nothing of symbols.
        let missing = err(check_linetype_name(&document, "TEST Spike"));
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert!(!missing.message.contains("symbol"), "{}", missing.message);
    }
}
