//! The small pieces every annotation manager's form is made of (annotation_managers,
//! leader_manager): a named number box, check box, field, button and problem line, an
//! enumeration's names in a choice, and a refusal in the command line's own words. One place, so
//! the managers' fields are named, sized and worded alike. The port of
//! annotation/form_widgets.hpp, over the named controls (drive::controls).
//!
//! The C++'s `chosen` and `choose` read and set a QComboBox's enumeration value by its item data;
//! here `combo` draws over the enumeration value itself, so they have no counterpart.

use egui::{Response, Ui};
use katana_core::Status;
use katana_entity::EnumName;
use katana_entity::entity::to_string;

use crate::drive::controls::{self, Choice, LineOptions, SpinRange};

/// The colour of a problem line (#d9534f), as the managers' stylesheet gave it.
const PROBLEM_COLOUR: egui::Color32 = egui::Color32::from_rgb(0xd9, 0x53, 0x4f);

/// A number box named `name`: from `minimum` to `maximum`, stepped by `step`, shown to `decimals`
/// places with `suffix` after.
#[allow(
    clippy::too_many_arguments,
    reason = "the C++ helper's own parameters, in its order"
)]
pub fn spin(
    ui: &mut Ui,
    name: &str,
    value: &mut f64,
    minimum: f64,
    maximum: f64,
    step: f64,
    decimals: usize,
    suffix: &'static str,
) -> Response {
    controls::spin(
        ui,
        name,
        value,
        SpinRange::new(minimum, maximum, step, decimals).with_suffix(suffix),
    )
}

/// A check box named `name`.
pub fn check(ui: &mut Ui, name: &str, text: &str, on: &mut bool) -> Response {
    controls::check(ui, name, text, on)
}

/// A field named `name`, with `placeholder` shown while it is empty.
pub fn edit(ui: &mut Ui, name: &str, value: &mut String, placeholder: &str) -> Response {
    controls::line_with(
        ui,
        name,
        value,
        LineOptions {
            placeholder,
            ..LineOptions::default()
        },
    )
}

/// A button named `name`. (The C++ turned off its auto-default, so Enter in a field never pressed
/// it; an egui button is never pressed by Enter in another control.)
pub fn button(ui: &mut Ui, name: &str, text: &str) -> Response {
    controls::button(ui, name, text)
}

/// The line a manager says what it refused on: named `name`, wrapped, in the problem colour.
pub fn problem_line(ui: &mut Ui, name: &str, text: &str) -> Response {
    controls::label_as(
        ui,
        name,
        text,
        egui::Label::new(egui::RichText::new(text).color(PROBLEM_COLOUR)).wrap(),
    )
}

/// An enumeration's names in a choice named `name`, by value: `values` in their order, each shown
/// as its `to_string` name, `chosen` the one chosen.
pub fn combo<E: EnumName + PartialEq>(
    ui: &mut Ui,
    name: &str,
    values: &[E],
    chosen: &mut E,
) -> Response {
    let items: Vec<String> = values
        .iter()
        .map(|&value| String::from(to_string(value)))
        .collect();
    let mut choice = Choice {
        index: values.iter().position(|value| value == chosen),
        ..Choice::default()
    };
    let response = controls::choice(ui, name, &items, &mut choice);
    if let Some(value) = choice.index.and_then(|at| values.get(at)) {
        *chosen = *value;
    }
    response
}

/// A refusal as the problem line says it: the message, and what it was about; empty for success.
pub fn describe(status: &Status) -> String {
    match status {
        Ok(()) => String::new(),
        Err(error) if error.context.is_empty() => error.message.clone(),
        Err(error) => format!("{}: {}", error.message, error.context),
    }
}
