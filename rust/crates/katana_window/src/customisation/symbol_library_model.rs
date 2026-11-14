//! The symbol library's grid: one item per symbol cad's symbol_library lists, shown as a 64-pixel
//! picture captioned with its name, with a badge for the entities in the drawing that draw it, and
//! filtered by the three things a person narrows a library by - a group in the tree, a chip (All,
//! In drawing, Used by codes, Missing, At vertices) and a search over name, group and survey code.
//! The port of customisation/symbol_library_model.
//!
//! A list model of its own rather than a filter over one: the filter is the whole of what it does,
//! and it is over at most a few thousand entries, so refiltering at each change is simpler and
//! costs nothing a person could notice. Read-only (docs/cad.md: a list shows, the form beside it
//! acts).
//!
//! Pictures come from the dialog (`picture`), which knows whether the Document is still there and
//! which DefinitionThumbnails to ask; the model adds the badge and the missing mark, and keeps
//! each composed picture until the entries change.
//!
//! What the C++ answered by a Qt item role is a method here: `display`, `decoration`, `tool_tip`,
//! `name`, `uses` and `is_missing`, each of the row shown.

use std::cell::RefCell;
use std::collections::BTreeMap;

use katana_cad::style_catalogue::DefinitionSource;
use katana_cad::symbol_assign::SymbolLibraryEntry;

use super::codes_seams::UNDEFINED_NAME_COLOUR;
use crate::canvas::{
    Brush, Canvas, Color, Font, FontSize, FontWeight, Image, Pen, PenCap, PenJoin, PenStyle,
    RasterCanvas, RectF, RenderHint, TextFlags,
};
use crate::format::grouped;
use crate::theme::palette;

/// The chips, in the order the filter bar shows them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SymbolChip {
    #[default]
    All,
    InDrawing,
    UsedByCodes,
    Missing,
    VertexMode,
}

impl SymbolChip {
    /// The chips in the bar's order.
    pub const ALL: [SymbolChip; 5] = [
        SymbolChip::All,
        SymbolChip::InDrawing,
        SymbolChip::UsedByCodes,
        SymbolChip::Missing,
        SymbolChip::VertexMode,
    ];
}

/// What the group tree narrows to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SymbolGroupKind {
    /// Everything listed.
    #[default]
    All,
    /// Katana's own shapes.
    BuiltIn,
    /// Library definitions with no group.
    Ungrouped,
    /// Names nothing defines.
    Missing,
    /// A library group and every group below it.
    Group,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SymbolGroupFilter {
    pub kind: SymbolGroupKind,
    /// Group only: "Test" or "Test/Marks".
    pub path: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SymbolFilter {
    /// Folded when compared: names are case-sensitive, a search is not (D3).
    pub text: String,
    pub chip: SymbolChip,
    pub group: SymbolGroupFilter,
}

fn contains_folded(haystack: &str, folded_needle: &str) -> bool {
    haystack.to_lowercase().contains(folded_needle)
}

fn in_group(entry: &SymbolLibraryEntry, group: &SymbolGroupFilter) -> bool {
    let item = &entry.entry;
    match group.kind {
        SymbolGroupKind::All => true,
        SymbolGroupKind::BuiltIn => item.source == DefinitionSource::BuiltIn,
        SymbolGroupKind::Ungrouped => {
            item.source == DefinitionSource::Library && item.group.is_empty()
        }
        SymbolGroupKind::Missing => item.missing,
        // The group itself and everything below it: choosing "Test" in the tree shows Test/Marks
        // and Test/Vegetation too, as a folder does.
        SymbolGroupKind::Group => {
            item.source == DefinitionSource::Library
                && (item.group == group.path
                    || (item.group.len() > group.path.len()
                        && item.group.starts_with(group.path.as_str())
                        && item.group.as_bytes()[group.path.len()] == b'/'))
        }
    }
}

fn on_chip(entry: &SymbolLibraryEntry, chip: SymbolChip) -> bool {
    match chip {
        SymbolChip::All => true,
        SymbolChip::InDrawing => entry.entry.users.used(),
        SymbolChip::UsedByCodes => !entry.codes.is_empty(),
        SymbolChip::Missing => entry.entry.missing,
        SymbolChip::VertexMode => entry.entry.at_vertices,
    }
}

/// Whether one entry passes. Text matches the name, the group, or the key of a survey code that
/// draws it, anywhere and with case folded.
pub fn symbol_matches(entry: &SymbolLibraryEntry, filter: &SymbolFilter) -> bool {
    if !in_group(entry, &filter.group) || !on_chip(entry, filter.chip) {
        return false;
    }
    let needle = filter.text.trim().to_lowercase();
    if needle.is_empty() {
        return true;
    }
    contains_folded(&entry.entry.name, &needle)
        || contains_folded(&entry.entry.group, &needle)
        || entry
            .codes
            .iter()
            .any(|code| contains_folded(&code.key, &needle))
}

/// The entities in the drawing that draw the symbol - points and lines wearing a style that names
/// it - which is what the badge counts.
pub fn symbol_uses(entry: &SymbolLibraryEntry) -> usize {
    entry.entry.users.entities
}

/// The picture of a name, PICTURE_SIZE across; a null image draws none.
pub type PictureOf = Box<dyn Fn(&SymbolLibraryEntry) -> Image>;

/// The grid's rows (see the module comment).
#[derive(Default)]
pub struct SymbolGridModel {
    /// The picture of a name; None draws none.
    pub picture: Option<PictureOf>,
    entries: Vec<SymbolLibraryEntry>,
    /// Indices into `entries`, in order.
    shown: Vec<usize>,
    filter: SymbolFilter,
    pictures: RefCell<BTreeMap<usize, Image>>,
}

impl SymbolGridModel {
    /// Device-independent pixels.
    pub const PICTURE_SIZE: u32 = 64;

    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces everything listed; the filter is kept and applied.
    pub fn set_entries(&mut self, entries: Vec<SymbolLibraryEntry>) {
        self.entries = entries;
        self.pictures.borrow_mut().clear();
        self.apply_filter();
    }

    pub fn set_filter(&mut self, filter: SymbolFilter) {
        self.filter = filter;
        self.apply_filter();
    }

    pub fn filter(&self) -> &SymbolFilter {
        &self.filter
    }

    pub fn entries(&self) -> &[SymbolLibraryEntry] {
        &self.entries
    }

    fn apply_filter(&mut self) {
        self.shown = (0..self.entries.len())
            .filter(|&index| symbol_matches(&self.entries[index], &self.filter))
            .collect();
    }

    /// The entry shown at `row`; None out of range.
    pub fn entry_at(&self, row: usize) -> Option<&SymbolLibraryEntry> {
        self.shown.get(row).map(|&index| &self.entries[index])
    }

    /// The row showing exactly `name`.
    pub fn row_of(&self, name: &str) -> Option<usize> {
        self.shown
            .iter()
            .position(|&index| self.entries[index].entry.name == name)
    }

    /// How many entries pass the filter with its chip replaced by `chip`: the count a chip's
    /// label shows.
    pub fn count_for(&self, chip: SymbolChip) -> usize {
        let filter = SymbolFilter {
            chip,
            ..self.filter.clone()
        };
        self.entries
            .iter()
            .filter(|entry| symbol_matches(entry, &filter))
            .count()
    }

    pub fn row_count(&self) -> usize {
        self.shown.len()
    }

    /// Qt::DisplayRole: the name.
    pub fn display(&self, row: usize) -> Option<String> {
        self.entry_at(row).map(|entry| entry.entry.name.clone())
    }

    /// kNameRole: the exact name.
    pub fn name(&self, row: usize) -> Option<&str> {
        self.entry_at(row).map(|entry| entry.entry.name.as_str())
    }

    /// kUsesRole: symbol_uses.
    pub fn uses(&self, row: usize) -> Option<usize> {
        self.entry_at(row).map(symbol_uses)
    }

    /// kMissingRole: nothing defines it. (Its caption is then drawn in the picker's amber, the
    /// C++'s ForegroundRole.)
    pub fn is_missing(&self, row: usize) -> Option<bool> {
        self.entry_at(row).map(|entry| entry.entry.missing)
    }

    /// Qt::DecorationRole: the picture with its badge and missing mark.
    pub fn decoration(&self, row: usize) -> Option<Image> {
        self.shown.get(row).map(|&index| self.composed(index, None))
    }

    /// The same, the picture asked of `picture` where it is not kept yet: for a dialog whose
    /// pictures are of a Document it is only lent, which `picture` cannot keep.
    pub fn decoration_with(
        &self,
        row: usize,
        picture: &dyn Fn(&SymbolLibraryEntry) -> Image,
    ) -> Option<Image> {
        self.shown
            .get(row)
            .map(|&index| self.composed(index, Some(picture)))
    }

    /// Qt::ToolTipRole.
    pub fn tool_tip(&self, row: usize) -> Option<String> {
        self.entry_at(row).map(tool_tip_of)
    }

    fn composed(
        &self,
        index: usize,
        lent: Option<&dyn Fn(&SymbolLibraryEntry) -> Image>,
    ) -> Image {
        if let Some(found) = self.pictures.borrow().get(&index) {
            return found.clone();
        }
        let entry = &self.entries[index];
        let mut image = match (lent, &self.picture) {
            (Some(picture), _) => picture(entry),
            (None, Some(picture)) => picture(entry),
            (None, None) => Image::default(),
        };
        if image.is_null() {
            image = Image::new(Self::PICTURE_SIZE, Self::PICTURE_SIZE);
            image.fill(Color::TRANSPARENT);
        }
        // Logical pixels: a picture painted for a HiDPI screen carries its device pixel ratio,
        // and the canvas works in logical units on it.
        let ratio = image.device_pixel_ratio();
        let width = f64::from(image.width()) / ratio;
        let height = f64::from(image.height()) / ratio;
        let missing = entry.entry.missing;
        let uses = symbol_uses(entry);
        RasterCanvas::paint_image(&mut image, &mut |canvas: &mut dyn Canvas| {
            canvas.set_render_hint(RenderHint::Antialiasing, true);
            if missing {
                // The picker's amber: a name nothing defines looks the same in every manager.
                canvas.set_pen(Pen::with(
                    UNDEFINED_NAME_COLOUR,
                    2.0,
                    PenStyle::Solid,
                    PenCap::Square,
                    PenJoin::Bevel,
                ));
                canvas.set_brush(Brush::none());
                canvas.draw_rect(RectF::new(1.0, 1.0, width - 2.0, height - 2.0));
            }
            if uses > 0 {
                // The count in the top-right corner, as a palette shows how many references a
                // block has: readable at a glance, never covering the middle of the mark.
                let font = Font {
                    size: FontSize::Pixels(10),
                    weight: FontWeight::Bold,
                    ..canvas.font()
                };
                canvas.set_font(font.clone());
                let text = if uses > 999 {
                    String::from("999+")
                } else {
                    format!("{uses}")
                };
                let advance = canvas.metrics().horizontal_advance(&font, &text).round();
                let badge_width = (advance + 8.0).max(16.0);
                let badge = RectF::new(width - badge_width - 2.0, 2.0, badge_width, 14.0);
                canvas.set_pen(Pen::none());
                canvas.set_brush(Brush::solid(palette::accent()));
                canvas.draw_rounded_rect(badge, 7.0, 7.0);
                canvas.set_pen(Pen::new(palette::accent_text()));
                canvas.draw_text_in(badge, TextFlags::CENTER, &text);
            }
        });
        self.pictures.borrow_mut().insert(index, image.clone());
        image
    }
}

fn tool_tip_of(entry: &SymbolLibraryEntry) -> String {
    let mut lines = vec![entry.entry.name.clone()];
    match entry.entry.source {
        DefinitionSource::Library => {
            lines.push(if entry.entry.group.is_empty() {
                String::from("(ungrouped)")
            } else {
                entry.entry.group.clone()
            });
            if !entry.entry.source_file.is_empty() {
                lines.push(format!("From {}", entry.entry.source_file));
            }
        }
        DefinitionSource::BuiltIn => {
            lines.push(String::from("Built in: Katana draws it without a library"));
        }
        DefinitionSource::Undefined | DefinitionSource::ModelLinetype => {
            lines.push(format!(
                "Not defined - drawn as the built-in \"{}\"",
                entry.fallback
            ));
        }
    }
    let uses = symbol_uses(entry);
    if uses > 0 {
        lines.push(format!(
            "Drawn by {} {} in the drawing",
            grouped(uses as u64),
            if uses == 1 { "entity" } else { "entities" }
        ));
    }
    if !entry.codes.is_empty() {
        let keys: Vec<&str> = entry.codes.iter().map(|code| code.key.as_str()).collect();
        lines.push(format!("Survey codes: {}", keys.join(", ")));
    }
    lines.join("\n")
}
