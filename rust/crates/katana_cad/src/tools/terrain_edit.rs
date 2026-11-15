//! Terrain > Edit Surface (src/katana_cad/tools/terrain_edit.cpp; see
//! families.rs; docs/surface_editing.md, "In the window"): Delete Triangle,
//! Flip Edge, Add Point, Delete Point, Move Point, Delete Triangles Inside,
//! Add Breakline, Boundary and Hole, each acting on the ACTIVE surface.
//!
//! A tool never edits a surface itself: what it was told becomes the SURFACE
//! EDIT line a person would type, planned by the verb's own
//! plan_surface_edit and committed as the verb commits it - one undo step
//! through Document::surfaces_command - so the tool and the line cannot come
//! to differ. The message the tool reports is that line, followed by the
//! verb's reply.
//!
//! The active surface is surface_edit_target() when the drawing has it (the
//! Surface Edit toolbar's choice), else the first visible surface. Every tool
//! takes [Surface] at its prompt: S, then the name, which also becomes the
//! toolbar's choice (set_surface_edit_target tells the toolbar). Each
//! restarts after an edit, as Point does, so a run of flips or deletions is a
//! run of clicks; Esc ends it. What a run keeps from one click to the next is
//! held outside the tool, since the host remakes the tool for each click:
//! the surface (the edit target), and Delete Triangles Inside's Outside.

use std::cell::Cell;
use std::rc::Rc;

use katana_core::text::format_exact_real;
use katana_entity::{Entity, EntityId, Geometry, PointGeometry};
use katana_geometry::{Point2, Polyline2, Segment2, Triangle2};
use katana_terrain::TinSurface;
use katana_terrain::tin_edit::{
    edge_at, held_vertices, on_constrained_edge, triangle_at, vertex_at,
};
use katana_terrain::tin_surface::NO_TRIANGLE;

use crate::interactive_tool::{
    InteractiveTool, ToolCatalog, ToolContext, ToolInfo, ToolInput, ToolStep, ToolStepOutcome,
    vertex_reach,
};
use crate::terrain::drawing_shapes::{LINE_KINDS, POLYGON_KINDS, ShapeKind, shape_of};
use crate::terrain::surface_edit_verbs::{
    active_edit_surface, plan_surface_edit, set_surface_edit_target, surface_name_word,
};
use crate::tool_feedback::{FeedbackMark, FeedbackRole, ToolFeedback};
use crate::tools::families::Report;
use crate::tools::modify_edit_support::{
    ToolPtr, as_sentence, base_entity, base_point, base_undo, base_value, format_number, is_option,
    parse_number, tool_factory, trimmed_text,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditKind {
    DeleteTriangle,
    FlipEdge,
    AddPoint,
    DeletePoint,
    MovePoint,
    DeleteInside,
    Breakline,
    Boundary,
    Hole,
}

fn picks_entity(kind: EditKind) -> bool {
    matches!(
        kind,
        EditKind::DeleteInside | EditKind::Breakline | EditKind::Boundary | EditKind::Hole
    )
}

fn coordinates(at: Point2) -> String {
    format!("{},{}", format_exact_real(at.x), format_exact_real(at.y))
}

// Whether a pick is a polygon or a line is the verb's own reader's answer
// (drawing_shapes.rs), asked here: the tool and the line it writes cannot
// come to disagree about an entity, as they did about a curve polyline drawn
// back to its start without the closed flag.
fn is_polygon(entity: &Entity) -> bool {
    shape_of(entity, ShapeKind::Polygon).shape.is_some()
}

fn is_line(entity: &Entity) -> bool {
    shape_of(entity, ShapeKind::Line).shape.is_some()
}

fn outline(triangle: &Triangle2) -> Polyline2 {
    Polyline2::new(vec![triangle.a, triangle.b, triangle.c], true)
}

// A mark with no label, flagged by nothing.
fn mark(role: FeedbackRole, geometry: Geometry, label: String) -> FeedbackMark {
    FeedbackMark {
        role,
        geometry,
        label,
        ..FeedbackMark::default()
    }
}

fn vertex_mark(position: Point2) -> Geometry {
    Geometry::Point(PointGeometry { position })
}

fn no_surface() -> ToolStep {
    ToolStep::rejected("There is no surface to edit: make one first (Terrain > Surface From).")
}

// The surface the tools edit now (active_edit_surface), by name.
fn surface_name(cx: &ToolContext<'_>) -> Option<String> {
    cx.document
        .and_then(|document| active_edit_surface(document).ok())
}

fn active_surface<'a>(cx: &ToolContext<'a>) -> Option<&'a TinSurface> {
    let name = surface_name(cx)?;
    cx.document?.surfaces().find(&name)?.surface.as_deref()
}

fn surface_level(cx: &ToolContext<'_>, at: Point2) -> Option<f64> {
    active_surface(cx)?.elevation_at(at)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Main,
    Level,
    Surface,
    Target,
}

struct SurfaceEditTool {
    kind: EditKind,
    /// The Outside choice of Delete Triangles Inside, held outside the tool
    /// so that it survives the tool being remade after each edit.
    outside: Rc<Cell<bool>>,
    step: Step,
    placed: Option<Point2>,
}

thread_local! {
    // Delete Triangles Inside's Outside choice, which the C++ catalogue entry
    // holds (a shared_ptr its factory captures); the program runs its tools
    // on one thread, so the choice is that thread's (cad_plan.md section 7,
    // risk 4). Only that entry's tool reads it.
    static OUTSIDE: Rc<Cell<bool>> = Rc::new(Cell::new(false));
}

impl SurfaceEditTool {
    fn choose_surface(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        let name = trimmed_text(text);
        let Some(named) = cx
            .document
            .and_then(|document| document.surfaces().find(name))
        else {
            return ToolStep::rejected(format!("The drawing has no surface called {name}."));
        };
        set_surface_edit_target(named.name.clone());
        self.step = Step::Main;
        ToolStep::next(format!("Editing {}.", named.name))
    }

    // The first click of Move Point: the vertex within the view's reach,
    // held while the target is chosen. A vertex the verb would refuse is
    // refused now, before the second click.
    fn pick_vertex(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        let Some(surface) = active_surface(cx) else {
            return no_surface();
        };
        let Some(vertex) = vertex_at(surface, at, Some(vertex_reach(cx))) else {
            return ToolStep::rejected("No vertex of the surface is within reach: click one.");
        };
        if held_vertices(surface)[vertex as usize] != 0 {
            return ToolStep::rejected(
                "A rim, breakline or boundary vertex is kept, not moved: it is the footprint or \
                 a line the surface keeps.",
            );
        }
        let p = surface.vertices()[vertex as usize];
        self.placed = Some(Point2::new(p.x, p.y));
        self.step = Step::Target;
        ToolStep::next("")
    }

    // The second: SURFACE EDIT ... MOVE POINT <vertex> TO <here>. The vertex
    // is named by its own coordinates, so no reach decides which one it is. A
    // refusal leaves the vertex held, for another click.
    fn move_to(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        let from = self.placed.unwrap_or_default();
        let step = Self::run(
            cx,
            vec![
                String::from("MOVE"),
                String::from("POINT"),
                coordinates(from),
                String::from("TO"),
                coordinates(at),
            ],
        );
        if step.outcome != ToolStepOutcome::Rejected {
            self.step = Step::Main;
            self.placed = None;
        }
        step
    }

    fn add_at(&mut self, cx: &ToolContext<'_>, level: f64) -> ToolStep {
        let at = self.placed.unwrap_or_default();
        self.step = Step::Main;
        self.placed = None;
        Self::run(
            cx,
            vec![
                String::from("ADD"),
                String::from("POINT"),
                format!("{},{}", coordinates(at), format_exact_real(level)),
            ],
        )
    }

    // The SURFACE EDIT line for `operation`, planned by the verb and handed
    // back as its one command.
    fn run(cx: &ToolContext<'_>, operation: Vec<String>) -> ToolStep {
        let (Some(name), Some(document)) = (surface_name(cx), cx.document) else {
            return no_surface();
        };
        let mut line = format!("SURFACE EDIT {}", surface_name_word(&name));
        let mut tokens = vec![String::from("SURFACE"), String::from("EDIT"), name];
        for word in operation {
            line.push(' ');
            line.push_str(&word);
            tokens.push(word);
        }
        let plan = match plan_surface_edit(document, &tokens, None) {
            Ok(plan) => plan,
            Err(error) => return ToolStep::rejected(as_sentence(&error.message)),
        };
        let message = format!("{line}\n{}", plan.reply);
        let Some(surfaces) = plan.surfaces else {
            return ToolStep::done(None, message, true);
        };
        // surfaces_command takes the document shared: it only builds the
        // command, which runs when the host executes it, so a tool lent the
        // document can make one.
        match document.surfaces_command(surfaces, plan.step_name) {
            Ok(command) => ToolStep::done(command, message, true),
            Err(error) => ToolStep::rejected(as_sentence(&error.message)),
        }
    }
}

impl InteractiveTool for SurfaceEditTool {
    fn prompt(&self, cx: &ToolContext<'_>) -> String {
        let Some(name) = surface_name(cx) else {
            return String::from(
                "There is no surface to edit: make one first (Terrain > Surface From), or press \
                 Esc",
            );
        };
        let on = format!(" of {name}");
        let placed = self.placed.unwrap_or_default();
        match self.step {
            Step::Surface => return format!("Surface to edit <{name}>"),
            Step::Level => {
                return format!(
                    "Level of the point at {}{}",
                    coordinates(placed),
                    match surface_level(cx, placed) {
                        Some(level) => format!(" <{}>", format_number(level)),
                        None => String::new(),
                    }
                );
            }
            Step::Target => {
                return format!(
                    "Click where the vertex of {name} at {} goes, or type x,y; it keeps its level",
                    coordinates(placed)
                );
            }
            Step::Main => {}
        }
        match self.kind {
            EditKind::DeleteTriangle => format!("Click the triangle{on} to delete or [Surface]"),
            EditKind::FlipEdge => format!("Click near the edge{on} to flip or [Surface]"),
            EditKind::AddPoint => {
                format!("Click where the new point of {name} goes, or type x,y,z, or [Surface]")
            }
            EditKind::DeletePoint => format!("Click the vertex{on} to delete or [Surface]"),
            EditKind::MovePoint => format!("Click the vertex{on} to move or [Surface]"),
            EditKind::DeleteInside => format!(
                "Pick the closed polyline to delete the triangles{on}{} or [Outside/Surface]",
                if self.outside.get() {
                    " OUTSIDE it"
                } else {
                    " inside it"
                }
            ),
            EditKind::Breakline => {
                format!("Pick the line or polyline to add to {name} as a breakline or [Surface]")
            }
            EditKind::Boundary => {
                format!("Pick the closed polyline to clip {name} to or [Surface]")
            }
            EditKind::Hole => format!("Pick the closed polyline to cut out of {name} or [Surface]"),
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.step == Step::Target {
            return ToolInput::Point;
        }
        if self.step != Step::Main {
            return ToolInput::Value;
        }
        if picks_entity(self.kind) {
            ToolInput::Entity
        } else {
            ToolInput::Point
        }
    }

    fn takes_heights(&self, _cx: &ToolContext<'_>) -> bool {
        self.kind == EditKind::AddPoint && self.step == Step::Main
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if self.step == Step::Target {
            return self.move_to(cx, at);
        }
        if self.step != Step::Main || picks_entity(self.kind) {
            return base_point(cx, self.expects(cx), at);
        }
        if surface_name(cx).is_none() {
            return no_surface();
        }
        match self.kind {
            EditKind::DeleteTriangle => Self::run(
                cx,
                vec![
                    String::from("DELETE"),
                    String::from("TRIANGLE"),
                    coordinates(at),
                ],
            ),
            EditKind::FlipEdge => Self::run(cx, vec![String::from("FLIP"), coordinates(at)]),
            EditKind::DeletePoint => Self::run(
                cx,
                vec![
                    String::from("DELETE"),
                    String::from("POINT"),
                    coordinates(at),
                    format!("reach={}", format_exact_real(vertex_reach(cx))),
                ],
            ),
            EditKind::AddPoint => {
                self.placed = Some(at);
                self.step = Step::Level;
                ToolStep::next("")
            }
            EditKind::MovePoint => self.pick_vertex(cx, at),
            _ => base_point(cx, self.expects(cx), at),
        }
    }

    fn point3d(&mut self, cx: &ToolContext<'_>, at: Point2, z: f64) -> ToolStep {
        if self.kind != EditKind::AddPoint || self.step != Step::Main {
            return self.point(cx, at);
        }
        if surface_name(cx).is_none() {
            return no_surface();
        }
        Self::run(
            cx,
            vec![
                String::from("ADD"),
                String::from("POINT"),
                format!("{},{}", coordinates(at), format_exact_real(z)),
            ],
        )
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if self.step != Step::Main || !picks_entity(self.kind) {
            return base_entity(cx, self.expects(cx), id, at);
        }
        let (Some(_), Some(document)) = (surface_name(cx), cx.document) else {
            return no_surface();
        };
        let Some(picked) = document.model().entities.find(id) else {
            return ToolStep::rejected("That entity is not in the drawing.");
        };
        let reference = format!("#{id}");
        match self.kind {
            EditKind::Breakline => {
                if !is_line(picked) {
                    return ToolStep::rejected(format!("Pick {LINE_KINDS}."));
                }
                Self::run(cx, vec![String::from("BREAKLINE"), reference])
            }
            EditKind::DeleteInside | EditKind::Boundary | EditKind::Hole => {
                if !is_polygon(picked) {
                    return ToolStep::rejected(format!("Pick {POLYGON_KINDS}."));
                }
                match self.kind {
                    EditKind::Boundary => Self::run(cx, vec![String::from("BOUNDARY"), reference]),
                    EditKind::Hole => Self::run(cx, vec![String::from("HOLE"), reference]),
                    _ => Self::run(
                        cx,
                        vec![
                            String::from("DELETE"),
                            String::from("TRIANGLES"),
                            String::from(if self.outside.get() {
                                "OUTSIDE"
                            } else {
                                "INSIDE"
                            }),
                            reference,
                        ],
                    ),
                }
            }
            _ => base_entity(cx, self.expects(cx), id, at),
        }
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if self.step == Step::Surface {
            return self.choose_surface(cx, text);
        }
        if self.step == Step::Target {
            return base_value(cx, text); // a place is a point, not a word
        }
        if self.step == Step::Level {
            let Some(level) = parse_number(text) else {
                return ToolStep::rejected("Type the point's level, a number.");
            };
            return self.add_at(cx, level);
        }
        if is_option(text, "Surface", "S") {
            self.step = Step::Surface;
            return ToolStep::next("");
        }
        if self.kind == EditKind::DeleteInside && is_option(text, "Outside", "O") {
            self.outside.set(!self.outside.get());
            return ToolStep::next(if self.outside.get() {
                "Deleting the triangles outside the polyline."
            } else {
                "Deleting the triangles inside the polyline."
            });
        }
        base_value(cx, text)
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Surface => {
                self.step = Step::Main; // the surface stays as it is
                ToolStep::next("")
            }
            Step::Level => match surface_level(cx, self.placed.unwrap_or_default()) {
                Some(level) => self.add_at(cx, level),
                None => ToolStep::rejected("The point is off the surface: type its level."),
            },
            Step::Target => {
                self.step = Step::Main; // the vertex is let go
                self.placed = None;
                ToolStep::next("")
            }
            Step::Main => ToolStep::done(None, "", false),
        }
    }

    fn undo(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        if self.step != Step::Main {
            self.step = Step::Main;
            self.placed = None;
            return ToolStep::next("");
        }
        base_undo(cx)
    }

    fn preview(&self, cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        let Some(surface) = active_surface(cx) else {
            return feedback;
        };
        if self.step == Step::Surface || picks_entity(self.kind) {
            return feedback;
        }
        let at = match (self.step, self.placed) {
            (Step::Level, Some(placed)) => placed,
            _ => cursor,
        };
        let triangle = triangle_at(surface, at);
        match self.kind {
            EditKind::DeleteTriangle => {
                let Some(triangle) = triangle else {
                    feedback.caption = String::from("off the surface");
                    feedback.refused = true;
                    return feedback;
                };
                feedback.marks.push(mark(
                    FeedbackRole::Removed,
                    Geometry::Polyline(outline(&surface.plan_triangle(triangle as usize))),
                    String::new(),
                ));
                feedback.caption = String::from("delete this triangle");
            }
            EditKind::FlipEdge => {
                let Some(edge) = edge_at(surface, at) else {
                    feedback.caption = String::from("off the surface");
                    feedback.refused = true;
                    return feedback;
                };
                let tri = surface.triangles()[edge.triangle as usize];
                let v = surface.vertices();
                let first = v[tri[edge.edge as usize] as usize];
                let second = v[tri[(edge.edge as usize + 1) % 3] as usize];
                let a = Point2::new(first.x, first.y);
                let b = Point2::new(second.x, second.y);
                feedback.marks.push(mark(
                    FeedbackRole::Target,
                    Geometry::Line(Segment2::new(a, b)),
                    String::new(),
                ));
                if surface.neighbors()[edge.triangle as usize][edge.edge as usize] == NO_TRIANGLE {
                    feedback.caption = String::from("an edge on the rim cannot be flipped");
                    feedback.refused = true;
                } else if surface.is_edge_constrained(edge.triangle as usize, edge.edge as usize) {
                    feedback.caption = String::from("a breakline is kept, not flipped");
                    feedback.refused = true;
                } else {
                    feedback.caption = String::from("flip this edge");
                }
            }
            EditKind::DeletePoint => {
                let Some(vertex) = vertex_at(surface, at, Some(vertex_reach(cx))) else {
                    feedback.caption = String::from("no vertex here");
                    feedback.refused = true;
                    return feedback;
                };
                let p = surface.vertices()[vertex as usize];
                let label = format!("z {}", format_number(p.z));
                // The rule the click is refused by (delete_points keeps the
                // vertex of a breakline or boundary edge), asked of the same
                // function, so what the preview promises is what the click
                // takes.
                if on_constrained_edge(surface, vertex) {
                    feedback.marks.push(mark(
                        FeedbackRole::Target,
                        vertex_mark(Point2::new(p.x, p.y)),
                        label,
                    ));
                    feedback.caption =
                        String::from("a breakline or boundary vertex is kept, not deleted");
                    feedback.refused = true;
                    return feedback;
                }
                feedback.marks.push(mark(
                    FeedbackRole::Removed,
                    vertex_mark(Point2::new(p.x, p.y)),
                    label,
                ));
                feedback.caption = String::from("delete this vertex");
            }
            EditKind::MovePoint => {
                if self.step == Step::Target {
                    // The vertex where it is, and where the click would put
                    // it.
                    let placed = self.placed.unwrap_or_default();
                    let z = surface.elevation_at(placed).unwrap_or(0.0);
                    feedback.marks.push(mark(
                        FeedbackRole::Removed,
                        vertex_mark(placed),
                        format!("z {}", format_number(z)),
                    ));
                    feedback
                        .marks
                        .push(mark(FeedbackRole::Added, vertex_mark(at), String::new()));
                    if triangle.is_none() {
                        feedback.caption =
                            String::from("off the surface: a vertex cannot be moved off it");
                        feedback.refused = true;
                    } else {
                        feedback.caption = String::from("move the vertex here, keeping its level");
                    }
                    return feedback;
                }
                let Some(vertex) = vertex_at(surface, at, Some(vertex_reach(cx))) else {
                    feedback.caption = String::from("no vertex here");
                    feedback.refused = true;
                    return feedback;
                };
                let p = surface.vertices()[vertex as usize];
                feedback.marks.push(mark(
                    FeedbackRole::Target,
                    vertex_mark(Point2::new(p.x, p.y)),
                    format!("z {}", format_number(p.z)),
                ));
                // The verb's own rule (move_point holds the rim and the ends
                // of constrained edges, as smoothing does), asked of the same
                // function.
                if held_vertices(surface)[vertex as usize] != 0 {
                    feedback.caption =
                        String::from("a rim, breakline or boundary vertex is kept, not moved");
                    feedback.refused = true;
                } else {
                    feedback.caption = String::from("move this vertex");
                }
            }
            EditKind::AddPoint => {
                if let Some(triangle) = triangle {
                    feedback.marks.push(mark(
                        FeedbackRole::Target,
                        Geometry::Polyline(outline(&surface.plan_triangle(triangle as usize))),
                        String::new(),
                    ));
                    if let Some(level) = surface.elevation_at(at) {
                        feedback.caption = format!("surface level {}", format_number(level));
                    }
                } else {
                    feedback.caption = String::from("off the surface: the point extends it");
                }
                feedback
                    .marks
                    .push(mark(FeedbackRole::Added, vertex_mark(at), String::new()));
            }
            _ => {}
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.placed
    }
}

fn edit_tool(
    id: &str,
    name: &str,
    order: i32,
    aliases: &[&str],
    tip: &str,
    kind: EditKind,
) -> ToolInfo {
    ToolInfo {
        id: String::from(id),
        name: String::from(name),
        category: String::from("Terrain"),
        group: String::from("Surface Edit"),
        order,
        aliases: aliases.iter().map(|&alias| String::from(alias)).collect(),
        tip: String::from(tip),
        // What the entry keeps from one remaking of the tool to the next is
        // the thread's OUTSIDE.
        make: Some(tool_factory(move |_cx, _start| -> ToolPtr {
            Box::new(SurfaceEditTool {
                kind,
                outside: OUTSIDE.with(Rc::clone),
                step: Step::Main,
                placed: None,
            })
        })),
        ..ToolInfo::default()
    }
}

pub fn add_terrain_edit_tools(catalog: &mut ToolCatalog, report: &mut Report<'_>) {
    report(catalog.add(edit_tool(
        "terrain.edit.deletetriangle",
        "Delete Triangle",
        10,
        &["TINDELTRI"],
        "Deletes the triangle of the active surface you click: SURFACE EDIT ... DELETE \
         TRIANGLE.",
        EditKind::DeleteTriangle,
    )));
    report(catalog.add(edit_tool(
        "terrain.edit.flip",
        "Flip Edge",
        20,
        &["TINFLIP"],
        "Swaps the edge of the active surface nearest the click for the other diagonal of its \
         two triangles: SURFACE EDIT ... FLIP.",
        EditKind::FlipEdge,
    )));
    report(catalog.add(edit_tool(
        "terrain.edit.addpoint",
        "Add Point",
        30,
        &["TINADDPT"],
        "Adds a point to the active surface where you click, at the level you type (Enter: the \
         surface's): SURFACE EDIT ... ADD POINT.",
        EditKind::AddPoint,
    )));
    report(catalog.add(edit_tool(
        "terrain.edit.deletepoint",
        "Delete Point",
        40,
        &["TINDELPT"],
        "Deletes the vertex of the active surface you click and fills the gap: SURFACE EDIT ... \
         DELETE POINT.",
        EditKind::DeletePoint,
    )));
    report(catalog.add(edit_tool(
        "terrain.edit.movepoint",
        "Move Point",
        45,
        &["TINMOVPT"],
        "Moves the vertex of the active surface you click to the place you click next, keeping \
         its level and re-triangulating round both: SURFACE EDIT ... MOVE POINT.",
        EditKind::MovePoint,
    )));
    report(catalog.add(edit_tool(
        "terrain.edit.deleteinside",
        "Delete Triangles Inside",
        50,
        &["TINDELIN"],
        "Deletes the triangles of the active surface inside a closed polyline you pick (O: \
         outside it): SURFACE EDIT ... DELETE TRIANGLES INSIDE.",
        EditKind::DeleteInside,
    )));
    report(catalog.add(edit_tool(
        "terrain.edit.breakline",
        "Add Breakline",
        60,
        &["TINBRK"],
        "Makes a line or polyline you pick edges of the active surface, at its levels, draped \
         where it has none: SURFACE EDIT ... BREAKLINE.",
        EditKind::Breakline,
    )));
    report(catalog.add(edit_tool(
        "terrain.edit.boundary",
        "Boundary",
        70,
        &["TINBOUNDARY"],
        "Clips the active surface to a closed polyline you pick, cutting its triangles exactly: \
         SURFACE EDIT ... BOUNDARY.",
        EditKind::Boundary,
    )));
    report(catalog.add(edit_tool(
        "terrain.edit.hole",
        "Hole",
        80,
        &["TINHOLE"],
        "Cuts what lies inside a closed polyline you pick out of the active surface: SURFACE \
         EDIT ... HOLE.",
        EditKind::Hole,
    )));
}
