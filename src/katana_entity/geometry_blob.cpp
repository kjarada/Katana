#include "katana/entity/geometry_blob.hpp"

#include <bit>
#include <cmath>
#include <limits>
#include <cstring>
#include <type_traits>
#include <variant>

#include "katana/entity/entity_geometry.hpp"

namespace katana::entity {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

namespace {

// A polyline of more vertices than this is not survey data, it is a corrupt
// length field. Checked before any allocation, so a damaged blob cannot ask
// for gigabytes. Ten million vertices is far past any real string.
constexpr std::uint32_t kMaximumVertices = 10'000'000;
// Strings in geometry are labels and dimension overrides, not documents.
constexpr std::uint32_t kMaximumStringBytes = 1'000'000;

// ---- writing ---------------------------------------------------------------------

void putU8(std::vector<std::byte>& out, std::uint8_t value)
{
    out.push_back(static_cast<std::byte>(value));
}

void putU32(std::vector<std::byte>& out, std::uint32_t value)
{
    // Explicitly little-endian rather than a memcpy of the native layout, so a
    // project written on one machine reads on another. The shifts compile to a
    // single store on a little-endian target.
    for (int shift = 0; shift < 32; shift += 8) {
        out.push_back(static_cast<std::byte>((value >> shift) & 0xFFu));
    }
}

void putU64(std::vector<std::byte>& out, std::uint64_t value)
{
    for (int shift = 0; shift < 64; shift += 8) {
        out.push_back(static_cast<std::byte>((value >> shift) & 0xFFu));
    }
}

void putDouble(std::vector<std::byte>& out, double value)
{
    // By bit pattern, so the round trip is exact for every value a double can
    // hold - negative zero and NaN included - rather than only for those that
    // survive a decimal detour.
    const auto bits = std::bit_cast<std::uint64_t>(value);
    for (int shift = 0; shift < 64; shift += 8) {
        out.push_back(static_cast<std::byte>((bits >> shift) & 0xFFu));
    }
}

void putPoint(std::vector<std::byte>& out, const Point2& point)
{
    putDouble(out, point.x);
    putDouble(out, point.y);
}

[[nodiscard]] katana::core::Status putString(std::vector<std::byte>& out,
                                             const std::string& text)
{
    if (text.size() > kMaximumStringBytes) {
        return makeError(ErrorCode::InvalidArgument, "text is too long to store",
                         std::to_string(text.size()));
    }
    if (!isValidUtf8(text)) {
        // The same rule the JSON writer enforces. Keeping it here means a blob
        // can always be re-encoded as JSON for an export without that writer
        // throwing, which is what the Result on geometryToJson exists for.
        return makeError(ErrorCode::InvalidArgument, "text is not valid UTF-8");
    }
    putU32(out, static_cast<std::uint32_t>(text.size()));
    const auto* bytes = reinterpret_cast<const std::byte*>(text.data());
    out.insert(out.end(), bytes, bytes + text.size());
    return {};
}

void putAnchor(std::vector<std::byte>& out, const AnchorRef& ref, std::uint8_t version)
{
    putU64(out, ref.entity);
    putU8(out, static_cast<std::uint8_t>(ref.point));
    putU32(out, ref.index);
    if (version >= kBlobVersionSmartLeader) {
        putDouble(out, ref.parameter);
    }
}

katana::core::Status putPoints(std::vector<std::byte>& out, const std::vector<Point2>& points)
{
    if (points.size() > kMaximumVertices) {
        return makeError(ErrorCode::InvalidArgument,
                         "a geometry has more vertices than the format allows",
                         std::to_string(points.size()));
    }
    putU32(out, static_cast<std::uint32_t>(points.size()));
    out.reserve(out.size() + points.size() * 16);
    for (const Point2& point : points) {
        putPoint(out, point);
    }
    return {};
}

// Whether version 1 can hold `geometry`: every kind it had, with the members
// added since at their defaults.
bool fitsVersionOne(const Geometry& geometry)
{
    if (const auto* text = std::get_if<TextGeometry>(&geometry)) {
        return text->style.empty() && text->paperHeight == 0.0 &&
               text->justify == TextJustify::BottomLeft &&
               !std::signbit(text->paperHeight);
    }
    if (const auto* dimension = std::get_if<DimensionGeometry>(&geometry)) {
        DimensionGeometry plain;
        plain.start = dimension->start;
        plain.end = dimension->end;
        plain.offset = dimension->offset;
        plain.textOverride = dimension->textOverride;
        // Compared by bits for the doubles, so a -0.0 angle is written in the
        // layout that keeps it.
        return *dimension == plain && !std::signbit(dimension->angle) &&
               !std::signbit(dimension->vertex.x) && !std::signbit(dimension->vertex.y);
    }
    // Every kind appended after Dimension is version 2's (the drawing
    // system's CurvePolyline, Ellipse and Spline as well as the annotation
    // system's Label and Leader): a version-1 reader knows none of them.
    return geometry.index() <= static_cast<std::size_t>(EntityType::Dimension);
}

// Whether version 2 can hold an anchor: one of the points it had, and no
// parameter - compared by bits, so a -0.0 is written in the layout that keeps
// it.
bool anchorFitsVersionTwo(const AnchorRef& ref)
{
    return static_cast<int>(ref.point) <= static_cast<int>(AnchorPoint::SegmentMid) &&
           std::bit_cast<std::uint64_t>(ref.parameter) == 0;
}

// Whether version 2 can hold `geometry`: its anchors do, and a leader is not
// a smart one.
bool fitsVersionTwo(const Geometry& geometry)
{
    if (const auto* dimension = std::get_if<DimensionGeometry>(&geometry)) {
        return anchorFitsVersionTwo(dimension->startRef) &&
               anchorFitsVersionTwo(dimension->endRef) &&
               anchorFitsVersionTwo(dimension->vertexRef);
    }
    if (const auto* leader = std::get_if<LeaderGeometry>(&geometry)) {
        return anchorFitsVersionTwo(leader->tipRef) && !leader->fields &&
               leader->labelStyle.empty();
    }
    return true;
}

// ---- reading ---------------------------------------------------------------------

// A cursor that can only move forward and always checks first. Every read goes
// through it, so there is one place where a truncated blob is caught.
class Reader {
  public:
    explicit Reader(std::span<const std::byte> blob) : blob_(blob) {}

    [[nodiscard]] bool has(std::size_t bytes) const { return blob_.size() - at_ >= bytes; }
    [[nodiscard]] std::size_t remaining() const { return blob_.size() - at_; }

    [[nodiscard]] bool readU8(std::uint8_t& value)
    {
        if (!has(1)) {
            return false;
        }
        value = static_cast<std::uint8_t>(blob_[at_++]);
        return true;
    }

    [[nodiscard]] bool readU32(std::uint32_t& value)
    {
        if (!has(4)) {
            return false;
        }
        value = 0;
        for (int shift = 0; shift < 32; shift += 8) {
            value |= static_cast<std::uint32_t>(blob_[at_++]) << shift;
        }
        return true;
    }

    [[nodiscard]] bool readU64(std::uint64_t& value)
    {
        if (!has(8)) {
            return false;
        }
        value = 0;
        for (int shift = 0; shift < 64; shift += 8) {
            value |= static_cast<std::uint64_t>(blob_[at_++]) << shift;
        }
        return true;
    }

    // Version 3 adds the parameter and the points after SegmentMid; a
    // version-2 anchor naming one of those is malformed.
    [[nodiscard]] bool readAnchor(AnchorRef& ref, std::uint8_t version)
    {
        std::uint8_t point = 0;
        const bool withParameter = version >= kBlobVersionSmartLeader;
        const AnchorPoint last = withParameter ? kLastAnchorPoint : AnchorPoint::SegmentMid;
        if (!readU64(ref.entity) || !readU8(point) || !readU32(ref.index) ||
            point > static_cast<std::uint8_t>(last)) {
            return false;
        }
        ref.point = static_cast<AnchorPoint>(point);
        return !withParameter || readDouble(ref.parameter);
    }

    [[nodiscard]] bool readPoints(std::vector<Point2>& points)
    {
        std::uint32_t count = 0;
        if (!readU32(count) || count > kMaximumVertices ||
            !has(static_cast<std::size_t>(count) * 16)) {
            return false;
        }
        points.resize(count);
        for (Point2& point : points) {
            if (!readPoint(point)) {
                return false;
            }
        }
        return true;
    }

    [[nodiscard]] bool readDouble(double& value)
    {
        if (!has(8)) {
            return false;
        }
        std::uint64_t bits = 0;
        for (int shift = 0; shift < 64; shift += 8) {
            bits |= static_cast<std::uint64_t>(blob_[at_++]) << shift;
        }
        value = std::bit_cast<double>(bits);
        return true;
    }

    [[nodiscard]] bool readPoint(Point2& point)
    {
        return readDouble(point.x) && readDouble(point.y);
    }

    [[nodiscard]] bool readString(std::string& text)
    {
        std::uint32_t length = 0;
        if (!readU32(length) || length > kMaximumStringBytes || !has(length)) {
            return false;
        }
        text.assign(reinterpret_cast<const char*>(blob_.data() + at_), length);
        at_ += length;
        return isValidUtf8(text);
    }

  private:
    std::span<const std::byte> blob_;
    std::size_t at_ = 0;
};

[[nodiscard]] katana::core::Error truncated()
{
    return katana::core::Error{ErrorCode::ParseFailure, "geometry blob is truncated or malformed",
                               {}};
}

} // namespace

Result<std::vector<std::byte>> geometryToBlob(const Geometry& geometry)
{
    std::vector<std::byte> out;
    // Two header bytes plus the largest fixed payload; a polyline grows from
    // here. Reserving keeps a save from reallocating per entity.
    out.reserve(48);
    const std::uint8_t version = fitsVersionOne(geometry)   ? kBlobVersion
                                 : fitsVersionTwo(geometry) ? kBlobVersionAnnotation
                                                            : kBlobVersionSmartLeader;
    const bool annotation = version >= kBlobVersionAnnotation;
    putU8(out, version);
    putU8(out, static_cast<std::uint8_t>(geometry.index()));

    katana::core::Status status;
    std::visit(
        [&out, &status, annotation, version](const auto& shape) {
            using Shape = std::decay_t<decltype(shape)>;
            if constexpr (std::is_same_v<Shape, PointGeometry>) {
                putPoint(out, shape.position);
            } else if constexpr (std::is_same_v<Shape, Segment2>) {
                putPoint(out, shape.start);
                putPoint(out, shape.end);
            } else if constexpr (std::is_same_v<Shape, Arc2>) {
                putPoint(out, shape.center);
                putDouble(out, shape.radius);
                putDouble(out, shape.startAngle);
                putDouble(out, shape.sweep);
            } else if constexpr (std::is_same_v<Shape, Polyline2>) {
                if (shape.vertices.size() > kMaximumVertices) {
                    status = makeError(ErrorCode::InvalidArgument,
                                       "polyline has more vertices than the format allows",
                                       std::to_string(shape.vertices.size()));
                    return;
                }
                putU32(out, static_cast<std::uint32_t>(shape.vertices.size()));
                putU8(out, shape.closed ? 1u : 0u);
                out.reserve(out.size() + shape.vertices.size() * 16);
                for (const Point2& vertex : shape.vertices) {
                    putPoint(out, vertex);
                }
            } else if constexpr (std::is_same_v<Shape, Circle2>) {
                putPoint(out, shape.center);
                putDouble(out, shape.radius);
            } else if constexpr (std::is_same_v<Shape, TextGeometry>) {
                putPoint(out, shape.position);
                putDouble(out, shape.height);
                putDouble(out, shape.rotation);
                status = putString(out, shape.text);
                if (status && annotation) {
                    status = putString(out, shape.style);
                    putDouble(out, shape.paperHeight);
                    putU8(out, static_cast<std::uint8_t>(shape.justify));
                }
            } else if constexpr (std::is_same_v<Shape, DimensionGeometry>) {
                putPoint(out, shape.start);
                putPoint(out, shape.end);
                putDouble(out, shape.offset);
                status = putString(out, shape.textOverride);
                if (status && annotation) {
                    putU8(out, static_cast<std::uint8_t>(shape.kind));
                    putDouble(out, shape.angle);
                    putPoint(out, shape.vertex);
                    putAnchor(out, shape.startRef, version);
                    putAnchor(out, shape.endRef, version);
                    putAnchor(out, shape.vertexRef, version);
                }
            } else if constexpr (std::is_same_v<Shape, LabelGeometry>) {
                putU64(out, shape.target);
                putU32(out, static_cast<std::uint32_t>(shape.part));
                putPoint(out, shape.anchor);
                putU8(out, shape.position ? 1u : 0u);
                if (shape.position) {
                    putPoint(out, *shape.position);
                }
                for (const std::string* text :
                     {&shape.style, &shape.alignment, &shape.textOverride, &shape.rule}) {
                    if (status) {
                        status = putString(out, *text);
                    }
                }
            } else if constexpr (std::is_same_v<Shape, LeaderGeometry>) {
                status = putPoints(out, shape.vertices);
                if (status) {
                    status = putString(out, shape.text);
                }
                if (status) {
                    status = putString(out, shape.style);
                }
                putU8(out, static_cast<std::uint8_t>(shape.arrow));
                putU8(out, static_cast<std::uint8_t>(shape.callout));
                putDouble(out, shape.paperHeight);
                putDouble(out, shape.arrowSize);
                putDouble(out, shape.landing);
                putAnchor(out, shape.tipRef, version);
                if (status && version >= kBlobVersionSmartLeader) {
                    putU8(out, shape.fields ? 1u : 0u);
                    status = putString(out, shape.labelStyle);
                }
            } else if constexpr (std::is_same_v<Shape, katana::geometry::CurvePolyline2>) {
                // 32 bytes a vertex: x, y, bulge, height - a NaN height for
                // "not surveyed", which validation keeps from meaning anything
                // else (a height must be finite).
                if (shape.vertices.size() > kMaximumVertices) {
                    status = makeError(ErrorCode::InvalidArgument,
                                       "polyline has more vertices than the format allows",
                                       std::to_string(shape.vertices.size()));
                    return;
                }
                putU32(out, static_cast<std::uint32_t>(shape.vertices.size()));
                putU8(out, shape.closed ? 1u : 0u);
                out.reserve(out.size() + shape.vertices.size() * 32);
                for (const auto& vertex : shape.vertices) {
                    putPoint(out, vertex.position);
                    putDouble(out, vertex.bulge);
                    putDouble(out, vertex.height ? *vertex.height
                                                 : std::numeric_limits<double>::quiet_NaN());
                }
            } else if constexpr (std::is_same_v<Shape, katana::geometry::Ellipse2>) {
                putPoint(out, shape.center);
                putPoint(out, shape.majorAxis);
                putDouble(out, shape.ratio);
                putDouble(out, shape.startParameter);
                putDouble(out, shape.sweep);
            } else if constexpr (std::is_same_v<Shape, katana::geometry::Spline2>) {
                if (shape.degree < 0 || shape.degree > 255 ||
                    shape.knots.size() > kMaximumVertices ||
                    shape.weights.size() > kMaximumVertices) {
                    status = makeError(ErrorCode::InvalidArgument,
                                       "spline is larger than the format allows");
                    return;
                }
                putU8(out, static_cast<std::uint8_t>(shape.degree));
                status = putPoints(out, shape.controlPoints);
                for (const auto* list : {&shape.knots, &shape.weights}) {
                    putU32(out, static_cast<std::uint32_t>(list->size()));
                    for (const double value : *list) {
                        putDouble(out, value);
                    }
                }
                if (status) {
                    status = putPoints(out, shape.fitPoints);
                }
            } else {
                // Without this, a geometry kind added later fell off the end of
                // the chain, wrote a two-byte header and no payload, and the
                // SAVE reported success - destroying the data at write time and
                // surfacing it only at the next open. The compiler is a better
                // place to find that out.
                static_assert(false, "geometryToBlob has no case for this geometry kind");
            }
        },
        geometry);

    if (!status) {
        return status.error();
    }
    return out;
}

Result<Geometry> geometryFromBlob(std::span<const std::byte> blob)
{
    Reader reader(blob);
    std::uint8_t version = 0;
    std::uint8_t kind = 0;
    if (!reader.readU8(version) || !reader.readU8(kind)) {
        return truncated();
    }
    if (version != kBlobVersion && version != kBlobVersionAnnotation &&
        version != kBlobVersionSmartLeader) {
        return makeError(ErrorCode::ParseFailure, "unknown geometry blob version",
                         std::to_string(version));
    }
    const bool annotation = version >= kBlobVersionAnnotation;

    Geometry geometry;
    switch (static_cast<EntityType>(kind)) {
    case EntityType::Point: {
        PointGeometry point;
        if (!reader.readPoint(point.position)) {
            return truncated();
        }
        geometry = point;
        break;
    }
    case EntityType::Line: {
        Segment2 segment;
        if (!reader.readPoint(segment.start) || !reader.readPoint(segment.end)) {
            return truncated();
        }
        geometry = segment;
        break;
    }
    case EntityType::Arc: {
        Arc2 arc;
        if (!reader.readPoint(arc.center) || !reader.readDouble(arc.radius) ||
            !reader.readDouble(arc.startAngle) || !reader.readDouble(arc.sweep)) {
            return truncated();
        }
        geometry = arc;
        break;
    }
    case EntityType::Polyline: {
        std::uint32_t count = 0;
        std::uint8_t closed = 0;
        if (!reader.readU32(count) || !reader.readU8(closed)) {
            return truncated();
        }
        if (count > kMaximumVertices) {
            return makeError(ErrorCode::ParseFailure, "polyline vertex count is implausible",
                             std::to_string(count));
        }
        // The length is checked against what is actually left BEFORE reserving,
        // so a corrupt count cannot turn into a huge allocation.
        if (!reader.has(static_cast<std::size_t>(count) * 16)) {
            return truncated();
        }
        Polyline2 polyline;
        polyline.closed = closed != 0;
        polyline.vertices.resize(count);
        for (std::uint32_t i = 0; i < count; ++i) {
            if (!reader.readPoint(polyline.vertices[i])) {
                return truncated();
            }
        }
        geometry = std::move(polyline);
        break;
    }
    case EntityType::Circle: {
        Circle2 circle;
        if (!reader.readPoint(circle.center) || !reader.readDouble(circle.radius)) {
            return truncated();
        }
        geometry = circle;
        break;
    }
    case EntityType::Text: {
        TextGeometry text;
        if (!reader.readPoint(text.position) || !reader.readDouble(text.height) ||
            !reader.readDouble(text.rotation) || !reader.readString(text.text)) {
            return truncated();
        }
        if (annotation) {
            std::uint8_t justify = 0;
            if (!reader.readString(text.style) || !reader.readDouble(text.paperHeight) ||
                !reader.readU8(justify) ||
                justify > static_cast<std::uint8_t>(TextJustify::TopRight)) {
                return truncated();
            }
            text.justify = static_cast<TextJustify>(justify);
        }
        geometry = std::move(text);
        break;
    }
    case EntityType::Dimension: {
        DimensionGeometry dimension;
        if (!reader.readPoint(dimension.start) || !reader.readPoint(dimension.end) ||
            !reader.readDouble(dimension.offset) || !reader.readString(dimension.textOverride)) {
            return truncated();
        }
        if (annotation) {
            std::uint8_t kindByte = 0;
            if (!reader.readU8(kindByte) ||
                kindByte > static_cast<std::uint8_t>(DimensionKind::OrdinateY) ||
                !reader.readDouble(dimension.angle) || !reader.readPoint(dimension.vertex) ||
                !reader.readAnchor(dimension.startRef, version) ||
                !reader.readAnchor(dimension.endRef, version) ||
                !reader.readAnchor(dimension.vertexRef, version)) {
                return truncated();
            }
            dimension.kind = static_cast<DimensionKind>(kindByte);
        }
        geometry = std::move(dimension);
        break;
    }
    case EntityType::Label: {
        if (!annotation) {
            return makeError(ErrorCode::ParseFailure, "a label in a version-1 geometry blob");
        }
        LabelGeometry label;
        std::uint32_t part = 0;
        std::uint8_t hasPosition = 0;
        if (!reader.readU64(label.target) || !reader.readU32(part) ||
            !reader.readPoint(label.anchor) || !reader.readU8(hasPosition)) {
            return truncated();
        }
        label.part = static_cast<std::int32_t>(part);
        if (hasPosition != 0) {
            Point2 position;
            if (!reader.readPoint(position)) {
                return truncated();
            }
            label.position = position;
        }
        if (!reader.readString(label.style) || !reader.readString(label.alignment) ||
            !reader.readString(label.textOverride) || !reader.readString(label.rule)) {
            return truncated();
        }
        geometry = std::move(label);
        break;
    }
    case EntityType::Leader: {
        if (!annotation) {
            return makeError(ErrorCode::ParseFailure, "a leader in a version-1 geometry blob");
        }
        LeaderGeometry leader;
        std::uint8_t arrow = 0;
        std::uint8_t callout = 0;
        if (!reader.readPoints(leader.vertices) || !reader.readString(leader.text) ||
            !reader.readString(leader.style) || !reader.readU8(arrow) || !reader.readU8(callout) ||
            !reader.readDouble(leader.paperHeight) || !reader.readDouble(leader.arrowSize) ||
            !reader.readDouble(leader.landing) || !reader.readAnchor(leader.tipRef, version) ||
            arrow > static_cast<std::uint8_t>(ArrowHead::Dot) ||
            callout > static_cast<std::uint8_t>(CalloutShape::Circle)) {
            return truncated();
        }
        if (version >= kBlobVersionSmartLeader) {
            std::uint8_t fields = 0;
            if (!reader.readU8(fields) || fields > 1 || !reader.readString(leader.labelStyle)) {
                return truncated();
            }
            leader.fields = fields != 0;
        }
        leader.arrow = static_cast<ArrowHead>(arrow);
        leader.callout = static_cast<CalloutShape>(callout);
        geometry = std::move(leader);
        break;
    }
    case EntityType::CurvePolyline: {
        if (!annotation) {
            return makeError(ErrorCode::ParseFailure,
                             "a curve polyline in a version-1 geometry blob");
        }
        std::uint32_t count = 0;
        std::uint8_t closed = 0;
        if (!reader.readU32(count) || !reader.readU8(closed)) {
            return truncated();
        }
        if (count > kMaximumVertices) {
            return makeError(ErrorCode::ParseFailure, "polyline vertex count is implausible",
                             std::to_string(count));
        }
        if (!reader.has(static_cast<std::size_t>(count) * 32)) {
            return truncated();
        }
        katana::geometry::CurvePolyline2 polyline;
        polyline.closed = closed != 0;
        polyline.vertices.resize(count);
        for (auto& vertex : polyline.vertices) {
            double height = 0.0;
            if (!reader.readPoint(vertex.position) || !reader.readDouble(vertex.bulge) ||
                !reader.readDouble(height)) {
                return truncated();
            }
            if (!std::isnan(height)) {
                vertex.height = height;
            }
        }
        geometry = std::move(polyline);
        break;
    }
    case EntityType::Ellipse: {
        if (!annotation) {
            return makeError(ErrorCode::ParseFailure, "an ellipse in a version-1 geometry blob");
        }
        katana::geometry::Ellipse2 ellipse;
        if (!reader.readPoint(ellipse.center) || !reader.readPoint(ellipse.majorAxis) ||
            !reader.readDouble(ellipse.ratio) || !reader.readDouble(ellipse.startParameter) ||
            !reader.readDouble(ellipse.sweep)) {
            return truncated();
        }
        geometry = ellipse;
        break;
    }
    case EntityType::Spline: {
        if (!annotation) {
            return makeError(ErrorCode::ParseFailure, "a spline in a version-1 geometry blob");
        }
        katana::geometry::Spline2 spline;
        std::uint8_t degree = 0;
        if (!reader.readU8(degree) || !reader.readPoints(spline.controlPoints)) {
            return truncated();
        }
        spline.degree = degree;
        for (auto* list : {&spline.knots, &spline.weights}) {
            std::uint32_t count = 0;
            if (!reader.readU32(count) || count > kMaximumVertices ||
                !reader.has(static_cast<std::size_t>(count) * 8)) {
                return truncated();
            }
            list->resize(count);
            for (double& value : *list) {
                if (!reader.readDouble(value)) {
                    return truncated();
                }
            }
        }
        if (!reader.readPoints(spline.fitPoints)) {
            return truncated();
        }
        geometry = std::move(spline);
        break;
    }
    default:
        return makeError(ErrorCode::ParseFailure, "unknown geometry kind in blob",
                         std::to_string(kind));
    }

    if (reader.remaining() != 0) {
        // Not ignored: trailing bytes mean the writer and this reader disagree
        // about the layout, and every later field would be decoded wrongly.
        return makeError(ErrorCode::ParseFailure, "geometry blob has trailing bytes",
                         std::to_string(reader.remaining()));
    }
    return geometry;
}

} // namespace katana::entity
