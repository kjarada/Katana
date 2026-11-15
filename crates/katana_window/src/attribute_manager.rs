//! The attribute manager (attribute_manager.hpp): Edit > Attributes.
//!
//! A string brought in from another CAD program's archive carries a TREE of attributes - `group {
//! name "Asset" ... }` - which the importer flattens to properties keyed "Asset/Dimensions/Size",
//! and per-vertex attributes keyed "vertex/3/Name". The properties panel shows those flat keys,
//! which is honest but unreadable on a real survey string with thirty of them. This dialog puts the
//! tree back: branches for the groups, leaves for the values, with their types, and add, edit,
//! rename and remove as ordinary commands so every change is on the one undo stack.
//!
//! It acts on the SELECTION. With several entities selected an edit applies to all of them, and a
//! value they do not share is shown as such rather than pretending they agree.
//!
//! As the C++ does, it runs its commands on the drawing itself (`Document::execute`) rather than
//! through a verb line - kept as it is, and recorded under "Not done": CLAUDE.md section 1 asks a
//! dialog to build the line a person would type (PROP SET, PROP RENAME, PROP REMOVE).
//!
//! The C++ named none of its controls, and asked a name with a modal QInputDialog. Here the
//! controls carry names, so a driver reaches them - `attributeManagerDialog`, `attributeTree`,
//! `attributeValue`, `attributeType`, `attributeName` (the name Add gives and Rename renames to,
//! in place of the modal question), `attributeAdd`, `attributeSave`, `attributeRename`,
//! `attributeRemove`, `attributeClose` - names the C++ did not have.

use std::collections::{BTreeMap, BTreeSet};

use egui::{RichText, Ui};
use katana_cad::Document;
use katana_commands::CommandPtr;
use katana_commands::entity_commands::{
    remove_entity_property, rename_entity_property, set_entity_property,
};
use katana_core::error::make_error;
use katana_core::text::parse_integer;
use katana_core::{ErrorCode, Result};
use katana_entity::entity::{property_to_string, type_name};
use katana_entity::{EntityId, PropertyValue};

use crate::drive::mailbox::lists_no;
use crate::drive::{
    Choice, ControlKind, ControlRecord, LineOptions, button, choice, custom, line_with, registry,
};
use crate::host::Host;
use crate::shell::DialogFrame;

/// Values that differ across the selection are shown as this rather than as one of them: a manager
/// that showed the first entity's value and wrote it to all of them would quietly overwrite the
/// others.
const VARIES: &str = "<varies>";

const TYPES: [&str; 4] = ["text", "integer", "real", "boolean"];

/// Every property of the selection, with its value where they agree and None where they do not.
pub fn merged_attributes(document: &Document) -> BTreeMap<String, Option<PropertyValue>> {
    let selected = document.selection().ids();
    let mut all: BTreeMap<String, Option<PropertyValue>> = BTreeMap::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for &id in &selected {
        let Some(entity) = document.model().entities.find(id) else {
            continue;
        };
        for (key, value) in &entity.properties {
            *counts.entry(key.clone()).or_default() += 1;
            match all.get_mut(key) {
                None => {
                    all.insert(key.clone(), Some(value.clone()));
                }
                Some(found) => {
                    if found.as_ref().is_some_and(|first| first != value) {
                        *found = None;
                    }
                }
            }
        }
    }
    // A property only some of them carry is as much a disagreement as one they value differently.
    for (key, value) in &mut all {
        if counts.get(key).copied().unwrap_or(0) != selected.len() {
            *value = None;
        }
    }
    all
}

/// A value as the form gives it: its text read as its type.
pub fn value_from_form(text: &str, kind: &str) -> Result<PropertyValue> {
    match kind {
        "text" => Ok(PropertyValue::Text(String::from(text))),
        "integer" => parse_integer(text.trim())
            .map(PropertyValue::Integer)
            .ok_or_else(|| make_error(ErrorCode::ParseFailure, "not an integer", text)),
        "real" => text
            .trim()
            .parse::<f64>()
            .map(PropertyValue::Real)
            .map_err(|_| make_error(ErrorCode::ParseFailure, "not a number", text)),
        _ => {
            let folded = text.trim().to_lowercase();
            match folded.as_str() {
                "true" | "1" => Ok(PropertyValue::Boolean(true)),
                "false" | "0" => Ok(PropertyValue::Boolean(false)),
                _ => Err(make_error(ErrorCode::ParseFailure, "not a boolean", text)),
            }
        }
    }
}

/// One row of the tree: a branch (no key) or a leaf with its whole key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct TreeRow {
    depth: usize,
    /// The branch's path, or the leaf's key.
    path: String,
    name: String,
    value: String,
    kind: String,
    leaf: bool,
}

/// The tree the keys make. The keys are sorted (a PropertyMap is ordered), so a parent is always
/// made before its children and one pass is enough - the same reason the layer tree builds in one
/// pass.
fn tree_rows(merged: &BTreeMap<String, Option<PropertyValue>>) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    let mut branches: BTreeSet<String> = BTreeSet::new();
    for (key, value) in merged {
        let parts: Vec<&str> = key.split('/').collect();
        let mut path = String::new();
        for (depth, part) in parts.iter().enumerate().take(parts.len() - 1) {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            if branches.insert(path.clone()) {
                rows.push(TreeRow {
                    depth,
                    path: path.clone(),
                    name: String::from(*part),
                    ..TreeRow::default()
                });
            }
        }
        rows.push(TreeRow {
            depth: parts.len() - 1,
            path: key.clone(),
            name: String::from(parts[parts.len() - 1]),
            value: value
                .as_ref()
                .map_or_else(|| String::from(VARIES), property_to_string),
            kind: value
                .as_ref()
                .map(|value| String::from(type_name(value)))
                .unwrap_or_default(),
            leaf: true,
        });
    }
    rows
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AttributeState {
    selected_key: String,
    value: String,
    kind: Choice,
    name: String,
    expanded: BTreeSet<String>,
    pressed: Option<&'static str>,
}

impl Default for AttributeState {
    fn default() -> Self {
        Self {
            selected_key: String::new(),
            value: String::new(),
            kind: Choice::at(0),
            name: String::new(),
            expanded: BTreeSet::new(),
            pressed: None,
        }
    }
}

/// Edit > Attributes (see the module comment).
pub struct AttributeManagerDialog {
    frame: DialogFrame,
    state: AttributeState,
    /// Whether it is shown.
    pub open: bool,
}

impl Default for AttributeManagerDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl AttributeManagerDialog {
    pub fn new() -> Self {
        Self {
            frame: DialogFrame::new("attributeManagerDialog", "Attributes"),
            state: AttributeState::default(),
            open: true,
        }
    }

    pub fn object_name(&self) -> &str {
        self.frame.name()
    }

    /// For the headless screenshot: opens the tree so a grab shows the attributes rather than a
    /// row of collapsed branches, the first row chosen.
    pub fn expand_all(&mut self, document: &Document) {
        let rows = tree_rows(&merged_attributes(document));
        for row in &rows {
            if !row.leaf {
                self.state.expanded.insert(row.path.clone());
            }
        }
        if let Some(first) = rows.first() {
            self.choose(document, first.leaf.then(|| first.path.clone()));
        }
    }

    /// A leaf chosen (or a branch, `None`): the form shows its value and type, or that it varies.
    fn choose(&mut self, document: &Document, key: Option<String>) {
        let Some(key) = key else {
            self.state.selected_key.clear();
            return;
        };
        let all = merged_attributes(document);
        self.state.selected_key = key.clone();
        match all.get(&key) {
            Some(Some(value)) => {
                self.state.value = property_to_string(value);
                let kind = type_name(value);
                self.state.kind = Choice::at(TYPES.iter().position(|t| *t == kind).unwrap_or(0));
            }
            _ => self.state.value = String::from(VARIES),
        }
    }

    /// The full key of the chosen leaf ("Asset/Dimensions/Size"), empty when a branch or nothing is
    /// chosen: a branch is not a property, it is the part of a name before a slash.
    pub fn selected_key(&self) -> &str {
        &self.state.selected_key
    }

    fn form_value(&self) -> Result<PropertyValue> {
        let types: Vec<String> = TYPES.iter().map(|t| String::from(*t)).collect();
        value_from_form(&self.state.value, &self.state.kind.current_text(&types))
    }

    fn execute(&self, host: &mut dyn Host, command: CommandPtr, done: &str) -> bool {
        match host.document_mut().execute(command) {
            Ok(()) => {
                host.log(done, false);
                true
            }
            Err(error) => {
                host.log(&error.describe(), true);
                false
            }
        }
    }

    /// Add: an attribute named by the name field, set on every selected entity; use / to nest it
    /// in a group.
    pub fn add(&mut self, host: &mut dyn Host) {
        let ids = host.document().selection().ids();
        if ids.is_empty() {
            host.log("Select the entities to add an attribute to first.", true);
            return;
        }
        let key = String::from(self.state.name.trim());
        if key.is_empty() {
            return;
        }
        let value = match self.form_value() {
            Ok(value) => value,
            Err(error) => {
                host.log(&error.describe(), true);
                return;
            }
        };
        let done = format!("Attribute {key} set on {} entities.", ids.len());
        self.execute(host, set_entity_property(ids, key, value), &done);
    }

    /// Save Value: the form's value set on the chosen attribute of every selected entity.
    pub fn save(&mut self, host: &mut dyn Host) {
        let key = self.state.selected_key.clone();
        if key.is_empty() {
            return;
        }
        let value = match self.form_value() {
            Ok(value) => value,
            Err(error) => {
                host.log(&error.describe(), true);
                return;
            }
        };
        let ids: Vec<EntityId> = host.document().selection().ids();
        let done = format!("Attribute {key} set on {} entities.", ids.len());
        self.execute(host, set_entity_property(ids, key, value), &done);
    }

    /// Rename: the chosen attribute renamed to the name field, on every selected entity, keeping
    /// its value and type.
    pub fn rename(&mut self, host: &mut dyn Host) {
        let key = self.state.selected_key.clone();
        let to = String::from(self.state.name.trim());
        if key.is_empty() || to.is_empty() {
            return;
        }
        let ids = host.document().selection().ids();
        let done = format!("Attribute {key} renamed to {to}.");
        if self.execute(host, rename_entity_property(ids, key, to), &done) {
            self.state.selected_key.clear();
        }
    }

    /// Remove: the chosen attribute removed from every selected entity.
    pub fn remove(&mut self, host: &mut dyn Host) {
        let key = self.state.selected_key.clone();
        if key.is_empty() {
            return;
        }
        let ids = host.document().selection().ids();
        let done = format!("Attribute {key} removed from {} entities.", ids.len());
        if self.execute(host, remove_entity_property(ids, key), &done) {
            self.state.selected_key.clear();
        }
    }

    /// Fills the form, as a person types: the value, its type, and the name Add and Rename use.
    pub fn set_form(&mut self, value: &str, kind: &str, name: &str) {
        self.state.value = String::from(value);
        self.state.kind = Choice::at(TYPES.iter().position(|t| *t == kind).unwrap_or(0));
        self.state.name = String::from(name);
    }

    /// Draws the dialog while it is open, over the host's drawing.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.open {
            return;
        }
        let ctx = ui.ctx().clone();
        let merged = merged_attributes(host.document());
        let rows = tree_rows(&merged);
        let selected = host.document().selection().size();
        let subject = if selected == 0 {
            String::from(
                "Nothing is selected: select entities in the drawing to see their attributes.",
            )
        } else {
            format!("{selected} entities selected.")
        };
        let mut chosen: Option<Option<String>> = None;
        let mut closed = false;
        let mut parts = (&mut self.state, &mut chosen, &mut closed);
        let shown = self.frame.show(
            &ctx,
            &mut parts,
            |ui, parts| {
                let (state, chosen, _) = parts;
                ui.label(subject.as_str());
                tree_ui(ui, state, &rows, chosen);
                ui.horizontal(|ui| {
                    ui.label("Value");
                    line_with(
                        ui,
                        "attributeValue",
                        &mut state.value,
                        LineOptions::default(),
                    );
                });
                let types: Vec<String> = TYPES.iter().map(|t| String::from(*t)).collect();
                ui.horizontal(|ui| {
                    ui.label("Type");
                    choice(ui, "attributeType", &types, &mut state.kind);
                });
                ui.horizontal(|ui| {
                    ui.label("Name");
                    line_with(
                        ui,
                        "attributeName",
                        &mut state.name,
                        LineOptions {
                            placeholder: "use / to nest, e.g. Asset/Dimensions/Size",
                            ..LineOptions::default()
                        },
                    );
                });
                ui.horizontal(|ui| {
                    if button(ui, "attributeAdd", "Add")
                        .on_hover_text(
                            "Add an attribute to every selected entity; use / to nest it in a \
                             group",
                        )
                        .clicked()
                    {
                        state.pressed = Some("add");
                    }
                    if button(ui, "attributeSave", "Save Value").clicked() {
                        state.pressed = Some("save");
                    }
                    if button(ui, "attributeRename", "Rename")
                        .on_hover_text(
                            "Rename it on every selected entity, keeping its value and type",
                        )
                        .clicked()
                    {
                        state.pressed = Some("rename");
                    }
                    if button(ui, "attributeRemove", "Remove").clicked() {
                        state.pressed = Some("remove");
                    }
                });
            },
            |ui, parts| {
                if button(ui, "attributeClose", "Close").clicked() {
                    *parts.2 = true;
                }
            },
        );
        if let Some(key) = chosen {
            self.choose(host.document(), key);
        }
        match self.state.pressed.take() {
            Some("add") => self.add(host),
            Some("save") => self.save(host),
            Some("rename") => self.rename(host),
            Some("remove") => self.remove(host),
            _ => {}
        }
        if closed || shown.closed {
            self.open = false;
        }
    }
}

/// The tree drawn: a branch opens and closes, a leaf is chosen. Recorded as `attributeTree`, its
/// rows shown "Attribute | Value | Type"; a fill chooses the row whose name is the text given.
fn tree_ui(
    ui: &mut Ui,
    state: &mut AttributeState,
    rows: &[TreeRow],
    chosen: &mut Option<Option<String>>,
) {
    let taken = registry::take(ui, "attributeTree");
    let mut shown = Vec::new();
    let mut hidden_below: Option<usize> = None;
    for row in rows {
        if let Some(depth) = hidden_below {
            if row.depth > depth {
                continue;
            }
            hidden_below = None;
        }
        shown.push(row);
        if !row.leaf && !state.expanded.contains(&row.path) {
            hidden_below = Some(row.depth);
        }
    }
    if let Some(taken) = &taken {
        let answer = match &taken.posted.act {
            crate::drive::Act::Fill(text) => match shown.iter().find(|row| row.name == *text) {
                Some(row) => {
                    *chosen = Some(row.leaf.then(|| row.path.clone()));
                    Ok(())
                }
                None => Err(lists_no("attributeTree", text)),
            },
            _ => Err(crate::drive::mailbox::no_button(&taken.posted)),
        };
        registry::settle(ui, taken, answer);
    }
    let response = egui::ScrollArea::vertical()
        .id_salt("attributeTreeScroll")
        .max_height(320.0)
        .show(ui, |ui| {
            for row in &shown {
                ui.horizontal(|ui| {
                    ui.add_space(14.0 * row.depth as f32);
                    if row.leaf {
                        let picked = state.selected_key == row.path;
                        if ui.selectable_label(picked, row.name.as_str()).clicked() {
                            *chosen = Some(Some(row.path.clone()));
                        }
                        ui.label(row.value.as_str());
                        ui.label(RichText::new(row.kind.as_str()).weak());
                    } else {
                        let open = state.expanded.contains(&row.path);
                        let arrow = if open { "\u{25be}" } else { "\u{25b8}" };
                        if ui
                            .selectable_label(false, format!("{arrow} {}", row.name))
                            .clicked()
                        {
                            if open {
                                state.expanded.remove(&row.path);
                            } else {
                                state.expanded.insert(row.path.clone());
                            }
                            *chosen = Some(None);
                        }
                    }
                });
            }
        });
    let area = ui.interact(
        response.inner_rect,
        response.id.with("rows"),
        egui::Sense::hover(),
    );
    let mut record = ControlRecord::new("attributeTree", ControlKind::Rows);
    record.items = shown
        .iter()
        .map(|row| format!("{} | {} | {}", row.name, row.value, row.kind))
        .collect();
    record.value = state.selected_key.clone();
    custom(ui, &area, record);
}
