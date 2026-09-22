#include "xml.hpp"

#include <algorithm>
#include <charconv>
#include <cstdint>

#include "text_utilities.hpp"

namespace katana::archive12d::detail {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;

// A mapfile is four elements deep. The cap is here because this reads
// untrusted text and the reader is recursive: without it a file made of
// 100,000 open tags would end the process by exhausting the stack rather than
// by reporting anything. 64 is far past any real document of this kind.
constexpr std::size_t kMaxDepth = 64;

[[nodiscard]] bool isNameStart(char c)
{
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_' || c == ':';
}

[[nodiscard]] bool isNameChar(char c)
{
    return isNameStart(c) || (c >= '0' && c <= '9') || c == '-' || c == '.';
}

[[nodiscard]] bool isSpace(char c)
{
    return c == ' ' || c == '\t' || c == '\r' || c == '\n';
}

class Parser {
  public:
    explicit Parser(std::string_view text) : text_(text) {}

    katana::core::Result<XmlNode> run()
    {
        skipProlog();
        if (failed_) {
            return error_;
        }
        if (at_ >= text_.size() || text_[at_] != '<') {
            return fail("there is no element here");
        }
        XmlNode root;
        if (!readElement(root, 0)) {
            return error_;
        }
        return root;
    }

  private:
    [[nodiscard]] katana::core::Error fail(std::string what)
    {
        failed_ = true;
        error_ = makeError(ErrorCode::InvalidArgument, std::move(what),
                           "line " + std::to_string(line()));
        return error_;
    }

    [[nodiscard]] std::size_t line() const
    {
        return 1 + static_cast<std::size_t>(
                       std::count(text_.begin(), text_.begin() + static_cast<std::ptrdiff_t>(
                                                                    std::min(at_, text_.size())),
                                  '\n'));
    }

    void skipSpace()
    {
        while (at_ < text_.size() && isSpace(text_[at_])) {
            ++at_;
        }
    }

    // Comments, processing instructions and stray white space, anywhere they
    // are allowed. Returns false once something else is next.
    bool skipTrivia()
    {
        bool skipped = false;
        while (at_ < text_.size()) {
            skipSpace();
            if (text_.compare(at_, 4, "<!--") == 0) {
                const std::size_t end = text_.find("-->", at_ + 4);
                if (end == std::string_view::npos) {
                    (void)fail("a comment is never closed");
                    return false;
                }
                at_ = end + 3;
                skipped = true;
                continue;
            }
            if (text_.compare(at_, 2, "<?") == 0) {
                const std::size_t end = text_.find("?>", at_ + 2);
                if (end == std::string_view::npos) {
                    (void)fail("a processing instruction is never closed");
                    return false;
                }
                at_ = end + 2;
                skipped = true;
                continue;
            }
            return skipped;
        }
        return skipped;
    }

    void skipProlog()
    {
        while (skipTrivia() && !failed_) {
        }
        if (!failed_ && text_.compare(at_, 9, "<!DOCTYPE") == 0) {
            // A doctype can carry entity definitions that change what the
            // rest of the document means. Refused rather than skipped.
            (void)fail("a document type declaration is not read");
        }
    }

    [[nodiscard]] std::string_view readName()
    {
        const std::size_t start = at_;
        if (at_ < text_.size() && isNameStart(text_[at_])) {
            ++at_;
            while (at_ < text_.size() && isNameChar(text_[at_])) {
                ++at_;
            }
        }
        return text_.substr(start, at_ - start);
    }

    // Resolves the five predefined entities and character references.
    bool appendResolved(std::string_view raw, std::string& out)
    {
        for (std::size_t i = 0; i < raw.size();) {
            if (raw[i] != '&') {
                out.push_back(raw[i++]);
                continue;
            }
            const std::size_t end = raw.find(';', i);
            if (end == std::string_view::npos) {
                (void)fail("an ampersand is not the start of an entity reference");
                return false;
            }
            const std::string_view name = raw.substr(i + 1, end - i - 1);
            if (name == "amp") {
                out.push_back('&');
            } else if (name == "lt") {
                out.push_back('<');
            } else if (name == "gt") {
                out.push_back('>');
            } else if (name == "quot") {
                out.push_back('"');
            } else if (name == "apos") {
                out.push_back('\'');
            } else if (name.size() > 1 && name[0] == '#') {
                std::uint32_t code = 0;
                const bool hex = name[1] == 'x' || name[1] == 'X';
                const std::string_view digits = name.substr(hex ? 2 : 1);
                const auto parsed = std::from_chars(digits.data(), digits.data() + digits.size(),
                                                    code, hex ? 16 : 10);
                if (parsed.ec != std::errc{} || parsed.ptr != digits.data() + digits.size() ||
                    code > 0x10FFFF) {
                    (void)fail("\"&" + std::string(name) + ";\" is not a character");
                    return false;
                }
                appendUtf8(code, out);
            } else {
                (void)fail("\"&" + std::string(name) + ";\" is an entity this does not know");
                return false;
            }
            i = end + 1;
        }
        return true;
    }

    static void appendUtf8(std::uint32_t code, std::string& out)
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

    bool readAttributes(XmlNode& node, bool& selfClosing)
    {
        selfClosing = false;
        while (true) {
            skipSpace();
            if (at_ >= text_.size()) {
                (void)fail("element \"" + node.name + "\" is never closed");
                return false;
            }
            if (text_[at_] == '>') {
                ++at_;
                return true;
            }
            if (text_.compare(at_, 2, "/>") == 0) {
                at_ += 2;
                selfClosing = true;
                return true;
            }
            const std::string_view name = readName();
            if (name.empty()) {
                (void)fail("element \"" + node.name + "\" has something that is not an attribute");
                return false;
            }
            skipSpace();
            if (at_ >= text_.size() || text_[at_] != '=') {
                (void)fail("attribute \"" + std::string(name) + "\" has no value");
                return false;
            }
            ++at_;
            skipSpace();
            if (at_ >= text_.size() || (text_[at_] != '"' && text_[at_] != '\'')) {
                (void)fail("attribute \"" + std::string(name) + "\" is not in quotes");
                return false;
            }
            const char quote = text_[at_++];
            const std::size_t start = at_;
            while (at_ < text_.size() && text_[at_] != quote) {
                ++at_;
            }
            if (at_ >= text_.size()) {
                (void)fail("attribute \"" + std::string(name) + "\" is never closed");
                return false;
            }
            std::string value;
            if (!appendResolved(text_.substr(start, at_ - start), value)) {
                return false;
            }
            ++at_;
            node.attributes.emplace_back(std::string(name), std::move(value));
        }
    }

    bool readElement(XmlNode& node, std::size_t depth)
    {
        if (depth > kMaxDepth) {
            (void)fail("elements are nested more than " + std::to_string(kMaxDepth) + " deep");
            return false;
        }
        ++at_; // the '<'
        const std::string_view name = readName();
        if (name.empty()) {
            (void)fail("a tag has no name");
            return false;
        }
        node.name = name;
        bool selfClosing = false;
        if (!readAttributes(node, selfClosing)) {
            return false;
        }
        if (selfClosing) {
            return true;
        }

        std::string text;
        while (true) {
            if (at_ >= text_.size()) {
                (void)fail("element \"" + node.name + "\" is never closed");
                return false;
            }
            if (text_[at_] != '<') {
                const std::size_t start = at_;
                while (at_ < text_.size() && text_[at_] != '<') {
                    ++at_;
                }
                if (!appendResolved(text_.substr(start, at_ - start), text)) {
                    return false;
                }
                continue;
            }
            if (text_.compare(at_, 9, "<![CDATA[") == 0) {
                const std::size_t end = text_.find("]]>", at_ + 9);
                if (end == std::string_view::npos) {
                    (void)fail("a CDATA section is never closed");
                    return false;
                }
                text.append(text_.substr(at_ + 9, end - at_ - 9));
                at_ = end + 3;
                continue;
            }
            if (text_.compare(at_, 4, "<!--") == 0 || text_.compare(at_, 2, "<?") == 0) {
                if (!skipTrivia() && failed_) {
                    return false;
                }
                continue;
            }
            if (text_.compare(at_, 2, "</") == 0) {
                at_ += 2;
                const std::string_view closing = readName();
                if (closing != node.name) {
                    (void)fail("\"" + node.name + "\" is closed by \"" + std::string(closing) +
                               "\"");
                    return false;
                }
                skipSpace();
                if (at_ >= text_.size() || text_[at_] != '>') {
                    (void)fail("the closing tag of \"" + node.name + "\" has no `>`");
                    return false;
                }
                ++at_;
                node.text = std::string(trimmed(text));
                return true;
            }
            XmlNode child;
            if (!readElement(child, depth + 1)) {
                return false;
            }
            node.children.push_back(std::move(child));
        }
    }

    std::string_view text_;
    std::size_t at_ = 0;
    bool failed_ = false;
    katana::core::Error error_{};
};

} // namespace

const XmlNode* XmlNode::child(std::string_view childName) const
{
    const auto found = std::find_if(children.begin(), children.end(),
                                    [childName](const XmlNode& c) { return c.name == childName; });
    return found == children.end() ? nullptr : &*found;
}

std::string XmlNode::childText(std::string_view childName) const
{
    const XmlNode* found = child(childName);
    return found == nullptr ? std::string{} : found->text;
}

std::vector<const XmlNode*> XmlNode::childrenNamed(std::string_view childName) const
{
    std::vector<const XmlNode*> found;
    for (const XmlNode& node : children) {
        if (node.name == childName) {
            found.push_back(&node);
        }
    }
    return found;
}

katana::core::Result<XmlNode> readXml(std::string_view text)
{
    Parser parser(text);
    return parser.run();
}

} // namespace katana::archive12d::detail
