//! The icons of the BIM menu's Road Objects section (docs/road_objects.md, "In the window"),
//! painted here rather than in icons so the road objects' drawings live with their workbench:
//! icons' paint_icon hands these values here, the canvas already on the 24-unit grid.
//!
//! Each is the object as it stands beside a road, seen side on: the object neutral, and in the
//! accent what makes it that object - the rail of a barrier, the reflector of a guide post, a
//! sign's plate, the lantern, the pipe through the headwall, the wall's section, the wires, the
//! marks.
//!
//! Painted with a pen of its own, as the C++ is: its fill sets the tone's alpha outright
//! (QColor::setAlpha(alpha)) where ToolInk's scales it, the same for the opaque tones the window
//! paints in. The stroke is the same - 1.7 units of the 24-unit grid, round caps and joins.

use crate::canvas::{Brush, Canvas, Color, Path, Pen, PenCap, PenJoin, PenStyle, PointF, RectF};
use crate::icons::Icon;
use crate::tools::tool_icons::{STROKE, pt, start};

struct RoadPen<'a> {
    canvas: &'a mut dyn Canvas,
    neutral: Color,
    accent: Color,
}

impl RoadPen<'_> {
    fn tone(&self, accent: bool) -> Color {
        if accent { self.accent } else { self.neutral }
    }

    fn stroke(&mut self, path: &Path, accent: bool, width: f64) {
        let pen = Pen::with(
            self.tone(accent),
            width,
            PenStyle::Solid,
            PenCap::Round,
            PenJoin::Round,
        );
        self.canvas.set_brush(Brush::none());
        self.canvas.set_pen(pen);
        self.canvas.draw_path(path);
    }

    fn fill(&mut self, path: &Path, accent: bool, alpha: u8) {
        let colour = self.tone(accent).with_alpha(alpha);
        self.canvas.set_pen(Pen::none());
        self.canvas.set_brush(Brush::solid(colour));
        self.canvas.draw_path(path);
    }

    fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, accent: bool, width: f64) {
        let mut path = start(x1, y1);
        path.line_to(pt(x2, y2));
        self.stroke(&path, accent, width);
    }
}

fn rectangle(x: f64, y: f64, w: f64, h: f64) -> Path {
    let mut path = Path::new();
    path.add_rect(RectF::new(x, y, w, h));
    path
}

fn circle(x: f64, y: f64, radius: f64) -> Path {
    let mut path = Path::new();
    path.add_ellipse(RectF::new(
        x - radius,
        y - radius,
        2.0 * radius,
        2.0 * radius,
    ));
    path
}

fn polygon(points: &[PointF]) -> Path {
    let mut path = Path::new();
    let mut first = true;
    for &point in points {
        if first {
            path.move_to(point);
            first = false;
        } else {
            path.line_to(point);
        }
    }
    path.close_subpath();
    path
}

/// The ground every object stands on, thinner than the object so it reads as where it stands
/// rather than as part of it.
fn ground(pen: &mut RoadPen<'_>) {
    pen.line(2.5, 20.5, 21.5, 20.5, false, 1.1);
}

/// Paints `which` - Icon::RoadBarrier to Icon::RoadObjects - on the 24-unit grid `canvas` is set
/// to, in the application's stroke: the object neutral, what makes it that object in the accent.
/// False for an icon that is not one of these, which it leaves unpainted.
pub fn paint_road_object_icon(
    which: Icon,
    canvas: &mut dyn Canvas,
    neutral: Color,
    accent: Color,
) -> bool {
    let mut pen = RoadPen {
        canvas,
        neutral,
        accent,
    };
    match which {
        Icon::RoadBarrier => {
            // Three posts and the beam along their tops.
            ground(&mut pen);
            for x in [5.0, 12.0, 19.0] {
                pen.line(x, 20.5, x, 9.0, false, STROKE);
            }
            let rail = rectangle(3.0, 9.0, 18.0, 4.0);
            pen.fill(&rail, true, 110);
            pen.stroke(&rail, true, STROKE);
            true
        }
        Icon::RoadGuidePosts => {
            // Two posts going away, each with its reflector near the top.
            ground(&mut pen);
            pen.stroke(&rectangle(5.0, 5.0, 4.0, 15.5), false, STROKE);
            pen.fill(&rectangle(5.0, 6.5, 4.0, 3.0), true, 255);
            pen.stroke(&rectangle(15.0, 10.0, 3.0, 10.5), false, STROKE);
            pen.fill(&rectangle(15.0, 11.2, 3.0, 2.3), true, 255);
            true
        }
        Icon::RoadSign => {
            // A plate on its post.
            ground(&mut pen);
            pen.line(12.0, 20.5, 12.0, 13.0, false, STROKE);
            let plate = rectangle(4.5, 3.5, 15.0, 9.5);
            pen.fill(&plate, true, 90);
            pen.stroke(&plate, true, STROKE);
            pen.line(8.0, 8.25, 16.0, 8.25, true, 1.3);
            true
        }
        Icon::RoadLightPole => {
            // The pole, its arm reaching over the road, and the lantern lit.
            ground(&mut pen);
            pen.line(7.0, 20.5, 7.0, 5.0, false, STROKE);
            let mut arm = start(7.0, 7.0);
            arm.quad_to(pt(7.5, 4.0), pt(15.0, 4.0));
            pen.stroke(&arm, false, STROKE);
            pen.fill(&rectangle(14.0, 3.0, 6.5, 2.8), true, 255);
            for x in [15.0, 17.25, 19.5] {
                pen.line(x, 8.5, x + (x - 17.25) * 0.4, 11.5, true, 1.2);
            }
            true
        }
        Icon::RoadCulvert => {
            // The road over a headwall, the pipe's two walls through it.
            pen.line(2.0, 5.0, 22.0, 5.0, false, STROKE);
            pen.stroke(&rectangle(4.0, 8.0, 16.0, 12.5), false, STROKE);
            pen.stroke(&circle(12.0, 14.5, 4.5), true, STROKE);
            pen.stroke(&circle(12.0, 14.5, 2.8), true, STROKE);
            true
        }
        Icon::RoadWall => {
            // A retaining wall's section, the stem on its footing, and the ground it holds up
            // behind it.
            let section = polygon(&[
                pt(7.0, 20.5),
                pt(17.0, 20.5),
                pt(17.0, 17.5),
                pt(13.5, 17.5),
                pt(13.5, 4.0),
                pt(10.5, 4.0),
                pt(10.5, 17.5),
                pt(7.0, 17.5),
            ]);
            pen.fill(&section, true, 110);
            pen.stroke(&section, true, STROKE);
            pen.line(13.5, 7.0, 21.5, 7.0, false, STROKE);
            pen.line(2.5, 17.5, 10.5, 17.5, false, STROKE);
            true
        }
        Icon::RoadFence => {
            // Posts, and the wires strung along them.
            ground(&mut pen);
            for x in [5.0, 12.0, 19.0] {
                pen.line(x, 20.5, x, 7.0, false, STROKE);
            }
            for y in [8.5, 12.5, 16.5] {
                pen.line(3.0, y, 21.0, y, true, 1.3);
            }
            true
        }
        Icon::RoadLineMarking => {
            // A road going away, its centre line broken into marks.
            pen.line(3.0, 21.0, 9.5, 3.0, false, STROKE);
            pen.line(21.0, 21.0, 14.5, 3.0, false, STROKE);
            pen.line(12.0, 20.0, 12.0, 16.0, true, 2.2);
            pen.line(12.0, 12.5, 12.0, 9.5, true, 2.0);
            pen.line(12.0, 6.5, 12.0, 4.5, true, 1.8);
            true
        }
        Icon::RoadObjects => {
            // The manager: a list of objects, each with its mark.
            for y in [6.0, 12.0, 18.0] {
                pen.fill(&rectangle(3.5, y - 1.8, 3.6, 3.6), true, 255);
                pen.line(10.0, y, 20.5, y, false, STROKE);
            }
            true
        }
        _ => false,
    }
}
