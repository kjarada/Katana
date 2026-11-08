//! Arranging a project's sheets (docs/plotting.md, "Arranging a sheet";
//! include/katana/cad/plotting/arrange_commands.hpp): the functions of
//! arrange.rs, each made ONE undoable step on a document through edit_sheet,
//! edit_viewport or edit_sheet_set (sheet_commands.rs). What the sheet
//! editor's Arrange menu calls, and what a command-line verb calls: the same
//! step, the same words in the history, whoever asked.
//!
//! A function that needs to know what a view shows takes it as `content`,
//! world points (arrange.rs). Left empty, it is gathered from the document's
//! drawing (viewport_content_in_set); the editor passes its own, which adds the
//! window's reference layers and meshes. An edit that changes nothing records
//! no step; a refused one changes nothing.
//!
//! The functions that share a C++ name with arrange.rs's (align_viewports,
//! distribute_viewports, match_scale, fit_viewport_to_content,
//! rotate_to_best_fit) are the C++ overloads that take a Document; the two
//! modules keep them apart. The C++'s defaulted `content` is an empty slice
//! here and its defaulted optionals are None.

use katana_core::error::make_error;
use katana_core::{ErrorCode, Result, Status};
use katana_geometry::{Box2, Point2};
use katana_math::cpp;

use crate::document::Document;
use crate::plot::sheet_scale_at_least;
use crate::plotting::arrange::{
    self, AUTO_SCALE_SPARE, AlignEdge, ArrangeResult, DistributeAxis, PaperChange, RotationFit,
    auto_arrange, fit_at_rotation, viewport_content_in_set,
};
use crate::plotting::layout::{SizeMm, TILING_GUTTER_MM};
use crate::plotting::sheet_commands::{edit_sheet, edit_sheet_set, edit_viewport};
use crate::plotting::sheet_set::{SheetSet, Viewport, ViewportKind, drawing_area};

/// The step names these record, for a history to show and a test to find.
pub const AUTO_ARRANGE_STEP: &str = "AUTO_ARRANGE";
pub const ALIGN_STEP: &str = "ALIGN_VIEWPORTS";
pub const DISTRIBUTE_STEP: &str = "DISTRIBUTE_VIEWPORTS";
pub const MATCH_SCALE_STEP: &str = "MATCH_SCALE";
pub const FIT_TO_CONTENT_STEP: &str = "FIT_VIEWPORT_TO_CONTENT";
pub const ROTATE_TO_FIT_STEP: &str = "ROTATE_TO_BEST_FIT";
pub const CHOOSE_PAPER_STEP: &str = "CHOOSE_PAPER";

fn is_plan_kind(kind: ViewportKind) -> bool {
    kind == ViewportKind::Plan || kind == ViewportKind::KeyPlan
}

fn is_section_kind(kind: ViewportKind) -> bool {
    kind == ViewportKind::LongSection || kind == ViewportKind::CrossSections
}

// The viewport `id`: the index of the sheet it is on, and its own.
fn locate(set: &SheetSet, id: &str) -> Result<(usize, usize)> {
    for (s, sheet) in set.sheets.iter().enumerate() {
        if let Some(v) = sheet
            .viewports
            .iter()
            .position(|viewport| viewport.id == id)
        {
            return Ok((s, v));
        }
    }
    Err(make_error(
        ErrorCode::NotFound,
        "no viewport with that id",
        id,
    ))
}

// `content`, or when it is empty what the viewport shows of the document's
// drawing. A section's content is the ground it cuts, which only a caller
// that has cut it can give.
fn content_for(
    document: &Document,
    set: &SheetSet,
    viewport: &Viewport,
    content: &[Point2],
) -> Result<Vec<Point2>> {
    if !content.is_empty() {
        return Ok(content.to_vec());
    }
    if is_section_kind(viewport.kind) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a section's content is the ground it cuts: give its (chainage or offset, level) \
             points",
            viewport.id.as_str(),
        ));
    }
    Ok(viewport_content_in_set(
        document.model(),
        set,
        viewport,
        None,
    ))
}

/// auto_arrange on sheet `sheet_index`. NotFound for an index past the end.
pub fn auto_arrange_sheet(document: &mut Document, sheet_index: usize) -> Result<ArrangeResult> {
    let mut result = ArrangeResult::default();
    edit_sheet(
        document,
        sheet_index,
        |sheet| {
            result = auto_arrange(sheet, TILING_GUTTER_MM);
            Ok(())
        },
        AUTO_ARRANGE_STEP,
    )?;
    Ok(result)
}

/// align_viewports on sheet `sheet_index`: the ids that moved. Errors as
/// arrange::align_viewports, and NotFound for an index past the end.
pub fn align_viewports(
    document: &mut Document,
    sheet_index: usize,
    ids: &[String],
    edge: AlignEdge,
) -> Result<Vec<String>> {
    let mut moved = Vec::new();
    edit_sheet(
        document,
        sheet_index,
        |sheet| {
            moved = arrange::align_viewports(sheet, ids, edge)?;
            Ok(())
        },
        ALIGN_STEP,
    )?;
    Ok(moved)
}

/// distribute_viewports on sheet `sheet_index`: the ids that moved. Errors as
/// arrange::distribute_viewports, and NotFound for an index past the end.
pub fn distribute_viewports(
    document: &mut Document,
    sheet_index: usize,
    ids: &[String],
    axis: DistributeAxis,
) -> Result<Vec<String>> {
    let mut moved = Vec::new();
    edit_sheet(
        document,
        sheet_index,
        |sheet| {
            moved = arrange::distribute_viewports(sheet, ids, axis)?;
            Ok(())
        },
        DISTRIBUTE_STEP,
    )?;
    Ok(moved)
}

/// match_scale across the set: the ids whose scale changed. `from_scale` (and
/// for a section `from_exaggeration`) is what an automatic `from_id` is drawn
/// at, when the caller knows it (the editor does); without it an automatic
/// plan's scale is worked out from what it shows, as the painter works it
/// out, and any other view's own is used.
pub fn match_scale(
    document: &mut Document,
    ids: &[String],
    from_id: &str,
    mut from_scale: Option<f64>,
    from_exaggeration: Option<f64>,
) -> Result<Vec<String>> {
    if from_scale.is_none() {
        // An automatic plan is matched at the scale it is drawn at, not at
        // the last one stored.
        let set = document.sheet_set();
        if let Ok((s, v)) = locate(&set, from_id) {
            let from = &set.sheets[s].viewports[v];
            if from.auto_scale && is_plan_kind(from.kind) {
                let content = viewport_content_in_set(document.model(), &set, from, None);
                from_scale = Some(drawn_scale(from, &content));
            }
        }
    }
    let mut changed = Vec::new();
    edit_sheet_set(
        document,
        |set| {
            changed = arrange::match_scale(set, ids, from_id, from_scale, from_exaggeration)?;
            Ok(())
        },
        MATCH_SCALE_STEP,
    )?;
    Ok(changed)
}

/// The scale viewport `viewport` is drawn at: its own, or for an automatic
/// plan or key plan the first of SHEET_SCALES at which `content` fits its
/// rectangle with AUTO_SCALE_SPARE to spare, turned as it is turned and
/// measured about the point it is drawn round (the content's middle with an
/// automatic centre, else the view's centre): the painter's rule. A view
/// along an alignment is measured by `content` itself, any other by the four
/// corners of `content`'s box, as the painter measures the drawing's box.
pub fn drawn_scale(viewport: &Viewport, content: &[Point2]) -> f64 {
    if !viewport.auto_scale || !is_plan_kind(viewport.kind) || viewport.rect.empty() {
        return viewport.scale;
    }
    // The painter's rule (resolvePlanViewport): the content measured about
    // the point it is drawn round (its own middle with an automatic centre,
    // else the view's centre), turned as the view turns it, with
    // AUTO_SCALE_SPARE to spare. The painter measures a stretch of alignment
    // by its points but the drawing by the four corners of its box, which a
    // turned view needs more room for than the drawing's own outline.
    let mut corners: Vec<Point2> = Vec::new();
    if viewport.source.alignment.is_empty() {
        let mut extent = Box2::default();
        for &point in content {
            extent.expand(point);
        }
        if !extent.empty() {
            corners = vec![
                extent.min,
                Point2::new(extent.max.x, extent.min.y),
                extent.max,
                Point2::new(extent.min.x, extent.max.y),
            ];
        }
    }
    let content = if corners.is_empty() {
        content
    } else {
        &corners[..]
    };
    let mut width = 0.0;
    let mut height = 0.0;
    if viewport.auto_centre {
        let Ok(fit) = fit_at_rotation(
            content,
            SizeMm {
                width: 1.0,
                height: 1.0,
            },
            viewport.rotation,
        ) else {
            return viewport.scale;
        };
        width = fit.width;
        height = fit.height;
    } else {
        for &point in content {
            let d = (point - viewport.centre).rotated(-viewport.rotation);
            width = cpp::max(width, 2.0 * d.x.abs());
            height = cpp::max(height, 2.0 * d.y.abs());
        }
    }
    let needed = cpp::max(
        width * 1000.0 / viewport.rect.width(),
        height * 1000.0 / viewport.rect.height(),
    ) * AUTO_SCALE_SPARE;
    if !(needed > 0.0) || !needed.is_finite() {
        return viewport.scale;
    }
    sheet_scale_at_least(needed).unwrap_or(viewport.scale)
}

/// fit_viewport_to_content on viewport `viewport_id`, wherever it is, inside
/// its sheet's drawing area, at `scale` when given (the scale an automatic
/// view is drawn at) and else at drawn_scale. Errors as
/// arrange::fit_viewport_to_content; NotFound for an unknown id.
pub fn fit_viewport_to_content(
    document: &mut Document,
    viewport_id: &str,
    content: &[Point2],
    scale: Option<f64>,
) -> Status {
    let set = document.sheet_set();
    let (s, v) = locate(&set, viewport_id)?;
    let found = &set.sheets[s].viewports[v];
    let points = content_for(document, &set, found, content)?;
    let drawn = scale.unwrap_or_else(|| drawn_scale(found, &points));
    let area = drawing_area(&set.sheets[s]);
    edit_viewport(
        document,
        viewport_id,
        |viewport| {
            viewport.scale = drawn;
            arrange::fit_viewport_to_content(viewport, &points, &area)
        },
        FIT_TO_CONTENT_STEP,
    )
}

/// rotate_to_best_fit on viewport `viewport_id`: the fit it chose. Errors as
/// arrange::rotate_to_best_fit; NotFound for an unknown id.
pub fn rotate_to_best_fit(
    document: &mut Document,
    viewport_id: &str,
    content: &[Point2],
) -> Result<RotationFit> {
    let set = document.sheet_set();
    let (s, v) = locate(&set, viewport_id)?;
    let points = content_for(document, &set, &set.sheets[s].viewports[v], content)?;
    let mut chosen = RotationFit::default();
    edit_viewport(
        document,
        viewport_id,
        |viewport| {
            chosen = arrange::rotate_to_best_fit(viewport, &points)?;
            Ok(())
        },
        ROTATE_TO_FIT_STEP,
    )?;
    Ok(chosen)
}

/// fit_paper_to_viewport on the sheet viewport `viewport_id` is on, at
/// `scale` when given and else at drawn_scale: "Choose paper for this scale".
/// Errors as fit_paper_to_viewport; NotFound for an unknown id.
pub fn choose_paper_for_scale(
    document: &mut Document,
    viewport_id: &str,
    content: &[Point2],
    scale: Option<f64>,
) -> Result<PaperChange> {
    let set = document.sheet_set();
    let (s, v) = locate(&set, viewport_id)?;
    let found = &set.sheets[s].viewports[v];
    let points = content_for(document, &set, found, content)?;
    let drawn = scale.unwrap_or_else(|| drawn_scale(found, &points));
    let mut change = PaperChange::default();
    edit_sheet(
        document,
        s,
        |sheet| {
            change = arrange::fit_paper_to_viewport(sheet, viewport_id, &points, drawn)?;
            Ok(())
        },
        CHOOSE_PAPER_STEP,
    )?;
    Ok(change)
}

/// The main view of sheet `sheet_index` that "Choose paper for this scale"
/// acts on: its first plan, else its first key plan. Empty when it has
/// neither or there is no such sheet.
pub fn main_plan_of(set: &SheetSet, sheet_index: usize) -> String {
    let Some(sheet) = set.sheets.get(sheet_index) else {
        return String::new();
    };
    for kind in [ViewportKind::Plan, ViewportKind::KeyPlan] {
        if let Some(viewport) = sheet
            .viewports
            .iter()
            .find(|viewport| viewport.kind == kind && !viewport.rect.empty())
        {
            return viewport.id.clone();
        }
    }
    String::new()
}
