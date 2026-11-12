//! View > Paper Style Scale: a non-modal command builder. The view menu, the toolbar and each plan
//! view's bar all open this dialog; only the shared VIEWS SCALE command edits state - the dialog
//! builds "VIEWS SCALE <id> <N|AUTO>" and hands it to the window's one executor, and reads the view
//! set it is given, never writing it. The port of view_scale_dialog.
//!
//! Object names: viewScaleDialog; viewScaleExplanation; viewScaleView (the open plan views, by
//! title and id) with viewScaleViewLabel; viewScaleMode (Auto or Fixed 1:N) with
//! viewScaleModeLabel; viewScaleDenominator with viewScaleDenominatorLabel; viewScaleLine (the line
//! Apply runs, or why there is none) with viewScaleLineLabel; viewScaleStatus; viewScaleButtons,
//! viewScaleApply and viewScaleClose.
//!
//! The view set is the workspace's, shared as the window keeps it (`ViewAccess::view_set`); a view
//! closed while the dialog is open is refused by name, never another view edited in its place.
//! The C++'s "No command executor is available." has no counterpart: the host always runs a line.

use std::cell::RefCell;
use std::rc::Rc;

use egui::{RichText, Ui};
use katana_cad::view_set::{NO_VIEW, ViewId, ViewSet, parse_paper_style_scale};
use katana_cad::viewport_layout::ViewKind;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};

use crate::drive::controls::{self, Choice, LineOptions};
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::theme;

/// The sizing modes, in the order of the choice: Auto keeps the marks' screen size.
const MODES: [&str; 2] = ["Auto (fixed screen size)", "Fixed 1:N"];

/// View > Paper Style Scale (see the module comment).
pub struct ViewScaleDialog {
    frame: DialogFrame,
    visible: bool,
    views: Rc<RefCell<ViewSet>>,
    /// The open plan views: each one's id and the text the choice shows.
    view_items: Vec<(ViewId, String)>,
    view: Choice,
    mode: Choice,
    denominator: String,
    status: String,
    status_error: bool,
}

/// The dialog and the window, lent together to the dialog frame's body and footer.
struct Lent<'a, 'h> {
    dialog: &'a mut ViewScaleDialog,
    host: &'a mut (dyn Host + 'h),
}

impl ViewScaleDialog {
    /// The dialog over the workspace's `views`, on plan view `view`.
    pub fn new(views: Rc<RefCell<ViewSet>>, view: ViewId) -> Self {
        let mut dialog = Self {
            frame: DialogFrame::new("viewScaleDialog", "Paper Style Scale"),
            visible: false,
            views,
            view_items: Vec::new(),
            view: Choice::default(),
            mode: Choice::at(0),
            denominator: String::new(),
            status: String::new(),
            status_error: false,
        };
        dialog.refresh(view);
        dialog
    }

    pub fn is_modal(&self) -> bool {
        self.frame.is_modal()
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn show(&mut self) {
        self.visible = true;
    }

    /// QDialog::close.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// The id of the view chosen; NO_VIEW for none (an empty list's invalid current data).
    fn chosen_view(&self) -> ViewId {
        self.view
            .index
            .and_then(|at| self.view_items.get(at))
            .map_or(NO_VIEW, |(id, _)| *id)
    }

    /// The open plan views read again, `view` chosen - the one chosen now for NO_VIEW - and its
    /// scale shown.
    pub fn refresh(&mut self, view: ViewId) {
        let view = if view == NO_VIEW {
            self.chosen_view()
        } else {
            view
        };
        let open = self.views.borrow();
        self.view_items = open
            .views()
            .into_iter()
            .filter(|state| state.kind == ViewKind::Plan)
            .map(|state| {
                (
                    state.id,
                    format!("{} (view {})", ViewSet::title(state), state.id),
                )
            })
            .collect();
        drop(open);
        self.view.index = self.view_items.iter().position(|(id, _)| *id == view);
        self.read_view();
    }

    /// The chosen view's scale into the mode and the denominator.
    fn read_view(&mut self) {
        let scale = self
            .views
            .borrow()
            .find(self.chosen_view())
            .and_then(|state| state.paper_style_scale);
        self.mode = Choice::at(usize::from(scale.is_some()));
        self.denominator = scale.map(format_exact_real).unwrap_or_default();
    }

    /// The VIEWS SCALE line the controls say. NotFound when the chosen view is no open plan view;
    /// the scale's own refusal for a denominator that is not one.
    pub fn line(&self) -> Result<String> {
        let id = self.chosen_view();
        let is_plan = self
            .views
            .borrow()
            .find(id)
            .is_some_and(|state| state.kind == ViewKind::Plan);
        if !is_plan {
            return Err(Error::new(
                ErrorCode::NotFound,
                "choose an open plan view; VIEWS lists them",
            ));
        }
        let value = if self.mode.index == Some(0) {
            "AUTO"
        } else {
            self.denominator.trim()
        };
        let parsed = parse_paper_style_scale(value)?;
        Ok(format!(
            "VIEWS SCALE {id} {}",
            parsed.map_or_else(|| String::from("AUTO"), format_exact_real)
        ))
    }

    fn set_status(&mut self, text: &str, error: bool) {
        self.status = String::from(text);
        self.status_error = error;
    }

    /// What viewScaleStatus says, and whether in the error colour.
    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn status_is_error(&self) -> bool {
        self.status_error
    }

    /// Apply: the line handed to the window's executor, its reply or refusal in the status.
    pub fn apply(&mut self, host: &mut dyn Host) -> bool {
        let command = match self.line() {
            Ok(command) => command,
            Err(error) => {
                self.set_status(&error.describe(), true);
                return false;
            }
        };
        let outcome = host.run(&command);
        let said = if outcome.ok {
            outcome.reply.clone()
        } else {
            outcome.error.clone()
        };
        self.set_status(&said, !outcome.ok);
        outcome.ok
    }

    /// One frame of the dialog, when it is shown.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.visible {
            return;
        }
        let ctx = ui.ctx().clone();
        let mut frame = self.frame.clone();
        let mut lent = Lent {
            dialog: &mut *self,
            host: &mut *host,
        };
        let shown = frame.show(
            &ctx,
            &mut lent,
            |ui, lent| lent.dialog.body(ui),
            |ui, lent| lent.dialog.footer(ui, &mut *lent.host),
        );
        self.frame = frame;
        if shown.closed {
            self.close();
        }
    }

    fn body(&mut self, ui: &mut Ui) {
        ui.set_max_width(460.0);
        let explanation = "Size paper-based library linestyles and symbols in this plan view.\n\
                           Auto keeps their screen size. Fixed 1:N uses N/1000 metres per paper \
                           millimetre; larger N makes the marks larger. Camera zoom, ground-sized \
                           marks, annotation and plots are unchanged.";
        controls::label_as(
            ui,
            "viewScaleExplanation",
            explanation,
            egui::Label::new(explanation).wrap(),
        );
        egui::Grid::new("viewScaleForm")
            .num_columns(2)
            .show(ui, |ui| {
                caption(ui, "viewScaleViewLabel", "&Plan view:");
                let items: Vec<String> = self
                    .view_items
                    .iter()
                    .map(|(_, shown)| shown.clone())
                    .collect();
                if controls::choice(ui, "viewScaleView", &items, &mut self.view).changed() {
                    self.read_view();
                }
                ui.end_row();
                caption(ui, "viewScaleModeLabel", "&Sizing:");
                let modes: Vec<String> = MODES.iter().map(|mode| String::from(*mode)).collect();
                controls::choice(ui, "viewScaleMode", &modes, &mut self.mode);
                ui.end_row();
                caption(ui, "viewScaleDenominatorLabel", "&Denominator N:");
                ui.add_enabled_ui(self.mode.index == Some(1), |ui| {
                    controls::line_with(
                        ui,
                        "viewScaleDenominator",
                        &mut self.denominator,
                        LineOptions {
                            placeholder: "For example, 500 or 1:500",
                            ..LineOptions::default()
                        },
                    )
                    .on_hover_text("Positive finite N in 1:N; this is not the camera zoom");
                });
                ui.end_row();
                caption(ui, "viewScaleLineLabel", "Command:");
                let line = match self.line() {
                    Ok(line) => line,
                    Err(error) => error.message,
                };
                controls::label_as(
                    ui,
                    "viewScaleLine",
                    &line,
                    egui::Label::new(line.as_str()).wrap().selectable(true),
                );
                ui.end_row();
            });
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let status = self.status.clone();
        let colour = if self.status_error {
            theme::error()
        } else {
            theme::text()
        };
        controls::label_as(
            ui,
            "viewScaleStatus",
            &status,
            egui::Label::new(RichText::new(status.as_str()).color(colour.to_egui()))
                .wrap()
                .selectable(true),
        );
        let buttons = ui.horizontal(|ui| {
            if controls::button_as(ui, "viewScaleApply", "&Apply", egui::Button::new("Apply"))
                .clicked()
            {
                self.apply(&mut *host);
                ui.ctx().request_repaint();
            }
            if controls::button(ui, "viewScaleClose", "Close").clicked() {
                self.close();
            }
        });
        controls::custom(
            ui,
            &buttons.response,
            ControlRecord::new("viewScaleButtons", ControlKind::Other),
        );
    }
}

/// A form's caption, named, its text as Qt holds it ('&' before the letter Alt reaches its field
/// by) and shown without the '&'.
fn caption(ui: &mut Ui, name: &str, text: &str) {
    let shown = text.replace('&', "");
    controls::label_as(ui, name, text, egui::Label::new(shown));
}
