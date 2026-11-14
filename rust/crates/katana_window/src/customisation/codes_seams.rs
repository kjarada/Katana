//! The seams of the survey code manager, the definition editor and the symbol library (CU2, step 7
//! wave 2) to four units of the C++ that are other agents' and were not written when these were:
//!
//! ```text
//! customisation_context.hpp  CodesContext, DefinitionEdit       CU1's customisation_context
//! name_picker.hpp            NamePicker, UNDEFINED_NAME_COLOUR   CU1's name_picker
//! style_preview.hpp          StylePreview, PreviewGround         CU1's style_preview
//! linework_labels.hpp        linework_control_label              UP's linework_labels (wave 3)
//! ```
//!
//! Each is the C++ header's shape as far as the three dialogs call it, and no more: the picker
//! lists the names cad's catalogue offers and keeps a name it does not list, and draws no
//! pictures; the preview keeps what it was asked to show and its scale and ground, and paints
//! nothing of a definition. THE INTEGRATOR JOINS THEM: each type here is to be replaced by the
//! real module's, after which this file goes.
//!
//! What of CustomisationContext is not here is the host's (host::Host), which a dialog is lent
//! while it draws and while one of its methods runs: the drawing (`document`), the command log
//! (`log`) and the window's one executor (`run`).

use std::cell::RefCell;
use std::rc::Rc;

use egui::{Response, Sense, Ui, Vec2};
use katana_cad::Document;
use katana_cad::style_catalogue::{NameRole, linetype_choices, symbol_choices};
use katana_entity::{EntityId, LineStyle};

use crate::canvas::Color;
use crate::customisation::definition_thumbnails::DefinitionThumbnails;
use crate::drive::controls::{Choice, choice, custom};
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::host::Host;
use crate::theme::palette;

// ---- customisation_context.hpp -----------------------------------------------------------------

/// What a manager asks the definition editor to do with a library definition. A new one is of
/// the kind the manager that asked lists: the symbol library's is a symbol, the Linetypes tab's a
/// linestyle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DefinitionEdit {
    NewSymbol,
    NewLinestyle,
    Edit,
    Duplicate,
    Delete,
}

/// What a commit hook hands back, called after a commit that succeeded.
pub type Committed = Box<dyn FnOnce(&mut dyn Host)>;

/// CustomisationContext::beginCommit: called BEFORE an editor commits to the session's
/// customisation, and what it hands back AFTER the commit succeeded - never after one that was
/// refused. It is lent the host at each call, since nothing of the window is kept by a dialog.
pub type BeginCommit = Rc<dyn Fn(&mut dyn Host) -> Option<Committed>>;

/// CustomisationContext::selectAndShow: select these entities and frame them in the active view.
pub type SelectAndShow = Rc<dyn Fn(&mut dyn Host, &[EntityId])>;

/// What a customisation dialog is given by whoever opens it, beside the host it is lent.
#[derive(Clone)]
pub struct CodesContext {
    /// The picture cache shared by every picker and browser the dialogs open. None: a table
    /// draws no pictures, and the symbol library makes a cache of its own.
    pub thumbnails: Option<Rc<RefCell<DefinitionThumbnails>>>,
    /// None (a test, a dialog built on its own): the dialog only selects.
    pub select_and_show: Option<SelectAndShow>,
    /// None: nothing is asked round a commit.
    pub begin_commit: Option<BeginCommit>,
    /// False for a dialog opened without the window's command line (the C++'s empty
    /// CustomisationContext::run): it then says it cannot run its line rather than doing the work
    /// itself.
    pub command_line: bool,
    /// Whether the maker keeps a definition editor to ask (the C++'s editDefinition being set):
    /// false, the buttons that would ask are disabled. What a button asked is read back from the
    /// dialog (SymbolLibraryDialog::take_definition_requests), since the editor needs the host the
    /// dialog is drawn with.
    pub definition_editor: bool,
}

impl Default for CodesContext {
    fn default() -> Self {
        Self {
            thumbnails: None,
            select_and_show: None,
            begin_commit: None,
            command_line: true,
            definition_editor: false,
        }
    }
}

impl CodesContext {
    /// The same context with no command line.
    pub fn without_command_line(mut self) -> Self {
        self.command_line = false;
        self
    }

    /// The commit hook's first call: what it hands back, to call after the commit.
    pub(crate) fn begin(&self, host: &mut dyn Host) -> Option<Committed> {
        self.begin_commit.as_ref().and_then(|begin| begin(host))
    }
}

// ---- name_picker.hpp ---------------------------------------------------------------------------

/// The mark for a name nothing defines - in a picker, and wherever a manager's table shows one, so
/// "not defined" reads the same everywhere: amber, between the theme's text and its error red,
/// since a missing name is drawn (by a fallback) and is not an error.
pub const UNDEFINED_NAME_COLOUR: Color = Color::rgb(0xE8, 0xA8, 0x3A);

/// An editable choice of a linetype name or a symbol name that never drops the name it was given:
/// a name in no list is kept, first of all, and `current_name` gives it back exactly as it was
/// set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamePicker {
    role: NameRole,
    offer_by_layer: bool,
    names: Vec<String>,
    chosen: Choice,
}

impl NamePicker {
    /// `role` picks the list; `offer_by_layer` adds ByLayer to a linetype list.
    pub fn new(role: NameRole, offer_by_layer: bool) -> Self {
        Self {
            role,
            offer_by_layer,
            names: Vec::new(),
            chosen: Choice::editable(""),
        }
    }

    pub fn role(&self) -> NameRole {
        self.role
    }

    fn listed(&self, document: &Document) -> Vec<String> {
        let choices = match self.role {
            NameRole::Linetype => linetype_choices(document, self.offer_by_layer),
            NameRole::Symbol => symbol_choices(document),
        };
        choices.into_iter().map(|entry| entry.name).collect()
    }

    /// Shows `name`, adding it first when no section holds it exactly. "" shows nothing chosen.
    pub fn set_current_name(&mut self, document: &Document, name: &str) {
        self.names = self.listed(document);
        if !name.is_empty() && !self.names.iter().any(|listed| listed == name) {
            self.names.insert(0, String::from(name));
        }
        self.chosen.index = self.names.iter().position(|listed| listed == name);
        self.chosen.text = String::from(name);
    }

    /// The name being shown, exactly: a listed item's, or what was typed.
    pub fn current_name(&self) -> String {
        self.chosen.text.clone()
    }

    /// QComboBox::setEditText: `text` shown as typed, the list as it was.
    pub fn set_edit_text(&mut self, text: &str) {
        self.chosen.index = self.names.iter().position(|listed| listed == text);
        self.chosen.text = String::from(text);
    }

    /// Rebuilds the list from the Document, keeping the current name.
    pub fn refresh(&mut self, document: &Document) {
        let kept = self.current_name();
        self.set_current_name(document, &kept);
    }

    /// The names offered, in list order: the kept name first when there is one.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Draws the picker as `name`; true when a person or a driver chose or typed another name.
    pub fn ui(&mut self, ui: &mut Ui, name: &str) -> bool {
        choice(ui, name, &self.names, &mut self.chosen).changed()
    }
}

// ---- style_preview.hpp -------------------------------------------------------------------------

/// The ground a preview is painted on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PreviewGround {
    /// White, pens by the paper colour rule.
    #[default]
    Paper,
    /// The viewport's dark ground.
    Screen,
}

/// How an unsaved definition is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DefinitionAs {
    Symbol,
    Linestyle,
}

/// What a preview was last asked to show.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Previewed {
    #[default]
    Nothing,
    /// A library symbol by name, at a size (0: the definition's own).
    Symbol { name: String, size: f64 },
    /// A library linestyle by name.
    Linestyle { name: String },
    /// A definition that is not in the Document's library, or not yet as it is given here.
    Definition {
        definition: Box<LineStyle>,
        shown_as: DefinitionAs,
    },
}

/// The preview pane as the three dialogs set it: what it shows, its plot scale and its ground.
#[derive(Clone, Debug, PartialEq)]
pub struct StylePreview {
    previewed: Previewed,
    denominator: i32,
    ground: PreviewGround,
}

impl Default for StylePreview {
    fn default() -> Self {
        Self::new()
    }
}

impl StylePreview {
    /// The plot scales offered, as the N of 1:N.
    pub const PLOT_SCALES: [i32; 7] = [100, 200, 250, 500, 1000, 2000, 5000];

    /// Nothing shown, at 1:500, on paper.
    pub fn new() -> Self {
        Self {
            previewed: Previewed::Nothing,
            denominator: 500,
            ground: PreviewGround::Paper,
        }
    }

    pub fn set_symbol(&mut self, name: &str, size: f64) {
        self.previewed = Previewed::Symbol {
            name: String::from(name),
            size,
        };
    }

    pub fn set_linestyle(&mut self, name: &str) {
        self.previewed = Previewed::Linestyle {
            name: String::from(name),
        };
    }

    pub fn set_definition(&mut self, definition: &LineStyle, shown_as: DefinitionAs) {
        self.previewed = Previewed::Definition {
            definition: Box::new(definition.clone()),
            shown_as,
        };
    }

    pub fn clear(&mut self) {
        self.previewed = Previewed::Nothing;
    }

    pub fn previewed(&self) -> &Previewed {
        &self.previewed
    }

    /// 1:N. Any positive N is accepted; anything else is ignored.
    pub fn set_scale_denominator(&mut self, denominator: i32) {
        if denominator > 0 {
            self.denominator = denominator;
        }
    }

    pub fn scale_denominator(&self) -> i32 {
        self.denominator
    }

    pub fn set_ground(&mut self, ground: PreviewGround) {
        self.ground = ground;
    }

    pub fn ground(&self) -> PreviewGround {
        self.ground
    }

    /// The pane, named `name` and at least `minimum` in size. Only its ground and frame are
    /// drawn here: what a definition draws is the real preview's to paint.
    pub fn ui(&self, ui: &mut Ui, name: &str, minimum: Vec2) -> Response {
        let (rect, response) = ui.allocate_exact_size(minimum, Sense::hover());
        if ui.is_rect_visible(rect) {
            let ground = match self.ground {
                PreviewGround::Paper => egui::Color32::WHITE,
                PreviewGround::Screen => palette::viewport().to_egui(),
            };
            ui.painter().rect_filled(rect, 0.0, ground);
        }
        custom(ui, &response, ControlRecord::new(name, ControlKind::Other));
        response
    }
}

// ---- linework_labels.hpp -----------------------------------------------------------------------

/// What a person reads each linework control code by, keyed by the name a file gives it
/// (entity's linework_code_members), in that list's order.
const LINEWORK_CONTROL_LABELS: [(&str, &str); 7] = [
    ("start", "Start"),
    ("end", "End"),
    ("close", "Close"),
    ("arcStart", "Begin curve"),
    ("arcEnd", "End curve"),
    ("join", "Join to point"),
    ("rectangle", "Rectangle"),
];

/// The label of the control a file calls `member`. A control the list does not know yet is shown
/// by the name a file gives it, rather than left out or left blank.
pub fn linework_control_label(member: &str) -> &str {
    LINEWORK_CONTROL_LABELS
        .iter()
        .find(|(name, _)| *name == member)
        .map_or(member, |(_, label)| *label)
}
