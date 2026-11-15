//! The twin of tests/entity/customisation/test_customisation_refusals.cpp:
//! what the Katana customisation format refuses (docs/customisation.md, "The
//! reader is strict") - each refusal by its error code and by the entry and
//! the member it names. The entries are named as the document says: the list,
//! the place in it counted from 0, and the entry's own name, `codes[1] "WM*"`.
//!
//! The expectations are written by hand from those rules, not read off a run.
//!
//! CustomisationWriter.TextThatIsNotUtf8IsRefusedNamingWhereItIs has no twin:
//! every one of its cases stores text that is not UTF-8 in the model, which a
//! String cannot hold, so the writer has no such text to refuse.

pub mod customisation_support;

use katana_core::error::{Error, ErrorCode};
use katana_core::text_encoding::encode_utf16_little_endian;
use katana_entity::customisation::{customisation_to_json, definition_from_json};
use katana_entity::{
    Color, Customisation, CustomisationBase, CustomisationSourceNote, CustomisationWriteOptions,
    LineStyle, LineworkCodes, Stroke, StrokeOp, StrokeText, SurveyAttribute, SurveyRule,
    SurveySymbol, SurveyTextStyle, validate,
};
use katana_geometry::Point2;

use customisation_support::{customisation_with, expect_names, read_customisation, refusal_of};

const NOT_A_CUSTOMISATION: &str = "not a Katana customisation file";
// What a file that begins with '<' or "//" - another program's format - is
// told beside that, with nothing of the JSON parser's account of the
// character.
const OLDER_FORMAT: &str = "not a Katana customisation file; the older formats are converted with katana_customisation_convert (a developer's tool, not installed; see docs/customisation.md)";
const NOT_A_DEFINITION: &str = "not a definition in the Katana customisation format";

/// What a refusal is checked against: the members of a customisation
/// otherwise sound, the entry and the member.
struct Refused {
    members: String,
    entry: String,
    member: String,
}

fn case(
    members: impl Into<String>,
    entry: impl Into<String>,
    member: impl Into<String>,
) -> Refused {
    Refused {
        members: members.into(),
        entry: entry.into(),
        member: member.into(),
    }
}

fn expect_refused(cases: &[Refused], code: ErrorCode) {
    for refused in cases {
        let error = refusal_of(customisation_with(&refused.members));
        assert_eq!(
            error.code,
            code,
            "{}\n{}",
            refused.members,
            error.describe()
        );
        expect_names(&error, &refused.entry, &refused.member);
    }
}

/// A customisation holding one definition, which the writer is then asked for.
fn holding(mut definition: LineStyle) -> Customisation {
    let mut customisation = Customisation {
        name: String::from("W"),
        ..Customisation::default()
    };
    definition.source = String::from("W");
    if let Err(error) = customisation.library.add(definition) {
        panic!("{}", error.describe());
    }
    customisation
}

fn holding_rule(rule: SurveyRule) -> Customisation {
    let mut customisation = Customisation {
        name: String::from("W"),
        ..Customisation::default()
    };
    if let Err(error) = customisation.map.add(rule) {
        panic!("{}", error.describe());
    }
    customisation
}

fn write_refusal(customisation: &Customisation) -> Error {
    match customisation_to_json(customisation, &CustomisationWriteOptions::default()) {
        Ok(text) => panic!("written without complaint:\n{text}"),
        Err(error) => error,
    }
}

fn stroke_of(op: StrokeOp) -> Stroke {
    Stroke {
        op,
        ..Stroke::default()
    }
}

fn named(name: &str) -> LineStyle {
    LineStyle {
        name: String::from(name),
        ..LineStyle::default()
    }
}

fn text_of(
    text: &str,
    angle: f64,
    height: f64,
    width_factor: f64,
    unnamed: [f64; 3],
) -> StrokeText {
    StrokeText {
        text: String::from(text),
        angle,
        height,
        width_factor,
        unnamed,
        ..StrokeText::default()
    }
}

fn attribute(kind: &str, name: &str, value: &str) -> SurveyAttribute {
    SurveyAttribute {
        r#type: String::from(kind),
        name: String::from(name),
        value: String::from(value),
    }
}

/// A symbol whose strokes are nine sound ones and then `tenth`: strokes[9].
fn test_valve_with(tenth: &str) -> String {
    let mut members = String::from(r#""symbols": [{"name": "TEST Valve", "strokes": ["#);
    for _ in 0..9 {
        members.push_str(r#"["draw", 1, 0], "#);
    }
    members.push_str(tenth);
    members.push_str("]}]");
    members
}

/// The word a stroke's text begins with: ["arc" -> arc.
fn stroke_word(stroke: &str) -> &str {
    let end = stroke[2..].find('"').map_or(stroke.len(), |at| at + 2);
    &stroke[2..end]
}

const TOO_LARGE: &str = "has a number too large to hold";
const TOO_SMALL: &str = "has a number too small to hold: it is not zero, and would be read as 0";

/// A value may nest as deep as its text is long: 100,000 "[" are a file of
/// 200 KB. Far past where anything that goes down a level of the program's
/// stack for a level of the value runs out of stack.
const DEEP: usize = 100_000;

/// DEEP lists one inside the other, the innermost empty: [[[...]]].
fn deep_lists() -> String {
    format!("{}{}", "[".repeat(DEEP), "]".repeat(DEEP))
}

/// DEEP objects one inside the other: {"a":{"a":...{"a":0}...}}.
fn deep_objects() -> String {
    let mut text = String::with_capacity(DEEP * 6 + 1);
    for _ in 0..DEEP {
        text.push_str("{\"a\":");
    }
    text.push('0');
    text.push_str(&"}".repeat(DEEP));
    text
}

mod customisation_refusals {
    use super::*;

    // ---- what is not a customisation file at all ----------------------------------------

    #[test]
    fn text_that_is_not_json_is_not_a_katana_customisation_file() {
        for text in [
            "",
            "   \n",
            "{",
            "not json",
            "{\"format\": \"katana-customisation\",}",
            "{\"format\": \"katana-customisation\", \"version\": 1, \"name\": \"T\"} x",
        ] {
            let error = refusal_of(text);
            assert_eq!(error.code, ErrorCode::ParseFailure, "{text}");
            assert_eq!(error.message, NOT_A_CUSTOMISATION, "{text}");
        }
    }

    #[test]
    fn json_that_does_not_say_it_is_one_is_not_a_katana_customisation_file() {
        // No format; another format; a format that is not text; not an object.
        for text in [
            "{}",
            r#"{"version": 1, "name": "T"}"#,
            r#"{"format": "katana-sheets", "version": 1, "name": "T"}"#,
            r#"{"format": "Katana-Customisation", "version": 1, "name": "T"}"#,
            r#"{"format": 1, "version": 1, "name": "T"}"#,
            "[]",
            "7",
            "\"katana-customisation\"",
            "null",
        ] {
            let error = refusal_of(text);
            assert_eq!(error.code, ErrorCode::ParseFailure, "{text}");
            assert_eq!(error.message, NOT_A_CUSTOMISATION, "{text}");
        }
    }

    #[test]
    fn malformed_json_says_where_it_stops_being_json() {
        // The third line lacks the comma after its member. The message is the
        // one every text that is not JSON gets; the place is beside it.
        let error = refusal_of(concat!(
            "{\n",
            "  \"format\": \"katana-customisation\",\n",
            "  \"version\": 1\n",
            "  \"name\": \"T\"\n",
            "}\n"
        ));
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert_eq!(error.message, NOT_A_CUSTOMISATION);
        assert!(
            error.context.contains("line 4"),
            "the name on line 4 is where a comma was expected: {}",
            error.describe()
        );
    }

    // A survey code file of the older kind is XML, and is commonly UTF-16.
    #[test]
    fn a_survey_code_file_in_xml_is_not_a_katana_customisation_file() {
        let xml = concat!(
            "<?xml version=\"1.0\"?>\n",
            "<map_file><version>11.0</version>\n",
            "<map_data><item><key>WM*</key><model>TEST SERVICES</model>",
            "<colour>blue</colour></item></map_data></map_file>\n"
        );
        let plain = refusal_of(xml);
        assert_eq!(plain.code, ErrorCode::ParseFailure);
        assert_eq!(plain.message, OLDER_FORMAT);
        assert!(
            plain.context.is_empty(),
            "the parser's own sentence is not shown: {}",
            plain.context
        );

        let utf16 = encode_utf16_little_endian(xml).expect("UTF-16");
        let wide = refusal_of(utf16);
        assert_eq!(wide.code, ErrorCode::ParseFailure);
        assert_eq!(wide.message, OLDER_FORMAT);
        assert!(wide.context.is_empty(), "{}", wide.context);
    }

    #[test]
    fn a_style_library_in_its_own_text_is_not_a_katana_customisation_file() {
        let error = refusal_of(concat!(
            "// a linestyle library\n",
            "worldstyle \"TEST Water Main\" {\n",
            "    group \"Test/Water\"\n",
            "    length 2.5\n",
            "    move 0 0\n",
            "    draw 1.5 0\n",
            "}\n"
        ));
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert_eq!(error.message, OLDER_FORMAT);
        assert!(error.context.is_empty(), "{}", error.context);
    }

    #[test]
    fn only_a_first_character_of_markup_or_a_comment_sends_a_file_to_the_converter() {
        // After blanks and a byte order mark, as JSON allows blanks: the first
        // character that is not one decides.
        for text in [
            "  \n\t<map/>",
            "\u{FEFF}<?xml version=\"1.0\"?>",
            "\r\n// notice\n",
        ] {
            let error = refusal_of(text);
            assert_eq!(error.message, OLDER_FORMAT, "{text}");
            assert!(error.context.is_empty(), "{text}");
        }
        // A single slash is no comment, a '<' later in a file is not its first
        // character, and a file of the older style libraries that begins with a
        // word is not told about the converter: it is only not a customisation,
        // with the parser's account beside it as ever.
        for text in ["/ not a comment", "{ \"a\": < }", "worldstyle \"A\" { }"] {
            let error = refusal_of(text);
            assert_eq!(error.message, NOT_A_CUSTOMISATION, "{text}");
            assert!(!error.context.is_empty(), "{text}");
        }
        // A definition read on its own is not told of the converter either: its
        // text is no file of another program.
        let definition = definition_from_json(b"<x/>", true, "").expect_err("markup");
        assert!(
            !definition.message.contains("katana_customisation_convert"),
            "{}",
            definition.describe()
        );
    }

    // ---- encodings ----------------------------------------------------------------------

    #[test]
    fn a_file_with_a_utf8_byte_order_mark_is_read() {
        // "Zürich Süd": ü is C3 BC in UTF-8.
        let text = "{\"format\": \"katana-customisation\", \"version\": 1, \"name\": \"Z\u{FC}rich S\u{FC}d\"}";
        let marked = read_customisation(format!("\u{FEFF}{text}"));
        assert_eq!(marked.name, "Z\u{FC}rich S\u{FC}d");
        assert!(marked == read_customisation(text));
    }

    #[test]
    fn a_file_in_utf16_is_read() {
        // What an editor on Windows saves as "Unicode": UTF-16LE behind FF FE.
        let text = concat!(
            "{\"format\": \"katana-customisation\", \"version\": 1, \"name\": \"Z\u{FC}rich\",",
            " \"linestyles\": [{\"name\": \"A\", \"strokes\": [[\"move\", 0.5, -2]]}]}"
        );
        let utf16 = encode_utf16_little_endian(text).expect("UTF-16");
        assert_eq!(utf16[0], 0xFF);
        assert_eq!(utf16[1], 0xFE);
        let read = read_customisation(&utf16);
        assert_eq!(read.name, "Z\u{FC}rich");
        let a = read.library.find("A").expect("A");
        assert_eq!(a.strokes[0].point.y, -2.0);
        assert!(read == read_customisation(text));
    }

    #[test]
    fn bytes_that_are_not_utf8_are_refused_rather_than_guessed_at() {
        // "Café" saved by an editor in the Windows-1252 code page: E9 alone is
        // not UTF-8. Elsewhere such bytes are decoded as that code page; here
        // the name would become some other name without a word said.
        let error = refusal_of(
            b"{\"format\": \"katana-customisation\", \"version\": 1, \"name\": \"Caf\xE9\"}",
        );
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert_eq!(error.message, NOT_A_CUSTOMISATION);
        assert!(error.context.contains("UTF-8"), "{}", error.describe());
    }

    // A byte order mark is a promise about the bytes behind it. When they break
    // it the file is still "bytes that are neither UTF-8 nor UTF-16", and gets
    // the one message every such file gets, with what is wrong beside it - not
    // the decoder's own message under some other headline.
    #[test]
    fn bytes_that_break_their_own_byte_order_mark_are_not_a_katana_customisation_file() {
        let sound = r#"{"format": "katana-customisation", "version": 1, "name": "T"}"#;

        // EF BB BF says UTF-8; E9 alone ("Café" in Windows-1252) is not.
        let marked = refusal_of(
            b"\xEF\xBB\xBF{\"format\": \"katana-customisation\", \"version\": 1, \"name\": \"Caf\xE9\"}",
        );
        assert_eq!(marked.code, ErrorCode::ParseFailure);
        assert_eq!(marked.message, NOT_A_CUSTOMISATION);
        assert!(marked.context.contains("UTF-8"), "{}", marked.describe());

        // FF FE says UTF-16, which is two bytes a unit: one byte more is a file
        // cut short.
        let utf16 = encode_utf16_little_endian(sound).expect("UTF-16");
        assert!(utf16.len().is_multiple_of(2));
        let mut longer = utf16.clone();
        longer.push(b'}');
        let odd = refusal_of(longer);
        assert_eq!(odd.code, ErrorCode::ParseFailure);
        assert_eq!(odd.message, NOT_A_CUSTOMISATION);
        assert!(odd.context.contains("UTF-16"), "{}", odd.describe());

        // A high surrogate (D83D, little-endian 3D D8) with no low one after
        // it, as the file's last unit.
        let mut cut = utf16.clone();
        cut.extend_from_slice(b"\x3D\xD8");
        let half = refusal_of(cut);
        assert_eq!(half.code, ErrorCode::ParseFailure);
        assert_eq!(half.message, NOT_A_CUSTOMISATION);
        assert!(half.context.contains("UTF-16"), "{}", half.describe());

        // The same goes for one definition's text.
        let definition = definition_from_json(b"\xEF\xBB\xBF{\"name\": \"Caf\xE9\"}", false, "")
            .expect_err("bytes that break their mark");
        assert_eq!(definition.code, ErrorCode::ParseFailure);
        assert_eq!(definition.message, NOT_A_DEFINITION);
    }

    // ---- the version --------------------------------------------------------------------

    #[test]
    fn a_newer_version_is_unsupported_whatever_else_the_file_holds() {
        let newer = refusal_of(r#"{"format": "katana-customisation", "version": 2, "name": "T"}"#);
        assert_eq!(newer.code, ErrorCode::Unsupported);
        assert!(
            newer.message.contains("newer version of Katana"),
            "{}",
            newer.describe()
        );
        assert!(newer.context.contains("version 2"), "{}", newer.describe());

        // A member this build has never heard of is what a newer file would
        // hold; it must be told the file is newer, not that the member is
        // unknown. And a version no 32-bit number holds is newer still.
        for text in [
            r#"{"format": "katana-customisation", "version": 2, "name": "T", "layers": []}"#,
            r#"{"format": "katana-customisation", "version": 7, "codes": "none"}"#,
            r#"{"format": "katana-customisation", "version": 99999999999, "name": "T"}"#,
        ] {
            assert_eq!(refusal_of(text).code, ErrorCode::Unsupported, "{text}");
        }
    }

    #[test]
    fn a_version_that_is_not_a_whole_number_from_one_is_refused() {
        for version in ["1.5", "1.0", "true", "\"1\"", "null", "0", "-1", "[1]"] {
            let text = format!(
                r#"{{"format": "katana-customisation", "version": {version}, "name": "T"}}"#
            );
            let error = refusal_of(&text);
            assert_eq!(error.code, ErrorCode::ParseFailure, "{text}");
            expect_names(&error, "top level", "version");
        }
        let missing = refusal_of(r#"{"format": "katana-customisation", "name": "T"}"#);
        assert_eq!(missing.code, ErrorCode::ParseFailure);
        expect_names(&missing, "top level", "version");
    }

    // ---- the name -----------------------------------------------------------------------

    #[test]
    fn a_customisation_with_no_name_is_refused() {
        let error = refusal_of(r#"{"format": "katana-customisation", "version": 1}"#);
        assert_eq!(
            error.code,
            ErrorCode::ParseFailure,
            "a member that must be there is not"
        );
        expect_names(&error, "top level", "name");

        let number = refusal_of(r#"{"format": "katana-customisation", "version": 1, "name": 7}"#);
        assert_eq!(number.code, ErrorCode::ParseFailure);
        expect_names(&number, "top level", "name");
    }

    #[test]
    fn a_name_that_a_project_could_not_record_is_refused() {
        // '/', '\', a line break and nothing at all: each well-formed, and each
        // a name the project store would refuse at the next SAVE.
        for name in ["Roads/2026", r"Roads\\2026", r"two\nlines", ""] {
            let text =
                format!(r#"{{"format": "katana-customisation", "version": 1, "name": "{name}"}}"#);
            let error = refusal_of(&text);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{text}");
            expect_names(&error, "top level", "name");
        }
    }

    #[test]
    fn a_name_that_could_not_be_told_from_another_or_seen_is_refused() {
        // A name is an identity: a project records it and is matched against
        // it. A blank at either end makes two names that look like one; a tab,
        // an escape (U+001B), a NUL and a delete (U+007F) are characters nobody
        // can see in a list. Each is written here as JSON escapes it.
        for name in [
            "NSW ",
            " NSW",
            " ",
            r"a\tb",
            r"a\u001b[31mb",
            r"a\u0000b",
            r"a\u007f",
        ] {
            let text =
                format!(r#"{{"format": "katana-customisation", "version": 1, "name": "{name}"}}"#);
            let error = refusal_of(&text);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{text}");
            expect_names(&error, "top level", "name");
        }
        // A blank INSIDE a name is part of it.
        assert_eq!(
            read_customisation(
                r#"{"format": "katana-customisation", "version": 1, "name": "Site set 2026"}"#
            )
            .name,
            "Site set 2026"
        );
    }

    #[test]
    fn the_names_of_sources_bases_and_definition_sources_are_held_to_the_same_rule() {
        expect_refused(
            &[
                case(
                    r#""sources": [{"name": "Fine"}, {"name": "a/b", "rules": true}]"#,
                    r#"sources[1] "a/b""#,
                    "name",
                ),
                case(
                    r#""basedOn": {"name": "a\\b", "digest": "0123456789abcdef"}"#,
                    "basedOn",
                    "name",
                ),
                case(
                    r#""linestyles": [{"name": "A", "from": "x/y"}]"#,
                    r#"linestyles[0] "A""#,
                    "from",
                ),
                case(
                    r#""symbols": [{"name": "A"}, {"name": "B", "from": "two\nlines"}]"#,
                    r#"symbols[1] "B""#,
                    "from",
                ),
                // a blank at an end, and a character that cannot be seen
                case(
                    r#""sources": [{"name": "Site "}]"#,
                    r#"sources[0] "Site ""#,
                    "name",
                ),
                case(
                    r#""basedOn": {"name": " NSW", "digest": "0123456789abcdef"}"#,
                    "basedOn",
                    "name",
                ),
                case(
                    r#""linestyles": [{"name": "A", "from": "a\tb"}]"#,
                    r#"linestyles[0] "A""#,
                    "from",
                ),
            ],
            ErrorCode::InvalidArgument,
        );
    }

    #[test]
    fn a_base_without_a_sound_digest_is_refused() {
        // Sixteen digits, hexadecimal, lower case: one short, one upper, one not
        // hex.
        for digest in [
            "0123456789abcde",
            "0123456789ABCDEF",
            "0123456789abcdeg",
            "",
        ] {
            let error = refusal_of(customisation_with(&format!(
                r#""basedOn": {{"name": "NSW", "digest": "{digest}"}}"#
            )));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{digest}");
            expect_names(&error, "basedOn", "digest");
        }
        expect_refused(
            &[
                case(r#""basedOn": {"name": "NSW"}"#, "basedOn", "digest"),
                case(
                    r#""basedOn": {"digest": "0123456789abcdef"}"#,
                    "basedOn",
                    "name",
                ),
            ],
            ErrorCode::ParseFailure,
        );
    }

    // ---- unknown members ----------------------------------------------------------------

    // At every level an object can sit at. A lenient reader would skip each of
    // these; here "linesytle" is a refusal rather than a rule that draws
    // nothing.
    #[test]
    fn an_unknown_member_is_refused_at_every_level_naming_the_entry() {
        let cases = [
            case(r#""colors": {}"#, "top level", "colors"),
            case(
                r#""sources": [{"name": "A", "defs": true}]"#,
                r#"sources[0] "A""#,
                "defs",
            ),
            case(
                r#""basedOn": {"name": "A", "digest": "0123456789abcdef", "date": "x"}"#,
                "basedOn",
                "date",
            ),
            case(r#""linework": {"begin": "ST"}"#, "linework", "begin"),
            case(r#""automation": {"codes": true}"#, "automation", "codes"),
            case(
                r#""linestyles": [{"name": "A"}, {"name": "B", "colour": "red"}]"#,
                r#"linestyles[1] "B""#,
                "colour",
            ),
            case(
                r#""symbols": [{"name": "TEST Valve", "mode": "vertex"}]"#,
                r#"symbols[0] "TEST Valve""#,
                "mode",
            ),
            case(
                r#""symbols": [{"name": "V", "strokes": [["move", 0, 0], ["text", {"hieght": 1}]]}]"#,
                r#"symbols[0] "V" strokes[1]"#,
                "hieght",
            ),
            case(
                r#""codes": [{"key": "A", "sets": "feature"},
                      {"key": "WM*", "sets": "feature", "linesytle": "TEST Water Main"}]"#,
                r#"codes[1] "WM*""#,
                "linesytle",
            ),
            case(
                r#""codes": [{"key": "AC*", "sets": "symbol", "symbol": {"name": "P", "scale": 2}}]"#,
                r#"codes[0] "AC*" symbol"#,
                "scale",
            ),
            case(
                r#""codes": [{"key": "T*", "sets": "text", "text": {"style": "ISO", "bold": true}}]"#,
                r#"codes[0] "T*" text"#,
                "bold",
            ),
            case(
                r#""codes": [{"key": "P*", "sets": "pipe", "pipe": {"diameter": "0.3"}}]"#,
                r#"codes[0] "P*" pipe"#,
                "diameter",
            ),
            case(
                r#""codes": [{"key": "P*", "sets": "vertexPipe", "vertexPipe": {"diameter": "0.3"}}]"#,
                r#"codes[0] "P*" vertexPipe"#,
                "diameter",
            ),
            case(
                r#""codes": [{"key": "P*", "sets": "segmentPipe", "segmentPipe": {"diameter": "0.3"}}]"#,
                r#"codes[0] "P*" segmentPipe"#,
                "diameter",
            ),
            case(
                r#""codes": [{"key": "A*", "sets": "attributes", "attributes": [
                          {"type": "text", "name": "Owner"},
                          {"type": "text", "name": "Zone", "units": "m"}]}]"#,
                r#"codes[0] "A*" attributes[1]"#,
                "units",
            ),
            case(
                r#""codes": [{"key": "A*", "sets": "vertexAttributes", "vertexAttributes": [
                          {"type": "text", "name": "Zone", "units": "m"}]}]"#,
                r#"codes[0] "A*" vertexAttributes[0]"#,
                "units",
            ),
            case(
                r#""codes": [{"key": "A*", "sets": "segmentPipe", "segmentAttributes": [
                          {"type": "text", "name": "Zone", "units": "m"}]}]"#,
                r#"codes[0] "A*" segmentAttributes[0]"#,
                "units",
            ),
        ];
        for refused in &cases {
            let error = refusal_of(customisation_with(&refused.members));
            assert_eq!(error.code, ErrorCode::ParseFailure, "{}", refused.members);
            assert_eq!(
                error.message,
                format!("{}: unknown member \"{}\"", refused.entry, refused.member),
                "{}",
                refused.members
            );
        }
    }

    #[test]
    fn a_member_of_the_older_formats_is_unknown_here() {
        // "model" is what a survey code file calls a rule's layer; the format's
        // word is "layer", and the old one is not quietly taken for it.
        let error = refusal_of(customisation_with(
            r#""codes": [{"key": "WM*", "sets": "feature", "model": "TEST SERVICES"}]"#,
        ));
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert_eq!(error.message, r#"codes[0] "WM*": unknown member "model""#);
        // The refusal lists what the entry may hold, so the right word is at
        // hand.
        assert!(error.context.contains("layer"), "{}", error.describe());
    }

    // ---- a member given twice -----------------------------------------------------------

    #[test]
    fn a_second_codes_list_does_not_silently_replace_the_first() {
        // Two rules, then a "codes" pasted again at the end holding one. A JSON
        // reader left to itself keeps the last and says nothing.
        let error = refusal_of(customisation_with(
            r#""codes": [{"key": "A", "sets": "feature"}, {"key": "B", "sets": "feature"}],
           "symbols": [], "codes": [{"key": "C", "sets": "feature"}]"#,
        ));
        assert_eq!(error.code, ErrorCode::ParseFailure);
        expect_names(&error, "top level", "codes");
        assert!(error.message.contains("twice"), "{}", error.describe());
    }

    #[test]
    fn a_member_given_twice_is_refused_at_every_level_naming_the_entry() {
        expect_refused(
            &[
                case(r#""name": "U""#, "top level", "name"),
                case(
                    r##""colours": {"sui gas": "#FFFF00", "sui gas": "#FF0000"}"##,
                    "colours",
                    "sui gas",
                ),
                case(
                    r#""linework": {"start": "S", "start": "ST"}"#,
                    "linework",
                    "start",
                ),
                case(
                    r#""sources": [{"name": "A"}, {"name": "B", "rules": true, "rules": false}]"#,
                    r#"sources[1] "B""#,
                    "rules",
                ),
                case(
                    r#""symbols": [{"name": "TEST Valve", "group": "a", "group": "b"}]"#,
                    r#"symbols[0] "TEST Valve""#,
                    "group",
                ),
                // Inside a stroke the stroke is as far as a place goes ("How an
                // entry is named"): the same `strokes[1]` an unknown member of
                // that object is refused under, above.
                case(
                    r#""linestyles": [{"name": "L", "strokes": [["move", 0, 0],
                                                        ["text", {"text": "a", "text": "b"}]]}]"#,
                    r#"linestyles[0] "L" strokes[1]"#,
                    "text",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": "feature"},
                          {"key": "WM*", "sets": "feature", "colour": "red", "colour": "blue"}]"#,
                    r#"codes[1] "WM*""#,
                    "colour",
                ),
                case(
                    r#""codes": [{"key": "AC*", "sets": "symbol", "symbol": {"size": 1, "size": 2}}]"#,
                    r#"codes[0] "AC*" symbol"#,
                    "size",
                ),
                case(
                    r#""codes": [{"key": "A*", "sets": "attributes", "attributes": [
                              {"type": "text", "name": "a", "name": "b"}]}]"#,
                    r#"codes[0] "A*" attributes[0]"#,
                    "name",
                ),
            ],
            ErrorCode::ParseFailure,
        );
    }

    // ---- values of the wrong type -------------------------------------------------------

    #[test]
    fn a_whole_number_member_given_a_fraction_is_refused_rather_than_cut_short() {
        expect_refused(
            &[
                case(
                    r#""linestyles": [{"name": "G", "stretchMode": 2.7}]"#,
                    r#"linestyles[0] "G""#,
                    "stretchMode",
                ),
                case(
                    r#""linestyles": [{"name": "G", "cycleMode": 1.0}]"#,
                    r#"linestyles[0] "G""#,
                    "cycleMode",
                ),
                case(
                    r#""symbols": [{"name": "G", "cycleMode": 1e2}]"#,
                    r#"symbols[0] "G""#,
                    "cycleMode",
                ),
            ],
            ErrorCode::ParseFailure,
        );
    }

    #[test]
    fn a_whole_number_member_given_true_is_refused_rather_than_read_as_one() {
        expect_refused(
            &[
                case(
                    r#""linestyles": [{"name": "G", "stretchMode": true}]"#,
                    r#"linestyles[0] "G""#,
                    "stretchMode",
                ),
                case(
                    r#""linestyles": [{"name": "G", "cycleMode": false}]"#,
                    r#"linestyles[0] "G""#,
                    "cycleMode",
                ),
                case(
                    r#""linestyles": [{"name": "G", "cycleMode": "1"}]"#,
                    r#"linestyles[0] "G""#,
                    "cycleMode",
                ),
            ],
            ErrorCode::ParseFailure,
        );
    }

    #[test]
    fn a_whole_number_too_large_to_hold_is_refused() {
        // One past the largest and one before the smallest a 32-bit int holds.
        expect_refused(
            &[
                case(
                    r#""linestyles": [{"name": "G", "stretchMode": 2147483648}]"#,
                    r#"linestyles[0] "G""#,
                    "stretchMode",
                ),
                case(
                    r#""linestyles": [{"name": "G", "cycleMode": -2147483649}]"#,
                    r#"linestyles[0] "G""#,
                    "cycleMode",
                ),
            ],
            ErrorCode::ParseFailure,
        );
        // The two ends themselves are read.
        let ends = read_customisation(customisation_with(
            r#""linestyles": [{"name": "G", "stretchMode": 2147483647, "cycleMode": -2147483648}]"#,
        ));
        let g = ends.library.find("G").expect("G");
        assert_eq!(g.stretch_mode, i32::MAX);
        assert_eq!(g.cycle_mode, i32::MIN);
    }

    #[test]
    fn a_value_of_the_wrong_kind_is_refused_naming_its_member() {
        expect_refused(
            &[
                // text that must be text: a weight and a pipe size are kept as
                // written ("0", "Normal", "$PipeDiameter") and are never numbers
                case(
                    r#""codes": [{"key": "A", "sets": "feature", "weight": 0}]"#,
                    r#"codes[0] "A""#,
                    "weight",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": "pipe", "pipe": {"size1": 0.375}}]"#,
                    r#"codes[0] "A" pipe"#,
                    "size1",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": "feature", "layer": ["a"]}]"#,
                    r#"codes[0] "A""#,
                    "layer",
                ),
                case(r#""description": 5"#, "top level", "description"),
                case(
                    r#""linestyles": [{"name": "G", "group": null}]"#,
                    r#"linestyles[0] "G""#,
                    "group",
                ),
                // numbers that must be numbers
                case(
                    r#""linestyles": [{"name": "G", "length": "2.5"}]"#,
                    r#"linestyles[0] "G""#,
                    "length",
                ),
                case(
                    r#""linestyles": [{"name": "G", "factor": true}]"#,
                    r#"linestyles[0] "G""#,
                    "factor",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": "symbol", "symbol": {"size": "big"}}]"#,
                    r#"codes[0] "A" symbol"#,
                    "size",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": "text", "text": {"slant": null}}]"#,
                    r#"codes[0] "A" text"#,
                    "slant",
                ),
                // true and false that must be just that
                case(
                    r#""linestyles": [{"name": "G", "atVertices": 1}]"#,
                    r#"linestyles[0] "G""#,
                    "atVertices",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": "symbol", "hide": "yes"}]"#,
                    r#"codes[0] "A""#,
                    "hide",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": "surface", "surface": 0}]"#,
                    r#"codes[0] "A""#,
                    "surface",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": "pipe", "pipe": {"active": "yes"}}]"#,
                    r#"codes[0] "A" pipe"#,
                    "active",
                ),
                case(
                    r#""automation": {"codesOnSurveyImport": "on"}"#,
                    "automation",
                    "codesOnSurveyImport",
                ),
                case(
                    r#""sources": [{"name": "A", "rules": 1}]"#,
                    r#"sources[0] "A""#,
                    "rules",
                ),
                // points, lines of text and lists
                case(
                    r#""linestyles": [{"name": "G", "origin": [1]}]"#,
                    r#"linestyles[0] "G""#,
                    "origin",
                ),
                case(
                    r#""linestyles": [{"name": "G", "origin": [1, "2"]}]"#,
                    r#"linestyles[0] "G""#,
                    "origin",
                ),
                case(
                    r#""linestyles": [{"name": "G", "anchors": [[0, 0]]}]"#,
                    r#"linestyles[0] "G""#,
                    "anchors",
                ),
                case(
                    r#""linestyles": [{"name": "G", "anchors": [[0, 0], [1]]}]"#,
                    r#"linestyles[0] "G""#,
                    "anchors",
                ),
                // ... and one value too many is as wrong as one too few: a third
                // coordinate or a third anchor would otherwise be dropped unsaid
                case(
                    r#""linestyles": [{"name": "G", "origin": [1, 2, 3]}]"#,
                    r#"linestyles[0] "G""#,
                    "origin",
                ),
                case(
                    r#""linestyles": [{"name": "G", "anchors": [[0, 0], [1, 1], [2, 2]]}]"#,
                    r#"linestyles[0] "G""#,
                    "anchors",
                ),
                case(
                    r#""linestyles": [{"name": "G", "anchors": [[0, 0, 0], [1, 1]]}]"#,
                    r#"linestyles[0] "G""#,
                    "anchors",
                ),
                case(
                    r#""linestyles": [{"name": "G", "anchors": []}]"#,
                    r#"linestyles[0] "G""#,
                    "anchors",
                ),
                case(
                    r#""linestyles": [{"name": "G", "strokes": {"move": [0, 0]}}]"#,
                    r#"linestyles[0] "G""#,
                    "strokes",
                ),
                case(r#""notice": "one line""#, "top level", "notice"),
                case(r#""notice": ["one", 2]"#, "top level", "notice"),
                case(
                    r#""sources": [{"name": "A", "notice": "one line"}]"#,
                    r#"sources[0] "A""#,
                    "notice",
                ),
                case(r#""sources": {"name": "A"}"#, "top level", "sources"),
                case(r#""linestyles": {"name": "A"}"#, "top level", "linestyles"),
                case(r#""symbols": "none""#, "top level", "symbols"),
                case(r#""codes": {"key": "A"}"#, "top level", "codes"),
                case(r#""colours": ["red"]"#, "top level", "colours"),
                case(
                    r#""codes": [{"key": "A", "sets": "attributes", "attributes": {"type": "text"}}]"#,
                    r#"codes[0] "A""#,
                    "attributes",
                ),
                case(r#""linework": {"start": 5}"#, "linework", "start"),
            ],
            ErrorCode::ParseFailure,
        );
    }

    #[test]
    fn an_entry_that_is_not_an_object_is_refused_by_its_place() {
        for (members, entry) in [
            (r#""linestyles": [{"name": "A"}, "B"]"#, "linestyles[1]"),
            (r#""symbols": [[]]"#, "symbols[0]"),
            (
                r#""codes": [{"key": "A", "sets": "feature"}, 7]"#,
                "codes[1]",
            ),
            (r#""sources": [null]"#, "sources[0]"),
            (r#""basedOn": "NSW""#, "basedOn"),
            (r#""linework": []"#, "linework"),
            (r#""automation": true"#, "automation"),
            (
                r#""codes": [{"key": "A", "sets": "symbol", "symbol": "Peg"}]"#,
                r#"codes[0] "A" symbol"#,
            ),
            (
                r#""codes": [{"key": "A", "sets": "attributes", "attributes": ["Owner"]}]"#,
                r#"codes[0] "A" attributes[0]"#,
            ),
        ] {
            let error = refusal_of(customisation_with(members));
            assert_eq!(error.code, ErrorCode::ParseFailure, "{members}");
            assert_eq!(
                error.message,
                format!("{entry}: must be an object, written {{...}}"),
                "{members}"
            );
        }
    }

    // ---- words outside an enumeration ---------------------------------------------------

    #[test]
    fn a_word_the_format_does_not_have_is_refused_naming_its_member() {
        expect_refused(
            &[
                case(
                    r#""linestyles": [{"name": "G", "units": "metric"}]"#,
                    r#"linestyles[0] "G""#,
                    "units",
                ),
                // the words are matched as written: no case is folded
                case(
                    r#""linestyles": [{"name": "G", "units": "Paper"}]"#,
                    r#"linestyles[0] "G""#,
                    "units",
                ),
                case(
                    r#""linestyles": [{"name": "G", "units": 1}]"#,
                    r#"linestyles[0] "G""#,
                    "units",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": "feature", "draw": "polyline"}]"#,
                    r#"codes[0] "A""#,
                    "draw",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": "feature", "draw": "Line"}]"#,
                    r#"codes[0] "A""#,
                    "draw",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": "features"}]"#,
                    r#"codes[0] "A""#,
                    "sets",
                ),
                // the older format's word for the same section is not this one's
                case(
                    r#""codes": [{"key": "A", "sets": "map_data"}]"#,
                    r#"codes[0] "A""#,
                    "sets",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": 1}]"#,
                    r#"codes[0] "A""#,
                    "sets",
                ),
            ],
            ErrorCode::ParseFailure,
        );
    }

    #[test]
    fn a_rule_must_say_its_key_and_what_it_sets() {
        // No default for "sets": a symbol rule read as a feature rule would, in
        // a merge, replace that key's layer and colour rule.
        expect_refused(
            &[
                case(r#""codes": [{"sets": "feature"}]"#, "codes[0]", "key"),
                case(r#""codes": [{"key": "A"}]"#, r#"codes[0] "A""#, "sets"),
                case(
                    r#""codes": [{"key": "A", "sets": "feature"}, {"key": "B", "layer": "L"}]"#,
                    r#"codes[1] "B""#,
                    "sets",
                ),
            ],
            ErrorCode::ParseFailure,
        );
    }

    #[test]
    fn an_attribute_is_text_or_an_integer_and_nothing_else() {
        expect_refused(
            &[
                case(
                    r#""codes": [{"key": "A", "sets": "attributes", "attributes": [
                              {"type": "real", "name": "Depth", "value": "1.2"}]}]"#,
                    r#"codes[0] "A" attributes[0]"#,
                    "type",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": "vertexAttributes", "vertexAttributes": [
                              {"type": "text", "name": "a"}, {"type": "Text", "name": "b"}]}]"#,
                    r#"codes[0] "A" vertexAttributes[1]"#,
                    "type",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": "segmentPipe", "segmentAttributes": [
                              {"name": "b"}]}]"#,
                    r#"codes[0] "A" segmentAttributes[0]"#,
                    "type",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": "attributes", "attributes": [
                              {"type": 3, "name": "b"}]}]"#,
                    r#"codes[0] "A" attributes[0]"#,
                    "type",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": "attributes", "attributes": [{"type": "text"}]}]"#,
                    r#"codes[0] "A" attributes[0]"#,
                    "name",
                ),
                case(
                    r#""codes": [{"key": "A", "sets": "attributes", "attributes": [
                              {"type": "integer", "name": "Zone", "value": 1}]}]"#,
                    r#"codes[0] "A" attributes[0]"#,
                    "value",
                ),
            ],
            ErrorCode::ParseFailure,
        );
        // An attribute with a name that is empty is well-formed and still not
        // an attribute: validate refuses it.
        let unnamed = refusal_of(customisation_with(
            r#""codes": [{"key": "A", "sets": "attributes", "attributes": [{"type": "text", "name": ""}]}]"#,
        ));
        assert_eq!(unnamed.code, ErrorCode::InvalidArgument);
        assert!(
            unnamed.message.starts_with(r#"codes[0] "A": "#),
            "{}",
            unnamed.describe()
        );
    }

    // ---- strokes ------------------------------------------------------------------------

    #[test]
    fn a_stroke_of_the_wrong_length_is_refused_naming_the_stroke() {
        // Each kind with a value too few or too many. An arc is a radius and two
        // angles; move and draw a point; circle and dot a radius; pen a name;
        // text one object.
        for stroke in [
            r#"["arc", 1, 2]"#,
            r#"["arc", 1, 2, 3, 4]"#,
            r#"["move", 1]"#,
            r#"["move", 1, 2, 3]"#,
            r#"["draw", 0]"#,
            r#"["circle"]"#,
            r#"["circle", 1, 2]"#,
            r#"["dot"]"#,
            r#"["dot", 1, 2]"#,
            r#"["pen"]"#,
            r#"["pen", "red", "blue"]"#,
            r#"["text"]"#,
            r#"["text", {}, {}]"#,
        ] {
            let error = refusal_of(customisation_with(&test_valve_with(stroke)));
            assert_eq!(error.code, ErrorCode::ParseFailure, "{stroke}");
            expect_names(
                &error,
                r#"symbols[0] "TEST Valve" strokes[9]"#,
                stroke_word(stroke),
            );
        }
    }

    #[test]
    fn a_stroke_that_is_not_one_of_the_seven_kinds_is_refused() {
        for stroke in [
            r#"["curve", 1, 2]"#,
            r#"["Move", 0, 0]"#,
            r#"["colour", "red"]"#,
        ] {
            let error = refusal_of(customisation_with(&test_valve_with(stroke)));
            assert_eq!(error.code, ErrorCode::ParseFailure, "{stroke}");
            expect_names(
                &error,
                r#"symbols[0] "TEST Valve" strokes[9]"#,
                stroke_word(stroke),
            );
        }
        // And what is not a stroke's shape at all.
        for stroke in [
            r#""move 0 0""#,
            "[]",
            "[0, 0]",
            r#"{"move": [0, 0]}"#,
            "null",
        ] {
            let error = refusal_of(customisation_with(&test_valve_with(stroke)));
            assert_eq!(error.code, ErrorCode::ParseFailure, "{stroke}");
            assert!(
                error
                    .message
                    .starts_with(r#"symbols[0] "TEST Valve" strokes[9]: "#),
                "{}",
                error.describe()
            );
        }
    }

    #[test]
    fn a_stroke_with_a_value_of_the_wrong_kind_is_refused() {
        let at = r#"symbols[0] "TEST Valve" strokes[9]"#;
        expect_refused(
            &[
                case(test_valve_with(r#"["move", "0", 0]"#), at, "move"),
                case(test_valve_with(r#"["draw", 0, null]"#), at, "draw"),
                case(test_valve_with(r#"["arc", 1, true, 3]"#), at, "arc"),
                case(test_valve_with(r#"["circle", [1]]"#), at, "circle"),
                case(test_valve_with(r#"["dot", "0"]"#), at, "dot"),
                case(test_valve_with(r#"["pen", 35]"#), at, "pen"),
                case(
                    test_valve_with(r#"["text", {"height": "1.5"}]"#),
                    at,
                    "height",
                ),
                case(
                    test_valve_with(r#"["text", {"extra": [0, 0]}]"#),
                    at,
                    "extra",
                ),
                case(
                    test_valve_with(r#"["text", {"extra": [0, 0, "0"]}]"#),
                    at,
                    "extra",
                ),
                // a fourth number is not three
                case(
                    test_valve_with(r#"["text", {"extra": [0, 0, 0, 0]}]"#),
                    at,
                    "extra",
                ),
                case(test_valve_with(r#"["text", {"extra": 0}]"#), at, "extra"),
            ],
            ErrorCode::ParseFailure,
        );
        let text = refusal_of(customisation_with(&test_valve_with(r#"["text", "W"]"#)));
        assert_eq!(text.code, ErrorCode::ParseFailure);
        assert!(
            text.message
                .starts_with(r#"symbols[0] "TEST Valve" strokes[9]: "#),
            "{}",
            text.describe()
        );
    }

    // ---- well-formed, and still not a customisation -------------------------------------

    #[test]
    fn two_definitions_of_one_name_are_refused() {
        let within = refusal_of(customisation_with(
            r#""linestyles": [{"name": "A"}, {"name": "B"}, {"name": "A"}]"#,
        ));
        assert_eq!(within.code, ErrorCode::InvalidArgument);
        assert!(
            within.message.starts_with(r#"linestyles[2] "A": "#),
            "{}",
            within.describe()
        );

        // The two lists are one library: a name is taken once across both.
        let across = refusal_of(customisation_with(
            r#""linestyles": [{"name": "A"}], "symbols": [{"name": "B"}, {"name": "A"}]"#,
        ));
        assert_eq!(across.code, ErrorCode::InvalidArgument);
        assert!(
            across.message.starts_with(r#"symbols[1] "A": "#),
            "{}",
            across.describe()
        );
        assert!(
            across.message.contains("\"linestyles\""),
            "it says where the other one is: {}",
            across.describe()
        );

        // Names are compared with regard to case, as everywhere in the model.
        assert_eq!(
            read_customisation(customisation_with(
                r#""linestyles": [{"name": "A"}, {"name": "a"}]"#
            ))
            .library
            .size(),
            2
        );
    }

    #[test]
    fn two_colour_names_that_fold_to_one_are_refused() {
        // Lower case, '_' and '-' as a blank, "gray" as "grey": spellings of
        // one name. The members of a JSON object have no order, so "the second
        // of the two" cannot be the file's second: the names are gone through
        // in the order of their bytes, the later of the two in THAT order is
        // the entry refused, and both spellings are beside it, the earlier
        // first. By hand: 'S' (53) sorts before 's' (73), ' ' (20) before '-'
        // (2D), and 'O' (4F) before 'o' (6F).
        for (colours, earlier, refused) in [
            (
                r##"{"SUI Gas": "#FFFF00", "sui_gas": "#FF0000"}"##,
                "SUI Gas",
                "sui_gas",
            ),
            // the same two the other way round in the file: the same answer
            (
                r##"{"sui_gas": "#FF0000", "SUI Gas": "#FFFF00"}"##,
                "SUI Gas",
                "sui_gas",
            ),
            (
                r##"{"sui gas": "#FFFF00", "sui-gas": "#FF0000"}"##,
                "sui gas",
                "sui-gas",
            ),
            (
                r##"{"off gray": "#808080", "Off Grey": "#808081"}"##,
                "Off Grey",
                "off gray",
            ),
        ] {
            let error = refusal_of(customisation_with(&format!("\"colours\": {colours}")));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{colours}");
            assert!(
                error
                    .message
                    .starts_with(&format!("colours \"{refused}\": ")),
                "{}",
                error.describe()
            );
            assert_eq!(
                error.context,
                format!("\"{earlier}\" and \"{refused}\""),
                "{}",
                error.describe()
            );
        }
    }

    #[test]
    fn a_colour_name_that_is_a_standard_name_is_refused() {
        // "red" in any spelling the fold reads as red, and a standard name of
        // two words; a table that redefined one would draw one name in two
        // colours.
        expect_refused(
            &[
                case(
                    r##""colours": {"red": "#FF0001"}"##,
                    r#"colours "red""#,
                    "red",
                ),
                case(
                    r##""colours": {"sui gas": "#FFFF00", "Red": "#FF0000"}"##,
                    r#"colours "Red""#,
                    "red",
                ),
                case(
                    r##""colours": {"Dark_Gray": "#404040"}"##,
                    r#"colours "Dark_Gray""#,
                    "dark grey",
                ),
            ],
            ErrorCode::InvalidArgument,
        );
    }

    #[test]
    fn a_colour_that_is_not_hexadecimal_is_refused() {
        expect_refused(
            &[
                case(
                    r#""colours": {"sui gas": "yellow"}"#,
                    r#"colours "sui gas""#,
                    "yellow",
                ),
                case(
                    r##""colours": {"sui gas": "#FFFF0"}"##,
                    r#"colours "sui gas""#,
                    "#FFFF0",
                ),
                case(
                    r##""colours": {"sui gas": "#GGFF00"}"##,
                    r#"colours "sui gas""#,
                    "#GGFF00",
                ),
            ],
            ErrorCode::ParseFailure,
        );
        let number = refusal_of(customisation_with(r#""colours": {"sui gas": 16776960}"#));
        assert_eq!(number.code, ErrorCode::ParseFailure);
        assert!(
            number.message.starts_with(r#"colours "sui gas": "#),
            "{}",
            number.describe()
        );

        // Either case of digit is read, and a fourth pair is the opacity.
        let read = read_customisation(customisation_with(
            r##""colours": {"sui gas": "#ffFF00", "glass": "#0102037f"}"##,
        ));
        assert_eq!(
            read.colours.find("sui gas"),
            Some(Color::new(255, 255, 0, 255))
        );
        assert_eq!(read.colours.find("glass"), Some(Color::new(1, 2, 3, 127)));
    }

    #[test]
    fn a_key_with_a_wildcard_anywhere_but_last_is_refused() {
        for key in ["W*M", "*WM", "W**", "WM?"] {
            let error = refusal_of(customisation_with(&format!(
                r#""codes": [{{"key": "A", "sets": "feature"}}, {{"key": "{key}", "sets": "feature"}}]"#
            )));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{key}");
            let entry = format!("codes[1] \"{key}\": ");
            assert!(error.message.starts_with(&entry), "{}", error.describe());
            assert!(
                error.message[entry.len()..].contains("key"),
                "it is the key that is wrong: {}",
                error.describe()
            );
        }
    }

    #[test]
    fn a_key_with_blanks_around_it_is_refused_rather_than_trimmed() {
        for key in [" WM*", "WM* ", r"\tWM"] {
            let error = refusal_of(customisation_with(&format!(
                r#""codes": [{{"key": "{key}", "sets": "feature"}}]"#
            )));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{key}");
            assert!(
                error.message.starts_with("codes[0] \""),
                "{}",
                error.describe()
            );
            assert!(error.message.contains("key"), "{}", error.describe());
        }
        let empty = refusal_of(customisation_with(
            r#""codes": [{"key": "", "sets": "feature"}]"#,
        ));
        assert_eq!(empty.code, ErrorCode::InvalidArgument);
        assert!(
            empty.message.starts_with(r#"codes[0] "": "#),
            "{}",
            empty.describe()
        );
    }

    #[test]
    fn a_layer_that_is_not_a_layer_path_is_refused() {
        // An empty level, a leading separator, "..", and a level that is a
        // blank.
        for layer in ["SURVEY//SERVICES", "/SURVEY", "SURVEY/..", "SURVEY/ /X"] {
            let error = refusal_of(customisation_with(&format!(
                r#""codes": [{{"key": "WM*", "sets": "feature", "layer": "{layer}"}}]"#
            )));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{layer}");
            expect_names(&error, r#"codes[0] "WM*""#, "layer");
        }
        // A path of several levels is one.
        assert_eq!(
            read_customisation(customisation_with(
                r#""codes": [{"key": "WM*", "sets": "feature", "layer": "SURVEY/SERVICES/WATER"}]"#
            ))
            .map
            .rules()[0]
                .model,
            "SURVEY/SERVICES/WATER"
        );
    }

    #[test]
    fn what_validate_refuses_of_a_definition_is_refused_naming_it() {
        // A factor of 0, a negative length, a negative text height: numbers the
        // format can write and the model will not take.
        for definition in [
            r#"{"name": "D", "factor": 0}"#,
            r#"{"name": "D", "factor": -1}"#,
            r#"{"name": "D", "length": -2.5}"#,
            r#"{"name": "D", "strokes": [["text", {"height": -1}]]}"#,
        ] {
            let error = refusal_of(customisation_with(&format!(
                r#""linestyles": [{{"name": "C"}}, {definition}]"#
            )));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{definition}");
            assert!(
                error.message.starts_with(r#"linestyles[1] "D": "#),
                "{}",
                error.describe()
            );
        }
        let unnamed = refusal_of(customisation_with(r#""symbols": [{"name": ""}]"#));
        assert_eq!(unnamed.code, ErrorCode::InvalidArgument);
        assert!(
            unnamed.message.starts_with(r#"symbols[0] "": "#),
            "{}",
            unnamed.describe()
        );
        let no_name = refusal_of(customisation_with(r#""symbols": [{"group": "G"}]"#));
        assert_eq!(no_name.code, ErrorCode::ParseFailure);
        expect_names(&no_name, "symbols[0]", "name");
    }

    #[test]
    fn linework_codes_that_could_not_be_told_apart_are_refused() {
        // "st" and "ST" are one token to the reader of a field code; a spelling
        // with a blank in it could never be one token at all. Which controls are
        // at fault is beside the refusal, in the words validate says it
        // (linework_codes.rs): the two that clash, the earlier first, with the
        // earlier's spelling - "join" given the default spelling of "end"
        // clashes with "end" - or the one control and its spelling.
        for (linework, context) in [
            (
                r#"{"start": "ST", "end": "st"}"#,
                "start and end are \"ST\"",
            ),
            (r#"{"close": "C L"}"#, "close=\"C L\""),
            (r#"{"join": "END"}"#, "end and join are \"END\""),
        ] {
            let error = refusal_of(customisation_with(&format!("\"linework\": {linework}")));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{linework}");
            assert!(
                error.message.starts_with("linework: "),
                "{}",
                error.describe()
            );
            assert_eq!(error.context, context, "{}", error.describe());
        }
    }

    #[test]
    fn a_negative_size_is_refused_as_negative_and_not_as_not_finite() {
        // 0 is "the definition's own size" for a symbol and "not said" for a
        // text; below it there is no size to draw at. But -1 is a finite
        // number, and the refusal must say what is wrong with it, not something
        // else.
        for rule in [
            r#"{"key": "A", "sets": "symbol", "symbol": {"name": "P", "size": -1}}"#,
            r#"{"key": "A", "sets": "text", "text": {"style": "ISO", "size": -0.5}}"#,
        ] {
            let error = refusal_of(customisation_with(&format!("\"codes\": [{rule}]")));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{rule}");
            assert!(
                error.message.starts_with(r#"codes[0] "A": "#),
                "{}",
                error.describe()
            );
            assert!(
                error.message.contains("size cannot be negative"),
                "{}",
                error.describe()
            );
            assert!(!error.message.contains("finite"), "{}", error.describe());
        }

        // The model's own check says the two apart as well: a size that is not
        // a number at all is "not finite", one below zero is "negative".
        let mut symbol = SurveyRule {
            key: String::from("A"),
            symbol: Some(SurveySymbol {
                size: f64::NAN,
                ..SurveySymbol::default()
            }),
            ..SurveyRule::default()
        };
        let status = validate(&symbol).expect_err("a NaN size");
        assert!(status.message.contains("finite"), "{}", status.describe());
        if let Some(value) = symbol.symbol.as_mut() {
            value.size = -1.0;
        }
        let status = validate(&symbol).expect_err("a negative size");
        assert_eq!(status.message, "a symbol's size cannot be negative");
        let mut text = SurveyRule {
            key: String::from("A"),
            text_style: Some(SurveyTextStyle {
                size: f64::NEG_INFINITY,
                ..SurveyTextStyle::default()
            }),
            ..SurveyRule::default()
        };
        let status = validate(&text).expect_err("an infinite size");
        assert!(status.message.contains("finite"), "{}", status.describe());
        if let Some(style) = text.text_style.as_mut() {
            style.size = -0.5;
        }
        let status = validate(&text).expect_err("a negative size");
        assert_eq!(status.message, "a text style's size cannot be negative");

        // Every other number of the two may be below zero, and negative zero is
        // not below it.
        let read = read_customisation(customisation_with(
            r#""codes": [{"key": "A", "sets": "symbol",
                      "symbol": {"size": -0.0, "rotation": -45, "offset": -0.2, "raise": -0.1},
                      "text": {"size": -0.0, "offset": -0.5, "raise": -0.25, "angle": -30,
                               "slant": -10}}]"#,
        ));
        assert_eq!(read.map.size(), 1);
        let rule = &read.map.rules()[0];
        let symbol = rule.symbol.as_ref().expect("a symbol");
        assert_eq!(symbol.rotation, -45.0);
        assert_eq!(symbol.raise, -0.1);
        let style = rule.text_style.as_ref().expect("a text style");
        assert_eq!(style.angle, -30.0);
        assert_eq!(style.slant, -10.0);
    }

    // ---- numbers a double cannot hold ---------------------------------------------------

    // 1e400 is JSON, and past the largest double (about 1.8e308). The JSON
    // reader stops at such a number; what the reader then says is what it
    // would say of any value of the wrong kind - the entry and the member -
    // with the number and where it stands beside it.
    #[test]
    fn a_number_too_large_to_hold_is_refused_naming_the_entry_the_member_and_the_place() {
        // The place is worked out by hand. Line 2 is
        //    ` "linestyles": [{"name": "A", "length": 1e400}]}`
        // one blank, then "linestyles" in its quotes (12 characters, columns 2
        // to 13), `: [{` (14 to 17), "name" (18 to 23), `: ` (24, 25), "A" (26
        // to 28), `, ` (29, 30), "length" (31 to 38), `: ` (39, 40), and 1e400
        // in columns 41 to 45. The column given is that of the number's LAST
        // character, which is where a JSON reader stands once it has read it.
        let error = refusal_of(concat!(
            "{\"format\": \"katana-customisation\", \"version\": 1, \"name\": \"T\",\n",
            " \"linestyles\": [{\"name\": \"A\", \"length\": 1e400}]}"
        ));
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert_eq!(
            error.message,
            format!(r#"linestyles[0] "A": "length" {TOO_LARGE}"#)
        );
        assert_eq!(error.context, "1e400 at line 2, column 45");

        let cases = [
            // below the least as well as above the greatest
            (
                String::from(r#""linestyles": [{"name": "A", "factor": -1e400}]"#),
                format!(r#"linestyles[0] "A": "factor" {TOO_LARGE}"#),
                "-1e400",
            ),
            // just past the largest double, 1.7976931348623157e308
            (
                String::from(r#""linestyles": [{"name": "A", "length": 2e308}]"#),
                format!(r#"linestyles[0] "A": "length" {TOO_LARGE}"#),
                "2e308",
            ),
            // in a list under a member: the member is still what is named
            (
                String::from(r#""linestyles": [{"name": "A", "origin": [0, 1e400]}]"#),
                format!(r#"linestyles[0] "A": "origin" {TOO_LARGE}"#),
                "1e400",
            ),
            (
                String::from(r#""linestyles": [{"name": "A", "anchors": [[0, 0], [1e999, 0]]}]"#),
                format!(r#"linestyles[0] "A": "anchors" {TOO_LARGE}"#),
                "1e999",
            ),
            // in a stroke, and in the object and the list a text stroke holds
            (
                test_valve_with(r#"["move", 1e400, 0]"#),
                format!(r#"symbols[0] "TEST Valve" strokes[9]: this stroke {TOO_LARGE}"#),
                "1e400",
            ),
            (
                test_valve_with(r#"["text", {"height": 1E+400}]"#),
                format!(r#"symbols[0] "TEST Valve" strokes[9]: "height" {TOO_LARGE}"#),
                "1E+400",
            ),
            (
                test_valve_with(r#"["text", {"extra": [0, 1e400, 0]}]"#),
                format!(r#"symbols[0] "TEST Valve" strokes[9]: "extra" {TOO_LARGE}"#),
                "1e400",
            ),
            // in a part of a rule
            (
                String::from(
                    r#""codes": [{"key": "A", "sets": "symbol", "symbol": {"name": "P", "size": 1e400}}]"#,
                ),
                format!(r#"codes[0] "A" symbol: "size" {TOO_LARGE}"#),
                "1e400",
            ),
            (
                String::from(
                    r#""codes": [{"key": "A", "sets": "text", "text": {"slant": 1e400}}]"#,
                ),
                format!(r#"codes[0] "A" text: "slant" {TOO_LARGE}"#),
                "1e400",
            ),
            // where no number belongs at all, it is still the number that
            // stopped the reading
            (
                String::from(r#""description": 1e400"#),
                format!(r#"top level: "description" {TOO_LARGE}"#),
                "1e400",
            ),
            // an entry whose name comes after the number has no name to show
            // yet
            (
                String::from(r#""linestyles": [{"length": 1e400, "name": "A"}]"#),
                format!(r#"linestyles[0]: "length" {TOO_LARGE}"#),
                "1e400",
            ),
            // bare among the strokes, it is in the list that "strokes" is
            (
                test_valve_with("1e400"),
                format!(r#"symbols[0] "TEST Valve": "strokes" {TOO_LARGE}"#),
                "1e400",
            ),
            // in an entry of a list inside a rule, and in the file's own objects
            (
                String::from(
                    r#""codes": [{"key": "A", "sets": "attributes", "attributes": [{"type": "text", "name": "N"}, {"type": "text", "name": "M", "value": 1e400}]}]"#,
                ),
                format!(r#"codes[0] "A" attributes[1]: "value" {TOO_LARGE}"#),
                "1e400",
            ),
            (
                String::from(r#""linework": {"start": "S", "end": 1e400}"#),
                format!(r#"linework: "end" {TOO_LARGE}"#),
                "1e400",
            ),
            (
                String::from(r#""colours": {"sui gas": 1e400}"#),
                format!(r#"colours: "sui gas" {TOO_LARGE}"#),
                "1e400",
            ),
            (
                String::from(r#""sources": [{"name": "A"}, {"name": "B", "rules": 1e400}]"#),
                format!(r#"sources[1] "B": "rules" {TOO_LARGE}"#),
                "1e400",
            ),
        ];
        for (members, message, number) in &cases {
            let refused = refusal_of(customisation_with(members));
            assert_eq!(refused.code, ErrorCode::ParseFailure, "{members}");
            assert_eq!(&refused.message, message, "{members}");
            assert!(
                refused
                    .context
                    .starts_with(&format!("{number} at line 1, column ")),
                "{}",
                refused.describe()
            );
        }

        // The largest double itself, and its negative, are numbers the JSON
        // reader holds, and so are no number "too large to hold"; they are past
        // the format's own bound (CUSTOMISATION_MOST_MAGNITUDE), and refused for
        // that, by the member (customisation_bounds.rs tries the bound from
        // both sides).
        let length = refusal_of(customisation_with(
            r#""linestyles": [{"name": "A", "length": 1.7976931348623157e308}]"#,
        ));
        assert_eq!(length.code, ErrorCode::ParseFailure);
        assert_eq!(
            length.message,
            r#"linestyles[0] "A": "length" is larger than 1000000000 in size, the most any number of a customisation holds"#
        );
        let origin = refusal_of(customisation_with(
            r#""linestyles": [{"name": "A", "origin": [-1.7976931348623157e308, 0]}]"#,
        ));
        assert_eq!(
            origin.message,
            r#"linestyles[0] "A": "origin" is larger than 1000000000 in size, the most any number of a customisation holds"#
        );
    }

    // The reading stops AT such a number, so what a file is told depends on
    // what it had said by then. The writer puts "format" and "version" first,
    // and so does anyone who starts from a file it wrote.
    #[test]
    fn a_number_too_large_before_the_file_says_what_it_is_is_not_a_katana_customisation_file() {
        for text in [
            // the same file with its members the other way round
            r#"{"linestyles": [{"name": "A", "length": 1e400}], "format": "katana-customisation", "version": 1, "name": "T"}"#,
            // JSON of some other kind
            r#"{"type": "FeatureCollection", "bbox": [0, 0, 1e400, 1]}"#,
            r#"{"format": "katana-sheets", "version": 1, "scale": 1e400}"#,
            // not an object at all
            "1e400",
            "[1e400]",
            "[[0, -1e400]]",
        ] {
            let error = refusal_of(text);
            assert_eq!(error.code, ErrorCode::ParseFailure, "{text}");
            assert_eq!(error.message, NOT_A_CUSTOMISATION, "{text}");
            assert!(
                error.context.contains("1e400 at line 1, column "),
                "{}",
                error.describe()
            );
        }

        // A file that has said it is NEWER is told that, as for anything else
        // a newer file may hold.
        let newer = refusal_of(
            r#"{"format": "katana-customisation", "version": 2, "name": "T", "limit": 1e400}"#,
        );
        assert_eq!(newer.code, ErrorCode::Unsupported);
        assert!(
            newer.message.contains("newer version of Katana"),
            "{}",
            newer.describe()
        );
    }

    // One definition's text has no "format" to say first: it is a definition
    // as soon as it is an object, and the number is refused by its member, or
    // by its stroke, under the definition's own name.
    #[test]
    fn a_number_too_large_in_a_definitions_text_is_refused_the_same_way() {
        let member = definition_from_json(br#"{"name": "A", "length": 1e400}"#, false, "")
            .expect_err("a length too large to hold");
        assert_eq!(member.code, ErrorCode::ParseFailure);
        assert_eq!(
            member.message,
            format!(r#"definition "A": "length" {TOO_LARGE}"#)
        );
        // {"name": "A", "length": 1e400} - by hand, the number's last character
        // is in column 29: `{"name": "A", ` is 14 characters, `"length": ` 10
        // more, and the number the next 5.
        assert_eq!(member.context, "1e400 at line 1, column 29");

        let stroke = definition_from_json(
            br#"{"name": "A", "strokes": [["move", 0, 0], ["circle", -1e400]]}"#,
            true,
            "",
        )
        .expect_err("a radius too large to hold");
        assert_eq!(stroke.code, ErrorCode::ParseFailure);
        assert_eq!(
            stroke.message,
            format!(r#"definition "A" strokes[1]: this stroke {TOO_LARGE}"#)
        );
        assert!(
            stroke.context.starts_with("-1e400 at line 1, column "),
            "{}",
            stroke.describe()
        );

        let text = definition_from_json(
            br#"{"name": "A", "strokes": [["text", {"extra": [0, 0, 1e400]}]]}"#,
            true,
            "",
        )
        .expect_err("an extra number too large to hold");
        assert_eq!(
            text.message,
            format!(r#"definition "A" strokes[0]: "extra" {TOO_LARGE}"#)
        );

        // Before the name is read there is none to show; and a text that is not
        // an object is not a definition at all.
        let unnamed = definition_from_json(br#"{"factor": 1e400, "name": "A"}"#, false, "")
            .expect_err("a factor too large to hold");
        assert_eq!(
            unnamed.message,
            format!(r#"definition: "factor" {TOO_LARGE}"#)
        );
        for not_one in ["1e400", "[1e400]"] {
            let refused = definition_from_json(not_one.as_bytes(), false, "").expect_err(not_one);
            assert_eq!(refused.code, ErrorCode::ParseFailure, "{not_one}");
            assert_eq!(refused.message, NOT_A_DEFINITION, "{not_one}");
            assert!(
                refused.context.contains("1e400 at line 1, column "),
                "{}",
                refused.describe()
            );
        }
    }

    // 1e-400 is below the smallest double (about 4.9e-324). The JSON reader
    // reads it as 0 and says nothing - and 0 is "not said" for a length, "the
    // definition's own" for a size, and refused for a factor in words that
    // blame a zero nobody wrote.
    #[test]
    fn a_number_too_small_to_hold_is_refused_rather_than_read_as_zero() {
        let zeros = "0".repeat(400); // 0.000...0001, written without an exponent
        let cases = [
            (
                String::from(r#""linestyles": [{"name": "A", "length": 1e-400}]"#),
                format!(r#"linestyles[0] "A": "length" {TOO_SMALL}"#),
                String::from("1e-400"),
            ),
            (
                String::from(r#""linestyles": [{"name": "A", "factor": 1E-400}]"#),
                format!(r#"linestyles[0] "A": "factor" {TOO_SMALL}"#),
                String::from("1E-400"),
            ),
            (
                String::from(r#""linestyles": [{"name": "A", "origin": [-1e-400, 0]}]"#),
                format!(r#"linestyles[0] "A": "origin" {TOO_SMALL}"#),
                String::from("[-1e-400,0]"),
            ),
            (
                String::from(
                    r#""linestyles": [{"name": "A", "anchors": [[0, 0], [0, 2.5e-999]]}]"#,
                ),
                format!(r#"linestyles[0] "A": "anchors" {TOO_SMALL}"#),
                String::from("[0,2.5e-999]"),
            ),
            (
                test_valve_with(r#"["move", 1e-400, 0]"#),
                format!(r#"symbols[0] "TEST Valve" strokes[9]: this stroke {TOO_SMALL}"#),
                String::from(r#"["move",1e-400,0]"#),
            ),
            (
                test_valve_with(r#"["arc", 1, 0, -1e-400]"#),
                format!(r#"symbols[0] "TEST Valve" strokes[9]: this stroke {TOO_SMALL}"#),
                String::from(r#"["arc",1,0,-1e-400]"#),
            ),
            (
                test_valve_with(r#"["text", {"angle": 1e-400}]"#),
                format!(r#"symbols[0] "TEST Valve" strokes[9]: "angle" {TOO_SMALL}"#),
                String::from("1e-400"),
            ),
            (
                test_valve_with(r#"["text", {"extra": [0, 0, 1e-999]}]"#),
                format!(r#"symbols[0] "TEST Valve" strokes[9]: "extra" {TOO_SMALL}"#),
                String::from("[0,0,1e-999]"),
            ),
            (
                format!(
                    r#""codes": [{{"key": "A", "sets": "symbol", "symbol": {{"rotation": 0.{zeros}1}}}}]"#
                ),
                format!(r#"codes[0] "A" symbol: "rotation" {TOO_SMALL}"#),
                format!("0.{zeros}1"),
            ),
            (
                String::from(
                    r#""codes": [{"key": "A", "sets": "text", "text": {"size": 1e-400}}]"#,
                ),
                format!(r#"codes[0] "A" text: "size" {TOO_SMALL}"#),
                String::from("1e-400"),
            ),
        ];
        for (members, message, context) in &cases {
            let error = refusal_of(customisation_with(members));
            assert_eq!(error.code, ErrorCode::ParseFailure, "{members}");
            assert_eq!(&error.message, message, "{members}");
            // What is shown of a value is cut at 60 characters.
            let shown = if context.len() > 60 {
                format!("{}...", &context[..60])
            } else {
                context.clone()
            };
            assert_eq!(error.context, shown, "{members}");
        }

        // Where no number belongs, or only a whole one, it is refused as any
        // other value there is - and shown as the file wrote it, not as 0.
        let named = refusal_of(customisation_with(r#""description": 1e-400"#));
        assert_eq!(named.code, ErrorCode::ParseFailure);
        assert_eq!(
            named.message,
            r#"top level: "description" must be text, in double quotes"#
        );
        assert_eq!(named.context, "1e-400");
        let whole = refusal_of(customisation_with(
            r#""linestyles": [{"name": "A", "cycleMode": 1e-400}]"#,
        ));
        assert_eq!(whole.code, ErrorCode::ParseFailure);
        assert_eq!(
            whole.message,
            r#"linestyles[0] "A": "cycleMode" must be a whole number"#
        );
        assert_eq!(whole.context, "1e-400");
        // ... and where a list of strokes, or one stroke of it, belongs.
        let strokes = refusal_of(customisation_with(
            r#""linestyles": [{"name": "A", "strokes": 1e-400}]"#,
        ));
        assert_eq!(strokes.code, ErrorCode::ParseFailure);
        assert_eq!(
            strokes.message,
            r#"linestyles[0] "A": "strokes" must be a list, written [...]"#
        );
        assert_eq!(strokes.context, "1e-400");
        let stroke = refusal_of(customisation_with(&test_valve_with("1e-400")));
        assert_eq!(stroke.code, ErrorCode::ParseFailure);
        assert!(
            stroke
                .message
                .starts_with(r#"symbols[0] "TEST Valve" strokes[9]: a stroke is a list"#),
            "{}",
            stroke.describe()
        );
        assert_eq!(stroke.context, "1e-400");
        let pen = refusal_of(customisation_with(&test_valve_with(r#"["pen", 1e-400]"#)));
        assert_eq!(pen.code, ErrorCode::ParseFailure);
        expect_names(&pen, r#"symbols[0] "TEST Valve" strokes[9]"#, "pen");
        assert_eq!(pen.context, r#"["pen",1e-400]"#);

        // One definition's text is read by the same code.
        let definition = definition_from_json(br#"{"name": "A", "factor": 1e-400}"#, false, "")
            .expect_err("a factor too small to hold");
        assert_eq!(definition.code, ErrorCode::ParseFailure);
        assert_eq!(
            definition.message,
            format!(r#"definition "A": "factor" {TOO_SMALL}"#)
        );
    }

    #[test]
    fn zero_written_at_length_is_zero_and_the_smallest_numbers_a_double_holds_are_read() {
        // Every digit of each of these is 0, whatever its exponent says: they
        // ARE zero, and the sign of the last two is kept.
        let zeros = read_customisation(customisation_with(
            r#""linestyles": [{"name": "A", "length": 0e-400, "origin": [0.000, 0E5],
                           "anchors": [[-0.0e-400, 0.0e999], [-0e-999, 0]]}]"#,
        ));
        let a = zeros.library.find("A").expect("A");
        assert_eq!(a.length, 0.0);
        assert_eq!(a.origin.x, 0.0);
        assert_eq!(a.origin.y, 0.0);
        assert_eq!(a.anchor1.x, 0.0);
        assert!(a.anchor1.x.is_sign_negative());
        assert_eq!(a.anchor1.y, 0.0);
        assert!(!a.anchor1.y.is_sign_negative());
        assert!(a.anchor2.x.is_sign_negative());

        // 5e-324 is the smallest number a double holds (2^-1074) and 1e-320 one
        // of the few thousand just above it, held to fewer digits: neither is
        // zero, and each is read as the nearest double there is.
        let tiny = read_customisation(customisation_with(
            r#""linestyles": [{"name": "A", "length": 5e-324,
                           "strokes": [["move", 1e-320, -5e-324]]}]"#,
        ));
        let b = tiny.library.find("A").expect("A");
        assert_eq!(b.length, f64::from_bits(1)); // denorm_min
        assert_eq!(b.strokes.len(), 1);
        assert!(b.strokes[0].point.x > 0.0);
        assert!(b.strokes[0].point.x < 1.1e-320);
        assert_eq!(b.strokes[0].point.y, -f64::from_bits(1));
    }

    // ---- values nested deep -------------------------------------------------------------

    #[test]
    fn a_value_nested_a_hundred_thousand_deep_is_refused_like_any_other_of_the_wrong_kind() {
        let lists = deep_lists();
        let objects = deep_objects();

        // Where the file has not said it is a customisation: its "format" is
        // the deep value, or the text itself is.
        for text in [
            format!("{{\"format\": {lists}}}"),
            format!("{{\"format\": {objects}}}"),
            lists.clone(),
            objects.clone(),
        ] {
            let error = refusal_of(text);
            assert_eq!(error.code, ErrorCode::ParseFailure);
            assert_eq!(error.message, NOT_A_CUSTOMISATION);
        }

        // Anywhere else it is a value of the wrong kind, refused by the entry
        // and the member exactly as a shallow one is.
        let cases = [
            case(
                format!("\"description\": {objects}"),
                "top level",
                "description",
            ),
            case(format!("\"sources\": {objects}"), "top level", "sources"),
            case(
                format!("\"linestyles\": [{{\"name\": \"A\", \"group\": {lists}}}]"),
                r#"linestyles[0] "A""#,
                "group",
            ),
            case(
                format!("\"linestyles\": [{{\"name\": \"A\", \"origin\": {lists}}}]"),
                r#"linestyles[0] "A""#,
                "origin",
            ),
            case(
                format!("\"linestyles\": [{{\"name\": \"A\", \"strokes\": {objects}}}]"),
                r#"linestyles[0] "A""#,
                "strokes",
            ),
            case(
                format!(
                    "\"symbols\": [{{\"name\": \"A\", \"strokes\": [[\"move\", {lists}, 0]]}}]"
                ),
                r#"symbols[0] "A" strokes[0]"#,
                "move",
            ),
            case(
                format!("\"symbols\": [{{\"name\": \"A\", \"strokes\": [[\"pen\", {objects}]]}}]"),
                r#"symbols[0] "A" strokes[0]"#,
                "pen",
            ),
            case(
                format!(
                    "\"symbols\": [{{\"name\": \"A\", \"strokes\": [[\"text\", {{\"height\": {lists}}}]]}}]"
                ),
                r#"symbols[0] "A" strokes[0]"#,
                "height",
            ),
            case(
                format!(
                    "\"symbols\": [{{\"name\": \"A\", \"strokes\": [[\"text\", {{\"extra\": [0, 0, {lists}]}}]]}}]"
                ),
                r#"symbols[0] "A" strokes[0]"#,
                "extra",
            ),
            case(
                format!(
                    "\"codes\": [{{\"key\": \"A\", \"sets\": \"feature\", \"layer\": {lists}}}]"
                ),
                r#"codes[0] "A""#,
                "layer",
            ),
            case(
                format!("\"codes\": [{{\"key\": \"A\", \"sets\": {objects}}}]"),
                r#"codes[0] "A""#,
                "sets",
            ),
            case(
                format!(
                    "\"codes\": [{{\"key\": \"A\", \"sets\": \"attributes\", \"attributes\": [{{\"type\": \"text\", \"name\": {lists}}}]}}]"
                ),
                r#"codes[0] "A" attributes[0]"#,
                "name",
            ),
            case(format!("\"zzz\": {lists}"), "top level", "zzz"),
            // the first of two is thrown away as the second is put in
            case(
                format!("\"description\": {lists}, \"description\": {objects}"),
                "top level",
                "description",
            ),
        ];
        for refused in &cases {
            let error = refusal_of(customisation_with(&refused.members));
            assert_eq!(
                error.code,
                ErrorCode::ParseFailure,
                "{} {}",
                refused.entry,
                refused.member
            );
            expect_names(&error, &refused.entry, &refused.member);
            // What a refusal shows of a value is cut short however much there
            // is.
            assert!(
                error.context.len() <= 240,
                "{} {}",
                refused.entry,
                refused.member
            );
        }

        // The file's own name, and what is shown of a value: its first 60
        // characters.
        let named = refusal_of(format!(
            r#"{{"format": "katana-customisation", "version": 1, "name": {lists}}}"#
        ));
        assert_eq!(named.code, ErrorCode::ParseFailure);
        assert_eq!(
            named.message,
            r#"top level: "name" must be text, in double quotes"#
        );
        assert_eq!(named.context, format!("{}...", "[".repeat(60)));
        let described = refusal_of(customisation_with(&format!("\"description\": {objects}")));
        // {"a": is five characters, so twelve of them are the first sixty.
        assert_eq!(described.context, format!("{}...", "{\"a\":".repeat(12)));

        // And where an object or a stroke should be.
        for (members, entry) in [
            (format!("\"linestyles\": [{lists}]"), "linestyles[0]"),
            (
                format!(
                    "\"codes\": [{{\"key\": \"A\", \"sets\": \"symbol\", \"symbol\": {lists}}}]"
                ),
                r#"codes[0] "A" symbol"#,
            ),
            (format!("\"basedOn\": {lists}"), "basedOn"),
        ] {
            let error = refusal_of(customisation_with(&members));
            assert_eq!(error.code, ErrorCode::ParseFailure, "{entry}");
            assert_eq!(
                error.message,
                format!("{entry}: must be an object, written {{...}}")
            );
        }
        for stroke in [&lists, &objects] {
            let error = refusal_of(customisation_with(&format!(
                "\"symbols\": [{{\"name\": \"A\", \"strokes\": [[\"move\", 0, 0], {stroke}]}}]"
            )));
            assert_eq!(error.code, ErrorCode::ParseFailure);
            assert!(
                error.message.starts_with(r#"symbols[0] "A" strokes[1]: "#),
                "{}",
                error.message
            );
        }
        let colour = refusal_of(customisation_with(&format!(
            "\"colours\": {{\"sui gas\": {lists}}}"
        )));
        assert_eq!(colour.code, ErrorCode::ParseFailure);
        assert!(
            colour.message.starts_with(r#"colours "sui gas": "#),
            "{}",
            colour.message
        );
    }

    #[test]
    fn a_definitions_text_nested_a_hundred_thousand_deep_is_refused_the_same_way() {
        let lists = deep_lists();
        let not_one = definition_from_json(lists.as_bytes(), false, "").expect_err("a list");
        assert_eq!(not_one.code, ErrorCode::ParseFailure);
        assert_eq!(not_one.message, NOT_A_DEFINITION);

        let member = definition_from_json(
            format!("{{\"name\": \"A\", \"group\": {lists}}}").as_bytes(),
            false,
            "",
        )
        .expect_err("a group that is a list");
        assert_eq!(member.code, ErrorCode::ParseFailure);
        expect_names(&member, r#"definition "A""#, "group");

        let stroke = definition_from_json(
            format!(
                "{{\"name\": \"A\", \"strokes\": [[\"circle\", {}]]}}",
                deep_objects()
            )
            .as_bytes(),
            false,
            "",
        )
        .expect_err("a radius that is an object");
        assert_eq!(stroke.code, ErrorCode::ParseFailure);
        expect_names(&stroke, r#"definition "A" strokes[0]"#, "circle");
    }
}

// ---- what the writer refuses ----------------------------------------------------------------

mod customisation_writer {
    use super::*;

    #[test]
    fn a_number_that_is_not_finite_is_refused_naming_the_stroke() {
        // validate does not look at a text's three trailing numbers, so a
        // library can hold one that is not a number at all; JSON cannot.
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut definition = named("N");
            definition.strokes.push(stroke_of(StrokeOp::Move));
            definition.strokes.push(Stroke {
                text: 0,
                ..stroke_of(StrokeOp::Text)
            });
            definition
                .texts
                .push(text_of("W", 0.0, 1.0, 1.0, [0.0, bad, 0.0]));
            let error = write_refusal(&holding(definition));
            assert_eq!(error.code, ErrorCode::InvalidArgument);
            expect_names(&error, r#"linestyles[0] "N" strokes[1]"#, "extra");
        }
    }

    #[test]
    fn a_stroke_carrying_a_member_its_kind_does_not_use_is_refused() {
        // The model has one flat stroke for all seven kinds, and validate lets
        // any member be set on any of them; a file has a place only for the
        // ones the kind uses. Each of these would come back without the stray
        // member.
        let cases = [
            (
                Stroke {
                    radius: 1.0,
                    ..stroke_of(StrokeOp::Move)
                },
                "move",
                "a radius",
            ),
            (
                Stroke {
                    end_angle: 90.0,
                    ..stroke_of(StrokeOp::Draw)
                },
                "draw",
                "an angle",
            ),
            (
                Stroke {
                    point: Point2::new(1.0, 0.0),
                    ..stroke_of(StrokeOp::Circle)
                },
                "circle",
                "a point",
            ),
            (
                Stroke {
                    pen: String::from("red"),
                    ..stroke_of(StrokeOp::Circle)
                },
                "circle",
                "a pen",
            ),
            (
                Stroke {
                    start_angle: 1.0,
                    ..stroke_of(StrokeOp::Dot)
                },
                "dot",
                "an angle",
            ),
            (
                Stroke {
                    pen: String::from("red"),
                    radius: 2.0,
                    ..stroke_of(StrokeOp::Pen)
                },
                "pen",
                "a radius",
            ),
            (
                Stroke {
                    point: Point2::new(0.0, 1.0),
                    ..stroke_of(StrokeOp::Arc)
                },
                "arc",
                "a point",
            ),
            // Negative zero is a value the kind does not use too: == calls it
            // 0, and the bits would not come back.
            (
                Stroke {
                    radius: -0.0,
                    ..stroke_of(StrokeOp::Move)
                },
                "move",
                "a radius",
            ),
        ];

        for (stray, kind, what) in cases {
            let mut definition = named("S");
            definition.strokes = vec![stroke_of(StrokeOp::Move), stray];
            let error = write_refusal(&holding(definition));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{kind}");
            assert!(
                error
                    .message
                    .starts_with(r#"linestyles[0] "S" strokes[1]: "#),
                "{}",
                error.describe()
            );
            assert!(
                error
                    .message
                    .contains(&format!("this {kind} stroke carries {what}")),
                "{}",
                error.describe()
            );
        }

        // A stroke that is not a text and names one.
        let mut naming = named("S");
        naming.strokes = vec![Stroke {
            text: 0,
            ..stroke_of(StrokeOp::Draw)
        }];
        naming.texts = vec![StrokeText::default()];
        let error = write_refusal(&holding(naming));
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error
                .message
                .starts_with(r#"linestyles[0] "S" strokes[0]: "#),
            "{}",
            error.describe()
        );
    }

    #[test]
    fn texts_that_are_not_exactly_the_text_strokes_in_order_are_refused() {
        let text_stroke = |index: usize| Stroke {
            text: index,
            ..stroke_of(StrokeOp::Text)
        };
        let lettered = |letter: &str| StrokeText {
            text: String::from(letter),
            ..StrokeText::default()
        };
        // A text no stroke places: it would be gone on the way back.
        let mut unplaced = named("T");
        unplaced.strokes = vec![stroke_of(StrokeOp::Move)];
        unplaced.texts = vec![StrokeText::default()];
        let error = write_refusal(&holding(unplaced));
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error.message.starts_with(r#"linestyles[0] "T": "#),
            "{}",
            error.describe()
        );
        assert!(
            error.message.contains("1 text and 0 text strokes"),
            "{}",
            error.describe()
        );

        // One text placed by two strokes: it would come back as two texts.
        let mut shared = named("T");
        shared.strokes = vec![text_stroke(0), text_stroke(0)];
        shared.texts = vec![StrokeText::default()];
        let error = write_refusal(&holding(shared));
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error
                .message
                .starts_with(r#"linestyles[0] "T" strokes[1]: "#),
            "{}",
            error.describe()
        );

        // Two texts placed in the other order: the file keeps a text inside its
        // stroke, so the texts would come back swapped.
        let mut swapped = named("T");
        swapped.strokes = vec![text_stroke(1), text_stroke(0)];
        swapped.texts = vec![lettered("a"), lettered("b")];
        let error = write_refusal(&holding(swapped));
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error
                .message
                .starts_with(r#"linestyles[0] "T" strokes[0]: "#),
            "{}",
            error.describe()
        );

        // In step, the same two are written.
        let mut sound = named("T");
        sound.strokes = vec![text_stroke(0), stroke_of(StrokeOp::Move), text_stroke(1)];
        sound.texts = vec![lettered("a"), lettered("b")];
        assert!(
            customisation_to_json(&holding(sound), &CustomisationWriteOptions::default()).is_ok()
        );
    }

    #[test]
    fn an_attribute_of_any_other_type_is_refused_because_it_could_not_be_read_back() {
        for kind in ["real", "", "Text"] {
            let rule = SurveyRule {
                key: String::from("A*"),
                attributes: vec![attribute("text", "Owner", "x")],
                vertex_attributes: vec![
                    attribute("integer", "N", "1"),
                    attribute(kind, "Depth", "1.2"),
                ],
                ..SurveyRule::default()
            };
            let error = write_refusal(&holding_rule(rule));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{kind}");
            expect_names(&error, r#"codes[0] "A*" vertexAttributes[1]"#, "type");
        }
    }

    #[test]
    fn what_the_reader_would_refuse_of_the_header_is_not_written() {
        // Each of these, written, would be a file Katana could not open again.
        let unnamed = Customisation::default();
        let error = write_refusal(&unnamed);
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        expect_names(&error, "top level", "name");

        let path = Customisation {
            name: String::from("Roads/2026"),
            ..Customisation::default()
        };
        let error = write_refusal(&path);
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        expect_names(&error, "top level", "name");

        let source = Customisation {
            name: String::from("W"),
            sources: vec![
                CustomisationSourceNote {
                    name: String::from("fine"),
                    ..CustomisationSourceNote::default()
                },
                CustomisationSourceNote {
                    name: String::from("a\\b"),
                    ..CustomisationSourceNote::default()
                },
            ],
            ..Customisation::default()
        };
        let error = write_refusal(&source);
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        expect_names(&error, r#"sources[1] "a\\b""#, "name");

        let based = Customisation {
            name: String::from("W"),
            based_on: Some(CustomisationBase {
                name: String::from("NSW"),
                digest: String::from("not a digest"),
            }),
            ..Customisation::default()
        };
        let error = write_refusal(&based);
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        expect_names(&error, "basedOn", "digest");

        let linework = Customisation {
            name: String::from("W"),
            linework: Some(LineworkCodes {
                end: String::from("ST"), // the start's spelling
                ..LineworkCodes::default()
            }),
            ..Customisation::default()
        };
        let error = write_refusal(&linework);
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error.message.starts_with("linework: "),
            "{}",
            error.describe()
        );

        let mut from = Customisation {
            name: String::from("W"),
            ..Customisation::default()
        };
        let definition = LineStyle {
            source: String::from("x/y"),
            ..named("D")
        };
        assert!(from.library.add(definition).is_ok());
        let error = write_refusal(&from);
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        expect_names(&error, r#"linestyles[0] "D""#, "from");
    }

    // The claim the writer's refusals rest on: whatever it does write, the
    // reader takes back. Shown here for the values nearest the refusals above -
    // the members a kind DOES use at awkward values, texts in step, both
    // attribute types, a source of another name and of none.
    #[test]
    fn what_it_does_write_next_to_each_refusal_reads_back_equal() {
        let mut customisation = Customisation {
            name: String::from("W"),
            ..Customisation::default()
        };
        let mut definition = named("Edge");
        definition.source = String::new();
        let arc = Stroke {
            radius: -1.75,
            start_angle: 720.0,
            end_angle: 0.5, // 719.5 degrees back: within the two turns an arc may sweep
            ..stroke_of(StrokeOp::Arc)
        };
        let dot = stroke_of(StrokeOp::Dot); // radius 0: the smallest drawable
        let pen = stroke_of(StrokeOp::Pen); // an empty pen name is still a pen
        let text = Stroke {
            text: 0,
            ..stroke_of(StrokeOp::Text)
        };
        definition.strokes = vec![arc, dot, pen, text];
        definition.texts = vec![text_of("", 0.0, 0.0, 1.0, [1e-300, -1e9, 0.1])];
        assert!(customisation.library.add(definition).is_ok());
        let rule = SurveyRule {
            key: String::from("A*"),
            segment_attributes: vec![
                attribute("integer", "N", ""),
                attribute("text", "N", "the same name twice is kept"),
            ],
            ..SurveyRule::default()
        };
        assert!(customisation.map.add(rule).is_ok());

        let written = customisation_to_json(&customisation, &CustomisationWriteOptions::default())
            .expect("written");
        assert!(read_customisation(&written) == customisation, "{written}");
    }
}
