// SURFACE LEGEND and SURFACE PLAN (cad/terrain/surface_display_verbs.hpp,
// docs/surface_display.md "On the command line").

#include "katana/cad/terrain/surface_display_verbs.hpp"

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <tuple>
#include <utility>
#include <vector>

#include <nlohmann/json.hpp>

#include "katana/cad/terrain/surface_colouring.hpp"
#include "katana/cad/terrain/surface_plan.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/core/text.hpp"

namespace katana::cad {

namespace {

using katana::core::ErrorCode;
using katana::core::formatExactReal;
using katana::core::makeError;
using katana::core::replyQuoted;
using katana::geometry::Box2;
using katana::geometry::Point2;

// The plan view's frame a verb looks through when none is given: the plan
// benchmark's 1600 x 1000 (docs/plan_view.md, "The benchmark").
constexpr int kDefaultWidthPixels = 1600;
constexpr int kDefaultHeightPixels = 1000;
// The largest frame a verb is asked to look through: twice a 4K display each
// way, past any view a person has. The image a frame gets is held to
// kImagePixelCap whatever the frame: 16 million pixels of 4 bytes, 64
// megabytes at most (an uncapped 7680 x 7680 image would be 236).
constexpr int kMaximumPixels = 7680;
// The pixel cap the plan painter gives a surface's image on screen
// (PlanPaintOptions::rasterPixelCap).
constexpr std::size_t kImagePixelCap = 16'000'000;

std::string kindOf(const SurfaceLegend& legend)
{
    switch (legend.faces) {
    case SurfaceFaces::None:
        return "none";
    case SurfaceFaces::Flat:
        return "flat";
    case SurfaceFaces::Aspect:
        return "sectors";
    case SurfaceFaces::Elevation:
    case SurfaceFaces::Slope:
        break;
    }
    return legend.gradient ? "gradient" : "bands";
}

const katana::terrain::NamedSurface* surfaceNamed(const Document& document, const std::string& name)
{
    return document.surfaces().find(name);
}

katana::core::Error noSurface(const std::string& name)
{
    return makeError(ErrorCode::NotFound, "no surface of that name; SURFACE LIST lists them", name);
}

katana::core::Result<std::string> legendVerb(const Document& document,
                                             std::span<const std::string> args)
{
    if (args.size() < 2) {
        return makeError(ErrorCode::ParseFailure, "SURFACE LEGEND <name> [3D] [JSON]");
    }
    const katana::terrain::NamedSurface* named = surfaceNamed(document, args[1]);
    if (named == nullptr || named->surface == nullptr) {
        return noSurface(args[1]);
    }
    bool threeD = false;
    bool json = false;
    for (std::size_t i = 2; i < args.size(); ++i) {
        const std::string word = katana::core::uppered(args[i]);
        if (word == "3D") {
            threeD = true;
        } else if (word == "JSON") {
            json = true;
        } else {
            return makeError(ErrorCode::InvalidArgument,
                             "is not an option of SURFACE LEGEND <name>: 3D, JSON", args[i]);
        }
    }
    const SurfaceDisplayStyle& style = styleOf(document.surfaceStyles(), *named);
    // A wireframe or hidden surface is drawn in no colour of its faces
    // (docs/surface_display.md, "In 3D"), whatever faces3d says.
    const bool facesDrawn = !threeD || (style.shading != SurfaceStyle::Wireframe &&
                                        style.shading != SurfaceStyle::Hidden);
    const SurfaceFaces faces =
        !facesDrawn ? SurfaceFaces::None : (threeD ? style.faces3d : style.planFaces);
    const SurfaceColouring colouring = surfaceColouring(style, faces, *named->surface);
    const SurfaceLegend legend = surfaceLegend(colouring);

    if (json) {
        nlohmann::ordered_json out;
        out["surface"] = named->name;
        out["style"] = style.name;
        out["view"] = threeD ? "3d" : "plan";
        out["faces"] = std::string(surfaceFacesWord(faces));
        out["kind"] = kindOf(legend);
        out["title"] = legend.title;
        out["entries"] = nlohmann::ordered_json::array();
        for (const SurfaceLegendEntry& entry : legend.entries) {
            nlohmann::ordered_json item;
            item["colour"] = entry.colour.toHex();
            item["from"] = entry.from.has_value() ? nlohmann::ordered_json(*entry.from) : nullptr;
            item["to"] = entry.to.has_value() ? nlohmann::ordered_json(*entry.to) : nullptr;
            item["label"] = entry.label;
            out["entries"].push_back(std::move(item));
        }
        return out.dump();
    }

    std::string reply = "legend surface=" + replyQuoted(named->name) +
                        " style=" + replyQuoted(style.name) + " view=" + (threeD ? "3d" : "plan") +
                        " faces=" + std::string(surfaceFacesWord(faces)) + " kind=" + kindOf(legend) +
                        " entries=" + std::to_string(legend.entries.size());
    for (std::size_t i = 0; i < legend.entries.size(); ++i) {
        const SurfaceLegendEntry& entry = legend.entries[i];
        reply += "\nentry index=" + std::to_string(i + 1) + " colour=" + entry.colour.toHex();
        if (entry.from.has_value()) {
            reply += " from=" + formatExactReal(*entry.from);
        }
        if (entry.to.has_value()) {
            reply += " to=" + formatExactReal(*entry.to);
        }
        if (!entry.label.empty()) {
            reply += " label=" + replyQuoted(entry.label);
        }
    }
    return reply;
}

katana::core::Result<std::pair<int, int>> readPixels(const std::string& value)
{
    const auto comma = value.find(',');
    const auto width = comma == std::string::npos
                           ? std::optional<std::int64_t>{}
                           : katana::core::parseInteger(std::string_view(value).substr(0, comma));
    const auto height = comma == std::string::npos
                            ? std::optional<std::int64_t>{}
                            : katana::core::parseInteger(std::string_view(value).substr(comma + 1));
    if (!width || !height || *width < 1 || *height < 1 || *width > kMaximumPixels ||
        *height > kMaximumPixels) {
        return makeError(ErrorCode::InvalidArgument,
                         "pixels= is W,H, two whole numbers from 1 to " + std::to_string(kMaximumPixels),
                         value);
    }
    return std::pair{static_cast<int>(*width), static_cast<int>(*height)};
}

std::string boxText(const Box2& box)
{
    return formatExactReal(box.min.x) + "," + formatExactReal(box.min.y) + "," +
           formatExactReal(box.max.x) + "," + formatExactReal(box.max.y);
}

katana::core::Result<std::string> planVerb(const Document& document, std::span<const std::string> args,
                                           const ScopeViewProvider& views)
{
    if (args.size() < 2) {
        return makeError(ErrorCode::ParseFailure,
                         "SURFACE PLAN <name> [AREA x0,y0,x1,y1 | VIEW [id]] [pixels=W,H]");
    }
    const katana::terrain::NamedSurface* named = surfaceNamed(document, args[1]);
    if (named == nullptr || named->surface == nullptr) {
        return noSurface(args[1]);
    }
    std::vector<std::string> words;
    int width = kDefaultWidthPixels;
    int height = kDefaultHeightPixels;
    bool pixelsGiven = false;
    for (std::size_t i = 2; i < args.size(); ++i) {
        const std::string lowered = katana::core::lowered(args[i]);
        if (lowered.starts_with("pixels=")) {
            auto read = readPixels(args[i].substr(7));
            if (!read) {
                return read.error();
            }
            std::tie(width, height) = *read;
            pixelsGiven = true;
            continue;
        }
        if (lowered == "where") {
            return makeError(ErrorCode::InvalidArgument,
                             "a surface is not entities: SURFACE PLAN takes AREA or VIEW, and no WHERE",
                             args[i]);
        }
        words.push_back(args[i]);
    }

    // The window looked through: the whole surface, AREA, or what the plan
    // view shows - through the one scope parser.
    Box2 area = named->surface->bounds();
    std::string scope = "scope=surface";
    // VIEW: the plan view's own transform, so the surface is judged at the
    // pixels per unit the view draws it at (absent: the area is fitted to
    // width x height).
    std::optional<ViewTransform> viewFrame;
    if (!words.empty()) {
        std::size_t at = 0;
        auto parsed = parseScopeWords(words, at);
        if (!parsed) {
            return parsed.error();
        }
        if (at != words.size()) {
            return makeError(ErrorCode::InvalidArgument,
                             "is not an option of SURFACE PLAN <name>: AREA x0,y0,x1,y1, VIEW [id], "
                             "pixels=W,H",
                             words[at]);
        }
        if (parsed->source == ScopeSource::Area) {
            area = parsed->area;
            scope = "scope=area area=" + boxText(area);
        } else if (parsed->source == ScopeSource::View) {
            // EXTENTS drops the view's area (resolveScope), and a plan is
            // looked at through one: say so, not that the view has none.
            if (parsed->extents) {
                return makeError(ErrorCode::InvalidArgument,
                                 "SURFACE PLAN looks at an area, and VIEW EXTENTS has none: drop "
                                 "EXTENTS for the view's own area, or give AREA x0,y0,x1,y1",
                                 "EXTENTS");
            }
            auto resolved = resolveScope(*parsed, views);
            if (!resolved) {
                return resolved.error();
            }
            if (!resolved->scope.area.has_value()) {
                return makeError(ErrorCode::InvalidArgument,
                                 "that view has no plan area; name a plan view, or give AREA",
                                 words.front());
            }
            area = *resolved->scope.area;
            scope = "scope=view view=" + std::to_string(resolved->view.value_or(0)) +
                    " area=" + boxText(area);
            // The view's own size and scale, unless pixels= asks for another
            // frame: what the plan view draws, not what a 1600 x 1000 one
            // would.
            if (!pixelsGiven && resolved->frame.has_value() && resolved->frame->scale > 0.0 &&
                std::isfinite(resolved->frame->scale)) {
                viewFrame = resolved->frame;
                width = std::clamp(static_cast<int>(std::lround(viewFrame->widthPixels)), 1, kMaximumPixels);
                height = std::clamp(static_cast<int>(std::lround(viewFrame->heightPixels)), 1, kMaximumPixels);
            }
        } else {
            return makeError(ErrorCode::InvalidArgument,
                             "a surface is not entities: SURFACE PLAN takes AREA x0,y0,x1,y1 or VIEW",
                             words.front());
        }
    }

    // The plan view's own frame (VIEW): its box at its scale. Otherwise
    // looking at `area` as a plan view of width x height pixels frames it:
    // centred, at the scale that fits it on both sides.
    const double unitsPerPixel =
        viewFrame.has_value()
            ? 1.0 / viewFrame->scale
            : std::max(area.width() / static_cast<double>(width), area.height() / static_cast<double>(height));
    // A point has no frame: the counts of a plan that was never worked out
    // would read as "nothing of the surface is there".
    if (!(unitsPerPixel > 0.0) || !std::isfinite(unitsPerPixel)) {
        return makeError(ErrorCode::InvalidArgument,
                         "AREA has no width and no height: a plan view cannot frame a point", boxText(area));
    }
    Box2 visible = area;
    if (!viewFrame.has_value()) {
        const Point2 centre = area.center();
        visible = Box2(Point2(centre.x - 0.5 * width * unitsPerPixel, centre.y - 0.5 * height * unitsPerPixel),
                       Point2(centre.x + 0.5 * width * unitsPerPixel, centre.y + 0.5 * height * unitsPerPixel));
    }
    SurfacePlanView view;
    view.visible = visible;
    view.unitsPerPixel = unitsPerPixel;
    const SurfacePlanImageGrid grid =
        planImageGrid(visible, named->surface->bounds(), unitsPerPixel, kImagePixelCap);
    view.imageBox = grid.box;
    view.imageWidth = grid.width;
    view.imageHeight = grid.height;
    SurfacePlanCache cache;
    const SurfacePlan plan = surfacePlan(*named, styleOf(document.surfaceStyles(), *named), view, cache);
    const SurfaceDisplayStyle& style = styleOf(document.surfaceStyles(), *named);
    const SurfacePlanStats& s = plan.stats;
    std::string reply = "plan surface=" + replyQuoted(named->name) + " style=" + replyQuoted(style.name) +
                        " visible=" + (named->visible ? "yes" : "no") +
                        " pixels=" + std::to_string(width) + "," + std::to_string(height) +
                        " unitsPerPixel=" + formatExactReal(unitsPerPixel) +
                        " triangles=" + std::to_string(named->surface->triangleCount()) +
                        " inView=" + std::to_string(s.trianglesInView) +
                        " edges=" + std::to_string(s.edgesStroked) +
                        " tinted=" + std::to_string(s.trianglesTinted) +
                        " faces=" + std::string(surfaceFacesWord(style.planFaces)) +
                        " image=" + std::to_string(plan.image.width) + "x" + std::to_string(plan.image.height) +
                        " covered=" + std::to_string(s.pixelsCovered) +
                        " levels=" + std::to_string(s.contourLevels) +
                        " contours=" + std::to_string(s.contourRuns) +
                        " minor=" + (style.contours.show ? (s.minorContoursHidden ? "hidden" : "drawn") : "off") +
                        " major=" + (style.contours.show ? (s.majorContoursHidden ? "hidden" : "drawn") : "off") +
                        " labels=" + std::to_string(s.labels) +
                        " points=" + std::to_string(s.pointsMarked) +
                        " pointPixels=" + std::to_string(s.pointsInImage) +
                        " boundary=" + std::to_string(s.boundaryRuns) +
                        " breaklines=" + std::to_string(s.breaklineRuns);
    if (!plan.problem.empty()) {
        reply += " problem=" + replyQuoted(plan.problem);
    }
    reply += "\n" + scope;
    return reply;
}

} // namespace

bool isSurfaceDisplayVerb(std::string_view verb, std::span<const std::string> args)
{
    if (!katana::core::equalsIgnoringCase(verb, "SURFACE") || args.empty()) {
        return false;
    }
    return katana::core::equalsIgnoringCase(args.front(), "LEGEND") ||
           katana::core::equalsIgnoringCase(args.front(), "PLAN");
}

katana::core::Result<std::string> runSurfaceDisplayVerb(const Document& document,
                                                        std::span<const std::string> args,
                                                        const ScopeViewProvider& views)
{
    if (args.empty()) {
        return makeError(ErrorCode::ParseFailure, "SURFACE LEGEND|PLAN <name> ...");
    }
    if (katana::core::equalsIgnoringCase(args.front(), "LEGEND")) {
        return legendVerb(document, args);
    }
    if (katana::core::equalsIgnoringCase(args.front(), "PLAN")) {
        return planVerb(document, args, views);
    }
    return makeError(ErrorCode::ParseFailure, "SURFACE LEGEND|PLAN <name> ...", args.front());
}

std::string surfaceDisplayVerbHelp()
{
    return R"(Display   SURFACE LEGEND name [3D] [JSON]   the colours of its faces in its style: one
          record a band, stop or sector (3D: faces3d over its own heights)
          SURFACE PLAN name [AREA x0,y0,x1,y1 | VIEW [id]] [pixels=W,H]   what the plan
          view draws of it there: triangles in view, stroked and tinted, contours,
          labels, points (docs/surface_display.md)
)";
}

} // namespace katana::cad
