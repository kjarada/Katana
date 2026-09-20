#include "katana/cad/dashing.hpp"

#include <algorithm>
#include <cmath>

namespace katana::cad {

using katana::entity::Linetype;
using katana::entity::LinetypeElement;

namespace {

[[nodiscard]] double effectiveScale(const DashOptions& options)
{
    // One place where the scale factors are combined, so that adding a
    // per-entity scale later is a change here and nowhere else.
    return std::isfinite(options.patternScale) && options.patternScale > 0.0
               ? options.patternScale
               : 1.0;
}

} // namespace

bool shouldDash(const Linetype& linetype, const DashOptions& options)
{
    if (linetype.isContinuous()) {
        return false;
    }
    const double scale = effectiveScale(options);
    if (!(linetype.patternLength() * scale > 0.0)) {
        return false;
    }
    if (!std::isfinite(options.viewScale) || options.viewScale <= 0.0) {
        return false;
    }
    const double shortest = linetype.shortestElement();
    if (!(shortest > 0.0)) {
        // Every element is a dot: there is no length to resolve, so the dots
        // are laid out by pattern length alone and are always drawable.
        return linetype.patternLength() * scale * options.viewScale >=
               options.minimumElementPixels;
    }
    return shortest * scale * options.viewScale >= options.minimumElementPixels;
}

bool forEachDash(const std::vector<Point2>& points, bool closed, const Linetype& linetype,
                 const DashOptions& options,
                 const std::function<void(const Point2& from, const Point2& to)>& emit)
{
    if (points.size() < 2 || !shouldDash(linetype, options)) {
        return false;
    }
    const double scale = effectiveScale(options);
    const double period = linetype.patternLength() * scale;
    if (!(period > 0.0)) {
        return false;
    }

    // Total path length first, so the span count can be bounded BEFORE anything
    // is emitted. The contract is all-or-nothing: a caller told "false" draws
    // the whole path solid, and must never receive a truncated one.
    double pathLength = 0.0;
    for (std::size_t i = 1; i < points.size(); ++i) {
        pathLength += points[i - 1].distanceTo(points[i]);
    }
    if (closed) {
        pathLength += points.back().distanceTo(points.front());
    }
    if (!(pathLength > 0.0)) {
        return false;
    }
    std::size_t penDownPerPeriod = 0;
    for (const LinetypeElement& element : linetype.pattern) {
        if (element.isPenDown()) {
            ++penDownPerPeriod;
        }
    }
    const double periods = pathLength / period;
    // +1 per period for the span a segment boundary can split.
    if (periods * static_cast<double>(penDownPerPeriod + 1) >
        static_cast<double>(options.maximumSpans)) {
        return false;
    }

    // Walk the path once, carrying the pattern cursor across vertices, so the
    // pattern is continuous along the whole path rather than restarting at
    // every vertex (DXF LWPOLYLINE group 70 bit 128, the $PLINEGEN default).
    std::size_t element = 0;
    double remainingInElement = std::abs(linetype.pattern[0].length) * scale;
    bool penDown = linetype.pattern[0].isPenDown();
    const bool startsWithDot = linetype.pattern[0].isDot();
    if (startsWithDot) {
        emit(points.front(), points.front());
        // A dot consumes no length, so step straight to the next element.
        element = 1 % linetype.pattern.size();
        remainingInElement = std::abs(linetype.pattern[element].length) * scale;
        penDown = linetype.pattern[element].isPenDown();
    }

    const std::size_t segmentCount = closed ? points.size() : points.size() - 1;
    for (std::size_t s = 0; s < segmentCount; ++s) {
        const Point2& a = points[s];
        const Point2& b = points[(s + 1) % points.size()];
        const double segmentLength = a.distanceTo(b);
        if (!(segmentLength > 0.0)) {
            continue;
        }
        const auto pointAt = [&a, &b, segmentLength](double distance) {
            const double t = std::clamp(distance / segmentLength, 0.0, 1.0);
            return a + (b - a) * t;
        };

        double consumed = 0.0;
        while (consumed < segmentLength) {
            const double take = std::min(remainingInElement, segmentLength - consumed);
            if (penDown && take > 0.0) {
                emit(pointAt(consumed), pointAt(consumed + take));
            }
            consumed += take;
            remainingInElement -= take;

            if (remainingInElement > 0.0) {
                break; // the element continues into the next segment
            }
            // Advance, skipping over dots by emitting them in place.
            do {
                element = (element + 1) % linetype.pattern.size();
                const LinetypeElement& next = linetype.pattern[element];
                penDown = next.isPenDown();
                remainingInElement = std::abs(next.length) * scale;
                if (next.isDot()) {
                    emit(pointAt(consumed), pointAt(consumed));
                }
            } while (remainingInElement <= 0.0 && consumed < segmentLength);

            if (remainingInElement <= 0.0) {
                break; // every remaining element is a dot; stop to avoid spinning
            }
        }
    }
    return true;
}

std::vector<double> qtDashPattern(const Linetype& linetype, const DashOptions& options,
                                  double penWidthPixels)
{
    if (!shouldDash(linetype, options)) {
        return {};
    }
    // QPen multiplies the array by the pen width when it draws, so the array is
    // in units of pen width and the conversion has to divide by it. Forgetting
    // this renders perfectly and makes every pattern as many times too long as
    // the pen is wide - the viewport draws at 1.5 and 2 px, so 1.5x and 2x.
    const double width = std::isfinite(penWidthPixels) && penWidthPixels > 0.0
                             ? penWidthPixels
                             : 1.0; // a cosmetic pen is drawn one pixel wide
    const double scale = effectiveScale(options) * options.viewScale / width;

    std::vector<double> qt;
    qt.reserve(linetype.pattern.size());
    for (const LinetypeElement& element : linetype.pattern) {
        // A dot has no length; a zero-length on-span draws nothing under a flat
        // cap, so it becomes the thinnest mark Qt will render.
        const double length = element.isDot() ? width * 0.5 : std::abs(element.length) * scale;
        qt.push_back(std::max(length, 0.01));
    }
    // QPen requires an even count: the array alternates on/off starting with
    // on, so an odd one would invert the meaning of every later entry when it
    // repeats. validate() already requires the pattern to end with a gap, which
    // makes it even, but a project loaded from elsewhere is not this program's
    // to trust.
    if (qt.size() % 2 != 0) {
        qt.push_back(0.01);
    }
    return qt;
}

} // namespace katana::cad
