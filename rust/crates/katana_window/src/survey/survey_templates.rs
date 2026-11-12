//! Saved delimited-file layouts, shared by the import wizard and the point export
//! (survey_templates.hpp/.cpp).
//!
//! A template is surveyio's own text form of a DelimitedLayout (layout_template /
//! parse_layout_template), stored under a name a person gives it, in the person's settings under
//! survey/templates - per user, not per drawing, because a template describes the files one
//! instrument or one office writes, which outlive any one drawing. The text is stored, never a
//! half-parsed layout, so a template saved by a newer Katana that this one cannot read is refused
//! by parse_layout_template when it is chosen, not read as something close to it.
//!
//! The settings are the host's (host::UserSettings, in QSettings' place; the plan's section
//! 4.11), under the C++'s key. The C++ kept every list of templates current by refilling each one
//! after a save or a delete (keepTemplateChoiceCurrent), and again when its dialog was shown: in
//! egui a list is drawn from the settings at every frame, which is both at once - the import
//! wizard and the Export dialog, both kept and both non-modal, offer a template the moment the
//! other saves it, and stop offering one the moment it is deleted.

use egui::{Response, Ui};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Status};
use katana_surveyio::parse_layout_template;

use crate::drive::{self, Choice};
use crate::host::UserSettings;

/// The settings' group the templates are kept in: the C++'s QSettings group.
pub const TEMPLATE_GROUP: &str = "survey/templates";

/// The first item of a list of templates: none chosen.
pub const NO_TEMPLATE: &str = "(saved templates)";

fn key_of(name: &str) -> String {
    format!("{TEMPLATE_GROUP}/{name}")
}

/// Name and template text, sorted by name (the settings keep their keys in order).
pub fn saved_layout_templates(settings: &UserSettings) -> Vec<(String, String)> {
    settings
        .child_keys(TEMPLATE_GROUP)
        .into_iter()
        .map(|name| {
            let text = settings
                .text(&key_of(&name))
                .map(String::from)
                .unwrap_or_default();
            (name, text)
        })
        .collect()
}

/// InvalidArgument for an empty name or one holding '/' or '\' (the settings read those as
/// groups); the template text is checked by parse_layout_template first, so nothing unreadable is
/// ever saved.
pub fn save_layout_template(settings: &mut UserSettings, name: &str, text: &str) -> Status {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.contains(['/', '\\']) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a template needs a name, without '/' or '\\'",
            name,
        ));
    }
    parse_layout_template(text)?;
    settings.set_text(&key_of(trimmed), text);
    if settings.save().is_err() {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the template could not be saved in the settings",
            trimmed,
        ));
    }
    Ok(())
}

/// NotFound when there is no template of that name.
pub fn delete_layout_template(settings: &mut UserSettings, name: &str) -> Status {
    if !settings.remove(&key_of(name)) {
        return Err(make_error(
            ErrorCode::NotFound,
            "no saved template of that name",
            name,
        ));
    }
    // As the C++'s sync, whose outcome it did not read.
    let _ = settings.save();
    Ok(())
}

/// A list of the saved templates (the C++'s QComboBox filled by fillTemplateChoice): "(saved
/// templates)" and then every saved name, the current name kept while it is still there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TemplateChoice {
    chosen: Option<String>,
}

impl TemplateChoice {
    pub fn new() -> Self {
        Self::default()
    }

    /// The template chosen; None for "(saved templates)".
    pub fn current(&self) -> Option<&str> {
        self.chosen.as_deref()
    }

    /// Chooses `name` without applying it, as the C++ chose a template just saved with the box's
    /// signals blocked.
    pub fn set_current(&mut self, name: Option<&str>) {
        self.chosen = name.map(String::from);
    }

    /// Draws the list over `templates` (saved_layout_templates, read for this frame) under the
    /// object name `name`. Answers the text of a template the person has just chosen - a change
    /// to an item past the first, as the C++'s currentIndexChanged with an index above 0 - for
    /// the caller to apply.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        name: &str,
        templates: &[(String, String)],
    ) -> (Response, Option<String>) {
        let mut items = Vec::with_capacity(templates.len() + 1);
        items.push(String::from(NO_TEMPLATE));
        items.extend(templates.iter().map(|(saved, _)| saved.clone()));
        // The current name kept when it is still there; otherwise the first item, silently, as
        // a refill with the signals blocked did.
        let index = self
            .chosen
            .as_ref()
            .and_then(|chosen| templates.iter().position(|(saved, _)| saved == chosen))
            .map_or(0, |at| at + 1);
        if index == 0 {
            self.chosen = None;
        }
        let mut chosen = Choice::at(index);
        let response = drive::choice(ui, name, &items, &mut chosen).on_hover_text(
            templates
                .get(index.saturating_sub(1))
                .filter(|_| index > 0)
                .map_or("A layout saved earlier", |(_, text)| text.as_str()),
        );
        let mut applied = None;
        if response.changed() {
            match chosen.index {
                Some(at) if at > 0 && at <= templates.len() => {
                    let (saved, text) = &templates[at - 1];
                    self.chosen = Some(saved.clone());
                    applied = Some(text.clone());
                }
                _ => self.chosen = None,
            }
        }
        (response, applied)
    }
}
