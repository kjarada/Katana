#include "katana/entity/geometry_blob.hpp"

#include <bit>
#include <cstring>
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
    putU8(out, kBlobVersion);
    putU8(out, static_cast<std::uint8_t>(geometry.index()));

    katana::core::Status status;
    std::visit(
        [&out, &status](const auto& shape) {
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
            } else if constexpr (std::is_same_v<Shape, DimensionGeometry>) {
                putPoint(out, shape.start);
                putPoint(out, shape.end);
                putDouble(out, shape.offset);
                status = putString(out, shape.textOverride);
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
    if (version != kBlobVersion) {
        return makeError(ErrorCode::ParseFailure, "unknown geometry blob version",
                         std::to_string(version));
    }

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
        geometry = std::move(text);
        break;
    }
    case EntityType::Dimension: {
        DimensionGeometry dimension;
        if (!reader.readPoint(dimension.start) || !reader.readPoint(dimension.end) ||
            !reader.readDouble(dimension.offset) || !reader.readString(dimension.textOverride)) {
            return truncated();
        }
        geometry = std::move(dimension);
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
