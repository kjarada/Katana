#pragma once

// A streaming (pull) XML tokenizer for survey files too large to hold as a
// tree.
//
// Why not katana::core::readXml, the one XML reader the project otherwise
// insists on: it builds the whole document as nested XmlNode values, which is
// the right shape for a 12d mapfile and the wrong one for a Trimble JobXML job
// of a few hundred megabytes - every element becomes a heap node with a string
// name and a vector of attributes, and the tree has to exist in full before
// the first record can be read. This reader hands out one event at a time as
// views into the caller's bytes and allocates nothing of its own.
//
// It keeps every REFUSAL core/xml.hpp keeps, for the reasons given there (they
// are security properties of reading untrusted files, not omissions):
//   - a document type declaration is refused;
//   - an entity reference other than the five predefined ones is refused
//     (appendXmlText);
//   - nesting deeper than core::kXmlMaxDepth elements is refused.
// And it refuses what a truncated or damaged file looks like: an end tag that
// does not match its start tag, text or a second element outside the root,
// a construct that runs off the end of the file.
//
// Attribute values and text come back RAW (undecoded); appendXmlText decodes
// them when a caller wants the characters. Most values in a survey file are
// numbers with no '&' in them, so decoding only on demand is the difference
// between allocating per value and not.

#include <array>
#include <cstddef>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/core/xml.hpp"

namespace katana::surveyio::detail {

class XmlPullReader {
  public:
    enum class Event {
        StartElement,  // name(), depth() (1 = the root), attribute()
        EndElement,    // name(), depth() of the element being closed
        Text,          // text(), raw; textIsCData() when it came from a CDATA section
        EndOfDocument, // the root element closed and nothing but white space followed
        Error,         // error(); every later call returns Error again
    };

    // `document` is not copied: it must outlive the reader.
    explicit XmlPullReader(std::string_view document) noexcept : doc_(document) {}

    [[nodiscard]] Event next();

    [[nodiscard]] std::string_view name() const { return name_; }
    [[nodiscard]] std::size_t depth() const { return eventDepth_; }
    [[nodiscard]] std::string_view text() const { return text_; }
    [[nodiscard]] bool textIsCData() const { return cdata_; }
    // True when the current Text holds an '&' (always false for CDATA):
    // noted while the text is scanned, so a caller need not scan it again to
    // know whether appendXmlText has anything to decode.
    [[nodiscard]] bool textHasReference() const { return textHasReference_; }
    // Byte offset in the document at which the current event began.
    [[nodiscard]] std::size_t offset() const { return eventOffset_; }

    // The RAW value of attribute `attributeName` on the current start
    // element; false when there is none.
    [[nodiscard]] bool attribute(std::string_view attributeName, std::string_view& rawValue) const;
    // False when the current start element has no attributes at all, which
    // is most of them: a caller can skip looking for any.
    [[nodiscard]] bool hasAttributes() const { return !attributes_.empty(); }

    [[nodiscard]] const katana::core::Error& error() const { return error_; }

  private:
    Event fail(std::size_t at, std::string message);
    Event readMarkup();
    Event readStartTag(std::size_t tagStart);
    Event readEndTag(std::size_t tagStart);

    std::string_view doc_;
    std::size_t pos_ = 0;
    std::array<std::string_view, katana::core::kXmlMaxDepth> open_{};
    std::size_t depth_ = 0;
    bool rootSeen_ = false;
    bool rootClosed_ = false;
    bool pendingSelfClose_ = false;
    bool failed_ = false;

    std::string_view name_{};
    std::string_view text_{};
    std::string_view attributes_{}; // the raw attribute region of the current start tag
    bool cdata_ = false;
    bool textHasReference_ = false;
    std::size_t eventDepth_ = 0;
    std::size_t eventOffset_ = 0;
    katana::core::Error error_{};
};

// Byte-wise equality for the short names and values of a survey file.
// string_view's == is a call to the C library's memcmp, which for the few
// bytes of a tag name costs more than the comparison; a large job makes
// millions of them.
[[nodiscard]] inline bool sameText(std::string_view a, std::string_view b) noexcept
{
    if (a.size() != b.size()) {
        return false;
    }
    for (std::size_t i = 0; i < a.size(); ++i) {
        if (a[i] != b[i]) {
            return false;
        }
    }
    return true;
}

// Appends `raw` to `out` with the five predefined entities and character
// references (&#65; &#x41;) replaced by the characters they name. Anything
// else after an '&' is refused (ParseFailure), for the reason given above.
[[nodiscard]] katana::core::Status appendXmlText(std::string_view raw, std::string& out);

// 1-based line number of byte offsets met in increasing order, counted
// incrementally so a reader that asks once per record pays for each byte
// once. Asking for an earlier offset than last time recounts from the start.
class LineCounter {
  public:
    explicit LineCounter(std::string_view document) noexcept : doc_(document) {}
    [[nodiscard]] std::size_t lineAt(std::size_t offset);

  private:
    std::string_view doc_;
    std::size_t countedTo_ = 0;
    std::size_t line_ = 1;
};

} // namespace katana::surveyio::detail
