// Terrain > Edit Surface (see families.hpp; docs/surface_editing.md, "In the
// window"): Delete Triangle, Flip Edge, Add Point, Delete Point, Delete
// Triangles Inside, Add Breakline, Boundary and Hole, each acting on the
// ACTIVE surface.
//
// A tool never edits a surface itself: what it was told becomes the SURFACE
// EDIT line a person would type, planned by the verb's own planSurfaceEdit
// and committed as the verb commits it - one undo step through
// Document::surfacesCommand - so the tool and the line cannot come to differ.
// The message the tool reports is that line, followed by the verb's reply.
//
// The active surface is surfaceEditTarget() when the drawing has it (the
// Surface Edit toolbar's choice), else the first visible surface. Every tool
// takes [Surface] at its prompt: S, then the name, which also becomes the
// toolbar's choice. Each restarts after an edit, as Point does, so a run of
// flips or deletions is a run of clicks; Esc ends it.

#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

#include "families.hpp"
#include "katana/cad/terrain/surface_edit_verbs.hpp"
#include "katana/core/text.hpp"
#include "katana/geometry/curves2d.hpp"
#include "katana/terrain/tin_edit.hpp"
#include "modify_edit_support.hpp"

namespace katana::cad::tools {

namespace {

using katana::entity::EntityId;
using katana::entity::PointGeometry;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using modify_edit::asSentence;
using modify_edit::formatNumber;
using modify_edit::isOption;
using modify_edit::parseNumber;

enum class EditKind {
    DeleteTriangle,
    FlipEdge,
    AddPoint,
    DeletePoint,
    DeleteInside,
    Breakline,
    Boundary,
    Hole,
};

bool picksEntity(EditKind kind)
{
    return kind == EditKind::DeleteInside || kind == EditKind::Breakline ||
           kind == EditKind::Boundary || kind == EditKind::Hole;
}

std::string coordinates(const Point2& at)
{
    return katana::core::formatExactReal(at.x) + "," + katana::core::formatExactReal(at.y);
}

// A closed shape a polygon is read from (surface_edit_verbs.cpp's polygonOf
// takes the same kinds; this only decides whether a pick is one).
bool isClosedShape(const katana::entity::Geometry& geometry)
{
    if (const auto* polyline = std::get_if<Polyline2>(&geometry)) {
        return polyline->closed ||
               (polyline->vertices.size() > 3 &&
                polyline->vertices.front() == polyline->vertices.back());
    }
    if (const auto* curve = std::get_if<katana::geometry::CurvePolyline2>(&geometry)) {
        return curve->closed;
    }
    if (const auto* ellipse = std::get_if<katana::geometry::Ellipse2>(&geometry)) {
        return ellipse->isFull();
    }
    return std::holds_alternative<katana::geometry::Circle2>(geometry);
}

bool isLinework(const katana::entity::Geometry& geometry)
{
    return std::holds_alternative<Segment2>(geometry) ||
           std::holds_alternative<Polyline2>(geometry) ||
           std::holds_alternative<katana::geometry::CurvePolyline2>(geometry);
}

Polyline2 outline(const katana::geometry::Triangle2& triangle)
{
    Polyline2 ring;
    ring.vertices = {triangle.a, triangle.b, triangle.c};
    ring.closed = true;
    return ring;
}

class SurfaceEditTool final : public InteractiveTool {
  public:
    SurfaceEditTool(const ToolContext& context, EditKind kind) : context_(context), kind_(kind) {}

    [[nodiscard]] std::string prompt() const override
    {
        const auto name = surfaceName();
        if (!name) {
            return "There is no surface to edit: make one first (Terrain > Surface From), or "
                   "press Esc";
        }
        const std::string on = " of " + *name;
        switch (step_) {
        case Step::Surface:
            return "Surface to edit <" + *name + ">";
        case Step::Level:
            return "Level of the point at " + coordinates(*placed_) +
                   (surfaceLevel(*placed_) ? " <" + formatNumber(*surfaceLevel(*placed_)) + ">"
                                           : std::string{});
        case Step::Main:
            break;
        }
        switch (kind_) {
        case EditKind::DeleteTriangle:
            return "Click the triangle" + on + " to delete or [Surface]";
        case EditKind::FlipEdge:
            return "Click near the edge" + on + " to flip or [Surface]";
        case EditKind::AddPoint:
            return "Click where the new point of " + *name +
                   " goes, or type x,y,z, or [Surface]";
        case EditKind::DeletePoint:
            return "Click the vertex" + on + " to delete or [Surface]";
        case EditKind::DeleteInside:
            return std::string("Pick the closed polyline to delete the triangles") + on +
                   (outside_ ? " OUTSIDE it" : " inside it") + " or [Outside/Surface]";
        case EditKind::Breakline:
            return "Pick the line or polyline to add to " + *name +
                   " as a breakline or [Surface]";
        case EditKind::Boundary:
            return "Pick the closed polyline to clip " + *name + " to or [Surface]";
        case EditKind::Hole:
            return "Pick the closed polyline to cut out of " + *name + " or [Surface]";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        if (step_ != Step::Main) {
            return ToolInput::Value;
        }
        return picksEntity(kind_) ? ToolInput::Entity : ToolInput::Point;
    }

    [[nodiscard]] bool takesHeights() const override
    {
        return kind_ == EditKind::AddPoint && step_ == Step::Main;
    }

    ToolStep point(const Point2& at) override
    {
        if (step_ != Step::Main || picksEntity(kind_)) {
            return InteractiveTool::point(at);
        }
        if (!surfaceName()) {
            return noSurface();
        }
        switch (kind_) {
        case EditKind::DeleteTriangle:
            return run({"DELETE", "TRIANGLE", coordinates(at)});
        case EditKind::FlipEdge:
            return run({"FLIP", coordinates(at)});
        case EditKind::DeletePoint:
            return run({"DELETE", "POINT", coordinates(at),
                        "reach=" + katana::core::formatExactReal(vertexReach(context_))});
        case EditKind::AddPoint:
            placed_ = at;
            step_ = Step::Level;
            return ToolStep::next();
        default:
            return InteractiveTool::point(at);
        }
    }

    ToolStep point3d(const Point2& at, double z) override
    {
        if (kind_ != EditKind::AddPoint || step_ != Step::Main) {
            return point(at);
        }
        if (!surfaceName()) {
            return noSurface();
        }
        return run({"ADD", "POINT", coordinates(at) + "," + katana::core::formatExactReal(z)});
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        if (step_ != Step::Main || !picksEntity(kind_)) {
            return InteractiveTool::entity(id, at);
        }
        if (!surfaceName()) {
            return noSurface();
        }
        const auto* picked = context_.document->model().entities.find(id);
        if (picked == nullptr) {
            return ToolStep::rejected("That entity is not in the drawing.");
        }
        const std::string ref = "#" + std::to_string(id);
        switch (kind_) {
        case EditKind::Breakline:
            if (!isLinework(picked->geometry)) {
                return ToolStep::rejected("Pick a line, polyline or curve polyline.");
            }
            return run({"BREAKLINE", ref});
        case EditKind::DeleteInside:
        case EditKind::Boundary:
        case EditKind::Hole:
            if (!isClosedShape(picked->geometry)) {
                return ToolStep::rejected(
                    "Pick a closed polyline, curve polyline, circle or ellipse.");
            }
            if (kind_ == EditKind::Boundary) {
                return run({"BOUNDARY", ref});
            }
            if (kind_ == EditKind::Hole) {
                return run({"HOLE", ref});
            }
            return run({"DELETE", "TRIANGLES", outside_ ? "OUTSIDE" : "INSIDE", ref});
        default:
            return InteractiveTool::entity(id, at);
        }
    }

    ToolStep value(std::string_view text) override
    {
        if (step_ == Step::Surface) {
            return chooseSurface(text);
        }
        if (step_ == Step::Level) {
            const auto level = parseNumber(text);
            if (!level) {
                return ToolStep::rejected("Type the point's level, a number.");
            }
            return addAt(*level);
        }
        if (isOption(text, "Surface", "S")) {
            step_ = Step::Surface;
            return ToolStep::next();
        }
        if (kind_ == EditKind::DeleteInside && isOption(text, "Outside", "O")) {
            outside_ = !outside_;
            return ToolStep::next(outside_ ? "Deleting the triangles outside the polyline."
                                           : "Deleting the triangles inside the polyline.");
        }
        return InteractiveTool::value(text);
    }

    ToolStep enter() override
    {
        if (step_ == Step::Surface) {
            step_ = Step::Main; // the surface stays as it is
            return ToolStep::next();
        }
        if (step_ == Step::Level) {
            if (const auto level = surfaceLevel(*placed_)) {
                return addAt(*level);
            }
            return ToolStep::rejected("The point is off the surface: type its level.");
        }
        return ToolStep::done(nullptr);
    }

    ToolStep undo() override
    {
        if (step_ == Step::Level || step_ == Step::Surface) {
            step_ = Step::Main;
            placed_.reset();
            return ToolStep::next();
        }
        return InteractiveTool::undo();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        const auto* surface = activeSurface();
        if (surface == nullptr || step_ == Step::Surface || picksEntity(kind_)) {
            return feedback;
        }
        const Point2 at = step_ == Step::Level && placed_ ? *placed_ : cursor;
        const auto triangle = katana::terrain::triangleAt(*surface, at);
        switch (kind_) {
        case EditKind::DeleteTriangle:
            if (!triangle) {
                feedback.caption = "off the surface";
                feedback.refused = true;
                return feedback;
            }
            feedback.marks.push_back(
                {FeedbackRole::Removed, outline(surface->planTriangle(*triangle)), {}});
            feedback.caption = "delete this triangle";
            return feedback;
        case EditKind::FlipEdge: {
            const auto edge = katana::terrain::edgeAt(*surface, at);
            if (!edge) {
                feedback.caption = "off the surface";
                feedback.refused = true;
                return feedback;
            }
            const auto& tri = surface->triangles()[edge->triangle];
            const auto& v = surface->vertices();
            const Point2 a(v[tri[edge->edge]].x, v[tri[edge->edge]].y);
            const Point2 b(v[tri[(edge->edge + 1) % 3]].x, v[tri[(edge->edge + 1) % 3]].y);
            feedback.marks.push_back({FeedbackRole::Target, Segment2{a, b}, {}});
            if (surface->neighbors()[edge->triangle][edge->edge] == katana::terrain::kNoTriangle) {
                feedback.caption = "an edge on the rim cannot be flipped";
                feedback.refused = true;
            } else if (surface->isEdgeConstrained(edge->triangle, edge->edge)) {
                feedback.caption = "a breakline is kept, not flipped";
                feedback.refused = true;
            } else {
                feedback.caption = "flip this edge";
            }
            return feedback;
        }
        case EditKind::DeletePoint: {
            const auto vertex = katana::terrain::vertexAt(*surface, at, vertexReach(context_));
            if (!vertex) {
                feedback.caption = "no vertex here";
                feedback.refused = true;
                return feedback;
            }
            const auto& p = surface->vertices()[*vertex];
            feedback.marks.push_back({FeedbackRole::Removed, PointGeometry{Point2(p.x, p.y)},
                                      "z " + formatNumber(p.z)});
            feedback.caption = "delete this vertex";
            return feedback;
        }
        case EditKind::AddPoint:
            if (triangle) {
                feedback.marks.push_back(
                    {FeedbackRole::Target, outline(surface->planTriangle(*triangle)), {}});
                if (const auto level = surface->elevationAt(at)) {
                    feedback.caption = "surface level " + formatNumber(*level);
                }
            } else {
                feedback.caption = "off the surface: the point extends it";
            }
            feedback.marks.push_back({FeedbackRole::Added, PointGeometry{at}, {}});
            return feedback;
        default:
            return feedback;
        }
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override { return placed_; }

  private:
    enum class Step { Main, Level, Surface };

    [[nodiscard]] std::optional<std::string> surfaceName() const
    {
        if (context_.document == nullptr) {
            return std::nullopt;
        }
        auto name = activeEditSurface(*context_.document);
        return name ? std::optional<std::string>(*name) : std::nullopt;
    }

    [[nodiscard]] const katana::terrain::TinSurface* activeSurface() const
    {
        const auto name = surfaceName();
        if (!name) {
            return nullptr;
        }
        const auto* named = context_.document->surfaces().find(*name);
        return named != nullptr ? named->surface.get() : nullptr;
    }

    [[nodiscard]] std::optional<double> surfaceLevel(const Point2& at) const
    {
        const auto* surface = activeSurface();
        return surface != nullptr ? surface->elevationAt(at) : std::nullopt;
    }

    [[nodiscard]] static ToolStep noSurface()
    {
        return ToolStep::rejected("There is no surface to edit: make one first (Terrain > "
                                  "Surface From).");
    }

    ToolStep chooseSurface(std::string_view text)
    {
        const std::string name(katana::core::trimmed(text));
        const auto* named = context_.document->surfaces().find(name);
        if (named == nullptr) {
            return ToolStep::rejected("The drawing has no surface called " + name + ".");
        }
        setSurfaceEditTarget(named->name);
        step_ = Step::Main;
        return ToolStep::next("Editing " + named->name + ".");
    }

    ToolStep addAt(double level)
    {
        const Point2 at = *placed_;
        step_ = Step::Main;
        placed_.reset();
        return run({"ADD", "POINT", coordinates(at) + "," + katana::core::formatExactReal(level)});
    }

    // The SURFACE EDIT line for `operation`, planned by the verb and handed
    // back as its one command.
    ToolStep run(std::vector<std::string> operation)
    {
        const auto name = surfaceName();
        if (!name) {
            return noSurface();
        }
        std::vector<std::string> tokens{"SURFACE", "EDIT", *name};
        std::string line = "SURFACE EDIT " + surfaceNameWord(*name);
        for (std::string& word : operation) {
            line += " " + word;
            tokens.push_back(std::move(word));
        }
        auto plan = planSurfaceEdit(*context_.document, tokens, {});
        if (!plan) {
            return ToolStep::rejected(asSentence(plan.error().message));
        }
        if (!plan->surfaces) {
            return ToolStep::done(nullptr, line + "\n" + plan->reply, true);
        }
        // surfacesCommand only builds the command, capturing the document it
        // will change when the host executes it; the document a tool is given
        // is const so that it cannot change it before then, and building the
        // command does not.
        auto command = const_cast<Document*>(context_.document)
                           ->surfacesCommand(std::move(*plan->surfaces), plan->stepName);
        if (!command) {
            return ToolStep::rejected(asSentence(command.error().message));
        }
        return ToolStep::done(std::move(*command), line + "\n" + plan->reply, true);
    }

    ToolContext context_;
    EditKind kind_;
    Step step_ = Step::Main;
    std::optional<Point2> placed_;
    bool outside_ = false;
};

ToolInfo editTool(std::string id, std::string name, int order, std::vector<std::string> aliases,
                  std::string tip, EditKind kind)
{
    ToolInfo info;
    info.id = std::move(id);
    info.name = std::move(name);
    info.category = "Terrain";
    info.group = "Surface Edit";
    info.order = order;
    info.aliases = std::move(aliases);
    info.tip = std::move(tip);
    info.make = [kind](const ToolContext& context) -> std::unique_ptr<InteractiveTool> {
        return std::make_unique<SurfaceEditTool>(context, kind);
    };
    return info;
}

} // namespace

void addTerrainEditTools(ToolCatalog& catalog, const Report& report)
{
    report(catalog.add(editTool("terrain.edit.deletetriangle", "Delete Triangle", 10,
                                {"TINDELTRI"},
                                "Deletes the triangle of the active surface you click: SURFACE "
                                "EDIT ... DELETE TRIANGLE.",
                                EditKind::DeleteTriangle)));
    report(catalog.add(editTool("terrain.edit.flip", "Flip Edge", 20, {"TINFLIP"},
                                "Swaps the edge of the active surface nearest the click for the "
                                "other diagonal of its two triangles: SURFACE EDIT ... FLIP.",
                                EditKind::FlipEdge)));
    report(catalog.add(editTool("terrain.edit.addpoint", "Add Point", 30, {"TINADDPT"},
                                "Adds a point to the active surface where you click, at the "
                                "level you type (Enter: the surface's): SURFACE EDIT ... ADD "
                                "POINT.",
                                EditKind::AddPoint)));
    report(catalog.add(editTool("terrain.edit.deletepoint", "Delete Point", 40, {"TINDELPT"},
                                "Deletes the vertex of the active surface you click and fills "
                                "the gap: SURFACE EDIT ... DELETE POINT.",
                                EditKind::DeletePoint)));
    report(catalog.add(editTool("terrain.edit.deleteinside", "Delete Triangles Inside", 50,
                                {"TINDELIN"},
                                "Deletes the triangles of the active surface inside a closed "
                                "polyline you pick (O: outside it): SURFACE EDIT ... DELETE "
                                "TRIANGLES INSIDE.",
                                EditKind::DeleteInside)));
    report(catalog.add(editTool("terrain.edit.breakline", "Add Breakline", 60, {"TINBRK"},
                                "Makes a line or polyline you pick edges of the active surface, "
                                "at its levels, draped where it has none: SURFACE EDIT ... "
                                "BREAKLINE.",
                                EditKind::Breakline)));
    report(catalog.add(editTool("terrain.edit.boundary", "Boundary", 70, {"TINBOUNDARY"},
                                "Clips the active surface to a closed polyline you pick, cutting "
                                "its triangles exactly: SURFACE EDIT ... BOUNDARY.",
                                EditKind::Boundary)));
    report(catalog.add(editTool("terrain.edit.hole", "Hole", 80, {"TINHOLE"},
                                "Cuts what lies inside a closed polyline you pick out of the "
                                "active surface: SURFACE EDIT ... HOLE.",
                                EditKind::Hole)));
}

} // namespace katana::cad::tools
