#include "katana/cad/plotting/generators.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <format>
#include <map>
#include <numbers>
#include <utility>

#include "katana/cad/plotting/layout.hpp"

namespace katana::cad::plotting {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Vec2;

namespace {

// Vertical exaggerations a drafter labels a section with (the owner's app's).
constexpr std::array<double, 7> kExaggerations{1.0, 2.0, 2.5, 4.0, 5.0, 8.0, 10.0};
// A long section's usual exaggeration when nothing says otherwise: H 1:500
// V 1:50, the convention road and pipe long sections are drawn at.
constexpr double kLongSectionExaggeration = 10.0;
// How much of a cross-section cell the section may fill: the rest is its
// title and the offset and level labels under it.
constexpr double kSectionFill = 0.8;
// How much of a strip's height the alignment may use; the rest keeps a curve
// off the viewport's edge, where its labels would be cut.
constexpr double kStripFill = 0.9;
// Chord tolerance for an alignment drawn as a polyline: a millimetre on the
// ground, far below anything a plotted sheet can show.
constexpr double kChordTolerance = 0.001;
// Two paper sizes this close (mm) are the same size.
constexpr double kPaperMatchMm = 2.0;

double metresAcross(double millimetres, double scale)
{
    return millimetres * scale / 1000.0;
}

Viewport planViewport(const Box2& rect, double scale, Point2 centre)
{
    Viewport viewport;
    viewport.kind = ViewportKind::Plan;
    viewport.rect = rect;
    viewport.scale = scale;
    viewport.centre = centre;
    viewport.northArrow = true;
    viewport.scaleBar = true;
    return viewport;
}

// Ids for a generator's own output: s1.., vp1.., in order. prepareForAppend
// renumbers them when the sheets join a set.
void numberSheets(std::vector<Sheet>& sheets)
{
    std::size_t viewportNumber = 0;
    for (std::size_t i = 0; i < sheets.size(); ++i) {
        if (sheets[i].id.empty()) {
            sheets[i].id = "s" + std::to_string(i + 1);
        }
        for (Viewport& viewport : sheets[i].viewports) {
            viewport.id = "vp" + std::to_string(++viewportNumber);
        }
    }
}

// A rectangle in world coordinates: `centre`, `width` along `rotation`,
// `height` across it. Counter-clockwise from the bottom-left.
std::vector<Point2> worldRectangle(Point2 centre, double width, double height, double rotation)
{
    const Vec2 along(std::cos(rotation), std::sin(rotation));
    const Vec2 up(-along.y, along.x);
    const Vec2 a = along * (width / 2.0);
    const Vec2 u = up * (height / 2.0);
    return {centre - a - u, centre + a - u, centre + a + u, centre - a + u};
}

// A key-plan sheet: every sheet's outline, numbered, fitted on one sheet.
Result<Sheet> keyPlanSheet(const SheetTemplate& paper, const std::vector<Sheet>& sheets,
                           const std::vector<std::vector<Point2>>& outlines)
{
    Box2 extent;
    for (const auto& outline : outlines) {
        for (const Point2& point : outline) {
            extent.expand(point);
        }
    }
    auto fitted = fitToSheet(extent, paper);
    if (!fitted) {
        return fitted.error();
    }
    Sheet sheet = std::move(fitted->front());
    sheet.name = "KEY PLAN";
    Viewport& key = sheet.viewports.front();
    key.kind = ViewportKind::KeyPlan;
    for (std::size_t i = 0; i < outlines.size(); ++i) {
        WorldMark mark;
        mark.kind = WorldMark::Kind::SheetOutline;
        mark.points = outlines[i];
        mark.sheet = sheets[i].id;
        key.marks.push_back(std::move(mark));
    }
    return sheet;
}

Result<geometry::SolvedAlignment> solvedAlignment(const entity::Model& model,
                                                  const std::string& name)
{
    const entity::Alignment* alignment = model.alignments.find(name);
    if (alignment == nullptr) {
        return makeError(ErrorCode::NotFound, "no alignment of that name", name);
    }
    return geometry::solveAlignment(alignment->horizontal);
}

std::optional<double> numberProperty(const entity::PropertyMap& properties, std::string_view key)
{
    const auto found = properties.find(key);
    if (found == properties.end()) {
        return std::nullopt;
    }
    if (const auto* real = std::get_if<double>(&found->second)) {
        return *real;
    }
    if (const auto* integer = std::get_if<std::int64_t>(&found->second)) {
        return static_cast<double>(*integer);
    }
    return std::nullopt;
}

// The ISO A size `width` x `height` millimetres is, either way round.
std::optional<std::pair<PaperSize, bool>> isoPaper(double width, double height)
{
    const bool landscape = width >= height;
    for (const PaperSize paper :
         {PaperSize::A0, PaperSize::A1, PaperSize::A2, PaperSize::A3, PaperSize::A4}) {
        const PaperDimensions size = paperDimensions(paper, landscape);
        if (std::abs(size.widthMm - width) <= kPaperMatchMm &&
            std::abs(size.heightMm - height) <= kPaperMatchMm) {
            return std::pair{paper, landscape};
        }
    }
    return std::nullopt;
}

Box2 intersection(const Box2& a, const Box2& b)
{
    return Box2(Point2(std::max(a.min.x, b.min.x), std::max(a.min.y, b.min.y)),
                Point2(std::min(a.max.x, b.max.x), std::min(a.max.y, b.max.y)));
}

// Appends a generator's output to `out`, renumbered after what is there.
void append(std::vector<Sheet>& out, std::vector<Sheet> more)
{
    SheetSet so_far;
    so_far.sheets = out;
    prepareForAppend(so_far, more);
    for (Sheet& sheet : more) {
        out.push_back(std::move(sheet));
    }
}

} // namespace

Sheet blankSheet(const SheetTemplate& paper, std::string name)
{
    Sheet sheet;
    sheet.name = std::move(name);
    sheet.paper = paper.paper;
    sheet.landscape = paper.landscape;
    // A portrait sheet has no frame (frame.hpp); say so rather than name one
    // it cannot have.
    sheet.frame = paper.landscape ? paper.frame : std::string{};
    return sheet;
}

Result<std::vector<Sheet>> fitToSheet(const Box2& extent, const SheetTemplate& paper)
{
    if (extent.empty() || !std::isfinite(extent.width()) || !std::isfinite(extent.height()) ||
        (!(extent.width() > 0.0) && !(extent.height() > 0.0))) {
        return makeError(ErrorCode::InvalidArgument, "there is no extent to fit on a sheet");
    }
    Sheet sheet = blankSheet(paper, "PLAN");
    const Box2 area = tilingArea(sheet);
    // The denominator at which each dimension just fits: metres to
    // millimetres, over the millimetres available.
    const double needed = std::max(extent.width() * 1000.0 / area.width(),
                                   extent.height() * 1000.0 / area.height());
    auto scale = sheetScaleAtLeast(needed);
    if (!scale) {
        return scale.error();
    }
    sheet.viewports.push_back(planViewport(area, *scale, extent.center()));
    std::vector<Sheet> sheets{std::move(sheet)};
    numberSheets(sheets);
    return sheets;
}

Result<std::vector<Sheet>> gridSheets(const GridRequest& request)
{
    const Box2& area = request.area;
    if (area.empty() || !(area.width() > 0.0) || !(area.height() > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "the area to tile has no size");
    }
    if (!(request.scale > 0.0) || !std::isfinite(request.scale)) {
        return makeError(ErrorCode::InvalidArgument, "the scale must be positive");
    }
    const Box2 rect = tilingArea(blankSheet(request.paper, {}));
    const double tileW = metresAcross(rect.width(), request.scale);
    const double tileH = metresAcross(rect.height(), request.scale);
    const double overlap = request.overlapM;
    if (!(overlap >= 0.0) || !(overlap < std::min(tileW, tileH))) {
        return makeError(ErrorCode::InvalidArgument,
                         "the overlap must be at least zero and less than a tile",
                         std::format("{} m against tiles of {} x {} m", overlap, tileW, tileH));
    }
    const double stepX = tileW - overlap;
    const double stepY = tileH - overlap;
    // Tiles needed along each axis; the 1e-9 keeps an area that fits exactly
    // from being given a sliver of a tile by rounding.
    const auto count = [](double length, double tile, double step, double shared) {
        return length <= tile ? std::size_t{1}
                              : static_cast<std::size_t>(
                                    std::ceil((length - shared) / step - 1e-9));
    };
    const std::size_t columns = count(area.width(), tileW, stepX, overlap);
    const std::size_t rows = count(area.height(), tileH, stepY, overlap);
    // The tiles cover a little more than the area; centre them on it.
    const double left0 = area.center().x - (static_cast<double>(columns) * stepX + overlap) / 2.0;
    const double top0 = area.center().y + (static_cast<double>(rows) * stepY + overlap) / 2.0;

    std::vector<Sheet> tiles;
    std::vector<std::vector<Point2>> outlines;
    const std::size_t first = request.keyPlan ? 2 : 1; // the key plan is sheet 1
    const auto idOf = [first, columns](std::size_t row, std::size_t column) {
        return "s" + std::to_string(first + row * columns + column);
    };
    for (std::size_t row = 0; row < rows; ++row) {
        for (std::size_t column = 0; column < columns; ++column) {
            const double left = left0 + static_cast<double>(column) * stepX;
            const double top = top0 - static_cast<double>(row) * stepY;
            const Box2 tile(Point2(left, top - tileH), Point2(left + tileW, top));
            Sheet sheet = blankSheet(request.paper,
                                     std::format("PLAN TILE {}", row * columns + column + 1));
            sheet.id = idOf(row, column);
            Viewport plan = planViewport(rect, request.scale, tile.center());
            // A match line in the middle of each shared strip, facing the
            // neighbour across it.
            const auto match = [&plan](Point2 a, Point2 b, std::string neighbour) {
                plan.marks.push_back(
                    {WorldMark::Kind::MatchLine, {a, b}, "MATCH LINE", std::move(neighbour)});
            };
            if (column + 1 < columns) {
                const double x = tile.max.x - overlap / 2.0;
                match(Point2(x, tile.min.y), Point2(x, tile.max.y), idOf(row, column + 1));
            }
            if (column > 0) {
                const double x = tile.min.x + overlap / 2.0;
                match(Point2(x, tile.min.y), Point2(x, tile.max.y), idOf(row, column - 1));
            }
            if (row > 0) {
                const double y = tile.max.y - overlap / 2.0;
                match(Point2(tile.min.x, y), Point2(tile.max.x, y), idOf(row - 1, column));
            }
            if (row + 1 < rows) {
                const double y = tile.min.y + overlap / 2.0;
                match(Point2(tile.min.x, y), Point2(tile.max.x, y), idOf(row + 1, column));
            }
            sheet.viewports.push_back(std::move(plan));
            outlines.push_back(worldRectangle(tile.center(), tileW, tileH, 0.0));
            tiles.push_back(std::move(sheet));
        }
    }
    std::vector<Sheet> sheets;
    if (request.keyPlan) {
        auto key = keyPlanSheet(request.paper, tiles, outlines);
        if (!key) {
            return key.error();
        }
        key->id = "s1";
        sheets.push_back(std::move(*key));
    }
    for (Sheet& tile : tiles) {
        sheets.push_back(std::move(tile));
    }
    numberSheets(sheets);
    return sheets;
}

Result<std::vector<Sheet>> stripSheets(const geometry::SolvedAlignment& alignment,
                                       const std::string& name, const StripRequest& request)
{
    if (!(request.scale > 0.0) || !std::isfinite(request.scale)) {
        return makeError(ErrorCode::InvalidArgument, "the scale must be positive");
    }
    const Box2 rect = request.planRect.value_or(tilingArea(blankSheet(request.paper, {})));
    const double stripW = metresAcross(rect.width(), request.scale);
    const double stripH = metresAcross(rect.height(), request.scale);
    const double overlap = request.overlapM;
    if (!(overlap >= 0.0) || !(overlap < stripW)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the overlap must be at least zero and less than a strip");
    }
    const double from = std::max(request.fromChainage.value_or(alignment.startStation()),
                                 alignment.startStation());
    const double to =
        std::min(request.toChainage.value_or(alignment.endStation()), alignment.endStation());
    if (!(to > from)) {
        return makeError(ErrorCode::InvalidArgument, "the chainage range is empty",
                         std::format("{} to {}", from, to));
    }
    const double usable = stripW - overlap;
    // Sample spacing along the alignment for the fit test: a hundredth of the
    // strip's smaller side, so no bend between samples can leave it unseen.
    const double spacing = std::min(stripW, stripH) / 100.0;

    struct Strip {
        double from;
        double to;
        Point2 centre;
        double rotation;
    };
    // Whether [a, b] fits in one strip, and where the strip then sits: along
    // the chord from a to b, centred on the alignment's extent across it.
    const auto place = [&](double a, double b) -> std::optional<Strip> {
        const Point2 start = *alignment.pointAtStation(a);
        const Point2 end = *alignment.pointAtStation(b);
        const Vec2 chord = end - start;
        const double rotation = chord.length() > 1e-9
                                    ? chord.angle()
                                    : *alignment.directionAtStation((a + b) / 2.0);
        const Vec2 along(std::cos(rotation), std::sin(rotation));
        const Vec2 up(-along.y, along.x);
        double alongMin = 0.0;
        double alongMax = 0.0;
        double upMin = 0.0;
        double upMax = 0.0;
        const auto samples = static_cast<std::size_t>(std::ceil((b - a) / spacing));
        for (std::size_t i = 0; i <= samples; ++i) {
            const double station =
                i == samples ? b : a + (b - a) * static_cast<double>(i) / static_cast<double>(samples);
            const Vec2 offset = *alignment.pointAtStation(station) - start;
            alongMin = std::min(alongMin, offset.dot(along));
            alongMax = std::max(alongMax, offset.dot(along));
            upMin = std::min(upMin, offset.dot(up));
            upMax = std::max(upMax, offset.dot(up));
        }
        if (alongMax - alongMin > usable + 1e-9 || upMax - upMin > stripH * kStripFill) {
            return std::nullopt;
        }
        const Point2 centre = start + along * ((alongMin + alongMax) / 2.0) +
                              up * ((upMin + upMax) / 2.0);
        return Strip{a, b, centre, rotation};
    };

    std::vector<Strip> strips;
    double at = from;
    while (to - at > 1e-6) {
        double end = std::min(at + usable, to);
        std::optional<Strip> strip = place(at, end);
        // Shorter until it fits: a curve tighter than the strip is tall
        // bends out of it before the strip's length is used. A tenth at a
        // time, and never below a twentieth of a strip - a hairpin that
        // fits nowhere is drawn in short strips rather than not at all.
        for (int attempt = 0; !strip && attempt < 30; ++attempt) {
            const double shorter = at + (end - at) * 0.9;
            if (shorter - at < usable / 20.0) {
                break;
            }
            end = shorter;
            strip = place(at, end);
        }
        if (!strip) {
            const Point2 centre = *alignment.pointAtStation((at + end) / 2.0);
            strip = Strip{at, end, centre, *alignment.directionAtStation((at + end) / 2.0)};
        }
        strips.push_back(*strip);
        at = end;
    }

    std::vector<Sheet> sheets;
    std::vector<std::vector<Point2>> outlines;
    const std::size_t first = request.keyPlan ? 2 : 1;
    const auto idOf = [first](std::size_t index) { return "s" + std::to_string(first + index); };
    for (std::size_t i = 0; i < strips.size(); ++i) {
        const Strip& strip = strips[i];
        Sheet sheet =
            blankSheet(request.paper, std::format("{} CH {:.3f} TO {:.3f}", name, strip.from, strip.to));
        sheet.id = idOf(i);
        Viewport plan = planViewport(rect, request.scale, strip.centre);
        plan.rotation = strip.rotation;
        plan.source.alignment = name;
        plan.source.chainageFrom = strip.from;
        plan.source.chainageTo = strip.to;
        // Match lines square across the alignment where one sheet hands over
        // to the next, as long as the strip is tall.
        const auto matchAt = [&](double chainage, std::string neighbour) {
            const auto left = alignment.pointAtStationOffset(chainage, stripH / 2.0);
            const auto right = alignment.pointAtStationOffset(chainage, -stripH / 2.0);
            if (left && right) {
                plan.marks.push_back({WorldMark::Kind::MatchLine,
                                      {*right, *left},
                                      std::format("MATCH LINE CH {:.3f}", chainage),
                                      std::move(neighbour)});
            }
        };
        if (i > 0) {
            matchAt(strip.from, idOf(i - 1));
        }
        if (i + 1 < strips.size()) {
            matchAt(strip.to, idOf(i + 1));
        }
        sheet.viewports.push_back(std::move(plan));
        outlines.push_back(worldRectangle(strip.centre, stripW, stripH, strip.rotation));
        sheets.push_back(std::move(sheet));
    }
    if (request.keyPlan) {
        auto key = keyPlanSheet(request.paper, sheets, outlines);
        if (!key) {
            return key.error();
        }
        key->id = "s1";
        sheets.insert(sheets.begin(), std::move(*key));
    }
    numberSheets(sheets);
    return sheets;
}

Result<std::vector<Sheet>> crossSectionSheets(const geometry::SolvedAlignment& alignment,
                                              const std::string& name,
                                              const CrossSectionRequest& request)
{
    if (request.rows == 0 || request.columns == 0) {
        return makeError(ErrorCode::InvalidArgument, "a sheet needs at least one row and column");
    }
    if (!(request.halfWidth > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "a cross section needs a width");
    }
    const geometry::Polyline2 line = alignment.toPolyline(kChordTolerance);
    // Distances along the alignment as drawn, from its start. cad::section
    // works in these; the chainage is the alignment's start station plus it.
    std::vector<double> distances;
    if (request.stations.empty()) {
        auto stations = sectionStations(line, request.interval);
        if (!stations) {
            return stations.error();
        }
        distances = std::move(*stations);
    } else {
        for (const double chainage : request.stations) {
            distances.push_back(chainage - alignment.startStation());
        }
        std::sort(distances.begin(), distances.end());
        distances.erase(std::unique(distances.begin(), distances.end()), distances.end());
    }

    struct Cut {
        double chainage;
        std::optional<double> low;
        std::optional<double> high;
    };
    std::vector<Cut> cuts;
    for (const double distance : distances) {
        auto across = crossSectionLine(line, distance, request.halfWidth);
        if (!across) {
            return across.error();
        }
        Cut cut{alignment.startStation() + distance, std::nullopt, std::nullopt};
        if (!request.surfaces.empty()) {
            SectionOptions options;
            options.interval = request.halfWidth / 50.0;
            options.includeCrossings = false;
            auto section = extractSection(*across, request.surfaces, nullptr, options);
            if (!section) {
                return section.error();
            }
            const Box2 extent = section->extent();
            if (!extent.empty()) {
                cut.low = extent.min.y;
                cut.high = extent.max.y;
            }
        }
        cuts.push_back(cut);
    }

    // The cell grid, filled down each column and then across.
    const Box2 area = tilingArea(blankSheet(request.paper, {}));
    const double gutter = kTilingGutterMm;
    const auto rows = static_cast<double>(request.rows);
    const auto columns = static_cast<double>(request.columns);
    const double cellW = (area.width() - (columns - 1.0) * gutter) / columns;
    const double cellH = (area.height() - (rows - 1.0) * gutter) / rows;
    if (!(cellW > 0.0) || !(cellH > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "too many rows or columns for the sheet");
    }
    double scale = request.scale;
    if (!(scale > 0.0)) {
        auto fitted = sheetScaleAtLeast(2.0 * request.halfWidth * 1000.0 / (cellW * kSectionFill));
        if (!fitted) {
            return fitted.error();
        }
        scale = *fitted;
    }
    double exaggeration = request.exaggeration;
    if (!(exaggeration > 0.0)) {
        double deepest = 0.0;
        for (const Cut& cut : cuts) {
            if (cut.low && cut.high) {
                deepest = std::max(deepest, *cut.high - *cut.low);
            }
        }
        exaggeration = 1.0;
        for (const double candidate : kExaggerations) {
            if (deepest * 1000.0 * candidate / scale <= cellH * kSectionFill) {
                exaggeration = candidate;
            }
        }
    }

    const std::size_t perSheet = request.rows * request.columns;
    std::vector<Sheet> sheets;
    for (std::size_t i = 0; i < cuts.size(); ++i) {
        const std::size_t slot = i % perSheet;
        if (slot == 0) {
            const std::size_t last = std::min(i + perSheet, cuts.size()) - 1;
            sheets.push_back(blankSheet(request.paper,
                                        std::format("{} CROSS SECTIONS CH {:.3f} TO {:.3f}", name,
                                                    cuts[i].chainage, cuts[last].chainage)));
        }
        const double column = static_cast<double>(slot / request.rows);
        const double row = static_cast<double>(slot % request.rows);
        const double left = area.min.x + column * (cellW + gutter);
        const double top = area.max.y - row * (cellH + gutter);
        Viewport viewport;
        viewport.kind = ViewportKind::CrossSections;
        viewport.rect = Box2(Point2(left, top - cellH), Point2(left + cellW, top));
        viewport.scale = scale;
        viewport.verticalExaggeration = exaggeration;
        viewport.source.alignment = name;
        viewport.source.stations = {cuts[i].chainage};
        viewport.source.sectionHalfWidth = request.halfWidth;
        if (cuts[i].low && cuts[i].high) {
            viewport.centre = Point2(0.0, (*cuts[i].low + *cuts[i].high) / 2.0);
        } else {
            viewport.autoCentre = true;
        }
        sheets.back().viewports.push_back(std::move(viewport));
    }
    numberSheets(sheets);
    return sheets;
}

Result<std::vector<Sheet>> sheetsFromPlotFrames(const entity::Model& model,
                                                const SheetTemplate& paper,
                                                std::vector<std::string>* skipped)
{
    std::vector<const entity::Entity*> frames;
    model.entities.forEach([&frames](const entity::Entity& entity) {
        if (entity.properties.contains("plot_frame.width")) {
            frames.push_back(&entity);
        }
    });
    std::sort(frames.begin(), frames.end(),
              [](const entity::Entity* a, const entity::Entity* b) { return a->id < b->id; });
    const auto skip = [skipped](std::string why) {
        if (skipped != nullptr) {
            skipped->push_back(std::move(why));
        }
    };
    std::vector<Sheet> sheets;
    for (const entity::Entity* frame : frames) {
        const auto& p = frame->properties;
        const std::string label = "plot frame " + std::to_string(frame->id);
        const auto width = numberProperty(p, "plot_frame.width");
        const auto height = numberProperty(p, "plot_frame.height");
        const double scale = numberProperty(p, "plot_frame.scale").value_or(1000.0);
        if (!width || !height || !(scale > 0.0)) {
            skip(label + ": no paper size or scale");
            continue;
        }
        const auto iso = isoPaper(*width, *height);
        if (!iso) {
            skip(std::format("{}: {} x {} mm is not an ISO A size", label, *width, *height));
            continue;
        }
        SheetTemplate own = paper;
        own.paper = iso->first;
        own.landscape = iso->second;
        std::string name = label;
        if (const auto found = frame->metadata.find("12d.name"); found != frame->metadata.end()) {
            if (const auto* text = std::get_if<std::string>(&found->second); text && !text->empty()) {
                name = *text;
            }
        }
        Sheet sheet = blankSheet(own, name);
        const PaperDimensions size = paperDimensions(own.paper, own.landscape);
        const double left = numberProperty(p, "plot_frame.left_margin").value_or(0.0);
        const double right = numberProperty(p, "plot_frame.right_margin").value_or(0.0);
        const double top = numberProperty(p, "plot_frame.top_margin").value_or(0.0);
        const double bottom = numberProperty(p, "plot_frame.bottom_margin").value_or(0.0);
        // The frame's own window on the paper, kept inside this sheet's
        // drawing area so it cannot run over the title block.
        const Box2 rect = intersection(
            Box2(Point2(left, bottom), Point2(size.widthMm - right, size.heightMm - top)),
            drawingArea(sheet));
        if (rect.empty() || !(rect.width() > 0.0) || !(rect.height() > 0.0)) {
            skip(label + ": its margins leave nothing to draw");
            continue;
        }
        // Paper millimetres to the ground, as the importer placed the frame:
        // from its origin, along its rotation, at its scale.
        const double rotation =
            numberProperty(p, "plot_frame.rotation").value_or(0.0) * std::numbers::pi / 180.0;
        const Point2 origin(numberProperty(p, "plot_frame.xorigin").value_or(0.0),
                            numberProperty(p, "plot_frame.yorigin").value_or(0.0));
        const Vec2 along(std::cos(rotation), std::sin(rotation));
        const Vec2 up(-along.y, along.x);
        const Point2 centre = origin + along * metresAcross(rect.center().x, scale) +
                              up * metresAcross(rect.center().y, scale);
        Viewport plan = planViewport(rect, scale, centre);
        plan.rotation = rotation;
        // The frame is an outline on the drawing; on its own sheet it would
        // be drawn along the paper's edge, over the title block.
        plan.hiddenLayers.hide(frame->layer);
        sheet.viewports.push_back(std::move(plan));
        sheets.push_back(std::move(sheet));
    }
    numberSheets(sheets);
    return sheets;
}

Result<std::vector<Sheet>> smartLayout(const entity::Model& model, const LayoutRequest& request)
{
    const bool alongAlignment = request.planAlongAlignment || request.longSection ||
                                request.crossSectionInterval > 0.0;
    if (!request.planArea && !alongAlignment && !request.model3d && !request.legend) {
        return makeError(ErrorCode::InvalidArgument, "the layout shows nothing");
    }
    std::optional<geometry::SolvedAlignment> solved;
    if (alongAlignment) {
        auto found = solvedAlignment(model, request.alignment);
        if (!found) {
            return found.error();
        }
        solved = std::move(*found);
    }
    std::vector<Sheet> out;
    const Box2 area = tilingArea(blankSheet(request.paper, {}));

    if (solved && request.planAlongAlignment) {
        // Plan and profile: the plan along the alignment above, its long
        // section below at the same horizontal scale - the sheet a road or a
        // pipe is built from.
        const std::vector<Box2> cells =
            presetCells(request.longSection ? TilingPreset::MainBelow : TilingPreset::Full, area);
        double scale = request.scale;
        if (!(scale > 0.0)) {
            auto fitted = sheetScaleAtLeast(solved->length() * 1000.0 / cells.front().width());
            if (!fitted) {
                return fitted.error();
            }
            scale = *fitted;
        }
        StripRequest strip;
        strip.scale = scale;
        strip.paper = request.paper;
        strip.planRect = cells.front();
        auto strips = stripSheets(*solved, request.alignment, strip);
        if (!strips) {
            return strips.error();
        }
        if (request.longSection) {
            for (Sheet& sheet : *strips) {
                const Viewport& plan = sheet.viewports.front();
                Viewport profile;
                profile.kind = ViewportKind::LongSection;
                profile.rect = cells[1];
                profile.scale = scale;
                profile.verticalExaggeration = kLongSectionExaggeration;
                profile.source = plan.source;
                profile.centre = Point2((plan.source.chainageFrom + plan.source.chainageTo) / 2.0, 0.0);
                profile.autoCentre = true;
                sheet.viewports.push_back(std::move(profile));
            }
        }
        append(out, std::move(*strips));
    } else if (solved && request.longSection) {
        // Long sections alone: each sheet as many chainages as its width holds.
        double scale = request.scale;
        if (!(scale > 0.0)) {
            auto fitted = sheetScaleAtLeast(solved->length() * 1000.0 / area.width());
            if (!fitted) {
                return fitted.error();
            }
            scale = *fitted;
        }
        const double perSheet = metresAcross(area.width(), scale);
        std::vector<Sheet> sections;
        for (double at = solved->startStation(); solved->endStation() - at > 1e-6; at += perSheet) {
            const double end = std::min(at + perSheet, solved->endStation());
            Sheet sheet = blankSheet(request.paper, std::format("{} LONG SECTION CH {:.3f} TO {:.3f}",
                                                                request.alignment, at, end));
            Viewport profile;
            profile.kind = ViewportKind::LongSection;
            profile.rect = area;
            profile.scale = scale;
            profile.verticalExaggeration = kLongSectionExaggeration;
            profile.source.alignment = request.alignment;
            profile.source.chainageFrom = at;
            profile.source.chainageTo = end;
            profile.centre = Point2(at + perSheet / 2.0, 0.0);
            profile.autoCentre = true;
            sheet.viewports.push_back(std::move(profile));
            sections.push_back(std::move(sheet));
        }
        numberSheets(sections);
        append(out, std::move(sections));
    }

    if (request.planArea) {
        std::vector<Sheet> plans;
        if (!(request.scale > 0.0)) {
            auto fitted = fitToSheet(*request.planArea, request.paper);
            if (!fitted) {
                return fitted.error();
            }
            plans = std::move(*fitted);
        } else {
            const double needed = std::max(request.planArea->width() * 1000.0 / area.width(),
                                           request.planArea->height() * 1000.0 / area.height());
            if (needed <= request.scale) {
                Sheet sheet = blankSheet(request.paper, "PLAN");
                sheet.viewports.push_back(planViewport(area, request.scale, request.planArea->center()));
                plans.push_back(std::move(sheet));
                numberSheets(plans);
            } else {
                GridRequest grid;
                grid.area = *request.planArea;
                grid.scale = request.scale;
                grid.paper = request.paper;
                auto tiles = gridSheets(grid);
                if (!tiles) {
                    return tiles.error();
                }
                plans = std::move(*tiles);
            }
        }
        append(out, std::move(plans));
    }

    // The 3D snapshot and the legend: beside a lone plan when there is one,
    // on a sheet of their own otherwise.
    std::vector<Viewport> extras;
    if (request.model3d) {
        Viewport view;
        view.kind = ViewportKind::Model3D;
        view.autoScale = true;
        if (request.planArea) {
            view.centre = request.planArea->center();
        } else if (solved) {
            view.centre = *solved->pointAtStation((solved->startStation() + solved->endStation()) / 2.0);
        }
        extras.push_back(std::move(view));
    }
    if (request.legend) {
        Viewport legend;
        legend.kind = ViewportKind::Legend;
        extras.push_back(std::move(legend));
    }
    if (!extras.empty()) {
        const bool lonePlan = out.size() == 1 && out.front().viewports.size() == 1 &&
                              out.front().viewports.front().kind == ViewportKind::Plan;
        if (lonePlan) {
            Sheet& sheet = out.front();
            for (Viewport& extra : extras) {
                sheet.viewports.push_back(std::move(extra));
            }
            tileViewports(sheet, sheet.viewports.size() == 2 ? TilingPreset::MainRight
                                                              : TilingPreset::MainTwoRight);
            // The plan's cell is smaller now; an automatic scale is refitted.
            Viewport& plan = sheet.viewports.front();
            if (!(request.scale > 0.0) && request.planArea) {
                const double needed =
                    std::max(request.planArea->width() * 1000.0 / plan.rect.width(),
                             request.planArea->height() * 1000.0 / plan.rect.height());
                auto fitted = sheetScaleAtLeast(needed);
                if (!fitted) {
                    return fitted.error();
                }
                plan.scale = *fitted;
            }
            numberSheets(out);
        } else {
            Sheet sheet = blankSheet(request.paper, extras.size() == 1 && request.legend
                                                        ? "LEGEND"
                                                        : "3D VIEW");
            for (Viewport& extra : extras) {
                sheet.viewports.push_back(std::move(extra));
            }
            tileViewports(sheet, sheet.viewports.size() == 1 ? TilingPreset::Full
                                                             : TilingPreset::Columns2);
            std::vector<Sheet> one{std::move(sheet)};
            numberSheets(one);
            append(out, std::move(one));
        }
    }

    if (solved && request.crossSectionInterval > 0.0) {
        CrossSectionRequest sections;
        sections.interval = request.crossSectionInterval;
        sections.halfWidth = request.crossSectionHalfWidth;
        sections.paper = request.paper;
        auto cut = crossSectionSheets(*solved, request.alignment, sections);
        if (!cut) {
            return cut.error();
        }
        append(out, std::move(*cut));
    }
    return out;
}

void prepareForAppend(const SheetSet& set, std::vector<Sheet>& sheets)
{
    SheetSet numbering = set;
    std::map<std::string, std::string> renamed;
    for (Sheet& sheet : sheets) {
        const std::string fresh = nextSheetId(numbering);
        if (!sheet.id.empty()) {
            renamed.emplace(sheet.id, fresh);
        }
        sheet.id = fresh;
        for (Viewport& viewport : sheet.viewports) {
            viewport.id.clear();
        }
        Sheet placeholder;
        placeholder.id = fresh;
        numbering.sheets.push_back(std::move(placeholder));
    }
    // Viewport ids after every id the set already uses, in order.
    std::string next = nextViewportId(set);
    std::size_t number = std::stoul(next.substr(2));
    for (Sheet& sheet : sheets) {
        for (Viewport& viewport : sheet.viewports) {
            viewport.id = "vp" + std::to_string(number++);
            for (WorldMark& mark : viewport.marks) {
                if (const auto found = renamed.find(mark.sheet); found != renamed.end()) {
                    mark.sheet = found->second;
                }
            }
        }
    }
}

} // namespace katana::cad::plotting
