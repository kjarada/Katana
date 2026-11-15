//! The controls of one GDAL algorithm's arguments (geo/argument_form.hpp; docs/geoprocessing.md,
//! "The window"): what the GDAL Toolbox's Algorithm tab and each step of its Pipeline tab show.
//!
//! Nothing here is written by hand for an algorithm: the controls are built from what GDAL declares
//! of each argument, read at run time (`katana_gis::processing::describe`) - a box to tick for a
//! flag, a choice for a word with choices, a number box held to GDAL's bounds, a field for the
//! rest - so an algorithm GDAL adds has its form with no change here. Each control is named
//! <prefix><argument> ("gdalArg.zfactor"). The arguments GDAL files as Base, and the required
//! ones, are shown; the rest sit under Advanced (<advancedName>), closed at first.
//!
//! An argument is GIVEN when its control holds something other than GDAL's default, and only a
//! given argument is a word of the line, so a line says what a person chose and no more. A number
//! box says "not given" by its least value, which is below GDAL's minimum.

use std::collections::BTreeSet;

use egui::Ui;
use katana_core::text::{format_general, parse_finite_double, parse_integer};
use katana_core::Result;
use katana_gis::processing as gp;

use super::geo_dialog_support::line_word;
use super::terrain_dialog_parts::invalid;
use crate::drive::{Choice, LineOptions, SpinRange, check, choice, line_with, spin, whole};
use crate::survey::survey_dialogs::hidden;

/// Far beyond any value a survey takes, and exact in a double: the ends of a number box with no
/// bound of GDAL's.
const UNBOUNDED: f64 = 1e12;

/// The places a real's box keeps (QDoubleSpinBox::setDecimals(9)).
const REAL_DECIMALS: usize = 9;

/// The control of one argument, and what it holds.
#[derive(Clone, Debug, PartialEq)]
enum Control {
    Check(bool),
    Choice {
        items: Vec<String>,
        chosen: Choice,
    },
    Whole {
        value: i64,
        minimum: i64,
        maximum: i64,
    },
    Real {
        value: f64,
        minimum: f64,
        maximum: f64,
    },
    Line(String),
}

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    spec: gp::ArgSpec,
    control: Control,
    /// Shown above Advanced: GDAL files it as Base, or it is required.
    base: bool,
}

fn is_list(kind: gp::ArgType) -> bool {
    matches!(
        kind,
        gp::ArgType::StringList | gp::ArgType::IntegerList | gp::ArgType::RealList
    )
}

/// GDAL's default as a line writes it; empty for none.
fn default_word(arg: &gp::ArgSpec) -> String {
    arg.default_value
        .as_ref()
        .map(gp::to_string)
        .unwrap_or_default()
}

/// The number box's value that says "not given": below GDAL's minimum, or the far end when it has
/// none.
fn sentinel_of(arg: &gp::ArgSpec) -> f64 {
    arg.min.map_or(-UNBOUNDED, |bound| bound.value - 1.0)
}

fn reads_as_number(text: &str, whole: bool) -> bool {
    parse_finite_double(text.trim()).is_some_and(|number| !whole || number == number.floor())
}

/// QString::toInt: a whole number in an int's range, blanks round it ignored; 0 for anything else
/// ("2.0" too).
fn to_int(text: &str) -> i64 {
    parse_integer(text.trim())
        .filter(|number| i32::try_from(*number).is_ok())
        .unwrap_or(0)
}

/// QString::toDouble: 0 for a text that is no number.
fn to_double(text: &str) -> f64 {
    parse_finite_double(text.trim()).unwrap_or(0.0)
}

fn word(name: &str, value: &str) -> String {
    format!("--{name}={value}")
}

/// A real as a line writes it: 2, 0.5, 1e-06 - never 2.000000.
fn real_text(value: f64) -> String {
    format_general(value, 12)
}

fn real_range(minimum: f64, maximum: f64) -> SpinRange {
    SpinRange::new(minimum, maximum, 1.0, REAL_DECIMALS)
}

/// QDoubleSpinBox::setValue: rounded to the box's places and held to its range.
fn real_set(value: f64, minimum: f64, maximum: f64) -> f64 {
    real_range(minimum, maximum)
        .rounded(value)
        .clamp(minimum, maximum)
}

/// The controls of one algorithm's arguments (see the module comment).
#[derive(Clone, Debug, PartialEq)]
pub struct ArgumentForm {
    prefix: String,
    advanced_name: String,
    path: Vec<String>,
    entries: Vec<Entry>,
    advanced_open: bool,
}

impl ArgumentForm {
    /// A form with no algorithm yet, its controls to be named `prefix` and the argument, its
    /// Advanced group `advanced_name`.
    pub fn new(prefix: &str, advanced_name: &str) -> Self {
        Self {
            prefix: String::from(prefix),
            advanced_name: String::from(advanced_name),
            path: Vec::new(),
            entries: Vec::new(),
            advanced_open: false,
        }
    }

    /// Builds the controls for `spec`'s arguments: every one that is no dataset, is not refused by
    /// the executor and is not in `leave_out`; with `pipeline_step`, only those a pipeline's step
    /// takes. Replaces what was.
    pub fn set_algorithm(
        &mut self,
        spec: &gp::AlgorithmSpec,
        leave_out: &BTreeSet<String>,
        pipeline_step: bool,
    ) {
        self.entries.clear();
        self.path.clone_from(&spec.info.path);
        for arg in &spec.args {
            if arg.is_dataset()
                || arg.is_output
                || leave_out.contains(&arg.name)
                || (pipeline_step && !arg.in_pipeline_step)
            {
                continue;
            }
            // What the executor refuses whatever the algorithm (--quiet) is no choice to offer.
            if gp::check_tokens(&self.path, &[format!("--{}", arg.name)]).is_err() {
                continue;
            }
            let fallback = default_word(arg);
            let control = if arg.r#type == gp::ArgType::Boolean {
                Control::Check(fallback == "true")
            } else if arg.r#type == gp::ArgType::String && !arg.choices.is_empty() {
                let mut items = vec![String::new()];
                items.extend(arg.choices.iter().cloned());
                // setCurrentText on a choice that is not editable: the item of that text, else
                // the first, which is blank.
                let index = items
                    .iter()
                    .position(|item| *item == fallback)
                    .unwrap_or(0);
                Control::Choice {
                    items,
                    chosen: Choice::at(index),
                }
            } else if arg.r#type == gp::ArgType::Integer {
                let low = sentinel_of(arg).max(f64::from(i32::MIN));
                let high = arg.max.map_or(f64::from(i32::MAX), |bound| bound.value);
                let minimum = low.ceil() as i64;
                let maximum = (high.floor() as i64).max(minimum);
                let value = if reads_as_number(&fallback, true) {
                    to_int(&fallback).clamp(minimum, maximum)
                } else {
                    minimum
                };
                Control::Whole {
                    value,
                    minimum,
                    maximum,
                }
            } else if arg.r#type == gp::ArgType::Real {
                let minimum = sentinel_of(arg);
                let maximum = arg.max.map_or(UNBOUNDED, |bound| bound.value).max(minimum);
                let value = if reads_as_number(&fallback, false) {
                    real_set(to_double(&fallback), minimum, maximum)
                } else {
                    minimum
                };
                Control::Real {
                    value,
                    minimum,
                    maximum,
                }
            } else {
                Control::Line(fallback)
            };
            let base = arg.category == "Base" || arg.required;
            self.entries.push(Entry {
                spec: arg.clone(),
                control,
                base,
            });
        }
    }

    /// What the control holds, as a line writes it; empty for a number box left at "not given".
    #[allow(
        clippy::float_cmp,
        reason = "A real's box says not given by holding exactly its least value."
    )]
    fn text(entry: &Entry) -> String {
        match &entry.control {
            Control::Check(on) => String::from(if *on { "true" } else { "false" }),
            Control::Choice { items, chosen } => chosen.current_text(items),
            Control::Whole { value, minimum, .. } => {
                if value == minimum {
                    String::new()
                } else {
                    format!("{value}")
                }
            }
            Control::Real { value, minimum, .. } => {
                if value == minimum {
                    String::new()
                } else {
                    real_text(*value)
                }
            }
            Control::Line(text) => String::from(text.trim()),
        }
    }

    fn default_text(entry: &Entry) -> String {
        if entry.spec.r#type == gp::ArgType::Boolean {
            return String::from(if default_word(&entry.spec) == "true" {
                "true"
            } else {
                "false"
            });
        }
        default_word(&entry.spec)
    }

    fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.spec.name == name)
    }

    fn entry_given(entry: &Entry) -> bool {
        let value = Self::text(entry);
        !value.is_empty() && value != Self::default_text(entry)
    }

    /// Whether an argument is given: set to something other than GDAL's default.
    pub fn given(&self, name: &str) -> bool {
        self.entry(name).is_some_and(Self::entry_given)
    }

    /// The arguments the form shows, in GDAL's order.
    pub fn arguments(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| entry.spec.name.clone())
            .collect()
    }

    /// Whether the form has a control for the argument `name` (the C++'s `control(name)` not
    /// null).
    pub fn has_control(&self, name: &str) -> bool {
        self.entry(name).is_some()
    }

    /// The object name of an argument's control; None when the form has none of that name.
    pub fn control_name(&self, name: &str) -> Option<String> {
        self.entry(name)
            .map(|entry| format!("{}{}", self.prefix, entry.spec.name))
    }

    /// The least a number box takes - its "not given" - and the most; None for an argument that
    /// has no number box.
    pub fn number_range(&self, name: &str) -> Option<(f64, f64)> {
        match self.entry(name)?.control {
            Control::Whole {
                minimum, maximum, ..
            } => Some((minimum as f64, maximum as f64)),
            Control::Real {
                minimum, maximum, ..
            } => Some((minimum, maximum)),
            _ => None,
        }
    }

    /// The items a choice offers, the blank first; None for an argument that has no choice.
    pub fn choices(&self, name: &str) -> Option<&[String]> {
        match &self.entry(name)?.control {
            Control::Choice { items, .. } => Some(items),
            _ => None,
        }
    }

    /// What an argument's control holds, as a line writes it; None when the form has no such
    /// argument.
    pub fn value(&self, name: &str) -> Option<String> {
        self.entry(name).map(Self::text)
    }

    /// Whether the control at `at` takes an edit: not one of an exclusion group whose rival is
    /// given, nor dependent on an argument the form shows that is not given.
    fn enabled(&self, at: usize) -> bool {
        let entry = &self.entries[at];
        // One of an exclusion group: disabled once a rival is given.
        if !entry.spec.exclusion_group.is_empty() {
            for (other, rival) in self.entries.iter().enumerate() {
                if other != at
                    && rival.spec.exclusion_group == entry.spec.exclusion_group
                    && Self::entry_given(rival)
                {
                    return false;
                }
            }
        }
        // Dependent: enabled once what it depends on (that the form shows) is given.
        for other in &entry.spec.depends_on {
            if self.has_control(other) && !self.given(other) {
                return false;
            }
        }
        true
    }

    /// Whether an argument's control takes an edit; false when the form has none of that name.
    pub fn is_enabled(&self, name: &str) -> bool {
        self.entries
            .iter()
            .position(|entry| entry.spec.name == name)
            .is_some_and(|at| self.enabled(at))
    }

    /// The words of the arguments given, --name=value in GDAL's order (a word with a blank quoted,
    /// as the command line reads it), or why a value is refused, naming the argument: not a
    /// number, below an exclusive minimum or above a maximum, a list with too few or too many
    /// items, a double quote, a required argument not given.
    pub fn words(&self) -> Result<Vec<String>> {
        let mut words = Vec::new();
        for (at, entry) in self.entries.iter().enumerate() {
            let arg = &entry.spec;
            let name = arg.name.as_str();
            let enabled = self.enabled(at);
            let given = Self::entry_given(entry);
            if !enabled || !given {
                if arg.required && enabled && !given && Self::default_text(entry).is_empty() {
                    return Err(invalid(format!("{name} is required: give it a value")));
                }
                continue;
            }
            let value = Self::text(entry);
            if value.contains('"') {
                return Err(invalid(format!(
                    "{name} holds a double quote, which a command line cannot carry"
                )));
            }
            match arg.r#type {
                gp::ArgType::Boolean => {
                    words.push(if value == "true" {
                        format!("--{name}")
                    } else {
                        word(name, "false")
                    });
                }
                gp::ArgType::Integer | gp::ArgType::Real => {
                    let number = to_double(&value);
                    if let Some(bound) = arg.min
                        && (if bound.inclusive {
                            number < bound.value
                        } else {
                            number <= bound.value
                        })
                    {
                        return Err(invalid(format!(
                            "{name} must be {}{}, not {value}",
                            if bound.inclusive {
                                "at least "
                            } else {
                                "above "
                            },
                            real_text(bound.value)
                        )));
                    }
                    if let Some(bound) = arg.max
                        && (if bound.inclusive {
                            number > bound.value
                        } else {
                            number >= bound.value
                        })
                    {
                        return Err(invalid(format!(
                            "{name} must be {}{}, not {value}",
                            if bound.inclusive {
                                "at most "
                            } else {
                                "below "
                            },
                            real_text(bound.value)
                        )));
                    }
                    words.push(word(name, &value));
                }
                gp::ArgType::StringList | gp::ArgType::IntegerList | gp::ArgType::RealList => {
                    words.extend(Self::list_words(arg, &value)?);
                }
                gp::ArgType::String => {
                    words.push(line_word(&word(name, &value), name)?);
                }
                gp::ArgType::Dataset | gp::ArgType::DatasetList => {}
            }
        }
        Ok(words)
    }

    /// A list argument's words: one packed word, or a word an item.
    fn list_words(arg: &gp::ArgSpec, value: &str) -> Result<Vec<String>> {
        let name = arg.name.as_str();
        let items: Vec<&str> = value.split(',').map(str::trim).collect();
        let count = i32::try_from(items.len()).unwrap_or(i32::MAX);
        if count < arg.min_count || (arg.max_count >= 0 && count > arg.max_count) {
            let wanted = if arg.min_count == arg.max_count {
                format!("{}", arg.min_count)
            } else {
                format!(
                    "{} to {}",
                    arg.min_count,
                    if arg.max_count < 0 {
                        String::from("any number")
                    } else {
                        format!("{}", arg.max_count)
                    }
                )
            };
            return Err(invalid(format!(
                "{name} takes {wanted} values, not {count}"
            )));
        }
        for item in &items {
            if item.is_empty()
                || (arg.r#type != gp::ArgType::StringList
                    && !reads_as_number(item, arg.r#type == gp::ArgType::IntegerList))
            {
                let kind = match arg.r#type {
                    gp::ArgType::StringList => "words",
                    gp::ArgType::IntegerList => "whole numbers",
                    _ => "numbers",
                };
                return Err(invalid(format!(
                    "{name} takes {kind} separated by commas: '{value}'"
                )));
            }
        }
        // Packed lists (--size=10,20) are one word; the rest are given once per item, as GDAL's
        // own command line repeats them.
        if arg.packed_values || !arg.repeatable {
            return Ok(vec![line_word(&word(name, &items.join(",")), name)?]);
        }
        items
            .iter()
            .map(|item| line_word(&word(name, item), name))
            .collect()
    }

    /// Sets an argument from a value as a line writes it ("2", "flat", "a,b", "true"): what
    /// re-reading a pipeline's text does. False when the form has no such argument or the value
    /// does not fit its control.
    #[allow(
        clippy::float_cmp,
        reason = "A value fits a real's box when the box holds exactly it afterwards."
    )]
    pub fn set_value(&mut self, name: &str, value: &str) -> bool {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.spec.name == name)
        else {
            return false;
        };
        match &mut entry.control {
            Control::Check(on) => {
                let lower = value.to_lowercase();
                *on = value.is_empty() || lower == "true" || value == "1" || lower == "yes";
                true
            }
            Control::Choice { items, chosen } => {
                // Qt::MatchFixedString: the whole text, letter case not counted.
                let wanted = value.to_lowercase();
                match items.iter().position(|item| item.to_lowercase() == wanted) {
                    Some(index) => {
                        chosen.index = Some(index);
                        true
                    }
                    None => false,
                }
            }
            Control::Whole {
                value: held,
                minimum,
                maximum,
            } => {
                if !reads_as_number(value, true) {
                    return false;
                }
                let wanted = to_int(value);
                *held = wanted.clamp(*minimum, *maximum);
                *held == wanted
            }
            Control::Real {
                value: held,
                minimum,
                maximum,
            } => {
                if !reads_as_number(value, false) {
                    return false;
                }
                let wanted = to_double(value);
                *held = real_set(wanted, *minimum, *maximum);
                *held == wanted
            }
            Control::Line(text) => {
                *text = String::from(value);
                true
            }
        }
    }

    /// Every control back to GDAL's default.
    pub fn clear(&mut self) {
        for entry in &mut self.entries {
            let fallback = default_word(&entry.spec);
            match &mut entry.control {
                Control::Check(on) => *on = fallback == "true",
                Control::Choice { items, chosen } => {
                    if let Some(index) = items.iter().position(|item| *item == fallback) {
                        chosen.index = Some(index);
                    }
                }
                Control::Whole {
                    value,
                    minimum,
                    maximum,
                } => {
                    *value = if reads_as_number(&fallback, true) {
                        to_int(&fallback).clamp(*minimum, *maximum)
                    } else {
                        *minimum
                    };
                }
                Control::Real {
                    value,
                    minimum,
                    maximum,
                } => {
                    *value = if reads_as_number(&fallback, false) {
                        real_set(to_double(&fallback), *minimum, *maximum)
                    } else {
                        *minimum
                    };
                }
                Control::Line(text) => *text = fallback,
            }
        }
    }

    /// One row: the argument's label and its control. True when the control was edited.
    fn row(ui: &mut Ui, prefix: &str, entry: &mut Entry, enabled: bool) -> bool {
        let arg = &entry.spec;
        let name = format!("{prefix}{}", arg.name);
        let tip = format!(
            "{}{}",
            arg.description,
            if arg.required { " (required)" } else { "" }
        );
        let label = format!("{}{}:", arg.name, if arg.required { " *" } else { "" });
        let placeholder = if is_list(arg.r#type) { "a,b,c" } else { "" };
        ui.horizontal(|ui| {
            ui.label(label);
            ui.add_enabled_ui(enabled, |ui| {
                let response = match &mut entry.control {
                    Control::Check(on) => check(ui, &name, "", on),
                    Control::Choice { items, chosen } => choice(ui, &name, items, chosen),
                    Control::Whole {
                        value,
                        minimum,
                        maximum,
                    } => whole(ui, &name, value, *minimum..=*maximum),
                    Control::Real {
                        value,
                        minimum,
                        maximum,
                    } => spin(ui, &name, value, real_range(*minimum, *maximum)),
                    Control::Line(text) => line_with(
                        ui,
                        &name,
                        text,
                        LineOptions {
                            placeholder,
                            ..LineOptions::default()
                        },
                    ),
                };
                response.on_hover_text(tip).changed()
            })
            .inner
        })
        .inner
    }

    /// Draws the form; true when any control was edited this frame.
    pub fn ui(&mut self, ui: &mut Ui) -> bool {
        let enabled: Vec<bool> = (0..self.entries.len())
            .map(|at| self.enabled(at))
            .collect();
        let mut changed = false;
        let prefix = self.prefix.clone();
        for (at, entry) in self.entries.iter_mut().enumerate() {
            if entry.base && Self::row(ui, &prefix, entry, enabled[at]) {
                changed = true;
            }
        }
        // The Advanced group is there only when it has rows, and its rows only while it is open;
        // hidden, they are still there to fill, as a hidden Qt widget is.
        let any_advanced = self.entries.iter().any(|entry| !entry.base);
        let advanced_name = self.advanced_name.clone();
        let open = &mut self.advanced_open;
        let mut header = |ui: &mut Ui| {
            check(ui, &advanced_name, "Advanced", open)
                .on_hover_text("The arguments GDAL files as Advanced, Esoteric or Common");
        };
        if any_advanced {
            header(ui);
        } else {
            hidden(ui, &format!("{advanced_name}Header"), header);
        }
        let shown = any_advanced && self.advanced_open;
        let entries = &mut self.entries;
        let mut rows = |ui: &mut Ui| {
            for (at, entry) in entries.iter_mut().enumerate() {
                if !entry.base && Self::row(ui, &prefix, entry, enabled[at]) {
                    changed = true;
                }
            }
        };
        if shown {
            ui.indent(format!("{advanced_name}Body"), |ui| rows(ui));
        } else {
            hidden(ui, &format!("{advanced_name}Body"), rows);
        }
        changed
    }
}
