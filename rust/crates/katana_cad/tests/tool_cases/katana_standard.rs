// Katana Standard: the customisation tracked as
// resources/customisation/katana-standard.customisation.json
// (docs/katana_standard.md; tests/cad/customisation/test_katana_standard.cpp).
// Its survey code KEYS are the NSW customisation's, byte for byte, on the
// owner's decision, and each key stands for the same thing and has the same
// kinds of rule; everything the library names, draws and words is its own.
//
// The tests read the COMMITTED file with the strict reader and hold what the
// library promises, so a regenerated file that drifted from its design fails
// here and not in somebody's drawing. Every figure is from a source that is
// not this program:
//
//   * the counts of keys, rules, definitions and strokes, and the figures
//     below, are counted by tools/katana_standard/census.py and
//     tools/customisation_census.py, which read the file with the standard
//     `json` module and share no code with Katana, and by
//     tools/katana_standard/parity.py, which reads both files the same way:
//     632 keys; 1103 rules, of which 917 are the reference's own structure
//     (its 916 effective rules and the one symbol rule that AD* needs) and
//     186 are the service attribute rules this library adds; 221 definitions
//     (98 linestyles, 123 symbols); 28 colours;
//   * contrast is the WCAG 2.x formula, a published standard;
//   * a hand-worked check says why a figure is what it is where it can: 572
//     keys have a feature rule and 60 have none (52 that only set a surface
//     flag, the 7 text codes and the bare `*`); 360 of the 572 draw a line,
//     205 a point and 7 only name a layer; 203 symbol rules are the 202
//     points with a symbol and the one line (BI*) with one; the 119 surface
//     rules are the reference's 52 surface-only keys and 67
//     feature-and-surface keys, and every one says false; 244 attributes are
//     applied (186 service words, 12 model zones, 28 stated diameters and 18
//     stated heights) and 641 are prompts;
//   * the palette's two floors are the WCAG contrast formula and the OKLab
//     distance of Bjorn Ottosson's published matrices, written out below;
//   * the service colours are AS 5488's: the standard names them and gives no
//     sRGB, so the values are Katana's, and the one table of them is
//     utility_type_colour (utilities/utility_drawing.rs), which UTILITY DRAW
//     draws in and which the file's colours are held equal to.
//
// Nothing here names a definition, a layer, a group or a legend label of the
// NSW customisation: where the two are compared it is by key and by kind of
// entity, from the two tracked files, at run time. What else the two share is
// counted by tools/katana_standard/originality.py (nothing: no name, group,
// layer, label, colour name, attribute name or text style, and of colour
// values only the white of communications; a name is also compared with the
// other's names less their lead-in of capitals and with its legend labels,
// and a label with its names) and held here by count.
//
// The C++ matches keys, layer words and lead-ins with std::regex; there is no
// regular expression crate here, so each pattern is matched by hand below,
// with the pattern beside it and why the hand match is the same match.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use katana_entity::customisation::customisation_from_json;
use katana_entity::{Color, Customisation, SurveyRule, SurveySection};
use katana_math::cpp;

const STANDARD_JSON: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../resources/customisation/katana-standard.customisation.json"
);
const NSW_JSON: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../resources/customisation/nsw.customisation.json"
);
const STANDARD_DOC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../docs/katana_standard.md"
);

fn slurp(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

// A tracked customisation file, read once. `problem` is set when it cannot be
// read, so that every test says why instead of failing on an empty value.
struct Loaded {
    text: Vec<u8>,
    value: Customisation,
    problem: String,
}

fn load(path: &str) -> Loaded {
    let text = slurp(path);
    if text.is_empty() {
        return Loaded {
            text,
            value: Customisation::default(),
            problem: format!("no file at {path}"),
        };
    }
    match customisation_from_json(&text) {
        Ok(value) => Loaded {
            text,
            value,
            problem: String::new(),
        },
        Err(error) => Loaded {
            text,
            value: Customisation::default(),
            problem: error.describe(),
        },
    }
}

// The C++ KS_LOAD(): Katana Standard, read once, and the test fails here when
// it could not be.
#[track_caller]
fn standard() -> &'static Loaded {
    static LOADED: OnceLock<Loaded> = OnceLock::new();
    let loaded = LOADED.get_or_init(|| load(STANDARD_JSON));
    assert!(loaded.problem.is_empty(), "{}", loaded.problem);
    loaded
}

// The NSW customisation, tracked beside it: the one thing it shares with this
// library is its keys, and these tests read it to hold that.
#[track_caller]
fn nsw() -> &'static Loaded {
    static LOADED: OnceLock<Loaded> = OnceLock::new();
    let loaded = LOADED.get_or_init(|| load(NSW_JSON));
    assert!(loaded.problem.is_empty(), "{}", loaded.problem);
    loaded
}

// The rules of one section, by key as written (`AC*`, `PBMK`, `*`); the
// first of a key, as std::map::emplace keeps it.
fn rules_of(c: &Customisation, section: SurveySection) -> BTreeMap<String, &SurveyRule> {
    let mut found = BTreeMap::new();
    for rule in c.map.rules() {
        if rule.section == section {
            found.entry(rule.key.clone()).or_insert(rule);
        }
    }
    found
}

fn has_attribute(rule: &SurveyRule, name: &str, value: &str) -> bool {
    rule.attributes
        .iter()
        .any(|attribute| attribute.name == name && attribute.value == value)
}

fn asks_for(rule: &SurveyRule, name: &str) -> bool {
    has_attribute(rule, name, "")
}

fn has_attribute_named(rule: &SurveyRule, name: &str) -> bool {
    rule.attributes
        .iter()
        .any(|attribute| attribute.name == name)
}

// The first path level of a rule's group: the class it belongs to.
fn class_of(rule: &SurveyRule) -> String {
    match rule.group.find('/') {
        Some(slash) => String::from(&rule.group[..slash]),
        None => rule.group.clone(),
    }
}

// WCAG 2.x relative luminance and contrast ratio.
fn linear(channel: u8) -> f64 {
    let c = f64::from(channel) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(c: Color) -> f64 {
    0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
}

fn contrast(a: Color, b: Color) -> f64 {
    let hi = cpp::max(luminance(a), luminance(b));
    let lo = cpp::min(luminance(a), luminance(b));
    (hi + 0.05) / (lo + 0.05)
}

// Distance in OKLab (Bjorn Ottosson, 2020: the published matrices), the space
// in which a step of 0.02 is about the least a person sees between two large
// patches. The palette's own check uses the same published figures.
fn oklab_distance(a: Color, b: Color) -> f64 {
    let lab = |c: Color| -> [f64; 3] {
        let r = linear(c.r);
        let g = linear(c.g);
        let bl = linear(c.b);
        let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * bl).cbrt();
        let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * bl).cbrt();
        let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * bl).cbrt();
        [
            0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
        ]
    };
    let p = lab(a);
    let q = lab(b);
    ((p[0] - q[0]) * (p[0] - q[0]) + (p[1] - q[1]) * (p[1] - q[1]) + (p[2] - q[2]) * (p[2] - q[2]))
        .sqrt()
}

// Millimetres at most this far from a whole number of thousandths.
fn at_most_three_decimals(value: f64) -> bool {
    (value * 1000.0 - (value * 1000.0).round()).abs() < 1e-6
}

// The words CLAUDE.md keeps out of new text, assembled so that this file does
// not hold them.
fn forbidden_words() -> Vec<String> {
    [
        ["1", "2d"],
        ["ex", "ds"],
        ["tf", "nsw"],
        ["transport for n", "sw"],
        ["n", "sw"],
    ]
    .iter()
    .map(|[front, back]| format!("{front}{back}"))
    .collect()
}

// The pieces std::getline(stream, piece, delimiter) reads: split at the
// delimiter, with no empty piece after a delimiter at the very end (getline
// meets the end there and reads nothing).
fn getline_pieces(text: &str, delimiter: char) -> Vec<&str> {
    let mut pieces: Vec<&str> = text.split(delimiter).collect();
    if pieces.last().is_some_and(|last| last.is_empty()) {
        pieces.pop();
    }
    pieces
}

// Where `needle` first is in `haystack` at or after `from` (std::string::find).
fn find_bytes(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > haystack.len() {
        return None;
    }
    if needle.is_empty() {
        return Some(from);
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| at + from)
}

// std::regex's \s in the classic locale: space, tab, newline, vertical tab,
// form feed and carriage return.
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

fn is_capital_or_digit(byte: u8) -> bool {
    byte.is_ascii_uppercase() || byte.is_ascii_digit()
}

// How many bytes at the start of `bytes` are `class`.
fn run_of(bytes: &[u8], class: fn(u8) -> bool) -> usize {
    bytes.iter().take_while(|&&byte| class(byte)).count()
}

// `\*|[A-Z0-9]+\*?`, matched whole: the bare star, or capitals and digits with
// one star after them or none.
fn is_key_shape(key: &str) -> bool {
    if key == "*" {
        return true;
    }
    let body = key.strip_suffix('*').unwrap_or(key);
    !body.is_empty() && body.bytes().all(is_capital_or_digit)
}

// `[a-z]+(-[a-z]+)*`, matched whole: lower-case words joined by single
// hyphens.
fn is_layer_word(level: &str) -> bool {
    level
        .split('-')
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_lowercase()))
}

// The C++ `Kind` and the helpers of the NSW comparison, kept with the case
// that uses them.
mod kinds {
    use katana_entity::{Customisation, SurveyBreakline, SurveyMatchKind, SurveySection};

    // What a code makes of the rules that answer it, as a kind of entity: the
    // things a field file's result depends on and a person does not see.
    #[derive(Debug, PartialEq)]
    pub struct Kind {
        pub matched: SurveyMatchKind,
        pub breakline: i32, // -1 says nothing of it, 0 a point, 1 a line
        pub tinable: i32,   // -1 says nothing of it, 0 stays out of a surface, 1 joins one
        pub hide: bool,
        pub symbol: bool,
        pub layer_named: bool,
        pub colour_named: bool,
        pub text: bool,
        pub text_units: String,
        pub text_size: f64,
        pub text_justify_x: String,
        pub text_justify_y: String,
        pub pipe: bool,
        pub vertex_pipe: bool,
        pub segment_pipe: bool,
        pub pipe_justify: String,
        pub pipe_shape: String,
        pub pipe_active: bool,
        pub pipe_size1: bool,
        pub pipe_size2: bool,
    }

    pub fn kind_of(library: &Customisation, code: &str) -> Kind {
        let found = library.map.lookup(code);
        let r = &found.resolved;
        let text = r.text_style.as_ref();
        let pipe = r.pipe.as_ref();
        Kind {
            matched: found.kind,
            breakline: match r.breakline {
                Some(SurveyBreakline::Line) => 1,
                Some(SurveyBreakline::Point) => 0,
                None => -1,
            },
            tinable: match r.tinable {
                Some(true) => 1,
                Some(false) => 0,
                None => -1,
            },
            hide: r.hide.unwrap_or(false),
            symbol: r
                .symbol
                .as_ref()
                .is_some_and(|symbol| !symbol.style.is_empty()),
            layer_named: !r.model.is_empty(),
            colour_named: !r.colour.is_empty(),
            text: text.is_some(),
            text_units: text.map(|style| style.r#type.clone()).unwrap_or_default(),
            text_size: text.map_or(0.0, |style| style.size),
            text_justify_x: text
                .map(|style| style.justify_x.clone())
                .unwrap_or_default(),
            text_justify_y: text
                .map(|style| style.justify_y.clone())
                .unwrap_or_default(),
            pipe: r.pipe.is_some(),
            vertex_pipe: r.vertex_pipe.is_some(),
            segment_pipe: r.segment_pipe.is_some(),
            pipe_justify: pipe.map(|p| p.justify.clone()).unwrap_or_default(),
            pipe_shape: pipe.map(|p| p.shape.clone()).unwrap_or_default(),
            pipe_active: pipe.is_some_and(|p| p.active),
            pipe_size1: pipe.is_some_and(|p| !p.size1.is_empty()),
            pipe_size2: pipe.is_some_and(|p| !p.size2.is_empty()),
        }
    }

    // The kinds of rule a key makes, in order of first appearance, without a
    // second rule of a kind (the other library repeats rules and only the
    // first of a kind sets a field). The attribute rules this library adds to
    // a service key the other gives none are left out when
    // `drop_extra_attributes`.
    pub fn sections_of(
        library: &Customisation,
        key: &str,
        drop_extra_attributes: bool,
        other_has_attributes: bool,
    ) -> Vec<SurveySection> {
        let mut sections = Vec::new();
        for rule in library.map.rules() {
            if rule.key != key {
                continue;
            }
            if !sections.contains(&rule.section) {
                sections.push(rule.section);
            }
        }
        if drop_extra_attributes && !other_has_attributes {
            sections.retain(|&section| section != SurveySection::StringAttribute);
        }
        sections
    }

    pub fn has_section(library: &Customisation, key: &str, section: SurveySection) -> bool {
        library
            .map
            .rules()
            .iter()
            .any(|rule| rule.key == key && rule.section == section)
    }
}

// What the two NSW comparisons share: nothing but the keys in common.
mod shared {
    use std::collections::BTreeSet;

    use katana_cad::utilities::utility_drawing::utility_type_colour;
    use katana_entity::colour_names::resolve_colour;
    use katana_entity::style_library::style_groups;
    use katana_entity::{Color, Customisation, SurveySection};
    use katana_survey::subsurface::UtilityType;

    use super::{is_capital_or_digit, is_space, rules_of, run_of};

    // The names of every attribute a library's rules apply or ask for,
    // whichever list of a rule holds them.
    fn attribute_names_of(library: &Customisation) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        for rule in library.map.rules() {
            for list in [
                &rule.attributes,
                &rule.vertex_attributes,
                &rule.segment_attributes,
            ] {
                for attribute in list {
                    names.insert(attribute.name.clone());
                }
            }
        }
        names
    }

    // The text styles a library's text rules name.
    fn text_styles_of(library: &Customisation) -> BTreeSet<String> {
        let mut styles = BTreeSet::new();
        for rule in library.map.rules() {
            if let Some(text) = &rule.text_style
                && !text.textstyle.is_empty()
            {
                styles.insert(text.textstyle.clone());
            }
        }
        styles
    }

    // The letters and digits of a name, in lower case, and nothing else: two
    // names are the same name when these are, so a blank, a hyphen or a comma
    // more or less does not make a name another name (originality.py,
    // "squeeze"). ASCII by hand, so that the answer does not depend on the
    // locale.
    pub fn squeezed(text: &str) -> String {
        let mut out = String::new();
        for byte in text.bytes() {
            if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
                out.push(char::from(byte));
            } else if byte.is_ascii_uppercase() {
                out.push(char::from(byte.to_ascii_lowercase()));
            }
        }
        out
    }

    // The length of the lead-in `^(?:[A-Z]\s+)?[A-Z0-9]{2,}\s+(?=\S)` at the
    // start of `text`, if one is there. The engine tries the optional single
    // capital first, then goes without it; no run here can match shorter
    // than all of itself, since what follows a shorter run of capitals is a
    // capital, not the blank the pattern needs next, and a shorter run of
    // blanks leaves a blank where the look-ahead wants a non-blank.
    fn name_lead_in(text: &[u8]) -> Option<usize> {
        let with_capital = if text.first().is_some_and(u8::is_ascii_uppercase) {
            let blanks = run_of(&text[1..], is_space);
            (blanks > 0).then_some(1 + blanks)
        } else {
            None
        };
        for start in with_capital.into_iter().chain([0]) {
            let word = run_of(&text[start..], is_capital_or_digit);
            if word < 2 {
                continue;
            }
            let blanks = run_of(&text[start + word..], is_space);
            if blanks == 0 {
                continue;
            }
            let end = start + word + blanks;
            if end < text.len() {
                return Some(end);
            }
        }
        None
    }

    // Every shorter form of a definition name that taking a lead-in off its
    // front gives: a word of two or more capitals and digits, with or without
    // a single capital before it, when more follows. "E WALL Stone Line" gives
    // "Stone Line"; "AB CD Peg" gives "CD Peg" and "Peg"; "Stone Line" gives
    // nothing (originality.py, "without_lead_ins").
    pub fn without_lead_ins(name: &str) -> Vec<String> {
        let mut shorter = Vec::new();
        let mut rest = String::from(name);
        while let Some(end) = name_lead_in(rest.as_bytes()) {
            rest = String::from(&rest[end..]);
            shorter.push(rest.clone());
        }
        shorter
    }

    // A legend label less the lead-ins the other library may give one: a
    // "[KEY]" in brackets (`^\[[^\]]*\]\s*`), then a single capital and a word
    // of two to eight capitals and digits (`^[A-Z]\s+[A-Z0-9]{2,8}\s+`)
    // (originality.py, "clean_label"). A run of capitals longer than eight is
    // no match: the pattern would need a blank where a capital follows.
    pub fn without_label_lead_ins(comment: &str) -> String {
        // All of it when it is all blanks.
        let start = comment
            .bytes()
            .position(|byte| !matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
            .unwrap_or(comment.len());
        let mut text = &comment[start..];
        if text.starts_with('[')
            && let Some(close) = text.find(']')
        {
            let after = close + 1;
            text = &text[after + run_of(&text.as_bytes()[after..], is_space)..];
        }
        let bytes = text.as_bytes();
        if bytes.first().is_some_and(u8::is_ascii_uppercase) {
            let blanks = run_of(&bytes[1..], is_space);
            if blanks > 0 {
                let word = run_of(&bytes[1 + blanks..], is_capital_or_digit);
                if (2..=8).contains(&word) {
                    let tail = run_of(&bytes[1 + blanks + word..], is_space);
                    if tail > 0 {
                        return String::from(&text[1 + blanks + word + tail..]);
                    }
                }
            }
        }
        String::from(text)
    }

    // Whether the survey code keys of the library compared with are promised
    // to be this library's: they are the NSW customisation's, on the owner's
    // decision, and nothing is said of any other library a build compiles in.
    pub enum Keys {
        TheSame,
        NotCompared,
    }

    // Counts only, found at run time: nothing here names what the other holds.
    // Katana Standard was made so that nothing but the survey code keys is the
    // other's: no definition name, group path, layer path, colour name,
    // attribute name or text style of it is one of the other's, whole, and
    // (with the NSW customisation) its keys are the other's exactly.
    #[track_caller]
    pub fn expect_nothing_but_the_keys_in_common_with(
        file: &Customisation,
        other: &Customisation,
        keys: Keys,
    ) {
        let mut shared_names = 0;
        file.library.for_each(|style| {
            if other.library.contains(&style.name) {
                shared_names += 1;
            }
        });
        assert_eq!(shared_names, 0, "definition names in common");

        // Nor is a definition called what the other calls a thing in any of
        // the three ways it does: by a definition name (equal as letters and
        // digits, whatever the blanks and punctuation), by such a name less its
        // lead-in of capitals, or by a legend label. A symbol named after the
        // other's label for the thing would be the other's name for it as much
        // as one named after its symbol (tools/katana_standard/originality.py
        // counts all three at none, and a label of this library that is a
        // definition name of the other's at none too).
        let mut other_names = BTreeSet::new();
        let mut other_names_less_lead_in = BTreeSet::new();
        other.library.for_each(|style| {
            other_names.insert(squeezed(&style.name));
            for shorter in without_lead_ins(&style.name) {
                other_names_less_lead_in.insert(squeezed(&shorter));
            }
        });
        let mut other_labels = BTreeSet::new();
        for rule in other.map.rules() {
            other_labels.insert(squeezed(&without_label_lead_ins(&rule.comment)));
        }
        // A name of punctuation alone is no name.
        for names in [
            &mut other_names,
            &mut other_names_less_lead_in,
            &mut other_labels,
        ] {
            names.remove("");
        }
        let mut names_equal = 0;
        let mut names_less_lead_in = 0;
        let mut names_that_are_labels = 0;
        let mut definitions = 0;
        file.library.for_each(|style| {
            let name = squeezed(&style.name);
            definitions += 1;
            names_equal += usize::from(other_names.contains(&name));
            names_less_lead_in += usize::from(other_names_less_lead_in.contains(&name));
            names_that_are_labels += usize::from(other_labels.contains(&name));
        });
        assert!(definitions > 0);
        assert_eq!(
            names_equal, 0,
            "definition names in common, as letters and digits"
        );
        assert_eq!(
            names_less_lead_in, 0,
            "definition names that are the other's less a lead-in"
        );
        assert_eq!(
            names_that_are_labels, 0,
            "definition names that are legend labels of the other"
        );

        let other_groups: BTreeSet<String> = style_groups(&other.library).into_iter().collect();
        let shared_groups = style_groups(&file.library)
            .iter()
            .filter(|group| other_groups.contains(*group))
            .count();
        assert_eq!(shared_groups, 0, "group paths in common");

        // The keys are the one thing shared, and shared whole.
        if matches!(keys, Keys::TheSame) {
            let other_keys: BTreeSet<String> = other.map.keys().into_iter().collect();
            let file_keys = file.map.keys();
            let shared_keys = file_keys
                .iter()
                .filter(|key| other_keys.contains(*key))
                .count();
            assert_eq!(shared_keys, other_keys.len(), "survey code keys in common");
            assert_eq!(shared_keys, file_keys.len(), "survey code keys in common");
        }

        // No attribute is called what the other calls one, and no text style
        // is the other's (tools/katana_standard/originality.py counts both at
        // none).
        let other_attributes = attribute_names_of(other);
        let shared_attributes = attribute_names_of(file)
            .iter()
            .filter(|name| other_attributes.contains(*name))
            .count();
        assert_eq!(shared_attributes, 0, "attribute names in common");
        let other_text_styles = text_styles_of(other);
        let shared_text_styles = text_styles_of(file)
            .iter()
            .filter(|style| other_text_styles.contains(*style))
            .count();
        assert_eq!(shared_text_styles, 0, "text styles in common");

        let shared_colours = file
            .colours
            .entries()
            .iter()
            .filter(|entry| other.colours.find(&entry.name).is_some())
            .count();
        assert_eq!(shared_colours, 0, "colour names in common");

        // Nor is any colour VALUE one of the built-in's, whether from its
        // table or named by a rule (a name it keeps in the table, or a
        // standard name).
        let mut other_values = BTreeSet::new();
        for entry in other.colours.entries() {
            other_values.insert(entry.colour.to_hex());
        }
        for rule in other.map.rules() {
            let names = [
                rule.colour.clone(),
                rule.symbol
                    .as_ref()
                    .map(|symbol| symbol.colour.clone())
                    .unwrap_or_default(),
                rule.text_style
                    .as_ref()
                    .map(|text| text.colour.clone())
                    .unwrap_or_default(),
            ];
            for name in &names {
                if let Some(colour) = resolve_colour(&other.colours, name) {
                    other_values.insert(colour.to_hex());
                }
            }
        }
        // One value is not held to this: communications' own, which AS 5488
        // names white and the one table (utility_type_colour) therefore makes
        // pure white. White is the colour every drawing system starts from, so
        // a built-in that draws in it shares the value without sharing a
        // design. The exemption is that colour and no other white-ish one, it
        // is one library colour, and the other 27 values are what shows the
        // two are independent: they stay at none in common. (This is the one
        // expectation of the colour work that was loosened after a run showed
        // the collision, and it is the owner's to accept;
        // docs/katana_standard.md says what the comparison excuses. A
        // near-white would also print black, but it would not be AS 5488's
        // white.)
        let communications = utility_type_colour(UtilityType::Telecommunications);
        assert_eq!(
            communications.to_hex(),
            Color::new(255, 255, 255, 255).to_hex(),
            "AS 5488 communications is white"
        );
        let mut shared_values = 0;
        let mut exempt = 0;
        for entry in file.colours.entries() {
            if entry.colour.to_hex() == communications.to_hex() {
                exempt += 1;
                continue;
            }
            shared_values += usize::from(other_values.contains(&entry.colour.to_hex()));
        }
        assert_eq!(exempt, 1, "the one white is communications");
        assert_eq!(shared_values, 0, "colour values in common");

        let mut other_layers = BTreeSet::new();
        for rule in other.map.rules() {
            if !rule.model.is_empty() {
                other_layers.insert(rule.model.clone());
            }
        }
        let shared_layers = rules_of(file, SurveySection::Map)
            .values()
            .filter(|rule| other_layers.contains(&rule.model))
            .count();
        assert_eq!(shared_layers, 0, "layer paths in common");

        // And no legend label is one of the other's: the labels are the
        // comments of the feature rules, the other's with its own lead-ins
        // taken off (a "[KEY]", then a single capital and a word of capitals),
        // compared as letters and digits. Nor is a label a definition name of
        // the other's, whole or less its lead-in.
        let mut shared_labels = 0;
        let mut labels_that_are_names = 0;
        for rule in rules_of(file, SurveySection::Map).values() {
            let label = squeezed(&rule.comment);
            shared_labels += usize::from(other_labels.contains(&label));
            labels_that_are_names += usize::from(other_names.contains(&label))
                + usize::from(other_names_less_lead_in.contains(&label));
        }
        assert_eq!(shared_labels, 0, "legend labels in common");
        assert_eq!(
            labels_that_are_names, 0,
            "legend labels that are definition names of the other"
        );
    }
}

mod katana_standard {
    use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

    use katana_cad::customisation_host::built_ins;
    use katana_cad::plot::{PlotSettings, paper_colour};
    use katana_cad::utilities::utility_drawing::{utility_type_colour, utility_type_word};
    use katana_entity::colour_names::{resolve_colour, standard_colour};
    use katana_entity::customisation::{customisation_from_json, customisation_to_json};
    use katana_entity::style_library::style_groups;
    use katana_entity::{
        Color, CustomisationWriteOptions, StrokeOp, StyleUnits, SurveyBreakline, SurveyMatchKind,
        SurveySection,
    };
    use katana_math::cpp;
    use katana_survey::subsurface::UtilityType;

    use super::kinds::{has_section, kind_of, sections_of};
    use super::shared::{Keys, expect_nothing_but_the_keys_in_common_with};
    use super::{
        STANDARD_DOC, asks_for, at_most_three_decimals, class_of, contrast, find_bytes,
        forbidden_words, getline_pieces, has_attribute, has_attribute_named, is_key_shape,
        is_layer_word, nsw, oklab_distance, rules_of, slurp, standard,
    };

    #[test]
    fn the_committed_file_is_read_by_the_strict_reader_and_is_named_katana_standard() {
        let file = &standard().value;
        assert_eq!(file.name, "Katana Standard");
        assert_eq!(file.notice.len(), 3);
        assert!(!file.description.is_empty());
    }

    #[test]
    fn it_holds_the_counts_of_its_independent_census() {
        let file = &standard().value;
        let mut symbols = 0;
        let mut linestyles = 0;
        let mut at_vertices = 0;
        let mut strokes: HashMap<StrokeOp, usize> = HashMap::new();
        file.library.for_each(|style| {
            if style.symbol {
                symbols += 1;
            } else {
                linestyles += 1;
            }
            if style.at_vertices {
                at_vertices += 1;
            }
            for stroke in &style.strokes {
                *strokes.entry(stroke.op).or_insert(0) += 1;
            }
        });
        let count = |op: StrokeOp| strokes.get(&op).copied().unwrap_or(0);
        assert_eq!(file.library.size(), 221);
        assert_eq!(linestyles, 98);
        assert_eq!(symbols, 123);
        assert_eq!(
            at_vertices, 123,
            "every symbol is at vertices and no linestyle is"
        );
        // census.py: arc 32, circle 118, dot 40, draw 1709, move 701, text 54.
        assert_eq!(count(StrokeOp::Arc), 32);
        assert_eq!(count(StrokeOp::Circle), 118);
        assert_eq!(count(StrokeOp::Dot), 40);
        assert_eq!(count(StrokeOp::Draw), 1709);
        assert_eq!(count(StrokeOp::Move), 701);
        assert_eq!(count(StrokeOp::Text), 54);
        assert_eq!(
            count(StrokeOp::Pen),
            0,
            "no stroke names a pen: the rule gives the colour"
        );
        assert_eq!(style_groups(&file.library).len(), 51);
        assert_eq!(file.map.size(), 1103);
        assert_eq!(file.map.keys().len(), 632);
        assert_eq!(file.colours.size(), 28);
    }

    #[test]
    fn every_class_holds_the_number_of_keys_its_groups_say_and_sixty_keys_have_no_feature_rule() {
        let file = &standard().value;
        // A key's class is the first level of the group on its feature rule;
        // the keys that have none (the text codes, the surface-only keys and
        // the bare `*`) have no group in the file.
        let mut per_class: BTreeMap<String, usize> = BTreeMap::new();
        for rule in rules_of(file, SurveySection::Map).values() {
            *per_class.entry(class_of(rule)).or_insert(0) += 1;
        }
        // census.py "classes".
        let census: BTreeMap<String, usize> = [
            ("Boundaries and Cadastre", 23),
            ("Roads and Pavements", 35),
            ("Street Furniture and Signs", 36),
            ("Kerbs and Drainage", 79),
            ("Rail and Transit", 12),
            ("Utilities", 118),
            ("Vegetation and Landscape", 33),
            ("Buildings and Structures", 33),
            ("Fences and Walls", 8),
            ("Terrain and Breaklines", 25),
            ("Survey Control and Annotation", 39),
            ("Miscellaneous", 16),
            ("Design and Setting Out", 115),
        ]
        .into_iter()
        .map(|(name, count)| (String::from(name), count))
        .collect();
        assert_eq!(per_class, census);
        let with_feature: usize = census.values().sum();
        assert_eq!(with_feature, 572);
        // 52 keys that only set a surface flag, the 7 text codes, and the bare
        // `*` (52 + 7 + 1 = 60, held at the top of this file).
        assert_eq!(file.map.keys().len() - with_feature, 60);
    }

    #[test]
    fn every_key_is_capitals_and_digits_with_optional_star_and_no_rule_is_written_twice_in_a_section()
     {
        let file = &standard().value;
        let mut seen: HashSet<(String, SurveySection)> = HashSet::new();
        for rule in file.map.rules() {
            assert!(is_key_shape(&rule.key), "key {}", rule.key);
            assert!(
                seen.insert((rule.key.clone(), rule.section)),
                "{} is written twice in one section",
                rule.key
            );
        }
        // The three forms of key: 422 prefix keys, 209 exact keys, the bare `*`.
        let mut prefix = 0;
        let mut exact = 0;
        let mut bare = 0;
        for key in file.map.keys() {
            if key == "*" {
                bare += 1;
            } else if key.ends_with('*') {
                prefix += 1;
            } else {
                exact += 1;
            }
        }
        assert_eq!(prefix, 422);
        assert_eq!(exact, 209);
        assert_eq!(bare, 1);
        // A key is the code of a feature or of a flag, never the empty one.
        assert_eq!(rules_of(file, SurveySection::Map).len(), 572);
        assert_eq!(rules_of(file, SurveySection::Tinable).len(), 119);
    }

    #[test]
    fn a_rule_of_each_kind_is_written_for_the_codes_that_need_it() {
        let file = &standard().value;
        let mut by_section: HashMap<SurveySection, usize> = HashMap::new();
        let mut lines = 0;
        let mut points = 0;
        let mut layer_only = 0;
        for rule in file.map.rules() {
            *by_section.entry(rule.section).or_insert(0) += 1;
            if rule.section == SurveySection::Map {
                match rule.breakline {
                    None => layer_only += 1,
                    Some(SurveyBreakline::Line) => lines += 1,
                    Some(SurveyBreakline::Point) => points += 1,
                }
            }
        }
        let count = |section: SurveySection| by_section.get(&section).copied().unwrap_or(0);
        assert_eq!(count(SurveySection::Map), 572);
        assert_eq!(count(SurveySection::Tinable), 119);
        assert_eq!(count(SurveySection::Pipe), 1);
        assert_eq!(count(SurveySection::VertexPipe), 1);
        assert_eq!(count(SurveySection::SegmentPipe), 1);
        assert_eq!(count(SurveySection::VertexTextStyle), 7);
        // 12 attribute rules the reference has, and 186 service rules this
        // library adds.
        assert_eq!(count(SurveySection::StringAttribute), 198);
        assert_eq!(count(SurveySection::VertexAttribute), 1);
        // 572 keys with a feature rule: 360 lines, 205 points, 7 that only
        // name the layer of a text code (1* to 8*).
        assert_eq!(lines, 360);
        assert_eq!(points, 205);
        assert_eq!(layer_only, 7);
        assert_eq!(lines + points + layer_only, 572);
        // A symbol rule for every point but three that name none (POCP, PRTR
        // and the design key PLIG*), and for the one line that carries a
        // symbol at every vertex (BI*): 205 - 3 + 1.
        assert_eq!(count(SurveySection::VertexSymbol), 203);
    }

    #[test]
    fn every_name_a_rule_uses_is_defined_except_the_plain_line_and_every_definition_is_used() {
        let file = &standard().value;
        let referenced: BTreeSet<String> = file.map.styles_referenced().into_iter().collect();
        let undefined: BTreeSet<String> = referenced
            .iter()
            .filter(|name| !file.library.contains(name))
            .cloned()
            .collect();
        assert_eq!(
            undefined,
            BTreeSet::from([String::from("continuous")]),
            "the one word that names no definition is Katana's plain line"
        );
        let mut unused = Vec::new();
        file.library.for_each(|style| {
            if !referenced.contains(&style.name) {
                unused.push(style.name.clone());
            }
        });
        assert!(
            unused.is_empty(),
            "defined and no code names them: {unused:?}"
        );
        // A rule's linestyle is a linestyle and its symbol is a symbol, so
        // CODE CHECK has nothing to warn of.
        for rule in file.map.rules() {
            if !rule.linestyle.is_empty() && rule.linestyle != "continuous" {
                let Some(style) = file.library.find(&rule.linestyle) else {
                    panic!("{}", rule.key);
                };
                assert!(
                    !style.symbol,
                    "{} names a symbol as its linestyle",
                    rule.key
                );
            }
            if let Some(symbol) = &rule.symbol
                && !symbol.style.is_empty()
            {
                let Some(style) = file.library.find(&symbol.style) else {
                    panic!("{}", rule.key);
                };
                assert!(style.symbol, "{} names a linestyle as its symbol", rule.key);
            }
        }
    }

    #[test]
    fn every_colour_a_rule_names_is_in_the_table_and_the_table_holds_no_other() {
        let file = &standard().value;
        let mut used = BTreeSet::new();
        for rule in file.map.rules() {
            if !rule.colour.is_empty() {
                used.insert(rule.colour.clone());
            }
            if let Some(symbol) = &rule.symbol
                && !symbol.colour.is_empty()
            {
                used.insert(symbol.colour.clone());
            }
            if let Some(text) = &rule.text_style
                && !text.colour.is_empty()
            {
                used.insert(text.colour.clone());
            }
        }
        let table: BTreeSet<String> = file
            .colours
            .entries()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(used, table);
        assert_eq!(table.len(), 28);
    }

    #[test]
    fn every_colour_is_named_by_its_role_and_no_standard_name_folds_onto_it() {
        let file = &standard().value;
        for entry in file.colours.entries() {
            assert!(entry.name.starts_with("katana "), "{}", entry.name);
            assert_eq!(
                entry.name,
                entry.name.to_ascii_lowercase(),
                "colour names are lower case"
            );
            assert!(
                standard_colour(&entry.name).is_none(),
                "{} is a standard colour name",
                entry.name
            );
        }
    }

    #[test]
    fn every_colour_is_readable_on_the_plan_ground_and_on_paper_except_yellow_gas_and_cream_sewer_which_print_light()
     {
        let file = &standard().value;
        let ground = Color::new(0x1E, 0x23, 0x29, 255);
        let paper = Color::new(255, 255, 255, 255);
        let plot = PlotSettings::default();
        // AS 5488's yellow and cream cannot be 3:1 on white and still be
        // yellow and cream, so the two keep the ground's 3:1 and print light.
        // They are the only colours excused, and a colour that needs no excuse
        // is not given one.
        let prints_light =
            BTreeSet::from([String::from("katana gas"), String::from("katana sewer")]);
        let mut under = BTreeSet::new();
        for entry in file.colours.entries() {
            // WCAG 2.x asks 3:1 of a graphical object against its ground.
            assert!(
                contrast(entry.colour, ground) >= 3.0,
                "{} on the ground",
                entry.name
            );
            // On paper it is the pen as the plot prints it: white prints black.
            let on_paper = contrast(paper_colour(entry.colour, &plot), paper);
            if on_paper < 3.0 {
                under.insert(entry.name.clone());
            }
            if !prints_light.contains(&entry.name) {
                assert!(on_paper >= 3.0, "{} on paper", entry.name);
            }
        }
        assert_eq!(under, prints_light);
    }

    #[test]
    fn no_two_colours_are_closer_than_five_hundredths_in_ok_lab_and_the_warm_classes_are_six_hundredths_apart()
     {
        let file = &standard().value;
        let entries = file.colours.entries();
        // The six warm classes, which sit in the narrow band of lightness that
        // both grounds leave and were placed by a search: each is at least
        // 0.06 from every other colour, as colours.py holds it.
        let warm = [
            "katana kerb",
            "katana wall",
            "katana contour",
            "katana fuel",
            "katana fence",
            "katana breakline",
        ];
        let mut warm_pairs = 0;
        for (i, a) in entries.iter().enumerate() {
            for b in &entries[i + 1..] {
                let d = oklab_distance(a.colour, b.colour);
                assert!(d >= 0.05, "{} and {}", a.name, b.name);
                if warm.contains(&a.name.as_str()) || warm.contains(&b.name.as_str()) {
                    warm_pairs += 1;
                    assert!(d >= 0.06, "{} and {}", a.name, b.name);
                }
            }
        }
        // Reaches the cases it claims to: six colours against the other 22,
        // and among themselves 15 pairs: 6 * 22 + 15.
        assert_eq!(warm_pairs, 147);
    }

    #[test]
    fn the_classes_drawn_with_the_thinnest_pens_keep_a_floor_of_three_and_a_quarter_on_both_grounds()
     {
        let file = &standard().value;
        let ground = Color::new(0x1E, 0x23, 0x29, 255);
        let paper = Color::new(255, 255, 255, 255);
        let plot = PlotSettings::default();
        // Tree, planting, ground, building, wall and communications: a 0.13 to
        // 0.18 mm line of one of these is the faintest thing on a plan.
        // Communications is AS 5488's white, so on paper it is the pen as the
        // plot prints it, black.
        for name in [
            "katana tree",
            "katana planting",
            "katana ground",
            "katana building",
            "katana wall",
            "katana telecom",
        ] {
            let Some(colour) = file.colours.find(name) else {
                panic!("{name}");
            };
            assert!(contrast(colour, ground) >= 3.25, "{name} on the ground");
            assert!(
                contrast(paper_colour(colour, &plot), paper) >= 3.25,
                "{name} on paper"
            );
        }
    }

    #[test]
    fn the_kerb_top_is_a_plain_line_and_its_back_and_its_gutter_lip_have_patterns_of_their_own() {
        let file = &standard().value;
        let map = rules_of(file, SurveySection::Map);
        // The top edge of a kerb and the top edge of a median are hard edges
        // drawn plain; the back of the kerb and the lip of the gutter are other
        // things and each has a pattern of its own, since no pen is applied
        // yet.
        assert_eq!(map["KB*"].linestyle, "continuous");
        assert_eq!(map["TM*"].linestyle, "continuous");
        assert_eq!(map["LP*"].linestyle, "Gutter Lip Line");
        assert_eq!(map["BK*"].linestyle, "Kerb Back Line");
        for name in ["Gutter Lip Line", "Kerb Back Line"] {
            let Some(style) = file.library.find(name) else {
                panic!("{name}");
            };
            assert!(!style.symbol, "{name}");
        }
    }

    #[test]
    fn every_layer_is_three_levels_of_lower_case_words_and_there_are_one_hundred_and_sixty() {
        let file = &standard().value;
        let mut layers = BTreeSet::new();
        for (key, rule) in rules_of(file, SurveySection::Map) {
            layers.insert(rule.model.clone());
            let levels = getline_pieces(&rule.model, '/');
            assert_eq!(levels.len(), 3, "{key} layer {}", rule.model);
            for level in levels {
                assert!(is_layer_word(level), "{key} level '{level}'");
            }
        }
        assert_eq!(layers.len(), 160);
    }

    #[test]
    fn every_group_is_two_levels_deep_and_there_are_fifty_eight_in_all() {
        let file = &standard().value;
        let mut of_definitions = BTreeSet::new();
        file.library.for_each(|style| {
            of_definitions.insert(style.group.clone());
        });
        let of_rules: BTreeSet<String> = rules_of(file, SurveySection::Map)
            .values()
            .map(|rule| rule.group.clone())
            .collect();
        let every_group: BTreeSet<String> = of_definitions.union(&of_rules).cloned().collect();
        for group in &every_group {
            assert_eq!(group.matches('/').count(), 1, "{group}");
        }
        // 57 groups of the codes: 13 classes and their groups, every one of
        // them holding a code. The definitions' 51 are the groups that have a
        // definition; of the codes' 57, 7 have none (four design groups drawn
        // with plain lines or with another group's definitions - the road
        // design strings, earthworks, kerb and carriageway, and bridge design
        // - the paths, the land use areas and the text codes), and the shapes
        // every service shares have a group no code has (Utilities/All
        // Services): 57 - 7 + 1 = 51 groups with a definition (held at the top
        // of this file) and 57 + 1 = 58 in all.
        assert_eq!(of_rules.len(), 57);
        assert_eq!(of_definitions.len(), 51);
        assert_eq!(every_group.len(), 58);
        assert!(!of_rules.contains("Utilities/All Services"));
        assert!(of_definitions.contains("Utilities/All Services"));
        assert!(!of_definitions.contains("Survey Control and Annotation/Text and Notes"));
        assert!(of_rules.contains("Survey Control and Annotation/Text and Notes"));
    }

    #[test]
    fn the_weights_are_the_five_of_the_scale_and_the_bold_one_is_unused() {
        let file = &standard().value;
        let mut weights: BTreeMap<String, usize> = BTreeMap::new();
        for rule in rules_of(file, SurveySection::Map).values() {
            *weights.entry(rule.weight.clone()).or_insert(0) += 1;
        }
        // census.py: 0.13 21, 0.18 180, 0.25 289, 0.35 79, 0.50 3 - and 0.70,
        // the sixth of the scale, reserved. They add to the 572 feature rules
        // (held at the top of this file).
        let expected: BTreeMap<String, usize> = [
            ("0.13", 21),
            ("0.18", 180),
            ("0.25", 289),
            ("0.35", 79),
            ("0.50", 3),
        ]
        .into_iter()
        .map(|(weight, count)| (String::from(weight), count))
        .collect();
        assert_eq!(weights, expected);
        assert!(!weights.contains_key("0.70"));
        // Nothing in the planting or the ground colours is lighter than the
        // fine pen.
        for (key, rule) in rules_of(file, SurveySection::Map) {
            if rule.colour == "katana planting" || rule.colour == "katana ground" {
                assert_ne!(rule.weight, "0.13", "{key}");
            }
        }
    }

    #[test]
    fn only_four_symbols_are_hidden_and_they_are_those_the_other_library_hides() {
        let file = &standard().value;
        let hidden: BTreeSet<String> = rules_of(file, SurveySection::VertexSymbol)
            .into_iter()
            .filter(|(_, rule)| rule.hide.unwrap_or(false))
            .map(|(key, _)| key)
            .collect();
        // census.py "hideKeys": the four keys whose second symbol rule in the
        // other library says hide where the first leaves it unsaid, so that a
        // lookup there hides the symbol (parity.py, "nsw-fields-merged").
        let expected: BTreeSet<String> = ["PBDY", "PBMZ", "PCRM", "PPSM"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(hidden, expected);
    }

    #[test]
    fn every_surface_rule_keeps_its_code_out_of_a_surface_and_there_are_one_hundred_and_nineteen() {
        let file = &standard().value;
        let surfaces = rules_of(file, SurveySection::Tinable);
        // The other library has 119 keys that set a surface flag (52 that set
        // nothing else, and 67 beside a feature rule, held at the top of this
        // file) and every one says false.
        assert_eq!(surfaces.len(), 119);
        for (key, rule) in &surfaces {
            let Some(tinable) = rule.tinable else {
                panic!("{key} says nothing of surfaces");
            };
            assert!(!tinable, "{key}");
        }
        // By hand, a few of each family: a speed zone, a culvert, a pipe, a
        // trunk's own flag key, a control mark's family key, a hazard's flag
        // key.
        for key in ["S9*", "D1*", "U3*", "PT04*", "PSS*", "PHAZ*", "CA*", "SO*"] {
            assert!(surfaces.contains_key(key), "{key}");
        }
        // And a few that are not: a kerb top, a title boundary, a water main.
        for key in ["KB*", "BT*", "WM*"] {
            assert!(!surfaces.contains_key(key), "{key}");
        }
    }

    #[test]
    fn only_the_bare_star_is_a_pipe_and_sizes_it_from_the_diameter_attribute() {
        let file = &standard().value;
        let kinds = [
            (SurveySection::Pipe, "pipe"),
            (SurveySection::VertexPipe, "vertexPipe"),
            (SurveySection::SegmentPipe, "segmentPipe"),
        ];
        for (section, word) in kinds {
            let pipes = rules_of(file, section);
            assert_eq!(pipes.len(), 1, "{word}");
            let Some(rule) = pipes.get("*") else {
                panic!("{word}");
            };
            let pipe = match section {
                SurveySection::Pipe => &rule.pipe,
                SurveySection::VertexPipe => &rule.vertex_pipe,
                _ => &rule.segment_pipe,
            };
            let Some(pipe) = pipe else {
                panic!("{word}");
            };
            assert!(pipe.active, "{word}");
            assert_eq!(
                pipe.justify, "Obvert",
                "{word}: a string with a diameter is measured from its top"
            );
            assert_eq!(pipe.shape, "diameter", "{word}");
            assert_eq!(
                pipe.size1, "$Diameter (mm)",
                "{word} sizes from an attribute"
            );
            assert!(pipe.size2.is_empty(), "{word}");
        }
    }

    #[test]
    fn the_seven_text_codes_are_standard_text_in_millimetres_of_paper() {
        let file = &standard().value;
        let texts = rules_of(file, SurveySection::VertexTextStyle);
        let mut sizes: BTreeMap<String, f64> = BTreeMap::new();
        for (key, rule) in &texts {
            let Some(text) = &rule.text_style else {
                panic!("{key} has no text style");
            };
            assert_eq!(text.textstyle, "Standard", "{key}");
            assert_eq!(text.r#type, "paper", "{key}");
            assert_eq!(text.colour, "katana note", "{key}");
            assert_eq!(text.justify_x, "left", "{key}");
            assert_eq!(text.justify_y, "bottom", "{key}");
            sizes.insert(key.clone(), text.size);
        }
        // The keys 1 to 5 and 7 are the sizes 1.5, 2.5, 3.5, 5, 7 and 10 mm; 8
        // is the parcel label's 2.5 (census.py "textSizes").
        let expected: BTreeMap<String, f64> = [
            ("1", 1.5),
            ("2", 2.5),
            ("3", 3.5),
            ("4", 5.0),
            ("5", 7.0),
            ("7", 10.0),
            ("8", 2.5),
        ]
        .into_iter()
        .map(|(key, size)| (String::from(key), size))
        .collect();
        assert_eq!(sizes, expected);
        // And each has a feature rule under its star that names the layer all
        // text goes on.
        let features = rules_of(file, SurveySection::Map);
        for key in ["1*", "2*", "3*", "4*", "5*", "7*", "8*"] {
            let Some(feature) = features.get(key) else {
                panic!("{key}");
            };
            assert_eq!(feature.model, "control/annotation/text", "{key}");
            assert!(
                feature.breakline.is_none(),
                "{key} draws nothing of its own"
            );
        }
    }

    #[test]
    fn every_utility_code_names_its_service_and_no_other_code_claims_one_but_the_stormwater_ones() {
        let file = &standard().value;
        let mut disused_by_status = BTreeSet::new();
        let mut utilities = 0;
        let mut typed_utilities = 0;
        let mut stormwater_typed = 0;
        let mut per_service: BTreeMap<String, usize> = BTreeMap::new();
        let attributes = rules_of(file, SurveySection::StringAttribute);
        for (key, feature) in rules_of(file, SurveySection::Map) {
            let found = attributes.get(&key);
            let says = found.is_some_and(|rule| has_attribute_named(rule, "utility.type"));
            let in_utilities = feature.model.starts_with("utilities/");
            // The stormwater pits, structures, pipes and culverts are services
            // too, though they are catalogued with drainage.
            let stormwater = feature.model.starts_with("drainage/pipes/")
                || feature.model.starts_with("drainage/structures/");
            if !in_utilities {
                assert_eq!(
                    says,
                    stormwater,
                    "{key}{}",
                    if stormwater {
                        " is stormwater and must say so"
                    } else {
                        " is not a service and must not claim one"
                    }
                );
                if stormwater
                    && says
                    && let Some(rule) = found
                {
                    assert!(has_attribute(rule, "utility.type", "stormwater"), "{key}");
                    stormwater_typed += 1;
                }
                continue;
            }
            utilities += 1;
            // utilities/<service word>/<family>: a coded service and UTILITY
            // DRAW share one tree.
            let service = getline_pieces(&feature.model, '/')
                .get(1)
                .copied()
                .unwrap_or_default();
            if key == "TS*" {
                // The reference gives it an attribute rule (its model zone),
                // whose entries are fixed, so it cannot also say its service.
                assert!(!says, "{key}");
                continue;
            }
            let Some(rule) = found else {
                panic!("{key}");
            };
            assert!(
                has_attribute(rule, "utility.type", service),
                "{key} {}",
                feature.model
            );
            typed_utilities += 1;
            *per_service.entry(String::from(service)).or_insert(0) += 1;
            if has_attribute_named(rule, "utility.status") {
                disused_by_status.insert(key);
            }
        }
        assert_eq!(utilities, 118);
        assert_eq!(typed_utilities, 117);
        // The pits and structures of drainage and its pipes and culverts, all
        // typed.
        assert_eq!(stormwater_typed, 69);
        // The other library has no disused service, and neither has this one.
        assert!(disused_by_status.is_empty());
        // census.py "utilityTypes".
        let expected: BTreeMap<String, usize> = [
            ("electricity", 23),
            ("fire-service", 1),
            ("fuel", 7),
            ("gas", 15),
            ("its", 4),
            ("recycled-water", 8),
            ("sewer", 13),
            ("telecommunications", 22),
            ("unknown", 8),
            ("water", 16),
        ]
        .into_iter()
        .map(|(service, count)| (String::from(service), count))
        .collect();
        assert_eq!(per_service, expected);
    }

    #[test]
    fn every_service_colour_is_the_colour_utility_draw_uses() {
        let file = &standard().value;
        // Every kind of service there is, by the word the codes say it is
        // (utility.type) and UTILITY DRAW names its layers by.
        let kinds = [
            UtilityType::Unknown,
            UtilityType::Electricity,
            UtilityType::Telecommunications,
            UtilityType::Gas,
            UtilityType::Water,
            UtilityType::RecycledWater,
            UtilityType::FireService,
            UtilityType::Sewer,
            UtilityType::Stormwater,
            UtilityType::Fuel,
            UtilityType::IntelligentTransport,
            UtilityType::Other,
        ];
        let mut by_word: BTreeMap<String, UtilityType> = BTreeMap::new();
        for kind in kinds {
            by_word
                .entry(String::from(utility_type_word(kind)))
                .or_insert(kind);
        }
        assert_eq!(by_word.len(), 12);

        let attributes = rules_of(file, SurveySection::StringAttribute);
        let mut reached = BTreeSet::new();
        let mut checked = 0;
        for (key, feature) in rules_of(file, SurveySection::Map) {
            let Some(found) = attributes.get(&key) else {
                continue;
            };
            for attribute in &found.attributes {
                if attribute.name != "utility.type" {
                    continue;
                }
                // The 117 utility codes and the 69 stormwater pits, structures
                // and pipes.
                let Some(&kind) = by_word.get(&attribute.value) else {
                    panic!("{key} says {}", attribute.value);
                };
                let expected = utility_type_colour(kind);
                assert!(!feature.colour.is_empty(), "{key} draws in no colour");
                assert_eq!(
                    resolve_colour(&file.colours, &feature.colour),
                    Some(expected),
                    "{key} is {} and draws in {}",
                    attribute.value,
                    feature.colour
                );
                checked += 1;
                reached.insert(kind);
            }
        }
        assert_eq!(checked, 186);
        // It reached every kind but "other", for which the library has no
        // code, so the loop is not passing for want of anything to compare.
        let mut expected_kinds: BTreeSet<UtilityType> = kinds.into_iter().collect();
        expected_kinds.remove(&UtilityType::Other);
        assert_eq!(reached, expected_kinds);
    }

    #[test]
    fn every_buried_utility_line_asks_for_its_owner_its_condition_and_its_depth_or_height() {
        let file = &standard().value;
        let attributes = rules_of(file, SurveySection::StringAttribute);
        let mut lines = 0;
        let mut depth = 0;
        let mut height = 0;
        let mut neither = BTreeSet::new();
        for (key, feature) in rules_of(file, SurveySection::Map) {
            if !feature.model.starts_with("utilities/")
                || feature.breakline != Some(SurveyBreakline::Line)
            {
                continue;
            }
            lines += 1;
            let Some(rule) = attributes.get(&key) else {
                panic!("{key} has no attribute rule");
            };
            if key == "TS*" {
                // The telephone sump's only attribute rule is the model zone.
                assert!(!asks_for(rule, "Owner"), "{key}");
                continue;
            }
            assert!(asks_for(rule, "Owner"), "{key}");
            assert!(asks_for(rule, "Condition"), "{key}");
            if asks_for(rule, "Depth (m)") {
                depth += 1;
            } else if asks_for(rule, "Height (m)") {
                height += 1;
            } else {
                neither.insert(key);
            }
        }
        // 52 lines on a utilities layer. 38 are buried and ask for a depth; 4
        // are strung above ground and ask for a height (UE*, UL*, UO*, OA*);
        // the outlines of structures and the stay wire ask for neither (a
        // reservoir, a pond, a pump-out station, a septic tank, a transformer
        // cabinet, a pylon footing, a telephone box, a junction box and a stay
        // wire); and TS*.
        assert_eq!(lines, 52);
        assert_eq!(depth, 38);
        assert_eq!(height, 4);
        let outlines: BTreeSet<String> = [
            "EC*", "EL*", "RE*", "SC*", "SP*", "SQ*", "ST*", "TX*", "US*",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        assert_eq!(neither, outlines);
        assert_eq!(depth + height + neither.len() + 1, lines);
    }

    #[test]
    fn an_attribute_with_no_value_is_a_prompt_and_only_a_few_kinds_are_applied() {
        let file = &standard().value;
        let mut prompts = 0;
        let mut applied = 0;
        let mut applied_names: BTreeMap<String, usize> = BTreeMap::new();
        for (key, rule) in rules_of(file, SurveySection::StringAttribute) {
            let mut names = BTreeSet::new();
            for attribute in &rule.attributes {
                assert!(
                    attribute.r#type == "text" || attribute.r#type == "integer",
                    "{key}"
                );
                assert!(
                    names.insert(attribute.name.clone()),
                    "{key} repeats {}",
                    attribute.name
                );
                if attribute.value.is_empty() {
                    prompts += 1;
                } else {
                    applied += 1;
                    *applied_names.entry(attribute.name.clone()).or_insert(0) += 1;
                    // Applied: the service word, a model zone, or a size the
                    // key itself states (a whole number).
                    let service = attribute.name == "utility.type";
                    let zone = attribute.name == "Check Zone";
                    let stated =
                        attribute.name == "Diameter (mm)" || attribute.name == "Height (mm)";
                    assert!(
                        service || zone || stated,
                        "{key} applies {}",
                        attribute.name
                    );
                    if zone || stated {
                        assert_eq!(attribute.r#type, "integer", "{key}");
                        assert!(
                            attribute.value.bytes().all(|byte| byte.is_ascii_digit()),
                            "{key}"
                        );
                    }
                }
            }
        }
        // census.py: attributesApplied 244, attributesPrompted 641. The
        // applied are 186 service words, 12 model zones, 28 stated diameters
        // (the 14 inverts PI01 to PI18 and the 14 pipes U1 to V8) and 18
        // stated heights (the culverts D0 to D9 and the box culverts B0 to B9
        // that exist), 244 in all (held at the top of this file).
        assert_eq!(applied, 244);
        assert_eq!(prompts, 641);
        let expected: BTreeMap<String, usize> = [
            ("Check Zone", 12),
            ("Diameter (mm)", 28),
            ("Height (mm)", 18),
            ("utility.type", 186),
        ]
        .into_iter()
        .map(|(name, count)| (String::from(name), count))
        .collect();
        assert_eq!(applied_names, expected);
    }

    #[test]
    fn every_definition_is_in_paper_millimetres_and_stays_inside_the_bounds_of_the_design() {
        let file = &standard().value;
        file.library.for_each(|style| {
            assert_eq!(style.units, StyleUnits::Paper, "{}", style.name);
            assert_eq!(style.factor, 1.0, "{}", style.name);
            assert_eq!(style.stretch_mode, 0, "{}", style.name);
            assert_eq!(style.cycle_mode, 0, "{}", style.name);
            assert_eq!(style.source, "Katana Standard", "{}", style.name);
            assert!(!style.group.is_empty(), "{}", style.name);
            let symbol = style.symbol;
            assert_eq!(style.at_vertices, symbol, "{}", style.name);
            assert!(
                style.strokes.len() <= if symbol { 70 } else { 90 },
                "{}",
                style.name
            );
            assert!(style.strokes.len() >= 2, "{}", style.name);
            if symbol {
                assert_eq!(
                    style.length, 0.0,
                    "{} is a symbol and has no period",
                    style.name
                );
            } else {
                assert!(style.length > 0.0, "{}", style.name);
            }
            let mut cx = 0.0;
            let mut cy = 0.0;
            for stroke in &style.strokes {
                if stroke.op == StrokeOp::Move || stroke.op == StrokeOp::Draw {
                    cx = stroke.point.x;
                    cy = stroke.point.y;
                    assert!(
                        at_most_three_decimals(cx) && at_most_three_decimals(cy),
                        "{}",
                        style.name
                    );
                }
                let mut low_x = cx;
                let mut high_x = cx;
                let mut low_y = cy;
                let mut high_y = cy;
                if stroke.op == StrokeOp::Circle {
                    low_x -= stroke.radius;
                    high_x += stroke.radius;
                    low_y -= stroke.radius;
                    high_y += stroke.radius;
                } else if stroke.op == StrokeOp::Arc {
                    // The ends and the four axes the sweep crosses give the
                    // box: the axes from -720 to 720 degrees, every multiple
                    // of 90 (exact in binary, as the C++'s += 90 makes them).
                    let mut angles = vec![stroke.start_angle, stroke.end_angle];
                    for quarter in -8..=8 {
                        let a = 90.0 * f64::from(quarter);
                        if a > cpp::min(stroke.start_angle, stroke.end_angle)
                            && a < cpp::max(stroke.start_angle, stroke.end_angle)
                        {
                            angles.push(a);
                        }
                    }
                    let pi = (-1.0_f64).acos();
                    for &a in &angles {
                        let x = cx + stroke.radius.abs() * (a * pi / 180.0).cos();
                        let y = cy + stroke.radius.abs() * (a * pi / 180.0).sin();
                        low_x = cpp::min(low_x, x);
                        high_x = cpp::max(high_x, x);
                        low_y = cpp::min(low_y, y);
                        high_y = cpp::max(high_y, y);
                    }
                }
                if stroke.op == StrokeOp::Circle
                    || stroke.op == StrokeOp::Arc
                    || stroke.op == StrokeOp::Dot
                {
                    assert!(at_most_three_decimals(stroke.radius), "{}", style.name);
                }
                if symbol {
                    continue;
                }
                // A cell closes on itself: x inside one period, y inside the
                // band.
                assert!(low_x >= -1e-9, "{}", style.name);
                assert!(high_x <= style.length + 1e-9, "{}", style.name);
                assert!(low_y >= -3.4 - 1e-9, "{}", style.name);
                assert!(high_y <= 3.4 + 1e-9, "{}", style.name);
            }
        });
    }

    #[test]
    fn every_definition_name_is_title_case_with_no_digit_and_no_stop() {
        let file = &standard().value;
        let small = ["and", "of"];
        file.library.for_each(|style| {
            for word in getline_pieces(&style.name, ' ') {
                let Some(&front) = word.as_bytes().first() else {
                    panic!("'{}'", style.name);
                };
                assert!(
                    front.is_ascii_uppercase() || small.contains(&word),
                    "{}: '{word}'",
                    style.name
                );
            }
            assert!(
                !style
                    .name
                    .bytes()
                    .any(|byte| b"0123456789.-_/".contains(&byte)),
                "{}",
                style.name
            );
        });
    }

    #[test]
    fn the_writer_writes_the_file_back_byte_for_byte() {
        let ks = standard();
        let file = &ks.value;
        let written = match customisation_to_json(file, &CustomisationWriteOptions::default()) {
            Ok(written) => written,
            Err(error) => panic!("{}", error.describe()),
        };
        assert_eq!(written.len(), ks.text.len());
        assert!(
            written.as_bytes() == ks.text.as_slice(),
            "the committed bytes are not the writer's canonical form"
        );
        // And once more: what the writer wrote reads back equal.
        let again = match customisation_from_json(written.as_bytes()) {
            Ok(again) => again,
            Err(error) => panic!("{}", error.describe()),
        };
        assert!(again == *file);
    }

    #[test]
    fn the_file_says_nothing_of_linework_or_automation_and_names_no_source() {
        let ks = standard();
        let file = &ks.value;
        // Absent is not the defaults: merged onto a session it must leave the
        // session's control words and switches alone.
        assert!(file.linework.is_none());
        assert!(file.automation.is_none());
        assert!(file.sources.is_empty());
        assert!(file.based_on.is_none());
        assert!(find_bytes(&ks.text, b"\"linework\"", 0).is_none());
        assert!(find_bytes(&ks.text, b"\"automation\"", 0).is_none());
    }

    #[test]
    fn a_numbered_string_answers_its_keys_rule_and_a_typo_answers_none() {
        let file = &standard().value;
        let map = &file.map;
        // WM, WM1 and WM01 are one code: the digits are the string number.
        for name in ["WM", "WM1", "WM01", "WM12"] {
            let found = map.lookup(name);
            assert!(found.matched(), "{name}");
            assert_eq!(found.resolved.model, "utilities/water/main", "{name}");
            assert_eq!(found.resolved.linestyle, "Water Supply Pipe", "{name}");
            assert_eq!(
                found.resolved.breakline,
                Some(SurveyBreakline::Line),
                "{name}"
            );
        }
        // A bulk bin is a line with a bin at every vertex: the one such key.
        let bin = map.lookup("BI2");
        assert_eq!(bin.resolved.breakline, Some(SurveyBreakline::Line));
        let Some(bin_symbol) = &bin.resolved.symbol else {
            panic!("BI2 has no symbol");
        };
        assert_eq!(bin_symbol.style, "Litter Bin");
        // The general fence is a line with no symbol, drawn as posts on a wire.
        let fence = map.lookup("FE3");
        assert_eq!(fence.resolved.linestyle, "Post and Wire Fence");
        assert!(fence.resolved.symbol.is_none());
        // A gully pit is a point with a symbol, and an exact key: PGUL1 is
        // none.
        let pit = map.lookup("PGUL");
        assert_eq!(pit.kind, SurveyMatchKind::Exact);
        assert_eq!(pit.resolved.breakline, Some(SurveyBreakline::Point));
        let Some(pit_symbol) = &pit.resolved.symbol else {
            panic!("PGUL has no symbol");
        };
        assert_eq!(pit_symbol.style, "Gully Grate");
        assert!(!map.lookup("PGUL1").matched());
        // The valve and the water main share a colour and not a layer.
        assert_eq!(
            map.lookup("PWSV").resolved.colour,
            map.lookup("WM").resolved.colour
        );
        assert_ne!(
            map.lookup("PWSV").resolved.model,
            map.lookup("WM").resolved.model
        );
        // A longer prefix wins field by field over a shorter one: Z0* over Z*.
        let index = map.lookup("Z07");
        assert_eq!(index.resolved.comment, "Index contour, regular");
        assert_eq!(map.lookup("Z1").resolved.comment, "Heavy contour");
        // A code that no key's prefix fits is answered by the bare `*` alone,
        // which names no layer, linestyle or symbol, so it is no match. None
        // of these starts a key: PWSW (the keys of P are exact), QZ (only QQ*
        // starts with Q), WQ, W, U, and the digits 6 and 9 (the digit keys are
        // 1 2 3 4 5 7 8).
        for typo in ["PWSW", "QZ", "WQ", "W", "U", "6", "9"] {
            let found = map.lookup(typo);
            assert!(!found.matched(), "{typo}");
            assert_eq!(found.kind, SurveyMatchKind::FallbackOnly, "{typo}");
        }
    }

    #[test]
    fn no_word_that_the_project_keeps_out_of_new_text_is_in_the_file() {
        let ks = standard();
        // std::tolower in the classic locale: ASCII letters only.
        let text = ks.text.to_ascii_lowercase();
        for word in forbidden_words() {
            assert!(
                find_bytes(&text, word.as_bytes(), 0).is_none(),
                "the file holds a forbidden word"
            );
        }
    }

    #[test]
    fn every_definition_and_every_code_is_catalogued_in_the_document() {
        let file = &standard().value;
        let doc = slurp(STANDARD_DOC);
        assert!(!doc.is_empty(), "no document at {STANDARD_DOC}");
        // The catalogue's tables open a LINE with the name, or the key, in
        // backticks. At the start of a line, so that a "Used by" cell holding
        // the one code that uses a definition does not count.
        let mut missing = Vec::new();
        file.library.for_each(|style| {
            let row = format!("\n| `{}` |", style.name);
            if find_bytes(&doc, row.as_bytes(), 0).is_none() {
                missing.push(style.name.clone());
            }
        });
        for key in file.map.keys() {
            // Once in its table and once in the index by key.
            let row = format!("\n| `{key}` |");
            let Some(first) = find_bytes(&doc, row.as_bytes(), 0) else {
                panic!("{key} is not catalogued");
            };
            assert!(
                find_bytes(&doc, row.as_bytes(), first + 1).is_some(),
                "{key} is not in the index"
            );
        }
        assert!(missing.is_empty(), "not catalogued: {missing:?}");
        // A code is looked up by what it is called as often as by its key:
        // every legend label opens a line of the index by label, and the key
        // that has it is on that line.
        let mut labels = 0;
        for (key, rule) in rules_of(file, SurveySection::Map) {
            assert!(
                !rule.comment.is_empty(),
                "{key} has a feature rule and no legend label"
            );
            labels += 1;
            let opening = format!("\n| {} | ", rule.comment);
            let Some(at) = find_bytes(&doc, opening.as_bytes(), 0) else {
                panic!("{key}: '{}' is not in the index", rule.comment);
            };
            let end = find_bytes(&doc, b"\n", at + 1).unwrap_or(doc.len());
            let line = &doc[at..end];
            assert!(
                find_bytes(line, format!("`{key}`").as_bytes(), 0).is_some(),
                "{key} is not on the line of its label '{}'",
                rule.comment
            );
        }
        // The 572 keys that have a feature rule (census.py: rulesBySets
        // feature).
        assert_eq!(labels, 572);
    }

    // ---- the keys of the NSW built-in ------------------------------------------------------

    #[test]
    fn every_key_of_the_nsw_built_in_is_here_and_no_other_key() {
        let file = &standard().value;
        let other = nsw();
        // Byte for byte: `AB`, `AB*`, `AB1*` and `*` are four keys. Both lists
        // are in name order.
        let ours = file.map.keys();
        let theirs = other.value.map.keys();
        // parity.py and tools/customisation_census.py: the other library has
        // 632 keys.
        assert_eq!(theirs.len(), 632);
        let our_set: BTreeSet<&String> = ours.iter().collect();
        let their_set: BTreeSet<&String> = theirs.iter().collect();
        let missing: Vec<&&String> = their_set.difference(&our_set).collect();
        let extra: Vec<&&String> = our_set.difference(&their_set).collect();
        assert!(
            missing.is_empty(),
            "{} keys are missing, the first {:?}",
            missing.len(),
            missing.first()
        );
        assert!(
            extra.is_empty(),
            "{} keys are extra, the first {:?}",
            extra.len(),
            extra.first()
        );
        assert_eq!(ours, theirs);
    }

    #[test]
    fn forty_codes_across_all_classes_resolve_to_the_kind_of_entity_the_nsw_built_in_makes() {
        let file = &standard().value;
        let other = nsw();
        // Forty keys, chosen by hand to take every class (a class of Katana
        // Standard's own arrangement), every kind of rule and every oddity once
        // or more: a line, a point, a line that is also a point's symbol (BI*),
        // a key that only sets a surface flag, a hidden symbol (PBDY), a vertex
        // attribute (PNAL), a model zone (KB*, FE*, TS*), the text keys, the
        // layer-only key, the bare `*`, and prefix families that overlap (Z*
        // and Z0*, PT04 and PT04*). AD* is left out: it is the one key whose
        // clean form differs, and parity_exceptions.json says so.
        let keys = [
            "BT*", "BE*", "PBDY", "PNAL", // boundaries and cadastre
            "CZ*", "AS*", "PTMX", // roads and pavements
            "S9*", "AC*", "BI*", "PSGL", // street furniture and signs
            "KB*", "U3*", "PGUL", "POBV", // kerbs and drainage
            "RA*", "PRBX", // rail and transit
            "WM*", "PWSV", "TS*", // utilities
            "PT04", "PT04*", "JD*", // vegetation
            "BU*", "UT*", "PFLG", // buildings and structures
            "FE*", "AG*", "POST", // fences and walls
            "Z0*", "Z*", "PSHT", // terrain
            "PBMK", "4", "8*", // survey control and annotation
            "PHAZ", "HP*", // miscellaneous
            "L8*", "BF*", // design and setting out
            "*",   // the bare star
        ];
        assert_eq!(keys.len(), 40);
        let distinct: BTreeSet<&str> = keys.iter().copied().collect();
        assert_eq!(distinct.len(), 40, "the sample names a key twice");
        for key in keys {
            assert!(
                has_section(file, key, SurveySection::Map)
                    || has_section(file, key, SurveySection::Tinable)
                    || has_section(file, key, SurveySection::VertexTextStyle)
                    || has_section(file, key, SurveySection::Pipe),
                "key {key}: the sample names a key this library lacks"
            );
            assert!(!other.value.map.rules().is_empty(), "key {key}");

            // 1. The same ordered kinds of rule. This library adds one
            //    attributes rule to a service key where the other has none;
            //    that is left out.
            let other_has_attributes =
                has_section(&other.value, key, SurveySection::StringAttribute);
            assert_eq!(
                sections_of(file, key, true, other_has_attributes),
                sections_of(&other.value, key, false, true),
                "key {key}"
            );

            // 2. The first attributes rule of the other library: the same
            //    entries (a type and whether a value is given), no more and no
            //    fewer.
            if other_has_attributes {
                let first_attributes = |library: &katana_entity::Customisation| {
                    library
                        .map
                        .rules()
                        .iter()
                        .find(|rule| {
                            rule.key == key && rule.section == SurveySection::StringAttribute
                        })
                        .map(|rule| rule.attributes.clone())
                };
                let Some(theirs) = first_attributes(&other.value) else {
                    panic!("key {key}");
                };
                let Some(ours) = first_attributes(file) else {
                    panic!("key {key}");
                };
                assert_eq!(ours.len(), theirs.len(), "key {key}");
                for (i, (mine, its)) in ours.iter().zip(&theirs).enumerate() {
                    assert_eq!(mine.r#type, its.r#type, "key {key}: entry {i}");
                    assert_eq!(
                        mine.value.is_empty(),
                        its.value.is_empty(),
                        "key {key}: entry {i}"
                    );
                }
            }

            // 3. The same code makes the same kind of entity. A code that the
            //    key answers: the key without its star, or the key itself.
            let code = if key == "*" {
                "QZ"
            } else {
                key.strip_suffix('*').unwrap_or(key)
            };
            let a = kind_of(&other.value, code);
            let b = kind_of(file, code);
            assert_eq!(b.matched, a.matched, "key {key}: {code}");
            assert_eq!(
                b.breakline, a.breakline,
                "key {key}: {code}: a line in one and a point in the other"
            );
            assert_eq!(b.tinable, a.tinable, "key {key}: {code}: the surface flag");
            assert_eq!(b.hide, a.hide, "key {key}: {code}");
            assert_eq!(
                b.symbol, a.symbol,
                "key {key}: {code}: a symbol in one and none in the other"
            );
            assert_eq!(b.layer_named, a.layer_named, "key {key}: {code}");
            assert_eq!(b.colour_named, a.colour_named, "key {key}: {code}");
            assert_eq!(b.text, a.text, "key {key}: {code}");
            assert_eq!(b.text_units, a.text_units, "key {key}: {code}");
            assert_eq!(b.text_size, a.text_size, "key {key}: {code}");
            assert_eq!(b.text_justify_x, a.text_justify_x, "key {key}: {code}");
            assert_eq!(b.text_justify_y, a.text_justify_y, "key {key}: {code}");
            assert_eq!(b.pipe, a.pipe, "key {key}: {code}");
            assert_eq!(b.vertex_pipe, a.vertex_pipe, "key {key}: {code}");
            assert_eq!(b.segment_pipe, a.segment_pipe, "key {key}: {code}");
            assert_eq!(b.pipe_justify, a.pipe_justify, "key {key}: {code}");
            assert_eq!(b.pipe_shape, a.pipe_shape, "key {key}: {code}");
            assert_eq!(b.pipe_active, a.pipe_active, "key {key}: {code}");
            assert_eq!(b.pipe_size1, a.pipe_size1, "key {key}: {code}");
            assert_eq!(b.pipe_size2, a.pipe_size2, "key {key}: {code}");
        }
        // The sample reaches what it claims to: it holds a hidden symbol, a
        // text key, a pipe, a key with only a surface flag and a line with a
        // symbol.
        assert!(kind_of(file, "PBDY").hide);
        assert!(kind_of(&other.value, "PBDY").hide);
        assert!(kind_of(file, "4").text);
        assert!(kind_of(file, "QZ").pipe);
        assert_eq!(kind_of(file, "PT04").tinable, 0);
        assert!(kind_of(file, "BI1").symbol);
        assert_eq!(kind_of(file, "BI1").breakline, 1);
    }

    #[test]
    fn nothing_but_the_keys_is_shared_with_any_other_compiled_in_built_in() {
        let file = &standard().value;
        // EVERY built-in this build compiled in but this one: Katana Standard
        // is the default now, so the other is an alternate, and a build may
        // carry several. Skipped only where there is none to compare with. The
        // keys are promised the same for the NSW customisation alone, which is
        // known here by the name the tracked file gives itself; of any other
        // alternate a build was given, nothing is said about its keys.
        let tracked = nsw();
        let mut compared = 0;
        for entry in built_ins() {
            let Some(customisation) = &entry.built_in.customisation else {
                continue;
            };
            if customisation.name == file.name {
                continue;
            }
            compared += 1;
            eprintln!("compared with the built-in number {compared}");
            let is_the_nsw_one = customisation.name == tracked.value.name;
            expect_nothing_but_the_keys_in_common_with(
                file,
                customisation,
                if is_the_nsw_one {
                    Keys::TheSame
                } else {
                    Keys::NotCompared
                },
            );
        }
        if compared == 0 {
            eprintln!("this build has no other customisation compiled in to compare with");
        }
    }

    #[test]
    fn nothing_but_the_keys_is_shared_with_the_nsw_file_tracked_beside_it() {
        let file = &standard().value;
        // The same comparison against the tracked file, so that it runs in
        // every build and not only in one that compiled the other built-in in.
        let other = nsw();
        expect_nothing_but_the_keys_in_common_with(file, &other.value, Keys::TheSame);
    }
}
