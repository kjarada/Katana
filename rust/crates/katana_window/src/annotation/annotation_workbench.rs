//! The annotation front end (docs/annotation.md, "In the window"): in the Format menu Text
//! Styles..., Label Styles and Rules... and Dimension Styles..., and the plan view's annotation
//! scale on the Format toolbar; in the Annotate menu, after its tools, Edit Text..., Edit
//! Label..., Label Layout Report... and the Leaders manager (Leaders..., Leaders for Selection...,
//! Arrange Leaders and Balloons...). The port of annotation/annotation_workbench.
//!
//! No dialog here changes the drawing itself: each builds the line a person would type -
//! TEXTSTYLE, LABELSTYLE, AUTOLABEL, DIMSTYLE, TEXTEDIT, LABEL SET, LABEL LAYOUT - and runs it
//! through the window's one executor (host::Host::run), and the scale box sets the scale as
//! ANNOSCALE does, by Document::set_annotation_scale - each ONE undoable step, so nothing here
//! decides anything a script or an agent could not do the same way. Stable object names
//! throughout, for the headless driver and the tests.
//!
//! THE RUST SHAPE. The C++ workbench was handed the Format menu and toolbar to fill, and later the
//! Annotate menu, whose tools the window had put in it first, to append to. Here it delivers rows
//! (shell::Workbench): the Format menu's section, and the Format toolbar's scale box as a widget
//! the window draws through `scale_box_ui`. The Annotate menu's entries are made by `add_*` from
//! the window's Annotate menu as it stands - the letters its tools have taken - and delivered under
//! that menu's name for the window to append after them, in the order they were added. The C++'s
//! late runner (a dialog made before the window's command line ran its lines through whatever
//! runner the window set later) has no counterpart: the host each call is lent IS the executor at
//! that moment.

use std::rc::Rc;

use egui::Ui;
use katana_cad::plot::STANDARD_SCALES;
use katana_cad::{Document, DocumentChange};
use katana_core::text::{format_exact_real, parse_finite_double};
use katana_entity::Geometry;

use super::annotation_managers::{LabelStyleManagerDialog, TextStyleManagerDialog};
use super::dialog_support::Followed;
use super::dimension_style_manager::DimensionStyleManagerDialog;
use super::label_edit_dialog::LabelEditDialog;
use super::label_layout_report::LabelLayoutReportDialog;
use super::leader_manager::{LeaderManagerContext, LeaderManagerDialog, LeaderViews, Tab};
use super::text_edit_dialog::TextEditDialog;
use crate::customisation::scope_filter_widget::scope_filter_views;
use crate::drive::action::{Action, Menu, MenuItem, ToolBar, ToolBarItem};
use crate::drive::controls::{self, Choice};
use crate::host::Host;
use crate::shell::workbench::{DockSpec, Workbench};
use crate::tools::tool_menus::with_mnemonic;

/// The Format menu, as the main window names it.
pub const FORMAT_MENU: &str = "formatMenu";
pub const FORMAT_MENU_TITLE: &str = "F&ormat";
/// The Annotate menu, as the main window names it.
pub const ANNOTATE_MENU: &str = "annotateMenu";
pub const ANNOTATE_MENU_TITLE: &str = "&Annotate";
/// The Format toolbar, as the main window names it.
pub const FORMAT_TOOL_BAR: &str = "FormatToolBar";
pub const FORMAT_TOOL_BAR_TITLE: &str = "Format";
/// The Format toolbar's scale box.
pub const SCALE_BOX: &str = "annotationScaleCombo";

pub const TEXT_STYLES_ACTION: &str = "formatTextStyles";
pub const LABEL_STYLES_ACTION: &str = "formatLabelStyles";
pub const DIMENSION_STYLES_ACTION: &str = "formatDimensionStyles";
pub const EDIT_TEXT_ACTION: &str = "annotateEditText";
pub const EDIT_LABEL_ACTION: &str = "annotateEditLabel";
pub const LABEL_LAYOUT_ACTION: &str = "annotateLabelLayout";
pub const LEADERS_ACTION: &str = "annotateLeaders";
pub const LEADERS_FOR_SELECTION_ACTION: &str = "annotateLeadersForSelection";
pub const ARRANGE_LEADERS_ACTION: &str = "annotateArrangeLeaders";

/// "1:500" for 500.
fn scale_text(scale: f64) -> String {
    format!("1:{}", format_exact_real(scale))
}

/// The letters `items` and the menu's own items have taken (with_mnemonic adds an item's own
/// letter to `taken`), so each entry added reaches one item
/// (qt_every_shortcut_and_menu_letter_reaches_one_thing_headless).
fn letters_taken(annotate: &Menu, added: &[MenuItem]) -> String {
    let mut taken = String::new();
    for action in annotate.actions() {
        let _ = with_mnemonic(&action.text, &mut taken);
    }
    for item in added {
        if let MenuItem::Action(action) = item {
            let _ = with_mnemonic(&action.text, &mut taken);
        }
    }
    taken
}

/// One dialog the workbench keeps, made the first time it is asked for, and whether it is shown.
struct Kept<D> {
    dialog: Option<D>,
    open: bool,
}

impl<D> Default for Kept<D> {
    fn default() -> Self {
        Self {
            dialog: None,
            open: false,
        }
    }
}

impl<D> Kept<D> {
    /// The dialog, made by `make` the first time, and shown.
    fn shown(&mut self, make: impl FnOnce() -> D) -> &mut D {
        self.open = true;
        self.dialog.get_or_insert_with(make)
    }

    /// Draws it when it is shown; a close hides it, and it is kept for the next time.
    fn ui(&mut self, draw: impl FnOnce(&mut D) -> bool) {
        if self.open
            && let Some(dialog) = &mut self.dialog
        {
            self.open = draw(dialog);
        }
    }
}

/// See the module comment.
pub struct AnnotationWorkbench {
    followed: Followed,
    scale_items: Vec<String>,
    scale: Choice,
    text_styles: Kept<TextStyleManagerDialog>,
    label_styles: Kept<LabelStyleManagerDialog>,
    dimension_styles: Kept<DimensionStyleManagerDialog>,
    text_edit: Kept<TextEditDialog>,
    edit_label: Kept<LabelEditDialog>,
    label_layout: Kept<LabelLayoutReportDialog>,
    leaders: Kept<LeaderManagerDialog>,
    /// The Annotate menu's entries added (add_edit_text_action, add_label_actions,
    /// add_leader_actions), in the order added.
    annotate_items: Vec<MenuItem>,
}

impl AnnotationWorkbench {
    /// The workbench over `document`: its scale box shows the document's scale.
    pub fn new(document: &Document) -> Self {
        let scale_items: Vec<String> = STANDARD_SCALES.iter().map(|&scale| scale_text(scale)).collect();
        let mut workbench = Self {
            followed: Followed::new(document),
            scale_items,
            // An editable box of the standard scales: a scale nobody listed can be typed.
            scale: Choice::editable(""),
            text_styles: Kept::default(),
            label_styles: Kept::default(),
            dimension_styles: Kept::default(),
            text_edit: Kept::default(),
            edit_label: Kept::default(),
            label_layout: Kept::default(),
            leaders: Kept::default(),
            annotate_items: Vec::new(),
        };
        workbench.show_scale(document);
        workbench
    }

    // ---- the Format menu and toolbar ----------------------------------------------------------

    /// Format > Text Styles... Menu letters T, E and I: the Format menu's others are L, Y, B, S,
    /// D, R, G and P, and a letter that reaches two items reaches neither.
    pub fn text_styles_action() -> Action {
        Action::new(TEXT_STYLES_ACTION, "&Text Styles...")
            .icon("TextStyles")
            .tip(
                "The drawing's text styles: face, height on paper, width, slant, colour, \
                 background mask, readability (TEXTSTYLE)",
            )
            .opens("textStyleManagerDialog")
    }

    pub fn label_styles_action() -> Action {
        Action::new(LABEL_STYLES_ACTION, "Lab&el Styles and Rules...")
            .icon("LabelStyles")
            .tip(
                "Label styles - what a label says and where it goes - and the rules that label \
                 the drawing by layer, code and kind (LABELSTYLE, AUTOLABEL)",
            )
            .opens("labelStyleManagerDialog")
    }

    pub fn dimension_styles_action() -> Action {
        Action::new(DIMENSION_STYLES_ACTION, "D&imension Styles...")
            .icon("DimensionStyles")
            .tip(
                "The drawing's dimension styles: text, arrows, extension lines, units and paper \
                 sizing, and the layers that use each (DIMSTYLE)",
            )
            .opens("dimensionStyleManagerDialog")
    }

    /// The Format menu's Annotation Styles section.
    pub fn format_menu(&self) -> Menu {
        Menu::new(FORMAT_MENU, FORMAT_MENU_TITLE)
            .section("Annotation Styles")
            .action(Self::text_styles_action())
            .action(Self::label_styles_action())
            .action(Self::dimension_styles_action())
    }

    /// The Format toolbar's scale box, which the window draws with `scale_box_ui`.
    pub fn format_tool_bar(&self) -> ToolBar {
        let mut bar = ToolBar::new(FORMAT_TOOL_BAR, FORMAT_TOOL_BAR_TITLE);
        bar.items.push(ToolBarItem::Widget(String::from(SCALE_BOX)));
        bar
    }

    /// The scale box's text now (QComboBox::currentText).
    pub fn scale_text(&self) -> String {
        self.scale.current_text(&self.scale_items)
    }

    /// The box shows the document's scale; looked at whenever the document changes, since an undo
    /// or an ANNOSCALE typed changes it too.
    fn show_scale(&mut self, document: &Document) {
        let text = scale_text(document.annotation_scale());
        if self.scale_text() != text {
            match self.scale_items.iter().position(|item| *item == text) {
                Some(at) => self.scale.choose(at, &self.scale_items),
                None => {
                    self.scale.index = None;
                    self.scale.text = text;
                }
            }
        }
    }

    /// What the drawing did since the workbench last looked: the scale box follows an undo, a
    /// drawing replaced and the metadata.
    fn look(&mut self, document: &Document) {
        if self.followed.take().has(
            DocumentChange::HISTORY | DocumentChange::REPLACED | DocumentChange::METADATA,
        ) {
            self.show_scale(document);
        }
    }

    /// The box's text taken as the annotation scale, "1:500" or "500"; refused - a text that is not
    /// a positive number, or a scale the document refuses - the box shows what the document still
    /// has.
    pub fn commit_scale(&mut self, document: &mut Document) {
        let typed = self.scale_text();
        let mut text = typed.trim();
        if let Some(rest) = text.strip_prefix("1:") {
            text = rest;
        }
        let set = parse_finite_double(text)
            .filter(|value| *value > 0.0)
            .is_some_and(|value| document.set_annotation_scale(value).is_ok());
        if !set {
            self.show_scale(document); // refused: show what the document still has
        }
        self.look(document);
    }

    /// The Format toolbar's scale box ("annotationScaleCombo"): an editable box of the standard
    /// scales, where every drafter looks for the plan view's annotation scale. A scale chosen from
    /// the list, filled by a driver, or typed and left is set (QComboBox::activated and
    /// QLineEdit::editingFinished).
    pub fn scale_box_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) -> egui::Response {
        self.look(host.document());
        let response = controls::choice(ui, SCALE_BOX, &self.scale_items, &mut self.scale)
            .on_hover_text(
                "Annotation scale: text, labels, leaders and paper-sized dimensions are drawn at \
                 their paper size for this scale in the plan view (ANNOSCALE)",
            );
        if (response.changed() && !response.has_focus()) || response.lost_focus() {
            self.commit_scale(host.document_mut());
            ui.ctx().request_repaint();
        }
        response
    }

    // ---- the Annotate menu --------------------------------------------------------------------

    /// Annotate > Edit Text... ("annotateEditText", its data the dialog's name "textEditDialog"),
    /// put at the end of `annotate`, the window's Annotate menu with its tools. Menu letter X: the
    /// tools' letters are given as the catalogue fills the menu, the first letters of words first,
    /// and X begins none of their names. But a tool short of letters is moved onto any letter of
    /// its name, so the X is taken only while it is free, and a free letter otherwise: two items
    /// on one letter is a key that cycles between them.
    pub fn add_edit_text_action(&mut self, annotate: &Menu) -> Action {
        let mut taken = letters_taken(annotate, &self.annotate_items);
        let text = if taken.contains('X') {
            with_mnemonic("Edit Text...", &mut taken)
        } else {
            String::from("Edit Te&xt...")
        };
        let action = Action::new(EDIT_TEXT_ACTION, &text)
            .icon("EditText")
            .tip(
                "Edit the selected text: its words over several lines, style, height on paper, \
                 justification, rotation and position (TEXTEDIT)",
            )
            .opens("textEditDialog");
        self.annotate_items
            .push(MenuItem::Section(String::from("Edit Annotation")));
        self.annotate_items.push(MenuItem::Action(action.clone()));
        action
    }

    /// Annotate > Edit Label... (annotateEditLabel) and Label Layout Report...
    /// (annotateLabelLayout), put after the menu's tools, each with a menu letter the tools have
    /// left free.
    pub fn add_label_actions(&mut self, annotate: &Menu) {
        let mut taken = letters_taken(annotate, &self.annotate_items);
        let make = |taken: &mut String, icon: &'static str, text: &str, name: &str, dialog: &str, tip: &str| {
            Action::new(name, &with_mnemonic(text, taken))
                .icon(icon)
                .tip(tip)
                // The dialog's object name, as the headless --dialog looks it up.
                .opens(dialog)
        };
        let edit_label = make(
            &mut taken,
            "EditLabel",
            "Edit Label...",
            EDIT_LABEL_ACTION,
            "labelEditDialog",
            "The selected label's style, own text, pinned place and layer (LABEL SET)",
        );
        let label_layout = make(
            &mut taken,
            "LabelLayout",
            "Label Layout Report...",
            LABEL_LAYOUT_ACTION,
            "labelLayoutDialog",
            "How many labels are placed, moved to find room, left without room or label nothing \
             at the annotation scale; selects those without room (LABEL LAYOUT)",
        );
        // Edit Label joins Edit Text's section when Edit Text is the last item there
        // (add_edit_text_action), so the editors sit together after the tools.
        let last_is_edit_text = match self.annotate_items.last() {
            Some(MenuItem::Action(action)) => action.name == EDIT_TEXT_ACTION,
            Some(_) => false,
            None => annotate
                .items
                .last()
                .is_some_and(|item| matches!(item, MenuItem::Action(action) if action.name == EDIT_TEXT_ACTION)),
        };
        if !last_is_edit_text {
            self.annotate_items
                .push(MenuItem::Section(String::from("Edit Annotation")));
        }
        self.annotate_items.push(MenuItem::Action(edit_label));
        self.annotate_items.push(MenuItem::Action(label_layout));
    }

    /// The Leaders manager's three entries, after the menu's tools: "annotateLeaders",
    /// "annotateLeadersForSelection", "annotateArrangeLeaders", each with a menu letter no item
    /// there has, under "Leader Manager" - not "Leaders": the tools' own Leaders group is above.
    pub fn add_leader_actions(&mut self, annotate: &Menu) {
        let mut taken = letters_taken(annotate, &self.annotate_items);
        let entries: [(&str, &str, &str, &'static str); 3] = [
            (
                LEADERS_ACTION,
                "Leaders...",
                "Edit leader positions, note frames, arrows and text sizes; a note read off its \
                 entity - attributes, level, length, chainage - the values to put in it, its \
                 look, and an attribute of that entity set through it (LEADER SET, LEADER PROP)",
                "LeaderManager",
            ),
            (
                LEADERS_FOR_SELECTION_ACTION,
                "Leaders for Selection...",
                "A leader, or a numbered balloon, to each selected entity, its note read off it \
                 (LEADER FOR, BALLOON FOR)",
                "LeadersForSelection",
            ),
            (
                ARRANGE_LEADERS_ACTION,
                "Arrange Leaders and Balloons...",
                "Line the selected leaders' notes up in a column, and number the balloons again \
                 (LEADER ALIGN, BALLOON RENUMBER)",
                "ArrangeLeaders",
            ),
        ];
        self.annotate_items
            .push(MenuItem::Section(String::from("Leader Manager")));
        for (name, text, tip, icon) in entries {
            let action = Action::new(name, &with_mnemonic(text, &mut taken))
                .icon(icon)
                .tip(tip)
                .opens("leaderManagerDialog");
            self.annotate_items.push(MenuItem::Action(action));
        }
    }

    /// The Annotate menu's entries added, in order, for the window to append after its tools.
    pub fn annotate_menu(&self) -> Menu {
        let mut menu = Menu::new(ANNOTATE_MENU, ANNOTATE_MENU_TITLE);
        menu.items.clone_from(&self.annotate_items);
        menu
    }

    // ---- the dialogs --------------------------------------------------------------------------

    /// Format > Text Styles...: the manager, made the first time, shown.
    pub fn show_text_styles(&mut self, document: &Document) -> &mut TextStyleManagerDialog {
        self.text_styles
            .shown(|| TextStyleManagerDialog::new(document))
    }

    pub fn show_label_styles(&mut self, document: &Document) -> &mut LabelStyleManagerDialog {
        self.label_styles
            .shown(|| LabelStyleManagerDialog::new(document))
    }

    pub fn show_dimension_styles(&mut self, document: &Document) -> &mut DimensionStyleManagerDialog {
        self.dimension_styles
            .shown(|| DimensionStyleManagerDialog::new(document))
    }

    /// Annotate > Edit Text...: the dialog on the one text selected.
    pub fn show_text_edit(&mut self, document: &Document) -> &mut TextEditDialog {
        self.text_edit.shown(|| TextEditDialog::new(document))
    }

    /// Edit Label on the selected label; with none it says so and runs nothing
    /// (label_edit_dialog).
    pub fn show_edit_label(&mut self, document: &Document) -> &mut LabelEditDialog {
        let dialog = self.edit_label.shown(|| LabelEditDialog::new(document));
        let _ = dialog.load_selection(document);
        dialog
    }

    /// The report, run afresh each time it is shown (label_layout_report).
    pub fn show_label_layout(&mut self, host: &mut dyn Host) -> &mut LabelLayoutReportDialog {
        let dialog = self.label_layout.shown(LabelLayoutReportDialog::new);
        let _ = dialog.run(host);
        dialog
    }

    /// The Leaders manager on its tab: 0 Leader, 1 For Selection, 2 Arrange. Made the first time,
    /// its lines run through the window's one executor and its scope controls offered the open
    /// views when they ask, not now. On the Leader tab it shows the first leader selected when the
    /// one it shows is not.
    pub fn show_leaders(&mut self, host: &mut dyn Host, tab: i32) -> &mut LeaderManagerDialog {
        let view_set = host.views().map(|views| views.view_set());
        let dialog = self.leaders.shown(|| {
            let views: Option<LeaderViews> = view_set.map(|set| {
                let views: LeaderViews = Rc::new(move || scope_filter_views(&set.borrow()));
                views
            });
            LeaderManagerDialog::new(host.document(), LeaderManagerContext { views })
        });
        let tab = tab.clamp(0, 2);
        dialog.show_tab(match tab {
            0 => Tab::Leader,
            1 => Tab::ForSelection,
            _ => Tab::Arrange,
        });
        let document = host.document();
        if tab == 0 && !document.selection().contains(dialog.current_leader()) {
            for id in document.selection().ids() {
                let is_leader = document
                    .model()
                    .entities
                    .find(id)
                    .is_some_and(|entity| matches!(entity.geometry, Geometry::Leader(_)));
                if is_leader {
                    let _ = dialog.show_leader(document, id);
                    break;
                }
            }
        }
        dialog
    }

    /// The dialogs as made, if they have been: the tests' way to them.
    pub fn text_style_manager(&self) -> Option<&TextStyleManagerDialog> {
        self.text_styles.dialog.as_ref()
    }

    pub fn label_style_manager(&self) -> Option<&LabelStyleManagerDialog> {
        self.label_styles.dialog.as_ref()
    }

    pub fn dimension_style_manager(&mut self) -> Option<&mut DimensionStyleManagerDialog> {
        self.dimension_styles.dialog.as_mut()
    }

    pub fn text_edit_dialog(&mut self) -> Option<&mut TextEditDialog> {
        self.text_edit.dialog.as_mut()
    }

    pub fn leader_manager(&mut self) -> Option<&mut LeaderManagerDialog> {
        self.leaders.dialog.as_mut()
    }
}

impl Workbench for AnnotationWorkbench {
    fn menus(&self) -> Vec<Menu> {
        let mut menus = vec![self.format_menu()];
        if !self.annotate_items.is_empty() {
            menus.push(self.annotate_menu());
        }
        menus
    }

    fn tool_bars(&self) -> Vec<ToolBar> {
        vec![self.format_tool_bar()]
    }

    fn docks(&self) -> Vec<DockSpec> {
        Vec::new()
    }

    /// TEXTSTYLE, DIMSTYLE, ANNOSCALE and the rest are the interpreter's: no line is this
    /// workbench's own.
    fn run_line(&mut self, _host: &mut dyn Host, _line: &str) -> bool {
        false
    }

    fn trigger(&mut self, host: &mut dyn Host, name: &str) -> bool {
        match name {
            TEXT_STYLES_ACTION => {
                let _ = self.show_text_styles(host.document());
            }
            LABEL_STYLES_ACTION => {
                let _ = self.show_label_styles(host.document());
            }
            DIMENSION_STYLES_ACTION => {
                let _ = self.show_dimension_styles(host.document());
            }
            EDIT_TEXT_ACTION => {
                let _ = self.show_text_edit(host.document());
            }
            EDIT_LABEL_ACTION => {
                let _ = self.show_edit_label(host.document());
            }
            LABEL_LAYOUT_ACTION => {
                let _ = self.show_label_layout(host);
            }
            LEADERS_ACTION => {
                let _ = self.show_leaders(host, 0);
            }
            LEADERS_FOR_SELECTION_ACTION => {
                let _ = self.show_leaders(host, 1);
            }
            ARRANGE_LEADERS_ACTION => {
                let _ = self.show_leaders(host, 2);
            }
            _ => return false,
        }
        true
    }

    fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.look(host.document());
        let ctx = ui.ctx().clone();
        self.text_styles.ui(|dialog| dialog.ui(&ctx, host));
        self.label_styles.ui(|dialog| dialog.ui(&ctx, host));
        self.dimension_styles.ui(|dialog| dialog.ui(&ctx, host));
        self.text_edit.ui(|dialog| dialog.ui(&ctx, host));
        self.edit_label.ui(|dialog| dialog.ui(&ctx, host));
        self.label_layout.ui(|dialog| dialog.ui(&ctx, host));
        self.leaders.ui(|dialog| dialog.ui(&ctx, host));
    }
}
