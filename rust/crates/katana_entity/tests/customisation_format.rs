//! The twin of tests/entity/customisation/test_customisation_format.cpp: the
//! Katana customisation format (docs/customisation.md) - a customisation that
//! is written reads back as the same value, the bytes written are the layout
//! the document states, and the degenerate files read as what they say.
//!
//! Every expectation here is written by hand from the format's stated rules -
//! the layout, the member names, the defaults - and none is a copy of what a
//! run printed.
//!
//! Customisation.ANameIsOneLineOfTextAndNotAPath also hands the check bytes
//! that are not UTF-8, which a `&str` cannot hold: its UTF-8 half is the twin.

#![allow(
    clippy::excessive_precision,
    reason = "Preserve the decimal constants copied verbatim from the C++ oracle."
)]

pub mod customisation_support;

use katana_core::error::ErrorCode;
use katana_entity::customisation::{
    customisation_digest, customisation_to_json, definition_from_json, definition_to_json,
    validate_customisation_name,
};
use katana_entity::{
    Color, Customisation, CustomisationAutomation, CustomisationBase, CustomisationSourceNote,
    CustomisationWriteOptions, LineStyle, LineworkCodes, Stroke, StrokeOp, StrokeText,
    StyleLibrary, StyleUnits, SurveyAttribute, SurveyBreakline, SurveyMap, SurveyPipe, SurveyRule,
    SurveySection, SurveySymbol, SurveyTextStyle,
};
use katana_geometry::Point2;

use customisation_support::{
    customisation_with, expect_names, read_customisation, write_customisation,
    write_customisation_with,
};

fn move_to(x: f64, y: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Move,
        point: Point2::new(x, y),
        ..Stroke::default()
    }
}

fn draw_to(x: f64, y: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Draw,
        point: Point2::new(x, y),
        ..Stroke::default()
    }
}

fn arc(radius: f64, start: f64, end: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Arc,
        radius,
        start_angle: start,
        end_angle: end,
        ..Stroke::default()
    }
}

fn circle(radius: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Circle,
        radius,
        ..Stroke::default()
    }
}

fn dot(radius: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Dot,
        radius,
        ..Stroke::default()
    }
}

fn pen(name: &str) -> Stroke {
    Stroke {
        op: StrokeOp::Pen,
        pen: String::from(name),
        ..Stroke::default()
    }
}

/// A text stroke and the text it places, kept in step as the model requires.
fn add_text(definition: &mut LineStyle, text: StrokeText) {
    let stroke = Stroke {
        op: StrokeOp::Text,
        text: definition.texts.len(),
        ..Stroke::default()
    };
    definition.texts.push(text);
    definition.strokes.push(stroke);
}

fn add_definition(customisation: &mut Customisation, definition: LineStyle) {
    if let Err(error) = customisation.library.add(definition) {
        panic!("{}", error.describe());
    }
}

fn add_rule(customisation: &mut Customisation, rule: SurveyRule) {
    if let Err(error) = customisation.map.add(rule) {
        panic!("{}", error.describe());
    }
}

fn named_customisation(name: &str) -> Customisation {
    Customisation {
        name: String::from(name),
        ..Customisation::default()
    }
}

fn text_of(
    text: &str,
    angle: f64,
    height: f64,
    justify: &str,
    font: &str,
    width_factor: f64,
    unnamed: [f64; 3],
) -> StrokeText {
    StrokeText {
        text: String::from(text),
        angle,
        height,
        justify: String::from(justify),
        font: String::from(font),
        width_factor,
        unnamed,
    }
}

fn source_note(
    name: &str,
    definitions: bool,
    rules: bool,
    notice: &[&str],
) -> CustomisationSourceNote {
    CustomisationSourceNote {
        name: String::from(name),
        definitions,
        rules,
        notice: notice.iter().map(|&line| String::from(line)).collect(),
    }
}

fn base(name: &str, digest: &str) -> CustomisationBase {
    CustomisationBase {
        name: String::from(name),
        digest: String::from(digest),
    }
}

fn attribute(kind: &str, name: &str, value: &str) -> SurveyAttribute {
    SurveyAttribute {
        r#type: String::from(kind),
        name: String::from(name),
        value: String::from(value),
    }
}

fn pipe_of(justify: &str, shape: &str, size1: &str, size2: &str, active: bool) -> SurveyPipe {
    SurveyPipe {
        justify: String::from(justify),
        shape: String::from(shape),
        size1: String::from(size1),
        size2: String::from(size2),
        active,
    }
}

fn symbol_of(
    style: &str,
    colour: &str,
    size: f64,
    rotation: f64,
    offset: f64,
    raise: f64,
) -> SurveySymbol {
    SurveySymbol {
        style: String::from(style),
        colour: String::from(colour),
        size,
        rotation,
        offset,
        raise,
    }
}

// ---- the small customisation whose text is written out below ----------------------------

fn fence() -> LineStyle {
    let mut definition = LineStyle {
        name: String::from("Fence"),
        group: String::from("Site/Lines"),
        length: 2.5,
        source: String::from("Site"),
        strokes: vec![
            move_to(0.0, 0.0),
            draw_to(1.5, 0.0),
            pen("red"),
            arc(-0.25, 0.0, 180.0),
            circle(0.5),
            dot(0.0),
        ],
        ..LineStyle::default()
    };
    let label = StrokeText {
        text: String::from("F"),
        height: 1.5,
        justify: String::from("middle-centre"),
        ..StrokeText::default()
    };
    add_text(&mut definition, label);
    definition
}

fn small_customisation() -> Customisation {
    let mut customisation = Customisation {
        name: String::from("Site"),
        description: String::from("A small site set."),
        notice: vec![String::from("Line one."), String::from("Line \"two\".")],
        sources: vec![
            source_note("Site", true, true, &["Made by hand."]),
            source_note("Shared", false, false, &[]),
        ],
        based_on: Some(base("NSW", "0123456789abcdef")),
        linework: Some(LineworkCodes {
            close: String::from("C"),
            ..LineworkCodes::default()
        }),
        automation: Some(CustomisationAutomation {
            codes_on_survey_import: true,
            linework_on_survey_import: false,
        }),
        ..Customisation::default()
    };
    assert!(
        customisation
            .colours
            .add("Site Orange", Color::new(255, 127, 0, 255))
            .is_ok()
    );
    assert!(
        customisation
            .colours
            .add("site_grey", Color::new(128, 128, 128, 255))
            .is_ok()
    );

    add_definition(
        &mut customisation,
        LineStyle {
            name: String::from("Empty"),
            source: String::from("Shared"),
            ..LineStyle::default()
        },
    );

    add_definition(&mut customisation, fence());

    add_definition(
        &mut customisation,
        LineStyle {
            name: String::from("Gate"),
            units: StyleUnits::TwoPoint,
            anchor1: Point2::new(0.0, 0.75),
            anchor2: Point2::new(14.0, 0.75),
            stretch_mode: 2,
            cycle_mode: 1,
            source: String::from("Site"),
            strokes: vec![move_to(0.0, 0.0), draw_to(14.0, 0.0)],
            ..LineStyle::default()
        },
    );

    add_definition(
        &mut customisation,
        LineStyle {
            name: String::from("Peg"),
            units: StyleUnits::Paper,
            at_vertices: true,
            factor: 2.0,
            origin: Point2::new(1.0, -2.0),
            source: String::from("Site"),
            symbol: true,
            strokes: vec![circle(0.5)],
            ..LineStyle::default()
        },
    );

    add_rule(
        &mut customisation,
        SurveyRule {
            key: String::from("FN*"),
            section: SurveySection::Map,
            model: String::from("SITE/FENCES"),
            colour: String::from("Site Orange"),
            breakline: Some(SurveyBreakline::Line),
            linestyle: String::from("Fence"),
            weight: String::from("0"),
            group: String::from("Site"),
            comment: String::from("a fence"),
            ..SurveyRule::default()
        },
    );

    add_rule(
        &mut customisation,
        SurveyRule {
            key: String::from("PG"),
            section: SurveySection::VertexSymbol,
            hide: Some(false),
            symbol: Some(symbol_of("Peg", "", 1.5, 0.0, 0.0, 0.0)),
            ..SurveyRule::default()
        },
    );

    add_rule(
        &mut customisation,
        SurveyRule {
            key: String::from("*"),
            section: SurveySection::VertexTextStyle,
            text_style: Some(SurveyTextStyle::default()),
            ..SurveyRule::default()
        },
    );

    add_rule(
        &mut customisation,
        SurveyRule {
            key: String::from("TN*"),
            section: SurveySection::Tinable,
            tinable: Some(false),
            ..SurveyRule::default()
        },
    );

    add_rule(
        &mut customisation,
        SurveyRule {
            key: String::from("PP*"),
            section: SurveySection::Pipe,
            pipe: Some(pipe_of("Obvert", "", "$PipeDiameter", "", true)),
            attributes: vec![
                attribute("text", "Owner", "Council"),
                attribute("integer", "Zone", ""),
            ],
            ..SurveyRule::default()
        },
    );
    customisation
}

// By hand, from "Layout" in docs/customisation.md: two blanks a level; the
// top-level members one a line in their fixed order; the entries of notice,
// sources, colours, linestyles, symbols and codes one a line, colours in the
// order of their folded names ("site grey" before "site orange") and
// definitions in name order; a definition's strokes one a line; a member at
// its default left out, except those always written and a present optional
// ("hide": false, "text": {}); every member of linework and automation.
const SMALL_CUSTOMISATION_TEXT: &str = r##"{
  "format": "katana-customisation",
  "version": 1,
  "name": "Site",
  "description": "A small site set.",
  "notice": [
    "Line one.",
    "Line \"two\"."
  ],
  "sources": [
    {"name": "Site", "definitions": true, "rules": true, "notice": ["Made by hand."]},
    {"name": "Shared"}
  ],
  "basedOn": {"name": "NSW", "digest": "0123456789abcdef"},
  "colours": {
    "site_grey": "#808080",
    "Site Orange": "#FF7F00"
  },
  "linework": {"start": "ST", "end": "END", "close": "C", "arcStart": "BC", "arcEnd": "EC", "join": "JPN", "rectangle": "RECT"},
  "automation": {"codesOnSurveyImport": true, "lineworkOnSurveyImport": false},
  "linestyles": [
    {"name": "Empty", "from": "Shared"},
    {"name": "Fence", "group": "Site/Lines", "length": 2.5, "strokes": [
      ["move", 0, 0],
      ["draw", 1.5, 0],
      ["pen", "red"],
      ["arc", -0.25, 0, 180],
      ["circle", 0.5],
      ["dot", 0],
      ["text", {"text": "F", "height": 1.5, "justify": "middle-centre"}]
    ]},
    {"name": "Gate", "units": "twoPoint", "anchors": [[0, 0.75], [14, 0.75]], "stretchMode": 2, "cycleMode": 1, "strokes": [
      ["move", 0, 0],
      ["draw", 14, 0]
    ]}
  ],
  "symbols": [
    {"name": "Peg", "units": "paper", "atVertices": true, "factor": 2, "origin": [1, -2], "strokes": [
      ["circle", 0.5]
    ]}
  ],
  "codes": [
    {"key": "FN*", "sets": "feature", "layer": "SITE/FENCES", "colour": "Site Orange", "draw": "line", "linestyle": "Fence", "weight": "0", "group": "Site", "comment": "a fence"},
    {"key": "PG", "sets": "symbol", "hide": false, "symbol": {"name": "Peg", "size": 1.5}},
    {"key": "*", "sets": "text", "text": {}},
    {"key": "TN*", "sets": "surface", "surface": false},
    {"key": "PP*", "sets": "pipe", "pipe": {"justify": "Obvert", "size1": "$PipeDiameter", "active": true}, "attributes": [{"type": "text", "name": "Owner", "value": "Council"}, {"type": "integer", "name": "Zone"}]}
  ]
}
"##;

// ---- the field-complete customisation ---------------------------------------------------

/// A rule with every field a rule has, each at a value no other field of its
/// kind has: two members that held the same value could be crossed in the
/// reader - "underline" read into `strikeout` - and the rule would still come
/// back equal. The model lets any field sit on a rule of any section, and so
/// does the format, so one of these is made for each section.
///
/// Five members are true-or-false, and with every one of them true no two
/// could be told apart. `variant` (0, 1 or 2) settles them so that each pair
/// differs in at least one variant:
///
/// ```text
///   variant        surface  hide   underline  strikeout  italic
///      0            true    false    false      true      true
///      1            false   true     true       false     true
///      2            true    true     true       true      false
/// ```
fn rule_with_every_field(
    key: &str,
    section: SurveySection,
    breakline: SurveyBreakline,
    variant: i32,
) -> SurveyRule {
    SurveyRule {
        key: String::from(key),
        section,
        model: String::from("SURVEY/SERVICES"),
        colour: String::from("sui water"),
        linestyle: String::from("L Everything"),
        weight: String::from("Normal"),
        group: String::from("G - SERVICES"),
        comment: String::from("every field, with a \"quote\" and a \\ backslash"),
        breakline: Some(breakline),
        tinable: Some(variant != 1),
        hide: Some(variant != 0),
        symbol: Some(symbol_of("S Peg", "white", 1.5, 45.0, 0.2, -0.1)),
        text_style: Some(SurveyTextStyle {
            textstyle: String::from("ISO"),
            colour: String::from("red"),
            r#type: String::from("paper"),
            size: 2.5,
            justify_x: String::from("left"),
            justify_y: String::from("bottom"),
            offset: 0.5,
            raise: 0.25,
            angle: 30.0,
            slant: 10.0,
            width_factor: 0.8,
            underline: variant != 0,
            strikeout: variant != 1,
            italic: variant != 2,
            weight: String::from("0"),
        }),
        pipe: Some(pipe_of("Obvert", "diameter", "$PipeDiameter", "0.3", true)),
        vertex_pipe: Some(pipe_of("Invert", "culvert", "0.375", "0.6", true)),
        segment_pipe: Some(pipe_of("Centre", "diameter", "1", "2", true)),
        attributes: vec![
            attribute("text", "Owner", "Council & Co"),
            attribute("integer", "Zone", "1"),
        ],
        vertex_attributes: vec![attribute("integer", "N", "2"), attribute("text", "Old", "")],
        segment_attributes: vec![attribute("text", "S", "$Other")],
    }
}

fn field_complete_customisation() -> Customisation {
    let mut customisation = Customisation {
        name: String::from("Everything"),
        description: String::from("Every member of the format.\nOn two lines."),
        notice: vec![
            String::from("First line of the notice."),
            String::new(),
            String::from("A third, after an empty one."),
        ],
        sources: vec![
            source_note(
                "Everything",
                true,
                true,
                &["Its own notice.", "In two lines."],
            ),
            source_note("Rules Only", false, true, &[]),
            source_note("Other Set", true, false, &["The other set's notice."]),
        ],
        based_on: Some(base("Base Set", "00ff00ff00ff00ff")),
        // Every spelling changed from its default, and one control switched
        // off.
        linework: Some(LineworkCodes {
            start: String::from("S"),
            end: String::from("E"),
            close: String::from("C"),
            arc_start: String::from("PC"),
            arc_end: String::from("PT"),
            join: String::new(),
            rectangle: String::from("BOX"),
        }),
        // Both off, where both are on when left out. (Which is which is pinned
        // by the small customisation, where one is on and the other off.)
        automation: Some(CustomisationAutomation {
            codes_on_survey_import: false,
            linework_on_survey_import: false,
        }),
        ..Customisation::default()
    };
    assert!(
        customisation
            .colours
            .add("sui water", Color::new(0, 112, 255, 255))
            .is_ok()
    );
    assert!(
        customisation
            .colours
            .add("Half Clear", Color::new(1, 2, 3, 128))
            .is_ok()
    );

    // A symbol with every member of a definition and every kind of stroke,
    // and no number at the value it has when left out: a dot of radius 0, an
    // arc from 0 or an anchor at x = 0 would come back the same from a reader
    // that never read that number.
    let mut everything = LineStyle {
        name: String::from("S Everything"),
        group: String::from("Survey/S"),
        units: StyleUnits::Paper,
        at_vertices: true,
        length: 2.5,
        factor: 3.0,
        origin: Point2::new(1.0, -2.0),
        anchor1: Point2::new(0.5, 0.75),
        anchor2: Point2::new(14.0, -0.25),
        stretch_mode: 2,
        cycle_mode: -1,
        source: String::from("Other Set"),
        symbol: true,
        strokes: vec![
            pen("pen 035"),
            move_to(0.0, 0.0),
            draw_to(3.0, 0.1),
            arc(-1.75, 30.0, 180.0),
            circle(0.5),
            dot(0.25),
        ],
        ..LineStyle::default()
    };
    add_text(
        &mut everything,
        text_of(
            "W \"M\" \\ 1",
            90.0,
            1.5,
            "middle-centre",
            "Arial",
            0.85,
            [0.5, -0.3, 0.035],
        ),
    );
    add_text(&mut everything, StrokeText::default()); // a text with nothing said: ["text", {}]
    everything.strokes.push(pen("view_colour"));
    everything.strokes.push(move_to(0.00001, 12345678.125));
    add_definition(&mut customisation, everything);

    add_definition(
        &mut customisation,
        LineStyle {
            name: String::from("L Everything"),
            group: String::from("Survey/L"),
            source: String::from("Everything"),
            strokes: vec![move_to(0.0, 0.0), draw_to(1.0, 0.0)],
            ..LineStyle::default()
        },
    );

    // Drawn at vertices and NOT listed as a symbol: the two are said
    // separately, and neither may be worked out from the other.
    add_definition(
        &mut customisation,
        LineStyle {
            name: String::from("V Marks"),
            at_vertices: true,
            source: String::from("Everything"),
            strokes: vec![dot(0.0)],
            ..LineStyle::default()
        },
    );

    add_definition(
        &mut customisation,
        LineStyle {
            name: String::from("T Gate"),
            units: StyleUnits::TwoPoint,
            anchor2: Point2::new(14.0, 0.0), // one anchor moved is enough for both to be written
            source: String::new(),           // made in a session: of no customisation
            strokes: vec![move_to(0.0, 0.0), draw_to(14.0, 0.0)],
            ..LineStyle::default()
        },
    );

    add_definition(
        &mut customisation,
        LineStyle {
            name: String::from("Empty"),
            source: String::from("Everything"),
            symbol: true,
            ..LineStyle::default()
        },
    );

    // One rule a section, each with every field, the three variants in turn.
    for (key, section, breakline, variant) in [
        ("WM*", SurveySection::Map, SurveyBreakline::Line, 0),
        (
            "AC*",
            SurveySection::VertexSymbol,
            SurveyBreakline::Point,
            1,
        ),
        (
            "1",
            SurveySection::VertexTextStyle,
            SurveyBreakline::Line,
            2,
        ),
        ("*", SurveySection::Pipe, SurveyBreakline::Line, 0),
        ("VP*", SurveySection::VertexPipe, SurveyBreakline::Line, 1),
        ("SP*", SurveySection::SegmentPipe, SurveyBreakline::Line, 2),
        (
            "LP*",
            SurveySection::StringAttribute,
            SurveyBreakline::Line,
            0,
        ),
        (
            "ZPNL",
            SurveySection::VertexAttribute,
            SurveyBreakline::Line,
            1,
        ),
        ("TN*", SurveySection::Tinable, SurveyBreakline::Line, 2),
    ] {
        add_rule(
            &mut customisation,
            rule_with_every_field(key, section, breakline, variant),
        );
    }

    // Optionals that are present and say nothing: not the same as absent.
    add_rule(
        &mut customisation,
        SurveyRule {
            key: String::from("WM*"),
            section: SurveySection::Map,
            tinable: Some(false),
            hide: Some(false),
            symbol: Some(SurveySymbol::default()),
            text_style: Some(SurveyTextStyle::default()),
            pipe: Some(SurveyPipe::default()),
            vertex_pipe: Some(SurveyPipe::default()),
            segment_pipe: Some(SurveyPipe::default()),
            ..SurveyRule::default()
        },
    );

    // A rule that says nothing but what it must.
    add_rule(
        &mut customisation,
        SurveyRule {
            key: String::from("WM*"),
            section: SurveySection::Map,
            ..SurveyRule::default()
        },
    );
    customisation
}

fn count(text: &str, piece: &str) -> usize {
    text.matches(piece).count()
}

mod customisation_format {
    use super::*;

    // ---- round trip -------------------------------------------------------------------

    #[test]
    fn every_field_of_the_model_comes_back_as_it_was_written() {
        let original = field_complete_customisation();
        let text = write_customisation(&original);
        let again = read_customisation(&text);

        assert!(again == original, "{text}");
        // Said member by member as well, so that a failure says where.
        assert_eq!(again.name, original.name);
        assert_eq!(again.description, original.description);
        assert_eq!(again.notice, original.notice);
        assert_eq!(again.sources, original.sources);
        assert_eq!(again.based_on, original.based_on);
        assert_eq!(again.colours, original.colours);
        assert_eq!(again.linework, original.linework);
        assert_eq!(again.automation, original.automation);
        assert_eq!(again.library.all(), original.library.all());
        assert_eq!(
            again.map.rules(),
            original.map.rules(),
            "the rules, in their order"
        );
    }

    // The round trip above proves nothing about a member the fixture never
    // writes. Every member name and every word of the format, listed here by
    // hand from docs/customisation.md, is in the fixture's text.
    #[test]
    fn the_field_complete_fixture_writes_every_member_and_word_the_format_has() {
        let text = write_customisation(&field_complete_customisation());
        for member in [
            // the file
            "format",
            "version",
            "name",
            "description",
            "notice",
            "sources",
            "basedOn",
            "colours",
            "linework",
            "automation",
            "linestyles",
            "symbols",
            "codes",
            // a source and the base
            "definitions",
            "rules",
            "digest",
            // linework and automation
            "start",
            "end",
            "close",
            "arcStart",
            "arcEnd",
            "join",
            "rectangle",
            "codesOnSurveyImport",
            "lineworkOnSurveyImport",
            // a definition
            "group",
            "units",
            "atVertices",
            "length",
            "factor",
            "origin",
            "anchors",
            "stretchMode",
            "cycleMode",
            "from",
            "strokes",
            // a text stroke
            "text",
            "angle",
            "height",
            "justify",
            "font",
            "widthFactor",
            "extra",
            // a rule
            "key",
            "sets",
            "layer",
            "colour",
            "draw",
            "linestyle",
            "weight",
            "comment",
            "surface",
            "hide",
            "symbol",
            "pipe",
            "vertexPipe",
            "segmentPipe",
            "attributes",
            "vertexAttributes",
            "segmentAttributes",
            // a rule's symbol, text and pipes, and an attribute
            "size",
            "rotation",
            "offset",
            "raise",
            "style",
            "justifyX",
            "justifyY",
            "slant",
            "underline",
            "strikeout",
            "italic",
            "shape",
            "size1",
            "size2",
            "active",
            "type",
            "value",
        ] {
            assert!(
                text.contains(&format!("\"{member}\": ")),
                "the fixture never writes \"{member}\""
            );
        }
        for stroke in ["move", "draw", "arc", "circle", "dot", "pen", "text"] {
            assert!(text.contains(&format!("[\"{stroke}\", ")), "{stroke}");
        }
        for sets in [
            "feature",
            "symbol",
            "text",
            "pipe",
            "vertexPipe",
            "segmentPipe",
            "attributes",
            "vertexAttributes",
            "surface",
        ] {
            assert!(text.contains(&format!("\"sets\": \"{sets}\"")), "{sets}");
        }
        // "world" is the default and so is never written; the two others are.
        assert!(text.contains("\"units\": \"paper\""));
        assert!(text.contains("\"units\": \"twoPoint\""));
        assert!(!text.contains("\"units\": \"world\""));
        assert!(text.contains("\"draw\": \"line\""));
        assert!(text.contains("\"draw\": \"point\""));
        assert!(text.contains("\"type\": \"text\""));
        assert!(text.contains("\"type\": \"integer\""));
        // The optionals that are present and say nothing, each written as {}:
        // the symbol, the text and the three pipes of the eleventh rule.
        assert!(
            text.contains(concat!(
                r#""surface": false, "hide": false, "symbol": {}, "text": {}, "#,
                r#""pipe": {}, "vertexPipe": {}, "segmentPipe": {}}"#
            )),
            "{text}"
        );
        // A definition made in a session keeps "no source" under a named file.
        assert!(text.contains("\"from\": \"\""));
    }

    // The members of a definition and of each kind of stroke, in the order the
    // format fixes, for the symbol of the fixture that has every one of them.
    // By hand: the file's four blanks before the definition, six before a
    // stroke; 0.00001 in its shorter form with an exponent; "from" because its
    // source is not the file's name; a text with nothing said as {}.
    #[test]
    fn a_definition_with_every_member_is_written_in_the_order_the_format_fixes() {
        let text = write_customisation(&field_complete_customisation());
        let expected = r##"
    {"name": "S Everything", "group": "Survey/S", "units": "paper", "atVertices": true, "length": 2.5, "factor": 3, "origin": [1, -2], "anchors": [[0.5, 0.75], [14, -0.25]], "stretchMode": 2, "cycleMode": -1, "from": "Other Set", "strokes": [
      ["pen", "pen 035"],
      ["move", 0, 0],
      ["draw", 3, 0.1],
      ["arc", -1.75, 30, 180],
      ["circle", 0.5],
      ["dot", 0.25],
      ["text", {"text": "W \"M\" \\ 1", "angle": 90, "height": 1.5, "justify": "middle-centre", "font": "Arial", "widthFactor": 0.85, "extra": [0.5, -0.3, 0.035]}],
      ["text", {}],
      ["pen", "view_colour"],
      ["move", 1e-05, 12345678.125]
    ]}
  ],"##;
        assert!(text.contains(expected), "{text}");

        // The linestyle drawn at vertices says so, and sits in "linestyles": the
        // last of the three there, by name. Its source is the file's own name,
        // so it has no "from"; its dot of radius 0 is written with its 0, since
        // a stroke's values are never left out.
        let at_vertices = r##"
    {"name": "V Marks", "atVertices": true, "strokes": [
      ["dot", 0]
    ]}
  ],
  "symbols": ["##;
        assert!(text.contains(at_vertices), "{text}");
    }

    // The same for a rule: every member a rule has, and every member of its
    // symbol, its text, its three pipes and its three attribute lists, on the
    // one line a rule is written on, for each of the fixture's three variants
    // (its first three rules). A member that is false where false is the value
    // left out - a text's flags - is not written; "surface" and "hide" are
    // optionals, written whenever present. An attribute's empty value is left
    // out.
    #[test]
    fn a_rule_with_every_field_is_written_in_the_order_the_format_fixes() {
        let text = write_customisation(&field_complete_customisation());
        // What the three have in common, by hand, in the three stretches the
        // varying members sit between.
        let up_to_surface = r##""layer": "SURVEY/SERVICES", "colour": "sui water", "draw": "DRAW", "linestyle": "L Everything", "weight": "Normal", "group": "G - SERVICES", "comment": "every field, with a \"quote\" and a \\ backslash", "##;
        let up_to_flags = concat!(
            r##""symbol": {"name": "S Peg", "colour": "white", "size": 1.5, "rotation": 45, "offset": 0.2, "raise": -0.1}, "##,
            r##""text": {"style": "ISO", "colour": "red", "units": "paper", "size": 2.5, "justifyX": "left", "justifyY": "bottom", "offset": 0.5, "raise": 0.25, "angle": 30, "slant": 10, "widthFactor": 0.8, "##
        );
        let from_weight = concat!(
            r##""weight": "0"}, "##,
            r##""pipe": {"justify": "Obvert", "shape": "diameter", "size1": "$PipeDiameter", "size2": "0.3", "active": true}, "##,
            r##""vertexPipe": {"justify": "Invert", "shape": "culvert", "size1": "0.375", "size2": "0.6", "active": true}, "##,
            r##""segmentPipe": {"justify": "Centre", "shape": "diameter", "size1": "1", "size2": "2", "active": true}, "##,
            r##""attributes": [{"type": "text", "name": "Owner", "value": "Council & Co"}, {"type": "integer", "name": "Zone", "value": "1"}], "##,
            r##""vertexAttributes": [{"type": "integer", "name": "N", "value": "2"}, {"type": "text", "name": "Old"}], "##,
            r##""segmentAttributes": [{"type": "text", "name": "S", "value": "$Other"}]},"##
        );
        for (key_and_sets, draw, switches, flags) in [
            (
                r#""key": "WM*", "sets": "feature", "#,
                "line",
                r#""surface": true, "hide": false, "#,
                r#""strikeout": true, "italic": true, "#,
            ),
            (
                r#""key": "AC*", "sets": "symbol", "#,
                "point",
                r#""surface": false, "hide": true, "#,
                r#""underline": true, "italic": true, "#,
            ),
            (
                r#""key": "1", "sets": "text", "#,
                "line",
                r#""surface": true, "hide": true, "#,
                r#""underline": true, "strikeout": true, "#,
            ),
        ] {
            let common = up_to_surface.replacen("DRAW", draw, 1);
            let expected = format!(
                "\n    {{{key_and_sets}{common}{switches}{up_to_flags}{flags}{from_weight}"
            );
            assert!(text.contains(&expected), "{expected}\n\n{text}");
        }
    }

    #[test]
    fn rules_keep_their_order_because_order_is_precedence() {
        // Three rules of one key and one section, told apart by their layers:
        // which one a code takes its layer from is decided by their order
        // alone.
        let mut customisation = named_customisation("Order");
        for layer in ["SECOND", "FIRST", "THIRD"] {
            add_rule(
                &mut customisation,
                SurveyRule {
                    key: String::from("WM*"),
                    section: SurveySection::Map,
                    model: String::from(layer),
                    ..SurveyRule::default()
                },
            );
        }
        let again = read_customisation(write_customisation(&customisation));
        assert_eq!(again.map.size(), 3);
        assert_eq!(again.map.rules()[0].model, "SECOND");
        assert_eq!(again.map.rules()[1].model, "FIRST");
        assert_eq!(again.map.rules()[2].model, "THIRD");
        assert_eq!(
            again.map.lookup("WM01").resolved.model,
            "SECOND",
            "the earliest rule wins"
        );
    }

    #[test]
    fn writing_what_was_read_gives_the_same_bytes() {
        for customisation in [
            field_complete_customisation(),
            small_customisation(),
            named_customisation("Bare"),
        ] {
            let first = write_customisation(&customisation);
            assert_eq!(
                write_customisation(&customisation),
                first,
                "one value, two writes"
            );
            assert_eq!(
                write_customisation(&read_customisation(&first)),
                first,
                "{}",
                customisation.name
            );
        }
    }

    // ---- the layout -------------------------------------------------------------------

    #[test]
    fn a_small_customisation_is_written_byte_for_byte_as_the_layout_says() {
        assert_eq!(
            write_customisation(&small_customisation()),
            SMALL_CUSTOMISATION_TEXT
        );
    }

    #[test]
    fn the_text_of_the_small_customisation_reads_as_the_value_it_was_written_from() {
        assert!(read_customisation(SMALL_CUSTOMISATION_TEXT) == small_customisation());
    }

    // The example docs/customisation.md prints under "A worked example",
    // character for character: a file a person may well copy to start their
    // own. It reads as what it says, and it is laid out as the writer lays a
    // file out. The C++ case's copy of it is held to the document by the `docs`
    // test (tools/check_docs.py, check_worked_example); this is a copy of that.
    #[test]
    fn the_worked_example_of_the_document_is_written_as_it_is_printed() {
        let example = r##"{
  "format": "katana-customisation",
  "version": 1,
  "name": "Site",
  "colours": {
    "sui test water": "#0070FF"
  },
  "linestyles": [
    {"name": "TEST Water Main", "group": "Test/Water", "units": "paper", "length": 12, "strokes": [
      ["move", 0, 0],
      ["draw", 8, 0],
      ["move", 10, -0.75],
      ["text", {"text": "W", "height": 1.5, "justify": "middle-centre"}]
    ]}
  ],
  "symbols": [
    {"name": "TEST Bollard", "group": "Test/Bollards", "units": "paper", "strokes": [
      ["circle", 0.75],
      ["dot", 0]
    ]}
  ],
  "codes": [
    {"key": "WM*", "sets": "feature", "layer": "TEST SERVICES", "colour": "sui test water", "draw": "line", "linestyle": "TEST Water Main"},
    {"key": "AC*", "sets": "symbol", "symbol": {"name": "TEST Bollard"}},
    {"key": "*", "sets": "attributes", "attributes": [{"type": "text", "name": "Surveyed by"}]}
  ]
}
"##;
        let read = read_customisation(example);
        assert_eq!(write_customisation(&read), example);

        // What the example says, in the model's terms.
        assert_eq!(read.name, "Site");
        assert_eq!(
            read.colours.find("sui test water"),
            Some(Color::new(0x00, 0x70, 0xFF, 255))
        );
        let main = read
            .library
            .find("TEST Water Main")
            .expect("the water main");
        assert!(!main.symbol);
        assert_eq!(main.units, StyleUnits::Paper);
        assert_eq!(main.length, 12.0);
        assert_eq!(main.texts.len(), 1);
        assert_eq!(main.texts[0].text, "W");
        let bollard = read.library.find("TEST Bollard").expect("the bollard");
        assert!(bollard.symbol);
        // A water main: WM01 goes on its layer, as a line, in its linestyle;
        // and the `*` rule's attribute reaches it too.
        let water_main = read.map.lookup("WM01");
        assert_eq!(water_main.resolved.model, "TEST SERVICES");
        assert_eq!(water_main.resolved.breakline, Some(SurveyBreakline::Line));
        assert_eq!(water_main.resolved.linestyle, "TEST Water Main");
        assert_eq!(water_main.resolved.attributes.len(), 1);
        assert_eq!(water_main.resolved.attributes[0].name, "Surveyed by");
        let symbol = read
            .map
            .lookup("AC3")
            .resolved
            .symbol
            .expect("the bollard's symbol");
        assert_eq!(symbol.style, "TEST Bollard");
    }

    #[test]
    fn text_is_escaped_as_json_and_nothing_else_is() {
        // A quote and a backslash take a backslash; a control character below
        // U+0020 is \n, \t or \u00XX with lower-case digits (U+001F here); '/'
        // and what is not ASCII are written as they are (the micro sign is C2
        // B5 in UTF-8).
        let customisation = Customisation {
            name: String::from("E"),
            description: String::from("a \"b\" c\\d\ne\tf\u{1F}g/h \u{B5}m"),
            ..Customisation::default()
        };
        let text = write_customisation(&customisation);
        assert!(
            text.contains("\"description\": \"a \\\"b\\\" c\\\\d\\ne\\tf\\u001fg/h \u{B5}m\""),
            "{text}"
        );
        assert_eq!(
            read_customisation(&text).description,
            customisation.description
        );
    }

    #[test]
    fn numbers_are_the_shortest_text_that_reads_back_and_never_padded() {
        let mut customisation = named_customisation("N");
        add_definition(
            &mut customisation,
            LineStyle {
                name: String::from("N"),
                source: String::from("N"),
                strokes: vec![
                    move_to(0.1, 100.0),
                    draw_to(0.00001, 1e9),
                    draw_to(12345678.125, -2.5),
                    arc(1.0 / 3.0, 0.0, 360.0),
                ],
                ..LineStyle::default()
            },
        );
        let text = write_customisation(&customisation);
        // 0.00001 and 1e9 - the largest number the format holds - in the form
        // with an exponent, which is the shorter ("1e+09" is 5 characters,
        // "1000000000" 10); a third is the 16 digits it takes to say which
        // double it is.
        assert!(text.contains("[\"move\", 0.1, 100]"), "{text}");
        assert!(text.contains("[\"draw\", 1e-05, 1e+09]"), "{text}");
        assert!(text.contains("[\"draw\", 12345678.125, -2.5]"), "{text}");
        assert!(
            text.contains("[\"arc\", 0.3333333333333333, 0, 360]"),
            "{text}"
        );
    }

    // ---- degenerate files -------------------------------------------------------------

    #[test]
    fn a_customisation_of_its_three_required_members_is_empty_and_says_nothing_of_the_rest() {
        let read =
            read_customisation(r#"{"format": "katana-customisation", "version": 1, "name": "T"}"#);
        assert_eq!(read.name, "T");
        assert!(read.description.is_empty());
        assert!(read.notice.is_empty());
        assert!(read.sources.is_empty());
        assert!(read.based_on.is_none());
        assert!(read.colours.empty());
        assert!(
            read.linework.is_none(),
            "absent: it says nothing about the control codes"
        );
        assert!(read.automation.is_none());
        assert!(read.library.empty());
        assert!(read.map.empty());

        // And it is written back as exactly those three, one a line.
        assert_eq!(
            write_customisation(&read),
            "{\n  \"format\": \"katana-customisation\",\n  \"version\": 1,\n  \"name\": \"T\"\n}\n"
        );
    }

    #[test]
    fn empty_lists_are_the_same_as_lists_left_out() {
        let empty = read_customisation(customisation_with(
            r#""description": "", "notice": [], "sources": [], "colours": {}, "linestyles": [],
           "symbols": [], "codes": []"#,
        ));
        assert!(empty == read_customisation(customisation_with("")));
    }

    #[test]
    fn linework_and_automation_present_but_empty_are_the_defaults_and_not_absent() {
        let read = read_customisation(customisation_with(r#""linework": {}, "automation": {}"#));
        let linework = read.linework.as_ref().expect("linework");
        assert_eq!(
            *linework,
            LineworkCodes::default(),
            "ST, END, CL, BC, EC, JPN and RECT"
        );
        assert_eq!(linework.start, "ST");
        assert_eq!(linework.rectangle, "RECT");
        let automation = read.automation.expect("automation");
        assert!(automation.codes_on_survey_import);
        assert!(automation.linework_on_survey_import);

        // A member left out of one of them is its default; the others are as
        // given.
        let partial = read_customisation(customisation_with(
            r#""linework": {"close": "C", "join": ""}, "automation": {"lineworkOnSurveyImport": false}"#,
        ));
        let linework = partial.linework.as_ref().expect("linework");
        assert_eq!(linework.close, "C");
        assert_eq!(
            linework.join, "",
            "an empty spelling switches the control off"
        );
        assert_eq!(linework.end, "END");
        let automation = partial.automation.expect("automation");
        assert!(automation.codes_on_survey_import);
        assert!(!automation.linework_on_survey_import);
        // And the other switch on its own, so that neither is read as the
        // other.
        let other = read_customisation(customisation_with(
            r#""automation": {"codesOnSurveyImport": false}"#,
        ));
        let automation = other.automation.expect("automation");
        assert!(!automation.codes_on_survey_import);
        assert!(automation.linework_on_survey_import);

        // Each of the seven spellings is the one its own member gives: seven
        // different words, one a control.
        let seven = read_customisation(customisation_with(
            r#""linework": {"start": "a", "end": "b", "close": "c", "arcStart": "d", "arcEnd": "e",
                        "join": "f", "rectangle": "g"}"#,
        ));
        let linework = seven.linework.expect("linework");
        assert_eq!(linework.start, "a");
        assert_eq!(linework.end, "b");
        assert_eq!(linework.close, "c");
        assert_eq!(linework.arc_start, "d");
        assert_eq!(linework.arc_end, "e");
        assert_eq!(linework.join, "f");
        assert_eq!(linework.rectangle, "g");
    }

    #[test]
    fn a_definition_with_no_strokes_reads_with_or_without_an_empty_list() {
        let read = read_customisation(customisation_with(
            r#""linestyles": [{"name": "Bare"}, {"name": "Listed", "strokes": []}]"#,
        ));
        assert_eq!(read.library.size(), 2);
        for name in ["Bare", "Listed"] {
            let definition = read.library.find(name).expect(name);
            assert!(definition.strokes.is_empty());
            assert!(definition.texts.is_empty());
            assert!(!definition.symbol);
            assert_eq!(
                definition.source, "T",
                "the file's name, when it says no \"from\""
            );
        }
    }

    // Every member a file may leave out, written out at the value it has when
    // left out - the defaults docs/customisation.md lists - reads as if it were
    // not there. This is the reader's half of "a member at its default is
    // omitted", for the words the writer never writes ("units": "world").
    #[test]
    fn every_member_written_out_at_its_default_reads_as_if_left_out() {
        let spelled = read_customisation(customisation_with(
            r#"
        "linestyles": [{"name": "D", "group": "", "units": "world", "atVertices": false,
                        "length": 0, "factor": 1, "origin": [0, 0], "anchors": [[0, 0], [0, 0]],
                        "stretchMode": 0, "cycleMode": 0, "from": "T",
                        "strokes": [["text", {"text": "", "angle": 0, "height": 0, "justify": "",
                                              "font": "", "widthFactor": 1, "extra": [0, 0, 0]}]]}],
        "codes": [{"key": "A", "sets": "feature", "layer": "", "colour": "", "linestyle": "",
                   "weight": "", "group": "", "comment": "",
                   "symbol": {"name": "", "colour": "", "size": 0, "rotation": 0, "offset": 0,
                              "raise": 0},
                   "text": {"style": "", "colour": "", "units": "", "size": 0, "justifyX": "",
                            "justifyY": "", "offset": 0, "raise": 0, "angle": 0, "slant": 0,
                            "widthFactor": 1, "underline": false, "strikeout": false,
                            "italic": false, "weight": ""},
                   "pipe": {"justify": "", "shape": "", "size1": "", "size2": "",
                            "active": false},
                   "attributes": [], "vertexAttributes": [], "segmentAttributes": []}],
        "sources": [{"name": "S", "definitions": false, "rules": false, "notice": []}]"#,
        ));
        let left = read_customisation(customisation_with(
            r#"
        "linestyles": [{"name": "D", "strokes": [["text", {}]]}],
        "codes": [{"key": "A", "sets": "feature", "symbol": {}, "text": {}, "pipe": {}}],
        "sources": [{"name": "S"}]"#,
        ));
        assert!(spelled == left);

        // And those defaults are the model's own: a definition and a rule made
        // in code with nothing set but what the file gives.
        let mut definition = LineStyle {
            name: String::from("D"),
            source: String::from("T"),
            ..LineStyle::default()
        };
        add_text(&mut definition, StrokeText::default());
        assert_eq!(left.library.size(), 1);
        assert_eq!(*left.library.find("D").expect("D"), definition);
        let rule = SurveyRule {
            key: String::from("A"),
            symbol: Some(SurveySymbol::default()),
            text_style: Some(SurveyTextStyle::default()),
            pipe: Some(SurveyPipe::default()),
            ..SurveyRule::default()
        };
        assert_eq!(left.map.size(), 1);
        assert_eq!(left.map.rules()[0], rule);
        assert_eq!(left.library.find("D").expect("D").factor, 1.0);
        assert_eq!(
            left.library.find("D").expect("D").texts[0].width_factor,
            1.0
        );
        assert_eq!(
            left.map.rules()[0]
                .text_style
                .as_ref()
                .expect("a text style")
                .width_factor,
            1.0
        );
    }

    #[test]
    fn the_array_a_definition_sits_in_says_whether_it_is_a_symbol() {
        let read = read_customisation(customisation_with(
            r#""linestyles": [{"name": "Line"}], "symbols": [{"name": "Mark"}]"#,
        ));
        assert!(!read.library.find("Line").expect("Line").symbol);
        assert!(read.library.find("Mark").expect("Mark").symbol);
        // Not whether it is drawn at vertices: that is said separately, and a
        // symbol need not be.
        assert!(!read.library.find("Mark").expect("Mark").at_vertices);

        // Nor the other way about: a definition drawn at vertices that sits in
        // "linestyles" is not a symbol for that. All four combinations exist.
        let vertex = read_customisation(customisation_with(
            r#""linestyles": [{"name": "V", "atVertices": true}],
           "symbols": [{"name": "S", "atVertices": true}]"#,
        ));
        let v = vertex.library.find("V").expect("V");
        assert!(v.at_vertices);
        assert!(!v.symbol);
        let s = vertex.library.find("S").expect("S");
        assert!(s.at_vertices);
        assert!(s.symbol);
        // Written back, each is in the list it was read from and says for
        // itself that it is drawn at vertices.
        assert_eq!(
            write_customisation(&vertex),
            concat!(
                "{\n",
                "  \"format\": \"katana-customisation\",\n",
                "  \"version\": 1,\n",
                "  \"name\": \"T\",\n",
                "  \"linestyles\": [\n",
                "    {\"name\": \"V\", \"atVertices\": true}\n",
                "  ],\n",
                "  \"symbols\": [\n",
                "    {\"name\": \"S\", \"atVertices\": true}\n",
                "  ]\n",
                "}\n"
            )
        );
    }

    // Each of the nine words a rule's "sets" may be is one section of the
    // model, and that section is written as that word. Taken from the format's
    // own list (docs/customisation.md, "A rule"), which gives them in the order
    // of the sections and says what each is about: "feature" is where a code
    // goes and how it is drawn - the layer, colour, line or point and
    // linestyle the model's Map section holds - "attributes" are those on the
    // string and "surface" is whether the code goes into one, which the model
    // calls tinable. The round trip alone cannot see two of these words
    // exchanged, because the reader and the writer share one table of them.
    #[test]
    fn each_word_a_rule_sets_is_one_section_of_the_model_and_is_written_as_that_word() {
        let words = [
            ("feature", SurveySection::Map),
            ("symbol", SurveySection::VertexSymbol),
            ("text", SurveySection::VertexTextStyle),
            ("pipe", SurveySection::Pipe),
            ("vertexPipe", SurveySection::VertexPipe),
            ("segmentPipe", SurveySection::SegmentPipe),
            ("attributes", SurveySection::StringAttribute),
            ("vertexAttributes", SurveySection::VertexAttribute),
            ("surface", SurveySection::Tinable),
        ];
        assert_eq!(words.len(), 9);
        for (word, section) in words {
            let read = read_customisation(customisation_with(&format!(
                "\"codes\": [{{\"key\": \"A\", \"sets\": \"{word}\"}}]"
            )));
            assert_eq!(read.map.size(), 1, "{word}");
            assert_eq!(read.map.rules()[0].section, section, "{word}");

            let mut made = named_customisation("T");
            add_rule(
                &mut made,
                SurveyRule {
                    key: String::from("A"),
                    section,
                    ..SurveyRule::default()
                },
            );
            assert_eq!(
                write_customisation(&made),
                format!(
                    "{{\n  \"format\": \"katana-customisation\",\n  \"version\": 1,\n  \"name\": \"T\",\n  \"codes\": [\n    {{\"key\": \"A\", \"sets\": \"{word}\"}}\n  ]\n}}\n"
                ),
                "{word}"
            );
        }
    }

    // The reference rules repeat themselves - 446 of the 1,624 say again, field
    // for field, what an earlier rule said - and the format keeps a
    // customisation as it is: two rules alike are two rules, each on its own
    // line, and the second keeps its place in the order.
    #[test]
    fn two_rules_alike_in_every_field_are_both_kept_and_both_written() {
        let read = read_customisation(customisation_with(
            r#""codes": [{"key": "WM*", "sets": "feature", "layer": "SURVEY"},
                     {"key": "AC*", "sets": "symbol"},
                     {"key": "WM*", "sets": "feature", "layer": "SURVEY"}]"#,
        ));
        assert_eq!(read.map.size(), 3);
        assert_eq!(read.map.rules()[0], read.map.rules()[2]);
        assert_eq!(read.map.rules()[1].key, "AC*");
        assert_eq!(
            write_customisation(&read),
            concat!(
                "{\n",
                "  \"format\": \"katana-customisation\",\n",
                "  \"version\": 1,\n",
                "  \"name\": \"T\",\n",
                "  \"codes\": [\n",
                "    {\"key\": \"WM*\", \"sets\": \"feature\", \"layer\": \"SURVEY\"},\n",
                "    {\"key\": \"AC*\", \"sets\": \"symbol\"},\n",
                "    {\"key\": \"WM*\", \"sets\": \"feature\", \"layer\": \"SURVEY\"}\n",
                "  ]\n",
                "}\n"
            )
        );
    }

    // "In name order" is the order of the names' bytes, as the library keeps
    // them: every upper-case letter is before every lower-case one ('Z' is 5A,
    // 'a' is 61), so "Zinc" is before "apple".
    #[test]
    fn definitions_are_written_in_the_order_of_their_bytes_so_upper_case_comes_first() {
        let mut customisation = named_customisation("T");
        for name in ["apple", "Zinc", "zinc", "Apple"] {
            add_definition(
                &mut customisation,
                LineStyle {
                    name: String::from(name),
                    source: String::from("T"),
                    ..LineStyle::default()
                },
            );
        }
        add_definition(
            &mut customisation,
            LineStyle {
                name: String::from("Mark"),
                source: String::from("T"),
                symbol: true,
                ..LineStyle::default()
            },
        );
        assert_eq!(
            write_customisation(&customisation),
            concat!(
                "{\n",
                "  \"format\": \"katana-customisation\",\n",
                "  \"version\": 1,\n",
                "  \"name\": \"T\",\n",
                "  \"linestyles\": [\n",
                "    {\"name\": \"Apple\"},\n",
                "    {\"name\": \"Zinc\"},\n",
                "    {\"name\": \"apple\"},\n",
                "    {\"name\": \"zinc\"}\n",
                "  ],\n",
                "  \"symbols\": [\n",
                "    {\"name\": \"Mark\"}\n",
                "  ]\n",
                "}\n"
            )
        );
    }

    #[test]
    fn a_definitions_source_is_its_from_or_else_the_files_name() {
        let read = read_customisation(customisation_with(
            r#""linestyles": [{"name": "Own"}, {"name": "Other", "from": "Site"},
                          {"name": "Session", "from": ""}]"#,
        ));
        assert_eq!(read.library.find("Own").expect("Own").source, "T");
        assert_eq!(read.library.find("Other").expect("Other").source, "Site");
        assert_eq!(
            read.library.find("Session").expect("Session").source,
            "",
            "made in a session: of no customisation"
        );

        // Written back, only the two that differ from the name say so.
        let text = write_customisation(&read);
        assert_eq!(count(&text, "\"from\": "), 2, "{text}");
    }

    // ---- negative zero and the reach of the numbers -----------------------------------

    #[test]
    fn negative_zero_survives_wherever_a_number_is() {
        let mut customisation = named_customisation("Z");
        let mut definition = LineStyle {
            name: String::from("Z"),
            source: String::from("Z"),
            length: -0.0, // equal to the default 0 by ==, and still not left out
            origin: Point2::new(-0.0, 0.0),
            anchor1: Point2::new(0.0, -0.0),
            anchor2: Point2::new(-0.0, 0.0),
            strokes: vec![
                move_to(-0.0, 0.0),
                arc(-0.0, -0.0, 0.0),
                circle(-0.0),
                dot(-0.0),
            ],
            ..LineStyle::default()
        };
        add_text(
            &mut definition,
            text_of("", -0.0, -0.0, "", "", -0.0, [-0.0, 0.0, -0.0]),
        );
        add_definition(&mut customisation, definition);
        add_rule(
            &mut customisation,
            SurveyRule {
                key: String::from("Z"),
                symbol: Some(symbol_of("", "", -0.0, -0.0, -0.0, -0.0)),
                text_style: Some(SurveyTextStyle {
                    size: -0.0,
                    offset: -0.0,
                    raise: -0.0,
                    angle: -0.0,
                    slant: -0.0,
                    width_factor: -0.0,
                    ..SurveyTextStyle::default()
                }),
                ..SurveyRule::default()
            },
        );

        // Every number a definition and a rule have, each written -0.0 where it
        // is negative zero and 0 where it is zero: by hand from the layout, with
        // nothing left out for being "equal to" a default of 0 or 1.
        let text = write_customisation(&customisation);
        assert_eq!(
            text,
            r##"{
  "format": "katana-customisation",
  "version": 1,
  "name": "Z",
  "linestyles": [
    {"name": "Z", "length": -0.0, "origin": [-0.0, 0], "anchors": [[0, -0.0], [-0.0, 0]], "strokes": [
      ["move", -0.0, 0],
      ["arc", -0.0, -0.0, 0],
      ["circle", -0.0],
      ["dot", -0.0],
      ["text", {"angle": -0.0, "height": -0.0, "widthFactor": -0.0, "extra": [-0.0, 0, -0.0]}]
    ]}
  ],
  "codes": [
    {"key": "Z", "sets": "feature", "symbol": {"size": -0.0, "rotation": -0.0, "offset": -0.0, "raise": -0.0}, "text": {"size": -0.0, "offset": -0.0, "raise": -0.0, "angle": -0.0, "slant": -0.0, "widthFactor": -0.0}}
  ]
}
"##
        );

        let again = read_customisation(&text);
        let read = again.library.find("Z").expect("Z");
        assert!(read.length.is_sign_negative());
        assert!(read.origin.x.is_sign_negative());
        assert!(!read.origin.y.is_sign_negative());
        assert!(!read.anchor1.x.is_sign_negative());
        assert!(read.anchor1.y.is_sign_negative());
        assert!(read.anchor2.x.is_sign_negative());
        assert!(!read.anchor2.y.is_sign_negative());
        assert_eq!(read.strokes.len(), 5);
        assert!(read.strokes[0].point.x.is_sign_negative());
        assert!(!read.strokes[0].point.y.is_sign_negative());
        assert!(read.strokes[1].radius.is_sign_negative());
        assert!(read.strokes[1].start_angle.is_sign_negative());
        assert!(!read.strokes[1].end_angle.is_sign_negative());
        assert!(read.strokes[2].radius.is_sign_negative());
        assert!(read.strokes[3].radius.is_sign_negative());
        assert_eq!(read.texts.len(), 1);
        assert!(read.texts[0].angle.is_sign_negative());
        assert!(read.texts[0].height.is_sign_negative());
        assert!(read.texts[0].width_factor.is_sign_negative());
        assert!(read.texts[0].unnamed[0].is_sign_negative());
        assert!(!read.texts[0].unnamed[1].is_sign_negative());
        assert!(read.texts[0].unnamed[2].is_sign_negative());
        assert_eq!(again.map.size(), 1);
        let back = &again.map.rules()[0];
        let symbol = back.symbol.as_ref().expect("a symbol");
        assert!(symbol.size.is_sign_negative());
        assert!(symbol.rotation.is_sign_negative());
        assert!(symbol.offset.is_sign_negative());
        assert!(symbol.raise.is_sign_negative());
        let style = back.text_style.as_ref().expect("a text style");
        assert!(style.size.is_sign_negative());
        assert!(style.offset.is_sign_negative());
        assert!(style.raise.is_sign_negative());
        assert!(style.angle.is_sign_negative());
        assert!(style.slant.is_sign_negative());
        assert!(style.width_factor.is_sign_negative());
    }

    // A property over doubles: whatever finite number a customisation holds
    // comes back with the same BITS, by every way the reader takes a number in.
    // The classes it claims to reach are counted and each count is asserted,
    // because a generator that never made a subnormal would pass this without
    // having tried one.
    #[test]
    fn every_finite_double_within_the_bound_comes_back_bit_for_bit() {
        // The cases a text form is most likely to get wrong, named.
        let mut values: Vec<f64> = vec![
            0.0,
            -0.0,
            0.1,
            1.0 / 3.0,
            5e-324,                  // the smallest subnormal
            2.2250738585072009e-308, // the largest subnormal
            2.2250738585072014e-308, // the smallest normal
            1e9,                     // the largest the format holds
            -1e9,
            999999999.9999999, // the double just below it
            536870912.0,       // 2^29: the last power of two within it
            123456789.0,       // written in full, 9 digits
            0.000001,
            12345678.125,
            -0.47350000000000003,
        ];
        // Then any bit pattern at all: SplitMix64 (Steele, Lea and Flood, 2014)
        // from a fixed seed, so the run is the same every time.
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        while values.len() < 4000 {
            let value = f64::from_bits(next());
            // Only what the format holds: a number larger in size than a
            // billion is refused, and is tried in customisation_bounds.rs. About
            // half of all bit patterns are within it, so the run reaches as many
            // cases as it did.
            if value.is_finite() && value.abs() <= 1.0e9 {
                values.push(value);
            }
        }
        // And subnormals made on purpose: a random pattern has one in 2,047.
        for _ in 0..50 {
            values.push(f64::from_bits(next() & 0x800F_FFFF_FFFF_FFFF));
        }
        // Taken two at a time and three at a time below, so a multiple of six.
        while !values.len().is_multiple_of(6) {
            values.push(1.5);
        }

        // Every value goes down each of the three ways the reader takes a
        // number in, which share no code past the JSON reader: a stroke's
        // values, made into doubles as the text is parsed; a point read from
        // the tree (a definition's origin and two anchors); a member read from
        // the tree (the angle and width factor of a text stroke's object and its
        // three "extra" numbers, a rule's symbol - rotation, offset, raise - and
        // its text - offset, raise, angle, slant, width factor). No member is
        // used that the model restricts (a length, a factor, a size), so any
        // finite double is a value each of these may hold.
        // A multiple of six: nothing is left over by either.
        let (pairs, _) = values.as_chunks::<2>();
        let (triples, _) = values.as_chunks::<3>();
        let mut customisation = named_customisation("D");
        let mut definition = LineStyle {
            name: String::from("D"),
            source: String::from("D"),
            ..LineStyle::default()
        };
        for &[x, y] in pairs {
            definition.strokes.push(move_to(x, y));
        }
        for &[a, b, c] in triples {
            let text = StrokeText {
                angle: a,
                width_factor: b,
                unnamed: [a, b, c],
                ..StrokeText::default()
            };
            add_text(&mut definition, text);
        }
        add_definition(&mut customisation, definition);
        for (i, &[x, y]) in pairs.iter().enumerate() {
            add_definition(
                &mut customisation,
                LineStyle {
                    name: format!("P{i}"),
                    source: String::from("D"),
                    origin: Point2::new(x, y),
                    anchor1: Point2::new(y, x),
                    anchor2: Point2::new(x, x),
                    ..LineStyle::default()
                },
            );
        }
        for &[a, b, c] in triples {
            add_rule(
                &mut customisation,
                SurveyRule {
                    key: String::from("R"),
                    symbol: Some(symbol_of("", "", 0.0, a, b, c)),
                    text_style: Some(SurveyTextStyle {
                        offset: a,
                        raise: b,
                        angle: c,
                        slant: a,
                        width_factor: b,
                        ..SurveyTextStyle::default()
                    }),
                    ..SurveyRule::default()
                },
            );
        }

        let text = write_customisation(&customisation);
        let again = read_customisation(&text);
        let read = again.library.find("D").expect("D");
        assert_eq!(read.strokes.len(), values.len() / 2 + values.len() / 3);
        assert_eq!(read.texts.len(), values.len() / 3);
        assert_eq!(again.map.size(), values.len() / 3);

        let same = |back: f64, value: f64| back.to_bits() == value.to_bits();
        for (i, &[x, y]) in pairs.iter().enumerate() {
            let points = again.library.find(&format!("P{i}")).expect("a point");
            assert!(
                same(points.origin.x, x) && same(points.origin.y, y),
                "an origin, values {}: wrote {x:?} and {y:?}, read {:?} and {:?}",
                2 * i,
                points.origin.x,
                points.origin.y
            );
            assert!(
                same(points.anchor1.x, y)
                    && same(points.anchor1.y, x)
                    && same(points.anchor2.x, x)
                    && same(points.anchor2.y, x),
                "the anchors, values {}: {x:?} and {y:?}",
                2 * i
            );
        }
        for (i, &[a, b, c]) in triples.iter().enumerate() {
            let stroke = &read.texts[i];
            assert!(
                same(stroke.angle, a) && same(stroke.width_factor, b),
                "a text stroke's angle and width factor, values {}: {a:?} and {b:?}",
                3 * i
            );
            assert!(
                same(stroke.unnamed[0], a)
                    && same(stroke.unnamed[1], b)
                    && same(stroke.unnamed[2], c),
                "a text stroke's extra, values {}: {a:?}, {b:?} and {c:?}",
                3 * i
            );
            let rule = &again.map.rules()[i];
            let symbol = rule.symbol.as_ref().expect("a symbol");
            let style = rule.text_style.as_ref().expect("a text style");
            assert!(
                same(symbol.rotation, a) && same(symbol.offset, b) && same(symbol.raise, c),
                "a rule's symbol, values {}: {a:?}, {b:?} and {c:?}",
                3 * i
            );
            assert!(
                same(style.offset, a)
                    && same(style.raise, b)
                    && same(style.angle, c)
                    && same(style.slant, a)
                    && same(style.width_factor, b),
                "a rule's text, values {}: {a:?}, {b:?} and {c:?}",
                3 * i
            );
        }

        let mut negative = 0;
        let mut subnormal = 0;
        let mut at_least_one = 0; // 1 or more: digits before the point
        let mut tiny = 0; // below 1e-5: written with a negative exponent
        for (i, &value) in values.iter().enumerate() {
            let stroke = &read.strokes[i / 2];
            let back = if i % 2 == 0 {
                stroke.point.x
            } else {
                stroke.point.y
            };
            assert_eq!(
                back.to_bits(),
                value.to_bits(),
                "a stroke, value {i}: wrote {value:?}, read {back:?}"
            );
            if value.is_sign_negative() {
                negative += 1;
            }
            if value.is_subnormal() {
                subnormal += 1;
            }
            if value.abs() >= 1.0 {
                at_least_one += 1;
            }
            if value != 0.0 && value.abs() < 1e-5 {
                tiny += 1;
            }
        }
        // By hand: half of all bit patterns are negative. Of the patterns within
        // a billion in size, the exponent field is 0 to 1052 of its 2047 values:
        // 1006 of those 1053 are below 1e-5 and 30 are 1 or more (a bit under 3
        // in 100, about 110 of the 4,000), and the named values add some; 50
        // subnormals were added outright, and two more are named above.
        assert!(negative > 1500, "{negative}");
        assert!(subnormal >= 52, "{subnormal}");
        assert!(at_least_one > 20, "{at_least_one}");
        assert!(tiny > 1000, "{tiny}");
        assert!(
            text.contains("e+"),
            "no number was written with an exponent"
        );
        assert!(text.contains("e-"));
    }

    // ---- what a write holds -----------------------------------------------------------

    #[test]
    fn the_options_leave_out_linestyles_symbols_or_codes_and_nothing_else() {
        let whole = small_customisation();

        let codes_only = CustomisationWriteOptions {
            linestyles: false,
            symbols: false,
            ..CustomisationWriteOptions::default()
        };
        let rules = read_customisation(write_customisation_with(&whole, &codes_only));
        assert!(rules.library.empty());
        assert_eq!(rules.map, whole.map);
        assert_eq!(
            rules.colours, whole.colours,
            "the colours go with whatever is written"
        );
        assert_eq!(rules.linework, whole.linework);
        assert_eq!(rules.name, "Site");
        // "And nothing else": what was read is the whole value with only its
        // definitions gone - the description, the notice, the sources, the
        // base, the colours, the linework and the automation are all as they
        // were.
        let mut without_definitions = whole.clone();
        without_definitions.library = StyleLibrary::default();
        assert!(rules == without_definitions);

        let symbols_only = CustomisationWriteOptions {
            linestyles: false,
            codes: false,
            ..CustomisationWriteOptions::default()
        };
        let symbols = read_customisation(write_customisation_with(&whole, &symbols_only));
        assert_eq!(symbols.library.names(), ["Peg"]);
        assert!(symbols.map.empty());
        let mut only_symbols = whole.clone();
        only_symbols.map = SurveyMap::default();
        for linestyle in ["Empty", "Fence", "Gate"] {
            assert!(
                only_symbols.library.remove(linestyle).is_ok(),
                "{linestyle}"
            );
        }
        assert!(symbols == only_symbols);

        let linestyles_only = CustomisationWriteOptions {
            symbols: false,
            codes: false,
            ..CustomisationWriteOptions::default()
        };
        let linestyles = read_customisation(write_customisation_with(&whole, &linestyles_only));
        assert_eq!(linestyles.library.names(), ["Empty", "Fence", "Gate"]);
        assert!(linestyles.map.empty());
        let mut only_linestyles = whole.clone();
        only_linestyles.map = SurveyMap::default();
        assert!(only_linestyles.library.remove("Peg").is_ok());
        assert!(linestyles == only_linestyles);

        // With every option on, the whole value.
        assert!(
            read_customisation(write_customisation_with(
                &whole,
                &CustomisationWriteOptions::default()
            )) == whole
        );
    }

    #[test]
    fn only_the_named_definitions_are_written_in_name_order_whatever_order_they_are_named_in() {
        let options = CustomisationWriteOptions {
            // one of them twice
            only: ["Peg", "Gate", "Empty", "Gate"].map(String::from).to_vec(),
            ..CustomisationWriteOptions::default()
        };
        let text = write_customisation_with(&small_customisation(), &options);
        let read = read_customisation(&text);
        assert_eq!(read.library.names(), ["Empty", "Gate", "Peg"]);
        assert!(
            text.find("\"name\": \"Empty\"").expect("Empty")
                < text.find("\"name\": \"Gate\"").expect("Gate")
        );
        assert_eq!(
            read.map,
            small_customisation().map,
            "the names choose definitions, not rules"
        );
    }

    #[test]
    fn a_definition_asked_for_by_name_is_never_left_out_silently() {
        let missing = CustomisationWriteOptions {
            only: vec![String::from("Fence"), String::from("No Such Style")],
            ..CustomisationWriteOptions::default()
        };
        let unknown = customisation_to_json(&small_customisation(), &missing)
            .expect_err("a name the library lacks");
        assert_eq!(unknown.code, ErrorCode::NotFound);
        assert!(
            unknown.describe().contains("No Such Style"),
            "{}",
            unknown.describe()
        );

        // Peg is a symbol; asking for it by name while leaving symbols out is
        // two instructions that cannot both be followed.
        let excluded = CustomisationWriteOptions {
            symbols: false,
            only: vec![String::from("Fence"), String::from("Peg")],
            ..CustomisationWriteOptions::default()
        };
        let dropped = customisation_to_json(&small_customisation(), &excluded)
            .expect_err("a symbol asked for with symbols left out");
        assert_eq!(dropped.code, ErrorCode::InvalidArgument);
        assert!(dropped.describe().contains("Peg"), "{}", dropped.describe());
    }

    // ---- one definition as text -------------------------------------------------------

    // The object a file holds for "Fence" above, with the file's four blanks of
    // indent taken off and no line break after it.
    const FENCE_TEXT: &str = r##"{"name": "Fence", "group": "Site/Lines", "length": 2.5, "strokes": [
  ["move", 0, 0],
  ["draw", 1.5, 0],
  ["pen", "red"],
  ["arc", -0.25, 0, 180],
  ["circle", 0.5],
  ["dot", 0],
  ["text", {"text": "F", "height": 1.5, "justify": "middle-centre"}]
]}"##;

    #[test]
    fn a_definition_on_its_own_is_the_object_a_file_holds() {
        let text = definition_to_json(&fence(), "Site").expect("written");
        assert_eq!(text, FENCE_TEXT);

        let read = definition_from_json(FENCE_TEXT.as_bytes(), false, "Site").expect("read");
        assert_eq!(read, fence());
    }

    #[test]
    fn a_definition_on_its_own_is_told_its_array_and_the_name_it_sits_under() {
        // Under no name, the source is said; under another's, it is said too.
        let alone = definition_to_json(&fence(), "").expect("written");
        assert!(
            alone.contains("\"length\": 2.5, \"from\": \"Site\", \"strokes\": ["),
            "{alone}"
        );
        let back = definition_from_json(alone.as_bytes(), false, "").expect("read back");
        assert_eq!(back.source, "Site");

        // The text says neither which array nor which name: the caller does.
        let symbol = definition_from_json(br#"{"name": "Mark"}"#, true, "Mine").expect("a symbol");
        assert!(symbol.symbol);
        assert_eq!(symbol.source, "Mine");
        let unnamed = definition_from_json(br#"{"name": "Mark"}"#, false, "").expect("a linestyle");
        assert!(!unnamed.symbol);
        assert_eq!(unnamed.source, "");
    }

    #[test]
    fn a_definition_on_its_own_is_refused_as_one_in_a_file_is_and_named_as_a_definition() {
        let not_json = definition_from_json(b"move 0 0", false, "").expect_err("not JSON");
        assert_eq!(not_json.code, ErrorCode::ParseFailure);
        assert_eq!(
            not_json.message,
            "not a definition in the Katana customisation format"
        );

        let list = definition_from_json(b"[]", false, "").expect_err("a list");
        assert_eq!(
            list.message,
            "not a definition in the Katana customisation format"
        );

        let unknown = definition_from_json(br#"{"name": "Fence", "colour": "red"}"#, false, "")
            .expect_err("an unknown member");
        assert_eq!(unknown.code, ErrorCode::ParseFailure);
        expect_names(&unknown, "definition \"Fence\"", "colour");

        let twice = definition_from_json(
            br#"{"name": "Fence", "group": "a", "group": "b"}"#,
            false,
            "",
        )
        .expect_err("a member given twice");
        assert_eq!(twice.code, ErrorCode::ParseFailure);
        expect_names(&twice, "definition \"Fence\"", "group");

        // No library is there to validate it, so reading does: a factor of 0
        // would collapse every stroke onto the origin.
        let invalid = definition_from_json(br#"{"name": "Fence", "factor": 0}"#, false, "")
            .expect_err("a factor of 0");
        assert_eq!(invalid.code, ErrorCode::InvalidArgument);
        assert!(
            invalid.message.starts_with("definition \"Fence\": "),
            "{}",
            invalid.describe()
        );

        // And writing one that validate refuses fails the same way.
        let mut unbounded = fence();
        unbounded.length = f64::INFINITY;
        let written = definition_to_json(&unbounded, "").expect_err("an infinite length");
        assert_eq!(written.code, ErrorCode::InvalidArgument);
        assert!(
            written.message.starts_with("definition \"Fence\": "),
            "{}",
            written.describe()
        );
    }
}

// ---- comparing, naming, the digest ----------------------------------------------------------

mod customisation {
    use super::*;

    #[test]
    fn two_are_equal_only_when_every_member_is() {
        let base_value = small_customisation();
        assert!(base_value == small_customisation());

        let mut changed = vec![base_value.clone(); 10];
        changed[0].name = String::from("Other");
        changed[1].description.push('.');
        changed[2].notice.push(String::from("one more line"));
        changed[3].sources.pop();
        changed[4].based_on = None;
        assert!(
            changed[5]
                .colours
                .add("one more", Color::new(1, 2, 3, 255))
                .is_ok()
        );
        changed[6].linework.as_mut().expect("linework").start = String::from("S");
        changed[7].automation = None;
        {
            let fence_again = LineStyle {
                symbol: true, // one flag of one definition
                ..fence()
            };
            assert!(changed[8].library.update(&fence_again).is_ok());
        }
        assert!(changed[9].map.r#move(0, 1).is_ok()); // the same rules in another order
        for (i, change) in changed.iter().enumerate() {
            // == and != both, as the C++ case asks both.
            let equal = *change == base_value;
            assert!(!equal, "change {i} went unnoticed");
            assert!(*change != base_value);
        }

        // A definition more, with every other one equal, is a difference too.
        let mut larger = base_value.clone();
        let extra = LineStyle {
            name: String::from("Extra"),
            ..LineStyle::default()
        };
        assert!(larger.library.add(extra).is_ok());
        let larger_equal = larger == base_value;
        let base_equal = base_value == larger;
        assert!(!larger_equal);
        assert!(!base_equal);
    }

    #[test]
    fn a_name_is_one_line_of_text_and_not_a_path() {
        assert!(validate_customisation_name("NSW").is_ok());
        assert!(validate_customisation_name("Site set 2026 (draft)").is_ok());
        assert!(
            validate_customisation_name("Z\u{FC}rich").is_ok(),
            "any UTF-8"
        );
        assert!(
            validate_customisation_name("linestyles.4d").is_ok(),
            "a file's name is a name"
        );

        // What a project's record of names cannot hold: it stores one a line,
        // and refuses a path separator (storage's validateMetadata).
        for name in ["", "Roads/2026", "Roads\\2026", "two\nlines", "two\rlines"] {
            let error = validate_customisation_name(name).expect_err(name);
            assert_eq!(error.code, ErrorCode::InvalidArgument);
        }
        // (Bytes that are not UTF-8, the C++ case's last, cannot be a &str.)
    }

    // A name is an identity - a project records it, and a drawing is matched
    // to its customisation by it - so two names that look alike must BE alike,
    // and a name must be something a person can see. The project's store would
    // take every one of these; they are refused here.
    #[test]
    fn a_name_has_no_blank_at_either_end_and_no_character_that_cannot_be_seen() {
        // A blank before or after, and blanks alone.
        for name in ["NSW ", " NSW", " ", "   ", " Site set "] {
            let error = validate_customisation_name(name).expect_err(name);
            assert_eq!(error.code, ErrorCode::InvalidArgument);
            assert!(error.message.contains("blank"), "{}", error.describe());
        }
        // The control characters: U+0000 to U+001F and U+007F. A tab, an escape
        // sequence that would colour a terminal, a NUL in the middle, a delete,
        // a form feed; and a tab at an end is a control character before it is
        // a blank.
        for name in [
            "a\tb",
            "a\u{1B}[31mb",
            "a\0b",
            "a\u{7F}",
            "\u{C}NSW",
            "NSW\t",
            "\u{1}",
        ] {
            let error =
                validate_customisation_name(name).expect_err(&format!("{} bytes", name.len()));
            assert_eq!(error.code, ErrorCode::InvalidArgument);
            assert!(
                error.message.contains("control character"),
                "{}",
                error.describe()
            );
        }
        // What is refused is shown with the character made visible, as JSON
        // escapes it, so that the refusal itself can be read.
        let escape = validate_customisation_name("a\u{1B}[31mb").expect_err("an escape sequence");
        assert_eq!(escape.context, "\"a\\u001b[31mb\"");

        // A blank inside a name, two of them, and what is not ASCII (a no-break
        // space, C2 A0, included: only the plain blank is looked for) are a
        // name.
        for name in [
            "Site set 2026",
            "a  b",
            "N S W",
            "Z\u{FC}rich S\u{FC}d",
            "NSW\u{A0}",
        ] {
            assert!(validate_customisation_name(name).is_ok(), "\"{name}\"");
        }
    }

    // FNV-1a, 64-bit, against the algorithm's published test vectors (Fowler,
    // Noll and Vo: the empty string, "a" and "foobar") - the vectors the cad
    // layer's sourceNameHash is pinned to, here in the 16 digits a file holds.
    #[test]
    fn the_digest_is_fnv1a64_as_sixteen_lower_case_hex_digits() {
        assert_eq!(customisation_digest(b""), "cbf29ce484222325");
        assert_eq!(customisation_digest(b"a"), "af63dc4c8601ec8c");
        assert_eq!(customisation_digest(b"foobar"), "85944171f73967e8");
        // Sixteen digits ALWAYS: none of the three above begins with 0, so a
        // digest written without its leading zeros would pass them. The same
        // published list gives 0x08985907b541d342 for "fo".
        assert_eq!(customisation_digest(b"fo"), "08985907b541d342");
        // Every byte counts, a zero byte and one above 0x7F included.
        assert_ne!(customisation_digest(b"a\0"), customisation_digest(b"a"));
        assert_ne!(
            customisation_digest(b"\xC3\xBC"),
            customisation_digest(b"\xC3\xBD")
        );
        // A digest it gives is one a file's "basedOn" accepts.
        let customisation = Customisation {
            name: String::from("Kept"),
            based_on: Some(base("NSW", &customisation_digest(b"anything"))),
            ..Customisation::default()
        };
        assert!(read_customisation(write_customisation(&customisation)) == customisation);
    }
}
