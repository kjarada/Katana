//! DIMSTYLE's fields as the command line types them
//! (include/katana/cad/annotation/dimension_style_verbs.hpp;
//! docs/annotation.md, "Dimension styles"): the one statement of what each
//! field is called, how its value is read and how it is written back. The
//! interpreter's DIMSTYLE reads its NEW and SET lines with it and writes
//! INFO's record with it, and the window's Dimension Styles manager builds its
//! lines with it - so the dialog cannot write a line the verb would read
//! differently.
//!
//! ```text
//! TEXT GAP EXTOFF EXTBEYOND ARROW   the sizes: model units, or paper mm for a
//!                                   paper-sized style
//! HEAD                              None | Tick | ClosedFilled | Open | Dot
//! SCALE DECIMALS ROUND              the unit factor, the places, the step
//! PREFIX SUFFIX                     text before and after the number
//! TRIM PAPER                        on | off
//! ```
//!
//! Field names are case-insensitive; a line gives them in upper case and a
//! record in lower case, as the other annotation verbs' keys are.

use std::sync::OnceLock;

use katana_core::error::make_error;
use katana_core::text::{
    equals_ignoring_case, format_exact_real, lowered, parse_finite_double, parse_integer,
};
use katana_core::{ErrorCode, Result, Status};
use katana_entity::dimension_text::format_measurement;
use katana_entity::tables::arrow_head_from_string;
use katana_entity::{DimensionStyle, EnumName, Model};

use crate::annotation::command_words::{command_word, record_value};

// Where each field lives in the style. A field is a number, a whole number, a
// head, a text or a switch; the table below is the one list of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Text,
    Gap,
    ExtensionOffset,
    ExtensionBeyond,
    Arrow,
    Head,
    Scale,
    Decimals,
    Round,
    Prefix,
    Suffix,
    Trim,
    Paper,
}

const FIELDS: [(&str, Field); 13] = [
    ("TEXT", Field::Text),
    ("GAP", Field::Gap),
    ("EXTOFF", Field::ExtensionOffset),
    ("EXTBEYOND", Field::ExtensionBeyond),
    ("ARROW", Field::Arrow),
    ("HEAD", Field::Head),
    ("SCALE", Field::Scale),
    ("DECIMALS", Field::Decimals),
    ("ROUND", Field::Round),
    ("PREFIX", Field::Prefix),
    ("SUFFIX", Field::Suffix),
    ("TRIM", Field::Trim),
    ("PAPER", Field::Paper),
];

fn find_field(name: &str) -> Option<(&'static str, Field)> {
    FIELDS
        .iter()
        .copied()
        .find(|(field_name, _)| equals_ignoring_case(field_name.as_bytes(), name.as_bytes()))
}

fn field_list() -> String {
    let mut list = String::new();
    for (name, _) in FIELDS {
        if !list.is_empty() {
            list.push(' ');
        }
        list.push_str(name);
    }
    list
}

// A field's name in lower case, as a record keys it.
fn lowered_name(name: &str) -> String {
    String::from_utf8_lossy(&lowered(name)).into_owned()
}

// The words a switch takes, as the annotation verbs' switches do. Anything
// else is refused: "PAPER of" once turned paper sizing off without a word.
fn switch_of(value: &str, field: &str) -> Result<bool> {
    match lowered(value).as_slice() {
        b"on" | b"yes" | b"true" | b"1" => Ok(true),
        b"off" | b"no" | b"false" | b"0" => Ok(false),
        _ => Err(make_error(
            ErrorCode::ParseFailure,
            format!("expected on or off for {field}"),
            value,
        )),
    }
}

fn on_off(on: bool) -> String {
    String::from(if on { "on" } else { "off" })
}

fn value_of(style: &DimensionStyle, field: Field) -> String {
    match field {
        Field::Text => format_exact_real(style.text_height),
        Field::Gap => format_exact_real(style.text_gap),
        Field::ExtensionOffset => format_exact_real(style.extension_offset),
        Field::ExtensionBeyond => format_exact_real(style.extension_beyond),
        Field::Arrow => format_exact_real(style.arrow_size),
        Field::Head => String::from(style.arrow_head.name()),
        Field::Scale => format_exact_real(style.unit_scale),
        Field::Decimals => format!("{}", style.decimals),
        Field::Round => format_exact_real(style.round_to),
        Field::Prefix => style.prefix.clone(),
        Field::Suffix => style.suffix.clone(),
        Field::Trim => on_off(style.suppress_trailing_zeros),
        Field::Paper => on_off(style.paper_sized),
    }
}

/// The fields, upper case, in the order a line and a record give them.
pub fn dimension_style_fields() -> &'static [&'static str] {
    static NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| FIELDS.iter().map(|&(name, _)| name).collect())
}

/// Sets one field of `style` from its typed value. InvalidArgument for a
/// field that does not exist (naming the fields that do), ParseFailure for a
/// value that does not read - a number, a head's name, on or off. Whether the
/// value is allowed (a text height of zero) is validate()'s, which the table
/// command applies.
pub fn set_dimension_style_field(style: &mut DimensionStyle, field: &str, value: &str) -> Status {
    let typed = field;
    let Some((name, field)) = find_field(typed) else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "unknown dimension style field",
            format!("{typed} (fields: {})", field_list()),
        ));
    };
    let number = || {
        parse_finite_double(value).ok_or_else(|| {
            make_error(
                ErrorCode::ParseFailure,
                format!("expected a number for {name}"),
                value,
            )
        })
    };
    // A whole number: "2.7" places would be quietly two. Its range is
    // validate()'s; here only that it is an int at all.
    let whole = || {
        parse_integer(value)
            .and_then(|whole| i32::try_from(whole).ok())
            .ok_or_else(|| {
                make_error(
                    ErrorCode::ParseFailure,
                    format!("expected a whole number for {name}"),
                    value,
                )
            })
    };
    match field {
        Field::Text => style.text_height = number()?,
        Field::Gap => style.text_gap = number()?,
        Field::ExtensionOffset => style.extension_offset = number()?,
        Field::ExtensionBeyond => style.extension_beyond = number()?,
        Field::Arrow => style.arrow_size = number()?,
        Field::Head => style.arrow_head = arrow_head_from_string(value)?,
        Field::Scale => style.unit_scale = number()?,
        Field::Decimals => style.decimals = whole()?,
        Field::Round => style.round_to = number()?,
        Field::Prefix => style.prefix = String::from(value),
        Field::Suffix => style.suffix = String::from(value),
        Field::Trim => style.suppress_trailing_zeros = switch_of(value, name)?,
        Field::Paper => style.paper_sized = switch_of(value, name)?,
    }
    Ok(())
}

/// `words` as "FIELD value FIELD value ...", applied in order onto `style`,
/// ALL OR NOTHING: on any failure `style` is as it was. InvalidArgument for an
/// odd count, which would leave a field with no value.
pub fn set_dimension_style_fields(style: &mut DimensionStyle, words: &[String]) -> Status {
    if words.is_empty() || !words.len().is_multiple_of(2) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "fields come in pairs: FIELD value [FIELD value ...]",
            format!("fields: {}", field_list()),
        ));
    }
    let mut changed = style.clone();
    for i in (0..words.len()).step_by(2) {
        set_dimension_style_field(&mut changed, &words[i], &words[i + 1])?;
    }
    *style = changed;
    Ok(())
}

/// One field's value as a line and a record give it: numbers exact (core's
/// format_exact_real, so 0.625 reads back as 0.625), the head by name, on or
/// off, a prefix or suffix as it is. Empty for an unknown field.
pub fn dimension_style_field_value(style: &DimensionStyle, field: &str) -> String {
    find_field(field).map_or_else(String::new, |(_, field)| value_of(style, field))
}

/// The "FIELD value" pairs that turn `from` into `to`, each value one word of
/// a command line (command_words), in field order, separated by blanks: what
/// a SET line needs to carry. Empty when the two agree field for field (the
/// name is not a field). InvalidArgument when a prefix or suffix that differs
/// holds a double quote, which no command line can carry.
pub fn dimension_style_changes(from: &DimensionStyle, to: &DimensionStyle) -> Result<String> {
    let mut line = String::new();
    for (name, field) in FIELDS {
        let before = value_of(from, field);
        let after = value_of(to, field);
        if before == after {
            continue;
        }
        let Ok(word) = command_word(&after) else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "the {} holds a double quote, which a command line cannot carry",
                    lowered_name(name)
                ),
                after,
            ));
        };
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(name);
        line.push(' ');
        line.push_str(&word);
    }
    Ok(line)
}

/// The layers that name the style, ascending - what DIMSTYLE DELETE is
/// refused for. A layer that names none is drawn with the default style but
/// does not name it, and is not counted.
pub fn layers_using_dimension_style(model: &Model, name: &str) -> Vec<String> {
    let mut users = Vec::new();
    for layer in model.layers.all() {
        if layer.dimension_style == name {
            users.push(layer.name);
        }
    }
    users
}

/// DIMSTYLE INFO's record: name=, every field in lower case (key=value, a
/// value quoted when it holds a blank or is empty), layers=N
/// (layers_using_dimension_style) and reads= - what a dimension of ten units
/// reads as, the question anyone setting a style is asking.
pub fn describe_dimension_style(model: &Model, style: &DimensionStyle) -> String {
    let mut out = format!("name={}", record_value(&style.name));
    for (name, field) in FIELDS {
        out.push(' ');
        out.push_str(&lowered_name(name));
        out.push('=');
        out.push_str(&record_value(&value_of(style, field)));
    }
    out.push_str(&format!(
        " layers={} reads={}",
        layers_using_dimension_style(model, &style.name).len(),
        record_value(&format_measurement(10.0, style))
    ));
    out
}
