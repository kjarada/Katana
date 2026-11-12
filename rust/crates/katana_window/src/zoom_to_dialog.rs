//! View > Zoom To: frame, in a view of the workspace, what a scope takes - the shared "Apply to" and
//! "Only those that match" (the ONE ScopeFilterWidget, CLAUDE.md section 1.1) - through the ZOOM
//! line it builds (docs/desktop.md, "Zoom To"; the verb is cad's view_verbs). The port of
//! zoom_to_dialog.
//!
//! The dialog frames nothing itself: it hands ZOOM <scope words> view=<id> to the window's one
//! executor, so the log shows what ran and what ZOOM answered, and the status line says it as a
//! sentence - a scope that takes nothing is ZOOM's own answer (matched=0, nothing moved).
//! Non-modal and kept, so several scopes can be tried in turn; linked views follow each ZOOM as
//! they follow a wheel.
//!
//! Object names: zoomToDialog; the scope and filter controls with the prefix zoomTo
//! (zoomToScopeSelection, zoomToScopeDrawing, zoomToFilterLayer ...); zoomToView (the active view,
//! then every open view that frames a scope - plan, 3D and elevation, view_verbs::zoom_takes - by
//! its title and id); zoomToLine (the line the controls say, updated as they change); zoomToRun;
//! zoomToStatus (what ZOOM answered, as a sentence; a refusal in the error colour); zoomToClose.
//!
//! The views are the workspace's, reached through the host's ViewAccess as the window keeps them;
//! the C++'s "nothing runs it" has no counterpart, since the host always runs a line.

use std::cell::RefCell;
use std::rc::Rc;

use egui::{RichText, Ui};
use katana_cad::view_set::{NO_VIEW, ViewId, ViewSet};
use katana_cad::view_verbs::{ZoomRequestKind, zoom_takes};
use katana_core::Result;

use crate::customisation::{ScopeFilterWidget, scope_filter_views};
use crate::drive::controls::{self, Choice};
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::theme;

/// What the dialog reads of the window.
#[derive(Default)]
pub struct ZoomToContext {
    /// Whether the scope's layer list is read from the host's drawing (the C++'s `document`,
    /// which a test may leave null: then the views alone are read).
    pub reads_drawing: bool,
    /// The views to choose among and to answer the View scope; None in a test that needs none
    /// (the view list then offers the active view alone).
    pub views: Option<Rc<RefCell<ViewSet>>>,
}

impl ZoomToContext {
    /// The window's: its drawing, and its workspace's views when it has a workspace.
    pub fn of(host: &mut dyn Host) -> Self {
        Self {
            reads_drawing: true,
            views: host.views().map(|access| access.view_set()),
        }
    }
}

/// The digits after `key=` where `key` stands as a word of its own (`\bkey=(\d+)`), first found:
/// a word character is PCRE's without Unicode properties, [A-Za-z0-9_].
fn field_digits<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let wanted = format!("{key}=");
    let mut from = 0;
    while let Some(found) = line[from..].find(&wanted) {
        let at = from + found;
        let boundary = line[..at]
            .chars()
            .next_back()
            .is_none_or(|before| !(before.is_ascii_alphanumeric() || before == '_'));
        let rest = &line[at + wanted.len()..];
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if boundary && digits > 0 {
            return Some(&rest[..digits]);
        }
        from = at + wanted.len();
    }
    None
}

/// QString::toLongLong: the number, or 0 when it does not read (as it would not past i64).
fn whole(digits: &str) -> i64 {
    digits.parse().unwrap_or(0)
}

/// View > Zoom To (see the module comment).
pub struct ZoomToDialog {
    frame: DialogFrame,
    visible: bool,
    reads_drawing: bool,
    views: Option<Rc<RefCell<ViewSet>>>,
    scope: ScopeFilterWidget,
    /// The active view (id 0), then each view that frames a scope: its id and what the choice
    /// shows.
    view_items: Vec<(ViewId, String)>,
    view: Choice,
    status: String,
    status_error: bool,
}

/// The dialog and the window, lent together to the dialog frame's body and footer.
struct Lent<'a, 'h> {
    dialog: &'a mut ZoomToDialog,
    host: &'a mut (dyn Host + 'h),
}

impl ZoomToDialog {
    pub fn new(context: ZoomToContext, host: &dyn Host) -> Self {
        let mut scope = ScopeFilterWidget::new("zoomTo");
        if let Some(open) = &context.views {
            let open = Rc::clone(open);
            scope.views = Some(Box::new(move || scope_filter_views(&open.borrow())));
        }
        let mut dialog = Self {
            frame: DialogFrame::new("zoomToDialog", "Zoom To"),
            visible: false,
            reads_drawing: context.reads_drawing,
            views: context.views,
            scope,
            view_items: Vec::new(),
            view: Choice::default(),
            status: String::new(),
            status_error: false,
        };
        dialog.refresh(host);
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

    /// The scope and filter controls.
    pub fn scope_controls(&mut self) -> &mut ScopeFilterWidget {
        &mut self.scope
    }

    /// The id of the view chosen; 0 for the active view (and for none, as invalid data reads).
    fn chosen_view(&self) -> ViewId {
        self.view
            .index
            .and_then(|at| self.view_items.get(at))
            .map_or(NO_VIEW, |(id, _)| *id)
    }

    /// Reads the layers and the open views again, keeping what is chosen: when the dialog is
    /// shown, and after the drawing or the views changed.
    pub fn refresh(&mut self, host: &dyn Host) {
        if self.reads_drawing {
            self.scope.reload(host.document().model());
        } else {
            self.scope.reload_views();
        }
        // The choice survives by the view's id; a view closed since falls back to the active
        // view.
        let chosen = self.chosen_view();
        self.view_items = vec![(NO_VIEW, String::from("Active view"))];
        if let Some(open) = &self.views {
            for state in open.borrow().views() {
                // Only the views that frame a scope (zoom_takes): ZOOM would refuse the rest by
                // their kind.
                if !zoom_takes(state.kind, ZoomRequestKind::Scope) {
                    continue;
                }
                self.view_items.push((
                    state.id,
                    format!("{} (view {})", ViewSet::title(state), state.id),
                ));
            }
        }
        let at = self
            .view_items
            .iter()
            .position(|(id, _)| *id == chosen)
            .unwrap_or(0);
        self.view = Choice::at(at);
    }

    /// The ZOOM line the controls say; fails naming what they cannot say.
    pub fn line(&self) -> Result<String> {
        let words = self.scope.verb_words()?;
        let mut text = format!("ZOOM {words}");
        // No view= for the active view, so the line reads as a person types it.
        let id = self.chosen_view();
        if id != NO_VIEW {
            text.push_str(&format!(" view={id}"));
        }
        Ok(text)
    }

    /// What Zoom does: build the line, run it, show the answer. False when the controls did not
    /// read or ZOOM refused, with the reason in the status.
    pub fn zoom(&mut self, host: &mut dyn Host) -> bool {
        let text = match self.line() {
            Ok(text) => text,
            Err(error) => {
                self.set_status(&error.describe(), true);
                return false;
            }
        };
        let outcome = host.run(&text);
        // A sentence, and a refusal in the error colour, as Online Data words its status: the raw
        // records - long reals among them - wrapped over five lines, and the dialog, grown only to
        // its least height, pressed the type grid above until its captions lost their descenders.
        // The records are in the command log the executor writes.
        let said = if outcome.ok {
            Self::spoken_reply(&outcome.reply)
        } else {
            outcome.error.clone()
        };
        self.set_status(&said, !outcome.ok);
        outcome.ok
    }

    fn set_status(&mut self, text: &str, is_error: bool) {
        self.status = String::from(text);
        self.status_error = is_error;
    }

    /// What zoomToStatus says, and whether in the error colour.
    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn status_is_error(&self) -> bool {
        self.status_error
    }

    /// ZOOM's reply to a scope as the status says it: "3 matched, framed in view 2; view 1
    /// followed.", "Nothing matched, so no view moved.", or that the view draws none of them.
    pub fn spoken_reply(reply: &str) -> String {
        // The records a ZOOM on a scope answers (view_verbs): the scope's, then the view it framed
        // - with shown=, how many of them it shows - then each view that followed it; or, when the
        // view shows none of them, that view's record with moved=no.
        let mut matched: i64 = -1;
        let mut shown: i64 = -1;
        let mut framed = String::new();
        let mut unmoved = String::new();
        let mut followed: Vec<String> = Vec::new();
        for line in reply.split('\n').filter(|line| !line.is_empty()) {
            if line.starts_with("scope=") {
                if let Some(digits) = field_digits(line, "matched") {
                    matched = whole(digits);
                }
                continue;
            }
            // ^view=(\d+)
            let Some(rest) = line.strip_prefix("view=") else {
                continue;
            };
            let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            if digits == 0 {
                continue;
            }
            let view = String::from(&rest[..digits]);
            if line.contains(" followed=") {
                followed.push(view);
                continue;
            }
            if line.contains(" moved=no") {
                unmoved = view;
            } else {
                framed = view;
            }
            if let Some(digits) = field_digits(line, "shown") {
                shown = whole(digits);
            }
        }
        if matched == 0 {
            return String::from("Nothing matched, so no view moved.");
        }
        let took = if matched > 0 {
            format!("{matched} matched")
        } else {
            String::from("ZOOM answered")
        };
        if !unmoved.is_empty() || framed.is_empty() {
            // It said "framed in view 2" of a view that showed none of it.
            return if unmoved.is_empty() {
                format!("{took}, and no view moved.")
            } else {
                format!("{took}, but view {unmoved} shows none of them: nothing moved.")
            };
        }
        let mut text = if shown >= 0 && shown < matched {
            format!("{took}, {shown} of them shown and framed in view {framed}")
        } else {
            format!("{took}, framed in view {framed}")
        };
        if let Some(last) = followed.pop() {
            let (plural, views) = if followed.is_empty() {
                ("", last)
            } else {
                ("s", format!("{} and {last}", followed.join(", ")))
            };
            text.push_str(&format!("; view{plural} {views} followed"));
        }
        text.push('.');
        text
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
        ui.set_max_width(520.0);
        self.scope.ui(ui);
        egui::Grid::new("zoomToForm").num_columns(2).show(ui, |ui| {
            ui.label("In:");
            let items: Vec<String> = self
                .view_items
                .iter()
                .map(|(_, shown)| shown.clone())
                .collect();
            controls::choice(ui, "zoomToView", &items, &mut self.view).on_hover_text(
                "The view to frame what the scope takes in; the views linked with it follow",
            );
            ui.end_row();
            ui.label("Line:");
            let line = match self.line() {
                Ok(line) => line,
                Err(error) => error.describe(),
            };
            controls::label_as(
                ui,
                "zoomToLine",
                &line,
                egui::Label::new(line.as_str()).wrap().selectable(true),
            );
            ui.end_row();
        });
    }

    /// The status and the buttons: Zoom keeps the dialog open for the next scope; Close closes it.
    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let status = self.status.clone();
        let colour = if self.status_error {
            theme::error()
        } else {
            theme::text()
        };
        controls::label_as(
            ui,
            "zoomToStatus",
            &status,
            egui::Label::new(RichText::new(status.as_str()).color(colour.to_egui()))
                .wrap()
                .selectable(true),
        );
        ui.horizontal(|ui| {
            if controls::button_as(ui, "zoomToRun", "&Zoom", egui::Button::new("Zoom")).clicked() {
                self.zoom(&mut *host);
                ui.ctx().request_repaint();
            }
            if controls::button(ui, "zoomToClose", "Close").clicked() {
                self.close();
            }
        });
    }
}
