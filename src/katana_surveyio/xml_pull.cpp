#include "xml_pull.hpp"

#include <algorithm>
#include <charconv>
#include <cstdint>
#include <optional>

namespace katana::surveyio::detail {

using katana::core::ErrorCode;
using katana::core::makeError;

namespace {

bool isSpace(char c)
{
    return c == ' ' || c == '\t' || c == '\r' || c == '\n';
}

// XML's name rules, restricted to what can be checked a byte at a time: any
// byte of a multi-byte UTF-8 sequence is accepted, so a name in another script
// is read, and every ASCII byte is checked against the rule exactly.
bool isNameStart(char c)
{
    const auto u = static_cast<unsigned char>(c);
    return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || c == '_' || c == ':' || u >= 0x80;
}

bool isNameChar(char c)
{
    return isNameStart(c) || (c >= '0' && c <= '9') || c == '-' || c == '.';
}

bool allSpace(std::string_view text)
{
    return std::all_of(text.begin(), text.end(), isSpace);
}

std::size_t lineOf(std::string_view doc, std::size_t offset)
{
    offset = std::min(offset, doc.size());
    return 1 + static_cast<std::size_t>(std::count(doc.begin(), doc.begin() + static_cast<std::ptrdiff_t>(offset), '\n'));
}

void appendUtf8(std::uint32_t code, std::string& out)
{
    if (code < 0x80) {
        out.push_back(static_cast<char>(code));
    } else if (code < 0x800) {
        out.push_back(static_cast<char>(0xC0 | (code >> 6)));
        out.push_back(static_cast<char>(0x80 | (code & 0x3F)));
    } else if (code < 0x10000) {
        out.push_back(static_cast<char>(0xE0 | (code >> 12)));
        out.push_back(static_cast<char>(0x80 | ((code >> 6) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | (code & 0x3F)));
    } else {
        out.push_back(static_cast<char>(0xF0 | (code >> 18)));
        out.push_back(static_cast<char>(0x80 | ((code >> 12) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | ((code >> 6) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | (code & 0x3F)));
    }
}

} // namespace

XmlPullReader::Event XmlPullReader::fail(std::size_t at, std::string message)
{
    failed_ = true;
    error_ = makeError(ErrorCode::ParseFailure,
                       "the XML is damaged or incomplete at line " +
                           std::to_string(lineOf(doc_, at)) + ": " + std::move(message));
    return Event::Error;
}

bool XmlPullReader::attribute(std::string_view attributeName, std::string_view& rawValue) const
{
    // The region was checked when the tag was read, so this walk can trust
    // its shape: name, optional space, '=', optional space, quoted value.
    std::size_t p = 0;
    const std::string_view region = attributes_;
    while (p < region.size()) {
        while (p < region.size() && isSpace(region[p])) {
            ++p;
        }
        const std::size_t nameStart = p;
        while (p < region.size() && isNameChar(region[p])) {
            ++p;
        }
        const std::string_view found = region.substr(nameStart, p - nameStart);
        while (p < region.size() && region[p] != '"' && region[p] != '\'') {
            ++p;
        }
        if (p >= region.size()) {
            return false;
        }
        const char quote = region[p];
        const std::size_t valueStart = p + 1;
        const std::size_t close = region.find(quote, valueStart);
        if (close == std::string_view::npos) {
            return false;
        }
        if (found == attributeName) {
            rawValue = region.substr(valueStart, close - valueStart);
            return true;
        }
        p = close + 1;
    }
    return false;
}

XmlPullReader::Event XmlPullReader::readStartTag(std::size_t tagStart)
{
    std::size_t p = tagStart + 1;
    const std::size_t size = doc_.size();
    if (p >= size || !isNameStart(doc_[p])) {
        return fail(tagStart, "a '<' that does not begin a tag");
    }
    const std::size_t nameStart = p;
    while (p < size && isNameChar(doc_[p])) {
        ++p;
    }
    const std::string_view tagName = doc_.substr(nameStart, p - nameStart);
    const std::size_t attributesStart = p;
    std::size_t attributesEnd = p;
    bool selfClosing = false;
    for (;;) {
        const bool spaced = p < size && isSpace(doc_[p]);
        while (p < size && isSpace(doc_[p])) {
            ++p;
        }
        if (p >= size) {
            return fail(tagStart, "the tag <" + std::string(tagName) + "> is not closed");
        }
        const char c = doc_[p];
        if (c == '>') {
            attributesEnd = p;
            ++p;
            break;
        }
        if (c == '/') {
            if (p + 1 < size && doc_[p + 1] == '>') {
                attributesEnd = p;
                p += 2;
                selfClosing = true;
                break;
            }
            return fail(p, "a '/' inside the tag <" + std::string(tagName) + ">");
        }
        if (!spaced || !isNameStart(c)) {
            return fail(p, "an unexpected character in the tag <" + std::string(tagName) + ">");
        }
        while (p < size && isNameChar(doc_[p])) {
            ++p;
        }
        while (p < size && isSpace(doc_[p])) {
            ++p;
        }
        if (p >= size || doc_[p] != '=') {
            return fail(p, "an attribute with no value in <" + std::string(tagName) + ">");
        }
        ++p;
        while (p < size && isSpace(doc_[p])) {
            ++p;
        }
        if (p >= size || (doc_[p] != '"' && doc_[p] != '\'')) {
            return fail(p, "an attribute value that is not quoted in <" + std::string(tagName) + ">");
        }
        const std::size_t close = doc_.find(doc_[p], p + 1);
        if (close == std::string_view::npos) {
            return fail(p, "an attribute value that is never closed in <" + std::string(tagName) + ">");
        }
        if (doc_.substr(p + 1, close - p - 1).find('<') != std::string_view::npos) {
            return fail(p, "a '<' inside an attribute value in <" + std::string(tagName) + ">");
        }
        p = close + 1;
    }

    if (rootClosed_) {
        return fail(tagStart, "a second root element <" + std::string(tagName) + ">");
    }
    if (depth_ >= open_.size()) {
        return fail(tagStart, "elements nested deeper than " + std::to_string(open_.size()) +
                                  " levels, which is refused");
    }
    rootSeen_ = true;
    pos_ = p;
    name_ = tagName;
    attributes_ = doc_.substr(attributesStart, attributesEnd - attributesStart);
    eventOffset_ = tagStart;
    eventDepth_ = depth_ + 1;
    if (selfClosing) {
        pendingSelfClose_ = true;
        if (depth_ == 0) {
            rootClosed_ = true;
        }
    } else {
        open_[depth_++] = tagName;
    }
    return Event::StartElement;
}

XmlPullReader::Event XmlPullReader::readEndTag(std::size_t tagStart)
{
    std::size_t p = tagStart + 2;
    const std::size_t size = doc_.size();
    const std::size_t nameStart = p;
    while (p < size && isNameChar(doc_[p])) {
        ++p;
    }
    const std::string_view tagName = doc_.substr(nameStart, p - nameStart);
    while (p < size && isSpace(doc_[p])) {
        ++p;
    }
    if (p >= size || doc_[p] != '>') {
        return fail(tagStart, "the end tag </" + std::string(tagName) + "> is not closed");
    }
    if (depth_ == 0) {
        return fail(tagStart, "the end tag </" + std::string(tagName) + "> closes nothing");
    }
    if (open_[depth_ - 1] != tagName) {
        return fail(tagStart, "</" + std::string(tagName) + "> where </" +
                                  std::string(open_[depth_ - 1]) + "> was expected");
    }
    pos_ = p + 1;
    name_ = tagName;
    eventOffset_ = tagStart;
    eventDepth_ = depth_;
    --depth_;
    if (depth_ == 0) {
        rootClosed_ = true;
    }
    return Event::EndElement;
}

XmlPullReader::Event XmlPullReader::readMarkup()
{
    // Called with doc_[pos_] == '<'. Returns Text for a CDATA section, the
    // tag's event for a tag, and EndOfDocument as "nothing to report, carry
    // on" for comments and processing instructions - next() loops on it.
    const std::size_t tagStart = pos_;
    const std::string_view rest = doc_.substr(pos_);
    if (rest.starts_with("<!--")) {
        const std::size_t close = doc_.find("-->", pos_ + 4);
        if (close == std::string_view::npos) {
            return fail(tagStart, "a comment that is never closed");
        }
        pos_ = close + 3;
        return Event::EndOfDocument;
    }
    if (rest.starts_with("<![CDATA[")) {
        const std::size_t close = doc_.find("]]>", pos_ + 9);
        if (close == std::string_view::npos) {
            return fail(tagStart, "a CDATA section that is never closed");
        }
        if (depth_ == 0) {
            return fail(tagStart, "a CDATA section outside the root element");
        }
        text_ = doc_.substr(pos_ + 9, close - pos_ - 9);
        cdata_ = true;
        eventOffset_ = tagStart;
        eventDepth_ = depth_;
        pos_ = close + 3;
        return Event::Text;
    }
    if (rest.starts_with("<!DOCTYPE")) {
        return fail(tagStart, "a document type declaration (DOCTYPE), which is refused because it "
                              "can change what the rest of the file means");
    }
    if (rest.starts_with("<!")) {
        return fail(tagStart, "a '<!' declaration this reader does not accept");
    }
    if (rest.starts_with("<?")) {
        const std::size_t close = doc_.find("?>", pos_ + 2);
        if (close == std::string_view::npos) {
            return fail(tagStart, "a processing instruction that is never closed");
        }
        pos_ = close + 2;
        return Event::EndOfDocument;
    }
    if (rest.starts_with("</")) {
        return readEndTag(tagStart);
    }
    return readStartTag(tagStart);
}

XmlPullReader::Event XmlPullReader::next()
{
    if (failed_) {
        return Event::Error;
    }
    if (pendingSelfClose_) {
        // name_ and eventDepth_ still describe the self-closed element.
        pendingSelfClose_ = false;
        return Event::EndElement;
    }
    for (;;) {
        if (pos_ >= doc_.size()) {
            if (depth_ > 0) {
                return fail(pos_, "the file ends inside <" + std::string(open_[depth_ - 1]) +
                                      ">, so it is incomplete");
            }
            if (!rootSeen_) {
                return fail(pos_, "there is no root element");
            }
            return Event::EndOfDocument;
        }
        if (doc_[pos_] == '<') {
            const Event event = readMarkup();
            if (event == Event::EndOfDocument) {
                continue; // a comment or processing instruction
            }
            return event;
        }
        const std::size_t start = pos_;
        const std::size_t lt = doc_.find('<', pos_);
        pos_ = lt == std::string_view::npos ? doc_.size() : lt;
        const std::string_view raw = doc_.substr(start, pos_ - start);
        if (depth_ == 0) {
            if (!allSpace(raw)) {
                return fail(start, "text outside the root element");
            }
            continue;
        }
        text_ = raw;
        cdata_ = false;
        eventOffset_ = start;
        eventDepth_ = depth_;
        return Event::Text;
    }
}

katana::core::Status appendXmlText(std::string_view raw, std::string& out)
{
    std::size_t p = 0;
    while (p < raw.size()) {
        const std::size_t amp = raw.find('&', p);
        if (amp == std::string_view::npos) {
            out.append(raw.substr(p));
            break;
        }
        out.append(raw.substr(p, amp - p));
        const std::size_t semi = raw.find(';', amp + 1);
        // The longest reference that can be valid is "&#x10FFFF;".
        if (semi == std::string_view::npos || semi - amp > 10) {
            return makeError(ErrorCode::ParseFailure, "an '&' that does not begin an entity reference",
                             std::string(raw.substr(amp, std::min<std::size_t>(12, raw.size() - amp))));
        }
        const std::string_view entity = raw.substr(amp + 1, semi - amp - 1);
        if (entity == "lt") {
            out.push_back('<');
        } else if (entity == "gt") {
            out.push_back('>');
        } else if (entity == "amp") {
            out.push_back('&');
        } else if (entity == "quot") {
            out.push_back('"');
        } else if (entity == "apos") {
            out.push_back('\'');
        } else if (entity.size() >= 2 && entity[0] == '#') {
            const bool hex = entity[1] == 'x';
            const std::string_view digits = entity.substr(hex ? 2 : 1);
            std::uint32_t code = 0;
            const auto [end, ec] =
                std::from_chars(digits.data(), digits.data() + digits.size(), code, hex ? 16 : 10);
            const bool valid = ec == std::errc{} && end == digits.data() + digits.size() &&
                               !digits.empty() && code != 0 && code <= 0x10FFFF &&
                               !(code >= 0xD800 && code <= 0xDFFF);
            if (!valid) {
                return makeError(ErrorCode::ParseFailure,
                                 "a character reference that names no character",
                                 "&" + std::string(entity) + ";");
            }
            appendUtf8(code, out);
        } else {
            return makeError(ErrorCode::ParseFailure,
                             "an entity reference this reader does not define, which is refused",
                             "&" + std::string(entity) + ";");
        }
        p = semi + 1;
    }
    return {};
}

std::size_t LineCounter::lineAt(std::size_t offset)
{
    offset = std::min(offset, doc_.size());
    if (offset < countedTo_) {
        countedTo_ = 0;
        line_ = 1;
    }
    line_ += static_cast<std::size_t>(std::count(doc_.begin() + static_cast<std::ptrdiff_t>(countedTo_),
                                                 doc_.begin() + static_cast<std::ptrdiff_t>(offset), '\n'));
    countedTo_ = offset;
    return line_;
}

} // namespace katana::surveyio::detail
