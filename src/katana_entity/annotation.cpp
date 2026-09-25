#include "katana/entity/annotation.hpp"

#include <cmath>
#include <string>
#include <vector>

#include "katana/entity/label_text.hpp"
#include "katana/math/numerics.hpp"
#include "validation.hpp"

namespace katana::entity {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

char folded(char c)
{
    return (c >= 'A' && c <= 'Z') ? static_cast<char>(c - 'A' + 'a') : c;
}

// ASCII case folded, and '-', '_' and spaces ignored, so "along-line",
// "AlongLine" and "along line" are one name - how a person or an agent types
// an option they have read once.
bool sameName(std::string_view a, std::string_view b)
{
    const auto next = [](std::string_view s, std::size_t& i) -> int {
        while (i < s.size() && (s[i] == '-' || s[i] == '_' || s[i] == ' ')) {
            ++i;
        }
        return i == s.size() ? -1 : static_cast<unsigned char>(folded(s[i++]));
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

template <typename Enum, std::size_t N>
Result<Enum> enumFromString(std::string_view name, const Enum (&values)[N], const char* what)
{
    for (const Enum value : values) {
        if (sameName(toString(value), name)) {
            return value;
        }
    }
    std::string known;
    for (const Enum value : values) {
        known += known.empty() ? "" : " ";
        known += toString(value);
    }
    return makeError(ErrorCode::ParseFailure, std::string("unknown ") + what + " (" + known + ")",
                     std::string(name));
}

Status requireFiniteAtLeast(double value, double minimum, const char* what, const std::string& name)
{
    if (!std::isfinite(value) || value < minimum) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string(what) + " must be finite and at least " +
                             std::to_string(minimum),
                         name);
    }
    return {};
}

} // namespace

// ---- text styles --------------------------------------------------------------------

Status validate(const TextStyle& style)
{
    if (auto status = detail::validateName(style.name, "text style"); !status) {
        return status;
    }
    if (!isValidUtf8(style.fontFamily)) {
        return makeError(ErrorCode::InvalidArgument, "font family is not valid UTF-8", style.name);
    }
    if (auto status = requireFiniteAtLeast(style.paperHeight, 0.0, "paper height", style.name);
        !status) {
        return status;
    }
    // A width factor of zero draws nothing and a negative one mirrors: DXF
    // accepts neither for a STYLE, and neither is a text a person can read.
    if (!std::isfinite(style.widthFactor) || style.widthFactor <= 0.0 || style.widthFactor > 100.0) {
        return makeError(ErrorCode::InvalidArgument,
                         "width factor must be greater than zero and at most 100", style.name);
    }
    // 85 degrees: the limit AutoCAD's STYLE command gives the obliquing angle.
    if (!std::isfinite(style.oblique) ||
        std::abs(style.oblique) >= 85.0 * katana::math::kDegToRad) {
        return makeError(ErrorCode::InvalidArgument,
                         "the obliquing angle must be less than 85 degrees either way", style.name);
    }
    if (auto status = requireFiniteAtLeast(style.maskMargin, 0.0, "mask margin", style.name);
        !status) {
        return status;
    }
    if (!std::isfinite(style.lineSpacing) || style.lineSpacing < 0.25 || style.lineSpacing > 4.0) {
        // AutoCAD's MTEXT accepts 0.25 to 4.0 for its line spacing factor.
        return makeError(ErrorCode::InvalidArgument, "line spacing must be between 0.25 and 4",
                         style.name);
    }
    return {};
}

Status TextStylePolicy::validate(const TextStyle& style)
{
    return katana::entity::validate(style);
}

void TextStylePolicy::seed(NamedMap<TextStyle>& items)
{
    TextStyle standard;
    standard.name = std::string(kDefaultTextStyleName);
    items.emplace(standard.name, std::move(standard));
}

// ---- label styles --------------------------------------------------------------------

std::string_view toString(LabelKind kind)
{
    switch (kind) {
    case LabelKind::Point:
        return "point";
    case LabelKind::Segment:
        return "segment";
    case LabelKind::Arc:
        return "arc";
    case LabelKind::Area:
        return "area";
    case LabelKind::Chainage:
        return "chainage";
    }
    return "point";
}

Result<LabelKind> labelKindFromString(std::string_view name)
{
    static constexpr LabelKind kValues[] = {LabelKind::Point, LabelKind::Segment, LabelKind::Arc,
                                            LabelKind::Area, LabelKind::Chainage};
    return enumFromString(name, kValues, "label kind");
}

std::string_view toString(LabelPlacement placement)
{
    switch (placement) {
    case LabelPlacement::Auto:
        return "auto";
    case LabelPlacement::Above:
        return "above";
    case LabelPlacement::Below:
        return "below";
    case LabelPlacement::Along:
        return "along";
    case LabelPlacement::Centroid:
        return "centroid";
    case LabelPlacement::Right:
        return "right";
    case LabelPlacement::Left:
        return "left";
    }
    return "auto";
}

Result<LabelPlacement> labelPlacementFromString(std::string_view name)
{
    static constexpr LabelPlacement kValues[] = {
        LabelPlacement::Auto,     LabelPlacement::Above, LabelPlacement::Below,
        LabelPlacement::Along,    LabelPlacement::Centroid, LabelPlacement::Right,
        LabelPlacement::Left};
    return enumFromString(name, kValues, "label placement");
}

std::string_view toString(LabelOrientation orientation)
{
    return orientation == LabelOrientation::Horizontal ? "horizontal" : "aligned";
}

Result<LabelOrientation> labelOrientationFromString(std::string_view name)
{
    static constexpr LabelOrientation kValues[] = {LabelOrientation::Aligned,
                                                   LabelOrientation::Horizontal};
    return enumFromString(name, kValues, "label orientation");
}

std::string_view toString(LabelMarker marker)
{
    switch (marker) {
    case LabelMarker::None:
        return "none";
    case LabelMarker::Cross:
        return "cross";
    case LabelMarker::Dot:
        return "dot";
    case LabelMarker::Circle:
        return "circle";
    }
    return "none";
}

Result<LabelMarker> labelMarkerFromString(std::string_view name)
{
    static constexpr LabelMarker kValues[] = {LabelMarker::None, LabelMarker::Cross,
                                              LabelMarker::Dot, LabelMarker::Circle};
    return enumFromString(name, kValues, "label marker");
}

Status validate(const LabelStyle& style)
{
    if (auto status = detail::validateName(style.name, "label style"); !status) {
        return status;
    }
    if (static_cast<int>(style.kind) > static_cast<int>(LabelKind::Chainage) ||
        static_cast<int>(style.placement) > static_cast<int>(LabelPlacement::Left) ||
        static_cast<int>(style.orientation) > static_cast<int>(LabelOrientation::Horizontal) ||
        static_cast<int>(style.marker) > static_cast<int>(LabelMarker::Circle)) {
        return makeError(ErrorCode::InvalidArgument, "label style has an unknown option",
                         style.name);
    }
    if (!isValidUtf8(style.text) || !isValidUtf8(style.textStyle)) {
        return makeError(ErrorCode::InvalidArgument, "label text is not valid UTF-8", style.name);
    }
    if (style.text.empty()) {
        return makeError(ErrorCode::InvalidArgument, "a label style needs a template", style.name);
    }
    // A template that could never print is refused on the way in, naming
    // what is wrong, rather than stored to draw a label with a hole in it.
    if (auto status = checkLabelTemplate(style.text, style.kind); !status) {
        return makeError(status.error().code, status.error().message,
                         style.name + ": " + status.error().context);
    }
    struct NonNegative {
        const char* what;
        double value;
    };
    for (const NonNegative& field :
         {NonNegative{"paper height", style.paperHeight}, NonNegative{"offset", style.offset},
          NonNegative{"marker size", style.markerSize},
          NonNegative{"minimum length", style.minimumLength},
          NonNegative{"tick interval", style.tickInterval},
          NonNegative{"tick length", style.tickLength}}) {
        if (auto status = requireFiniteAtLeast(field.value, 0.0, field.what, style.name);
            !status) {
            return status;
        }
    }
    if (style.kind == LabelKind::Chainage &&
        !(std::isfinite(style.interval) && style.interval > katana::math::tolerance::kGeometric)) {
        return makeError(ErrorCode::InvalidArgument, "a chainage label's interval must be positive",
                         style.name);
    }
    return {};
}

Status LabelStylePolicy::validate(const LabelStyle& style)
{
    return katana::entity::validate(style);
}

// ---- rules ------------------------------------------------------------------------------

Status validate(const LabelRule& rule)
{
    if (auto status = detail::validateName(rule.name, "label rule"); !status) {
        return status;
    }
    if (rule.labelStyle.empty()) {
        return makeError(ErrorCode::InvalidArgument, "a label rule needs a label style", rule.name);
    }
    for (const std::string* text : {&rule.labelStyle, &rule.layer, &rule.code, &rule.entityType,
                                    &rule.labelLayer}) {
        if (!isValidUtf8(*text)) {
            return makeError(ErrorCode::InvalidArgument, "label rule text is not valid UTF-8",
                             rule.name);
        }
    }
    if (!rule.entityType.empty() && !entityTypeFromString(rule.entityType)) {
        return makeError(ErrorCode::InvalidArgument, "unknown entity type in a label rule",
                         rule.name + " type=" + rule.entityType);
    }
    return {};
}

Status LabelRulePolicy::validate(const LabelRule& rule)
{
    return katana::entity::validate(rule);
}

bool globMatches(std::string_view pattern, std::string_view text)
{
    // The iterative matcher with one backtrack point: on a mismatch after a
    // '*', the star swallows one more character and the match resumes.
    // Linear in practice, and no recursion for a pattern of many stars.
    if (pattern.empty()) {
        return true; // no filter
    }
    std::size_t p = 0;
    std::size_t t = 0;
    std::size_t star = std::string_view::npos;
    std::size_t resume = 0;
    while (t < text.size()) {
        if (p < pattern.size() &&
            (pattern[p] == '?' || folded(pattern[p]) == folded(text[t]))) {
            ++p;
            ++t;
        } else if (p < pattern.size() && pattern[p] == '*') {
            star = p++;
            resume = t;
        } else if (star != std::string_view::npos) {
            p = star + 1;
            t = ++resume;
        } else {
            return false;
        }
    }
    while (p < pattern.size() && pattern[p] == '*') {
        ++p;
    }
    return p == pattern.size();
}

} // namespace katana::entity
