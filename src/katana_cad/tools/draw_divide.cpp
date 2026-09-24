// Divide and Measure (see families.hpp): points along an object - N equal
// parts, or one every so far - as AutoCAD's DIVIDE and MEASURE place them,
// for setting-out marks, fence posts, chainage points or light poles.
//
// The points are ordinary point entities on the current layer. [Style] puts
// them in a named style instead, and a style carries a point symbol (Format >
// Styles), which is how a symbol goes along an object here: the symbol is
// the style's, not a block's, so it is drawn and plotted as every styled
// point is. On a 3D string each point gets the height interpolated along it
// (modify_edit::heightAt), so points divided along a surveyed kerb are
// levelled points, and a point where the string has no height gets none -
// absent is not zero.
//
// Where they go, measured along the object from its start (a circle from its
// east point, anticlockwise, as it is drawn):
//   Divide into N: an open object gets N - 1 points, its ends being marked
//   already; a closed one gets N, since each of its N parts ends at a point.
//   Measure every d: at d, 2d, ... short of the far end. An open object is
//   measured from the end nearer the pick, as AutoCAD measures; a closed one
//   from its start.
// All the points are ONE command, so one undo takes them all.

#include <cmath>
#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "everyday_support.hpp"
#include "families.hpp"
#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"
#include "modify_edit_support.hpp"

namespace katana::cad::tools {

namespace {

namespace cmd = katana::commands;
using everyday::fixed;
using everyday::Path;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;
using modify_edit::isOption;
using modify_edit::kindName;
using modify_edit::withArticle;
namespace tolerance = katana::math::tolerance;

// The array command's guard against a runaway typo (entity_commands.cpp),
// kept here for the same reason: a length typed in millimetres into a drawing
// in metres asks for a million points.
constexpr std::int64_t kMostPoints = 100000;

class AlongTool final : public InteractiveTool {
  public:
    enum class Kind { Divide, Measure };

    AlongTool(const ToolContext& context, Kind kind)
        : document_(context.document), attributes_(context.attributes), kind_(kind)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::Object:
            return kind_ == Kind::Divide
                       ? "Select the object to divide"
                       : "Select the object to measure, near the end to measure from";
        case Step::Amount:
            return kind_ == Kind::Divide ? "Enter the number of segments or [Style]"
                                         : "Specify length of segment or [Style]";
        case Step::Style:
            return "Style for the points, or ByLayer <" +
                   (attributes_.style.empty() ? std::string("ByLayer") : attributes_.style) +
                   ">";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return step_ == Step::Object ? ToolInput::Entity : ToolInput::Value;
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        if (step_ != Step::Object) {
            return InteractiveTool::entity(id, at);
        }
        const Entity* entity =
            document_ != nullptr ? document_->model().entities.find(id) : nullptr;
        if (entity == nullptr) {
            return ToolStep::rejected("that entity is not in the drawing");
        }
        auto path = Path::of(entity->geometry);
        if (!path) {
            return ToolStep::rejected(withArticle(kindName(entity->geometry)) +
                                      " has no length to place points along");
        }
        if (path->length() <= tolerance::kGeometric) {
            return ToolStep::rejected("that " + kindName(entity->geometry) + " has no length");
        }
        // From the nearer end, for an open object only: a closed one has no
        // end, and is measured from its start.
        fromEnd_ = !path->closed() &&
                   at.distanceTo(path->pointAt(path->length())) < at.distanceTo(path->pointAt(0.0));
        source_ = *entity;
        path_ = std::move(path);
        step_ = Step::Amount;
        return ToolStep::next("Length " + fixed(path_->length()));
    }

    ToolStep value(std::string_view text) override
    {
        if (step_ == Step::Style) {
            return chooseStyle(text);
        }
        if (step_ != Step::Amount) {
            return InteractiveTool::value(text);
        }
        if (isOption(text, "Style", "S")) {
            step_ = Step::Style;
            return ToolStep::next();
        }
        return kind_ == Kind::Divide ? divide(text) : measure(text);
    }

    ToolStep enter() override
    {
        switch (step_) {
        case Step::Object:
            return ToolStep::done(nullptr);
        case Step::Amount:
            return ToolStep::rejected(kind_ == Kind::Divide ? "type the number of segments"
                                                            : "type the length of a segment");
        case Step::Style:
            step_ = Step::Amount; // the style stays as it is
            return ToolStep::next();
        }
        return ToolStep::done(nullptr);
    }

    ToolStep undo() override
    {
        switch (step_) {
        case Step::Object:
            return ToolStep::rejected("nothing to undo; no object has been picked yet");
        case Step::Amount:
            step_ = Step::Object;
            path_.reset();
            return ToolStep::next();
        case Step::Style:
            step_ = Step::Amount;
            return ToolStep::next();
        }
        return ToolStep::rejected("nothing to undo");
    }

    [[nodiscard]] ToolFeedback preview(const Point2& /*cursor*/) const override
    {
        ToolFeedback feedback;
        if (path_) {
            // Where the stations are counted from.
            feedback.markers.push_back(path_->pointAt(fromEnd_ ? path_->length() : 0.0));
        }
        return feedback;
    }

  private:
    enum class Step { Object, Amount, Style };

    ToolStep chooseStyle(std::string_view text)
    {
        const std::string name(katana::core::trimmed(text));
        if (katana::core::equalsIgnoringCase(name, "ByLayer")) {
            attributes_.style.clear();
        } else if (document_ == nullptr || !document_->model().styles.contains(name)) {
            return ToolStep::rejected("there is no style '" + name +
                                      "' in the drawing (Format > Styles makes one)");
        } else {
            attributes_.style = name;
        }
        step_ = Step::Amount;
        return ToolStep::next();
    }

    ToolStep divide(std::string_view text)
    {
        const auto parts = katana::core::parseInteger(katana::core::trimmed(text));
        if (!parts || *parts < 2) {
            return ToolStep::rejected("the number of segments is a whole number of 2 or more");
        }
        if (*parts > kMostPoints) {
            return ToolStep::rejected("that would make " + std::to_string(*parts) +
                                      " points; the most is " + std::to_string(kMostPoints));
        }
        const auto n = static_cast<std::size_t>(*parts);
        const double length = path_->length();
        std::vector<double> stations;
        // k * L / n rather than a running sum, so the last station is as
        // exact as the first and no error accumulates along the object.
        for (std::size_t k = path_->closed() ? 0 : 1; k < n; ++k) {
            stations.push_back(length * static_cast<double>(k) / static_cast<double>(n));
        }
        return place(stations, "Divided into " + std::to_string(n) + " parts");
    }

    ToolStep measure(std::string_view text)
    {
        const auto every = modify_edit::parseNumber(text);
        if (!every || *every <= tolerance::kGeometric) {
            return ToolStep::rejected("the length of a segment is a number more than zero");
        }
        const double length = path_->length();
        // Short of the far end by more than the tolerance, so a length that
        // divides the object exactly does not put a point on its end.
        const double count = std::floor((length - tolerance::kGeometric) / *every);
        if (count < 1.0) {
            return ToolStep::rejected("the object is " + fixed(length) +
                                      " long, shorter than one segment");
        }
        if (count > static_cast<double>(kMostPoints)) {
            return ToolStep::rejected("that would make " + fixed(count, 0) +
                                      " points; the most is " + std::to_string(kMostPoints));
        }
        std::vector<double> stations;
        for (std::size_t k = 1; k <= static_cast<std::size_t>(count); ++k) {
            const double along = *every * static_cast<double>(k);
            stations.push_back(fromEnd_ ? length - along : along);
        }
        return place(stations, "Measured every " + modify_edit::formatNumber(*every));
    }

    ToolStep place(const std::vector<double>& stations, std::string what)
    {
        std::vector<Entity> points;
        points.reserve(stations.size());
        for (const double station : stations) {
            Entity point;
            point.geometry = katana::entity::PointGeometry{path_->pointAt(station)};
            point.layer = attributes_.layer;
            point.style = attributes_.style;
            point.color = attributes_.color;
            const auto height = modify_edit::heightAt(
                source_, std::get<katana::entity::PointGeometry>(point.geometry).position);
            if (height) {
                katana::entity::setHeights(point.properties, {height});
            }
            points.push_back(std::move(point));
        }
        const std::size_t count = points.size();
        return ToolStep::done(cmd::createEntities(std::move(points)),
                              what + ": " + std::to_string(count) +
                                  (count == 1 ? " point" : " points"),
                              true);
    }

    const Document* document_ = nullptr;
    cmd::EntityAttributes attributes_;
    Kind kind_;
    Step step_ = Step::Object;
    Entity source_;
    std::optional<Path> path_;
    bool fromEnd_ = false;
};

ToolInfo along(std::string id, std::string name, int order, std::vector<std::string> aliases,
               std::string tip, AlongTool::Kind kind)
{
    ToolInfo info;
    info.id = std::move(id);
    info.name = std::move(name);
    info.category = "Draw";
    info.group = "Points";
    info.order = order;
    info.aliases = std::move(aliases);
    info.tip = std::move(tip);
    info.make = [kind](const ToolContext& context) -> std::unique_ptr<InteractiveTool> {
        return std::make_unique<AlongTool>(context, kind);
    };
    return info;
}

} // namespace

void addDrawDivideTools(ToolCatalog& catalog, const Report& report)
{
    report(catalog.add(along("draw.divide", "Divide", 10, {"DIVIDE", "DIV"},
                             "Places points dividing a line, arc, circle or polyline into equal "
                             "parts; S puts them in a style, whose symbol they then show.",
                             AlongTool::Kind::Divide)));
    report(catalog.add(along("draw.measure", "Measure", 20, {"MEASURE", "ME"},
                             "Places a point every given length along a line, arc, circle or "
                             "polyline, from the end nearer the pick; S puts them in a style.",
                             AlongTool::Kind::Measure)));
}

} // namespace katana::cad::tools
