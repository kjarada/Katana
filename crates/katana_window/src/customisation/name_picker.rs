//! An editable choice for a linetype name or a symbol name - the one picker the style form, the
//! layer form, the symbol browser and the survey-code manager all use, so the QT-02 rule is kept
//! in one place. The port of customisation/name_picker.
//!
//! A PICKER NEVER DROPS THE NAME IT WAS GIVEN (decision D3). A name in no list - a library name
//! whose library is not loaded, a typo in an imported survey code file - is kept as a marked item,
//! "<name> (not defined)" in amber, whose tooltip says what it draws as meanwhile, and
//! `current_name` gives the name back exactly as it was set. The old choices rebuilt their list
//! and wrote whatever it showed back into the style, rewriting names they could not show.
//!
//! A linetype name that needs no definition is kept the same way but NOT marked: the plain
//! continuous line ("1", "0", "continuous" in any case; D4) and ByLayer in any spelling - exactly
//! the names cad's missing_names never reports, so the picker's mark and a manager's Missing count
//! agree.
//!
//! The list, in sections (header rows that cannot be chosen):
//!
//! ```text
//! linetype: [ByLayer - a Style's linetype only]; Drawing linetypes (the model's Linetype table);
//!           Library linestyles (non-vertex library definitions, decision D2)
//! symbol:   Built-in symbols; Library symbols (decision D3's four reasons)
//! ```
//!
//! each entry with a picture from the shared DefinitionThumbnails, and the kept undefined name,
//! when there is one, first of all. The choices come from cad's linetype_choices and
//! symbol_choices, so the picker and the catalogue verbs (STYLE SYMBOLS) cannot disagree.
//!
//! Names are case-SENSITIVE and kept as given; the completions' search folds case and matches
//! anywhere in a name, as a person looking for "kerb" expects. Only the search folds: a name typed
//! and finished is that name, in that case - "Kerb" typed is never swapped for a listed "KERB",
//! nor "KERB" for "Kerb".
//!
//! A CHOICE is the user's only - picking from the list, picking a highlighted completion, or
//! finishing typing (Enter, with the completions open or not, or leaving the field) with a name
//! different from the last one - and never `set_current_name` or `refresh`: a form applies an edit
//! when a person makes one, and a command is never run from a list's own change (docs/cad.md).
//! Where the C++ called `onNameChosen`, each of a person's acts here answers the name chosen, and
//! `ui` answers the one its frame saw.
//!
//! What is the C++'s and not here. The picker held the Document by pointer; here the Document is
//! lent to each method that reads it (`new`, `set_current_name`, `refresh`), and a dialog that may
//! outlive its Document calls none of them once its watcher says the Document has gone. The
//! completions are Qt's QCompleter popup in the C++; here they are the picker's own list, opened
//! by typing and drawn under the field, with the same rules for what Enter takes. A name is a
//! String, so a byte that is not UTF-8 cannot be given to a picker at all.

use egui::{Response, Ui};
use katana_cad::dashing::DashOptions;
use katana_cad::document::Document;
use katana_cad::style_catalogue::{
    CatalogueEntry, DefinitionSource, NameRole, linetype_choices, symbol_choices,
};
use katana_cad::style_resolver::{style_sample_drawing, style_sample_path};
use katana_cad::survey_coding::is_plain_linestyle;
use katana_entity::Style;
use katana_entity::display::{BY_LAYER_LINETYPE, is_by_layer};
use katana_entity::tables::built_in_symbol_for;

use super::customisation_context::{CustomisationContext, SharedThumbnails};
use super::definition_thumbnails::ThumbnailKind;
use super::style_painter::{
    CanvasRasterBackend, RasterBackend, StylePaintTarget, paint_style_drawing, painted_extent,
};
use crate::canvas::{Canvas, Color, Image, Pen, PenCap, PenJoin, PenStyle, RenderHint};
use crate::drive::controls::custom;
use crate::drive::mailbox::{Act, fill_disabled, no_button};
use crate::drive::registry::{ControlKind, ControlRecord, settle, take};
use crate::theme::palette;

/// The mark for a name nothing defines - in this picker, and wherever a manager's table shows one,
/// so "not defined" reads the same everywhere. A status colour, like the viewport's drawing
/// colours, not chrome: amber, between the theme's text and its error red, since a missing name is
/// drawn (by a fallback) and is not an error.
pub const UNDEFINED_NAME_COLOUR: Color = Color::rgb(0xE8, 0xA8, 0x3A);

/// Picture sizes, in logical pixels: a linestyle needs length to show its pattern, a symbol is
/// square.
const LINESTYLE_ICON_SIZE: (u32, u32) = (48, 14);
const SYMBOL_ICON_SIZE: (u32, u32) = (18, 18);
/// A model linetype's picture: this many periods, as the thumbnails show a library linestyle
/// (definition_thumbnails, SAMPLE_PERIODS).
const SAMPLE_PERIODS: f64 = 4.0;
const MARGIN_FRACTION: f64 = 0.12;
/// QCompleter::setMaxVisibleItems.
const MOST_COMPLETIONS_SHOWN: usize = 16;

fn undefined_label(name: &str) -> String {
    format!("{name} (not defined)")
}

/// The fold the completions' search compares by (QString::toCaseFolded in the C++).
fn folded(text: &str) -> String {
    text.to_lowercase()
}

/// A model Linetype along the style sample path, painted as the thumbnails paint a library
/// linestyle. The thumbnail cache cannot hold it: it keys on the LIBRARY's generation, and a model
/// linetype changes with the model.
fn model_linetype_image(
    document: &Document,
    name: &str,
    pixels: (u32, u32),
    ground: Color,
) -> Image {
    let mut image = Image::new(pixels.0, pixels.1);
    image.fill(ground);
    let period = document
        .model()
        .linetypes
        .find(name)
        .map_or(0.0, |linetype| linetype.pattern_length());
    let width = katana_math::cpp::max(SAMPLE_PERIODS * period, 1.0);
    let style = Style {
        name: String::from("sample"),
        linetype: String::from(name),
        ..Style::default()
    };
    let sample = style_sample_path(width);
    let dashes = DashOptions {
        view_scale: f64::from(pixels.0) / width,
        ..DashOptions::default()
    };
    let drawing = style_sample_drawing(
        document.model(),
        document.style_library(),
        &style,
        &sample,
        1.0,
        &dashes,
        0.0,
    );
    if drawing.empty() {
        return image;
    }
    let mut target = StylePaintTarget {
        entity_pen: Pen::with(
            palette::text(),
            1.5,
            PenStyle::Solid,
            PenCap::Flat,
            PenJoin::Bevel,
        ),
        // The session's own colour names, as every other picture of a Document's style gets them
        // (StylePaintTarget::colours). A model linetype is dashes and names no pen today, so
        // nothing here is coloured by it yet.
        colours: Some(&document.customisation_state().colours),
        ..StylePaintTarget::default()
    };
    target.view.resize(f64::from(pixels.0), f64::from(pixels.1));
    CanvasRasterBackend.paint_image(&mut image, &mut |canvas: &mut dyn Canvas| {
        canvas.set_render_hint(RenderHint::Antialiasing, true);
        let extent = painted_extent(&drawing, &canvas.font(), canvas.metrics());
        let mut fitted = target.clone();
        fitted.view.fit(&extent, MARGIN_FRACTION);
        paint_style_drawing(canvas, &drawing, &fitted);
    });
    image
}

/// One row of the list: an entry, or a section's header.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PickerItem {
    /// What the row says: the name, the kept name's decorated label, or a header's title.
    pub label: String,
    /// The exact name (the C++'s kNameRole); None on a header row.
    pub name: Option<String>,
    /// Where the name is defined (kSourceRole); None on a header row.
    pub source: Option<DefinitionSource>,
    /// The mark's colour on the kept undefined name; None for the theme's.
    pub foreground: Option<Color>,
    pub tool_tip: String,
    /// The entry's picture; None for a header, for ByLayer, and with no thumbnails.
    pub icon: Option<Image>,
}

impl PickerItem {
    /// A header row: shown, never chosen.
    pub fn is_header(&self) -> bool {
        self.name.is_none()
    }
}

/// See the module comment.
pub struct NamePicker {
    thumbnails: Option<SharedThumbnails>,
    role: NameRole,
    offer_by_layer: bool,
    items: Vec<PickerItem>,
    /// The names alone - no headers, no decorated label - which the search looks through.
    completions: Vec<String>,
    /// Row 0 of the list, and of the completions, is the kept name - marked when undefined, plain
    /// when a plain-line name.
    kept: bool,
    /// The row shown (QComboBox::currentIndex).
    current: Option<usize>,
    /// What the field reads (the line edit's text).
    text: String,
    /// The name last set or last reported, so a person finishing an edit that changed nothing
    /// does not choose it again.
    last_name: String,
    /// The completions are open, as typing opens them.
    completing: bool,
    /// The completion highlighted, by its place among those the text matches.
    highlighted: Option<usize>,
    /// The pictures as textures, by row; made when a list is first drawn.
    textures: Vec<Option<egui::TextureHandle>>,
    placeholder: String,
}

impl NamePicker {
    /// `role` picks the list. `offer_by_layer` adds ByLayer to a linetype list - a Style's
    /// linetype may inherit its layer's, a Layer's may not - and is ignored for symbols.
    /// `context.thumbnails` may be None (no pictures).
    pub fn new(
        context: &CustomisationContext,
        document: &Document,
        role: NameRole,
        offer_by_layer: bool,
    ) -> Self {
        let mut picker = Self {
            thumbnails: context.thumbnails.clone(),
            role,
            offer_by_layer: role == NameRole::Linetype && offer_by_layer,
            items: Vec::new(),
            completions: Vec::new(),
            kept: false,
            current: None,
            text: String::new(),
            last_name: String::new(),
            completing: false,
            highlighted: None,
            textures: Vec::new(),
            placeholder: String::from(Self::default_placeholder(role)),
        };
        picker.rebuild(document, "");
        picker
    }

    pub fn role(&self) -> NameRole {
        self.role
    }

    /// What the empty field says for a list of `role`, until a form says otherwise.
    pub fn default_placeholder(role: NameRole) -> &'static str {
        if role == NameRole::Symbol {
            "(no symbol)"
        } else {
            "(no linetype)"
        }
    }

    /// What the empty field says.
    pub fn placeholder_text(&self) -> &str {
        &self.placeholder
    }

    /// QLineEdit::setPlaceholderText: a form over several styles says "<varies>" here.
    pub fn set_placeholder_text(&mut self, text: &str) {
        self.placeholder = String::from(text);
    }

    // ---- the list ------------------------------------------------------------------------------

    /// The rows, headers included.
    pub fn items(&self) -> &[PickerItem] {
        &self.items
    }

    /// QComboBox::count.
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// QComboBox::itemText: what row `row` says; empty for no such row.
    pub fn item_text(&self, row: usize) -> String {
        self.items
            .get(row)
            .map(|item| item.label.clone())
            .unwrap_or_default()
    }

    /// QComboBox::findText: the first row saying exactly `text`, a header too.
    pub fn find_text(&self, text: &str) -> Option<usize> {
        self.items.iter().position(|item| item.label == text)
    }

    /// QComboBox::currentIndex.
    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    /// QComboBox::currentText of an editable choice: what the field reads - a listed item's
    /// decorated label, or what was typed.
    pub fn current_text(&self) -> &str {
        &self.text
    }

    /// The names offered, in list order, without the headers: the kept name first when there is
    /// one.
    pub fn names(&self) -> Vec<String> {
        self.items
            .iter()
            .filter_map(|item| item.name.clone())
            .collect()
    }

    /// Whether the field is in the mark's colour: the kept undefined name is the one shown.
    pub fn field_is_marked(&self) -> bool {
        self.current
            .and_then(|row| self.items.get(row))
            .is_some_and(|item| item.source == Some(DefinitionSource::Undefined))
    }

    /// The picker's tooltip: the row shown's, which for the kept name says what is drawn instead.
    pub fn tool_tip(&self) -> String {
        self.current
            .and_then(|row| self.items.get(row))
            .map(|item| item.tool_tip.clone())
            .unwrap_or_default()
    }

    // ---- the name ------------------------------------------------------------------------------

    /// Shows `name`, adding it first when no section holds it exactly: as the marked undefined
    /// item, or unmarked for a plain-line linetype name. "" shows nothing chosen (no symbol; for a
    /// linetype, nothing to keep). Never a choice.
    pub fn set_current_name(&mut self, document: &Document, name: &str) {
        // The whole list is rebuilt only by refresh(): a form sets its pickers on every row a
        // person clicks, and the list itself has not changed. The kept item is the one row that
        // follows the name.
        //
        // By the flag, not the row's source: a kept plain-line name is not Undefined, and row 0 of
        // a Style's list is otherwise ByLayer.
        if self.kept {
            self.items.remove(0);
            self.completions.remove(0);
            self.kept = false;
        }
        let listed = self
            .items
            .iter()
            .any(|item| item.name.as_deref() == Some(name));
        if !name.is_empty() && !listed {
            let item = self.entry_item(document, &self.kept_entry(name));
            self.items.insert(0, item);
            self.completions.insert(0, String::from(name));
            self.kept = true;
        }
        self.textures.clear();
        self.select_name(name);
        self.last_name = String::from(name);
        self.close_completions();
    }

    /// The name being shown, exactly: the name of a listed item (never its decorated label), or
    /// what was typed when it matches no item's text exactly (case-sensitively).
    pub fn current_name(&self) -> String {
        match self.row_for_text(&self.text) {
            Some(row) => self.items[row].name.clone().unwrap_or_default(),
            None => self.text.clone(),
        }
    }

    /// Whether `current_name` is defined by something - a listed name, or a plain-line linetype
    /// name, which needs no definition - rather than kept (the marked item) or typed. False for ""
    /// (nothing chosen). Agrees with cad's missing_names on the plain-line names, which it never
    /// reports.
    pub fn current_is_defined(&self) -> bool {
        if let Some(row) = self.row_for_text(&self.text) {
            return self.items[row].source != Some(DefinitionSource::Undefined);
        }
        // Typed, and listed nowhere: still defined when it is a plain-line name.
        self.is_plain_name(&self.current_name())
    }

    /// Rebuilds the list from the Document - after a library or survey map is loaded, or a model
    /// linetype is added - keeping the current name (marked if it is now undefined). Never a
    /// choice.
    pub fn refresh(&mut self, document: &Document) {
        let keep = self.current_name();
        self.rebuild(document, &keep);
    }

    fn rebuild(&mut self, document: &Document, keep: &str) {
        self.items.clear();
        self.completions.clear();
        self.textures.clear();
        self.current = None;
        self.text.clear();
        let choices = if self.role == NameRole::Symbol {
            symbol_choices(document)
        } else {
            linetype_choices(document, self.offer_by_layer)
        };
        let listed = choices.iter().any(|entry| entry.name == keep);
        self.kept = false;
        if !keep.is_empty() && !listed {
            let item = self.entry_item(document, &self.kept_entry(keep));
            self.items.push(item);
            self.completions.push(String::from(keep));
            self.kept = true;
        }
        // A section with no title has no header row: ByLayer, alone.
        let sections: &[(DefinitionSource, &str)] = if self.role == NameRole::Symbol {
            &[
                (DefinitionSource::BuiltIn, "Built-in symbols"),
                (DefinitionSource::Library, "Library symbols"),
            ]
        } else {
            &[
                (DefinitionSource::BuiltIn, ""),
                (DefinitionSource::ModelLinetype, "Drawing linetypes"),
                (DefinitionSource::Library, "Library linestyles"),
            ]
        };
        for &(source, title) in sections {
            if !choices.iter().any(|entry| entry.source == source) {
                continue;
            }
            if !title.is_empty() {
                // Neither enabled nor selectable: the list shows it and a click passes it by.
                self.items.push(PickerItem {
                    label: String::from(title),
                    ..PickerItem::default()
                });
            }
            for entry in choices.iter().filter(|entry| entry.source == source) {
                let item = self.entry_item(document, entry);
                self.items.push(item);
                self.completions.push(entry.name.clone());
            }
        }
        self.select_name(keep);
        self.last_name = String::from(keep);
        self.close_completions();
    }

    fn entry_item(&self, document: &Document, entry: &CatalogueEntry) -> PickerItem {
        let undefined = entry.source == DefinitionSource::Undefined;
        let noun = if self.role == NameRole::Symbol {
            "symbol"
        } else {
            "linestyle"
        };
        let tool_tip = match entry.source {
            DefinitionSource::Undefined => self.fallback_description(&entry.name),
            DefinitionSource::ModelLinetype => String::from("A linetype of this drawing"),
            DefinitionSource::BuiltIn => {
                if self.role == NameRole::Symbol {
                    String::from("A built-in symbol")
                } else if self.offer_by_layer && entry.name == BY_LAYER_LINETYPE {
                    // The listed ByLayer.
                    String::from("The linetype of the entity's layer")
                } else {
                    // A kept plain-line name.
                    self.plain_description(&entry.name)
                }
            }
            DefinitionSource::Library => {
                // A definition's source is the NAME of the customisation it came from; one made in
                // a session has none.
                let mut tip = if entry.source_file.is_empty() {
                    format!("Library {noun} made in this session")
                } else {
                    format!(
                        "Library {noun} from the customisation {}",
                        entry.source_file
                    )
                };
                if !entry.group.is_empty() {
                    tip.push_str(&format!(", group {}", entry.group));
                }
                if entry.collision {
                    tip.push_str(
                        ". Also a linetype of this drawing: the library's definition is the one \
                         drawn.",
                    );
                }
                tip
            }
        };
        PickerItem {
            label: if undefined {
                undefined_label(&entry.name)
            } else {
                entry.name.clone()
            },
            name: Some(entry.name.clone()),
            source: Some(entry.source),
            foreground: undefined.then_some(UNDEFINED_NAME_COLOUR),
            tool_tip,
            icon: self.icon_for(document, entry),
        }
    }

    fn select_name(&mut self, name: &str) {
        if name.is_empty() {
            self.current = None;
            self.text.clear();
            return;
        }
        match self
            .items
            .iter()
            .position(|item| item.name.as_deref() == Some(name))
        {
            Some(row) => self.set_current_row(row),
            None => {
                // Not reached through set_current_name or rebuild, which list the name first; kept
                // anyway so the field never shows something else.
                self.current = None;
                self.text = String::from(name);
            }
        }
    }

    /// QComboBox::setCurrentIndex: the row shown, and the field reading what it says.
    fn set_current_row(&mut self, row: usize) {
        self.current = Some(row);
        self.text = self.item_text(row);
    }

    /// The row whose item's text is exactly `text`, preferring the current row; None when none.
    fn row_for_text(&self, text: &str) -> Option<usize> {
        let says = |row: usize| {
            self.items
                .get(row)
                .is_some_and(|item| item.label == text && item.name.is_some())
        };
        if let Some(current) = self.current
            && says(current)
        {
            return Some(current);
        }
        (0..self.items.len()).find(|&row| says(row))
    }

    /// Called by every way a person can choose: the name, when it moved.
    fn chosen(&mut self) -> Option<String> {
        let name = self.current_name();
        if name == self.last_name {
            return None; // finished an edit that changed nothing
        }
        self.last_name.clone_from(&name);
        Some(name)
    }

    fn icon_for(&self, document: &Document, entry: &CatalogueEntry) -> Option<Image> {
        let thumbnails = self.thumbnails.as_ref()?;
        // The thumbnails take DEVICE pixels (definition_thumbnails); a list's pictures are made
        // before any frame says how many a point is, so one a point.
        let pixels = if self.role == NameRole::Symbol {
            SYMBOL_ICON_SIZE
        } else {
            LINESTYLE_ICON_SIZE
        };
        let ground = palette::raised();
        let image = if self.role == NameRole::Symbol {
            thumbnails
                .borrow_mut()
                .thumbnail_of(document, ThumbnailKind::Symbol, &entry.name, pixels, ground)
                .image
        } else if entry.source == DefinitionSource::ModelLinetype {
            model_linetype_image(document, &entry.name, pixels, ground)
        } else if entry.source == DefinitionSource::BuiltIn {
            return None; // ByLayer has no pattern of its own to show
        } else {
            thumbnails
                .borrow_mut()
                .thumbnail_of(
                    document,
                    ThumbnailKind::Linestyle,
                    &entry.name,
                    pixels,
                    ground,
                )
                .image
        };
        (!image.is_null()).then_some(image)
    }

    fn fallback_description(&self, name: &str) -> String {
        if self.role == NameRole::Symbol {
            return format!(
                "Not defined in the drawing or any loaded library: drawn as the built-in symbol \
                 \"{}\" until a library defines it.",
                built_in_symbol_for(name)
            );
        }
        String::from(
            "Not defined in the drawing or any loaded library: drawn as a solid line until a \
             library or a linetype defines it.",
        )
    }

    /// A linetype name that needs no definition, by cad's missing_names' rule (style_catalogue's
    /// plain linetype), built from the same two public halves so the two cannot drift: the plain
    /// continuous line ("1", "0", "continuous" in any case; D4), or ByLayer in any spelling. "" is
    /// nothing chosen, not a name.
    fn is_plain_name(&self, name: &str) -> bool {
        self.role == NameRole::Linetype
            && !name.is_empty()
            && (is_plain_linestyle(name) || is_by_layer(name))
    }

    /// The entry for a name no section lists: Undefined, or plain.
    fn kept_entry(&self, name: &str) -> CatalogueEntry {
        // A plain-line name needs nothing to define it, so it is kept as given but not marked:
        // marked, it would contradict missing_names, which never reports one, and the style
        // manager's Missing count with it.
        let plain = self.is_plain_name(name);
        CatalogueEntry {
            name: String::from(name),
            source: if plain {
                DefinitionSource::BuiltIn
            } else {
                DefinitionSource::Undefined
            },
            missing: !plain,
            ..CatalogueEntry::default()
        }
    }

    fn plain_description(&self, name: &str) -> String {
        if is_by_layer(name) {
            // entity's resolved linetype: a Style's ByLayer inherits; a Layer's has no layer above
            // it, and draws what a missing layer does.
            return if self.offer_by_layer {
                format!(
                    "ByLayer, spelt \"{name}\": the entity is drawn in its layer's linetype"
                )
            } else {
                format!(
                    "ByLayer, spelt \"{name}\": a layer has no layer above it to inherit from, so \
                     it is drawn as a plain line"
                )
            };
        }
        format!("\"{name}\" is the plain continuous line: it needs no definition")
    }

    // ---- what a person does --------------------------------------------------------------------

    /// The names the text typed is part of, in any case, in list order: what the completions
    /// offer.
    pub fn completions(&self) -> Vec<String> {
        let wanted = folded(&self.text);
        self.completions
            .iter()
            .filter(|name| folded(name).contains(&wanted))
            .cloned()
            .collect()
    }

    /// QCompleter::completionCount.
    pub fn completion_count(&self) -> usize {
        self.completions().len()
    }

    /// Whether the completions are open.
    pub fn completions_open(&self) -> bool {
        self.completing
    }

    /// Whether a completion is highlighted, which Enter then takes.
    pub fn completion_highlighted(&self) -> bool {
        self.highlighted.is_some()
    }

    /// The field made to read `text`, as a person selecting what it held and typing does: the
    /// completions open on what it is part of, nothing in them highlighted. Not yet a choice.
    pub fn type_text(&mut self, text: &str) {
        self.text = String::from(text);
        self.highlighted = None;
        self.completing = !text.is_empty() && self.completion_count() > 0;
    }

    /// The arrow onto completion `index` of those offered.
    pub fn highlight_completion(&mut self, index: usize) {
        if self.completing && index < self.completion_count() {
            self.highlighted = Some(index);
        }
    }

    /// Escape: the completions closed, what was typed left in the field.
    pub fn close_completions(&mut self) {
        self.completing = false;
        self.highlighted = None;
    }

    /// Enter. On a highlighted completion that completion is the choice; with nothing highlighted
    /// - the completions open or not - what was typed is. The name chosen, when it moved.
    pub fn press_enter(&mut self) -> Option<String> {
        let highlighted = if self.completing {
            self.highlighted
                .and_then(|index| self.completions().get(index).cloned())
        } else {
            None
        };
        self.close_completions();
        match highlighted {
            Some(name) => self.choose_completion(&name),
            None => self.finish_edit(),
        }
    }

    /// A click outside: the completions closed, and what was typed the choice. The name chosen,
    /// when it moved since the last.
    pub fn leave(&mut self) -> Option<String> {
        self.close_completions();
        self.finish_edit()
    }

    /// A row picked from the list (QComboBox::activated); a header is not picked.
    pub fn choose_row(&mut self, row: usize) -> Option<String> {
        if self.items.get(row).is_none_or(PickerItem::is_header) {
            return None;
        }
        self.close_completions();
        self.set_current_row(row);
        self.chosen()
    }

    /// A completion picked: the field reads the name, and its item is made current so
    /// `current_name` reads its name.
    fn choose_completion(&mut self, name: &str) -> Option<String> {
        self.text = String::from(name);
        if let Some(row) = self.row_for_text(name) {
            self.set_current_row(row);
        }
        self.chosen()
    }

    /// An edit finished. The choice box's own lookup first, exact and case-sensitive - a row
    /// saying just what was typed becomes the row shown - then the choice.
    fn finish_edit(&mut self) -> Option<String> {
        let shown = self.current.map(|row| self.item_text(row));
        if !self.text.is_empty()
            && shown.as_deref() != Some(self.text.as_str())
            && let Some(row) = self.find_text(&self.text)
        {
            self.current = Some(row);
        }
        self.chosen()
    }

    // ---- the frame -----------------------------------------------------------------------------

    /// Draws the picker as the control `name`: the field, the list behind its arrow and, while a
    /// person types, the completions under it. A fill writes the field as `--fill` does - the row
    /// saying the text is shown, else the text is kept as typed - and is no choice. The name a
    /// person chose this frame, if any.
    pub fn ui(&mut self, ui: &mut Ui, name: &str) -> Option<String> {
        let taken = take(ui, name);
        if let Some(taken) = &taken {
            match &taken.posted.act {
                Act::Fill(text) => {
                    if ui.is_visible() && !ui.is_enabled() {
                        settle(ui, taken, Err(fill_disabled(&taken.posted)));
                    } else {
                        match self.find_text(text) {
                            Some(row) => self.set_current_row(row),
                            None => self.text.clone_from(text),
                        }
                        self.close_completions();
                        settle(ui, taken, Ok(()));
                    }
                }
                Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
                Act::ScrollTo => {}
            }
        }

        let mut chosen = None;
        let mut picked: Option<usize> = None;
        let mut completion: Option<String> = None;
        let marked = self.field_is_marked();
        let placeholder = self.placeholder.clone();
        let before = self.text.clone();
        let shown = ui.vertical(|ui| {
            let row = ui.horizontal(|ui| {
                let mut edit = egui::TextEdit::singleline(&mut self.text)
                    .id_salt(name)
                    .hint_text(placeholder);
                if marked {
                    edit = edit.text_color(UNDEFINED_NAME_COLOUR.to_egui());
                }
                let field = ui.add(edit);
                let list = egui::ComboBox::from_id_salt((name, "items"))
                    .selected_text("")
                    .show_ui(ui, |ui| {
                        self.ensure_textures(ui, name);
                        for (at, item) in self.items.iter().enumerate() {
                            if item.is_header() {
                                ui.strong(item.label.as_str());
                                continue;
                            }
                            let clicked = ui
                                .horizontal(|ui| {
                                    if let Some(Some(texture)) = self.textures.get(at) {
                                        let size = texture.size_vec2();
                                        let (rect, _) =
                                            ui.allocate_exact_size(size, egui::Sense::hover());
                                        ui.painter().image(
                                            texture.id(),
                                            rect,
                                            egui::Rect::from_min_max(
                                                egui::pos2(0.0, 0.0),
                                                egui::pos2(1.0, 1.0),
                                            ),
                                            egui::Color32::WHITE,
                                        );
                                    }
                                    let mut label = egui::RichText::new(item.label.as_str());
                                    if let Some(colour) = item.foreground {
                                        label = label.color(colour.to_egui());
                                    }
                                    let mut response =
                                        ui.selectable_label(self.current == Some(at), label);
                                    if !item.tool_tip.is_empty() {
                                        response = response.on_hover_text(item.tool_tip.as_str());
                                    }
                                    response.clicked()
                                })
                                .inner;
                            if clicked {
                                picked = Some(at);
                            }
                        }
                    });
                (field, list.response)
            });
            let (field, list) = row.inner;
            if self.completing {
                for (at, offered) in self
                    .completions()
                    .into_iter()
                    .take(MOST_COMPLETIONS_SHOWN)
                    .enumerate()
                {
                    if ui
                        .selectable_label(self.highlighted == Some(at), offered.as_str())
                        .clicked()
                    {
                        completion = Some(offered);
                    }
                }
            }
            (field, list)
        });
        let (field, list) = shown.inner;

        if self.text != before {
            // Typed by a person this frame: the completions open on it.
            let typed = self.text.clone();
            self.type_text(&typed);
        }
        if let Some(offered) = completion {
            self.close_completions();
            chosen = self.choose_completion(&offered);
        } else if let Some(row) = picked {
            chosen = self.choose_row(row);
        } else if field.lost_focus() {
            // Enter, or a click elsewhere: either finishes the edit.
            chosen = self.leave();
        }

        let mut response: Response = field.union(list);
        let tip = self.tool_tip();
        if !tip.is_empty() {
            response = response.on_hover_text(tip);
        }
        if let Some(taken) = &taken
            && taken.posted.act == Act::ScrollTo
        {
            response.scroll_to_me(None);
            settle(ui, taken, Ok(()));
        }
        let mut record = ControlRecord::new(name, ControlKind::Choice);
        record.text.clone_from(&self.placeholder);
        record.value.clone_from(&self.text);
        record.items = self.items.iter().map(|item| item.label.clone()).collect();
        custom(ui, &response, record);
        chosen
    }

    /// The rows' pictures as textures, made once for a list.
    fn ensure_textures(&mut self, ui: &Ui, name: &str) {
        if self.textures.len() == self.items.len() {
            return;
        }
        self.textures = self
            .items
            .iter()
            .enumerate()
            .map(|(at, item)| {
                item.icon.as_ref().map(|icon| {
                    ui.ctx().load_texture(
                        format!("{name}-{at}"),
                        icon.to_color_image(),
                        egui::TextureOptions::LINEAR,
                    )
                })
            })
            .collect();
    }
}
