#include "katana/entity/entity_geometry.hpp"

#include <algorithm>
#include <charconv>
#include <cmath>
#include <limits>
#include <string>

#include "katana/core/text.hpp"
#include "katana/entity/leader_values.hpp"
#include "katana/entity/text_block.hpp"

namespace katana::entity {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Arc2;
using katana::geometry::Box2;
using katana::geometry::Circle2;
using katana::geometry::CurvePolyline2;
using katana::geometry::CurveVertex;
using katana::geometry::Ellipse2;
using katana::geometry::Spline2;
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

void setHeights(PropertyMap& properties, const std::vector<std::optional<double>>& heights)
{
    for (const std::string_view key : {kElevationProperty, kElevationsProperty}) {
        if (const auto found = properties.find(key); found != properties.end()) {
            properties.erase(found);
        }
    }
    const auto known = [](const std::optional<double>& z) {
        return z.has_value() && std::isfinite(*z);
    };
    if (std::none_of(heights.begin(), heights.end(), known)) {
        return;
    }
    const bool same = std::all_of(heights.begin(), heights.end(), [&](const auto& z) {
        return known(z) && *z == *heights.front();
    });
    if (same) {
        properties.insert_or_assign(std::string(kElevationProperty), *heights.front());
        return;
    }
    std::string list;
    for (const auto& z : heights) {
        if (!list.empty()) {
            list += ' ';
        }
        list += known(z) ? katana::core::formatExactReal(*z) : std::string("null");
    }
    properties.insert_or_assign(std::string(kElevationsProperty), std::move(list));
}

std::vector<std::optional<double>> heightsOf(const PropertyMap& properties, std::size_t count)
{
    if (const auto found = properties.find(kElevationsProperty); found != properties.end()) {
        if (const auto* list = std::get_if<std::string>(&found->second)) {
            std::vector<std::optional<double>> parsed;
            std::size_t start = 0;
            while (start < list->size()) {
                while (start < list->size() && katana::core::isAsciiSpace((*list)[start])) {
                    ++start;
                }
                std::size_t end = start;
                while (end < list->size() && !katana::core::isAsciiSpace((*list)[end])) {
                    ++end;
                }
                if (end > start) {
                    // "null", or anything else that is not a finite number, is
                    // no height.
                    parsed.push_back(katana::core::parseFiniteDouble(
                        std::string_view(*list).substr(start, end - start)));
                }
                start = end;
            }
            if (parsed.size() == count) {
                return parsed;
            }
        }
    }
    std::vector<std::optional<double>> heights(count);
    if (const auto found = properties.find(kElevationProperty); found != properties.end()) {
        std::optional<double> z;
        if (const auto* real = std::get_if<double>(&found->second)) {
            z = *real;
        } else if (const auto* integer = std::get_if<std::int64_t>(&found->second)) {
            z = static_cast<double>(*integer);
        }
        if (z && std::isfinite(*z)) {
            std::fill(heights.begin(), heights.end(), z);
        }
    }
    return heights;
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
    case EntityType::Label:
        return "Label";
    case EntityType::Leader:
        return "Leader";
    case EntityType::CurvePolyline:
        return "CurvePolyline";
    case EntityType::Ellipse:
        return "Ellipse";
    case EntityType::Spline:
        return "Spline";
    }
    return "Unknown";
}

// ---- the annotation enumerations -------------------------------------------------------

namespace {

// Case-insensitive ASCII comparison, ignoring '-', '_' and spaces, so that
// "middle-centre", "MiddleCentre" and "middle centre" are one name.
bool sameName(std::string_view a, std::string_view b)
{
    const auto next = [](std::string_view s, std::size_t& i) -> int {
        while (i < s.size() && (s[i] == '-' || s[i] == '_' || s[i] == ' ')) {
            ++i;
        }
        if (i == s.size()) {
            return -1;
        }
        char c = s[i++];
        if (c >= 'A' && c <= 'Z') {
            c = static_cast<char>(c - 'A' + 'a');
        }
        return static_cast<unsigned char>(c);
    };
    std::size_t i = 0;
    std::size_t j = 0;
    while (true) {
        const int x = next(a, i);
        const int y = next(b, j);
        if (x != y) {
            return false;
        }
        if (x < 0) {
            return true;
        }
    }
}

} // namespace

std::string_view toString(TextJustify justify)
{
    switch (justify) {
    case TextJustify::BottomLeft:
        return "BL";
    case TextJustify::BottomCentre:
        return "BC";
    case TextJustify::BottomRight:
        return "BR";
    case TextJustify::MiddleLeft:
        return "ML";
    case TextJustify::MiddleCentre:
        return "MC";
    case TextJustify::MiddleRight:
        return "MR";
    case TextJustify::TopLeft:
        return "TL";
    case TextJustify::TopCentre:
        return "TC";
    case TextJustify::TopRight:
        return "TR";
    }
    return "BL";
}

Result<TextJustify> textJustifyFromString(std::string_view name)
{
    struct Name {
        std::string_view text;
        TextJustify justify;
    };
    // The short names, the long ones in both spellings of centre, and the
    // baseline row's one-letter forms.
    static constexpr Name kNames[] = {
        {"BL", TextJustify::BottomLeft},      {"BC", TextJustify::BottomCentre},
        {"BR", TextJustify::BottomRight},     {"ML", TextJustify::MiddleLeft},
        {"MC", TextJustify::MiddleCentre},    {"MR", TextJustify::MiddleRight},
        {"TL", TextJustify::TopLeft},         {"TC", TextJustify::TopCentre},
        {"TR", TextJustify::TopRight},        {"L", TextJustify::BottomLeft},
        {"C", TextJustify::BottomCentre},     {"R", TextJustify::BottomRight},
        {"M", TextJustify::MiddleCentre},     {"BottomLeft", TextJustify::BottomLeft},
        {"BottomCentre", TextJustify::BottomCentre}, {"BottomCenter", TextJustify::BottomCentre},
        {"BottomRight", TextJustify::BottomRight},   {"MiddleLeft", TextJustify::MiddleLeft},
        {"MiddleCentre", TextJustify::MiddleCentre}, {"MiddleCenter", TextJustify::MiddleCentre},
        {"MiddleRight", TextJustify::MiddleRight},   {"TopLeft", TextJustify::TopLeft},
        {"TopCentre", TextJustify::TopCentre},       {"TopCenter", TextJustify::TopCentre},
        {"TopRight", TextJustify::TopRight},
    };
    for (const Name& candidate : kNames) {
        if (sameName(candidate.text, name)) {
            return candidate.justify;
        }
    }
    return makeError(ErrorCode::ParseFailure,
                     "unknown justification (TL TC TR ML MC MR BL BC BR)", std::string(name));
}

std::string_view toString(AnchorPoint point)
{
    switch (point) {
    case AnchorPoint::Position:
        return "position";
    case AnchorPoint::Start:
        return "start";
    case AnchorPoint::End:
        return "end";
    case AnchorPoint::Mid:
        return "mid";
    case AnchorPoint::Centre:
        return "centre";
    case AnchorPoint::Vertex:
        return "vertex";
    case AnchorPoint::SegmentMid:
        return "segment-mid";
    case AnchorPoint::Along:
        return "along";
    case AnchorPoint::Inside:
        return "inside";
    }
    return "position";
}

Result<AnchorPoint> anchorPointFromString(std::string_view name)
{
    for (const AnchorPoint point :
         {AnchorPoint::Position, AnchorPoint::Start, AnchorPoint::End, AnchorPoint::Mid,
          AnchorPoint::Centre, AnchorPoint::Vertex, AnchorPoint::SegmentMid, AnchorPoint::Along,
          AnchorPoint::Inside}) {
        if (sameName(toString(point), name)) {
            return point;
        }
    }
    if (sameName(name, "center")) {
        return AnchorPoint::Centre;
    }
    return makeError(ErrorCode::ParseFailure, "unknown anchor point", std::string(name));
}

std::string_view toString(DimensionKind kind)
{
    switch (kind) {
    case DimensionKind::Aligned:
        return "aligned";
    case DimensionKind::Linear:
        return "linear";
    case DimensionKind::Angular:
        return "angular";
    case DimensionKind::Radius:
        return "radius";
    case DimensionKind::Diameter:
        return "diameter";
    case DimensionKind::OrdinateX:
        return "ordinate-x";
    case DimensionKind::OrdinateY:
        return "ordinate-y";
    }
    return "aligned";
}

Result<DimensionKind> dimensionKindFromString(std::string_view name)
{
    for (const DimensionKind kind :
         {DimensionKind::Aligned, DimensionKind::Linear, DimensionKind::Angular,
          DimensionKind::Radius, DimensionKind::Diameter, DimensionKind::OrdinateX,
          DimensionKind::OrdinateY}) {
        if (sameName(toString(kind), name)) {
            return kind;
        }
    }
    return makeError(ErrorCode::ParseFailure, "unknown dimension kind", std::string(name));
}

std::string_view toString(CalloutShape shape)
{
    switch (shape) {
    case CalloutShape::None:
        return "none";
    case CalloutShape::Box:
        return "box";
    case CalloutShape::Circle:
        return "circle";
    }
    return "none";
}

Result<CalloutShape> calloutShapeFromString(std::string_view name)
{
    for (const CalloutShape shape : {CalloutShape::None, CalloutShape::Box, CalloutShape::Circle}) {
        if (sameName(toString(shape), name)) {
            return shape;
        }
    }
    if (sameName(name, "balloon")) {
        return CalloutShape::Circle;
    }
    return makeError(ErrorCode::ParseFailure, "unknown callout shape (none box circle)",
                     std::string(name));
}

// ---- DimensionGeometry --------------------------------------------------------------------

double DimensionGeometry::measurement() const
{
    const Vec2 along(std::cos(angle), std::sin(angle));
    switch (kind) {
    case DimensionKind::Aligned:
        return start.distanceTo(end);
    case DimensionKind::Linear:
        return std::abs((end - start).dot(along));
    case DimensionKind::Angular: {
        const Vec2 first = start - vertex;
        const Vec2 second = end - vertex;
        double swept = std::atan2(first.cross(second), first.dot(second));
        if (swept < 0.0) {
            swept += katana::math::kTwoPi;
        }
        return swept;
    }
    case DimensionKind::Radius:
        return start.distanceTo(vertex);
    case DimensionKind::Diameter:
        return 2.0 * start.distanceTo(vertex);
    case DimensionKind::OrdinateX:
        return (start - vertex).dot(along);
    case DimensionKind::OrdinateY:
        return (start - vertex).dot(along.perpendicular());
    }
    return start.distanceTo(end);
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
        // In any case: callers once title-cased the name to the enumerator's
        // spelling, which turned "curvepolyline" into "Curvepolyline" and
        // refused the one kind with a capital inside its name.
        if (katana::core::equalsIgnoringCase(toString(type), name)) {
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
                         katana::core::formatExactReal(value));
    }
    return {};
}

// A reference's point is one there is, and only Along has a parameter: a
// fraction of the way along, finite and in [0, 1]. Every other point is
// exactly 0 there, so an anchor that needs no parameter is written as it
// always was (geometry_blob.cpp).
Status validateAnchor(const AnchorRef& ref)
{
    if (static_cast<int>(ref.point) > static_cast<int>(kLastAnchorPoint)) {
        return makeError(ErrorCode::InvalidGeometry, "unknown anchor point");
    }
    if (ref.point == AnchorPoint::Along) {
        if (!(std::isfinite(ref.parameter) && ref.parameter >= 0.0 && ref.parameter <= 1.0)) {
            return makeError(ErrorCode::InvalidGeometry,
                             "an anchor along an entity is a fraction from 0 to 1",
                             katana::core::formatExactReal(ref.parameter));
        }
    } else if (ref.parameter != 0.0) {
        return makeError(ErrorCode::InvalidGeometry,
                         "only an anchor along an entity has a fraction",
                         std::string(toString(ref.point)));
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
        if (!isValidUtf8(text.style)) {
            return makeError(ErrorCode::InvalidGeometry, "text style name is not valid UTF-8");
        }
        if (!(std::isfinite(text.paperHeight) && text.paperHeight >= 0.0)) {
            return makeError(ErrorCode::InvalidGeometry,
                             "text paper height must be zero or positive",
                             katana::core::formatExactReal(text.paperHeight));
        }
        if (static_cast<int>(text.justify) > static_cast<int>(TextJustify::TopRight)) {
            return makeError(ErrorCode::InvalidGeometry, "text justification is not one of the nine");
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
        if (auto status = requireFinite(dimension.vertex, "dimension vertex"); !status) {
            return status;
        }
        if (!std::isfinite(dimension.offset) || !std::isfinite(dimension.angle)) {
            return makeError(ErrorCode::InvalidGeometry, "dimension offset or angle is not finite");
        }
        if (static_cast<int>(dimension.kind) > static_cast<int>(DimensionKind::OrdinateY)) {
            return makeError(ErrorCode::InvalidGeometry, "unknown dimension kind");
        }
        for (const AnchorRef* ref : {&dimension.startRef, &dimension.endRef, &dimension.vertexRef}) {
            if (auto status = validateAnchor(*ref); !status) {
                return status;
            }
        }
        switch (dimension.kind) {
        case DimensionKind::Aligned:
        case DimensionKind::Linear:
        case DimensionKind::Radius:
        case DimensionKind::Diameter:
            if (!(dimension.measurement() > tol::kGeometric)) {
                return makeError(ErrorCode::InvalidGeometry, "dimension measures zero length");
            }
            break;
        case DimensionKind::Angular:
            if (!(dimension.start.distanceTo(dimension.vertex) > tol::kGeometric) ||
                !(dimension.end.distanceTo(dimension.vertex) > tol::kGeometric)) {
                return makeError(ErrorCode::InvalidGeometry,
                                 "an angular dimension's rays need a length");
            }
            if (!(dimension.measurement() > tol::kAngular)) {
                return makeError(ErrorCode::InvalidGeometry, "dimension measures zero angle");
            }
            break;
        case DimensionKind::OrdinateX:
        case DimensionKind::OrdinateY:
            // A feature ON the datum measures 0, which is a value like any
            // other; what an ordinate needs is a leader to put the value at.
            if (!(dimension.start.distanceTo(dimension.end) > tol::kGeometric)) {
                return makeError(ErrorCode::InvalidGeometry,
                                 "an ordinate dimension's leader has zero length");
            }
            break;
        }
        return {};
    }
    Status operator()(const LabelGeometry& label) const
    {
        if (label.style.empty() || !isValidUtf8(label.style)) {
            return makeError(ErrorCode::InvalidGeometry, "a label needs a label style");
        }
        if ((label.target == 0) == label.alignment.empty()) {
            return makeError(ErrorCode::InvalidGeometry,
                             "a label labels exactly one entity or one alignment");
        }
        if (!isValidUtf8(label.alignment) || !isValidUtf8(label.textOverride) ||
            !isValidUtf8(label.rule)) {
            return makeError(ErrorCode::InvalidGeometry, "label text is not valid UTF-8");
        }
        if (label.part < -1) {
            return makeError(ErrorCode::InvalidGeometry, "a label's part is -1 or a piece index",
                             std::to_string(label.part));
        }
        if (auto status = requireFinite(label.anchor, "label anchor"); !status) {
            return status;
        }
        if (label.position) {
            return requireFinite(*label.position, "label position");
        }
        return {};
    }
    Status operator()(const LeaderGeometry& leader) const
    {
        if (leader.vertices.size() < 2) {
            return makeError(ErrorCode::InvalidGeometry, "a leader needs at least two vertices");
        }
        for (const Point2& vertex : leader.vertices) {
            if (auto status = requireFinite(vertex, "leader vertex"); !status) {
                return status;
            }
        }
        if (!(Polyline2{leader.vertices, false}.length() > tol::kGeometric)) {
            return makeError(ErrorCode::InvalidGeometry, "leader has zero length");
        }
        if (!isValidUtf8(leader.text) || !isValidUtf8(leader.style) ||
            !isValidUtf8(leader.labelStyle)) {
            return makeError(ErrorCode::InvalidGeometry, "leader text is not valid UTF-8");
        }
        for (const double size : {leader.paperHeight, leader.arrowSize, leader.landing}) {
            if (!(std::isfinite(size) && size >= 0.0)) {
                return makeError(ErrorCode::InvalidGeometry,
                                 "a leader's sizes must be zero or positive",
                                 katana::core::formatExactReal(size));
            }
        }
        if (static_cast<int>(leader.arrow) > static_cast<int>(ArrowHead::Dot) ||
            static_cast<int>(leader.callout) > static_cast<int>(CalloutShape::Circle)) {
            return makeError(ErrorCode::InvalidGeometry, "a leader's arrow or callout is unknown");
        }
        if (auto status = validateAnchor(leader.tipRef); !status) {
            return status;
        }
        // A smart leader (docs/annotation.md, "Smart leaders"): its note is
        // read off the entity its tip names, from ONE template - its own or
        // its label style's.
        if (leader.fields && !leader.labelStyle.empty()) {
            return makeError(ErrorCode::InvalidGeometry,
                             "a leader's note is its own template or its label style's, not both");
        }
        if (!leader.labelStyle.empty() && !leader.text.empty()) {
            return makeError(ErrorCode::InvalidGeometry,
                             "a leader in a label style takes its note from the style; it has no "
                             "text of its own",
                             leader.labelStyle);
        }
        if ((leader.fields || !leader.labelStyle.empty()) && !leader.tipRef.associated()) {
            return makeError(
                ErrorCode::InvalidGeometry,
                "a smart leader reads the entity its tip is on, and its tip names none");
        }
        if (leader.fields) {
            if (leader.text.empty()) {
                return makeError(ErrorCode::InvalidGeometry, "a smart leader's template is empty");
            }
            if (auto status = checkLeaderTemplate(leader.text); !status) {
                return makeError(ErrorCode::InvalidGeometry, status.error().message,
                                 status.error().context);
            }
        }
        return {};
    }
    // A polyline with arcs may close on two vertices - two arcs make a
    // round shape - so, like Polyline2, only the vertex count of an open
    // walk and a length are required.
    Status operator()(const CurvePolyline2& polyline) const
    {
        if (polyline.vertices.size() < 2) {
            return makeError(ErrorCode::InvalidGeometry, "polyline needs at least two vertices");
        }
        for (const CurveVertex& vertex : polyline.vertices) {
            if (auto status = requireFinite(vertex.position, "polyline vertex"); !status) {
                return status;
            }
            if (!std::isfinite(vertex.bulge)) {
                return makeError(ErrorCode::InvalidGeometry, "polyline bulge is not finite");
            }
            if (vertex.height && !std::isfinite(*vertex.height)) {
                return makeError(ErrorCode::InvalidGeometry, "polyline height is not finite");
            }
        }
        if (!(polyline.length() > tol::kGeometric)) {
            return makeError(ErrorCode::InvalidGeometry, "polyline has zero length");
        }
        return {};
    }
    Status operator()(const Ellipse2& ellipse) const
    {
        if (auto status = requireFinite(ellipse.center, "ellipse centre"); !status) {
            return status;
        }
        if (!ellipse.majorAxis.isFinite() || !(ellipse.majorRadius() > tol::kGeometric)) {
            return makeError(ErrorCode::InvalidGeometry, "ellipse major axis must be positive");
        }
        if (!(std::isfinite(ellipse.ratio) && ellipse.ratio * ellipse.majorRadius() > tol::kGeometric &&
              ellipse.ratio <= 1.0 + tol::kRelative)) {
            return makeError(ErrorCode::InvalidGeometry,
                             "ellipse ratio must be greater than zero and at most 1",
                             katana::core::formatExactReal(ellipse.ratio));
        }
        if (!std::isfinite(ellipse.startParameter) || !std::isfinite(ellipse.sweep) ||
            !(ellipse.sweep > tol::kAngular) ||
            ellipse.sweep > katana::math::kTwoPi + tol::kAngular) {
            return makeError(ErrorCode::InvalidGeometry,
                             "ellipse sweep must be more than zero and at most a full turn");
        }
        return {};
    }
    Status operator()(const Spline2& spline) const
    {
        if (auto status = spline.checkStructure(); !status) {
            return status;
        }
        for (const Point2& p : spline.controlPoints) {
            if (auto status = requireFinite(p, "spline control point"); !status) {
                return status;
            }
        }
        for (const Point2& p : spline.fitPoints) {
            if (auto status = requireFinite(p, "spline fit point"); !status) {
                return status;
            }
        }
        if (!(spline.boundingBox().width() + spline.boundingBox().height() > tol::kGeometric)) {
            return makeError(ErrorCode::InvalidGeometry, "spline has zero length");
        }
        return {};
    }
};

// The lines a dimension draws its measurement along, for its box and for
// picking: the dimension line (the arc of an angular one, chorded), the
// radial line, or an ordinate's leader. Not what cad/dimension_draw.hpp
// draws - no extension lines, arrows or text - which is why boundingBox
// documents a dimension's box as its measured points and this.
std::vector<Segment2> dimensionLines(const DimensionGeometry& dimension)
{
    const Vec2 along(std::cos(dimension.angle), std::sin(dimension.angle));
    switch (dimension.kind) {
    case DimensionKind::Aligned: {
        const Vec2 shift =
            (dimension.end - dimension.start).normalized().perpendicular() * dimension.offset;
        return {Segment2{dimension.start + shift, dimension.end + shift}};
    }
    case DimensionKind::Linear: {
        const Point2 first = dimension.start + along.perpendicular() * dimension.offset;
        return {Segment2{first, first + along * (dimension.end - dimension.start).dot(along)}};
    }
    case DimensionKind::Angular: {
        const double radius = dimension.offset > tol::kGeometric
                                  ? dimension.offset
                                  : std::min(dimension.start.distanceTo(dimension.vertex),
                                             dimension.end.distanceTo(dimension.vertex));
        const double from = (dimension.start - dimension.vertex).angle();
        const Arc2 arc{dimension.vertex, radius, from, dimension.measurement()};
        constexpr int kChords = 32;
        std::vector<Segment2> lines;
        lines.reserve(kChords);
        for (int i = 0; i < kChords; ++i) {
            lines.push_back(Segment2{arc.pointAt(static_cast<double>(i) / kChords),
                                     arc.pointAt(static_cast<double>(i + 1) / kChords)});
        }
        return lines;
    }
    case DimensionKind::Radius:
    case DimensionKind::Diameter: {
        const Vec2 out = (dimension.start - dimension.vertex).normalized();
        const Point2 from = dimension.kind == DimensionKind::Diameter
                                ? dimension.vertex - (dimension.start - dimension.vertex)
                                : dimension.vertex;
        return {Segment2{from, dimension.start + out * std::max(dimension.offset, 0.0)}};
    }
    case DimensionKind::OrdinateX:
    case DimensionKind::OrdinateY:
        return {Segment2{dimension.start, dimension.end}};
    }
    return {};
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
            Box2 box;
            for (const Point2& corner : estimatedTextCorners(text)) {
                box.expand(corner);
            }
            return box;
        }
        Box2 operator()(const DimensionGeometry& dimension) const
        {
            Box2 box;
            for (const Segment2& line : dimensionLines(dimension)) {
                box.expand(line.start);
                box.expand(line.end);
            }
            box.expand(dimension.start);
            box.expand(dimension.end);
            if (dimension.usesVertex()) {
                box.expand(dimension.vertex);
            }
            return box;
        }
        // Only the anchor and a dragged position: the text is worked out
        // where it is drawn (LabelGeometry says why), and the painters cull
        // labels by their targets, not by this.
        Box2 operator()(const LabelGeometry& label) const
        {
            Box2 box;
            box.expand(label.anchor);
            if (label.position) {
                box.expand(*label.position);
            }
            return box;
        }
        // The line only: the note's size is in paper millimetres, so the
        // model box it covers depends on the scale it is looked at.
        Box2 operator()(const LeaderGeometry& leader) const
        {
            return Polyline2{leader.vertices, false}.boundingBox();
        }
        Box2 operator()(const CurvePolyline2& polyline) const { return polyline.boundingBox(); }
        Box2 operator()(const Ellipse2& ellipse) const { return ellipse.boundingBox(); }
        Box2 operator()(const Spline2& spline) const { return spline.boundingBox(); }
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
        // To the block's bottom edge - for one line justified BottomLeft,
        // the baseline, which is what a text was always picked by.
        double operator()(const TextGeometry& text) const
        {
            const auto corners = estimatedTextCorners(text);
            return Segment2{corners[0], corners[1]}.distanceTo(p);
        }
        double operator()(const DimensionGeometry& dimension) const
        {
            double nearest = std::numeric_limits<double>::infinity();
            for (const Segment2& line : dimensionLines(dimension)) {
                nearest = std::min(nearest, line.distanceTo(p));
            }
            return nearest;
        }
        double operator()(const LabelGeometry& label) const
        {
            double nearest = label.anchor.distanceTo(p);
            if (label.position) {
                nearest = std::min(nearest, label.position->distanceTo(p));
            }
            return nearest;
        }
        double operator()(const LeaderGeometry& leader) const
        {
            return Polyline2{leader.vertices, false}.distanceTo(p).value_or(
                std::numeric_limits<double>::infinity());
        }
        double operator()(const CurvePolyline2& polyline) const
        {
            return polyline.distanceTo(p).value_or(std::numeric_limits<double>::infinity());
        }
        double operator()(const Ellipse2& ellipse) const { return ellipse.distanceTo(p); }
        double operator()(const Spline2& spline) const { return spline.distanceTo(p); }
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
            result.vertex = s.point(dimension.vertex);
            result.angle = s.angle(dimension.angle);
            switch (dimension.kind) {
            case DimensionKind::Aligned:
            case DimensionKind::Linear:
                // The dimension line is on the other side of its direction
                // once mirrored, so its signed offset changes sign.
                result.offset = dimension.offset * s.scale * (s.mirrored ? -1.0 : 1.0);
                break;
            case DimensionKind::Angular:
                result.offset = dimension.offset * s.scale;
                // A mirror turns the counter-clockwise sweep from start to end
                // clockwise: swapping the rays measures the same angle again.
                if (s.mirrored) {
                    std::swap(result.start, result.end);
                    std::swap(result.startRef, result.endRef);
                }
                break;
            case DimensionKind::Radius:
            case DimensionKind::Diameter:
            case DimensionKind::OrdinateX:
            case DimensionKind::OrdinateY:
                result.offset = dimension.offset * s.scale;
                break;
            }
            return result;
        }
        Geometry operator()(const LabelGeometry& label) const
        {
            LabelGeometry result = label;
            result.anchor = s.point(label.anchor);
            if (label.position) {
                result.position = s.point(*label.position);
            }
            return result;
        }
        // The sizes are paper millimetres and stay as they are at any scale.
        Geometry operator()(const LeaderGeometry& leader) const
        {
            LeaderGeometry result = leader;
            for (Point2& vertex : result.vertices) {
                vertex = s.point(vertex);
            }
            return result;
        }
        // Heights are elevations, not plan distances: a plan scale leaves
        // them alone. A mirror turns every arc the other way.
        Geometry operator()(const CurvePolyline2& polyline) const
        {
            CurvePolyline2 result = polyline;
            for (CurveVertex& vertex : result.vertices) {
                vertex.position = s.point(vertex.position);
                if (s.mirrored) {
                    vertex.bulge = -vertex.bulge;
                }
            }
            return result;
        }
        // Mirrored, the point at t lands where the image's point at -t is
        // (the minor axis flips relative to the major), so the arc is
        // re-expressed as starting at -(start + sweep) rather than stored as
        // a negative sweep.
        Geometry operator()(const Ellipse2& ellipse) const
        {
            Ellipse2 result = ellipse;
            result.center = s.point(ellipse.center);
            result.majorAxis = transformVector(s.matrix, ellipse.majorAxis);
            if (s.mirrored) {
                result.startParameter = -(ellipse.startParameter + ellipse.sweep);
            }
            return result;
        }
        Geometry operator()(const Spline2& spline) const
        {
            Spline2 result = spline;
            for (Point2& p : result.controlPoints) {
                p = s.point(p);
            }
            for (Point2& p : result.fitPoints) {
                p = s.point(p);
            }
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
