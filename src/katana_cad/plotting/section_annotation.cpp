#include "katana/cad/plotting/section_annotation.hpp"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <format>
#include <numeric>
#include <type_traits>
#include <variant>

namespace katana::cad::plotting {

namespace {

// Written to two decimals, as a drafter writes a service's level and depth;
// a value that rounds to nothing is "0.00", never "-0.00".
std::string twoDecimals(double value)
{
    return std::format("{:.2f}", std::abs(value) < 0.005 ? 0.0 : value);
}

std::string upper(std::string_view text)
{
    std::string out(text);
    for (char& c : out) {
        c = static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
    }
    return out;
}

std::optional<double> between(std::optional<double> a, std::optional<double> b, double t)
{
    if (!a || !b) {
        return std::nullopt;
    }
    return *a + t * (*b - *a);
}

// Two paper boxes closer than `gap` on both axes.
bool crowds(const Box2& a, const Box2& b, double gap)
{
    return a.min.x < b.max.x + gap && b.min.x < a.max.x + gap && a.min.y < b.max.y + gap &&
           b.min.y < a.max.y + gap;
}

bool inside(const Box2& bounds, const Box2& box)
{
    constexpr double slack = 1e-9;
    return !box.empty() && box.min.x >= bounds.min.x - slack && box.max.x <= bounds.max.x + slack &&
           box.min.y >= bounds.min.y - slack && box.max.y <= bounds.max.y + slack;
}

} // namespace

// ---- the series ------------------------------------------------------------------------

bool isDesignSeries(std::string_view name)
{
    constexpr std::string_view design = "design";
    return name.size() >= design.size() && upper(name.substr(0, design.size())) == "DESIGN";
}

const SectionSurface* designSeries(const Section& section)
{
    for (const SectionSurface& surface : section.surfaces) {
        if (isDesignSeries(surface.name)) {
            return &surface;
        }
    }
    return nullptr;
}

const SectionSurface* groundSeries(const Section& section)
{
    for (const SectionSurface& surface : section.surfaces) {
        if (!isDesignSeries(surface.name)) {
            return &surface;
        }
    }
    return nullptr;
}

std::optional<double> levelAt(const SectionSurface& surface, double station)
{
    const auto& samples = surface.samples;
    if (samples.empty()) {
        return std::nullopt;
    }
    // A station a hair past an end - a range's end worked out from the
    // section's length - reads that end.
    if (std::abs(station - samples.back().station) <= kSectionEndSlack) {
        return samples.back().elevation;
    }
    if (std::abs(station - samples.front().station) <= kSectionEndSlack) {
        return samples.front().elevation;
    }
    const auto after = std::lower_bound(samples.begin(), samples.end(), station,
                                        [](const SectionSample& s, double at) {
                                            return s.station < at;
                                        });
    if (after == samples.end()) {
        return std::nullopt;
    }
    if (after->station == station) {
        return after->elevation;
    }
    if (after == samples.begin()) {
        return std::nullopt;
    }
    const auto before = std::prev(after);
    const double t = (station - before->station) / (after->station - before->station);
    return between(before->elevation, after->elevation, t);
}

std::optional<std::pair<double, double>> levelRange(const Section& section, double from, double to,
                                                    const entity::Model* model,
                                                    const LayerOverrides* hidden)
{
    double low = std::numeric_limits<double>::infinity();
    double high = -std::numeric_limits<double>::infinity();
    const auto take = [&](std::optional<double> level) {
        if (level && std::isfinite(*level)) {
            low = std::min(low, *level);
            high = std::max(high, *level);
        }
    };
    for (const SectionSurface& surface : section.surfaces) {
        for (const SectionSample& sample : surface.samples) {
            if (sample.station >= from && sample.station <= to) {
                take(sample.elevation);
            }
        }
        // A range that ends between two samples ends on the line between them.
        take(levelAt(surface, from));
        take(levelAt(surface, to));
    }
    for (const SectionCrossing& crossing : section.crossings) {
        if (crossing.station >= from && crossing.station <= to &&
            (hidden == nullptr || !hidden->hides(crossing.layer))) {
            take(crossingNote(section, crossing, model).level);
        }
    }
    if (!(high >= low)) {
        return std::nullopt;
    }
    return std::pair{low, high};
}

// ---- cut and fill ------------------------------------------------------------------------

std::optional<double> cutFillAt(const SectionSurface& design, const SectionSurface& ground,
                                double station)
{
    const auto d = levelAt(design, station);
    const auto g = levelAt(ground, station);
    if (!d || !g) {
        return std::nullopt;
    }
    return *d - *g;
}

std::string cutFillText(double designLessGround)
{
    if (std::abs(designLessGround) < 0.0005) {
        return "0.000";
    }
    return std::format("{:+.3f}", designLessGround);
}

std::vector<EarthworkRegion> earthworkRegions(const SectionSurface& design,
                                              const SectionSurface& ground, double from, double to)
{
    std::vector<EarthworkRegion> regions;
    if (design.samples.empty() || ground.samples.empty()) {
        return regions;
    }
    // Where both series are, within the range asked for.
    const double first =
        std::max({from, design.samples.front().station, ground.samples.front().station});
    const double last = std::min({to, design.samples.back().station, ground.samples.back().station});
    if (!(last > first)) {
        return regions;
    }
    // Every station either series has a sample at: between two of them both
    // are straight, so where they cross is found exactly.
    std::vector<double> stations{first, last};
    for (const SectionSurface* series : {&design, &ground}) {
        for (const SectionSample& sample : series->samples) {
            if (sample.station > first && sample.station < last) {
                stations.push_back(sample.station);
            }
        }
    }
    std::sort(stations.begin(), stations.end());
    stations.erase(std::unique(stations.begin(), stations.end()), stations.end());

    struct Open {
        Earthwork kind = Earthwork::Cut;
        std::vector<Point2> design;
        std::vector<Point2> ground;
    };
    std::optional<Open> open;
    const auto close = [&] {
        if (open && open->design.size() >= 2) {
            EarthworkRegion region;
            region.kind = open->kind;
            region.outline = open->design;
            // Back along the ground, leaving out a point where the two meet:
            // it is the design's already.
            for (auto it = open->ground.rbegin(); it != open->ground.rend(); ++it) {
                const bool meetsAtEnd = it == open->ground.rbegin() && *it == open->design.back();
                const bool meetsAtStart =
                    std::next(it) == open->ground.rend() && *it == open->design.front();
                if (!meetsAtEnd && !meetsAtStart) {
                    region.outline.push_back(*it);
                }
            }
            regions.push_back(std::move(region));
        }
        open.reset();
    };
    struct End {
        double station;
        double design;
        double ground;
    };
    const auto piece = [&](const End& a, const End& b) {
        const double sum = (a.design - a.ground) + (b.design - b.ground);
        if (sum == 0.0) {
            close(); // the two coincide: nothing to shade
            return;
        }
        const Earthwork kind = sum > 0.0 ? Earthwork::Fill : Earthwork::Cut;
        if (open && open->kind == kind) {
            open->design.emplace_back(b.station, b.design);
            open->ground.emplace_back(b.station, b.ground);
            return;
        }
        close();
        open = Open{kind,
                    {Point2(a.station, a.design), Point2(b.station, b.design)},
                    {Point2(a.station, a.ground), Point2(b.station, b.ground)}};
    };
    for (std::size_t i = 0; i + 1 < stations.size(); ++i) {
        const double s0 = stations[i];
        const double s1 = stations[i + 1];
        const auto d0 = levelAt(design, s0);
        const auto g0 = levelAt(ground, s0);
        const auto d1 = levelAt(design, s1);
        const auto g1 = levelAt(ground, s1);
        if (!d0 || !g0 || !d1 || !g1) {
            close(); // a gap in either: the region ends at it
            continue;
        }
        const End a{s0, *d0, *g0};
        const End b{s1, *d1, *g1};
        const double diffA = a.design - a.ground;
        const double diffB = b.design - b.ground;
        if (diffA * diffB < 0.0) {
            // They cross between the two: split there, the level the same.
            const double t = diffA / (diffA - diffB);
            const double station = s0 + t * (s1 - s0);
            const double level = a.design + t * (b.design - a.design);
            const End cross{station, level, level};
            piece(a, cross);
            piece(cross, b);
        } else {
            piece(a, b);
        }
    }
    close();
    return regions;
}

double outlineArea(std::span<const Point2> outline)
{
    if (outline.size() < 3) {
        return 0.0;
    }
    // Relative to the first point, so a level of 1000 m costs no precision.
    const Point2 origin = outline.front();
    double twice = 0.0;
    for (std::size_t i = 1; i + 1 < outline.size(); ++i) {
        const Point2 a = outline[i] - origin;
        const Point2 b = outline[i + 1] - origin;
        twice += a.x * b.y - b.x * a.y;
    }
    return std::abs(twice) / 2.0;
}

// ---- crossings ------------------------------------------------------------------------------

std::optional<double> ownLevel(const entity::Model& model, const SectionCrossing& crossing)
{
    const entity::Entity* entity = model.entities.find(crossing.entity);
    if (entity == nullptr) {
        return std::nullopt;
    }
    const entity::PropertyMap& properties = entity->properties;
    return std::visit(
        [&](const auto& shape) -> std::optional<double> {
            using Shape = std::decay_t<decltype(shape)>;
            if constexpr (std::is_same_v<Shape, geometry::Segment2>) {
                const auto heights = entity::heightsOf(properties, 2);
                return between(heights[0], heights[1], shape.parameterOf(crossing.plan));
            } else if constexpr (std::is_same_v<Shape, geometry::Polyline2>) {
                const std::size_t count = shape.vertices.size();
                if (shape.segmentCount() == 0) {
                    return std::nullopt;
                }
                // The piece it crosses: the one nearest the crossing point.
                std::size_t nearest = 0;
                double nearestDistance = std::numeric_limits<double>::infinity();
                for (std::size_t i = 0; i < shape.segmentCount(); ++i) {
                    const double distance = shape.segment(i).distanceTo(crossing.plan);
                    if (distance < nearestDistance) {
                        nearest = i;
                        nearestDistance = distance;
                    }
                }
                const auto heights = entity::heightsOf(properties, count);
                return between(heights[nearest], heights[(nearest + 1) % count],
                               shape.segment(nearest).parameterOf(crossing.plan));
            } else if constexpr (std::is_same_v<Shape, geometry::Arc2>) {
                const auto heights = entity::heightsOf(properties, 2);
                const geometry::Vec2 radial = crossing.plan - shape.center;
                return between(heights[0], heights[1],
                               shape.parameterOfAngle(std::atan2(radial.y, radial.x)));
            } else if constexpr (std::is_same_v<Shape, geometry::Circle2>) {
                return entity::heightsOf(properties, 1).front();
            } else {
                return std::nullopt;
            }
        },
        entity->geometry);
}

CrossingNote crossingNote(const Section& section, const SectionCrossing& crossing,
                          const entity::Model* model)
{
    CrossingNote note;
    note.shortText = upper(crossing.layer);
    if (model != nullptr) {
        if (const auto own = ownLevel(*model, crossing)) {
            note.level = own;
            note.ownLevel = true;
            if (const SectionSurface* ground = groundSeries(section)) {
                if (const auto surface = levelAt(*ground, crossing.station)) {
                    note.depth = *surface - *own;
                }
            }
        }
    }
    if (!note.ownLevel) {
        note.level = crossing.elevation;
    }
    note.text = note.shortText;
    if (note.level) {
        note.text += " RL " + twoDecimals(*note.level);
    }
    if (note.depth) {
        // Below the ground a depth; above it (an overhead line) a height.
        note.text += (*note.depth >= 0.0 ? " D " : " H ") + twoDecimals(std::abs(*note.depth));
    }
    return note;
}

// ---- placing labels ---------------------------------------------------------------------------

std::vector<std::optional<std::size_t>> placeLabels(std::span<const LabelCandidate> labels,
                                                    const Box2& bounds,
                                                    std::span<const Box2> obstacles, double gapMm)
{
    std::vector<std::optional<std::size_t>> chosen(labels.size());
    std::vector<std::size_t> order(labels.size());
    std::iota(order.begin(), order.end(), std::size_t{0});
    std::stable_sort(order.begin(), order.end(), [&labels](std::size_t a, std::size_t b) {
        return labels[a].priority < labels[b].priority;
    });
    std::vector<Box2> taken(obstacles.begin(), obstacles.end());
    for (const std::size_t index : order) {
        const std::vector<Box2>& boxes = labels[index].boxes;
        for (std::size_t k = 0; k < boxes.size(); ++k) {
            const Box2& box = boxes[k];
            if (!inside(bounds, box)) {
                continue;
            }
            const bool clear = std::none_of(taken.begin(), taken.end(), [&](const Box2& other) {
                return crowds(box, other, gapMm);
            });
            if (clear) {
                chosen[index] = k;
                taken.push_back(box);
                break;
            }
        }
    }
    return chosen;
}

} // namespace katana::cad::plotting
