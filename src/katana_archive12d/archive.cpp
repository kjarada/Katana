#include "katana/archive12d/archive.hpp"

#include <algorithm>
#include <charconv>
#include <cmath>
#include <utility>

#include "katana/archive12d/reader.hpp"
#include "text_utilities.hpp"

namespace katana::archive12d {

bool operator==(const Attribute& a, const Attribute& b)
{
    return a.name == b.name && a.declaredType == b.declaredType && a.value == b.value;
}

// ---- FieldList -------------------------------------------------------------

void FieldList::add(std::string key, std::string value, bool quoted)
{
    fields_.push_back(Field{std::move(key), std::move(value), quoted});
}

void FieldList::setReal(std::string_view key, double value)
{
    // Shortest text that reads back as the same double: a field that has been
    // through a FieldList must not come out a different number.
    char buffer[32];
    const auto result = std::to_chars(buffer, buffer + sizeof buffer, value);
    add(std::string(key), std::string(buffer, result.ptr));
}

void FieldList::setInteger(std::string_view key, std::int64_t value)
{
    add(std::string(key), std::to_string(value));
}

void FieldList::setText(std::string_view key, std::string value)
{
    add(std::string(key), std::move(value), true);
}

std::optional<Field> FieldList::take(std::string_view key)
{
    std::optional<Field> taken;
    for (auto it = fields_.begin(); it != fields_.end();) {
        if (it->key == key) {
            taken = std::move(*it);
            it = fields_.erase(it);
        } else {
            ++it;
        }
    }
    return taken;
}

const Field* FieldList::find(std::string_view key) const
{
    for (auto it = fields_.rbegin(); it != fields_.rend(); ++it) {
        if (it->key == key) {
            return &*it;
        }
    }
    return nullptr;
}

// A quoted value is still taken as a number when it reads as one: `xstart
// "500005"` is a coordinate somebody quoted, and refusing it would place the
// arc nowhere. The `null` keyword, quoted or not, is no number.
std::optional<double> FieldList::real(std::string_view key) const
{
    const Field* field = find(key);
    if (field == nullptr) {
        return std::nullopt;
    }
    return parseReal(field->value);
}

std::optional<std::int64_t> FieldList::integer(std::string_view key) const
{
    const Field* field = find(key);
    if (field == nullptr) {
        return std::nullopt;
    }
    return detail::parseInteger(field->value);
}

std::optional<bool> FieldList::boolean(std::string_view key) const
{
    const Field* field = find(key);
    if (field == nullptr) {
        return std::nullopt;
    }
    return detail::parseBoolean(field->value);
}

std::string FieldList::text(std::string_view key, std::string_view fallback) const
{
    const Field* field = find(key);
    return field == nullptr ? std::string(fallback) : field->value;
}

// ---- elements --------------------------------------------------------------

std::string_view toString(StringKind kind)
{
    switch (kind) {
    case StringKind::Super:
        return "super";
    case StringKind::TwoD:
        return "2d";
    case StringKind::ThreeD:
        return "3d";
    case StringKind::FourD:
        return "4d";
    case StringKind::Pipe:
        return "pipe";
    case StringKind::Polyline:
        return "polyline";
    case StringKind::Face:
        return "face";
    case StringKind::Interface:
        return "interface";
    }
    return "super";
}

std::size_t VertexString::segmentCount() const
{
    if (vertices.size() < 2) {
        return 0;
    }
    return closed ? vertices.size() : vertices.size() - 1;
}

namespace {

bool isIpPart(const AlignmentPart& part, bool vertical)
{
    const std::string& kind = part.kind;
    if (kind == "ip" || kind == "arc") {
        return true;
    }
    if (vertical) {
        return kind == "kvalue" || kind == "length" || kind == "radius" || kind == "asymmetric";
    }
    return kind == "spiral";
}

} // namespace

bool SuperAlignment::horizontalIsIpOnly() const
{
    return !horizontalParts.empty() &&
           std::all_of(horizontalParts.begin(), horizontalParts.end(),
                       [](const AlignmentPart& part) { return isIpPart(part, false); });
}

bool SuperAlignment::verticalIsIpOnly() const
{
    return !verticalParts.empty() &&
           std::all_of(verticalParts.begin(), verticalParts.end(),
                       [](const AlignmentPart& part) { return isIpPart(part, true); });
}

bool Tin::isSurfaceTriangle(std::size_t index) const
{
    if (index >= triangles.size()) {
        return false;
    }
    if (index < visible.size() && !visible[index]) {
        return false;
    }
    if (full) {
        // Manual 1.4.7.1: "any triangle that contains any of the first four
        // points is a construction triangle". Only a full_tin has them; a
        // `tin` lists visible triangles and its first four points are data.
        const auto& triangle = triangles[index];
        if (triangle[0] < 4 || triangle[1] < 4 || triangle[2] < 4) {
            return false;
        }
    }
    return true;
}

std::string elementKeyword(const Element& element)
{
    struct Visitor {
        std::string operator()(const VertexString& s) const
        {
            return "string " + std::string(toString(s.kind));
        }
        std::string operator()(const ArcString&) const { return "string arc"; }
        std::string operator()(const CircleString& c) const
        {
            return c.feature ? "string feature" : "string circle";
        }
        std::string operator()(const TextString&) const { return "string text"; }
        std::string operator()(const PlotFrame&) const { return "string plot_frame"; }
        std::string operator()(const DrainageString&) const { return "string drainage"; }
        std::string operator()(const SuperAlignment& a) const
        {
            switch (a.source) {
            case AlignmentSource::Alignment:
                return "string alignment";
            case AlignmentSource::Pipeline:
                return "string pipeline";
            case AlignmentSource::SuperAlignment:
                break;
            }
            return "string super_alignment";
        }
        std::string operator()(const LasCloud&) const { return "string las_cloud_data"; }
        std::string operator()(const Tin& t) const { return t.full ? "full_tin" : "tin"; }
        std::string operator()(const SuperTin&) const { return "super_tin"; }
        std::string operator()(const Trimesh&) const { return "primitive_3d"; }
    };
    return std::visit(Visitor{}, element);
}

} // namespace katana::archive12d
