//! What is under the cursor on a sheet (docs/plotting.md, "Editing on the canvas"): the paper
//! position in millimetres, the viewport there with its kind and scale, and - over a plan or key
//! plan - the point of the drawing drawn there, so a sheet can be checked against the survey
//! without leaving the editor. The sheet editor shows it in its status bar; an agent asks the
//! canvas for it. The port of plotting/sheet_readout.

use katana_cad::plot::paper_dimensions;
use katana_cad::plotting::sheet_set::{Sheet, Viewport, ViewportKind, scale_text};
use katana_cad::plotting::viewport_edits::plan_paper_to_world;
use katana_geometry::Point2;

use crate::format::qt_fixed;
use crate::sheet_painter::ResolvedViewport;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetCursorReadout {
    /// Whether there was a sheet to read, and whether the point is on its paper.
    pub on_sheet: bool,
    pub on_paper: bool,
    /// Millimetres from the paper's bottom-left corner, Y up: what the rulers count.
    pub paper: Point2,
    /// The topmost placed viewport under the point; empty for none.
    pub viewport_id: String,
    pub kind: ViewportKind,
    /// The scale that viewport is drawn at ("1:500", "H 1:500 V 1:50"); empty for a kind with no
    /// scale.
    pub scale: String,
    /// Plans and key plans: the world point (easting, northing) drawn there.
    pub world: Option<Point2>,
}

/// How a plan viewport is drawn once "auto" is decided: the painter's resolve_plan_viewport, or a
/// cached answer from it.
pub type PlanResolver<'a> = &'a dyn Fn(&Viewport) -> ResolvedViewport;

fn kind_label(kind: ViewportKind) -> &'static str {
    match kind {
        ViewportKind::Plan => "Plan",
        ViewportKind::LongSection => "Long section",
        ViewportKind::CrossSections => "Cross sections",
        ViewportKind::Model3D => "3D snapshot",
        ViewportKind::Legend => "Legend",
        ViewportKind::Notes => "Notes",
        ViewportKind::Image => "Image",
        ViewportKind::KeyPlan => "Key plan",
        ViewportKind::SheetIndex => "Drawing register",
        ViewportKind::Revisions => "Revision table",
    }
}

fn ratio(scale: f64) -> String {
    format!(
        "1:{}",
        qt_fixed(scale, if scale == scale.floor() { 0 } else { 2 })
    )
}

/// The readout at paper point `paper` of `sheet`. Without a resolver a plan reads at its stored
/// scale and centre.
pub fn sheet_cursor_readout(
    sheet: &Sheet,
    paper: Point2,
    resolve: Option<PlanResolver<'_>>,
) -> SheetCursorReadout {
    let mut readout = SheetCursorReadout {
        on_sheet: true,
        paper,
        ..SheetCursorReadout::default()
    };
    let size = paper_dimensions(sheet.paper, sheet.landscape);
    readout.on_paper = (0.0..=size.width_mm).contains(&paper.x)
        && (0.0..=size.height_mm).contains(&paper.y);
    // Front to back, as a click picks.
    for viewport in sheet.viewports.iter().rev() {
        if viewport.rect.empty() || !viewport.rect.contains(paper) {
            continue;
        }
        readout.viewport_id.clone_from(&viewport.id);
        readout.kind = viewport.kind;
        if viewport.kind == ViewportKind::Plan || viewport.kind == ViewportKind::KeyPlan {
            let at = match resolve {
                Some(resolve) => resolve(viewport),
                None => ResolvedViewport {
                    scale: viewport.scale,
                    centre: viewport.centre,
                },
            };
            if at.scale > 0.0 && at.scale.is_finite() {
                readout.scale = ratio(at.scale);
                readout.world = Some(plan_paper_to_world(viewport, at.scale, at.centre, paper));
            }
        } else if viewport.kind == ViewportKind::LongSection
            || viewport.kind == ViewportKind::CrossSections
        {
            readout.scale = scale_text(viewport);
        }
        break;
    }
    readout
}

/// One line for a status bar: "X 220.0  Y 185.0 mm   vp1 Plan 1:500   E 1010.000 N 2005.000";
/// empty when there is no sheet.
pub fn readout_text(readout: &SheetCursorReadout) -> String {
    if !readout.on_sheet {
        return String::new();
    }
    let mut text = format!(
        "X {}  Y {} mm",
        qt_fixed(readout.paper.x, 1),
        qt_fixed(readout.paper.y, 1)
    );
    if !readout.viewport_id.is_empty() {
        text.push_str(&format!(
            "   {} {}",
            readout.viewport_id,
            kind_label(readout.kind)
        ));
        if !readout.scale.is_empty() {
            text.push(' ');
            text.push_str(&readout.scale);
        }
    }
    if let Some(world) = readout.world {
        text.push_str(&format!(
            "   E {}  N {}",
            qt_fixed(world.x, 3),
            qt_fixed(world.y, 3)
        ));
    }
    text
}
