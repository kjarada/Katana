#pragma once

// The annotation tables: text styles, label styles and the auto-label rules
// (docs/annotation.md).
//
// They are named tables like the dimension styles beside them (tables.hpp,
// NamedTable), kept in the Model, changed only by the table commands of
// katana_commands (table_commands.cpp) and stored with the project (schema
// 11, project_store.cpp). What the three have in common is that every size
// in them is PAPER millimetres: an annotation is written for the sheet, and
// is drawn in the model at paperMm x scale / 1000 for the scale of the view
// or sheet viewport showing it (annotationModelSize), so a 2.5 mm label is
// 2.5 mm on a 1:200 sheet and on a 1:2000 one.

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/named_table.hpp"

namespace katana::entity {

// ---- the one conversion ------------------------------------------------------------

// Model units for `paperMillimetres` on a sheet at 1 : `scale`: a model unit
// is a metre (ProjectMetadata::linearUnit's default; a drawing in feet is
// not handled yet - docs/annotation.md, "Not yet"), and a millimetre on the
// sheet is `scale` millimetres on the ground.
[[nodiscard]] constexpr double annotationModelSize(double paperMillimetres, double scale)
{
    return paperMillimetres * scale / 1000.0;
}

// The other way: the paper millimetres `modelUnits` are on a sheet at
// 1 : `scale`. What a size given in model units - a model-unit dimension
// style's arrow - becomes when an annotation that is sized on paper takes it.
[[nodiscard]] constexpr double annotationPaperSize(double modelUnits, double scale)
{
    return modelUnits * 1000.0 / scale;
}

// The annotation scale a view uses when nothing has chosen one: 1 : 1000, the
// scale at which a 2.5 mm paper text is 2.5 model units tall - exactly the
// default height a TextGeometry has always had - so paper-sized text made in
// a drawing that never set a scale looks as the text beside it does.
inline constexpr double kDefaultAnnotationScale = 1000.0;

// ---- text styles --------------------------------------------------------------------

inline constexpr std::string_view kDefaultTextStyleName = "Standard";

struct TextStyle {
    std::string name{kDefaultTextStyleName};
    // The face; empty for the application's plain-text face (the painter's
    // PlanPaintOptions::fontFamily).
    std::string fontFamily{};
    // Millimetres on paper. A text set in the style with no paper height of
    // its own is drawn this tall; 0 leaves such a text at its model height,
    // which is how a style can dress model-unit text without resizing it.
    double paperHeight = 2.5;
    // Width of each character relative to the face's own (DXF STYLE group
    // 41); 1 is the face as designed.
    double widthFactor = 1.0;
    // Slant of the verticals, radians, positive leaning right (DXF group 50,
    // there in degrees). Refused at 85 degrees or more, where the text is a
    // line.
    double oblique = 0.0;
    bool bold = false;
    bool italic = false;
    // Empty is ByLayer: the colour the text entity resolves to anyway.
    std::optional<Color> color{};
    // A background mask: the rectangle behind the text is painted in the
    // medium's background colour (white paper, the view's ground) first, so
    // linework does not run through the text. `maskMargin` is how far the
    // mask reaches past the text, millimetres on paper.
    bool mask = false;
    double maskMargin = 0.5;
    // Readable: a text whose rotation would put it upside down - pointing
    // anywhere from just past straight up round to just short of straight
    // down - is turned half a turn and its justification mirrored, so it
    // reads from the bottom or the right of the sheet as drawing practice
    // asks (AS 1100.101 and ISO 3098 both). On by default for the new
    // styles; a text in no style is never turned.
    bool readable = true;
    // Multiplies the standard pitch between baselines (text_block.hpp).
    double lineSpacing = 1.0;

    friend bool operator==(const TextStyle&, const TextStyle&) = default;
};

[[nodiscard]] katana::core::Status validate(const TextStyle& style);

// Always contains "Standard", which can be neither removed nor renamed.
struct TextStylePolicy : NamedTablePolicy<TextStyle> {
    static constexpr std::string_view kNoun = "text style";
    static katana::core::Status validate(const TextStyle& style);
    static bool isProtected(std::string_view name) { return name == kDefaultTextStyleName; }
    static void seed(NamedMap<TextStyle>& items);
};

using TextStyleDatabase = NamedTable<TextStyle, TextStylePolicy>;

// ---- label styles ------------------------------------------------------------------

// What a label style labels, which decides the values its template can use
// (label_text.hpp lists them) and the placements that make sense.
enum class LabelKind : std::uint8_t {
    Point = 0,    // a point: its number, code, level, coordinates; spot levels
    Segment = 1,  // a line, or each segment of a polyline: bearing and distance
    Arc = 2,      // an arc (or a circle): radius, length, chord, delta
    Area = 3,     // a closed polyline or a circle: area and perimeter, at its centroid
    Chainage = 4, // an alignment: chainages at an interval, with ticks
};

[[nodiscard]] std::string_view toString(LabelKind kind);
[[nodiscard]] katana::core::Result<LabelKind> labelKindFromString(std::string_view name);

// Where the placer tries first. Auto is the kind's natural place: above a
// line, outside an arc, at an area's centroid, to the upper right of a point.
enum class LabelPlacement : std::uint8_t {
    Auto = 0,
    Above = 1,    // a line's left side seen along it, an arc's outside, above a point
    Below = 2,    // the other side
    Along = 3,    // centred ON the line, a mask behind it
    Centroid = 4, // an area's centroid, a line's midpoint
    Right = 5,    // of a point
    Left = 6,     // of a point
};

[[nodiscard]] std::string_view toString(LabelPlacement placement);
[[nodiscard]] katana::core::Result<LabelPlacement> labelPlacementFromString(std::string_view name);

// Horizontal on the sheet, or turned to the line it labels (and kept
// readable).
enum class LabelOrientation : std::uint8_t { Aligned = 0, Horizontal = 1 };

[[nodiscard]] std::string_view toString(LabelOrientation orientation);
[[nodiscard]] katana::core::Result<LabelOrientation>
labelOrientationFromString(std::string_view name);

// A mark at a point label's point: a spot level's cross.
enum class LabelMarker : std::uint8_t { None = 0, Cross = 1, Dot = 2, Circle = 3 };

[[nodiscard]] std::string_view toString(LabelMarker marker);
[[nodiscard]] katana::core::Result<LabelMarker> labelMarkerFromString(std::string_view name);

struct LabelStyle {
    std::string name{};
    LabelKind kind = LabelKind::Point;
    // The template: literal text with {value:format} fields, lines separated
    // by '\n' (label_text.hpp). What each kind's default is: the point's
    // number, the bearing and distance, the radius and length, the area,
    // the chainage.
    std::string text{"{point}"};
    // The text style the label is set in; empty for Standard.
    std::string textStyle{};
    // Millimetres on paper; 0 takes the text style's paper height (2.5 mm
    // when that is 0 too).
    double paperHeight = 0.0;
    LabelPlacement placement = LabelPlacement::Auto;
    LabelOrientation orientation = LabelOrientation::Aligned;
    // The gap between what is labelled and the text, millimetres on paper.
    double offset = 1.0;
    // A leader from the label back to what it labels when the placer (or a
    // drag) moved it away from its first place.
    bool leader = true;
    // The placer may move the label to its other candidate places when the
    // first is taken; off, a label that does not fit where the style puts
    // it is suppressed rather than moved.
    bool displace = true;
    // Placed before styles of lower priority, and so keeps its place when two
    // compete (label_layout.hpp).
    int priority = 0;
    // Empty takes the text style's colour, and that ByLayer.
    std::optional<Color> color{};
    LabelMarker marker = LabelMarker::None;
    double markerSize = 1.0; // millimetres on paper, across
    // Segment labels: a piece shorter than this many millimetres on the
    // sheet is not labelled, as a label would not fit along it anyway.
    double minimumLength = 0.0;
    // Chainage labels: a label every `interval` model units of chainage and a
    // tick every `tickInterval` (0: at the labels only), `tickLength`
    // millimetres either side of the line.
    double interval = 20.0;
    double tickInterval = 10.0;
    double tickLength = 1.5;

    friend bool operator==(const LabelStyle&, const LabelStyle&) = default;
};

[[nodiscard]] katana::core::Status validate(const LabelStyle& style);

// No built-in entry: LABELSTYLE DEFAULTS adds the standard set as one step,
// so a project written before them opens with none and looks as it did.
struct LabelStylePolicy : NamedTablePolicy<LabelStyle> {
    static constexpr std::string_view kNoun = "label style";
    static katana::core::Status validate(const LabelStyle& style);
};

using LabelStyleDatabase = NamedTable<LabelStyle, LabelStylePolicy>;

// ---- auto-label rules ------------------------------------------------------------------

// "Label every X with style S": the rules AUTOLABEL RUN applies
// (cad/annotation/auto_label.hpp). An entity matches when it passes every
// filter that is not empty; the patterns are globs (* any run, ? any one
// character), matched without regard to ASCII case, since a survey code is
// typed in whatever case the field crew used.
struct LabelRule {
    std::string name{};
    std::string labelStyle{};
    // The entity's layer, or any ancestor of it: "survey" matches
    // survey/points/EP, as switching a layer off would.
    std::string layer{};
    // The entity's survey code, found where survey coding finds it
    // (cad::codePropertyCandidates, cad/survey_coding.hpp).
    std::string code{};
    // Point, Line, Arc, Polyline, Circle - an EntityType's name - or empty for
    // every type the style's kind labels.
    std::string entityType{};
    // The layer the labels are put on; empty puts each on its target's.
    std::string labelLayer{};
    bool enabled = true;

    friend bool operator==(const LabelRule&, const LabelRule&) = default;
};

[[nodiscard]] katana::core::Status validate(const LabelRule& rule);

struct LabelRulePolicy : NamedTablePolicy<LabelRule> {
    static constexpr std::string_view kNoun = "label rule";
    static katana::core::Status validate(const LabelRule& rule);
};

using LabelRuleDatabase = NamedTable<LabelRule, LabelRulePolicy>;

// `pattern` (* and ?) matches all of `text`, ASCII case folded. An empty
// pattern matches everything.
[[nodiscard]] bool globMatches(std::string_view pattern, std::string_view text);


} // namespace katana::entity
