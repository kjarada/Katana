//! Small pictures of the sheets for the sheet list (docs/plotting.md, "Editing on the canvas").
//! Each is the sheet painter's own paint of the sheet at thumbnail size - the same function that
//! plots it - kept until something it shows changes. The port of plotting/sheet_thumbnails.
//!
//! ```text
//! the sheet itself, its position and the number of sheets (its number and the numbers its match
//! lines and key plan print, overridden or not), the order of the sheets,
//! the title-block values the sheets share, the project's field values, and
//! - only for a sheet that shows the drawing - the drawing's revision,
//! - only for a sheet that shows the other sheets (a drawing register, a key plan, a legend of
//!   the whole set) - every sheet.
//! ```
//!
//! So a sheet of notes is not painted again because a line was drawn, and a set of a hundred
//! sheets costs one paint for the sheet that was edited. The editor paints the stale ones a few at
//! a time while it is idle, the old picture standing in until then.

use std::collections::{BTreeMap, BTreeSet};

use katana_cad::plot::paper_dimensions;
use katana_cad::plotting::sheet_set::{
    FieldContext, LegendScope, Revision, Sheet, SheetDefaults, SheetSet, ViewportKind, mark_label,
};

use crate::canvas::{Brush, Canvas, Color, Image, Pen, PointF, RasterCanvas, RectF, RenderHint};
use crate::sheet_painter::{SheetPaintCache, SheetPaintOptions, SheetSource, paint_sheet};

/// The project's contribution to the title block, as one string to compare.
fn fields_text(fields: &FieldContext) -> String {
    let mut text = String::new();
    for part in [
        &fields.project_name,
        &fields.project_description,
        &fields.coordinate_system,
        &fields.file_name,
        &fields.plot_date,
    ] {
        text.push_str(part);
        text.push('\x1f');
    }
    text
}

/// What the marks of `sheet` print, as mark_label writes them in `set`.
fn mark_labels_text(set: &SheetSet, sheet: &Sheet) -> String {
    let mut text = String::new();
    for viewport in &sheet.viewports {
        for mark in &viewport.marks {
            text.push_str(&mark_label(set, mark));
            text.push('\x1f');
        }
    }
    text
}

fn same_order(order: &[String], set: &SheetSet) -> bool {
    order.len() == set.sheets.len()
        && order
            .iter()
            .zip(&set.sheets)
            .all(|(id, sheet)| *id == sheet.id)
}

/// Whether any viewport of `sheet` shows the drawing (a plan, a section, a 3D snapshot, a legend
/// of the layers it draws), so its picture goes stale with the drawing.
pub fn shows_the_drawing(sheet: &Sheet) -> bool {
    sheet.viewports.iter().any(|viewport| match viewport.kind {
        // Revisions: the set's revisions, in the key already.
        ViewportKind::Notes | ViewportKind::Image | ViewportKind::Revisions => false,
        // SheetIndex: an automatic plan's scale is the drawing's.
        ViewportKind::Plan
        | ViewportKind::LongSection
        | ViewportKind::CrossSections
        | ViewportKind::Model3D
        | ViewportKind::Legend
        | ViewportKind::KeyPlan
        | ViewportKind::SheetIndex => true,
    })
}

/// Whether any viewport of `sheet` shows what the other sheets hold: a drawing register (their
/// names, scales, papers), a key plan (their plans' outlines), a legend of the whole set (what
/// their plans show).
pub fn shows_the_other_sheets(sheet: &Sheet) -> bool {
    sheet.viewports.iter().any(|viewport| {
        viewport.kind == ViewportKind::SheetIndex
            || viewport.kind == ViewportKind::KeyPlan
            || (viewport.kind == ViewportKind::Legend
                && viewport.legend_scope == LegendScope::WholeSet)
    })
}

/// What a picture was painted from. The drawing's revision counts only for a sheet that shows the
/// drawing.
#[derive(Clone, Debug)]
struct Key {
    sheet: Sheet,
    index: usize,
    order: Vec<String>,
    defaults: SheetDefaults,
    numbering: String,
    revisions: Vec<Revision>,
    fields: String,
    revision: u64,
    pixel_ratio: f64,
    /// The labels its match lines and key-plan outlines print: they carry the number of the sheet
    /// they lead to, which that sheet's own sheet_number override can change.
    mark_labels: String,
    /// Every sheet, for a sheet that lists or outlines the others (shows_the_other_sheets); empty
    /// for the rest.
    sheets: Vec<Sheet>,
}

#[derive(Clone, Debug)]
struct Entry {
    key: Key,
    image: Image,
}

pub struct SheetThumbnails {
    box_size: (i32, i32),
    entries: BTreeMap<String, Entry>,
    cache: SheetPaintCache,
    renders: i32,
}

impl Default for SheetThumbnails {
    fn default() -> Self {
        Self::new((112, 80))
    }
}

impl SheetThumbnails {
    /// Pictures fitted inside `box_size` logical pixels (112 x 80 in the C++), the paper's shape
    /// kept.
    pub fn new(box_size: (i32, i32)) -> Self {
        Self {
            box_size,
            entries: BTreeMap::new(),
            cache: SheetPaintCache::new(),
            renders: 0,
        }
    }

    pub fn box_size(&self) -> (i32, i32) {
        self.box_size
    }

    fn matches(key: &Key, set: &SheetSet, index: usize, source: &SheetSource<'_>) -> bool {
        let sheet = &set.sheets[index];
        key.index == index
            && key.sheet == *sheet
            && same_order(&key.order, set)
            && key.defaults == set.defaults
            && key.numbering == set.numbering
            && key.revisions == set.revisions
            && key.fields == fields_text(&source.fields)
            && key.mark_labels == mark_labels_text(set, sheet)
            && (!shows_the_drawing(sheet) || key.revision == source.revision)
            && (!shows_the_other_sheets(sheet) || key.sheets == set.sheets)
    }

    /// Whether sheet `index` of `set` has no picture, or one of something that has changed since,
    /// for a drawing at `source`.
    pub fn is_stale(&self, set: &SheetSet, index: usize, source: &SheetSource<'_>) -> bool {
        let Some(sheet) = set.sheets.get(index) else {
            return false;
        };
        self.entries
            .get(&sheet.id)
            .is_none_or(|entry| !Self::matches(&entry.key, set, index, source))
    }

    /// How many sheets of `set` are stale.
    pub fn stale_count(&self, set: &SheetSet, source: &SheetSource<'_>) -> usize {
        (0..set.sheets.len())
            .filter(|&index| self.is_stale(set, index, source))
            .count()
    }

    /// The picture of sheet `index`, painted now when it is stale, at `pixel_ratio` device pixels
    /// per logical pixel. Null for an index past the end.
    pub fn thumbnail(
        &mut self,
        set: &SheetSet,
        index: usize,
        source: &SheetSource<'_>,
        pixel_ratio: f64,
    ) -> Image {
        let Some(sheet) = set.sheets.get(index) else {
            return Image::default();
        };
        if let Some(entry) = self.entries.get(&sheet.id)
            && entry.key.pixel_ratio == pixel_ratio
            && Self::matches(&entry.key, set, index, source)
        {
            return entry.image.clone();
        }
        let ratio = if pixel_ratio > 0.0 { pixel_ratio } else { 1.0 };
        let paper = paper_dimensions(sheet.paper, sheet.landscape);
        let (box_width, box_height) = (f64::from(self.box_size.0), f64::from(self.box_size.1));
        // The paper's shape inside the box, a pixel clear of its edge for the outline.
        let logical = katana_math::cpp::min(
            (box_width - 2.0) / paper.width_mm,
            (box_height - 2.0) / paper.height_mm,
        );
        let ppmm = logical * ratio;
        let width = (box_width * ratio).round().max(0.0) as u32;
        let height = (box_height * ratio).round().max(0.0) as u32;
        // A new image is transparent.
        let mut image = Image::new(width, height);
        let options = SheetPaintOptions {
            pixels_per_millimetre: ppmm,
            origin: PointF::new(
                (f64::from(width) - paper.width_mm * ppmm) / 2.0,
                (f64::from(height) - paper.height_mm * ppmm) / 2.0,
            ),
            // A thumbnail's 3D snapshot is a few dozen pixels: never render more.
            raster_dpi_cap: 25.0,
            raster_pixel_cap: 60_000,
            ..SheetPaintOptions::default()
        };
        let cache = &mut self.cache;
        RasterCanvas::paint_image(&mut image, &mut |canvas: &mut dyn Canvas| {
            canvas.set_render_hint(RenderHint::Antialiasing, true);
            let _ = paint_sheet(canvas, set, index, source, &options, cache);
            canvas.set_pen(Pen {
                color: Color::rgb(120, 122, 128),
                width: 1.0,
                ..Pen::default()
            });
            canvas.set_brush(Brush::none());
            canvas.draw_rect(
                RectF::new(
                    options.origin.x,
                    options.origin.y,
                    paper.width_mm * ppmm,
                    paper.height_mm * ppmm,
                )
                .adjusted(0.5, 0.5, -0.5, -0.5),
            );
        });
        image.set_device_pixel_ratio(ratio);
        self.renders += 1;

        let key = Key {
            sheet: sheet.clone(),
            index,
            order: set.sheets.iter().map(|each| each.id.clone()).collect(),
            defaults: set.defaults.clone(),
            numbering: set.numbering.clone(),
            revisions: set.revisions.clone(),
            fields: fields_text(&source.fields),
            revision: source.revision,
            pixel_ratio,
            mark_labels: mark_labels_text(set, sheet),
            sheets: if shows_the_other_sheets(sheet) {
                set.sheets.clone()
            } else {
                Vec::new()
            },
        };
        self.entries.insert(
            sheet.id.clone(),
            Entry {
                key,
                image: image.clone(),
            },
        );
        image
    }

    /// The last picture painted of the sheet with id `sheet_id`, stale or not; null when there is
    /// none.
    pub fn cached(&self, sheet_id: &str) -> Image {
        self.entries
            .get(sheet_id)
            .map(|entry| entry.image.clone())
            .unwrap_or_default()
    }

    /// Forgets the pictures of sheets that are no longer in `set`.
    pub fn prune(&mut self, set: &SheetSet) {
        let ids: BTreeSet<&str> = set.sheets.iter().map(|sheet| sheet.id.as_str()).collect();
        self.entries.retain(|id, _| ids.contains(id.as_str()));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.cache.clear();
    }

    /// How many pictures have been painted, for tests of the cache.
    pub fn renders(&self) -> i32 {
        self.renders
    }
}
