#pragma once

// Named resource tables that entities refer to by name (PLAN.MD Phase 05):
// LayerDatabase, StyleDatabase, PropertyDatabase.
//
// Tables are ordered by name for deterministic iteration and are not
// thread-safe. Cross-table rules (e.g. "a layer in use cannot be removed") are
// enforced by commands, which can see every table.

#include <map>
#include <utility>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/named_table.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/entity/entity.hpp"

namespace katana::entity {


inline constexpr std::string_view kDefaultLayerName = "0";
inline constexpr std::string_view kContinuousLinetype = "continuous";

// The built-in hatch pattern that draws no fill. Named rather than represented
// by an empty string so that "this layer is not hatched" and "this layer has
// not said" stay distinguishable, exactly as "continuous" does for linetypes.
inline constexpr std::string_view kNoHatch = "none";

struct Layer {
    std::string name{kDefaultLayerName};
    Color color{};
    bool visible = true;
    bool locked = false; // locked layers are drawn but their entities cannot be edited
    std::string linetype{kContinuousLinetype};
    double lineWeight = 0.25; // millimetres on paper
    // Empty means the document default ("Standard"). Appended last, as every
    // field here is, so the storage columns keep their order.
    std::string dimensionStyle{};
    std::string hatchPattern{kNoHatch};

    friend bool operator==(const Layer&, const Layer&) = default;
};

// Always contains layer "0", which can be modified but never removed or renamed.
//
// Layer names are PATHS: "design/surface/tin1". See layer_path.hpp for why the
// tree is derived from the names rather than stored. Two consequences shape
// this interface:
//
//   * Adding "design/surface/tin1" also creates "design" and "design/surface"
//     if they are missing, so every node in the tree is a real layer with its
//     own colour, visibility and lock rather than a placeholder the user
//     cannot select or set.
//   * Visibility and lock INHERIT down the tree (see effectivelyVisible and
//     effectivelyLocked): turning off "design" turns off everything under it,
//     which is the entire point of grouping. Colour and linetype do not
//     inherit - ByLayer already resolves those, and a second inheritance rule
//     layered on top would make the resolved colour impossible to predict.
//
// Because the map is keyed by full path, its ascending order IS a pre-order
// walk of the tree, and a subtree is a contiguous range rather than a search.
class LayerDatabase {
  public:
    LayerDatabase();

    // Creates `layer` and any missing ancestors (with default properties).
    // Fails with AlreadyExists when the layer itself is already present, and
    // with InvalidArgument when the name is not a valid path.
    [[nodiscard]] katana::core::Status add(Layer layer);
    // Same, but succeeds and changes nothing when the layer already exists.
    // What an importer wants: "make sure this path exists".
    [[nodiscard]] katana::core::Status ensure(std::string_view name);
    [[nodiscard]] katana::core::Status update(const Layer& layer); // matched by name
    // Removes ONE layer. Fails with InvalidArgument when it still has
    // children: silently orphaning or deleting them is the kind of quiet
    // destruction section 36 forbids. Use removeSubtree to take the branch.
    [[nodiscard]] katana::core::Result<Layer> remove(std::string_view name);
    // Removes `name` and everything under it, deepest first, and returns what
    // was removed in that order so an undo can put it back parents-first by
    // walking the result backwards.
    [[nodiscard]] katana::core::Result<std::vector<Layer>> removeSubtree(std::string_view name);
    // Restores the pristine state: only the default layer.
    void reset();

    // Renames `from` and every descendant to sit under `to`. Returns the
    // (old, new) pairs so the caller can move the entities that reference
    // them; nothing is changed if any target name is already taken, so a
    // collision cannot leave the tree half renamed.
    //
    // Fails with NotFound, InvalidArgument (bad path, renaming layer "0", or
    // moving a layer inside its own subtree) or AlreadyExists.
    [[nodiscard]] katana::core::Result<std::vector<std::pair<std::string, std::string>>>
    renameSubtree(std::string_view from, std::string_view to);

    [[nodiscard]] const Layer* find(std::string_view name) const;
    [[nodiscard]] bool contains(std::string_view name) const { return find(name) != nullptr; }
    [[nodiscard]] std::vector<std::string> names() const; // ascending
    [[nodiscard]] std::vector<Layer> all() const;          // ascending by name
    [[nodiscard]] std::size_t size() const { return layers_.size(); }

    // ---- the tree ---------------------------------------------------------

    // Top-level layers, ascending.
    [[nodiscard]] std::vector<std::string> roots() const;
    // Immediate children of `name`, ascending. Empty for a leaf or a missing
    // layer. Pass an empty name for the roots.
    [[nodiscard]] std::vector<std::string> children(std::string_view name) const;
    // `name` and everything beneath it, in pre-order. Empty when absent.
    [[nodiscard]] std::vector<std::string> subtree(std::string_view name) const;
    [[nodiscard]] bool hasChildren(std::string_view name) const;

    // Visible only when it and every ancestor is visible; locked when it or
    // any ancestor is locked. False / true respectively for a missing layer,
    // so an entity on a layer that has gone is not silently drawn or edited.
    [[nodiscard]] bool effectivelyVisible(std::string_view name) const;
    [[nodiscard]] bool effectivelyLocked(std::string_view name) const;

  private:
    std::map<std::string, Layer, std::less<>> layers_;
};

// ---- dimension styles -----------------------------------------------------------

inline constexpr std::string_view kDefaultDimensionStyleName = "Standard";

// How the arrow at each end of a dimension line is drawn.
enum class ArrowHead { None, Tick, ClosedFilled, Open, Dot };

[[nodiscard]] std::string_view toString(ArrowHead head);
[[nodiscard]] katana::core::Result<ArrowHead> arrowHeadFromString(std::string_view name);

// Everything that decides how a dimension is drawn and what its text says.
//
// Lengths here are MODEL units, unlike Layer::lineWeight which is millimetres
// on paper. A dimension is part of the drawing: its text has to stay the same
// size relative to the geometry it annotates when the view is zoomed, or the
// plotted sheet shows something different from the screen.
struct DimensionStyle {
    std::string name{kDefaultDimensionStyleName};

    // Geometry, in model units.
    double textHeight = 2.5;        // DIMTXT
    double textGap = 0.625;         // DIMGAP: dimension line to the text
    double extensionOffset = 0.625; // DIMEXO: gap between the measured point and
                                    // the start of its extension line
    double extensionBeyond = 1.25;  // DIMEXE: how far the extension line runs past
                                    // the dimension line
    double arrowSize = 2.5;         // DIMASZ
    ArrowHead arrowHead = ArrowHead::ClosedFilled;

    // Text.
    //
    // `unitScale` multiplies the measurement before it is shown (DIMLFAC), so a
    // drawing in metres can be dimensioned in millimetres with 1000 and a
    // "mm" suffix. It is a factor and a suffix rather than a unit enum because
    // the entity layer cannot see katana/geodesy, and duplicating a unit table
    // here would be a second definition of a foot.
    double unitScale = 1.0;
    std::string prefix{};
    std::string suffix{};
    int decimals = 3;               // DIMDEC
    // Rounds the scaled measurement to a multiple of this before formatting
    // (DIMRND). 0 disables it. Must be 0 or at least tolerance::kGeometric:
    // rounding finer than the geometric tolerance is rounding to noise.
    double roundTo = 0.0;
    bool suppressTrailingZeros = false; // DIMZIN

    friend bool operator==(const DimensionStyle&, const DimensionStyle&) = default;
};

// Fails with InvalidArgument, naming the specific rule broken.
[[nodiscard]] katana::core::Status validate(const DimensionStyle& style);

// Always contains "Standard", which can be neither removed nor renamed, exactly
// as layer "0" and the continuous linetype cannot.
struct DimensionStylePolicy : NamedTablePolicy<DimensionStyle> {
    static constexpr std::string_view kNoun = "dimension style";
    static katana::core::Status validate(const DimensionStyle& style);
    static bool isProtected(std::string_view name) { return name == kDefaultDimensionStyleName; }
    static void seed(NamedMap<DimensionStyle>& items);
};

using DimensionStyleDatabase = NamedTable<DimensionStyle, DimensionStylePolicy>;

// ---- linetypes ------------------------------------------------------------------
//
// A dash pattern, in MODEL units. The sign convention is DXF's (AutoCAD DXF
// Reference, LTYPE table entry, group code 49): a positive length is a dash
// (pen down), a negative length is a gap (pen up), and exactly zero is a dot.
//
// Lengths are model lengths, so a 0.5 m dash stays half a metre of ground at
// every zoom - which is the entire point of a linetype on a survey drawing,
// and the opposite of what a screen-space dash pattern does. A dot is the one
// exception: it has no length, so it draws at a fixed pixel size at any zoom,
// which matches CAD practice.
struct LinetypeElement {
    double length = 0.0;

    [[nodiscard]] constexpr bool isDash() const { return length > 0.0; }
    [[nodiscard]] constexpr bool isGap() const { return length < 0.0; }
    [[nodiscard]] constexpr bool isDot() const { return length == 0.0; }
    // Pen down covers both a dash and a dot.
    [[nodiscard]] constexpr bool isPenDown() const { return length >= 0.0; }

    friend constexpr bool operator==(const LinetypeElement&, const LinetypeElement&) = default;
};

struct Linetype {
    std::string name{};
    std::string description{}; // DXF group 3, e.g. "Dashed  __  __  __"
    // Empty means continuous. Otherwise the elements must strictly alternate
    // pen-down and pen-up, start with a dash or a dot, and end with a gap -
    // see validate() for why each of those is required rather than tidy.
    std::vector<LinetypeElement> pattern{};

    // The period, in model units: the sum of the ABSOLUTE element lengths
    // (DXF group 40). Computed rather than stored, so it cannot disagree with
    // the elements it is derived from.
    [[nodiscard]] double patternLength() const;
    [[nodiscard]] bool isContinuous() const { return pattern.empty(); }
    // Shortest non-dot element. Used to decide when a pattern is too fine to
    // resolve on screen and should be drawn solid instead.
    [[nodiscard]] double shortestElement() const;

    friend bool operator==(const Linetype&, const Linetype&) = default;
};

// Fails with InvalidArgument, naming the specific rule broken.
[[nodiscard]] katana::core::Status validate(const Linetype& linetype);

// Always contains "continuous" (an empty pattern), which can be neither removed
// nor renamed, exactly as layer "0" cannot.
struct LinetypePolicy : NamedTablePolicy<Linetype> {
    static constexpr std::string_view kNoun = "linetype";
    static katana::core::Status validate(const Linetype& linetype);
    // The continuous linetype exists so that every entity has something to
    // resolve to. Giving it a pattern would change what an unset ByLayer chain
    // draws as across the whole document, so it is refused on update as well
    // as on remove.
    static katana::core::Status checkUpdate(const Linetype& linetype);
    static bool isProtected(std::string_view name) { return name == kContinuousLinetype; }
    static void seed(NamedMap<Linetype>& items);
};

using LinetypeDatabase = NamedTable<Linetype, LinetypePolicy>;

// The named symbols a point may be drawn with. A fixed set, because a symbol
// is a shape a viewport and a plotter both know how to paint at any size;
// the names are the ones survey and civil packages use for the same marks.
// kNoSymbol draws the plain point mark of the viewport.
inline constexpr std::string_view kNoSymbol = "";
[[nodiscard]] const std::vector<std::string_view>& symbolNames();
[[nodiscard]] bool isSymbolName(std::string_view name);

struct Style {
    std::string name{};
    std::optional<Color> color{}; // empty: ByLayer
    double lineWeight = 0.25;     // millimetres on paper
    std::string linetype{kContinuousLinetype};
    // Empty means ByLayer. Unlike the layer field this is NOT defaulted to
    // "none": a style that says nothing about hatching must not override a
    // layer that does.
    std::string hatchPattern{};
    // What the style is for, or where it came from: a 12d linestyle arrives
    // with its 12d colour name here, so a colour Katana has no RGB for is
    // still known by name. Appended last, as every field is, so the storage
    // columns keep their order.
    std::string description{};
    // How a POINT in this style is drawn: one of symbolNames(), or kNoSymbol
    // for the viewport's plain mark. A 12d vertex symbol is a linestyle drawn
    // at a vertex, which is why the symbol lives on the style and not on the
    // point: "the style of a point" and "the style of a line" are one thing.
    std::string symbol{kNoSymbol};
    // The symbol's width in model units - a 12d symbol `size` - or 0 for the
    // viewport's default mark size.
    double symbolSize = 0.0;

    friend bool operator==(const Style&, const Style&) = default;
};

// Fails with InvalidArgument for a bad name, a non-finite or negative line
// weight or symbol size, or a symbol that is not one of symbolNames().
[[nodiscard]] katana::core::Status validate(const Style& style);

// ---- hatch patterns -------------------------------------------------------------
//
// One family of parallel lines. A pattern is a list of these, which is how DXF
// PAT files describe hatching and why a crosshatch is two families rather than
// a special kind of one.
//
// Angles are radians counter-clockwise from +x; spacing and offset are MODEL
// units, so a hatch keeps its size on the ground at every zoom exactly as a
// linetype pattern does. The offset is measured from the world origin, which is
// what makes two adjacent parcels hatched alike line up along their shared
// edge - see geometry::hatchLines.
struct HatchLineFamily {
    double angle = 0.0;
    double spacing = 1.0;
    double offset = 0.0;

    friend bool operator==(const HatchLineFamily&, const HatchLineFamily&) = default;
};

struct HatchPattern {
    std::string name{};
    std::string description{};
    // A filled area rather than a family of lines. DXF SOLID hatches import as
    // this; it is not expressible as a spacing, however small.
    bool solid = false;
    std::vector<HatchLineFamily> families{};

    // The built-in "none" is the only pattern allowed to draw nothing.
    [[nodiscard]] bool drawsNothing() const { return !solid && families.empty(); }

    friend bool operator==(const HatchPattern&, const HatchPattern&) = default;
};

// Fails with InvalidArgument, naming the specific rule broken.
[[nodiscard]] katana::core::Status validate(const HatchPattern& pattern);

struct HatchPatternPolicy : NamedTablePolicy<HatchPattern> {
    static constexpr std::string_view kNoun = "hatch pattern";
    static katana::core::Status validate(const HatchPattern& pattern);
    // "none" is what an unhatched layer resolves to, so giving it a fill would
    // hatch every layer that has never asked to be hatched.
    static katana::core::Status checkUpdate(const HatchPattern& pattern);
    static bool isProtected(std::string_view name) { return name == kNoHatch; }
    static void seed(NamedMap<HatchPattern>& items);
};

using HatchPatternDatabase = NamedTable<HatchPattern, HatchPatternPolicy>;

// ---- alignments -----------------------------------------------------------------
//
// A named horizontal alignment: the plan centreline a section is cut along, a
// profile is drawn against and a corridor is built around. It is a NAMED
// TABLE in the model and not an eighth Geometry alternative, for the reasons
// PLAN.MD Phase 21 sets out: it is a definition other things reference by
// name, exactly as a surface is, and the section code that already exists is
// on the silent-failure list for a new geometry kind. What is stored is the
// PI definition; the elements are derived on demand by
// geometry::solveAlignment, so a stored alignment cannot be discontinuous.
struct Alignment {
    std::string name{};
    std::string description{};
    katana::geometry::HorizontalAlignment horizontal{};
    // The design grade line along it, when one has been designed. Optional
    // rather than empty, because an alignment traced only to cut a section
    // has no design and should not pretend to an empty one. One profile per
    // alignment: a second (a kerb line, say) is a later need, and a separate
    // table when it comes.
    std::optional<katana::geometry::VerticalAlignment> vertical{};

    friend bool operator==(const Alignment&, const Alignment&) = default;
};

// Fails with InvalidArgument for a bad name, and with whatever
// geometry::solveAlignment or geometry::solveProfile reports for a definition
// that cannot be built - so an alignment that would fail to draw is refused on
// the way in, naming the PI or PVI, rather than stored and found out about
// later.
[[nodiscard]] katana::core::Status validate(const Alignment& alignment);

struct AlignmentPolicy : NamedTablePolicy<Alignment> {
    static constexpr std::string_view kNoun = "alignment";
    static katana::core::Status validate(const Alignment& alignment);
};

using AlignmentDatabase = NamedTable<Alignment, AlignmentPolicy>;

struct StylePolicy : NamedTablePolicy<Style> {
    static constexpr std::string_view kNoun = "style";
    static katana::core::Status validate(const Style& style);
};

using StyleDatabase = NamedTable<Style, StylePolicy>;

enum class PropertyType { Boolean, Integer, Real, Text };

[[nodiscard]] constexpr PropertyType typeOf(const PropertyValue& value)
{
    return static_cast<PropertyType>(value.index());
}

// Schema entry for an entity property. Defining a property makes it typed:
// values of the wrong type are rejected before they reach an entity.
struct PropertyDefinition {
    std::string name{};
    PropertyType type = PropertyType::Text;
    std::string description{};
    std::optional<PropertyValue> defaultValue{};

    friend bool operator==(const PropertyDefinition&, const PropertyDefinition&) = default;
};

// Registry of property definitions. Properties without a definition are
// free-form and always accepted.
class PropertyDatabase {
  public:
    [[nodiscard]] katana::core::Status define(PropertyDefinition definition);
    [[nodiscard]] katana::core::Result<PropertyDefinition> undefine(std::string_view name);
    void reset() { definitions_.clear(); }

    [[nodiscard]] const PropertyDefinition* find(std::string_view name) const;
    [[nodiscard]] std::vector<PropertyDefinition> all() const; // ascending by name

    // Ok when `name` is undefined, or defined with the type of `value`.
    [[nodiscard]] katana::core::Status validate(std::string_view name,
                                                const PropertyValue& value) const;

  private:
    std::map<std::string, PropertyDefinition, std::less<>> definitions_;
};

} // namespace katana::entity
