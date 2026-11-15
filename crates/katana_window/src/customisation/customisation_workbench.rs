//! The Format menu and toolbar: the three customisation managers and what goes with them - the port
//! of customisation/customisation_workbench (the owner's requests of 2026-09-24: "enhance the
//! linestyle and symbol and survey codes managers" and "make it professional CAD software").
//!
//! The menu, in two titled sections:
//!
//! ```text
//! Tables and Libraries
//!   Layers...                    the window's action (services.layers)
//!   Styles and Linetypes...      formatStyles       -> styleManagerDialog
//!   Symbol Library...            formatSymbols      -> symbolLibraryDialog
//!   Survey Code Manager...       formatSurveyCodes  -> surveyCodeManagerDialog
//! Across the Drawing
//!   Global Modify...             formatGlobalModify -> globalModifyDialog
//!   Purge Unused...              formatPurge
//! ```
//!
//! Each manager's action carries its dialog's object name, which is how the headless --dialog
//! switch finds the dialog an action opened. A customisation file is loaded by the CUSTOMISE line
//! and from File > Settings, which this object makes and owns (fileSettings -> settingsDialog): the
//! item is File's, but Settings must know whether the Survey Code Manager holds unapplied edits and
//! opens the managers kept here.
//!
//! The workbench owns what the managers share: the picture cache and the one context they are
//! built from. Each manager is NON-MODAL, made the first time it is asked for and kept - hidden,
//! not dropped - between uses, so the code manager's unapplied edits survive closing it; asking
//! again shows the same one. It owns the definition editor the same way: no menu item, but the
//! Symbol Library's and the Linetypes tab's buttons ask for it through the context.
//!
//! THE RUST SHAPE. The C++ workbench is handed the window's services (the Document, the views, the
//! executor, the log, an action factory) and keeps its dialogs as the window's children. Here the
//! window is lent as `&mut dyn Host` to every call (shell::workbench), so the drawing, the
//! executor, the log and the views are the host's; what a dialog asks of another - the editor
//! asked for by a manager, a manager opened from Settings - is queued by the asking dialog's
//! callback and carried out after it has been drawn, since a dialog cannot reach a sibling the
//! workbench holds while it draws.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use egui::Ui;
use katana_cad::BuiltInEntry;
use katana_cad::purge::{PurgeOptions, plan_purge, purge_command};
use katana_entity::EntityId;

use super::code_manager::{SurveyCodeManagerDialog, TAB_PAGES};
use super::customisation_context::{
    AfterCommit, CustomisationContext, DefinitionEdit, SharedThumbnails,
};
use super::definition_editor::DefinitionEditorDialog;
use super::definition_thumbnails::DefinitionThumbnails;
use super::global_modify_dialog::GlobalModifyDialog;
use super::scope_filter_widget::scope_filter_views;
use super::settings_dialog::{SettingsContext, SettingsDialog, run_through_host};
use super::style_painter::CanvasRasterBackend;
use super::symbol_library::SymbolLibraryDialog;
use crate::drive::action::{Action, Menu, ToolBar, ToolBarItem};
use crate::host::Host;
use crate::shell::workbench::{DockSpec, Workbench};
use crate::style_manager::StyleManagerDialog;

/// The Format menu and toolbar, as the window names them.
pub const FORMAT_MENU: &str = "formatMenu";
pub const FORMAT_MENU_TITLE: &str = "F&ormat";
pub const FORMAT_TOOL_BAR: &str = "FormatToolBar";
pub const FORMAT_TOOL_BAR_TITLE: &str = "Format";

/// The actions' object names.
pub const STYLES_ACTION: &str = "formatStyles";
pub const SYMBOLS_ACTION: &str = "formatSymbols";
pub const CODES_ACTION: &str = "formatSurveyCodes";
pub const GLOBAL_MODIFY_ACTION: &str = "formatGlobalModify";
pub const PURGE_ACTION: &str = "formatPurge";
pub const SETTINGS_ACTION: &str = "fileSettings";

/// The object names the headless driver finds each manager by, carried on its action. The style
/// manager's is the C++ workbench's own name for it (see `style_manager_action`).
pub const STYLE_MANAGER_DIALOG: &str = "styleManagerDialog";
pub const SYMBOL_LIBRARY_DIALOG: &str = "symbolLibraryDialog";
pub const CODE_MANAGER_DIALOG: &str = "surveyCodeManagerDialog";
pub const GLOBAL_MODIFY_DIALOG: &str = "globalModifyDialog";
pub const SETTINGS_DIALOG: &str = "settingsDialog";

/// What only the window knows, beside the host it lends at each call.
#[derive(Default)]
pub struct CustomisationServices {
    /// The window's Layers action, which the Format menu shows too. None: not shown.
    pub layers: Option<Action>,
    /// True in a session nobody watches: no modal box, no file dialog. Asked each time, as the
    /// window learns it after it is built. None: the host's.
    pub headless: Option<Box<dyn Fn() -> bool>>,
    /// Whether the managers' lines run through the window's executor (the C++'s services.run being
    /// set); false, a manager says it cannot run its line.
    pub has_runner: bool,
    /// True when this session has a kept customisation file - the one CUSTOMISE KEEP writes and
    /// the next start reads. It decides whether an editor's own commit is followed by a CUSTOMISE
    /// KEEP line. None, or false: no line is ever run.
    pub has_kept_file: Option<Rc<dyn Fn() -> bool>>,
    /// What the session's host offers, for Settings: a built-in to reset to, and the kept file's
    /// path as text. Asked once, when Settings is first opened. None: no built-in, no kept file.
    pub has_built_in: Option<Box<dyn Fn() -> bool>>,
    pub kept_file: Option<Box<dyn Fn() -> String>>,
    /// The built-ins Settings' chooser offers (a test's). None: those compiled in.
    pub built_ins: Option<Rc<dyn Fn() -> Vec<BuiltInEntry>>>,
}

/// What Settings asked to open, carried out after it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Opening {
    Codes,
    Linework,
    Symbols,
}

/// A dialog the workbench keeps, made the first time it is asked for, and whether it is shown.
struct Kept<D> {
    dialog: Option<D>,
    shown: bool,
}

impl<D> Default for Kept<D> {
    fn default() -> Self {
        Self {
            dialog: None,
            shown: false,
        }
    }
}

/// The Format menu's workbench (see the module comment).
pub struct CustomisationWorkbench {
    services: CustomisationServices,
    thumbnails: SharedThumbnails,
    styles: Kept<StyleManagerDialog>,
    symbols: Option<SymbolLibraryDialog>,
    codes: Option<SurveyCodeManagerDialog>,
    global_modify: Kept<GlobalModifyDialog>,
    definitions: Option<DefinitionEditorDialog>,
    settings: Option<SettingsDialog>,
    /// Asked instead of a question box in an interactive session - a test's answer. None: the
    /// question is a refusal said in the log, as egui has no modal box to wait in.
    #[allow(clippy::type_complexity, reason = "A boxed callback, named where it is set.")]
    pub confirm: Option<Box<dyn FnMut(&str) -> bool>>,
    /// The editor asked for by a manager (CustomisationContext::edit_definition).
    edits: Rc<RefCell<Vec<(DefinitionEdit, String)>>>,
    /// The editors Settings asked for.
    openings: Rc<RefCell<Vec<Opening>>>,
    /// Whether the code manager's buffer holds unapplied edits, as last looked at: what Settings
    /// asks at its press, while the manager itself is the workbench's to lend.
    codes_dirty: Rc<Cell<bool>>,
}

/// "1 style", "2 styles".
fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

impl CustomisationWorkbench {
    pub fn new(services: CustomisationServices) -> Self {
        Self {
            services,
            thumbnails: Rc::new(RefCell::new(DefinitionThumbnails::new(Arc::new(
                CanvasRasterBackend,
            )))),
            styles: Kept::default(),
            symbols: None,
            codes: None,
            global_modify: Kept::default(),
            definitions: None,
            settings: None,
            confirm: None,
            edits: Rc::new(RefCell::new(Vec::new())),
            openings: Rc::new(RefCell::new(Vec::new())),
            codes_dirty: Rc::new(Cell::new(false)),
        }
    }

    // ---- the actions ---------------------------------------------------------------------------

    pub fn style_manager_action() -> Action {
        Action::new(STYLES_ACTION, "St&yles and Linetypes...")
            .icon("FormatStyles")
            .tip(
                "The drawing's styles, its dash linetypes, its hatch patterns and the loaded \
                 library linestyles: edit, merge, rename, find what uses each, and what nothing \
                 defines",
            )
            .opens(STYLE_MANAGER_DIALOG)
    }

    pub fn symbol_library_action() -> Action {
        Action::new(SYMBOLS_ACTION, "Sym&bol Library...")
            .icon("FormatSymbols")
            .tip(
                "Every symbol the drawing can draw, by group: what each prints at a scale, which \
                 styles and survey codes use it; put one on the selected points",
            )
            .opens(SYMBOL_LIBRARY_DIALOG)
    }

    /// The action the Survey menu shows too.
    pub fn code_manager_action() -> Action {
        Action::new(CODES_ACTION, "&Survey Code Manager...")
            .icon("FormatSurveyCodes")
            .tip(
                "The survey codes of the session's customisation: test a code, edit its rules, \
                 see the drawing's codes and their issues, apply the codes, run linework",
            )
            .opens(CODE_MANAGER_DIALOG)
    }

    pub fn global_modify_action() -> Action {
        Action::new(GLOBAL_MODIFY_ACTION, "&Global Modify...")
            .icon("GlobalModify")
            .tip(
                "Change the selection, what a view shows, whole layers or the drawing at once: \
                 layer, colour, style, symbol, properties, and their layers' and styles' \
                 settings, as one undo step",
            )
            .opens(GLOBAL_MODIFY_DIALOG)
    }

    pub fn purge_action() -> Action {
        Action::new(PURGE_ACTION, "&Purge Unused...")
            .icon("Purge")
            .tip("Delete the styles, linetypes and hatch patterns nothing uses, as one undo step")
    }

    /// File > Settings: made here because its dialog is this object's; the window puts it in File,
    /// above Quit (on macOS, in the application menu).
    pub fn settings_action() -> Action {
        Action::new(SETTINGS_ACTION, "Settin&gs...")
            .icon("Settings")
            .tip(
                "The session's customisation - its linestyles, symbols and survey codes: load a \
                 file into it, write it to one, reset it to the built-in, keep it for the next \
                 start, and what is done to survey data as it comes in",
            )
            .opens(SETTINGS_DIALOG)
    }

    /// The Format menu: two titled sections, so a style that draws titles shows what each group is
    /// for.
    pub fn format_menu(&self) -> Menu {
        let mut menu = Menu::new(FORMAT_MENU, FORMAT_MENU_TITLE).section("Tables and Libraries");
        if let Some(layers) = &self.services.layers {
            menu = menu.action(layers.clone());
        }
        menu.action(Self::style_manager_action())
            .action(Self::symbol_library_action())
            .action(Self::code_manager_action())
            .section("Across the Drawing")
            .action(Self::global_modify_action())
            .action(Self::purge_action())
    }

    pub fn format_tool_bar(&self) -> ToolBar {
        let mut bar = ToolBar::new(FORMAT_TOOL_BAR, FORMAT_TOOL_BAR_TITLE);
        for action in [
            Self::style_manager_action(),
            Self::symbol_library_action(),
            Self::code_manager_action(),
            Self::global_modify_action(),
        ] {
            bar.items.push(ToolBarItem::Action(action));
        }
        bar
    }

    // ---- the context ---------------------------------------------------------------------------

    fn headless(&self, host: &dyn Host) -> bool {
        self.services
            .headless
            .as_ref()
            .map_or_else(|| host.headless(), |headless| headless())
    }

    /// What every manager is built from, for a caller building one of its own.
    pub fn context(&self) -> CustomisationContext {
        let edits = Rc::clone(&self.edits);
        let has_runner = self.services.has_runner;
        let has_kept_file = self.services.has_kept_file.clone();
        CustomisationContext {
            thumbnails: Some(Rc::clone(&self.thumbnails)),
            select_and_show: Some(Rc::new(select_and_show)),
            edit_definition: Some(Rc::new(move |what, name| {
                edits.borrow_mut().push((what, String::from(name)));
            })),
            // An editor's own commit keeps a kept session kept: asked BEFORE the commit, which is
            // what makes the session "not kept", and answered with the line a person would type -
            // CUSTOMISE KEEP through the window's executor. Nothing is handed back when the
            // session was not the kept one, or when this session has no kept file: KEEP would
            // only be refused, in the log, after every Save.
            begin_commit: Some(Rc::new(move |host: &mut dyn Host| -> Option<AfterCommit> {
                let kept_file = has_kept_file.as_ref().is_some_and(|has| has());
                if !host.document().customisation_state().kept || !has_runner || !kept_file {
                    return None;
                }
                Some(Box::new(|host: &mut dyn Host| {
                    let _ = host.run("CUSTOMISE KEEP");
                }))
            })),
            has_runner,
        }
    }

    pub fn thumbnails(&self) -> SharedThumbnails {
        Rc::clone(&self.thumbnails)
    }

    // ---- the managers --------------------------------------------------------------------------

    /// Shows the style manager, making it the first time, with its forms filled in.
    pub fn show_style_manager(&mut self, host: &mut dyn Host) -> &mut StyleManagerDialog {
        let first = self.styles.dialog.is_none();
        if first {
            let mut dialog = StyleManagerDialog::new(&self.context(), host.document());
            // The C++ workbench named its manager so (kStyleManagerName), as it names no dialog of
            // its own: what --dialog formatStyles finds it by.
            dialog.set_object_name(STYLE_MANAGER_DIALOG);
            self.styles.dialog = Some(dialog);
        }
        self.styles.shown = true;
        let dialog = self.styles.dialog.as_mut().expect("made above");
        if first {
            // The forms and previews filled in, rather than an empty pane, the first time it is
            // seen.
            dialog.show_first_rows(host.document());
        }
        dialog
    }

    pub fn show_symbol_library(&mut self, host: &mut dyn Host) -> &mut SymbolLibraryDialog {
        let headless = self.headless(&*host);
        if self.symbols.is_none() {
            let mut dialog = SymbolLibraryDialog::new(host.document(), self.context());
            // Opened on the first symbol, so its details and picture show what the pane is for.
            if let Some(first) = dialog.shown_names().first().cloned() {
                let _ = dialog.select_symbol(host, &first);
            }
            self.symbols = Some(dialog);
        }
        let dialog = self.symbols.as_mut().expect("made above");
        // A headless session opens no file dialog: Load and Export say in the log what to call.
        dialog.set_headless(headless);
        dialog.show();
        dialog
    }

    pub fn show_code_manager(&mut self, host: &mut dyn Host) -> &mut SurveyCodeManagerDialog {
        let interactive = !self.headless(&*host);
        if self.codes.is_none() {
            let mut dialog = SurveyCodeManagerDialog::new(host.document(), self.context());
            if let Some(views) = host.views() {
                // The View scope of its Apply Codes and Linework tabs, as Global Modify's: read
                // afresh each time.
                let views = views.view_set();
                dialog.set_views(Some(Rc::new(move || scope_filter_views(&views.borrow()))));
            }
            // Opened on the first rule, explained and in the form.
            if !dialog.buffer().rules().is_empty() {
                dialog.select_rule(host, 0);
            }
            self.codes = Some(dialog);
        }
        let dialog = self.codes.as_mut().expect("made above");
        // Interactive: file dialogs, and Apply / Discard / Cancel on closing with unapplied edits.
        dialog.set_interactive(interactive);
        dialog.show();
        self.codes_dirty.set(dialog.dirty());
        dialog
    }

    /// The code manager at its Linework tab, the one editor of the linework control codes: what
    /// Settings' Edit Linework Codes opens. The tab is brought forward by the name the manager
    /// gives its page; a manager without it says so in the log.
    pub fn show_linework(&mut self, host: &mut dyn Host) -> &mut SurveyCodeManagerDialog {
        let _ = self.show_code_manager(host);
        let page = TAB_PAGES.iter().position(|page| *page == "lineworkTab");
        if page.is_none() {
            host.log(
                "The Survey Code Manager is open, but its Linework tab was not found to bring \
                 forward: choose it there.",
                true,
            );
        }
        let dialog = self.codes.as_mut().expect("shown above");
        if let Some(page) = page {
            dialog.tab = page;
        }
        dialog
    }

    pub fn show_global_modify(&mut self, host: &mut dyn Host) -> &mut GlobalModifyDialog {
        if self.global_modify.dialog.is_none() {
            let mut dialog = GlobalModifyDialog::new(&self.context(), host.document());
            if let Some(views) = host.views() {
                let views = views.view_set();
                dialog.set_views(Rc::new(move || scope_filter_views(&views.borrow())));
            }
            self.global_modify.dialog = Some(dialog);
        }
        self.global_modify.shown = true;
        let dialog = self.global_modify.dialog.as_mut().expect("made above");
        dialog.reload(host.document());
        let _ = dialog.preview(host.document());
        dialog
    }

    /// The one definition editor, shown - made the first time.
    pub fn show_definition_editor(&mut self, host: &mut dyn Host) -> &mut DefinitionEditorDialog {
        if self.definitions.is_none() {
            self.definitions = Some(DefinitionEditorDialog::new(host.document(), self.context()));
        }
        let dialog = self.definitions.as_mut().expect("made above");
        dialog.show();
        dialog
    }

    /// File > Settings: the one Settings dialog, shown - made the first time, from the host and
    /// from what this object knows: whether the code manager's buffer is dirty, and how to open
    /// the editors.
    pub fn show_settings(&mut self, host: &mut dyn Host) -> &mut SettingsDialog {
        if self.settings.is_none() {
            let dirty = Rc::clone(&self.codes_dirty);
            let opening = |what: Opening, openings: &Rc<RefCell<Vec<Opening>>>| {
                let openings = Rc::clone(openings);
                Some(Box::new(move |_host: &mut dyn Host| openings.borrow_mut().push(what))
                    as Box<dyn FnMut(&mut dyn Host)>)
            };
            let built_ins = self.services.built_ins.clone();
            let context = SettingsContext {
                reads_drawing: true,
                run: self.services.has_runner.then(run_through_host),
                code_manager_dirty: Some(Box::new(move || dirty.get())),
                open_code_manager: opening(Opening::Codes, &self.openings),
                open_linework: opening(Opening::Linework, &self.openings),
                open_symbol_library: opening(Opening::Symbols, &self.openings),
                // The host's two facts, as they are now: the window made its host before anything
                // could open this.
                has_built_in: self.services.has_built_in.as_ref().is_some_and(|has| has()),
                kept_file: self
                    .services
                    .kept_file
                    .as_ref()
                    .map_or_else(String::new, |kept| kept()),
                built_ins: built_ins.map(|list| {
                    Box::new(move || list()) as Box<dyn Fn() -> Vec<BuiltInEntry>>
                }),
                ..SettingsContext::default()
            };
            self.settings = Some(SettingsDialog::new(context, &*host));
        }
        let dialog = self.settings.as_mut().expect("made above");
        dialog.show();
        dialog
    }

    /// A manager's request of the editor: the editor is shown, then asked. False when it refused,
    /// which it has said in its own message area and in the log.
    pub fn edit_definition(&mut self, host: &mut dyn Host, what: DefinitionEdit, name: &str) -> bool {
        self.show_definition_editor(host).request(host, what, name)
    }

    // ---- the managers made so far ------------------------------------------------------------

    pub fn style_manager(&mut self) -> Option<&mut StyleManagerDialog> {
        self.styles.dialog.as_mut()
    }

    pub fn style_manager_shown(&self) -> bool {
        self.styles.dialog.is_some() && self.styles.shown
    }

    pub fn symbol_library(&mut self) -> Option<&mut SymbolLibraryDialog> {
        self.symbols.as_mut()
    }

    pub fn code_manager(&mut self) -> Option<&mut SurveyCodeManagerDialog> {
        self.codes.as_mut()
    }

    pub fn global_modify(&mut self) -> Option<&mut GlobalModifyDialog> {
        self.global_modify.dialog.as_mut()
    }

    pub fn global_modify_shown(&self) -> bool {
        self.global_modify.dialog.is_some() && self.global_modify.shown
    }

    pub fn definition_editor(&mut self) -> Option<&mut DefinitionEditorDialog> {
        self.definitions.as_mut()
    }

    pub fn settings(&mut self) -> Option<&mut SettingsDialog> {
        self.settings.as_mut()
    }

    // ---- purge and close -----------------------------------------------------------------------

    /// Format > Purge Unused: every style, linetype and hatch pattern nothing uses, keeping the
    /// current style, deleted as ONE undo step and named in the log. An interactive session is
    /// asked first; a headless one is not. True when something was purged.
    pub fn purge_unused(&mut self, host: &mut dyn Host) -> bool {
        let mut options = PurgeOptions::default();
        // What the next thing drawn will wear is not unused, although nothing wears it yet - the
        // command line's PURGE keeps it for the same reason.
        let current = String::from(host.document().current_style());
        if !current.is_empty() {
            options.keep_styles.push(current);
        }
        let plan = plan_purge(host.document().model(), &options);
        if plan.empty() {
            host.log(
                "Purge Unused: nothing to purge - every style, linetype and hatch pattern is used.",
                false,
            );
            return false;
        }
        let what = format!(
            "{}, {} and {}",
            counted(plan.styles.len(), "style", "styles"),
            counted(plan.linetypes.len(), "linetype", "linetypes"),
            counted(plan.hatch_patterns.len(), "hatch pattern", "hatch patterns")
        );
        if !self.headless(&*host) {
            let question = format!("Delete {what} that nothing uses? One Undo restores them.");
            let yes = self.ask(host, "Purge Unused", &question);
            if !yes {
                host.log("Purge Unused: cancelled; nothing was deleted.", false);
                return false;
            }
        }
        let command = match purge_command(host.document().model(), &options) {
            Ok(Some(command)) => command,
            Ok(None) => {
                // plan_purge said otherwise a moment ago; the model has not changed since, so this
                // is only reached if the two ever disagree.
                host.log("Purge Unused: nothing to purge.", false);
                return false;
            }
            Err(error) => {
                host.log(&format!("Purge Unused: {}", error.describe()), true);
                return false;
            }
        };
        if let Err(error) = host.document_mut().execute(command) {
            host.log(&format!("Purge Unused: {}", error.describe()), true);
            return false;
        }
        let mut report = format!("Purge Unused: purged {what} (one Undo restores them).");
        if !plan.styles.is_empty() {
            report += &format!(" Styles: {}.", plan.styles.join(", "));
        }
        if !plan.linetypes.is_empty() {
            report += &format!(" Linetypes: {}.", plan.linetypes.join(", "));
        }
        if !plan.hatch_patterns.is_empty() {
            report += &format!(" Hatch patterns: {}.", plan.hatch_patterns.join(", "));
        }
        host.log(&report, false);
        true
    }

    /// A yes-or-no question: the test's answer when set; otherwise, egui having no modal box to
    /// wait in, a refusal said in the log - the question is asked again when the person does it
    /// again with the answer in hand.
    fn ask(&mut self, host: &mut dyn Host, title: &str, question: &str) -> bool {
        match &mut self.confirm {
            Some(confirm) => confirm(question),
            None => {
                host.warn_user(title, question);
                false
            }
        }
    }

    /// Whether the window may close, as far as the managers are concerned. The definition editor's
    /// unsaved form is asked about first: interactive, the editor is shown and asked whether to
    /// discard it (a yes remembered until the form changes); headless, refused and said. Then the
    /// code manager's unapplied rule edits, which live only in its buffer: headless, refused and
    /// said; interactive, the manager is shown and closed, so its own Apply / Discard / Cancel is
    /// asked over the rules it is about - in its footer, answered at a later frame, so the window
    /// may not close now.
    pub fn confirm_close(&mut self, host: &mut dyn Host) -> bool {
        let headless = self.headless(&*host);
        let editor_dirty = self
            .definitions
            .as_ref()
            .is_some_and(|editor| editor.dirty() && !editor.discard_agreed());
        if editor_dirty {
            if headless {
                host.log(
                    "Unsaved Definition: the definition editor has edits that are not saved, and \
                     a headless run has nobody to ask whether to discard them; Save or Revert \
                     them first.",
                    true,
                );
                return false;
            }
            let _ = self.show_definition_editor(host);
            let yes = self.ask(
                host,
                "Unsaved Definition",
                "The definition editor has edits that are not saved. Discard them?",
            );
            if !yes {
                return false;
            }
            // Remembered, and NOTHING reverted: the code manager and the window's own
            // unsaved-drawing question are still to come.
            if let Some(editor) = &mut self.definitions {
                editor.agree_to_discard();
            }
        }
        let codes_dirty = self.codes.as_ref().is_some_and(SurveyCodeManagerDialog::dirty);
        if !codes_dirty {
            return true;
        }
        // The manager's own headless close would say its edits are kept in it, which is untrue
        // when the window goes. A script can press Apply (applyMap) or Revert (revertMap).
        if headless {
            host.log(
                "Unapplied Edits: the Survey Code Manager has rule edits that are not on the \
                 drawing, and a headless run has nobody to ask whether to discard them; Apply or \
                 Revert them first.",
                true,
            );
            return false;
        }
        // Shown first, so the question comes over the rules it is about; closing it asks.
        let codes = self.show_code_manager(host);
        codes.reject(host);
        let codes = self.codes.as_ref().expect("shown above");
        !(codes.is_open() && codes.dirty())
    }

    // ---- the frame -----------------------------------------------------------------------------

    /// What the dialogs asked of each other since the last frame, carried out now: the editor a
    /// manager asked for, the managers Settings asked to open.
    fn carry_out(&mut self, host: &mut dyn Host) {
        // The Symbol Library's buttons queue their requests in it; the Linetypes tab's come
        // through the context's callback.
        let mut edits = self
            .symbols
            .as_mut()
            .map(SymbolLibraryDialog::take_definition_requests)
            .unwrap_or_default();
        edits.extend(std::mem::take(&mut *self.edits.borrow_mut()));
        for (what, name) in edits {
            let _ = self.edit_definition(host, what, &name);
        }
        let openings = std::mem::take(&mut *self.openings.borrow_mut());
        for opening in openings {
            match opening {
                Opening::Codes => {
                    let _ = self.show_code_manager(host);
                }
                Opening::Linework => {
                    let _ = self.show_linework(host);
                }
                Opening::Symbols => {
                    let _ = self.show_symbol_library(host);
                }
            }
        }
        if let Some(codes) = &self.codes {
            self.codes_dirty.set(codes.dirty());
        }
    }

    /// Imports a customisation file's definitions through the Symbol Library, as its Import does:
    /// what Settings and a test call with a path.
    pub fn import_definitions(&mut self, host: &mut dyn Host, path: &Path) -> bool {
        self.show_symbol_library(host).import_definitions_file(host, path)
    }
}

/// "Show me what uses this": select `ids` and frame them where the person is looking - the ACTIVE
/// plan view - by that view's Zoom to Selection line through the window's executor, so the log
/// says what moved the view and the views linked with it follow.
fn select_and_show(host: &mut dyn Host, ids: &[EntityId]) {
    host.document_mut().selection_mut().set(ids.to_vec());
    host.document().notify_selection_changed();
    let active = host.views().and_then(|views| views.active_plan_view());
    if let Some(view) = active {
        let _ = host.run(&format!("ZOOM SELECTION view={view}"));
    }
}

impl Workbench for CustomisationWorkbench {
    fn menus(&self) -> Vec<Menu> {
        vec![self.format_menu()]
    }

    fn tool_bars(&self) -> Vec<ToolBar> {
        vec![self.format_tool_bar()]
    }

    fn docks(&self) -> Vec<DockSpec> {
        Vec::new()
    }

    /// CUSTOMISE, PURGE and the rest are the interpreter's: no line is this workbench's own.
    fn run_line(&mut self, _host: &mut dyn Host, _line: &str) -> bool {
        false
    }

    fn trigger(&mut self, host: &mut dyn Host, name: &str) -> bool {
        match name {
            STYLES_ACTION => {
                let _ = self.show_style_manager(host);
            }
            SYMBOLS_ACTION => {
                let _ = self.show_symbol_library(host);
            }
            CODES_ACTION => {
                let _ = self.show_code_manager(host);
            }
            GLOBAL_MODIFY_ACTION => {
                let _ = self.show_global_modify(host);
            }
            PURGE_ACTION => {
                let _ = self.purge_unused(host);
            }
            SETTINGS_ACTION => {
                let _ = self.show_settings(host);
            }
            _ => return false,
        }
        true
    }

    fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if let Some(codes) = &self.codes {
            self.codes_dirty.set(codes.dirty());
        }
        let ctx = ui.ctx().clone();
        if self.styles.shown
            && let Some(styles) = &mut self.styles.dialog
        {
            self.styles.shown = styles.ui(&ctx, host);
        }
        if let Some(symbols) = &mut self.symbols {
            let _ = symbols.ui(&ctx, host);
        }
        if let Some(codes) = &mut self.codes {
            let _ = codes.ui(&ctx, host);
            self.codes_dirty.set(codes.dirty());
        }
        if self.global_modify.shown
            && let Some(global) = &mut self.global_modify.dialog
        {
            self.global_modify.shown = global.ui(&ctx, host);
        }
        if let Some(editor) = &mut self.definitions {
            let _ = editor.ui(&ctx, host);
        }
        if let Some(settings) = &mut self.settings {
            settings.ui(ui, host);
        }
        self.carry_out(host);
    }
}
