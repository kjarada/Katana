#pragma once

// One answer to "what does this NAME draw", for the viewport, the plot, and
// every preview and thumbnail - so that none of them can disagree with the
// others about what a style looks like.
//
// A name reaches here from Style::linetype, Layer::linetype or Style::symbol.
// Two tables can hold it: the Model's Linetype table (DXF-style dashes, saved
// in the project) and the session's 12d StyleLibrary (strokes, texts and
// pens; cad::Document::styleLibrary). The rules, as the lead decided them:
//
//   LINETYPE (decision D2). A NON-vertex library definition of the name wins
//   and is drawn by its own strokes, with NO Katana dash pattern applied to
//   them; otherwise a model Linetype gives dashes; otherwise the line is
//   solid. A name held by both tables is a COLLISION: reported, for a
//   diagnostics list, and never an error - a project can meet a library that
//   happens to reuse one of its names.
//
//   SYMBOL. A library definition of the name, of either kind (most symbols
//   the reference mapfiles use are not `mode vertex`); otherwise one of the
//   sixteen built-in shapes, the name's own or the one its words suggest
//   (entity::builtInSymbolFor), and the caller is told which.
//
//   SYMBOLS ON LINES (decision D8). A line whose style names a symbol draws
//   that symbol at EVERY vertex, as 12d does, and a symbol's name is never
//   used as a pattern along the line: a style whose linetype names its own
//   symbol - which is what the 12da import writes for a symbol string - is a
//   plain line with symbols on it.
//
// Nothing here keeps a pointer into a library beyond the call that returned
// it: a LineStyle* dangles as soon as Document::setStyleLibrary replaces the
// library. Anything kept across frames is a FlatDefinition, owned, in a
// DefinitionCache keyed on Document::libraryGeneration().

#include <cstddef>
#include <cstdint>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/dashing.hpp"
#include "katana/cad/style_drawing.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/entity/tables.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

// ---- linetypes ------------------------------------------------------------------

enum class LinetypeKind {
    LibraryDefinition, // drawn by `definition`'s strokes; no dash pattern at all
    ModelLinetype,     // dashed by `linetype` (which may itself be continuous)
    Solid,             // a plain line: nothing holds the name
};

struct ResolvedLinetype {
    LinetypeKind kind = LinetypeKind::Solid;
    // Valid for this call's library only - see the file comment.
    const katana::entity::LineStyle* definition = nullptr; // LibraryDefinition
    const katana::entity::Linetype* linetype = nullptr;    // ModelLinetype
    // The model's Linetype table AND the library both hold the name. The
    // library definition wins when it is not a vertex symbol; the model's
    // linetype wins when it is. Either way a user reading "WATR Main" in two
    // places deserves to be told.
    bool collision = false;
};

[[nodiscard]] ResolvedLinetype resolveLinetype(const katana::entity::Model& model,
                                               const katana::entity::StyleLibrary& library,
                                               std::string_view name);

// The pattern a LINE is drawn with, given its resolved linetype and symbol
// names (D8): resolveLinetype, except that a linetype naming the line's own
// symbol draws a plain line.
[[nodiscard]] ResolvedLinetype resolveLinePattern(const katana::entity::Model& model,
                                                  const katana::entity::StyleLibrary& library,
                                                  std::string_view linetype,
                                                  std::string_view symbol);

// ---- symbols --------------------------------------------------------------------

enum class SymbolKind {
    None,              // kNoSymbol: the viewport's plain point mark
    LibraryDefinition, // `definition`
    BuiltIn,           // the name IS one of the sixteen built-in shapes
    BuiltInFallback,   // nothing defines the name; `builtIn` is the guess drawn
};

struct ResolvedSymbol {
    SymbolKind kind = SymbolKind::None;
    const katana::entity::LineStyle* definition = nullptr; // LibraryDefinition
    // The built-in shape drawn, one of entity::symbolNames(), for BuiltIn and
    // BuiltInFallback; empty otherwise.
    std::string_view builtIn{};
};

// The library first, so a library that defines "circle" is drawn as the
// library says.
[[nodiscard]] ResolvedSymbol resolveSymbol(const katana::entity::StyleLibrary& library,
                                           std::string_view name);

// Flattened library definitions, kept from one frame to the next.
//
// Keyed by (name, library generation): the first question asked at a new
// generation empties the cache, so nothing flattened from a replaced library
// is ever returned. What it hands out is a shared, owned FlatDefinition, never
// a pointer into the library, so an answer stays valid after the cache or the
// library moves on. A name the library lacks is remembered too, as an empty
// answer, since the built-in fallback asks for it on every point.
//
// Not thread-safe: one cache per painter, on the thread that paints.
class DefinitionCache {
  public:
    [[nodiscard]] std::shared_ptr<const FlatDefinition>
    find(const katana::entity::StyleLibrary& library, std::uint64_t generation,
         std::string_view name);

    // The generation the entries belong to; absent before the first find.
    [[nodiscard]] std::optional<std::uint64_t> generation() const { return generation_; }
    [[nodiscard]] std::size_t size() const { return entries_.size(); }
    void clear();

  private:
    std::optional<std::uint64_t> generation_{};
    std::map<std::string, std::shared_ptr<const FlatDefinition>, std::less<>> entries_{};
};

// The strokes a point symbol draws, whichever table answers - library first,
// then the built-in shape's strokes wrapped as StyleStrokes with the entity's
// own pen - so a viewport and a preview cannot disagree about a symbol.
//
// `size` is Style::symbolSize, a WIDTH in model units; 0 means the library
// definition's own scale, or `fallbackHalfWidth` for a built-in shape (the
// caller's plain-mark size, in model units). Nothing is drawn for kNoSymbol.
[[nodiscard]] StyleDrawing pointSymbolDrawing(const katana::entity::StyleLibrary& library,
                                              std::string_view name,
                                              const katana::geometry::Point2& at, double size,
                                              double rotation, double paperScale,
                                              double fallbackHalfWidth);
// The same, with the library definition flattened once through `cache`.
[[nodiscard]] StyleDrawing pointSymbolDrawing(DefinitionCache& cache,
                                              const katana::entity::StyleLibrary& library,
                                              std::uint64_t generation, std::string_view name,
                                              const katana::geometry::Point2& at, double size,
                                              double rotation, double paperScale,
                                              double fallbackHalfWidth);

// ---- level of detail and extent --------------------------------------------------

// A symbol whose drawn extent is under this many pixels is drawn as a dot: at
// that size its strokes are one smudge anyway, and a map of ten thousand coded
// points zoomed out would otherwise lay out every stroke of every one.
inline constexpr double kMinimumSymbolPixels = 3.0;

// The box a drawing covers, each text counted by its height times its length
// about its anchor - an over-estimate, since a text's real extent needs a font,
// and the right way to be wrong for culling: a label reaching into the view
// from an anchor outside it is drawn.
[[nodiscard]] katana::geometry::Box2 drawnExtent(const StyleDrawing& drawing);

// True when `drawing` is smaller than `minimumPixels` across at `viewScale`
// pixels per model unit, so the caller draws a dot instead. An empty drawing
// is not "small": there is nothing to replace.
[[nodiscard]] bool belowSymbolDetail(const StyleDrawing& drawing, double viewScale,
                                     double minimumPixels = kMinimumSymbolPixels);

// Where a vertex symbol goes on an entity's plan shape (D8): every vertex of a
// polyline, both ends of a segment or an arc, a point's own position. A circle
// has no vertex and a text or a dimension is not a line, so they get none.
[[nodiscard]] std::vector<katana::geometry::Point2>
symbolVertices(const katana::entity::Geometry& geometry);

// ---- previews -------------------------------------------------------------------

// The line a style is previewed along: a straight run, a sharp corner and a
// half-circle arc, so a pattern is seen going straight, turning a corner and
// bending round a curve. It fits the box from (0, 0) to (width, width / 4).
// `vertices` are where a vertex symbol goes: the run's two ends, the corner
// and the arc's end - not the chords the arc is drawn with, since a string's
// vertices are the points it was surveyed at, not a curve's flattening.
struct StyleSamplePath {
    katana::geometry::Polyline2 path{};
    std::vector<katana::geometry::Point2> vertices{};
};
[[nodiscard]] StyleSamplePath styleSamplePath(double width);

// What `style` draws along `sample`: a library linestyle REPLACES the line, a
// model linetype gives its dash spans (a dot as a one-point stroke), anything
// else is the solid line; and a symbol sits at every vertex (D8). A style
// whose linetype is "ByLayer" has no layer here, so its sample is a plain
// line. `fallbackHalfWidth` is the built-in symbol's half-width for a style
// with no symbol size; 0 derives it from `dashOptions.viewScale` as the
// viewport's four-pixel plain mark.
[[nodiscard]] StyleDrawing styleSampleDrawing(const katana::entity::Model& model,
                                              const katana::entity::StyleLibrary& library,
                                              const katana::entity::Style& style,
                                              const StyleSamplePath& sample, double paperScale,
                                              const DashOptions& dashOptions,
                                              double fallbackHalfWidth = 0.0);

} // namespace katana::cad
