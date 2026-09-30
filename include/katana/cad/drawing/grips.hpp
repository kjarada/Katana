#pragma once

// Grips: the handles a CAD user drags to reshape what is selected, with no
// tool running (docs/drawing.md, "Grips"). The owner's request was vertex
// control, and the grip is its most direct form: click a vertex of the
// selected string, drag it where it goes, and that is one undo step.
//
// Everything a grip does is decided HERE, headless and per geometry kind -
// which grips an entity offers (gripsOf) and what dragging a set of them does
// (applyGripDrag, gripDragCommand) - so it is unit tested per kind with no
// view anywhere. The plan view only draws the grips it is given, finds the
// one under the cursor, and forwards the drag (src/katana_qt/drawing/).
//
// A drag moves the grabbed grip to a TARGET point (already snapped, tracked
// or typed by the view) and every other hot grip by the same displacement,
// so several vertices picked with Shift move together. What "move" means is
// the grip's kind:
//
//   Vertex            the polyline's vertex moves; each arc keeps its bulge
//   SegmentMid        a straight segment moves bodily (both ends: a stretch),
//                     an arc segment is reshaped through the target; with
//                     insertVertex (Ctrl) a new vertex is made there instead
//   End / Mid         a line's end moves, its middle moves it whole; an arc's
//                     end or middle reshapes it through the other two points
//   Centre            a circle, arc or ellipse moves whole
//   Quadrant          a circle's radius becomes the distance to the target;
//                     an ellipse's axis end sets that axis
//   Insertion         a point or a text moves
//   FitPoint          a spline's fit point moves and the spline is re-solved
//   ControlPoint      a spline's control point moves (a spline with no fit
//                     points)

#include <cstddef>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/commands/command.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"

namespace katana::cad {

enum class GripKind {
    Vertex,
    SegmentMid,
    End,
    Mid,
    Centre,
    Quadrant,
    Insertion,
    FitPoint,
    ControlPoint,
};

[[nodiscard]] const char* toString(GripKind kind);

struct Grip {
    katana::entity::EntityId entity = katana::entity::kInvalidEntityId;
    GripKind kind = GripKind::Vertex;
    // Which one: a vertex, a segment, an end (0 start, 1 end), a quadrant
    // (0 east of the axis, counter-clockwise), a fit or control point.
    std::size_t index = 0;
    katana::geometry::Point2 position;

    // The same handle (entity, kind, index), whatever its position.
    [[nodiscard]] bool sameHandle(const Grip& other) const
    {
        return entity == other.entity && kind == other.kind && index == other.index;
    }
    friend bool operator==(const Grip&, const Grip&) = default;
};

// The grips an entity offers, in a fixed order (vertices first, then segment
// middles, and so on), each with its position.
[[nodiscard]] std::vector<Grip> gripsOf(const katana::entity::Entity& entity);

// The grips of every entity in `ids` that `view` lets be edited (drawn there,
// on an unlocked layer: isSelectable), capped at `limit` in all: a selection
// of every entity of a large survey would otherwise draw a million squares -
// past the cap the view shows none, as CAD programs do past their grip limit.
// A selected entity on a layer `view` hides offers no grip there: that view
// shows it as a ghost at most (docs/desktop.md, "The selection in every
// view"), which is never picked - a grip on it dragged a design line from the
// as-built view that hid it. `view` has no default, as isSelectable's has
// none: kNoLayerOverrides (selection.hpp) for the document's rule alone.
[[nodiscard]] std::vector<Grip> gripsOfSelection(const Document& document,
                                                 const std::vector<katana::entity::EntityId>& ids,
                                                 const LayerOverrides& view,
                                                 std::size_t limit = 20000);

// The grip within `aperture` of `at`, nearest first; nullopt when none.
[[nodiscard]] std::optional<Grip> gripAt(const std::vector<Grip>& grips,
                                         const katana::geometry::Point2& at, double aperture);

struct GripDrag {
    Grip grabbed;                        // the grip the drag started on
    katana::geometry::Point2 target;     // where it goes
    std::vector<Grip> alsoHot;           // other hot grips, moved by the same displacement
    bool insertVertex = false;           // Ctrl on a segment middle: insert, do not stretch
};

// `entity` as the drag leaves it, for the grips of `drag` that belong to it
// (others are ignored). Fails with a sentence when the result would not be
// valid geometry (a circle of no radius, a polyline folded onto itself).
[[nodiscard]] katana::core::Result<katana::entity::Entity>
applyGripDrag(const katana::entity::Entity& entity, const GripDrag& drag);

// The whole drag as ONE command (GRIP_EDIT) over every entity it touches.
[[nodiscard]] katana::commands::CommandPtr gripDragCommand(GripDrag drag);

// Delete on hot vertex grips: removes those vertices, as ONE command
// (VERTEX_DELETE). Refused when no hot grip is a vertex, or a polyline would
// be left with too few.
[[nodiscard]] katana::commands::CommandPtr deleteHotVertices(std::vector<Grip> hot);

// What the drag would draw: the edited geometry of every touched entity,
// for the view's rubber band. Entities the drag cannot edit are left out.
[[nodiscard]] std::vector<katana::entity::Geometry> gripPreview(const Document& document,
                                                                const GripDrag& drag);

} // namespace katana::cad
