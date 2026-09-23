#pragma once

// A library of 12d linestyle and symbol definitions (PLAN.MD 20.3, slice 1).
//
// In 12d Model a symbol IS a linestyle - the manual says so where it explains
// vertex symbols: "There can be the same symbol (defined as a linestyle) for
// every vertex" (12da documentation, super string, 1.5.10). One grammar
// describes both, and the only difference is where the strokes go: `mode
// vertex` puts them at each vertex of a string, anything else runs them along
// it. So there is one type here for both, and `atVertices` is what separates
// them.
//
// The definitions live in the entity layer rather than in the layer that
// reads the file, because three layers need them and they cannot all see each
// other: `commands` applies a survey code, `cad` turns a definition into
// strokes to draw, `archive12d` reads the file. It is also where the names
// they are referenced BY already live - `Style::linetype` and `Style::symbol`
// are both names resolved against a library.
//
// A library is NOT part of a Model and is not saved inside a project, which
// is how 12d works: the libraries are a site-wide customisation shared by
// every project, named by a project rather than copied into it. The
// alternative - a table in the Model beside Layer and Linetype - was rejected
// because one production customisation alone is 792 definitions and 35,000
// strokes, which would be copied into every project file that used one of
// them, and because two projects would then be able to disagree about what
// "WATR Main" looks like.

#include <array>
#include <cstddef>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/named_table.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::entity {

// How a definition's coordinates are measured.
enum class StyleUnits {
    // `worldstyle`: model units. The mark keeps its size on the GROUND, so it
    // grows on the page as you zoom in - a 6 m road marking is 6 m wide.
    World,
    // `paperstyle`: millimetres on the plot. The mark keeps its size on the
    // PAGE whatever the scale - a 1.5 mm tick prints 1.5 mm wide.
    Paper,
    // `twoptstyle`: the strokes are stretched between two anchors, so the
    // definition fits the span it is drawn across - a gate, a doorway.
    TwoPoint,
};

[[nodiscard]] const char* toString(StyleUnits units);
// nullopt rather than a default for a word this does not know, so that a file
// using a fourth kind is reported instead of silently drawn as a worldstyle.
[[nodiscard]] std::optional<StyleUnits> parseStyleUnits(std::string_view text);

enum class StrokeOp {
    Move,   // pen up to the point
    Draw,   // pen down to the point
    Arc,    // radius about the current point, from startAngle to endAngle
    Circle, // radius about the current point
    Dot,    // a dot at the current point; radius 0 means the smallest drawable
    Text,   // characters sitting at the current point
    Pen,    // not a mark: the colour everything after it is drawn in
};

// What a `text` command carries beyond the point it sits at. Kept beside the
// strokes rather than in them because a library is overwhelmingly move and
// draw - 17,317 draws against 514 texts in the two reference library files -
// and two std::strings on every one of those would cost far more than this
// indirection does.
struct StrokeText {
    std::string text{};
    double angle = 0.0;  // degrees counter-clockwise
    double height = 0.0; // in the definition's own units
    // 12d's own spelling, "middle-centre" or "top-left". Kept verbatim: the
    // renderer maps it, and a spelling this does not know must still be
    // written back to the file as it was read.
    std::string justify{};
    std::string font{};
    double widthFactor = 1.0;
    // The three numbers a `text` command ends with. All 907 uses across the
    // two libraries have 0 in the first; the other two take values like -0.3
    // and 0.035, and no documentation available here names them. They are
    // KEPT so that what was read can be written back, and deliberately NOT
    // interpreted - guessing that they are an offset would move the text.
    std::array<double, 3> unnamed{};

    friend bool operator==(const StrokeText&, const StrokeText&) = default;
};

struct Stroke {
    static constexpr std::size_t kNoText = static_cast<std::size_t>(-1);

    StrokeOp op = StrokeOp::Move;
    katana::geometry::Point2 point{}; // Move, Draw
    double radius = 0.0;              // Arc, Circle, Dot
    double startAngle = 0.0;          // Arc, degrees
    double endAngle = 0.0;            // Arc, degrees
    std::size_t text = kNoText;       // Text: an index into LineStyle::texts
    std::string pen{}; // Pen: a 12d colour name; "view_colour" is the entity's own

    friend bool operator==(const Stroke&, const Stroke&) = default;
};

struct LineStyle {
    std::string name{};
    // A `/`-separated path, "Survey/WATR" - the tree a library browser shows,
    // the same shape as a layer path and for the same reason.
    std::string group{};
    StyleUnits units = StyleUnits::World;
    // `mode vertex`: drawn at each vertex of a string rather than along it,
    // which is what makes a definition a SYMBOL rather than a linestyle.
    bool atVertices = false;
    // The period of the pattern along a line, in the definition's units. 0
    // means the file said nothing, and the span of the strokes themselves is
    // used instead - see cad::linestyleStrokes.
    double length = 0.0;
    double factor = 1.0; // a scale applied to every coordinate
    katana::geometry::Point2 origin{};
    // TwoPoint only: where the two anchors sit in the definition's own
    // coordinates, so the strokes can be mapped onto the two points the style
    // is drawn between.
    katana::geometry::Point2 anchor1{};
    katana::geometry::Point2 anchor2{};
    // TwoPoint only, and 12d's own numbers kept as read: the files use 1 and
    // 2, and no documentation available here says what they mean, so they are
    // preserved rather than acted on.
    int stretchMode = 0;
    int cycleMode = 0;
    std::vector<Stroke> strokes{};
    std::vector<StrokeText> texts{};
    // The file this definition was read from - its NAME only, never a path, so
    // a library carries no trace of where on someone's disk it came from.
    // Empty for a definition made in code or in a session. A browser groups
    // and filters by it, and it is part of what says a definition is a symbol:
    // most symbols the reference mapfiles use are not `mode vertex`, but they
    // all come from the symbol file (docs/survey_coding.md).
    std::string source{};

    // The box the Move and Draw strokes cover, with `factor` applied. Text,
    // arcs and circles are included by their point; a text's extent needs a
    // font and is not known here. Empty for a definition with no strokes.
    [[nodiscard]] katana::geometry::Box2 bounds() const;

    friend bool operator==(const LineStyle&, const LineStyle&) = default;
};

// Fails with InvalidArgument, naming the rule broken.
[[nodiscard]] katana::core::Status validate(const LineStyle& style);

struct LineStylePolicy : NamedTablePolicy<LineStyle> {
    static constexpr std::string_view kNoun = "linestyle";
    static katana::core::Status validate(const LineStyle& style);
};

// The same container as every other table of named things. Names compare with
// regard to CASE, as they do in the rest of the model, although 12d's own
// comparisons are case-insensitive ("Bypass" and "BYPASS" are one linestyle
// there). That difference is deliberate and measured rather than overlooked:
// of the 372 references the reference mapfile makes into its two libraries,
// every single one matches exactly and none needs case folding. A
// name that does not resolve is reported by the caller rather than guessed
// at, so if another customisation ever does spell one differently it will say
// so instead of drawing the wrong mark.
using StyleLibrary = NamedTable<LineStyle, LineStylePolicy>;

// Later wins: a customisation is loaded in layers, and four of the Transport
// for NSW definitions ("BUIL Doorway" among them) appear in both files, so
// "already exists" is the normal case rather than an error. Returns whether a
// definition of that name was replaced.
[[nodiscard]] katana::core::Result<bool> addOrReplace(StyleLibrary& library, LineStyle style);

// Every distinct group, in name order, without the empty one.
[[nodiscard]] std::vector<std::string> styleGroups(const StyleLibrary& library);
// The definitions drawn at vertices - the symbols - in name order. Named for
// what it returns rather than `symbolNames`, which already means the sixteen
// shapes Katana draws without a library.
[[nodiscard]] std::vector<std::string> vertexStyleNames(const StyleLibrary& library);

} // namespace katana::entity
