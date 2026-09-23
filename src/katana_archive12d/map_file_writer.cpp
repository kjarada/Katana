// entity::SurveyMap -> 12d mapfile XML. The inverse of map_file.cpp, and
// tested as one: readMapFile(writeMapFile(m)) gives back m's rules.
//
// The layout copies what 12d writes - the element order inside an item, `yes`
// and `no` for flags, CRLF line ends, the xml12d wrapper and its units block -
// so that a file exported here looks to 12d like one of its own. Nothing of
// any customisation's CONTENT is copied: the element names are the format.

#include "katana/archive12d/map_file.hpp"

#include <algorithm>
#include <array>
#include <charconv>
#include <functional>
#include <map>
#include <optional>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

#include "katana/core/text.hpp"

namespace katana::archive12d {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;
using katana::entity::SurveyAttribute;
using katana::entity::SurveyPipe;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;

constexpr std::string_view kEol = "\r\n";

// 12d's order, from the two reference mapfiles: one has map_data,
// vertex_symbol_data, tinable_data, vertex_textstyle_data; the other has
// map_data, vertex_symbol_data, vertex_textstyle_data, the three pipe
// sections and the two attribute sections. This is the one order both agree
// with.
constexpr std::array<SurveySection, 9> kSectionOrder{
    SurveySection::Map,        SurveySection::VertexSymbol,    SurveySection::Tinable,
    SurveySection::VertexTextStyle, SurveySection::Pipe,       SurveySection::VertexPipe,
    SurveySection::SegmentPipe, SurveySection::StringAttribute, SurveySection::VertexAttribute};

[[nodiscard]] std::size_t rankOf(SurveySection section)
{
    return static_cast<std::size_t>(
        std::find(kSectionOrder.begin(), kSectionOrder.end(), section) - kSectionOrder.begin());
}

// Which pass of the sections each rule is written in.
//
// Among rules of ONE key the earlier wins a field both fill, and some fields
// are filled by more than one section - a comment by any, the string's
// attributes by pipe_data and string_attribute_data, each vertex's by
// vertex_pipe_data and vertex_attribute_data. So a rule may never be written
// ahead of an earlier rule of its own key, and when 12d's section order would
// put it there it goes in a further pass of the sections instead. Rules of
// DIFFERENT keys may pass each other freely: no code matches two different
// keys of one specificity, so their order never decides anything.
//
// Comparing with the key's latest rule is enough, because along a key's
// rules (pass, section rank) only ever grows.
[[nodiscard]] std::vector<std::size_t> passesOf(const std::vector<SurveyRule>& rules)
{
    std::map<std::string, std::pair<std::size_t, std::size_t>, std::less<>> latest;
    std::vector<std::size_t> passes;
    passes.reserve(rules.size());
    for (const SurveyRule& rule : rules) {
        const std::size_t rank = rankOf(rule.section);
        std::size_t pass = 0;
        if (const auto found = latest.find(rule.key); found != latest.end()) {
            const auto [latestPass, latestRank] = found->second;
            pass = rank < latestRank ? latestPass + 1 : latestPass;
        }
        latest[rule.key] = {pass, rank};
        passes.push_back(pass);
    }
    return passes;
}

// As in the .4d writer: the shortest plain decimal that reads back exactly.
[[nodiscard]] std::string number(double value)
{
    char buffer[1100];
    const auto written =
        std::to_chars(buffer, buffer + sizeof buffer, value, std::chars_format::fixed);
    if (written.ec != std::errc{}) {
        return "0"; // unreachable for a finite value; validate() refuses the rest
    }
    return {buffer, written.ptr};
}

// A character XML 1.0 cannot carry at all, even as a reference.
[[nodiscard]] bool forbiddenInXml(unsigned char ch)
{
    return ch < 0x20 && ch != '\t' && ch != '\n' && ch != '\r';
}

class Writer {
  public:
    explicit Writer(const MapFileWriteOptions& options) : options_(options) {}

    katana::core::Result<std::string> run(const katana::entity::SurveyMap& map)
    {
        out_ += "<?xml version=\"1.0\"?>";
        out_ += kEol;
        // The namespace 12d declares, so that a file written here validates
        // against the schema 12d reads it with.
        out_ += "<xml12d xmlns=\"http://www.12d.com/schema/xml12d-10.0\" "
                "xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" language=\"English\" "
                "version=\"1.0\" xsi:schemaLocation=\"http://www.12d.com/schema/xml12d-10.0 "
                "http://www.12d.com/schema/xml12d-10.0/xml12d.xsd\">";
        out_ += kEol;
        // The units the sizes in the file are in. Katana's drawings are in
        // metres, which is what 12d writes here for a metric project.
        open(1, "meta_data");
        open(2, "units");
        open(3, "metric");
        for (const auto& [name, value] :
             std::array<std::pair<const char*, const char*>, 7>{{{"linear", "metre"},
                                                                 {"area", "square metre"},
                                                                 {"volume", "cubic metre"},
                                                                 {"temperature", "celsius"},
                                                                 {"pressure", "millibars"},
                                                                 {"angular", "decimal degrees"},
                                                                 {"direction", "decimal degrees"}}}) {
            element(4, name, value);
        }
        close(3, "metric");
        close(2, "units");
        close(1, "meta_data");

        open(1, "map_file");
        if (auto status = text(2, "version", options_.version, "the version"); !status) {
            return status.error();
        }
        if (!options_.comments.empty()) {
            open(2, "comments");
            for (const std::string& comment : options_.comments) {
                if (comment.empty()) {
                    indent(3);
                    out_ += "<item/>";
                    out_ += kEol;
                } else if (auto status = text(3, "item", comment, "a comment"); !status) {
                    return status.error();
                }
            }
            close(2, "comments");
        }
        // A map whose every key is in 12d's order - one mapfile as 12d
        // writes it - needs one pass, and is written with each section once.
        const auto& rules = map.rules();
        const std::vector<std::size_t> passes = passesOf(rules);
        const std::size_t passCount =
            passes.empty() ? 0 : *std::max_element(passes.begin(), passes.end()) + 1;
        for (std::size_t pass = 0; pass < passCount; ++pass) {
            for (const SurveySection section : kSectionOrder) {
                const std::string name = katana::entity::toString(section);
                bool opened = false;
                for (std::size_t i = 0; i < rules.size(); ++i) {
                    if (passes[i] != pass || rules[i].section != section) {
                        continue;
                    }
                    if (!opened) {
                        open(2, name);
                        opened = true;
                    }
                    if (auto status = item(rules[i]); !status) {
                        return status.error();
                    }
                }
                if (opened) {
                    close(2, name);
                }
            }
        }
        close(1, "map_file");
        out_ += "</xml12d>";
        out_ += kEol;
        return std::move(out_);
    }

  private:
    void indent(int depth) { out_.append(static_cast<std::size_t>(depth) * 2, ' '); }

    void open(int depth, std::string_view name)
    {
        indent(depth);
        out_ += '<';
        out_ += name;
        out_ += '>';
        out_ += kEol;
    }

    void close(int depth, std::string_view name)
    {
        indent(depth);
        out_ += "</";
        out_ += name;
        out_ += '>';
        out_ += kEol;
    }

    // Trusted text: an element and value this file wrote itself.
    void element(int depth, std::string_view name, std::string_view value)
    {
        indent(depth);
        out_ += '<';
        out_ += name;
        out_ += '>';
        out_ += value;
        out_ += "</";
        out_ += name;
        out_ += '>';
        out_ += kEol;
    }

    // Text from the map: checked, then escaped. Empty text is not written at
    // all - the reader cannot tell it from an absent element, and an absent
    // one is what 12d writes for a field a rule says nothing about.
    [[nodiscard]] Status text(int depth, std::string_view name, std::string_view value,
                              std::string_view what)
    {
        if (value.empty()) {
            return {};
        }
        if (katana::core::isAsciiSpace(value.front()) || katana::core::isAsciiSpace(value.back())) {
            return makeError(ErrorCode::InvalidArgument,
                             std::string(what) + " begins or ends with white space, which XML "
                                                 "does not keep",
                             context(name, value));
        }
        std::string escaped;
        escaped.reserve(value.size());
        for (const char ch : value) {
            const auto byte = static_cast<unsigned char>(ch);
            if (forbiddenInXml(byte)) {
                return makeError(ErrorCode::InvalidArgument,
                                 std::string(what) + " holds a control character XML cannot carry",
                                 context(name, value));
            }
            switch (ch) {
            case '&':
                escaped += "&amp;";
                break;
            case '<':
                escaped += "&lt;";
                break;
            case '>':
                escaped += "&gt;";
                break;
            case '"':
                escaped += "&quot;";
                break;
            case '\'':
                escaped += "&apos;";
                break;
            case '\r':
                // An XML reader turns a bare CR into a line feed; a reference
                // is kept as written.
                escaped += "&#13;";
                break;
            default:
                escaped += ch;
            }
        }
        element(depth, name, escaped);
        return {};
    }

    [[nodiscard]] std::string context(std::string_view name, std::string_view value) const
    {
        const std::string where = "<" + std::string(name) + ">: \"" + std::string(value) + "\"";
        return key_.empty() ? where : "rule \"" + key_ + "\" " + where;
    }

    void flag(int depth, std::string_view name, std::optional<bool> value)
    {
        if (value) {
            element(depth, name, *value ? "yes" : "no");
        }
    }

    // A number the map holds as a plain double where 0 is what the reader
    // makes of an absent element - so 0 is written as nothing, as 12d does.
    void real(int depth, std::string_view name, double value, double absent = 0.0)
    {
        if (value != absent) {
            element(depth, name, number(value));
        }
    }

    [[nodiscard]] Status refuse(std::string_view what) const
    {
        return makeError(ErrorCode::InvalidArgument,
                         "a <" + std::string(katana::entity::toString(section_)) +
                             "> rule cannot hold " + std::string(what) +
                             ", so writing it would lose it",
                         "rule \"" + key_ + "\"");
    }

    // Everything a rule holds that its section has no element for. The
    // reader reads each field only in its own section, so a field anywhere
    // else would be written and never read back.
    [[nodiscard]] Status checkFieldsBelong(const SurveyRule& rule) const
    {
        const SurveySection s = rule.section;
        const bool map = s == SurveySection::Map;
        if (!map && (!rule.model.empty() || !rule.colour.empty() || !rule.linestyle.empty() ||
                     !rule.weight.empty() || !rule.group.empty() || rule.breakline)) {
            return refuse("a model, colour, linestyle, weight, group or breakline");
        }
        if (s != SurveySection::VertexSymbol && (rule.symbol || rule.hide)) {
            return refuse("a symbol or a hide flag");
        }
        if (s != SurveySection::Tinable && rule.tinable) {
            return refuse("a tinable flag");
        }
        if (s != SurveySection::VertexTextStyle && rule.textStyle) {
            return refuse("a text style");
        }
        if ((s != SurveySection::Pipe && rule.pipe) ||
            (s != SurveySection::VertexPipe && rule.vertexPipe) ||
            (s != SurveySection::SegmentPipe && rule.segmentPipe)) {
            return refuse("that kind of pipe");
        }
        if (s != SurveySection::Pipe && s != SurveySection::StringAttribute &&
            !rule.attributes.empty()) {
            return refuse("string attributes");
        }
        if (s != SurveySection::VertexPipe && s != SurveySection::VertexAttribute &&
            !rule.vertexAttributes.empty()) {
            return refuse("vertex attributes");
        }
        if (s != SurveySection::SegmentPipe && !rule.segmentAttributes.empty()) {
            return refuse("segment attributes");
        }
        return {};
    }

    [[nodiscard]] Status attributes(std::string_view container,
                                    const std::vector<SurveyAttribute>& list)
    {
        if (list.empty()) {
            return {};
        }
        open(4, container);
        for (const SurveyAttribute& attribute : list) {
            if (attribute.type != "text" && attribute.type != "integer") {
                return makeError(ErrorCode::InvalidArgument,
                                 "an attribute of type \"" + attribute.type +
                                     "\" is neither text nor integer, the two kinds a mapfile holds",
                                 "rule \"" + key_ + "\"");
            }
            if (attribute.name.empty()) {
                return makeError(ErrorCode::InvalidArgument, "an attribute has no name",
                                 "rule \"" + key_ + "\"");
            }
            open(5, attribute.type);
            if (auto status = text(6, "name", attribute.name, "an attribute name"); !status) {
                return status;
            }
            if (auto status = text(6, "value", attribute.value, "an attribute value"); !status) {
                return status;
            }
            close(5, attribute.type);
        }
        close(4, container);
        return {};
    }

    [[nodiscard]] Status pipe(const std::optional<SurveyPipe>& value)
    {
        if (!value) {
            return {};
        }
        if (value->justify.empty() && value->shape.empty() && value->size1.empty() &&
            value->size2.empty()) {
            // The reader takes a pipe with none of these as no pipe at all.
            return refuse("a pipe with no justify, shape or size");
        }
        for (const auto& [name, field] : std::array<std::pair<const char*, const std::string*>, 4>{
                 {{"justify", &value->justify},
                  {"shape", &value->shape},
                  {"size1", &value->size1},
                  {"size2", &value->size2}}}) {
            if (auto status = text(4, name, *field, "a pipe field"); !status) {
                return status;
            }
        }
        if (value->active) {
            element(4, "active", "yes");
        }
        return {};
    }

    [[nodiscard]] Status item(const SurveyRule& rule)
    {
        key_ = rule.key;
        section_ = rule.section;
        if (rule.key.empty()) {
            // The reader skips a keyless item: it names no code.
            return makeError(ErrorCode::InvalidArgument, "a rule has no key",
                             katana::entity::toString(rule.section));
        }
        if (auto status = checkFieldsBelong(rule); !status) {
            return status;
        }
        open(3, "item");
        Status status = text(4, "key", rule.key, "the key");
        const auto field = [&](std::string_view name, const std::string& value, int depth = 4) {
            if (status) {
                status = text(depth, name, value, "a field");
            }
        };

        switch (rule.section) {
        case SurveySection::Map:
            field("model", rule.model);
            field("colour", rule.colour);
            if (rule.breakline) {
                element(4, "breakline", katana::entity::toString(*rule.breakline));
            }
            field("linestyle", rule.linestyle);
            field("weight", rule.weight);
            break;

        case SurveySection::VertexSymbol:
            if (rule.symbol) {
                if (rule.symbol->style.empty()) {
                    // The reader drops a symbol that names no style.
                    return refuse("a symbol that names no style");
                }
                open(4, "symbol_data");
                field("style", rule.symbol->style, 5);
                field("colour", rule.symbol->colour, 5);
                real(5, "size", rule.symbol->size);
                real(5, "rotation", rule.symbol->rotation);
                real(5, "offset", rule.symbol->offset);
                real(5, "raise", rule.symbol->raise);
                close(4, "symbol_data");
            }
            flag(4, "hide", rule.hide);
            break;

        case SurveySection::VertexTextStyle:
            if (rule.textStyle) {
                const auto& t = *rule.textStyle;
                open(4, "textstyle_data");
                field("textstyle", t.textstyle, 5);
                field("colour", t.colour, 5);
                field("type", t.type, 5);
                real(5, "size", t.size);
                field("justify_x", t.justifyX, 5);
                field("justify_y", t.justifyY, 5);
                real(5, "offset", t.offset);
                real(5, "raise", t.raise);
                real(5, "angle", t.angle);
                real(5, "slant", t.slant);
                real(5, "x_factor", t.widthFactor, 1.0);
                field("weight", t.weight, 5);
                if (t.underline) {
                    element(5, "underline", "yes");
                }
                if (t.strikeout) {
                    element(5, "strikeout", "yes");
                }
                if (t.italic) {
                    element(5, "italic", "yes");
                }
                close(4, "textstyle_data");
            }
            break;

        // The attribute containers are spelled per section, as 12d spells
        // them; the reader accepts any of the four in any section.
        case SurveySection::Pipe:
            if (status) {
                status = attributes("attributes", rule.attributes);
            }
            if (status) {
                status = pipe(rule.pipe);
            }
            break;
        case SurveySection::VertexPipe:
            if (status) {
                status = attributes("vertex_attributes", rule.vertexAttributes);
            }
            if (status) {
                status = pipe(rule.vertexPipe);
            }
            break;
        case SurveySection::SegmentPipe:
            if (status) {
                status = attributes("segment_attributes", rule.segmentAttributes);
            }
            if (status) {
                status = pipe(rule.segmentPipe);
            }
            break;
        case SurveySection::StringAttribute:
            if (status) {
                status = attributes("map_attributes", rule.attributes);
            }
            break;
        case SurveySection::VertexAttribute:
            if (status) {
                status = attributes("map_attributes", rule.vertexAttributes);
            }
            break;
        case SurveySection::Tinable:
            flag(4, "tinable", rule.tinable);
            break;
        }
        field("comment", rule.comment);
        if (rule.section == SurveySection::Map) {
            field("group", rule.group);
        }
        if (!status) {
            return status;
        }
        close(3, "item");
        return {};
    }

    const MapFileWriteOptions& options_;
    std::string out_{};
    std::string key_{};
    SurveySection section_ = SurveySection::Map;
};

} // namespace

katana::core::Result<std::string> writeMapFile(const katana::entity::SurveyMap& map,
                                               const MapFileWriteOptions& options)
{
    return Writer(options).run(map);
}

} // namespace katana::archive12d
