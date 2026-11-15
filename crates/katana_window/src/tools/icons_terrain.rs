//! Icons of Terrain > Edit Surface's tools (see tool_icons; katana_cad's tools/terrain_edit,
//! docs/surface_editing.md). Each draws the same small TIN in the neutral tone - a
//! quadrilateral of four triangles round a centre vertex - and what the tool does to it in the
//! accent, so the nine read as one family on the Surface Edit toolbar.

use crate::canvas::PointF;

use super::tool_icons::{STROKE, ToolInk, polyline, pt, rectangle};

/// The corners of the TIN and its centre, on the 24-unit grid.
const CORNERS: [PointF; 4] = [
    PointF::new(3.0, 19.0),
    PointF::new(6.0, 5.0),
    PointF::new(19.0, 4.0),
    PointF::new(21.0, 18.0),
];
const CENTRE: PointF = PointF::new(12.0, 12.0);

fn draw_tin(ink: &mut ToolInk<'_>, with_centre: bool) {
    ink.stroke(&polyline(&CORNERS, true), false, STROKE);
    if with_centre {
        for corner in CORNERS {
            ink.line(corner.x, corner.y, CENTRE.x, CENTRE.y, false, STROKE);
        }
    }
}

pub fn paint_terrain_edit_icon(tool_id: &str, ink: &mut ToolInk<'_>) -> bool {
    if tool_id == "terrain.edit.deletetriangle" {
        // The lower triangle, filled and struck out.
        draw_tin(ink, true);
        ink.fill(
            &polyline(&[CORNERS[0], CORNERS[3], CENTRE], true),
            true,
            120,
        );
        ink.line(9.0, 18.0, 15.0, 14.0, true, STROKE);
        return true;
    }
    if tool_id == "terrain.edit.flip" {
        // A quadrilateral, its old diagonal dashed and the new one in the accent.
        ink.stroke(
            &polyline(
                &[pt(3.0, 12.0), pt(12.0, 3.0), pt(21.0, 12.0), pt(12.0, 21.0)],
                true,
            ),
            false,
            STROKE,
        );
        ink.dashed(&polyline(&[pt(3.0, 12.0), pt(21.0, 12.0)], false), false);
        ink.line(12.0, 3.0, 12.0, 21.0, true, STROKE);
        return true;
    }
    if tool_id == "terrain.edit.addpoint" {
        // The TIN with no centre yet, and the new vertex with its plus.
        draw_tin(ink, false);
        ink.dot(12.0, 12.0, 2.4, true);
        ink.line(12.0, 6.5, 12.0, 9.0, true, 1.4);
        ink.line(12.0, 15.0, 12.0, 17.5, true, 1.4);
        ink.line(6.5, 12.0, 9.0, 12.0, true, 1.4);
        ink.line(15.0, 12.0, 17.5, 12.0, true, 1.4);
        return true;
    }
    if tool_id == "terrain.edit.deletepoint" {
        // The centre vertex, crossed out.
        draw_tin(ink, true);
        ink.line(8.5, 8.5, 15.5, 15.5, true, 2.0);
        ink.line(15.5, 8.5, 8.5, 15.5, true, 2.0);
        return true;
    }
    if tool_id == "terrain.edit.movepoint" {
        // The vertex where it was, and an arrow in the accent to where it goes.
        draw_tin(ink, false);
        ink.dot(8.5, 13.5, 2.0, false);
        ink.line(11.0, 12.8, 17.5, 10.2, true, 1.8);
        ink.line(17.5, 10.2, 14.4, 9.6, true, 1.8);
        ink.line(17.5, 10.2, 15.6, 12.7, true, 1.8);
        return true;
    }
    if tool_id == "terrain.edit.deleteinside" {
        // A dashed polygon in the accent, the TIN's lines outside it only.
        draw_tin(ink, false);
        ink.dashed(
            &polyline(
                &[pt(8.0, 8.0), pt(16.0, 8.0), pt(16.0, 16.0), pt(8.0, 16.0)],
                true,
            ),
            true,
        );
        return true;
    }
    if tool_id == "terrain.edit.breakline" {
        // A bold line across the TIN.
        draw_tin(ink, true);
        ink.line(3.0, 13.0, 21.0, 9.0, true, 2.6);
        return true;
    }
    if tool_id == "terrain.edit.boundary" {
        // The TIN inside a closed outline in the accent.
        ink.stroke(
            &polyline(
                &[pt(6.0, 17.0), pt(9.0, 7.0), pt(17.0, 7.0), pt(18.0, 16.0)],
                true,
            ),
            false,
            STROKE,
        );
        ink.line(9.0, 7.0, 18.0, 16.0, false, STROKE);
        ink.stroke(&rectangle(3.0, 4.0, 18.0, 16.0), true, STROKE);
        return true;
    }
    if tool_id == "terrain.edit.hole" {
        // The TIN with a void cut in it, the void's rim in the accent.
        draw_tin(ink, false);
        ink.fill(&rectangle(9.0, 9.0, 6.0, 6.0), true, 70);
        ink.stroke(&rectangle(9.0, 9.0, 6.0, 6.0), true, STROKE);
        return true;
    }
    false
}
