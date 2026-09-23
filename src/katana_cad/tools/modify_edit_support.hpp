#pragma once

// What the Modify > Edit tools share (Trim, Extend, Offset, Fillet, Chamfer,
// Break, Break at Point, Join, Explode; see modify_edit.cpp): a session that
// holds what a tool has changed so far without touching the document, the
// heights a 3D string carries through an edit, a polyline measured along its
// length, and the typed-option conventions. In a family namespace because the
// other families are written at the same time and may well have helpers of
// the same names.

#include <cstddef>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/interactive_tool.hpp"
#include "katana/geometry/editing.hpp"

namespace katana::cad::tools::modify_edit {

using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::Geometry;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Curve2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

// What the tools remember from one use to the next, as AutoCAD remembers
// FILLETRAD, CHAMFERA/CHAMFERB and OFFSETDIST: a user who fillets with radius 5
// expects the next fillet to be radius 5 without typing it again. One object
// per program, shared by the tools' factories (modify_edit.cpp); nothing is
// stored in the drawing.
struct EditDefaults {
    double filletRadius = 0.0;
    double chamferFirst = 0.0;
    double chamferSecond = 0.0;
    // Nullopt: Through, which is also AutoCAD's default before a distance is typed.
    std::optional<double> offsetDistance;
    // How far apart two ends may be and still be joined. A millimetre in
    // metres: survey linework digitised or imported from another package often
    // misses by less than that, and a user should not have to snap every end.
    double joinTolerance = 0.001;
};

// ---- typed input -------------------------------------------------------------------

// True when `text` is the option `word` or its short form, in any case:
// "R", "r" and "radius" all choose [Radius].
[[nodiscard]] bool isOption(std::string_view text, std::string_view word,
                            std::string_view shortForm);
// A typed number, through core/text.hpp so the decimal point is always '.'.
[[nodiscard]] std::optional<double> parseNumber(std::string_view text);
// A number as a prompt shows it: the shortest text that reads back exactly.
[[nodiscard]] std::string formatNumber(double value);
// An error from a command or the geometry layer as a sentence for the
// command line: capitalised, with a full stop.
[[nodiscard]] std::string asSentence(std::string_view text);

// ---- the drawing -------------------------------------------------------------------

// "line", "arc", "circle", "polyline", "point", "text", "dimension".
[[nodiscard]] std::string kindName(const Geometry& geometry);
// Why `id` cannot be edited - it is not in the drawing, or its layer is
// locked - or nullopt when it can. A tool refuses the pick up front with this
// rather than returning a command the document would refuse.
[[nodiscard]] std::optional<std::string> refusalToEdit(const Document& document, EntityId id);

// The entities a Selection step ends with: what the tool was given or picked,
// and the document's selection as it is when Enter is pressed (the view
// selects as the Select tool does while a tool asks for a selection).
// Ascending, without duplicates.
[[nodiscard]] std::vector<EntityId> selectionNow(const ToolContext& context,
                                                 const std::vector<EntityId>& picked);

// ---- heights -----------------------------------------------------------------------
//
// A 3D string keeps a height per vertex in its properties (entity.hpp:
// setHeights / heightsOf). An edit that moves or adds vertices must say what
// the new vertices' heights are, or the list no longer matches the geometry
// and the readers drop it. A vertex that lies on the original gets the height
// interpolated there; one that does not (an extended end) gets none, because
// absent is not zero and a guess is not a survey.

// The points heights are kept for: a line's and an arc's two ends, a
// polyline's vertices, a circle's centre, a point's position.
[[nodiscard]] std::vector<Point2> heightVertices(const Geometry& geometry);
// The height the original has at `p`, when `p` lies on it and the heights on
// either side are known.
[[nodiscard]] std::optional<double> heightAt(const Entity& original, const Point2& p);
// Sets `piece`'s heights from where its vertices lie on `original`.
void carryHeights(const Entity& original, Entity& piece);

// ---- curves ------------------------------------------------------------------------

// The curves an entity cuts or bounds with: a line, arc or circle is itself,
// a polyline is its segments, anything else is nothing.
void appendEdges(const Geometry& geometry, std::vector<Curve2>& out);
[[nodiscard]] Geometry asGeometry(const Curve2& curve);
[[nodiscard]] std::optional<Curve2> asCurve(const Geometry& geometry);

// A polyline measured along its length ("stations", from its first vertex),
// so a break or a trim can say "from here to there" across vertices.
class PolylinePath {
  public:
    explicit PolylinePath(Polyline2 polyline);

    [[nodiscard]] const Polyline2& polyline() const { return polyline_; }
    [[nodiscard]] bool closed() const { return polyline_.closed; }
    // Including the closing segment of a closed polyline.
    [[nodiscard]] double length() const { return stations_.back(); }
    // The station of the point of the path nearest to `p`.
    [[nodiscard]] double stationOf(const Point2& p) const;
    [[nodiscard]] double stationOf(std::size_t segment, const Point2& pointOnSegment) const;
    [[nodiscard]] Point2 pointAt(double station) const;
    [[nodiscard]] std::size_t segmentCount() const { return path_.size() - 1; }
    [[nodiscard]] Segment2 segment(std::size_t index) const
    {
        return Segment2{path_[index], path_[index + 1]};
    }
    // The open polyline from station `from` to `to` (from < to), keeping every
    // vertex between.
    [[nodiscard]] Polyline2 between(double from, double to) const;
    // Closed paths: from `from` forward through the first vertex to `to`.
    [[nodiscard]] Polyline2 wrapping(double from, double to) const;

  private:
    Polyline2 polyline_;
    std::vector<Point2> path_;     // the vertices, and the first again when closed
    std::vector<double> stations_; // the station of each of path_
};

// ---- the session -------------------------------------------------------------------

// What a tool has changed so far, as a layer over the document. Trim takes
// several picks, and a later pick can land on a piece an earlier one made -
// which has no id until a command runs - so the tool edits this instead, and
// when it finishes turns the whole session into ONE command: one undo takes
// back everything the tool did, as AutoCAD's U does after a TRIM.
class EditSession {
  public:
    explicit EditSession(const Document* document) : document_(document) {}

    [[nodiscard]] const Document& document() const { return *document_; }
    // The entity as the document has it, or nullptr.
    [[nodiscard]] const Entity* original(EntityId id) const;
    // What the session has made of `id`: the document's entity until it is
    // edited, then its pieces (the first keeps the id); empty once removed.
    [[nodiscard]] std::vector<Entity> pieces(EntityId id) const;
    // New entities (offsets, fillet arcs), in the order made.
    [[nodiscard]] const std::vector<Entity>& added() const { return state_.added; }
    [[nodiscard]] bool edited(EntityId id) const { return state_.edited.contains(id); }
    [[nodiscard]] std::vector<EntityId> editedIds() const;
    // Everything the session has made, for a preview: the pieces of every
    // edited entity and every added one.
    [[nodiscard]] std::vector<Geometry> shapes() const;

    // Marks the start of one operation, which undo() takes back whole.
    void begin();
    void replace(EntityId id, std::vector<Entity> pieces);
    void add(Entity entity);
    // Back to before the last begin(). False when there is nothing to undo.
    bool undo();
    [[nodiscard]] std::size_t operations() const { return history_.size(); }

    // Everything as one command: pieces[0] modifies its entity, further
    // pieces and added entities are created, an emptied entity is removed.
    [[nodiscard]] katana::commands::CommandPtr commit(std::string name) const;

  private:
    struct State {
        std::map<EntityId, std::vector<Entity>> edited;
        std::vector<Entity> added;
    };

    const Document* document_ = nullptr;
    State state_;
    std::vector<State> history_;
};

// The piece of `pieces` nearest to `at`, or nullopt when there are none.
[[nodiscard]] std::optional<std::size_t> nearestPiece(const std::vector<Entity>& pieces,
                                                      const Point2& at);

// ---- the tools (one file each; registered in modify_edit.cpp) ----------------------

using ToolPtr = std::unique_ptr<InteractiveTool>;
using Defaults = std::shared_ptr<EditDefaults>;

[[nodiscard]] ToolPtr makeTrimTool(const ToolContext& context);
[[nodiscard]] ToolPtr makeExtendTool(const ToolContext& context);
[[nodiscard]] ToolPtr makeOffsetTool(const ToolContext& context, Defaults defaults);
[[nodiscard]] ToolPtr makeFilletTool(const ToolContext& context, Defaults defaults);
[[nodiscard]] ToolPtr makeChamferTool(const ToolContext& context, Defaults defaults);
[[nodiscard]] ToolPtr makeBreakTool(const ToolContext& context);
[[nodiscard]] ToolPtr makeBreakAtPointTool(const ToolContext& context);
[[nodiscard]] ToolPtr makeJoinTool(const ToolContext& context, Defaults defaults);
[[nodiscard]] ToolPtr makeExplodeTool(const ToolContext& context);

} // namespace katana::cad::tools::modify_edit
