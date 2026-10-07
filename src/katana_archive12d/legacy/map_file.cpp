#include "map_file.hpp"

#include <algorithm>
#include <array>
#include <utility>

#include "customisation_words.hpp"
#include "katana/archive12d/reader.hpp"
#include "katana/core/xml.hpp"
#include "text_utilities.hpp"

namespace katana::archive12d {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::XmlNode;
using katana::entity::SurveyAttribute;
using katana::entity::SurveyPipe;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;
using katana::entity::SurveySymbol;
using katana::entity::SurveyTextStyle;

constexpr std::size_t kMaxWarnings = 100;

// Every element an <item> may hold, in any section. Anything else is counted
// and named, so that a mapfile using a field this does not know says so
// rather than losing it quietly.
constexpr std::array<std::string_view, 18> kItemFields{
    "key",     "model",  "colour",  "linestyle",    "weight",         "group",
    "comment", "hide",   "tinable", "breakline",    "symbol_data",    "textstyle_data",
    "justify", "shape",  "size1",   "size2",        "active",         "attributes"};

// The containers of attributes, which differ by section although they mean
// the same thing: a name and a value attached to something.
constexpr std::array<std::string_view, 4> kAttributeContainers{
    "attributes", "vertex_attributes", "segment_attributes", "map_attributes"};

// The <map_file> element, wherever it is: 12d wraps it in <xml12d>, but a
// hand-made file need not.
[[nodiscard]] const XmlNode* findMapFile(const XmlNode& node, std::size_t depth = 0)
{
    if (node.name == "map_file") {
        return &node;
    }
    if (depth > 8) {
        return nullptr;
    }
    for (const XmlNode& child : node.children) {
        if (const XmlNode* found = findMapFile(child, depth + 1)) {
            return found;
        }
    }
    return nullptr;
}

class MapReader {
  public:
    MapReader(katana::entity::SurveyMap map, std::string_view text) : text_(text)
    {
        result_.map = std::move(map);
    }

    katana::core::Result<MapFileRead> run()
    {
        auto document = katana::core::readXml(text_);
        if (!document) {
            return document.error();
        }
        const XmlNode* mapFile = findMapFile(*document);
        if (mapFile == nullptr) {
            return makeError(ErrorCode::InvalidArgument,
                             "this is XML, but there is no <map_file> in it", document->name);
        }
        result_.version = mapFile->childText("version");
        if (const XmlNode* comments = mapFile->child("comments")) {
            for (const XmlNode* item : comments->childrenNamed("item")) {
                if (!item->text.empty()) {
                    result_.comments.push_back(item->text);
                }
            }
        }
        // A mapfile is written in sections, and it is the SECTION that says
        // what its rules are about - <map_attributes> means attributes on the
        // string inside string_attribute_data and attributes on each vertex
        // inside vertex_attribute_data. Reading only <map_data>, which is
        // what this first did, takes 457 of the 725 rules and quietly loses
        // every symbol.
        for (const XmlNode& section : mapFile->children) {
            if (section.name == "version" || section.name == "comments") {
                continue;
            }
            const auto which = detail::sectionOfElement(section.name);
            const auto items = section.childrenNamed("item");
            if (!which) {
                count(section.name, false);
                warn("<" + section.name +
                     "> is not a section this survey code file reader knows; its " +
                     std::to_string(items.size()) + " rules were not read");
                continue;
            }
            count(section.name, true);
            for (const XmlNode* item : items) {
                readItem(*item, *which, section.name);
            }
        }
        finishWarnings();
        return std::move(result_);
    }

  private:
    void warn(std::string text)
    {
        if (result_.warnings.size() < kMaxWarnings) {
            result_.warnings.push_back(std::move(text));
            return;
        }
        ++suppressed_;
    }

    void finishWarnings()
    {
        if (suppressed_ != 0) {
            result_.warnings.push_back(std::to_string(suppressed_) + " further warnings not shown");
        }
    }

    void count(std::string_view element, bool understood)
    {
        const auto found =
            std::find_if(result_.tally.begin(), result_.tally.end(),
                         [element](const ElementTally& t) { return t.keyword == element; });
        ElementTally& tally =
            found == result_.tally.end()
                ? result_.tally.emplace_back(ElementTally{std::string(element), 0, 0})
                : *found;
        ++tally.read;
        tally.imported += understood ? 1 : 0;
    }

    // A number, or nothing said. An empty element - `<rotation/>` - is not a
    // zero, it is silence, and 388 of the mapfiles' symbols are written that
    // way.
    [[nodiscard]] std::optional<double> number(const XmlNode& node, std::string_view name,
                                               const std::string& key)
    {
        const XmlNode* child = node.child(name);
        if (child == nullptr || child->text.empty()) {
            return std::nullopt;
        }
        const auto value = parseReal(child->text);
        if (!value) {
            warn("rule \"" + key + "\": <" + std::string(name) + "> is \"" + child->text +
                 "\", which is not a number");
        }
        return value;
    }

    [[nodiscard]] std::optional<bool> boolean(const XmlNode& node, std::string_view name,
                                              const std::string& key)
    {
        const XmlNode* child = node.child(name);
        if (child == nullptr || child->text.empty()) {
            return std::nullopt;
        }
        const auto value = detail::parseBoolean(child->text);
        if (!value) {
            warn("rule \"" + key + "\": <" + std::string(name) + "> is \"" + child->text +
                 "\", which is not a yes or a no");
        }
        return value;
    }

    // `<container><text><name>..</name><value>..</value></text></container>`,
    // whichever of the four spellings the section uses for the container.
    [[nodiscard]] std::vector<SurveyAttribute> attributes(const XmlNode& item,
                                                          const std::string& key)
    {
        std::vector<SurveyAttribute> found;
        for (const std::string_view container : kAttributeContainers) {
            const XmlNode* block = item.child(container);
            if (block == nullptr) {
                continue;
            }
            for (const XmlNode& typed : block->children) {
                const bool known = typed.name == "text" || typed.name == "integer";
                count(std::string(container) + "/" + typed.name, known);
                if (!known) {
                    warn("rule \"" + key + "\": <" + std::string(container) + "> holds a <" +
                         typed.name + ">, which is not a kind of attribute this reads");
                    continue;
                }
                SurveyAttribute attribute;
                attribute.type = typed.name;
                attribute.name = typed.childText("name");
                attribute.value = typed.childText("value");
                if (attribute.name.empty()) {
                    warn("rule \"" + key + "\": an attribute has no name");
                    continue;
                }
                found.push_back(std::move(attribute));
            }
        }
        return found;
    }

    [[nodiscard]] std::optional<SurveyPipe> pipe(const XmlNode& item, const std::string& key)
    {
        SurveyPipe out;
        out.justify = item.childText("justify");
        out.shape = item.childText("shape");
        out.size1 = item.childText("size1");
        out.size2 = item.childText("size2");
        out.active = boolean(item, "active", key).value_or(false);
        if (out.justify.empty() && out.shape.empty() && out.size1.empty() && out.size2.empty()) {
            return std::nullopt;
        }
        return out;
    }

    void readItem(const XmlNode& item, SurveySection section, const std::string& sectionName)
    {
        count("item", true);
        SurveyRule rule;
        rule.section = section;
        rule.key = item.childText("key");
        if (rule.key.empty()) {
            // The second survey code file of the built-in customisation ends
            // with an <item> holding only a <group>. It names no code, so
            // there is nothing it could ever apply to.
            if (!item.children.empty()) {
                warn("an <item> of <" + sectionName + "> has no <key> and was skipped");
            }
            return;
        }
        for (const XmlNode& field : item.children) {
            const bool known =
                std::find(kItemFields.begin(), kItemFields.end(), field.name) !=
                    kItemFields.end() ||
                std::find(kAttributeContainers.begin(), kAttributeContainers.end(), field.name) !=
                    kAttributeContainers.end();
            count(field.name, known);
            if (!known) {
                warn("rule \"" + rule.key + "\" of <" + sectionName + ">: <" + field.name +
                     "> is not a field this survey code file reader knows");
            }
        }
        rule.comment = item.childText("comment");

        switch (section) {
        case SurveySection::Map:
            rule.model = item.childText("model");
            rule.colour = item.childText("colour");
            rule.linestyle = item.childText("linestyle");
            rule.weight = item.childText("weight");
            rule.group = item.childText("group");
            if (const XmlNode* breakline = item.child("breakline");
                breakline != nullptr && !breakline->text.empty()) {
                rule.breakline = katana::entity::parseSurveyBreakline(breakline->text);
                if (!rule.breakline) {
                    warn("rule \"" + rule.key + "\": <breakline> is \"" + breakline->text +
                         "\", which is neither a line nor a point");
                }
            }
            break;

        case SurveySection::VertexSymbol: {
            rule.hide = boolean(item, "hide", rule.key);
            const XmlNode* symbol = item.child("symbol_data");
            if (symbol == nullptr) {
                warn("rule \"" + rule.key + "\" of <" + sectionName + "> has no <symbol_data>");
                break;
            }
            SurveySymbol out;
            out.style = symbol->childText("style");
            out.colour = symbol->childText("colour");
            out.size = number(*symbol, "size", rule.key).value_or(0.0);
            out.rotation = number(*symbol, "rotation", rule.key).value_or(0.0);
            out.offset = number(*symbol, "offset", rule.key).value_or(0.0);
            out.raise = number(*symbol, "raise", rule.key).value_or(0.0);
            if (out.style.empty()) {
                warn("rule \"" + rule.key + "\": <symbol_data> names no <style>");
                break;
            }
            rule.symbol = out;
            break;
        }

        case SurveySection::VertexTextStyle: {
            const XmlNode* text = item.child("textstyle_data");
            if (text == nullptr) {
                warn("rule \"" + rule.key + "\" of <" + sectionName + "> has no <textstyle_data>");
                break;
            }
            SurveyTextStyle out;
            out.textstyle = text->childText("textstyle");
            out.colour = text->childText("colour");
            out.type = text->childText("type");
            out.justifyX = text->childText("justify_x");
            out.justifyY = text->childText("justify_y");
            out.weight = text->childText("weight");
            out.size = number(*text, "size", rule.key).value_or(0.0);
            out.offset = number(*text, "offset", rule.key).value_or(0.0);
            out.raise = number(*text, "raise", rule.key).value_or(0.0);
            out.angle = number(*text, "angle", rule.key).value_or(0.0);
            out.slant = number(*text, "slant", rule.key).value_or(0.0);
            out.widthFactor = number(*text, "x_factor", rule.key).value_or(1.0);
            out.underline = boolean(*text, "underline", rule.key).value_or(false);
            out.strikeout = boolean(*text, "strikeout", rule.key).value_or(false);
            out.italic = boolean(*text, "italic", rule.key).value_or(false);
            rule.textStyle = out;
            break;
        }

        case SurveySection::Pipe:
            rule.pipe = pipe(item, rule.key);
            rule.attributes = attributes(item, rule.key);
            break;
        case SurveySection::VertexPipe:
            rule.vertexPipe = pipe(item, rule.key);
            rule.vertexAttributes = attributes(item, rule.key);
            break;
        case SurveySection::SegmentPipe:
            rule.segmentPipe = pipe(item, rule.key);
            rule.segmentAttributes = attributes(item, rule.key);
            break;

        case SurveySection::StringAttribute:
            rule.attributes = attributes(item, rule.key);
            break;
        case SurveySection::VertexAttribute:
            rule.vertexAttributes = attributes(item, rule.key);
            break;

        case SurveySection::Tinable:
            rule.tinable = boolean(item, "tinable", rule.key);
            break;
        }

        if (auto status = result_.map.add(std::move(rule)); !status) {
            warn("rule skipped: " + status.error().describe());
        }
    }

    std::string_view text_;
    MapFileRead result_{};
    std::size_t suppressed_ = 0;
};

} // namespace

katana::core::Result<MapFileRead> readMapFile(std::string_view text)
{
    return readMapFileInto({}, text);
}

katana::core::Result<MapFileRead> readMapFileInto(katana::entity::SurveyMap map,
                                                  std::string_view text)
{
    MapReader reader(std::move(map), text);
    return reader.run();
}

} // namespace katana::archive12d
