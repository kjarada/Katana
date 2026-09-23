#include "katana/entity/layer_path.hpp"

#include <algorithm>
#include <cctype>

#include "katana/core/text_encoding.hpp"

namespace katana::entity {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

namespace {

[[nodiscard]] bool isBlank(std::string_view segment)
{
    return std::all_of(segment.begin(), segment.end(), [](unsigned char c) {
        return std::isspace(c) != 0;
    });
}

} // namespace

Status validateLayerPath(std::string_view name)
{
    if (name.empty()) {
        return makeError(ErrorCode::InvalidArgument, "layer name is empty");
    }
    if (name.size() > kMaximumLayerNameLength) {
        return makeError(ErrorCode::InvalidArgument, "layer name is too long",
                         std::to_string(name.size()) + " > " +
                             std::to_string(kMaximumLayerNameLength));
    }
    if (!katana::core::isValidUtf8(name)) {
        // Every other name in the model is checked (detail::validateName). A
        // layer name typed on a CP1252 console - "Café" - was accepted, and
        // then no entity on it could be written as JSON (audit MOD-12). The
        // bytes are left out of the error, which may itself become JSON.
        return makeError(ErrorCode::InvalidArgument, "layer name is not valid UTF-8");
    }
    if (name.front() == kLayerSeparator || name.back() == kLayerSeparator) {
        return makeError(ErrorCode::InvalidArgument,
                         "layer name must not start or end with '/'", std::string(name));
    }

    const std::vector<std::string_view> segments = layerSegments(name);
    if (segments.size() > kMaximumLayerDepth) {
        return makeError(ErrorCode::InvalidArgument, "layer name is nested too deeply",
                         std::to_string(segments.size()) + " > " +
                             std::to_string(kMaximumLayerDepth));
    }
    for (const std::string_view segment : segments) {
        if (segment.empty()) {
            return makeError(ErrorCode::InvalidArgument, "layer name has an empty level",
                             std::string(name));
        }
        if (isBlank(segment)) {
            return makeError(ErrorCode::InvalidArgument, "layer level is only whitespace",
                             std::string(name));
        }
        if (segment != segment.substr(0, segment.find_last_not_of(" \t") + 1) ||
            segment.front() == ' ' || segment.front() == '\t') {
            // Surrounding spaces make two layers that look identical in a tree
            // and compare different everywhere else.
            return makeError(ErrorCode::InvalidArgument,
                             "layer level has leading or trailing whitespace", std::string(name));
        }
        if (segment == "." || segment == "..") {
            return makeError(ErrorCode::InvalidArgument,
                             "layer level must not be '.' or '..'", std::string(name));
        }
        for (const unsigned char c : segment) {
            if (c < 0x20 || c == 0x7F) {
                return makeError(ErrorCode::InvalidArgument,
                                 "layer name contains a control character", std::string(name));
            }
        }
    }
    return {};
}

std::vector<std::string_view> layerSegments(std::string_view name)
{
    std::vector<std::string_view> segments;
    std::size_t start = 0;
    while (true) {
        const std::size_t separator = name.find(kLayerSeparator, start);
        if (separator == std::string_view::npos) {
            segments.push_back(name.substr(start));
            return segments;
        }
        segments.push_back(name.substr(start, separator - start));
        start = separator + 1;
    }
}

std::string_view layerLeaf(std::string_view name)
{
    const std::size_t separator = name.rfind(kLayerSeparator);
    return separator == std::string_view::npos ? name : name.substr(separator + 1);
}

std::string_view layerParent(std::string_view name)
{
    const std::size_t separator = name.rfind(kLayerSeparator);
    return separator == std::string_view::npos ? std::string_view{} : name.substr(0, separator);
}

std::vector<std::string> layerAncestors(std::string_view name)
{
    std::vector<std::string> ancestors;
    std::size_t start = 0;
    while (true) {
        const std::size_t separator = name.find(kLayerSeparator, start);
        if (separator == std::string_view::npos) {
            return ancestors; // the name itself is not one of its ancestors
        }
        ancestors.emplace_back(name.substr(0, separator));
        start = separator + 1;
    }
}

std::size_t layerDepth(std::string_view name)
{
    if (name.empty()) {
        return 0;
    }
    return static_cast<std::size_t>(
               std::count(name.begin(), name.end(), kLayerSeparator)) +
           1;
}

bool isLayerUnder(std::string_view name, std::string_view ancestor)
{
    if (ancestor.empty()) {
        return true; // the empty path is the root of everything
    }
    if (name.size() < ancestor.size()) {
        return false;
    }
    if (name.compare(0, ancestor.size(), ancestor) != 0) {
        return false;
    }
    // Whole segments only: "designs/x" starts with "design" as TEXT but is a
    // different tree, and treating it as a descendant would hide or lock layers
    // the user never touched.
    return name.size() == ancestor.size() || name[ancestor.size()] == kLayerSeparator;
}

std::string joinLayerPath(std::string_view parent, std::string_view child)
{
    if (parent.empty()) {
        return std::string(child);
    }
    if (child.empty()) {
        return std::string(parent);
    }
    std::string joined;
    joined.reserve(parent.size() + 1 + child.size());
    joined.append(parent);
    joined.push_back(kLayerSeparator);
    joined.append(child);
    return joined;
}

std::string rewriteLayerPrefix(std::string_view name, std::string_view from, std::string_view to)
{
    if (!isLayerUnder(name, from)) {
        return std::string(name);
    }
    if (name.size() == from.size()) {
        return std::string(to);
    }
    std::string rewritten;
    const std::string_view tail = name.substr(from.size() + 1);
    rewritten.reserve(to.size() + 1 + tail.size());
    rewritten.append(to);
    rewritten.push_back(kLayerSeparator);
    rewritten.append(tail);
    return rewritten;
}

} // namespace katana::entity
