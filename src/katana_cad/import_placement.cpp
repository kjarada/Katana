#include "katana/cad/import_placement.hpp"

#include <cstdio>

#include "katana/core/text.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::geometry::Vec2;

namespace {

constexpr std::string_view kOffsetPrefix = "OFFSET=";

// A coordinate as the log prints one: to the millimetre, as the extents are.
// + 0.0 turns the -0 a subtraction can leave into 0, so a shift of nothing
// is never "-0.000".
std::string millimetres(double value)
{
    char text[64];
    std::snprintf(text, sizeof text, "%.3f", value + 0.0);
    return text;
}

std::string pair(Vec2 value) { return millimetres(value.x) + "," + millimetres(value.y); }

} // namespace

std::string placementWord(const ImportPlacement& placement)
{
    switch (placement.mode) {
    case ImportPlacementMode::Keep:
        return {};
    case ImportPlacementMode::Local:
        return "LOCAL";
    case ImportPlacementMode::Alongside:
        return "ALONGSIDE";
    case ImportPlacementMode::Offset:
        return std::string(kOffsetPrefix) + katana::core::formatExactReal(placement.offset.x) +
               "," + katana::core::formatExactReal(placement.offset.y);
    }
    return {};
}

katana::core::Result<std::optional<ImportPlacement>> parsePlacementWord(std::string_view word)
{
    using katana::core::equalsIgnoringCase;
    if (equalsIgnoringCase(word, "LOCAL")) {
        return std::optional(ImportPlacement{ImportPlacementMode::Local, {}});
    }
    if (equalsIgnoringCase(word, "ALONGSIDE")) {
        return std::optional(ImportPlacement{ImportPlacementMode::Alongside, {}});
    }
    if (word.size() < kOffsetPrefix.size() ||
        !equalsIgnoringCase(word.substr(0, kOffsetPrefix.size()), kOffsetPrefix)) {
        return std::optional<ImportPlacement>();
    }
    const std::string_view value = word.substr(kOffsetPrefix.size());
    const std::size_t comma = value.find(',');
    const auto east = comma == std::string_view::npos
                          ? std::nullopt
                          : katana::core::parseFiniteDouble(value.substr(0, comma));
    const auto north = comma == std::string_view::npos
                           ? std::nullopt
                           : katana::core::parseFiniteDouble(value.substr(comma + 1));
    if (!east || !north) {
        return makeError(ErrorCode::InvalidArgument,
                         "OFFSET= takes the east and north to move by, as OFFSET=dE,dN",
                         std::string(word));
    }
    return std::optional(ImportPlacement{ImportPlacementMode::Offset, Vec2(*east, *north)});
}

ImportShift resolveImportShift(const ImportPlacement& placement,
                               const katana::geometry::Box2& drawing,
                               const katana::geometry::Box2& incoming)
{
    ImportShift result;
    if (placement.mode == ImportPlacementMode::Keep) {
        return result;
    }
    const std::string word =
        placement.mode == ImportPlacementMode::Offset ? "OFFSET" : placementWord(placement);
    if (incoming.empty()) {
        result.said = word + ": the file holds nothing with a position, so there is nothing "
                             "to move.";
        return result;
    }
    switch (placement.mode) {
    case ImportPlacementMode::Keep:
        break;
    case ImportPlacementMode::Local:
        result.shift = Vec2(incoming.min.x, incoming.min.y);
        result.said = "LOCAL: moved as one piece by " + pair(Vec2(0.0 - incoming.min.x,
                                                                  0.0 - incoming.min.y)) +
                      ", so its lower-left corner sits at 0,0.";
        break;
    case ImportPlacementMode::Alongside:
        if (drawing.empty()) {
            result.said = "ALONGSIDE: the drawing is empty, so there is nothing to sit beside; "
                          "the data keeps its own coordinates.";
            break;
        }
        result.shift = Vec2(incoming.min.x - drawing.min.x, incoming.min.y - drawing.min.y);
        result.said = "ALONGSIDE: moved as one piece by " +
                      pair(Vec2(drawing.min.x - incoming.min.x, drawing.min.y - incoming.min.y)) +
                      ", so its lower-left corner sits on the drawing's, at " +
                      pair(Vec2(drawing.min.x, drawing.min.y)) + ".";
        break;
    case ImportPlacementMode::Offset:
        result.shift = Vec2(0.0 - placement.offset.x, 0.0 - placement.offset.y);
        result.said = "OFFSET: moved as one piece by " + pair(placement.offset) + ".";
        break;
    }
    return result;
}

} // namespace katana::cad
