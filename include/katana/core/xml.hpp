#pragma once

// A small strict XML reader, shared by everything in Katana that has to read
// an XML-shaped survey or design file: the 12d mapfile and .4d libraries
// (PLAN.MD 20.3) and LandXML. It lives in core because there must be exactly
// one of these - a second hand-written XML reader is a second piece of
// untrusted-input surface to audit, not a convenience.
//
// Not a general XML parser and not trying to be one. It reads elements, their
// attributes, their text and their children, and it REFUSES what it does not
// understand rather than skipping it - a file whose rules are silently
// half-read is read WRONG, which is worse than not read at all.
//
// Supported: the XML declaration and other processing instructions, comments,
// CDATA, self-closing elements, attributes in single or double quotes, and
// the five predefined entities. Measured against two production 12d mapfiles,
// which between them use a processing instruction, attributes on the root
// element only, 467 self-closing elements, no entity reference, no CDATA and
// no comment.
//
// THE REFUSALS ARE SECURITY PROPERTIES, not omissions to be filled in later.
// Each one is a way a hostile or broken file could otherwise get past the
// reader, so removing any of them needs a better reason than "the file we were
// given has one":
//   - a document type declaration is refused, because it can redefine what the
//     entities in the rest of the document mean (and is the entry point for
//     entity-expansion and external-entity attacks);
//   - an entity reference other than the five predefined ones is refused,
//     because there is no definition of it that this reader has read;
//   - nesting deeper than 64 elements is refused, because the reader is
//     recursive and a file of 100,000 open tags would otherwise exhaust the
//     stack - a crash instead of a diagnosis.
// Character references (&#39;, &#x27;) ARE read. A namespace prefix is kept as
// part of the element or attribute name and is not resolved, so a caller that
// cares about `gml:pos` asks for it by that whole name.
//
// The obvious alternative was to link a parser. Rejected: the modules that
// need this - archive12d, survey - are the ones that build with
// -DKATANA_BUILD_IO=OFF and so are the ones the sanitizer job covers, and
// putting a general XML parser behind that boundary to read files of this
// shape is more surface, not less. Rule 4 also forbids a third-party type in
// this header; there is none.

#include <cstddef>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::core {

// The deepest nesting `readXml` accepts. See the note above: this is the
// stack-exhaustion guard, not a document limit anybody should be near - a 12d
// mapfile is four elements deep. Exposed so that a caller and its tests can
// name the limit rather than repeat the number.
constexpr std::size_t kXmlMaxDepth = 64;

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
[[nodiscard]] Result<XmlNode> readXml(std::string_view text);

} // namespace katana::core
