//! Icons of the Text and dimensions tools (see tool_icons).
//!
//! The accent is what the tool makes - the letter, the dimension line and its arrows, the leader -
//! and the neutral tone is what it is made against: the start point and baseline, the measured
//! points and their extension lines, the note. The two dimensions share one language - two
//! points, extension lines, an arrowed line - so they read as a pair and differ only in how the
//! line lies. Arrowheads are filled triangles rather than strokes: at 16 px two strokes meeting
//! at a point blur into a blob, where a solid wedge still reads as an arrow.

use std::f64::consts::PI;

use super::tool_icons::{STROKE, ToolInk, arc, circle, polyline, pt, rectangle};

/// Thinner than the dimension line, as on a drawing, but not the 1.1 of a construction line: a
/// diagonal that thin anti-aliases away at 16 px.
const EXTENSION_WIDTH: f64 = 1.3;

/// A filled arrowhead with its point at (x, y), pointing along (dx, dy), which need not be a unit
/// vector.
fn arrowhead(ink: &mut ToolInk<'_>, x: f64, y: f64, dx: f64, dy: f64) {
    // Measured at 16 px: shorter than this and the head reads as a thicker end of the line
    // rather than an arrow.
    const LENGTH: f64 = 5.0;
    const HALF_WIDTH: f64 = 2.2;
    let norm = dx.hypot(dy);
    let ux = dx / norm;
    let uy = dy / norm;
    let back_x = x - ux * LENGTH;
    let back_y = y - uy * LENGTH;
    ink.fill(
        &polyline(
            &[
                pt(x, y),
                pt(back_x - uy * HALF_WIDTH, back_y + ux * HALF_WIDTH),
                pt(back_x + uy * HALF_WIDTH, back_y - ux * HALF_WIDTH),
            ],
            true,
        ),
        true,
        255,
    );
}

fn paint_text(ink: &mut ToolInk<'_>) {
    // A capital A, drawn as strokes rather than set in a font, so it is the same letter on every
    // machine and stays sharp at 16 px. It stands on the baseline from the start point the tool
    // asks for first. (A text cursor beside the letter was tried and read as "AI".)
    ink.stroke(
        &polyline(&[pt(5.0, 17.5), pt(12.0, 3.0), pt(19.0, 17.5)], false),
        true,
        STROKE,
    );
    ink.line(7.9, 12.0, 16.1, 12.0, true, STROKE);
    ink.line(4.0, 21.0, 21.0, 21.0, false, 1.1);
    ink.node(4.0, 21.0, false);
}

fn paint_multiline_text(ink: &mut ToolInk<'_>) {
    // Text's capital A, smaller, over two lines of text inside the frame of one block: several
    // lines that are one thing.
    ink.stroke(&rectangle(3.0, 3.0, 18.0, 18.0), false, 1.1);
    ink.stroke(
        &polyline(&[pt(6.0, 11.0), pt(9.5, 5.0), pt(13.0, 11.0)], false),
        true,
        STROKE,
    );
    ink.line(7.5, 8.8, 11.5, 8.8, true, STROKE);
    ink.line(6.0, 14.5, 18.0, 14.5, false, 1.4);
    ink.line(6.0, 18.0, 15.0, 18.0, false, 1.4);
}

fn paint_linear_dimension(ink: &mut ToolInk<'_>) {
    // Two points at different heights; the dimension measures only across.
    ink.line(4.0, 18.5, 4.0, 4.5, false, EXTENSION_WIDTH);
    ink.line(20.0, 13.5, 20.0, 4.5, false, EXTENSION_WIDTH);
    ink.node(4.0, 20.5, false);
    ink.node(20.0, 15.5, false);
    ink.line(8.0, 8.0, 16.0, 8.0, true, STROKE);
    arrowhead(ink, 4.0, 8.0, -1.0, 0.0);
    arrowhead(ink, 20.0, 8.0, 1.0, 0.0);
}

fn paint_aligned_dimension(ink: &mut ToolInk<'_>) {
    // Two points on a diagonal and the dimension parallel to them, offset up and to the left
    // along the normal (-1, -1) / sqrt 2. No line joins the points: with one, the four sides
    // closed into a box and read as a rotated rectangle at 16 px.
    const OFFSET: f64 = 8.0;
    let shift = OFFSET / 2.0_f64.sqrt();
    let ax = 8.5;
    let ay = 21.0;
    let bx = 21.0;
    let by = 8.5;
    ink.node(ax, ay, false);
    ink.node(bx, by, false);
    // Extension lines from just off each point to just past the dimension line.
    let gap = 2.0 / 2.0_f64.sqrt();
    let beyond = 1.2 / 2.0_f64.sqrt();
    ink.line(
        ax - gap,
        ay - gap,
        ax - shift - beyond,
        ay - shift - beyond,
        false,
        EXTENSION_WIDTH,
    );
    ink.line(
        bx - gap,
        by - gap,
        bx - shift - beyond,
        by - shift - beyond,
        false,
        EXTENSION_WIDTH,
    );
    let dax = ax - shift;
    let day = ay - shift;
    let dbx = bx - shift;
    let dby = by - shift;
    // The line stops short of the tips, which the arrowheads fill.
    let inset = 4.0 / 2.0_f64.sqrt();
    ink.line(
        dax + inset,
        day - inset,
        dbx - inset,
        dby + inset,
        true,
        STROKE,
    );
    arrowhead(ink, dax, day, -1.0, 1.0);
    arrowhead(ink, dbx, dby, 1.0, -1.0);
}

fn paint_leader(ink: &mut ToolInk<'_>) {
    // The arrow touches a feature at the bottom left; the leader rises to a short landing and
    // the note stands beside it.
    const TIP_X: f64 = 4.0;
    const TIP_Y: f64 = 20.5;
    const BEND_X: f64 = 11.0;
    const BEND_Y: f64 = 9.0;
    let dx = TIP_X - BEND_X;
    let dy = TIP_Y - BEND_Y;
    let length = dx.hypot(dy);
    // The line ends inside the arrowhead, so no gap shows between them.
    let reach = (length - 3.5) / length;
    ink.stroke(
        &polyline(
            &[
                pt(BEND_X + dx * reach, BEND_Y + dy * reach),
                pt(BEND_X, BEND_Y),
                pt(13.5, 9.0),
            ],
            false,
        ),
        true,
        STROKE,
    );
    arrowhead(ink, TIP_X, TIP_Y, dx, dy);
    // Two lines of it, centred on the landing as the tool places a note; a third merged the
    // three into one block at 16 px.
    ink.line(16.0, 7.0, 21.5, 7.0, false, 1.6);
    ink.line(16.0, 11.0, 20.0, 11.0, false, 1.6);
}

fn paint_angular_dimension(ink: &mut ToolInk<'_>) {
    // Two lines from a vertex at the bottom left, one level and one rising steeply, and the arc
    // between them with an arrow at each end. The angle is wide and the arc large so that the
    // arc still shows between its two arrowheads at 16 px.
    const VERTEX_X: f64 = 3.0;
    const VERTEX_Y: f64 = 21.0;
    const RISE_X: f64 = 13.0;
    const RISE_Y: f64 = 3.0;
    ink.line(VERTEX_X, VERTEX_Y, 21.5, VERTEX_Y, false, EXTENSION_WIDTH);
    ink.line(VERTEX_X, VERTEX_Y, RISE_X, RISE_Y, false, EXTENSION_WIDTH);
    ink.node(VERTEX_X, VERTEX_Y, false);
    // Radius 15 from the level line (0 degrees) up to the rising one, at atan2(18, 10) = 60.9
    // degrees on paper; the arc itself stops 17 degrees short of each line, where the
    // arrowheads take over.
    const RADIUS: f64 = 15.0;
    let rising = (VERTEX_Y - RISE_Y).atan2(RISE_X - VERTEX_X);
    let rising_degrees = rising * 180.0 / PI;
    ink.stroke(
        &arc(VERTEX_X, VERTEX_Y, RADIUS, 17.0, rising_degrees - 34.0),
        true,
        STROKE,
    );
    arrowhead(ink, VERTEX_X + RADIUS, VERTEX_Y, 0.0, 1.0);
    arrowhead(
        ink,
        VERTEX_X + RADIUS * rising.cos(),
        VERTEX_Y - RADIUS * rising.sin(),
        -rising.sin(),
        -rising.cos(),
    );
}

fn paint_radial_dimension(ink: &mut ToolInk<'_>, diameter: bool) {
    // A circle, and the dimension line from its centre (radius) or across it (diameter) with an
    // arrow on the curve.
    const CENTRE_X: f64 = 11.0;
    const CENTRE_Y: f64 = 13.0;
    const RADIUS: f64 = 8.0;
    ink.stroke(&circle(CENTRE_X, CENTRE_Y, RADIUS), false, EXTENSION_WIDTH);
    let ux = 0.7_f64.cos();
    let uy = -(0.7_f64.sin());
    let tip_x = CENTRE_X + ux * RADIUS;
    let tip_y = CENTRE_Y + uy * RADIUS;
    if diameter {
        let back_x = CENTRE_X - ux * RADIUS;
        let back_y = CENTRE_Y - uy * RADIUS;
        ink.line(
            back_x + ux * 4.0,
            back_y + uy * 4.0,
            tip_x - ux * 4.0,
            tip_y - uy * 4.0,
            true,
            STROKE,
        );
        arrowhead(ink, back_x, back_y, -ux, -uy);
    } else {
        ink.node(CENTRE_X, CENTRE_Y, false);
        ink.line(
            CENTRE_X,
            CENTRE_Y,
            tip_x - ux * 4.0,
            tip_y - uy * 4.0,
            true,
            STROKE,
        );
    }
    arrowhead(ink, tip_x, tip_y, ux, uy);
}

fn paint_ordinate_dimension(ink: &mut ToolInk<'_>) {
    // The datum's two axes at the bottom left, a feature, and its leader rising to where the
    // value is written.
    ink.line(3.0, 21.0, 21.0, 21.0, false, 1.1);
    ink.line(3.0, 21.0, 3.0, 3.0, false, 1.1);
    ink.node(3.0, 21.0, false);
    ink.node(13.0, 16.0, false);
    ink.stroke(
        &polyline(&[pt(13.0, 13.5), pt(13.0, 9.0), pt(16.0, 6.0)], false),
        true,
        STROKE,
    );
    ink.line(16.0, 6.0, 21.0, 6.0, true, STROKE);
}

fn paint_baseline_dimension(ink: &mut ToolInk<'_>) {
    // Three origins along the bottom and two dimension lines stacked above them, both from the
    // first origin's extension line: the shorter one below, the longer one a step further out,
    // as a baseline chain lies.
    ink.node(4.0, 20.5, false);
    ink.node(12.0, 20.5, false);
    ink.node(20.0, 20.5, false);
    ink.line(4.0, 18.5, 4.0, 3.5, false, EXTENSION_WIDTH);
    ink.line(12.0, 18.5, 12.0, 11.0, false, EXTENSION_WIDTH);
    ink.line(20.0, 18.5, 20.0, 3.5, false, EXTENSION_WIDTH);
    // Eight pixels between origins: the two heads meet and are the line.
    arrowhead(ink, 4.0, 13.0, -1.0, 0.0);
    arrowhead(ink, 12.0, 13.0, 1.0, 0.0);
    ink.line(8.0, 6.0, 16.0, 6.0, true, STROKE);
    arrowhead(ink, 4.0, 6.0, -1.0, 0.0);
    arrowhead(ink, 20.0, 6.0, 1.0, 0.0);
}

fn paint_continue_dimension(ink: &mut ToolInk<'_>) {
    // Three origins and two dimension lines end to end on one level, the second going on from
    // the first's end: a continued chain.
    ink.node(3.0, 20.5, false);
    ink.node(12.0, 20.5, false);
    ink.node(21.0, 20.5, false);
    for x in [3.0, 12.0, 21.0] {
        ink.line(x, 18.5, x, 5.5, false, EXTENSION_WIDTH);
    }
    arrowhead(ink, 3.0, 9.0, -1.0, 0.0);
    arrowhead(ink, 12.0, 9.0, 1.0, 0.0);
    arrowhead(ink, 12.0, 9.0, -1.0, 0.0);
    arrowhead(ink, 21.0, 9.0, 1.0, 0.0);
}

fn paint_balloon(ink: &mut ToolInk<'_>) {
    // A leader from a feature at the bottom left up to a circle with a number in it: a leader
    // whose note is framed, as the item balloons of a detail or a schedule are.
    const TIP_X: f64 = 4.0;
    const TIP_Y: f64 = 20.5;
    const CENTRE_X: f64 = 15.5;
    const CENTRE_Y: f64 = 8.5;
    const RADIUS: f64 = 6.0;
    let dx = TIP_X - CENTRE_X;
    let dy = TIP_Y - CENTRE_Y;
    let length = dx.hypot(dy);
    // From the circle's edge to inside the arrowhead, so no gap shows.
    ink.line(
        CENTRE_X + dx / length * RADIUS,
        CENTRE_Y + dy / length * RADIUS,
        TIP_X - dx / length * 3.5,
        TIP_Y - dy / length * 3.5,
        true,
        STROKE,
    );
    arrowhead(ink, TIP_X, TIP_Y, dx, dy);
    ink.stroke(&circle(CENTRE_X, CENTRE_Y, RADIUS), true, STROKE);
    ink.label(CENTRE_X, CENTRE_Y, 8.0, "1", false);
}

fn paint_label(ink: &mut ToolInk<'_>) {
    // A labelled point: the point at the bottom left, and a tag beside it holding two lines of
    // words - the label, which says what the point is rather than being drawn with it. No
    // leader: a label is placed beside what it labels, and a line would read as the Leader
    // tool's icon.
    ink.node(5.0, 19.0, false);
    ink.stroke(&rectangle(9.0, 4.0, 12.5, 10.0), true, STROKE);
    ink.line(11.5, 7.5, 18.5, 7.5, true, 1.6);
    ink.line(11.5, 11.0, 16.5, 11.0, true, 1.6);
}

// ---- the preset leaders (annotate_leader_presets) ----------------------------------------------
//
// One language for the six: the point the note is read off is a vertex handle (neutral, what is
// there), the leader rises from it to a short landing (accent, what the tool makes), and what
// stands at the landing says what the note reads - a level mark, a difference to a ground line,
// cut and fill arrows, a name tag, a code's bars, a chainage's ticks. No arrowhead: the tip is ON
// the point, and a head there would hide the point it reads.

/// The leader from the point at (x, y) to a bend at (bend_x, bend_y) and a landing to
/// landing_end; the point drawn last, over the line's end.
fn preset_leader(
    ink: &mut ToolInk<'_>,
    x: f64,
    y: f64,
    bend_x: f64,
    bend_y: f64,
    landing_end: f64,
) {
    ink.stroke(
        &polyline(
            &[pt(x, y), pt(bend_x, bend_y), pt(landing_end, bend_y)],
            false,
        ),
        true,
        STROKE,
    );
    ink.node(x, y, false);
}

fn paint_level_leader(ink: &mut ToolInk<'_>) {
    // A spot level's mark at the landing: a triangle standing on its point on a level line, as a
    // level is marked on a section.
    preset_leader(ink, 4.0, 19.5, 10.0, 9.0, 13.0);
    ink.line(14.0, 11.5, 22.5, 11.5, false, EXTENSION_WIDTH);
    ink.fill(
        &polyline(&[pt(14.8, 4.5), pt(21.6, 4.5), pt(18.2, 10.5)], true),
        true,
        255,
    );
}

fn paint_surface_leader(ink: &mut ToolInk<'_>) {
    // A point above a ground line, the height between them dashed, and the note a
    // plus-or-minus: how far above or below the surface.
    ink.stroke(
        &polyline(
            &[pt(2.0, 20.5), pt(8.0, 18.5), pt(14.0, 20.5), pt(22.0, 17.5)],
            false,
        ),
        false,
        EXTENSION_WIDTH,
    );
    ink.dashed(&polyline(&[pt(6.0, 13.5), pt(6.0, 18.2)], false), true);
    preset_leader(ink, 6.0, 11.0, 11.0, 5.5, 14.0);
    ink.label(19.0, 6.0, 10.0, "\u{b1}", true);
}

fn paint_cut_fill_leader(ink: &mut ToolInk<'_>) {
    // A point under a ground line - a cut - and the note a cut arrow down beside a fill arrow
    // up: one of the two is said.
    ink.stroke(
        &polyline(
            &[pt(2.0, 14.0), pt(9.0, 12.5), pt(15.0, 14.5), pt(22.0, 12.0)],
            false,
        ),
        false,
        EXTENSION_WIDTH,
    );
    preset_leader(ink, 5.0, 19.5, 10.5, 5.5, 12.5);
    ink.line(15.5, 2.5, 15.5, 6.5, true, 1.4);
    arrowhead(ink, 15.5, 10.5, 0.0, 1.0);
    ink.line(20.5, 10.5, 20.5, 6.5, true, 1.4);
    arrowhead(ink, 20.5, 2.5, 0.0, -1.0);
}

fn paint_point_name_leader(ink: &mut ToolInk<'_>) {
    // A name tag at the landing - its hole the neutral dot - for the point's name.
    preset_leader(ink, 4.0, 19.5, 10.0, 10.0, 12.5);
    ink.stroke(
        &polyline(
            &[
                pt(13.0, 6.5),
                pt(19.5, 6.5),
                pt(22.5, 10.0),
                pt(19.5, 13.5),
                pt(13.0, 13.5),
            ],
            true,
        ),
        true,
        STROKE,
    );
    ink.dot(19.2, 10.0, 1.1, false);
}

fn paint_code_leader(ink: &mut ToolInk<'_>) {
    // A code's bars at the landing, thick and thin, for the survey code.
    preset_leader(ink, 4.0, 19.5, 10.0, 10.0, 12.5);
    const BARS: [[f64; 2]; 5] = [
        [14.0, 1.6],
        [16.2, 0.9],
        [18.0, 1.6],
        [20.0, 0.9],
        [21.8, 1.6],
    ];
    for [x, width] in BARS {
        ink.line(x, 6.0, x, 14.0, true, width);
    }
}

fn paint_station_leader(ink: &mut ToolInk<'_>) {
    // An alignment along the bottom with two chainage ticks across it, a point off it with its
    // offset dashed down to it, and the leader up.
    ink.stroke(
        &polyline(
            &[pt(2.0, 21.0), pt(9.0, 18.5), pt(16.0, 17.8), pt(22.5, 19.0)],
            false,
        ),
        false,
        EXTENSION_WIDTH,
    );
    ink.line(8.6, 16.3, 9.4, 20.7, false, EXTENSION_WIDTH);
    ink.line(16.2, 15.6, 15.8, 20.0, false, EXTENSION_WIDTH);
    ink.dashed(&polyline(&[pt(12.5, 12.5), pt(12.5, 17.0)], false), true);
    preset_leader(ink, 12.5, 10.5, 17.0, 4.5, 21.5);
}

pub fn paint_annotate_icon(tool_id: &str, ink: &mut ToolInk<'_>) -> bool {
    match tool_id {
        "annotate.leader.level" => paint_level_leader(ink),
        "annotate.leader.surface" => paint_surface_leader(ink),
        "annotate.leader.cutfill" => paint_cut_fill_leader(ink),
        "annotate.leader.name" => paint_point_name_leader(ink),
        "annotate.leader.code" => paint_code_leader(ink),
        "annotate.leader.station" => paint_station_leader(ink),
        "annotate.text" => paint_text(ink),
        "annotate.mtext" => paint_multiline_text(ink),
        "annotate.dimlinear" => paint_linear_dimension(ink),
        "annotate.dimaligned" => paint_aligned_dimension(ink),
        "annotate.dimangular" => paint_angular_dimension(ink),
        "annotate.dimradius" => paint_radial_dimension(ink, false),
        "annotate.dimdiameter" => paint_radial_dimension(ink, true),
        "annotate.dimordinate" => paint_ordinate_dimension(ink),
        "annotate.leader" => paint_leader(ink),
        "annotate.label" => paint_label(ink),
        "annotate.dimbaseline" => paint_baseline_dimension(ink),
        "annotate.dimcontinue" => paint_continue_dimension(ink),
        "annotate.balloon" => paint_balloon(ink),
        _ => return false,
    }
    true
}
