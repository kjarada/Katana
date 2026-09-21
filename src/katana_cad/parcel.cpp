#include "katana/cad/parcel.hpp"

#include <cmath>
#include <span>
#include <string>
#include <vector>

#include "katana/entity/dimension_text.hpp"
#include "katana/entity/tables.hpp"
#include "katana/math/numerics.hpp"
#include "katana/math/summation.hpp"
#include "katana/survey/angles.hpp"
#include "katana/survey/cogo.hpp"
#include "katana/survey/coordinate.hpp"

namespace katana::cad {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

// The ONE place the drawing's (x, y) becomes the survey layer's
// (northing, easting). Get this wrong and every bearing is wrong while every
// area stays right, which is why the rectangle test checks bearings.
survey::Coordinate2 toSurvey(const Point2& point) { return {point.y, point.x}; }

// Through the one measurement formatter (locale-independent, ties away from
// zero), so a parcel's "100.000" is spelled the way a dimension's is.
std::string fixed(double value, int decimals)
{
    entity::DimensionStyle style;
    style.decimals = decimals;
    style.suppressTrailingZeros = false;
    return entity::formatMeasurement(value, style);
}

// A width for text this code cannot measure: 0.6 of the height per character
// is the usual aspect of a CAD stroke font, and only the centring depends on
// it. A label centred a little off is still readable; the code that draws
// text has the real metrics and is the wrong layer for this to depend on.
double approximateWidth(const std::string& text, double height)
{
    return 0.6 * height * static_cast<double>(text.size());
}

} // namespace

Result<ParcelReport> parcelReport(const Polyline2& boundary)
{
    if (!boundary.closed) {
        return makeError(ErrorCode::InvalidGeometry, "a parcel boundary must be a closed polyline");
    }
    std::vector<Point2> corners;
    corners.reserve(boundary.vertices.size());
    for (const Point2& vertex : boundary.vertices) {
        if (!std::isfinite(vertex.x) || !std::isfinite(vertex.y)) {
            return makeError(ErrorCode::InvalidArgument, "a parcel corner is not finite");
        }
        // A corner clicked twice is one corner, not a zero-length course
        // with no bearing.
        if (corners.empty() || corners.back().distanceTo(vertex) >= tol::kCoordinate) {
            corners.push_back(vertex);
        }
    }
    if (corners.size() > 1 && corners.front().distanceTo(corners.back()) < tol::kCoordinate) {
        corners.pop_back(); // the closing vertex repeated the first
    }
    if (corners.size() < 3) {
        return makeError(ErrorCode::InvalidGeometry,
                         "a parcel needs at least three distinct corners",
                         std::to_string(corners.size()) + " found");
    }

    std::vector<survey::Coordinate2> ring;
    ring.reserve(corners.size());
    for (const Point2& corner : corners) {
        ring.push_back(toSurvey(corner));
    }
    const auto signedArea = survey::polygonSignedArea(ring);
    if (!signedArea) {
        return signedArea.error();
    }
    if (std::abs(*signedArea) <= tol::kGeometric) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the boundary encloses no area: its corners are collinear");
    }
    const auto centroid = survey::polygonCentroid(ring);
    if (!centroid) {
        return centroid.error();
    }

    ParcelReport report;
    report.clockwise = *signedArea < 0.0;
    report.area = std::abs(*signedArea);
    report.centroid = Point2(centroid->easting, centroid->northing);
    katana::math::CompensatedSum perimeter;
    for (std::size_t i = 0; i < corners.size(); ++i) {
        const Point2& from = corners[i];
        const Point2& to = corners[(i + 1) % corners.size()];
        const auto leg = survey::inverse(toSurvey(from), toSurvey(to));
        if (!leg) {
            return leg.error();
        }
        // Whole seconds: the precision a deed quotes.
        auto bearing = survey::formatBearing(leg->azimuth, 0);
        if (!bearing) {
            return bearing.error();
        }
        report.courses.push_back({from, to, leg->azimuth, leg->distance, std::move(*bearing)});
        perimeter.add(leg->distance);
    }
    report.perimeter = perimeter.value();
    return report;
}

std::string legalDescription(const ParcelReport& report, std::string_view name)
{
    std::string text(name);
    if (!text.empty()) {
        text += ": ";
    }
    const Point2& start = report.courses.front().from;
    text += "Beginning at E " + fixed(start.x, 3) + " N " + fixed(start.y, 3);
    for (const ParcelCourse& course : report.courses) {
        text += "; thence " + course.bearing + ", " + fixed(course.distance, 3) + " m";
    }
    text += " to the point of beginning. Containing " + fixed(report.area, 3) + " m\xC2\xB2 (" +
            fixed(report.area / 10000.0, 4) + " ha).";
    return text;
}

Result<std::vector<entity::TextGeometry>> parcelLabels(const ParcelReport& report, double height)
{
    if (!(height > 0.0) || !std::isfinite(height)) {
        return makeError(ErrorCode::InvalidArgument, "the label height must be positive");
    }
    std::vector<entity::TextGeometry> labels;
    labels.reserve(report.courses.size() + 1);
    // Clear of the line by a fraction of the text height, so a label neither
    // touches the boundary nor floats away from it.
    const double gap = 0.6 * height;

    for (const ParcelCourse& course : report.courses) {
        const Point2 mid = (course.from + course.to) * 0.5;
        const double along = std::atan2(course.to.y - course.from.y, course.to.x - course.from.x);
        // Readable: along the course, but never past a quarter turn either
        // way, so a course running west or south has its label flipped to
        // read left to right or upward like every other.
        double rotation = katana::math::normalizeAngleSigned(along);
        if (rotation > katana::math::kPi / 2.0 || rotation <= -katana::math::kPi / 2.0) {
            rotation = katana::math::normalizeAngleSigned(rotation + katana::math::kPi);
        }
        // Inside the boundary: to the left of the course when the boundary
        // runs counter-clockwise, to the right when clockwise.
        const double side = report.clockwise ? -1.0 : 1.0;
        const Point2 inward(-std::sin(along) * side, std::cos(along) * side);
        // The text's own up direction after any flip. When it points away
        // from the inside, the glyphs would hang outside the boundary, so the
        // baseline moves in by a further text height to keep them in.
        const Point2 up(-std::sin(rotation), std::cos(rotation));
        const double inset = gap + (up.x * inward.x + up.y * inward.y < 0.0 ? height : 0.0);

        entity::TextGeometry label;
        label.text = course.bearing + "  " + fixed(course.distance, 3);
        label.height = height;
        label.rotation = rotation;
        const double width = approximateWidth(label.text, height);
        const Point2 readingDirection(std::cos(rotation), std::sin(rotation));
        label.position = mid + inward * inset - readingDirection * (0.5 * width);
        labels.push_back(std::move(label));
    }

    entity::TextGeometry area;
    area.text = fixed(report.area, 3) + " m\xC2\xB2";
    area.height = height;
    area.rotation = 0.0;
    area.position = Point2(report.centroid.x - 0.5 * approximateWidth(area.text, height),
                           report.centroid.y - 0.5 * height);
    labels.push_back(std::move(area));
    return labels;
}

} // namespace katana::cad
