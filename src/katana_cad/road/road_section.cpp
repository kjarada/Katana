#include "katana/cad/road/road_section.hpp"

#include <algorithm>
#include <cmath>
#include <functional>
#include <map>
#include <string>
#include <utility>

#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"
#include "katana/math/summation.hpp"
#include "katana/terrain/tin_builder.hpp"

namespace katana::cad::road {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::geometry::SolvedAlignment;
using katana::geometry::SolvedProfile;
using katana::geometry::TriangleMesh;

namespace {

// More sections than this is refused before one is made: a millimetre
// interval typed for a metre one, on a long road, would otherwise take the
// session with it. ALIGN STATIONS draws the same line (docs/cad.md).
constexpr std::size_t kMaximumSections = 100000;

// The daylight march: half a metre resolves any batter a machine can build,
// and forty bisections put the crossing at 5e-13 m (docs/cad.md, "Corridor
// quantities"). The ground is sampled at the same spacing across a section.
constexpr double kStep = 0.5;
constexpr int kBisections = 40;

std::string number(double value)
{
    return katana::core::formatExactReal(value);
}

// A point of one side, by its distance OUT from the centreline.
struct OutPoint {
    std::string code;
    double out = 0.0;
    double level = 0.0;
};

struct OutSolid {
    std::string key;
    std::size_t component = 0;
    std::optional<std::size_t> course;
    std::vector<ShapeColumn> columns; // `out` from the centreline, absolute levels
};

struct SideBuild {
    std::vector<OutPoint> top;       // from the centreline outward
    std::vector<OutPoint> formation; // from out 0 outward
    std::vector<OutSolid> solids;
    bool reached = false;
};

// Appends a component's first formation point where the line already ends
// at that distance out: one point when the levels agree, else the
// shallower corner stepped kFaceRun into its own component (room: a quarter
// of each component's width, for a component narrower than the step).
void joinFormation(std::vector<OutPoint>& line, OutPoint point, double previousRoom,
                   double currentRoom)
{
    if (line.empty()) {
        line.push_back(std::move(point));
        return;
    }
    OutPoint& last = line.back();
    if (std::abs(last.out - point.out) > tol::kGeometric ||
        std::abs(last.level - point.level) <= tol::kCoordinate) {
        if (std::abs(last.out - point.out) > tol::kGeometric) {
            line.push_back(std::move(point));
        }
        return;
    }
    const double step = std::min({kFaceRun, previousRoom / 4.0, currentRoom / 4.0});
    if (last.level > point.level) {
        last.out -= step; // the previous component's corner is the shallower
    } else {
        point.out += step;
    }
    line.push_back(std::move(point));
}

// The batter's rise (or fall) `d` out from the hinge: 1 in `run`, with a
// flat of benchWidth after every benchHeight.
double batterRise(const Component& batter, bool cut, double d)
{
    const double run = cut ? batter.cutSlope : batter.fillSlope;
    if (!(batter.benchHeight > 0.0) || !(batter.benchWidth > 0.0)) {
        return d / run;
    }
    const double slope = batter.benchHeight * run;
    const double period = slope + batter.benchWidth;
    const double cycles = std::floor(d / period);
    const double within = d - cycles * period;
    return cycles * batter.benchHeight + std::min(within, slope) / run;
}

using GroundAt = std::function<std::optional<double>(double signedOffset)>;

SideBuild buildSide(const RoadTemplate& roadTemplate, Side side, double designLevel,
                    const std::optional<Crossfalls>& superelevation, const GroundAt& groundAt,
                    bool haveGround)
{
    const double sign = side == Side::Left ? 1.0 : -1.0;
    const std::string sideWord(toString(side));
    SideBuild out;
    out.top.push_back(OutPoint{"CL", 0.0, designLevel});
    double atOut = 0.0;
    double atLevel = designLevel;
    double previousWidth = 0.0;
    const Component* batter = nullptr;
    std::string batterPrefix;

    for (std::size_t index = 0; index < roadTemplate.components.size(); ++index) {
        const Component& component = roadTemplate.components[index];
        if (component.side != side) {
            continue;
        }
        const std::string prefix = sideWord + "/" + component.name;
        if (component.kind == ComponentKind::Batter) {
            batter = &component;
            batterPrefix = prefix;
            continue;
        }
        // The component's top, a column at each change of it, with its
        // bottom; and the names of its points after the first, which is
        // where the component before it ended.
        std::vector<ShapeColumn> surface;
        std::vector<std::string> codes;
        if (component.kind == ComponentKind::Kerb) {
            for (const ShapeColumn& column : kerbShape(component)) {
                surface.push_back(
                    ShapeColumn{atOut + column.out, atLevel + column.top, atLevel + column.bottom});
            }
            // The points after the first, which is where the previous component
            // ended. A kerb facing outward is the other way round: its top
            // first (the face's crest), then down the face to the lip.
            const bool outward = component.faces == KerbFacing::Outward;
            switch (component.shape) {
            case KerbShape::Barrier:
            case KerbShape::Mountable:
                codes = outward ? std::vector<std::string>{"face", "lip"}
                                : std::vector<std::string>{"face", "back"};
                break;
            case KerbShape::KerbAndGutter:
                codes = outward ? std::vector<std::string>{"face", "invert", "lip"}
                                : std::vector<std::string>{"invert", "face", "back"};
                break;
            case KerbShape::EdgeStrip:
                codes = {"outer"};
                break;
            }
            out.solids.push_back(OutSolid{prefix, index, std::nullopt, surface});
        } else {
            if (component.kind == ComponentKind::Drain) {
                surface = {ShapeColumn{atOut, atLevel, atLevel},
                           ShapeColumn{atOut + component.width / 2.0, atLevel - component.depth,
                                       atLevel - component.depth},
                           ShapeColumn{atOut + component.width,
                                       atLevel + component.width * component.crossfall / 100.0,
                                       0.0}};
                surface.back().bottom = surface.back().top;
                codes = {"invert", "outer"};
            } else {
                double crossfall = component.crossfall;
                if (component.superelevated && superelevation) {
                    crossfall = side == Side::Left ? superelevation->left : superelevation->right;
                }
                const double outer = atLevel + component.width * crossfall / 100.0;
                surface = {ShapeColumn{atOut, atLevel, atLevel},
                           ShapeColumn{atOut + component.width, outer, outer}};
                codes = {"outer"};
            }
            double depth = 0.0;
            for (std::size_t c = 0; c < component.courses.size(); ++c) {
                const PavementCourse& course = component.courses[c];
                OutSolid solid{prefix + "/" + course.name, index, c, {}};
                for (const ShapeColumn& column : surface) {
                    solid.columns.push_back(ShapeColumn{column.out, column.top - depth,
                                                        column.top - depth - course.thickness});
                }
                depth += course.thickness;
                out.solids.push_back(std::move(solid));
            }
            for (ShapeColumn& column : surface) {
                column.bottom = column.top - depth;
            }
        }

        for (std::size_t j = 1; j < surface.size(); ++j) {
            out.top.push_back(
                OutPoint{prefix + "/" + codes[j - 1], surface[j].out, surface[j].top});
        }
        const double width = surface.back().out - surface.front().out;
        for (std::size_t j = 0; j < surface.size(); ++j) {
            OutPoint point{prefix + "/f" + std::to_string(j), surface[j].out, surface[j].bottom};
            if (j == 0) {
                joinFormation(out.formation, std::move(point), previousWidth, width);
            } else {
                out.formation.push_back(std::move(point));
            }
        }
        previousWidth = width;
        atOut = surface.back().out;
        atLevel = surface.back().top;
    }

    if (batter == nullptr) {
        // Nothing to carry to the ground: the side ends at its edge.
        out.reached = true;
        return out;
    }
    const OutPoint hinge = out.top.back();
    // The formation comes up to the hinge, where the batter starts: the
    // courses' outer bottom corner stepped kFaceRun back into its component.
    if (!out.formation.empty()) {
        OutPoint& corner = out.formation.back();
        if (std::abs(corner.out - hinge.out) <= tol::kGeometric &&
            std::abs(corner.level - hinge.level) > tol::kCoordinate) {
            corner.out -= std::min(kFaceRun, previousWidth / 4.0);
            out.formation.push_back(hinge);
        }
    } else {
        // A side of a batter alone (a ramp's edge, an embankment of nothing
        // but its slopes): nothing has put a point on the formation, and the
        // batter would start at its first bench or its daylight, so the line
        // would run straight from there to the other side's first point and
        // lose the triangle under the batter. The hinge - here the centreline
        // at the design level - is where it starts. buildSection joins it
        // with the other side's first point at the centreline.
        out.formation.push_back(OutPoint{batterPrefix + "/hinge", hinge.out, hinge.level});
    }
    if (!haveGround) {
        return out;
    }
    const auto groundAtHinge = groundAt(sign * hinge.out);
    if (!groundAtHinge) {
        return out;
    }
    const auto addPoint = [&out](OutPoint point) {
        out.top.push_back(point);
        out.formation.push_back(std::move(point));
    };
    const double difference = *groundAtHinge - hinge.level;
    if (std::abs(difference) <= tol::kCoordinate) {
        addPoint(OutPoint{batterPrefix + "/daylight", hinge.out, hinge.level}); // at grade
        out.reached = true;
        return out;
    }
    // In cut the batter climbs from the hinge to the ground; in fill it
    // falls to it. "Crossed": the batter has passed the ground.
    const bool cut = difference > 0.0;
    const double towards = cut ? 1.0 : -1.0;
    const auto levelAt = [&](double d) {
        return hinge.level + towards * batterRise(*batter, cut, d);
    };
    const auto crossed = [&](double d) -> std::optional<bool> {
        const auto z = groundAt(sign * (hinge.out + d));
        if (!z) {
            return std::nullopt;
        }
        const double level = levelAt(d);
        return cut ? level >= *z : level <= *z;
    };
    std::optional<double> reach;
    double previous = 0.0;
    // Half-metre steps, each counted from the hinge (k x kStep: a sum of steps
    // drifts), and the last one the limit itself: a limit that is not a
    // multiple of a step, or is under one step, is tried at the limit, not
    // given up on short of it.
    for (std::size_t k = 1;; ++k) {
        const double d = std::min(static_cast<double>(k) * kStep, batter->maximumWidth);
        const auto at = crossed(d);
        if (!at) {
            return out;
        }
        if (*at) {
            double low = previous;
            double high = d;
            for (int i = 0; i < kBisections; ++i) {
                const double mid = 0.5 * (low + high);
                const auto there = crossed(mid);
                if (!there) {
                    return out;
                }
                (*there ? high : low) = mid;
            }
            reach = 0.5 * (low + high);
            break;
        }
        previous = d;
        if (d >= batter->maximumWidth) {
            break;
        }
    }
    if (!reach) {
        return out;
    }
    if (batter->benchHeight > 0.0 && batter->benchWidth > 0.0) {
        const double run = cut ? batter->cutSlope : batter->fillSlope;
        const double slope = batter->benchHeight * run;
        // checkTemplate refuses a batter with more than kMaximumBenches; the
        // bound here is the loop's own, so no template can make it long.
        std::size_t bench = 1;
        for (double d = slope; d < *reach && bench <= kMaximumBenches;
             d += slope + batter->benchWidth, ++bench) {
            addPoint(OutPoint{batterPrefix + "/bench" + std::to_string(bench) + "-in",
                              hinge.out + d, levelAt(d)});
            const double benchEnd = d + batter->benchWidth;
            if (benchEnd >= *reach) {
                break;
            }
            addPoint(OutPoint{batterPrefix + "/bench" + std::to_string(bench) + "-out",
                              hinge.out + benchEnd, levelAt(benchEnd)});
        }
    }
    addPoint(OutPoint{batterPrefix + "/daylight", hinge.out + *reach, levelAt(*reach)});
    out.reached = true;
    return out;
}

// The part of a line, ordered by offset either way, inside [lo, hi], with
// points made where it crosses either end.
std::vector<SectionVertex> clipped(const std::vector<SectionVertex>& line, double lo, double hi)
{
    std::vector<SectionVertex> out;
    const auto add = [&out](double offset, double elevation) {
        if (out.empty() || std::abs(out.back().offset - offset) > tol::kAbsolute) {
            out.push_back(SectionVertex{offset, elevation});
        }
    };
    for (std::size_t i = 0; i + 1 < line.size(); ++i) {
        const SectionVertex& a = line[i];
        const SectionVertex& b = line[i + 1];
        const double from = std::min(a.offset, b.offset);
        const double to = std::max(a.offset, b.offset);
        const double s = std::max(from, lo);
        const double e = std::min(to, hi);
        if (s > e) {
            continue;
        }
        const auto at = [&](double offset) {
            if (to - from <= tol::kAbsolute) {
                return a.elevation;
            }
            return a.elevation +
                   (b.elevation - a.elevation) * (offset - a.offset) / (b.offset - a.offset);
        };
        // In the line's own direction.
        if (a.offset <= b.offset) {
            add(s, at(s));
            add(e, at(e));
        } else {
            add(e, at(e));
            add(s, at(s));
        }
    }
    return out;
}

// A line's level at an offset, the ends held beyond its span.
double levelOn(const std::vector<SectionVertex>& line, double offset)
{
    if (line.empty()) {
        return 0.0;
    }
    double best = line.front().elevation;
    double nearest = std::abs(line.front().offset - offset);
    for (std::size_t i = 0; i + 1 < line.size(); ++i) {
        const SectionVertex& a = line[i];
        const SectionVertex& b = line[i + 1];
        const double from = std::min(a.offset, b.offset);
        const double to = std::max(a.offset, b.offset);
        if (offset >= from && offset <= to) {
            if (to - from <= tol::kAbsolute) {
                return a.elevation;
            }
            return a.elevation +
                   (b.elevation - a.elevation) * (offset - a.offset) / (b.offset - a.offset);
        }
    }
    for (const SectionVertex& vertex : line) {
        const double distance = std::abs(vertex.offset - offset);
        if (distance < nearest) {
            nearest = distance;
            best = vertex.elevation;
        }
    }
    return best;
}

std::vector<SectionVertex> vertices(const std::vector<SectionPoint>& points)
{
    std::vector<SectionVertex> out;
    out.reserve(points.size());
    for (const SectionPoint& point : points) {
        out.push_back(SectionVertex{point.offset, point.level});
    }
    return out;
}

// The stretches of one side - distances OUT from the centreline, ascending -
// where the ground is above the formation (`cutting`) or below it, thick
// somewhere by more than kCoordinate. `formationLevels[i]` is the formation's
// level under `ground[i]`. The ground samples include every vertex of the
// formation, so between two samples both lines are straight and their
// difference crosses zero at one point, found exactly. A stretch whose
// thickest point is under kCoordinate (0.1 mm, the TIN's duplicate tolerance)
// is not earthworks: the daylight is bisected to about 1e-13 m, and lands on
// either side of the ground by that much.
std::vector<OutSpan> earthworkSpans(const std::vector<SectionVertex>& ground,
                                    const std::vector<double>& formationLevels, Side side,
                                    bool cutting)
{
    struct Sample {
        double out;
        double thickness;
    };
    const double sign = side == Side::Left ? 1.0 : -1.0;
    std::vector<Sample> samples;
    for (std::size_t i = 0; i < ground.size() && i < formationLevels.size(); ++i) {
        const double out = sign * ground[i].offset;
        if (out < -tol::kAbsolute) {
            continue; // the other side of the centreline
        }
        const double thickness = cutting ? ground[i].elevation - formationLevels[i]
                                         : formationLevels[i] - ground[i].elevation;
        samples.push_back(Sample{std::max(out, 0.0), thickness});
    }
    std::sort(samples.begin(), samples.end(),
              [](const Sample& a, const Sample& b) { return a.out < b.out; });
    // Where the thickness is zero between two samples of opposite sign (or
    // one of them zero).
    const auto crossing = [](const Sample& a, const Sample& b) {
        return a.out + (b.out - a.out) * (a.thickness / (a.thickness - b.thickness));
    };
    std::vector<OutSpan> spans;
    std::optional<double> start;
    double peak = 0.0;
    const auto close = [&](double end) {
        if (start && peak > tol::kCoordinate && end > *start) {
            spans.push_back(OutSpan{*start, end});
        }
        start.reset();
    };
    for (std::size_t i = 0; i < samples.size(); ++i) {
        if (samples[i].thickness > 0.0) {
            if (!start) {
                start = i == 0 ? samples[i].out : crossing(samples[i - 1], samples[i]);
                peak = 0.0;
            }
            peak = std::max(peak, samples[i].thickness);
        } else if (start) {
            close(crossing(samples[i - 1], samples[i]));
        }
    }
    if (start) {
        close(samples.back().out);
    }
    return spans;
}

Result<RoadSection> buildSection(const RoadTemplate& roadTemplate, const SolvedAlignment& alignment,
                                 const SolvedProfile& profile,
                                 const katana::terrain::TinSurface* ground, double station,
                                 const std::optional<Crossfalls>& superelevation)
{
    const auto centre = alignment.pointAtStation(station);
    const auto direction = alignment.directionAtStation(station);
    const auto elevation = profile.elevationAt(station);
    if (!centre || !direction) {
        return makeError(ErrorCode::InvalidArgument, "the station is outside the alignment",
                         number(station));
    }
    if (!elevation) {
        return makeError(ErrorCode::InvalidArgument, "the station is outside the design profile",
                         number(station));
    }
    RoadSection section;
    section.station = station;
    section.centre = *centre;
    section.direction = *direction;
    section.designLevel = *elevation;

    const double sine = std::sin(*direction);
    const double cosine = std::cos(*direction);
    const GroundAt groundAt = [&](double offset) -> std::optional<double> {
        if (ground == nullptr) {
            return std::nullopt;
        }
        return ground->elevationAt(Point2(centre->x - sine * offset, centre->y + cosine * offset));
    };
    const SideBuild left = buildSide(roadTemplate, Side::Left, *elevation, superelevation, groundAt,
                                     ground != nullptr);
    const SideBuild right = buildSide(roadTemplate, Side::Right, *elevation, superelevation,
                                      groundAt, ground != nullptr);
    section.leftReached = left.reached;
    section.rightReached = right.reached;

    // Left outermost to right outermost; the centreline once.
    for (auto it = left.top.rbegin(); it != left.top.rend(); ++it) {
        section.top.push_back(SectionPoint{it->code, it->out, it->level});
    }
    for (std::size_t i = 1; i < right.top.size(); ++i) {
        section.top.push_back(
            SectionPoint{right.top[i].code, -right.top[i].out, right.top[i].level});
    }
    // The two formations meet at the centreline, stepped there as between
    // two components when their first courses differ in depth.
    std::vector<OutPoint> leftFormation = left.formation;
    std::vector<OutPoint> rightFormation = right.formation;
    if (!leftFormation.empty() && !rightFormation.empty() &&
        std::abs(leftFormation.front().out) <= tol::kGeometric &&
        std::abs(rightFormation.front().out) <= tol::kGeometric) {
        const double leftRoom =
            leftFormation.size() > 1 ? leftFormation[1].out - leftFormation[0].out : kFaceRun;
        const double rightRoom =
            rightFormation.size() > 1 ? rightFormation[1].out - rightFormation[0].out : kFaceRun;
        const double difference = leftFormation.front().level - rightFormation.front().level;
        if (std::abs(difference) <= tol::kCoordinate) {
            rightFormation.erase(rightFormation.begin());
        } else if (difference > 0.0) {
            leftFormation.front().out += std::min(kFaceRun, leftRoom / 4.0);
        } else {
            rightFormation.front().out += std::min(kFaceRun, rightRoom / 4.0);
        }
    }
    for (auto it = leftFormation.rbegin(); it != leftFormation.rend(); ++it) {
        section.formation.push_back(SectionPoint{it->code, it->out, it->level});
    }
    for (const OutPoint& point : rightFormation) {
        section.formation.push_back(SectionPoint{point.code, -point.out, point.level});
    }
    for (const SideBuild* built : {&left, &right}) {
        const double sign = built == &left ? 1.0 : -1.0;
        for (const OutSolid& solid : built->solids) {
            SectionSolid made{solid.key, solid.component, solid.course, {}};
            for (const ShapeColumn& column : solid.columns) {
                made.columns.push_back(SolidColumn{sign * column.out, column.top, column.bottom});
            }
            section.solids.push_back(std::move(made));
        }
    }

    if (ground == nullptr || !left.reached || !right.reached || section.top.size() < 2) {
        return section;
    }
    // The ground under the whole span, at every vertex of both lines and
    // every half metre between, so an undulation between two vertices is
    // not straightened out.
    std::vector<double> offsets;
    for (const SectionPoint& point : section.top) {
        offsets.push_back(point.offset);
    }
    for (const SectionPoint& point : section.formation) {
        offsets.push_back(point.offset);
    }
    const double leftmost = section.top.front().offset;
    const double rightmost = section.top.back().offset;
    for (double o = rightmost; o < leftmost; o += kStep) {
        offsets.push_back(o);
    }
    std::sort(offsets.begin(), offsets.end(), std::greater<>()); // left to right
    offsets.erase(std::unique(offsets.begin(), offsets.end(),
                              [](double a, double b) { return std::abs(a - b) < 1e-9; }),
                  offsets.end());
    std::vector<SectionVertex> sampled;
    sampled.reserve(offsets.size());
    for (const double offset : offsets) {
        const auto z = groundAt(offset);
        if (!z) {
            return section; // ground missing under the section: not complete
        }
        sampled.push_back(SectionVertex{offset, *z});
    }
    section.ground = std::move(sampled);
    section.complete = true;
    const std::vector<SectionVertex> formation = vertices(section.formation);
    const SplitArea whole = areaBetween(section.ground, formation);
    section.cutArea = whole.above;  // ground above the formation
    section.fillArea = whole.below; // the formation above the ground
    const double beyond = std::max(std::abs(leftmost), std::abs(rightmost)) + 1.0;
    const SplitArea leftSplit =
        areaBetween(clipped(section.ground, 0.0, beyond), clipped(formation, 0.0, beyond));
    const SplitArea rightSplit =
        areaBetween(clipped(section.ground, -beyond, 0.0), clipped(formation, -beyond, 0.0));
    section.leftCut = leftSplit.above;
    section.leftFill = leftSplit.below;
    section.rightCut = rightSplit.above;
    section.rightFill = rightSplit.below;
    // Where the earthworks have thickness; a side with none has no cut (or no
    // fill) whatever sliver of area the daylight's last 1e-13 m left. This is
    // the one place that decides it: the end areas, the quantities and the
    // objects all follow.
    std::vector<double> formationLevels;
    formationLevels.reserve(section.ground.size());
    for (const SectionVertex& sample : section.ground) {
        formationLevels.push_back(levelOn(formation, sample.offset));
    }
    section.leftCutSpans = earthworkSpans(section.ground, formationLevels, Side::Left, true);
    section.leftFillSpans = earthworkSpans(section.ground, formationLevels, Side::Left, false);
    section.rightCutSpans = earthworkSpans(section.ground, formationLevels, Side::Right, true);
    section.rightFillSpans = earthworkSpans(section.ground, formationLevels, Side::Right, false);
    bool trimmed = false;
    const auto settle = [&trimmed](const std::vector<OutSpan>& spans, double& area) {
        if (spans.empty() && area != 0.0) {
            area = 0.0;
            trimmed = true;
        }
    };
    settle(section.leftCutSpans, section.leftCut);
    settle(section.leftFillSpans, section.leftFill);
    settle(section.rightCutSpans, section.rightCut);
    settle(section.rightFillSpans, section.rightFill);
    if (trimmed) {
        section.cutArea = section.leftCut + section.rightCut;
        section.fillArea = section.leftFill + section.rightFill;
    }
    return section;
}

std::string_view partOf(ComponentKind kind)
{
    switch (kind) {
    case ComponentKind::Lane:
        return "TRAFFICLANE";
    case ComponentKind::Shoulder:
        return "SHOULDER";
    case ComponentKind::Footpath:
        return "SIDEWALK";
    case ComponentKind::Median:
        return "CENTRALRESERVE";
    case ComponentKind::Kerb:
    case ComponentKind::Verge:
    case ComponentKind::Drain:
    case ComponentKind::Batter:
        return "ROADSIDE";
    }
    return "ROADSIDE";
}

std::string_view objectKindOf(ComponentKind kind)
{
    switch (kind) {
    case ComponentKind::Lane:
    case ComponentKind::Shoulder:
        return "pavement";
    case ComponentKind::Footpath:
        return "footpath";
    case ComponentKind::Verge:
        return "verge";
    case ComponentKind::Median:
        return "median";
    case ComponentKind::Drain:
        return "drain";
    case ComponentKind::Kerb:
        return "kerb";
    case ComponentKind::Batter:
        return "batter";
    }
    return "pavement";
}

// The outline in plan of a run of columns: the first column's line out,
// the last one's back.
katana::geometry::Polyline2 outlineOf(const SolvedAlignment& alignment,
                                      const std::vector<double>& stations,
                                      const std::vector<std::vector<SolidColumn>>& columns)
{
    katana::geometry::Polyline2 outline;
    outline.closed = true;
    for (std::size_t k = 0; k < stations.size(); ++k) {
        if (const auto p = alignment.pointAtStationOffset(stations[k], columns[k].front().offset)) {
            outline.vertices.push_back(*p);
        }
    }
    for (std::size_t k = stations.size(); k-- > 0;) {
        if (const auto p = alignment.pointAtStationOffset(stations[k], columns[k].back().offset)) {
            outline.vertices.push_back(*p);
        }
    }
    return outline;
}

double planLength(const SolvedAlignment& alignment, const std::vector<double>& stations,
                  const std::vector<std::vector<SolidColumn>>& columns)
{
    katana::math::CompensatedSum length;
    std::optional<Point2> previous;
    for (std::size_t k = 0; k < stations.size(); ++k) {
        const auto p = alignment.pointAtStationOffset(stations[k], columns[k].front().offset);
        if (!p) {
            continue;
        }
        if (previous) {
            length.add(previous->distanceTo(*p));
        }
        previous = p;
    }
    return length.value();
}

bim::Quantity quantity(std::string name, double value, std::string unit)
{
    return bim::Quantity{std::move(name), value, std::move(unit)};
}

} // namespace

// ---- one section --------------------------------------------------------------------------

Result<RoadSection> roadSection(const RoadTemplate& roadTemplate, const SolvedAlignment& alignment,
                                const SolvedProfile& profile,
                                const katana::terrain::TinSurface* ground, double station,
                                std::optional<Crossfalls> superelevation)
{
    if (auto status = checkTemplate(roadTemplate); !status) {
        return status.error();
    }
    return buildSection(roadTemplate, alignment, profile, ground, station, superelevation);
}

Result<RoadSection> templateSection(const RoadTemplate& roadTemplate)
{
    katana::geometry::HorizontalAlignment straight;
    straight.pis = {katana::geometry::AlignmentPI{Point2(0.0, 0.0)},
                    katana::geometry::AlignmentPI{Point2(100.0, 0.0)}};
    auto alignment = katana::geometry::solveAlignment(straight);
    katana::geometry::VerticalAlignment level;
    level.pvis = {katana::geometry::ProfilePVI{0.0, 0.0}, katana::geometry::ProfilePVI{100.0, 0.0}};
    auto profile = katana::geometry::solveProfile(level);
    if (!alignment || !profile) {
        return makeError(ErrorCode::Internal, "the straight a template is shown on did not solve");
    }
    return roadSection(roadTemplate, *alignment, *profile, nullptr, 50.0);
}

std::vector<double> sectionStations(double start, double end, double interval,
                                    const std::vector<double>& keys)
{
    std::vector<double> stations;
    if (!(interval > 0.0) || !std::isfinite(interval) || !(end >= start)) {
        return stations;
    }
    for (std::size_t k = 0;; ++k) {
        const double station = start + static_cast<double>(k) * interval;
        if (!(station < end)) {
            break;
        }
        stations.push_back(station);
    }
    stations.push_back(end);
    for (const double key : keys) {
        if (key > start && key < end) {
            stations.push_back(key);
        }
    }
    std::sort(stations.begin(), stations.end());
    stations.erase(
        std::unique(stations.begin(), stations.end(),
                    [](double a, double b) { return std::abs(a - b) <= tol::kGeometric; }),
        stations.end());
    return stations;
}

// ---- a corridor -------------------------------------------------------------------------

BuiltCorridor buildCorridor(const Corridor& corridor, const RoadTemplateSet& templates,
                            const SolvedAlignment& alignment, const SolvedProfile& profile,
                            const katana::terrain::TinSurface* ground)
{
    BuiltCorridor built;
    built.name = corridor.name;
    built.surface = corridor.surface;
    built.alignment = alignment;
    if (auto status = checkCorridor(corridor); !status) {
        built.problems.push_back(status.error().message);
        return built;
    }
    const double low = std::max(alignment.startStation(), profile.startStation());
    const double high = std::min(alignment.endStation(), profile.endStation());
    if (!(high - low > tol::kCoordinate)) {
        built.problems.push_back("the alignment " + corridor.alignment +
                                 " and its design profile do not overlap in station");
        return built;
    }
    double start = corridor.start.value_or(low);
    double end = corridor.end.value_or(high);
    if (start < low) {
        built.problems.push_back("the corridor starts at " + number(start) +
                                 ", before its alignment and profile begin: built from " +
                                 number(low));
        start = low;
    }
    if (end > high) {
        built.problems.push_back("the corridor ends at " + number(end) +
                                 ", after its alignment and profile end: built to " + number(high));
        end = high;
    }
    if (!(end - start > tol::kCoordinate)) {
        built.problems.push_back("the corridor's stations " + number(start) + " to " + number(end) +
                                 " are not on its alignment and profile");
        return built;
    }
    if ((end - start) / corridor.interval > static_cast<double>(kMaximumSections)) {
        built.problems.push_back("an interval of " + number(corridor.interval) + " over " +
                                 number(end - start) + " m is more than " +
                                 std::to_string(kMaximumSections) + " sections: not built");
        return built;
    }
    built.start = start;
    built.end = end;

    // Every key station: where the geometry, the grade, the superelevation
    // or the template changes character, which a schedule must not step over.
    std::vector<double> keys = alignment.keyStations();
    for (const double key : profile.keyStations()) {
        keys.push_back(key);
    }
    for (const SuperelevationPoint& point : corridor.superelevation) {
        keys.push_back(point.station);
    }
    for (const TemplateRegion& region : corridor.regions) {
        keys.push_back(region.start);
        keys.push_back(region.end);
    }
    const std::vector<double> stations = sectionStations(start, end, corridor.interval, keys);

    // The runs: the regions in station order, and the corridor's template
    // before, between and after them.
    struct Stretch {
        double from;
        double to;
        std::string templateName;
    };
    std::vector<Stretch> stretches;
    double cursor = start;
    for (const TemplateRegion& region : corridor.regions) {
        const double from = std::max(region.start, start);
        const double to = std::min(region.end, end);
        if (!(to - from > tol::kCoordinate)) {
            built.problems.push_back("the region " + number(region.start) + " to " +
                                     number(region.end) + " is outside the corridor's stations");
            continue;
        }
        if (from - cursor > tol::kCoordinate) {
            stretches.push_back(Stretch{cursor, from, corridor.templateName});
        }
        stretches.push_back(Stretch{from, to, region.templateName});
        cursor = to;
    }
    if (end - cursor > tol::kCoordinate) {
        stretches.push_back(Stretch{cursor, end, corridor.templateName});
    }

    bool batters = false;
    katana::math::CompensatedSum cut;
    katana::math::CompensatedSum fill;
    for (std::size_t n = 0; n < stretches.size(); ++n) {
        const Stretch& stretch = stretches[n];
        const RoadTemplate* roadTemplate = templates.find(stretch.templateName);
        if (roadTemplate == nullptr) {
            built.problems.push_back("the road template " + stretch.templateName + " (stations " +
                                     number(stretch.from) + " to " + number(stretch.to) +
                                     ") is not in the drawing: that stretch is not built");
            continue;
        }
        if (auto status = checkTemplate(*roadTemplate); !status) {
            built.problems.push_back(status.error().message);
            continue;
        }
        if (roadTemplate->components.empty()) {
            built.problems.push_back("the road template " + roadTemplate->name +
                                     " has no components: stations " + number(stretch.from) +
                                     " to " + number(stretch.to) + " have nothing to build");
        }
        batters = batters || std::ranges::any_of(roadTemplate->components, [](const Component& c) {
                      return c.kind == ComponentKind::Batter;
                  });
        CorridorRun run;
        run.region = n + 1;
        run.roadTemplate = *roadTemplate;
        run.start = stretch.from;
        run.end = stretch.to;
        for (const double station : stations) {
            if (station < stretch.from - tol::kGeometric ||
                station > stretch.to + tol::kGeometric) {
                continue;
            }
            const double at = std::clamp(station, stretch.from, stretch.to);
            auto section = buildSection(*roadTemplate, alignment, profile, ground, at,
                                        superelevationAt(corridor, at));
            if (!section) {
                built.problems.push_back(section.error().describe());
                continue;
            }
            if (!section->complete) {
                ++run.incompleteSections;
            }
            section->run = run.region;
            run.sections.push_back(std::move(*section));
        }
        // End areas between two complete sections; an interval touching an
        // incomplete one adds nothing, and the count says so.
        katana::math::CompensatedSum leftCut;
        katana::math::CompensatedSum leftFill;
        katana::math::CompensatedSum rightCut;
        katana::math::CompensatedSum rightFill;
        for (std::size_t k = 0; k + 1 < run.sections.size(); ++k) {
            const RoadSection& a = run.sections[k];
            const RoadSection& b = run.sections[k + 1];
            if (!a.complete || !b.complete) {
                continue;
            }
            const double length = b.station - a.station;
            leftCut.add(0.5 * (a.leftCut + b.leftCut) * length);
            leftFill.add(0.5 * (a.leftFill + b.leftFill) * length);
            rightCut.add(0.5 * (a.rightCut + b.rightCut) * length);
            rightFill.add(0.5 * (a.rightFill + b.rightFill) * length);
        }
        run.leftCut = leftCut.value();
        run.leftFill = leftFill.value();
        run.rightCut = rightCut.value();
        run.rightFill = rightFill.value();
        cut.add(run.leftCut);
        cut.add(run.rightCut);
        fill.add(run.leftFill);
        fill.add(run.rightFill);
        built.sections += run.sections.size();
        built.incompleteSections += run.incompleteSections;
        if (ground != nullptr && run.incompleteSections > 0) {
            built.problems.push_back(std::to_string(run.incompleteSections) + " of " +
                                     std::to_string(run.sections.size()) +
                                     " sections of stations " + number(run.start) + " to " +
                                     number(run.end) +
                                     " do not reach the ground: their earthworks are left out");
        }
        built.runs.push_back(std::move(run));
    }
    if (ground == nullptr && batters) {
        built.problems.push_back("no target surface: the batters and the earthworks are not built");
    }
    built.cut = cut.value();
    built.fill = fill.value();
    return built;
}

CorridorInputs corridorInputs(const Document& document, const Corridor& corridor)
{
    CorridorInputs inputs;
    const katana::entity::Alignment* alignment =
        document.model().alignments.find(corridor.alignment);
    if (alignment == nullptr) {
        inputs.problems.push_back("the alignment " + corridor.alignment + " is not in the drawing");
        return inputs;
    }
    if (!alignment->vertical) {
        inputs.problems.push_back("the alignment " + corridor.alignment +
                                  " has no design profile (ALIGN DESIGN)");
        return inputs;
    }
    auto solved = katana::geometry::solveAlignment(alignment->horizontal);
    if (!solved) {
        inputs.problems.push_back(solved.error().describe());
        return inputs;
    }
    auto profile = katana::geometry::solveProfile(*alignment->vertical);
    if (!profile) {
        inputs.problems.push_back(profile.error().describe());
        return inputs;
    }
    inputs.alignment = std::move(*solved);
    inputs.profile = std::move(*profile);
    if (!corridor.surface.empty()) {
        const katana::terrain::NamedSurface* named = document.surfaces().find(corridor.surface);
        if (named == nullptr || named->surface == nullptr) {
            inputs.problems.push_back("the surface " + corridor.surface + " is not in the drawing");
        } else {
            inputs.ground = named->surface.get();
        }
    }
    return inputs;
}

BuiltCorridor buildCorridor(const Document& document, const Corridor& corridor)
{
    const CorridorInputs inputs = corridorInputs(document, corridor);
    if (!inputs.alignment || !inputs.profile) {
        BuiltCorridor failed;
        failed.name = corridor.name;
        failed.problems = inputs.problems;
        return failed;
    }
    BuiltCorridor built = buildCorridor(corridor, document.roadTemplates(), *inputs.alignment,
                                        *inputs.profile, inputs.ground);
    built.problems.insert(built.problems.begin(), inputs.problems.begin(), inputs.problems.end());
    return built;
}

// ---- solids -------------------------------------------------------------------------------

Result<TriangleMesh> loftSolid(const SolvedAlignment& alignment,
                               const std::vector<double>& stations,
                               const std::vector<std::vector<SolidColumn>>& columns)
{
    const std::size_t count = stations.size();
    if (count < 2 || columns.size() != count) {
        return makeError(ErrorCode::InvalidArgument,
                         "a solid needs its columns at two stations at least");
    }
    const std::size_t across = columns.front().size();
    if (across < 2 ||
        std::ranges::any_of(columns, [across](const auto& c) { return c.size() != across; })) {
        return makeError(ErrorCode::InvalidArgument,
                         "a solid needs the same columns, two at least, at every station");
    }
    TriangleMesh mesh;
    mesh.vertices.reserve(count * across * 2);
    for (std::size_t k = 0; k < count; ++k) {
        for (const SolidColumn& column : columns[k]) {
            const auto plan = alignment.pointAtStationOffset(stations[k], column.offset);
            if (!plan) {
                return makeError(ErrorCode::InvalidArgument, "the station is outside the alignment",
                                 number(stations[k]));
            }
            mesh.vertices.emplace_back(plan->x, plan->y, column.top);
            mesh.vertices.emplace_back(plan->x, plan->y, column.bottom);
        }
    }
    const auto vertex = [across](std::size_t k, std::size_t i, std::size_t bottom) {
        return static_cast<std::uint32_t>((k * across + i) * 2 + bottom);
    };
    const auto quad = [&mesh](std::uint32_t a, std::uint32_t b, std::uint32_t c, std::uint32_t d) {
        mesh.faces.push_back({a, b, c});
        mesh.faces.push_back({a, c, d});
    };
    // Each quad is walked so that every edge it shares is walked the other
    // way by its neighbour (docs/road_design.md, "A closed solid").
    for (std::size_t k = 0; k + 1 < count; ++k) {
        for (std::size_t i = 0; i + 1 < across; ++i) {
            quad(vertex(k, i, 0), vertex(k, i + 1, 0), vertex(k + 1, i + 1, 0),
                 vertex(k + 1, i, 0));
            quad(vertex(k, i, 1), vertex(k + 1, i, 1), vertex(k + 1, i + 1, 1),
                 vertex(k, i + 1, 1));
        }
        quad(vertex(k, 0, 0), vertex(k + 1, 0, 0), vertex(k + 1, 0, 1), vertex(k, 0, 1));
        const std::size_t last = across - 1;
        quad(vertex(k + 1, last, 0), vertex(k, last, 0), vertex(k, last, 1),
             vertex(k + 1, last, 1));
    }
    for (std::size_t i = 0; i + 1 < across; ++i) {
        quad(vertex(0, i + 1, 0), vertex(0, i, 0), vertex(0, i, 1), vertex(0, i + 1, 1));
        const std::size_t k = count - 1;
        quad(vertex(k, i, 0), vertex(k, i + 1, 0), vertex(k, i + 1, 1), vertex(k, i, 1));
    }
    // Which way that turns depends on the side: offsets grow to the left, so
    // the right side's solids are mirror images. Turned outward by the sign
    // of the volume it encloses.
    if (meshVolume(mesh) < 0.0) {
        for (auto& face : mesh.faces) {
            std::swap(face[1], face[2]);
        }
    }
    return mesh;
}

namespace {

const std::vector<OutSpan>& spansOf(const RoadSection& section, Side side, bool cutting)
{
    if (side == Side::Left) {
        return cutting ? section.leftCutSpans : section.leftFillSpans;
    }
    return cutting ? section.rightCutSpans : section.rightFillSpans;
}

double areaOf(const RoadSection& section, Side side, bool cutting)
{
    if (side == Side::Left) {
        return cutting ? section.leftCut : section.leftFill;
    }
    return cutting ? section.rightCut : section.rightFill;
}

// The cut (or fill) of one side of one run, as a closed solid for each piece:
// a stretch of stations over which the side has earthworks at all
// (RoadSection::leftCutSpans ...), inside a stretch of complete sections. Each
// piece runs over the sections that have earthworks and one beyond each end
// where there is one (a section with none), because the end-area volume of
// the interval between the two tapers to nothing there. Its columns run from
// where its earthworks start across the section to where they end, so the
// solid, its plan outline and its GrossArea are the earthworks and not the
// whole half-section between the centreline and the daylight.
void addEarthworks(bim::BimModel& model, const BuiltCorridor& built, const CorridorRun& run,
                   Side side, bool cutting)
{
    const SolvedAlignment& alignment = *built.alignment;
    const std::string source = "corridor " + built.name;
    const std::string sideWord(toString(side));
    const std::string region = std::to_string(run.region);
    const std::string what = cutting ? "cut" : "fill";
    const double sign = side == Side::Left ? 1.0 : -1.0;
    const auto active = [&](std::size_t index) {
        return !spansOf(run.sections[index], side, cutting).empty();
    };
    std::size_t emitted = 0;
    std::size_t k = 0;
    while (k < run.sections.size()) {
        if (!run.sections[k].complete) {
            ++k;
            continue;
        }
        std::size_t last = k;
        while (last + 1 < run.sections.size() && run.sections[last + 1].complete) {
            ++last;
        }
        const std::size_t first = k;
        k = last + 1;
        if (last == first) {
            continue;
        }
        // The runs of sections that have earthworks, inside [first, last].
        for (std::size_t j = first; j <= last; ++j) {
            if (!active(j)) {
                continue;
            }
            std::size_t lastActive = j;
            while (lastActive + 1 <= last && active(lastActive + 1)) {
                ++lastActive;
            }
            const std::size_t firstActive = j;
            j = lastActive;
            const std::size_t from = firstActive > first ? firstActive - 1 : firstActive;
            const std::size_t to = lastActive < last ? lastActive + 1 : lastActive;
            if (from == to) {
                continue;
            }
            katana::math::CompensatedSum endArea;
            for (std::size_t m = from; m < to; ++m) {
                const RoadSection& a = run.sections[m];
                const RoadSection& b = run.sections[m + 1];
                endArea.add(0.5 * (areaOf(a, side, cutting) + areaOf(b, side, cutting)) *
                            (b.station - a.station));
            }
            if (!(endArea.value() > 0.0)) {
                continue;
            }
            std::vector<double> stations;
            std::vector<std::vector<SolidColumn>> columns;
            std::vector<Point2> inner;
            std::vector<Point2> outer;
            for (std::size_t m = from; m <= to; ++m) {
                const RoadSection& section = run.sections[m];
                // A section with none takes the stretch of its neighbour that
                // has some, with no thickness there; in plan it is a point
                // at the middle of it, which makes the outline taper as the
                // end-area volume does.
                const bool has = active(m);
                const std::size_t like = has ? m : (m == from ? m + 1 : m - 1);
                const auto& spans = spansOf(run.sections[like], side, cutting);
                const double outFrom = spans.front().from;
                const double outTo = spans.back().to;
                const std::vector<SectionVertex> formation = vertices(section.formation);
                std::vector<SolidColumn> across;
                for (std::size_t c = 0; c < kEarthworksColumns; ++c) {
                    const double out = outFrom + (outTo - outFrom) * static_cast<double>(c) /
                                                  static_cast<double>(kEarthworksColumns - 1);
                    const double offset = sign * out;
                    const double f = levelOn(formation, offset);
                    const double g = levelOn(section.ground, offset);
                    across.push_back(cutting ? SolidColumn{offset, std::max(g, f), f}
                                             : SolidColumn{offset, f, std::min(g, f)});
                }
                stations.push_back(section.station);
                columns.push_back(std::move(across));
                const double lo = has ? outFrom : 0.5 * (outFrom + outTo);
                const double hi = has ? outTo : lo;
                if (const auto p = alignment.pointAtStationOffset(section.station, sign * lo)) {
                    inner.push_back(*p);
                }
                if (const auto p = alignment.pointAtStationOffset(section.station, sign * hi)) {
                    outer.push_back(*p);
                }
            }
            auto mesh = loftSolid(alignment, stations, columns);
            if (!mesh) {
                model.problems.push_back(
                    bim::BimProblem{source, "earthworks " + sideWord + ": " + mesh.error().message});
                continue;
            }
            katana::geometry::Polyline2 outline;
            outline.closed = true;
            outline.vertices = inner;
            for (auto it = outer.rbegin(); it != outer.rend(); ++it) {
                // A tapered end is one point, not two.
                if (outline.vertices.empty() || !(outline.vertices.back() == *it)) {
                    outline.vertices.push_back(*it);
                }
            }
            if (outline.vertices.size() > 2 && outline.vertices.back() == outline.vertices.front()) {
                outline.vertices.pop_back(); // the outline is closed: no point twice
            }
            ++emitted;
            bim::BimObject object;
            object.id = "corridor/" + built.name + "/" + sideWord + "/earthworks/" + what + "/" +
                        region + (emitted > 1 ? "-" + std::to_string(emitted) : "");
            object.name = built.name + " " + sideWord + " earthworks " + what;
            object.kind = "earthworks";
            object.facility = built.name;
            object.part = "ROADSEGMENT";
            object.ifcClass = cutting ? "IfcEarthworksCut" : "IfcEarthworksFill";
            object.predefinedType = cutting ? "CUT" : "EMBANKMENT";
            object.material = cutting ? "excavated material" : "fill";
            // As a cut and fill map colours them (the "Cut Fill" surface
            // style): blue cut, red fill.
            object.colour = cutting ? katana::entity::Color{74, 127, 193, 255}
                                    : katana::entity::Color{193, 80, 74, 255};
            object.layer = cutting ? "road/earthworks/cut" : "road/earthworks/fill";
            object.plan.push_back(std::move(outline));
            object.properties = {{"template", run.roadTemplate.name},
                                 {"region", region},
                                 {"from", number(stations.front())},
                                 {"to", number(stations.back())},
                                 {"side", sideWord},
                                 {"method", "average end area"}};
            // The surface a cut is taken from, which the IFC export reads by
            // this name.
            if (cutting) {
                object.properties.emplace_back("Target Surface", built.surface);
            }
            object.quantities = {quantity("Length", stations.back() - stations.front(), "m"),
                                 quantity("GrossArea", object.plan.front().area(), "m2"),
                                 quantity("GrossVolume", endArea.value(), "m3")};
            object.mesh = std::move(*mesh);
            model.objects.push_back(std::move(object));
        }
    }
}

} // namespace

bim::BimModel corridorSolids(const BuiltCorridor& built)
{
    bim::BimModel model;
    const std::string source = "corridor " + built.name;
    for (const std::string& problem : built.problems) {
        model.problems.push_back(bim::BimProblem{source, problem});
    }
    if (!built.alignment) {
        return model;
    }
    const SolvedAlignment& alignment = *built.alignment;
    const auto fail = [&](const std::string& what) {
        model.problems.push_back(bim::BimProblem{source, what});
    };
    for (const CorridorRun& run : built.runs) {
        const std::string region = std::to_string(run.region);
        if (run.sections.size() < 2) {
            fail("stations " + number(run.start) + " to " + number(run.end) +
                 " have fewer than two sections: no solids are made for them");
            continue;
        }
        std::vector<double> stations;
        for (const RoadSection& section : run.sections) {
            stations.push_back(section.station);
        }
        const std::vector<std::pair<std::string, std::string>> range{
            {"template", run.roadTemplate.name},
            {"region", region},
            {"from", number(run.start)},
            {"to", number(run.end)}};

        // ---- courses and kerbs ----------------------------------------------------------
        for (std::size_t s = 0; s < run.sections.front().solids.size(); ++s) {
            const SectionSolid& first = run.sections.front().solids[s];
            std::vector<std::vector<SolidColumn>> columns;
            for (const RoadSection& section : run.sections) {
                if (s >= section.solids.size() || section.solids[s].key != first.key) {
                    break;
                }
                columns.push_back(section.solids[s].columns);
            }
            if (columns.size() != stations.size()) {
                fail("the solid " + first.key + " changes between sections: it is not made");
                continue;
            }
            auto mesh = loftSolid(alignment, stations, columns);
            if (!mesh) {
                fail(first.key + ": " + mesh.error().message);
                continue;
            }
            const Component& component = run.roadTemplate.components[first.component];
            const std::string sideWord(toString(component.side));
            bim::BimObject object;
            object.facility = built.name;
            object.part = std::string(partOf(component.kind));
            object.layer = component.layer;
            object.plan.push_back(outlineOf(alignment, stations, columns));
            object.properties = range;
            object.properties.emplace_back("side", sideWord);
            object.properties.emplace_back("component", component.name);
            const double volume = meshVolume(*mesh);
            if (first.course) {
                const PavementCourse& course = component.courses[*first.course];
                object.id = "corridor/" + built.name + "/" + sideWord + "/" + component.name + "/" +
                            course.name + "/" + region;
                object.name =
                    built.name + " " + sideWord + " " + component.name + " " + course.name;
                object.kind = std::string(objectKindOf(component.kind));
                // A footpath's top course is the path itself, a slab
                // (IfcSlab SIDEWALK); what is under it, and every other
                // course, an IfcCourse of the type its template names.
                if (component.kind == ComponentKind::Footpath && *first.course == 0) {
                    object.ifcClass = "IfcSlab";
                    object.predefinedType = "SIDEWALK";
                } else {
                    object.ifcClass = "IfcCourse";
                    object.predefinedType = course.ifcType;
                }
                object.material = course.material;
                object.colour = course.colour;
                object.properties.emplace_back("course", course.name);
                const double width =
                    std::abs(columns.front().back().offset - columns.front().front().offset);
                object.quantities = {quantity("Length", run.end - run.start, "m"),
                                     quantity("Width", width, "m"),
                                     quantity("Depth", course.thickness, "m"),
                                     quantity("GrossArea", object.plan.front().area(), "m2"),
                                     quantity("GrossVolume", volume, "m3")};
            } else {
                object.id =
                    "corridor/" + built.name + "/" + sideWord + "/" + component.name + "/" + region;
                object.name = built.name + " " + sideWord + " " + component.name;
                object.kind = "kerb";
                // IfcKerb has no predefined type of its own to name.
                object.ifcClass = "IfcKerb";
                object.predefinedType.clear();
                object.material = component.material;
                object.colour = component.colour;
                object.properties.emplace_back("shape", std::string(toString(component.shape)));
                // IfcKerb's Mountable attribute.
                object.properties.emplace_back(
                    "mountable", component.shape == KerbShape::Mountable ? "true" : "false");
                object.quantities = {
                    quantity("Length", planLength(alignment, stations, columns), "m"),
                    quantity("Width", kerbWidth(component), "m"),
                    quantity("Height", component.height, "m"),
                    quantity("GrossVolume", volume, "m3")};
            }
            object.mesh = std::move(*mesh);
            model.objects.push_back(std::move(object));
        }

        // ---- earthworks -----------------------------------------------------------------
        for (const Side side : {Side::Left, Side::Right}) {
            for (const bool cutting : {true, false}) {
                addEarthworks(model, built, run, side, cutting);
            }
        }
    }
    return model;
}

// ---- strings and surfaces ----------------------------------------------------------------

std::vector<const RoadSection*> corridorSections(const BuiltCorridor& built)
{
    std::vector<const RoadSection*> out;
    for (const CorridorRun& run : built.runs) {
        for (const RoadSection& section : run.sections) {
            out.push_back(&section);
        }
    }
    return out;
}

namespace {

// A section and the station its points are laid at along the alignment.
struct Placed {
    const RoadSection* section;
    double station;
};

// The sections where they stand, or - `step` - with the first section of each
// run that meets the one before at one station laid kFaceRun further on (no
// further than a quarter of the way to its run's next section). A surface
// holds one level at a point: two runs' sections at one station would make the
// TIN keep one of them, or blend them over the interval beside the join; a
// millimetre apart (more than the TIN's duplicate tolerance) they make a
// near-vertical face, the step a change of template is.
std::vector<Placed> placedSections(const std::vector<const RoadSection*>& sections, bool step)
{
    std::vector<Placed> out;
    out.reserve(sections.size());
    for (std::size_t i = 0; i < sections.size(); ++i) {
        double station = sections[i]->station;
        if (step && i > 0 && sections[i]->run != sections[i - 1]->run &&
            std::abs(station - sections[i - 1]->station) <= tol::kGeometric) {
            double room = kFaceRun;
            if (i + 1 < sections.size() && sections[i + 1]->run == sections[i]->run) {
                room = std::min(room, 0.25 * (sections[i + 1]->station - station));
            }
            station = sections[i - 1]->station + room;
        }
        out.push_back(Placed{sections[i], station});
    }
    return out;
}

// Strings by code over `placed`: a code missing from a section, a section not
// taken (`take` false) and the join of two runs end the strings through it.
template <typename Take>
std::vector<CorridorString> stringsOver(const std::vector<Placed>& placed,
                                        const SolvedAlignment& alignment, bool formation, Take take)
{
    std::vector<CorridorString> finished;
    std::map<std::string, CorridorString> open;
    const auto close = [&finished](CorridorString& string) {
        if (string.points.size() >= 2) {
            finished.push_back(std::move(string));
        }
    };
    const RoadSection* before = nullptr;
    for (const Placed& at : placed) {
        const RoadSection* section = at.section;
        const bool joined = before != nullptr && before->run != section->run;
        before = section;
        if (!take(*section) || joined) {
            for (auto& each : open) {
                close(each.second);
            }
            open.clear();
            if (!take(*section)) {
                continue;
            }
        }
        const auto& line = formation ? section->formation : section->top;
        std::map<std::string, CorridorString> next;
        for (const SectionPoint& point : line) {
            const auto plan = alignment.pointAtStationOffset(at.station, point.offset);
            if (!plan) {
                continue;
            }
            CorridorString string;
            if (auto found = open.find(point.code); found != open.end()) {
                string = std::move(found->second);
                open.erase(found);
            } else {
                string.code = point.code;
            }
            string.points.emplace_back(plan->x, plan->y, point.level);
            next.emplace(point.code, std::move(string));
        }
        for (auto& entry : open) {
            close(entry.second);
        }
        open = std::move(next);
    }
    for (auto& entry : open) {
        close(entry.second);
    }
    return finished;
}

} // namespace

std::vector<CorridorString> corridorStrings(const BuiltCorridor& built, bool formation)
{
    if (!built.alignment) {
        return {};
    }
    return stringsOver(placedSections(corridorSections(built), false), *built.alignment, formation,
                       [](const RoadSection& section) { return section.top.size() >= 2; });
}

Result<SectionsSurface> sectionsSurface(const std::vector<const RoadSection*>& sections,
                                        const SolvedAlignment& alignment, bool formation,
                                        bool completeOnly)
{
    SectionsSurface result;
    katana::terrain::TinInput input;
    std::vector<Point2> leftEdge;
    std::vector<Point2> rightEdge;
    // A section goes in when it is complete (or completeness is not asked
    // for) and the line it gives has two points at least.
    const auto take = [completeOnly, formation](const RoadSection& section) {
        const auto& line = formation ? section.formation : section.top;
        return (!completeOnly || section.complete) && line.size() >= 2;
    };
    const std::vector<Placed> placed = placedSections(sections, true);
    for (const Placed& entry : placed) {
        const RoadSection* section = entry.section;
        if (!take(*section)) {
            ++result.leftOut;
            continue;
        }
        const auto& line = formation ? section->formation : section->top;
        for (const SectionPoint& point : line) {
            if (const auto plan = alignment.pointAtStationOffset(entry.station, point.offset)) {
                input.points.emplace_back(plan->x, plan->y, point.level);
            }
        }
        if (const auto first = alignment.pointAtStationOffset(entry.station, line.front().offset)) {
            leftEdge.push_back(*first);
        }
        if (const auto last = alignment.pointAtStationOffset(entry.station, line.back().offset)) {
            rightEdge.push_back(*last);
        }
        ++result.sections;
    }
    if (result.sections < 2) {
        return makeError(ErrorCode::InvalidGeometry,
                         "fewer than two sections reach the ground: there is no surface to build",
                         std::to_string(result.leftOut) + " incomplete");
    }
    for (const CorridorString& string : stringsOver(placed, alignment, formation, take)) {
        katana::terrain::Breakline breakline;
        breakline.vertices = string.points;
        input.breaklines.push_back(std::move(breakline));
    }
    // A ring with gaps in it is not a ring: with sections left out the
    // surface is bounded by its hull instead (docs/cad.md).
    if (result.leftOut == 0) {
        input.boundary.vertices = leftEdge;
        input.boundary.vertices.insert(input.boundary.vertices.end(), rightEdge.rbegin(),
                                       rightEdge.rend());
        input.boundary.closed = true;
    }
    katana::terrain::TinBuildOptions options;
    // Points and string vertices coincide exactly by construction, with
    // equal levels; strings that cross on the inside of a tight curve take
    // the mean of their levels (docs/cad.md).
    options.crossingBreaklines = katana::terrain::CrossingBreaklinePolicy::Average;
    auto built = katana::terrain::buildTin(input, options);
    if (!built) {
        return built.error();
    }
    result.surface = std::move(built->surface);
    return result;
}

} // namespace katana::cad::road
