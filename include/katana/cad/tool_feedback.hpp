#pragma once

// What a tool's preview says (interactive_tool.hpp, InteractiveTool::preview):
// the result as it will be (the ghost), where the tool measures from, and -
// by ROLE - what in the drawing already there a click here acts on, what it
// adds and what it takes away. The owner's request of 2026-09-30: inserting a
// vertex gave "no visual clue where the vertex is going". One dashed pen for
// everything could not say "this segment splits", "the new vertex lands
// here" or "this vertex goes"; a role can, and the view draws each role its
// own way (src/katana_qt/drawing/feedback_painter.hpp), so a tool never
// chooses a colour.
//
// In a header of its own so that cad/drawing/grips.hpp can build one for a
// grip's drag without including interactive_tool.hpp, which includes
// grips.hpp for a tool's handles (ToolContext::handles).

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

// What a mark says about the drawing already there.
enum class FeedbackRole : std::uint8_t {
    Target,  // what a click here takes or acts on: the vertex, the segment, the polyline
    Added,   // where something new lands: a vertex read from the RESULT, not the cursor
    Removed, // what the step takes away: a vertex deleted or straightened out, a piece cut
    // Where ENTER, not a click here, would put something: Insert Vertex
    // beside a chosen vertex adds at the middle of a segment the cursor may
    // not be on, and a place Enter uses must be seen before it is pressed.
    Enter,
};

// "target", "added", "removed", "enter": how the headless pointer record
// names them.
[[nodiscard]] const char* toString(FeedbackRole role);

struct FeedbackMark {
    FeedbackRole role = FeedbackRole::Target;
    // A PointGeometry is a VERTEX, drawn as a fixed-size glyph; anything
    // else is a PIECE (a segment, an arc, a whole polyline), drawn along its
    // geometry.
    katana::entity::Geometry geometry;
    // Beside a vertex glyph: its number, "keep", "z 101.500". Empty for none.
    std::string label;
};

// The rubber band: what the view draws for the current cursor position, in
// model coordinates, over the drawing. Every member is optional; a tool that
// fills only `shapes` and `markers` is drawn as every tool was before roles.
// The members added with the roles are initialised here, so the tools that
// build one as {shapes, markers} still name every member they must.
struct ToolFeedback {
    // The result as it will be, dashed in one pen: what a click here makes.
    // Only the pieces that change, never a whole polyline again.
    std::vector<katana::entity::Geometry> shapes;
    // Points the tool measures from: a base point, a polygon's centre.
    std::vector<katana::geometry::Point2> markers;
    // What the preview says about the drawing already there, by role.
    std::vector<FeedbackMark> marks{};
    // One line beside the cursor: what a click (or Enter) here does - "vertex
    // 2 between 1 and 2 · 15.000 from 1" - or, with `refused`, why it cannot.
    std::string caption{};
    // A click here would be refused, for the reason `caption` gives; the
    // view says so in the refusal's colour.
    bool refused = false;
    // The polyline in play, whose vertices the view shows while the tool
    // runs (the grips are hidden then).
    std::optional<katana::entity::EntityId> focus{};
};

} // namespace katana::cad
