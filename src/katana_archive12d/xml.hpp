#pragma once

// Just enough XML to read a 12d mapfile. Internal to the module.
//
// Not a general XML parser and not trying to be one. It reads elements, their
// attributes, their text and their children, and it REFUSES what it does not
// understand rather than skipping it - a mapfile whose rules are silently
// half-read draws the wrong thing, which is worse than not drawing.
//
// Supported: the XML declaration and other processing instructions, comments,
// CDATA, self-closing elements, attributes in single or double quotes, and
// the five predefined entities. Measured against the two Transport for NSW
// mapfiles, which between them use a processing instruction, attributes on
// the root element only, 467 self-closing elements, no entity reference, no
// CDATA and no comment.
//
// Not supported, and reported: a document type declaration, a namespace
// prefix that is not simply part of the name, and any entity reference other
// than the five. Character references (&#39;) ARE read.
//
// The obvious alternative was to link a parser. This module deliberately has
// no third-party dependency - it is the one that builds with
// -DKATANA_BUILD_IO=OFF and so is the one the sanitizer job covers - and
// putting a general XML parser behind that boundary to read a file of this
// shape is more surface, not less.

#include <cstddef>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::archive12d::detail {

struct XmlNode {
    std::string name{};
    std::vector<std::pair<std::string, std::string>> attributes{};
    // Character data directly inside this element, with leading and trailing
    // white space trimmed. An element with children usually has none.
    std::string text{};
    std::vector<XmlNode> children{};

    // The first child of that name, or nullptr.
    [[nodiscard]] const XmlNode* child(std::string_view childName) const;
    // The text of the first child of that name, or an empty string. An empty
    // string and a missing child are deliberately the same answer: in a
    // mapfile `<rotation/>` and no rotation at all both mean "nothing said".
    [[nodiscard]] std::string childText(std::string_view childName) const;
    [[nodiscard]] bool has(std::string_view childName) const
    {
        return child(childName) != nullptr;
    }
    [[nodiscard]] std::vector<const XmlNode*> childrenNamed(std::string_view childName) const;
};

// The root element. Fails with InvalidArgument naming the line.
[[nodiscard]] katana::core::Result<XmlNode> readXml(std::string_view text);

} // namespace katana::archive12d::detail
