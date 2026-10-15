#pragma once

// The drawing's polygons and lines as the verbs that edit or measure a
// surface by them read them from their words: SURFACE EDIT's <polygon> and
// <line> (surface_edit_verbs.hpp, docs/surface_editing.md) and the VOLUME
// verbs' INSIDE <polygon> (volume_verbs.hpp, docs/volumes.md).
//
//   <polygon> | <line> := #id [#id ...] | x,y[,z] x,y[,z] ... | FROM <scope> [WHERE k=v ...]
//
// The two families each had a reader of their own, written side by side,
// and they had come to disagree: one took a full ellipse and a polyline
// drawn back to its start and the other did not, one kept the outline's
// heights and the other dropped them, one took FROM with no scope word as
// the selection and the other refused it. This is the one reader, and where
// they differed its rule is the one that takes more (docs/volumes.md,
// "Inside a polygon").
//
// WHAT AN ENTITY GIVES. As a polygon, closed, its corners never repeating
// the first at the end:
//   - a closed polyline, or an open one whose last vertex is its first
//     (within math::tolerance::kGeometric), drawn back to its start: its
//     vertices, each at the height its elevation or elevations property
//     gives it (entity::heightsOf, the one reader of them);
//   - a closed curve polyline, or an open one back at its start: its
//     straight segments exact and its arcs chorded within
//     geometry::kCurveChordTolerance (1 mm sagitta), a vertex at its own
//     height and a chord point at the height interpolated along its arc,
//     none where an end has none (CurvePolyline2::tessellateWithHeights);
//   - a circle, and a full ellipse: chorded within kCurveChordTolerance,
//     every corner at the entity's elevation property when it has one;
//   - none for fewer than three corners left (TooFew); for a line, an arc,
//     an elliptical arc or another open polyline, which bound nothing
//     (Open); and for a point, text, a spline, a dimension ..., which are no
//     outline at all (NoOutline). A spline is never closed (Spline2 has no
//     such flag), so it is no outline rather than an open one.
// As a line, its vertices in order, closed when the entity is (and then
// without the first repeated at the end):
//   - a line; a polyline; a curve polyline, chorded as above; with the
//     heights as above;
//   - none for fewer than two vertices (TooFew), and for anything else
//     (NoOutline).
//
// TYPED CORNERS are x,y or x,y,z, each a finite number, the third the
// corner's height. Typed as a polygon they are closed, the last dropped
// when it repeats the first, and need three corners; as a line, two.
//
// FROM <scope> is the one scope grammar (scope_verbs.hpp), resolved by the
// one matcher; FROM with no scope word after it is the selection, as the
// grammar says of no scope word. What a scope takes that is no shape is
// counted by why, never an error here: a scope that takes nothing is
// reported (CLAUDE.md, section 1.1), and what that means is the verb's.
//
// NOTHING HERE IS WORDED FOR A REPLY. The reader gives the data, the counts
// and why a #id or the typed corners are no shape; each verb family writes
// its own records and refusals from them, as its tests and its users know
// them. The one exception is the list of kinds a refusal names
// (kPolygonKinds), kept here so that it changes with the rule.

#include <cstddef>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

// The kinds a polygon is made from, and a line, as a refusal names them, so
// the sentence and the rule above are changed together.
inline constexpr std::string_view kPolygonKinds =
    "a closed polyline, curve polyline, circle or ellipse";
inline constexpr std::string_view kLineKinds = "a line, polyline or curve polyline";

// ---- typed corners ---------------------------------------------------------------------------

struct TypedCorner {
    katana::geometry::Point2 at;
    std::optional<double> z{};
};

// "x,y" or "x,y,z", each part a finite number (blanks around a part
// allowed); nullopt for anything else, so a word that is not a corner ends a
// run of them.
[[nodiscard]] std::optional<TypedCorner> parseTypedCorner(std::string_view word);

// ---- one shape ---------------------------------------------------------------------------------

enum class ShapeKind { Polygon, Line };

struct DrawingShape {
    // The entity it is; empty for typed corners.
    std::optional<katana::entity::EntityId> entity{};
    // Its corners or vertices in order; a polygon's closed and never
    // repeating its first corner at the end.
    katana::geometry::Polyline2 outline{};
    // One per vertex of `outline`; empty optional where a vertex has none.
    std::vector<std::optional<double>> heights{};
    // An arc, a circle or an ellipse was chorded to make it.
    bool chorded = false;
};

// Why an entity, or the typed corners, are no shape of the kind asked for.
enum class NotAShape {
    NoEntity,  // a #id the drawing has no entity of
    Open,      // a polygon asked: a line, an arc, an open polyline bounds nothing
    TooFew,    // under three corners for a polygon, two vertices for a line
    NoOutline, // none of the kinds the rule takes
};

struct ShapeOf {
    std::optional<DrawingShape> shape{};
    // Why there is none; meaningless when there is one.
    NotAShape why = NotAShape::NoOutline;
};

// The shape `entity` is as `kind`, by the rules above, its `entity` set.
[[nodiscard]] ShapeOf shapeOf(const katana::entity::Entity& entity, ShapeKind kind);

// The shape typed corners make as `kind`.
[[nodiscard]] ShapeOf shapeOf(const std::vector<TypedCorner>& corners, ShapeKind kind);

// ---- the words ---------------------------------------------------------------------------------

// What the words of a <polygon> or <line> name, read and nothing looked up.
// At most one of ids, corners and scope is given; none when the word at
// `at` is neither #id, FROM nor a corner, or there is no word.
struct ShapeWords {
    std::vector<katana::entity::EntityId> ids{};
    std::vector<TypedCorner> corners{};
    std::optional<ScopeWords> scope{};
    // A word after # that is not # and a whole number from 1: the reading
    // stopped on it, and the verb refuses it in its own words.
    std::optional<std::string> badId{};

    [[nodiscard]] bool empty() const
    {
        return ids.empty() && corners.empty() && !scope && !badId;
    }
};

// Reads a <polygon> or <line> from `words` at `at`, leaving `at` after it:
// one or more #ids, typed corners, or FROM and a scope. Fails only as
// parseScopeWords fails, with its refusal.
[[nodiscard]] katana::core::Result<ShapeWords> readShapeWords(const std::vector<std::string>& words,
                                                              std::size_t& at);

// ---- what the words name in the drawing ---------------------------------------------------------

// The entities `words` name: their #ids as given, or what their scope
// matched (ascending, matchScope's order).
struct NamedEntities {
    std::vector<katana::entity::EntityId> ids{};
    // FROM: what the scope took.
    std::optional<ScopeMatch> scope{};
    // #ids: the first the drawing has no entity of, where the naming
    // stopped; `ids` are those before it.
    std::optional<katana::entity::EntityId> missing{};
};

// Fails only as matchScope fails (VIEW headless, a layer the drawing lacks).
// Typed corners name no entity.
[[nodiscard]] katana::core::Result<NamedEntities>
entitiesNamed(const Document& document, const ShapeWords& words, const ScopeViewProvider& views);

// What a scope took that is no shape, by why.
struct SkippedShapes {
    std::size_t open = 0;
    std::size_t tooFew = 0;
    std::size_t noOutline = 0;

    [[nodiscard]] std::size_t total() const { return open + tooFew + noOutline; }
};

// A #id, or the typed corners, that is no shape, which refuses the line.
struct ShapeRefusal {
    // The #id; empty for the typed corners.
    std::optional<katana::entity::EntityId> entity{};
    NotAShape why = NotAShape::NoOutline;
};

struct DrawingShapes {
    // In the order named: the #ids as typed, a scope's ascending.
    std::vector<DrawingShape> shapes{};
    // FROM: what the scope took, and what it took that is no shape. Taking
    // no shape is no refusal here.
    std::optional<ScopeMatch> scope{};
    SkippedShapes skipped{};
    // #ids and typed corners: the first that is no shape, where the reading
    // stopped. A #id must be one, so the verb refuses the line.
    std::optional<ShapeRefusal> refused{};
};

// The shapes `words` name as `kind`: each #id in turn, the typed corners as
// one shape, or what the scope takes. Words that name nothing (empty())
// are typed corners of none: refused, TooFew. A badId is the verb's to
// refuse before this is asked. Fails only as entitiesNamed fails.
[[nodiscard]] katana::core::Result<DrawingShapes>
resolveShapes(const Document& document, const ShapeWords& words, ShapeKind kind,
              const ScopeViewProvider& views);

} // namespace katana::cad
