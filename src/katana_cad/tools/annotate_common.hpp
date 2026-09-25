#pragma once

// The Annotate family's shared parts (annotate*.cpp): the factories that
// annotate.cpp lists in the catalogue, and the few rules every annotation tool
// applies the same way - what a typed option or number is, how wide text is
// taken to be, how far apart its lines are, and which dimension style a new
// leader or dimension takes. One place, so Text and Leader cannot space their
// lines differently and two tools cannot disagree about what "U" means.

#include <cstddef>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/interactive_tool.hpp"
#include "katana/commands/command.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/tables.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad::tools::annotate {

[[nodiscard]] std::unique_ptr<InteractiveTool> makeTextTool(const ToolContext& context);
// annotate_text.cpp too: the Text tool's options and prompts, its lines made
// ONE entity.
[[nodiscard]] std::unique_ptr<InteractiveTool> makeMultilineTextTool(const ToolContext& context);
[[nodiscard]] std::unique_ptr<InteractiveTool> makeAlignedDimensionTool(const ToolContext& context);
[[nodiscard]] std::unique_ptr<InteractiveTool> makeLinearDimensionTool(const ToolContext& context);
[[nodiscard]] std::unique_ptr<InteractiveTool> makeLeaderTool(const ToolContext& context);
// annotate_dimension_kinds.cpp: the kinds the DIM verb makes beside aligned
// and linear.
[[nodiscard]] std::unique_ptr<InteractiveTool> makeAngularDimensionTool(const ToolContext& context);
[[nodiscard]] std::unique_ptr<InteractiveTool> makeRadiusDimensionTool(const ToolContext& context);
[[nodiscard]] std::unique_ptr<InteractiveTool> makeDiameterDimensionTool(const ToolContext& context);
[[nodiscard]] std::unique_ptr<InteractiveTool> makeOrdinateDimensionTool(const ToolContext& context);
// annotate_label.cpp: Label Objects, the LABEL verb's labels placed by hand.
[[nodiscard]] std::unique_ptr<InteractiveTool> makeLabelTool(const ToolContext& context);
// annotate_dimension_chain.cpp: DIM BASELINE and DIM CONTINUE by hand.
[[nodiscard]] std::unique_ptr<InteractiveTool> makeBaselineDimensionTool(const ToolContext& context);
[[nodiscard]] std::unique_ptr<InteractiveTool> makeContinueDimensionTool(const ToolContext& context);
// annotate_balloon.cpp: BALLOON by hand.
[[nodiscard]] std::unique_ptr<InteractiveTool> makeBalloonTool(const ToolContext& context);

// True when `typed` names the option `keyword`: the keyword or any prefix of it
// at least `shortest` letters long, in any case - "u", "UN" and "Undo" all name
// Undo - which is how a CAD command line accepts an option's capital letters.
[[nodiscard]] bool isOption(std::string_view typed, std::string_view keyword,
                            std::size_t shortest = 1);

// A typed height, angle, offset or distance. Through core/text.hpp, so "2.5"
// means two and a half on every machine and "2,5" is not a number.
[[nodiscard]] std::optional<double> typedNumber(std::string_view typed);

// Two points closer than the geometric tolerance are one point: a height, a
// direction or a length measured between them is rounding noise, not input.
[[nodiscard]] bool coincident(const katana::geometry::Point2& a,
                              const katana::geometry::Point2& b);

// A new entity with the current layer, style and colour - what every tool
// here creates.
[[nodiscard]] katana::entity::Entity newEntity(katana::entity::Geometry geometry,
                                               const katana::commands::EntityAttributes& attributes);

// ONE command adding every entity in `entities`, shown as `name` in the
// history, so one undo removes a whole multi-line text or a leader with its
// arrow and its text.
[[nodiscard]] katana::commands::CommandPtr createAll(std::string name,
                                                     std::vector<katana::entity::Entity> entities);

// The dimension style an annotation made now on the current layer is drawn
// with: the layer's named style, else the document's default - the rule
// resolveDimensionStyle applies when the dimension is drawn, so a leader's
// arrow and text are sized as a dimension beside it is.
[[nodiscard]] katana::entity::DimensionStyle styleForNewAnnotation(const ToolContext& context);

// The width `text` is taken to occupy at `height`. The model has no font
// metrics, so this is the 0.6-of-the-height advance per character that
// entity::boundingBox and the dimension label's centring already use, counted
// per character (UTF-8 code point) rather than per byte, so "é" is one
// character wide and not two.
[[nodiscard]] double estimatedTextWidth(std::string_view text, double height);

// Baseline to baseline between the lines of a multi-line text or a leader's
// note: 5/3 of the height, the single line spacing of AutoCAD's text (MTEXT
// documents it as about 1.66 times the height). Computed as height * 5 / 3 so
// that a height of 3 gives exactly 5.
[[nodiscard]] double lineSpacing(double height);

// A number as a prompt shows a default: "2.5", "90". Rounded to nine decimals
// first, so an angle stored in radians reads back as 90 and not
// 90.00000000000001.
[[nodiscard]] std::string formatNumber(double value);

} // namespace katana::cad::tools::annotate
