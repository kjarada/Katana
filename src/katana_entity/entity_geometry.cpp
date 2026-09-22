#include "katana/entity/entity_geometry.hpp"

#include <algorithm>
#include <charconv>
#include <cmath>
#include <limits>
#include <string>

namespace katana::entity {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Arc2;
using katana::geometry::Box2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;
using katana::math::Mat3;

std::string toString(const PropertyValue& value)
{
    struct Visitor {
        std::string operator()(bool v) const { return v ? "true" : "false"; }
        std::string operator()(std::int64_t v) const { return std::to_string(v); }
        std::string operator()(double v) const
        {
            // Shortest text that reads back as the same double.
            char buffer[32];
            const auto result = std::to_chars(buffer, buffer + sizeof buffer, v);
            return std::string(buffer, result.ptr);
        }
        std::string operator()(const std::string& v) const { return v; }
    };
    return std::visit(Visitor{}, value);
}

std::string_view typeName(const PropertyValue& value)
{
    struct Visitor {
        std::string_view operator()(bool) const { return "boolean"; }
        std::string_view operator()(std::int64_t) const { return "integer"; }
        std::string_view operator()(double) const { return "real"; }
        std::string_view operator()(const std::string&) const { return "text"; }
    };
    return std::visit(Visitor{}, value);
}

std::string_view toString(EntityType type)
{
    switch (type) {
    case EntityType::Point:
        return "Point";
    case EntityType::Line:
        return "Line";
    case EntityType::Arc:
        return "Arc";
    case EntityType::Polyline:
        return "Polyline";
    case EntityType::Circle:
        return "Circle";
    case EntityType::Text:
        return "Text";
    case EntityType::Dimension:
        return "Dimension";
    }
    return "Unknown";
}

bool isValidUtf8(std::string_view text)
{
    const auto* bytes = reinterpret_cast<const unsigned char*>(text.data());
    const std::size_t size = text.size();
    std::size_t i = 0;
    while (i < size) {
        const unsigned char lead = bytes[i];
        std::size_t following = 0;
        unsigned char lowSecond = 0x80;
        unsigned char highSecond = 0xBF;

        if (lead <= 0x7F) {
            i += 1;
            continue;
        }
        if (lead >= 0xC2 && lead <= 0xDF) {
            following = 1;
        } else if (lead == 0xE0) {
            following = 2;
            lowSecond = 0xA0; // anything lower would be an overlong encoding
        } else if (lead >= 0xE1 && lead <= 0xEC) {
            following = 2;
        } else if (lead == 0xED) {
            following = 2;
            highSecond = 0x9F; // D800-DFFF are surrogate halves, not characters
        } else if (lead >= 0xEE && lead <= 0xEF) {
            following = 2;
        } else if (lead == 0xF0) {
            following = 3;
            lowSecond = 0x90; // overlong
        } else if (lead >= 0xF1 && lead <= 0xF3) {
            following = 3;
        } else if (lead == 0xF4) {
            following = 3;
            highSecond = 0x8F; // above U+10FFFF
        } else {
            return false; // 0x80-0xC1 and 0xF5-0xFF are never lead bytes
        }

        if (i + following >= size) {
            return false; // truncated sequence
        }
        const unsigned char second = bytes[i + 1];
        if (second < lowSecond || second > highSecond) {
            return false;
        }
        for (std::size_t k = 2; k <= following; ++k) {
            const unsigned char continuation = bytes[i + k];
            if (continuation < 0x80 || continuation > 0xBF) {
                return false;
            }
        }
        i += following + 1;
    }
    return true;
}

Result<EntityType> entityTypeFromString(std::string_view name)
{
    // Derived from the variant rather than naming the last enumerator: a kind
    // appended after Dimension was previously unreachable here, which made it
    // unparseable by name - legacy-JSON rows of it would fail to load and
    // `SELECT TYPE <name>` would reject it, both with an error blaming the
    // input rather than this loop.
    constexpr int kKindCount = static_cast<int>(std::variant_size_v<Geometry>);
    for (int raw = 0; raw < kKindCount; ++raw) {
        const auto type = static_cast<EntityType>(raw);
        if (toString(type) == name) {
            return type;
        }
    }
    return makeError(ErrorCode::ParseFailure, "unknown entity type", std::string(name));
}

// ---- Color ---------------------------------------------------------------------

namespace {

int hexDigit(char ch)
{
    if (ch >= '0' && ch <= '9') {
        return ch - '0';
    }
    if (ch >= 'a' && ch <= 'f') {
        return ch - 'a' + 10;
    }
    if (ch >= 'A' && ch <= 'F') {
        return ch - 'A' + 10;
    }
    return -1;
}

} // namespace

Result<Color> Color::fromHex(std::string_view text)
{
    if ((text.size() != 7 && text.size() != 9) || text.front() != '#') {
        return makeError(ErrorCode::ParseFailure, "colour must be #RRGGBB or #RRGGBBAA",
                         std::string(text));
    }
    std::uint8_t channels[4] = {0, 0, 0, 255};
    for (std::size_t i = 0; i + 1 < text.size(); i += 2) {
        const int hi = hexDigit(text[i + 1]);
        const int lo = hexDigit(text[i + 2]);
        if (hi < 0 || lo < 0) {
            return makeError(ErrorCode::ParseFailure, "colour contains a non-hexadecimal digit",
                             std::string(text));
        }
        channels[i / 2] = static_cast<std::uint8_t>(hi * 16 + lo);
    }
    return Color{channels[0], channels[1], channels[2], channels[3]};
}

std::string Color::toHex() const
{
    constexpr char digits[] = "0123456789ABCDEF";
    std::string text = "#";
    const std::uint8_t channels[4] = {r, g, b, a};
    const std::size_t count = a == 255 ? 3 : 4;
    for (std::size_t i = 0; i < count; ++i) {
        text += digits[channels[i] >> 4];
        text += digits[channels[i] & 0x0F];
    }
    return text;
}

// ---- validate ---------------------------------------------------------------------

namespace {

Status requireFinite(const Point2& p, const char* what)
{
    if (!p.isFinite()) {
        return makeError(ErrorCode::InvalidGeometry, std::string(what) + " is not finite");
    }
    return {};
}

Status requirePositive(double value, const char* what)
{
    if (!(std::isfinite(value) && value > tol::kGeometric)) {
        return makeError(ErrorCode::InvalidGeometry, std::string(what) + " must be positive",
                         std::to_string(value));
    }
    return {};
}

struct Validator {
    Status operator()(const PointGeometry& point) const
    {
        return requireFinite(point.position, "point position");
    }
    Status operator()(const Segment2& segment) const
    {
        if (auto status = requireFinite(segment.start, "line start"); !status) {
            return status;
        }
        if (auto status = requireFinite(segment.end, "line end"); !status) {
            return status;
        }
        if (segment.isDegenerate()) {
            return makeError(ErrorCode::InvalidGeometry, "line has zero length");
        }
        return {};
    }
    Status operator()(const Arc2& arc) const
    {
        if (auto status = requireFinite(arc.center, "arc centre"); !status) {
            return status;
        }
        if (auto status = requirePositive(arc.radius, "arc radius"); !status) {
            return status;
        }
        if (!std::isfinite(arc.startAngle) || !std::isfinite(arc.sweep)) {
            return makeError(ErrorCode::InvalidGeometry, "arc angles are not finite");
        }
        if (std::abs(arc.sweep) > katana::math::kTwoPi + tol::kAngular) {
            return makeError(ErrorCode::InvalidGeometry, "arc sweeps more than a full turn");
        }
        if (!(arc.length() > tol::kGeometric)) {
            return makeError(ErrorCode::InvalidGeometry, "arc has zero length");
        }
        return {};
    }
    Status operator()(const Polyline2& polyline) const
    {
        if (polyline.vertices.size() < 2) {
            return makeError(ErrorCode::InvalidGeometry, "polyline needs at least two vertices");
        }
        for (const Point2& vertex : polyline.vertices) {
            if (auto status = requireFinite(vertex, "polyline vertex"); !status) {
                return status;
            }
        }
        if (!(polyline.length() > tol::kGeometric)) {
            return makeError(ErrorCode::InvalidGeometry, "polyline has zero length");
        }
        return {};
    }
    Status operator()(const Circle2& circle) const
    {
        if (auto status = requireFinite(circle.center, "circle centre"); !status) {
            return status;
        }
        return requirePositive(circle.radius, "circle radius");
    }
    Status operator()(const TextGeometry& text) const
    {
        if (auto status = requireFinite(text.position, "text position"); !status) {
            return status;
        }
        if (!std::isfinite(text.rotation)) {
            return makeError(ErrorCode::InvalidGeometry, "text rotation is not finite");
        }
        if (!isValidUtf8(text.text)) {
            return makeError(ErrorCode::InvalidGeometry, "text is not valid UTF-8");
        }
        if (text.text.empty()) {
            return makeError(ErrorCode::InvalidGeometry, "text is empty");
        }
        return requirePositive(text.height, "text height");
    }
    Status operator()(const DimensionGeometry& dimension) const
    {
        if (auto status = requireFinite(dimension.start, "dimension start"); !status) {
            return status;
        }
        if (auto status = requireFinite(dimension.end, "dimension end"); !status) {
            return status;
        }
        if (!std::isfinite(dimension.offset)) {
            return makeError(ErrorCode::InvalidGeometry, "dimension offset is not finite");
        }
        if (!(dimension.measurement() > tol::kGeometric)) {
            return makeError(ErrorCode::InvalidGeometry, "dimension measures zero length");
        }
        return {};
    }
};

// Character cell width as a fraction of the text height (no font metrics here).
constexpr double kApproximateGlyphAspect = 0.6;

Segment2 textBaseline(const TextGeometry& text)
{
    const double width = kApproximateGlyphAspect * text.height * static_cast<double>(text.text.size());
    return Segment2{text.position, text.position + Vec2(width, 0.0).rotated(text.rotation)};
}

Segment2 dimensionLine(const DimensionGeometry& dimension)
{
    const Vec2 shift =
        (dimension.end - dimension.start).normalized().perpendicular() * dimension.offset;
    return Segment2{dimension.start + shift, dimension.end + shift};
}

} // namespace

Status validate(const Geometry& geometry)
{
    return std::visit(Validator{}, geometry);
}

// ---- boundingBox / distanceTo --------------------------------------------------------

Box2 boundingBox(const Geometry& geometry)
{
    struct Visitor {
        Box2 operator()(const PointGeometry& point) const
        {
            Box2 box;
            box.expand(point.position);
            return box;
        }
        Box2 operator()(const Segment2& segment) const { return segment.boundingBox(); }
        Box2 operator()(const Arc2& arc) const { return arc.boundingBox(); }
        Box2 operator()(const Polyline2& polyline) const { return polyline.boundingBox(); }
        Box2 operator()(const Circle2& circle) const { return circle.boundingBox(); }
        Box2 operator()(const TextGeometry& text) const
        {
            const Segment2 baseline = textBaseline(text);
            const Vec2 up = Vec2(0.0, text.height).rotated(text.rotation);
            Box2 box = baseline.boundingBox();
            box.expand(baseline.start + up);
            box.expand(baseline.end + up);
            return box;
        }
        Box2 operator()(const DimensionGeometry& dimension) const
        {
            Box2 box = dimensionLine(dimension).boundingBox();
            box.expand(dimension.start);
            box.expand(dimension.end);
            return box;
        }
    };
    return std::visit(Visitor{}, geometry);
}

double distanceTo(const Geometry& geometry, const Point2& p)
{
    struct Visitor {
        const Point2& p;
        double operator()(const PointGeometry& point) const { return point.position.distanceTo(p); }
        double operator()(const Segment2& segment) const { return segment.distanceTo(p); }
        double operator()(const Arc2& arc) const { return arc.distanceTo(p); }
        double operator()(const Polyline2& polyline) const
        {
            return polyline.distanceTo(p).value_or(std::numeric_limits<double>::infinity());
        }
        double operator()(const Circle2& circle) const { return circle.distanceTo(p); }
        double operator()(const TextGeometry& text) const { return textBaseline(text).distanceTo(p); }
        double operator()(const DimensionGeometry& dimension) const
        {
            return dimensionLine(dimension).distanceTo(p);
        }
    };
    return std::visit(Visitor{p}, geometry);
}

// ---- transformed ------------------------------------------------------------------------

namespace {

struct Similarity {
    Mat3 matrix;
    double scale = 1.0;
    bool mirrored = false;

    [[nodiscard]] Point2 point(const Point2& p) const { return transformPoint(matrix, p); }
    // Direction angle after the transform.
    [[nodiscard]] double angle(double radians) const
    {
        return transformVector(matrix, Vec2(std::cos(radians), std::sin(radians))).angle();
    }
};

Result<Similarity> asSimilarity(const Mat3& m)
{
    const bool affine = m(2, 0) == 0.0 && m(2, 1) == 0.0 && m(2, 2) == 1.0;
    const Vec2 columnX(m(0, 0), m(1, 0));
    const Vec2 columnY(m(0, 1), m(1, 1));
    const double scaleX = columnX.length();
    const double scaleY = columnY.length();
    const bool finite = columnX.isFinite() && columnY.isFinite() &&
                        std::isfinite(m(0, 2)) && std::isfinite(m(1, 2));
    if (!affine || !finite || !(scaleX > 0.0) ||
        !katana::math::nearlyEqual(scaleX, scaleY, tol::kRelative) ||
        std::abs(columnX.dot(columnY)) > tol::kRelative * scaleX * scaleY) {
        return makeError(ErrorCode::InvalidArgument,
                         "transform must be a similarity (no shear or non-uniform scale)");
    }
    return Similarity{m, scaleX, columnX.cross(columnY) < 0.0};
}

} // namespace

Result<Geometry> transformed(const Geometry& geometry, const Mat3& transform)
{
    const auto similarity = asSimilarity(transform);
    if (!similarity) {
        return similarity.error();
    }
    const Similarity& s = *similarity;

    struct Visitor {
        const Similarity& s;
        Geometry operator()(const PointGeometry& point) const
        {
            return PointGeometry{s.point(point.position)};
        }
        Geometry operator()(const Segment2& segment) const
        {
            return Segment2{s.point(segment.start), s.point(segment.end)};
        }
        Geometry operator()(const Arc2& arc) const
        {
            return Arc2{s.point(arc.center), arc.radius * s.scale, s.angle(arc.startAngle),
                        s.mirrored ? -arc.sweep : arc.sweep};
        }
        Geometry operator()(const Polyline2& polyline) const
        {
            Polyline2 result = polyline;
            for (Point2& vertex : result.vertices) {
                vertex = s.point(vertex);
            }
            return result;
        }
        Geometry operator()(const Circle2& circle) const
        {
            return Circle2{s.point(circle.center), circle.radius * s.scale};
        }
        Geometry operator()(const TextGeometry& text) const
        {
            TextGeometry result = text;
            result.position = s.point(text.position);
            result.height = text.height * s.scale;
            result.rotation = s.angle(text.rotation);
            return result;
        }
        Geometry operator()(const DimensionGeometry& dimension) const
        {
            DimensionGeometry result = dimension;
            result.start = s.point(dimension.start);
            result.end = s.point(dimension.end);
            result.offset = dimension.offset * s.scale * (s.mirrored ? -1.0 : 1.0);
            return result;
        }
    };

    Geometry result = std::visit(Visitor{s}, geometry);
    if (auto status = validate(result); !status) {
        return status.error(); // e.g. scaled below the geometric tolerance
    }
    return result;
}

} // namespace katana::entity
