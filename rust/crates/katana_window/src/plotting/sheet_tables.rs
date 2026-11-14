//! The sheet tables as they are drawn (docs/plotting.md, "The drawing register and the revision
//! table"). The port of plotting/sheet_tables.
//!
//! The model lays a table out (cad's tables) and the sheet painter draws it. What the window adds
//! is the one thing the model cannot know: the scale an automatic plan is drawn at, which needs
//! the drawing's extent. The register decides those first, as the painter does for a title block,
//! so the register and every sheet's title block report the same scale.

use katana_cad::plotting::sheet_set::{SheetSet, ViewportKind};
use katana_cad::plotting::tables::{RegisterRow, drawing_register};

use crate::sheet_painter::{
    SheetPaintCache, SheetSource, resolve_plan_viewport_on_sheet, resolve_section_viewport,
};

/// `set` with every automatic plan and key-plan scale and centre decided
/// (resolve_plan_viewport_on_sheet), every automatic section's scale and exaggeration fitted
/// (resolve_section_viewport, cutting through `cache` when one is given), and the viewports
/// marked automatic no longer: the set as the painter draws it.
pub fn resolved_sheet_set(
    set: &SheetSet,
    source: &SheetSource<'_>,
    cache: Option<&mut SheetPaintCache>,
) -> SheetSet {
    let mut own = SheetPaintCache::new();
    let sections = match cache {
        Some(cache) => cache,
        None => &mut own,
    };
    let mut drawn = set.clone();
    for (s, sheet) in drawn.sheets.iter_mut().enumerate() {
        for viewport in &mut sheet.viewports {
            let plan_like =
                viewport.kind == ViewportKind::Plan || viewport.kind == ViewportKind::KeyPlan;
            let section = viewport.kind == ViewportKind::LongSection
                || viewport.kind == ViewportKind::CrossSections;
            if plan_like && (viewport.auto_scale || viewport.auto_centre) {
                // With the set and the sheet, as the painter resolves it: an automatic key plan
                // fits the sheets' outlines, not the drawing.
                let at = resolve_plan_viewport_on_sheet(viewport, source, set, s);
                viewport.scale = at.scale;
                viewport.centre = at.centre;
                viewport.auto_scale = false;
                viewport.auto_centre = false;
            } else if section && viewport.auto_scale {
                // The scale and exaggeration the section is fitted to (cad's section_fit), so a
                // register row says "H 1:1000 V 1:100" as its title block does.
                let fit = resolve_section_viewport(viewport, source, sections);
                viewport.scale = fit.scale;
                viewport.vertical_exaggeration = fit.exaggeration;
                viewport.auto_scale = false;
            }
        }
    }
    drawn
}

/// The drawing register as it plots: drawing_register of resolved_sheet_set.
pub fn drawn_register(set: &SheetSet, source: &SheetSource<'_>) -> Vec<RegisterRow> {
    drawing_register(&resolved_sheet_set(set, source, None))
}
