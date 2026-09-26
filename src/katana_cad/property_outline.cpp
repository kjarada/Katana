#include "katana/cad/property_outline.hpp"

#include <algorithm>
#include <map>
#include <set>
#include <string>
#include <utility>

#include "katana/core/text.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::PropertyMap;
using katana::entity::PropertyValue;

namespace {

constexpr bool isDigit(char c)
{
    return c >= '0' && c <= '9';
}

// <0, 0 or >0, comparing runs of digits as numbers and, with `foldCase`,
// letters without regard to case.
int naturalCompare(std::string_view a, std::string_view b, bool foldCase)
{
    std::size_t i = 0;
    std::size_t j = 0;
    while (i < a.size() && j < b.size()) {
        if (isDigit(a[i]) && isDigit(b[j])) {
            // A number of any length: past its leading zeros, the longer run
            // is the larger number, and runs of one length compare as text.
            std::size_t endA = i;
            while (endA < a.size() && isDigit(a[endA])) {
                ++endA;
            }
            std::size_t endB = j;
            while (endB < b.size() && isDigit(b[endB])) {
                ++endB;
            }
            std::size_t startA = i;
            while (startA + 1 < endA && a[startA] == '0') {
                ++startA;
            }
            std::size_t startB = j;
            while (startB + 1 < endB && b[startB] == '0') {
                ++startB;
            }
            const std::size_t lengthA = endA - startA;
            const std::size_t lengthB = endB - startB;
            if (lengthA != lengthB) {
                return lengthA < lengthB ? -1 : 1;
            }
            if (const int digits = a.substr(startA, lengthA).compare(b.substr(startB, lengthB));
                digits != 0) {
                return digits;
            }
            i = endA;
            j = endB;
            continue;
        }
        const char x = foldCase ? katana::core::asciiLower(a[i]) : a[i];
        const char y = foldCase ? katana::core::asciiLower(b[j]) : b[j];
        if (x != y) {
            return static_cast<unsigned char>(x) < static_cast<unsigned char>(y) ? -1 : 1;
        }
        ++i;
        ++j;
    }
    if (i < a.size()) {
        return 1;
    }
    return j < b.size() ? -1 : 0;
}

// What one level gathers for one name beneath the path.
struct Gathered {
    std::size_t holders = 0;
    const PropertyValue* first = nullptr;
    bool differs = false;
    // Views into the maps' keys, which outlive the call.
    std::set<std::string_view> children;
    std::size_t values = 0;
};

// The keys of `properties` beneath `prefix` ("" for all of them).
template <typename Visit>
void forEachBeneath(const PropertyMap& properties, std::string_view prefix, Visit&& visit)
{
    for (auto at = properties.lower_bound(prefix);
         at != properties.end() && std::string_view(at->first).starts_with(prefix); ++at) {
        visit(std::string_view(at->first).substr(prefix.size()), at->second);
    }
}

Result<std::size_t> wholeNumber(const std::string& word, std::string_view what, std::size_t least)
{
    const auto parsed = katana::core::parseInteger(word);
    if (!parsed || *parsed < static_cast<std::int64_t>(least)) {
        return makeError(ErrorCode::ParseFailure,
                         std::string(what) + " takes a whole number" +
                             (least > 0 ? " of at least " + std::to_string(least) : std::string()),
                         word);
    }
    return static_cast<std::size_t>(*parsed);
}

} // namespace

bool naturalLess(std::string_view a, std::string_view b)
{
    if (const int folded = naturalCompare(a, b, true); folded != 0) {
        return folded < 0;
    }
    // Equal but for case or leading zeros: the plain order, which is total.
    return a < b;
}

std::vector<PropertyOutlineEntry> propertyOutline(const PropertyMap& properties,
                                                  std::string_view path)
{
    const PropertyMap* const maps[] = {&properties};
    return propertyOutline(std::span<const PropertyMap* const>(maps), path);
}

std::vector<PropertyOutlineEntry> propertyOutline(std::span<const PropertyMap* const> maps,
                                                  std::string_view path)
{
    const std::string prefix = path.empty() ? std::string() : std::string(path) + '/';
    std::map<std::string_view, Gathered> gathered;
    for (const PropertyMap* properties : maps) {
        if (properties == nullptr) {
            continue;
        }
        forEachBeneath(*properties, prefix, [&](std::string_view rest, const PropertyValue& value) {
            const std::size_t slash = rest.find('/');
            Gathered& entry = gathered[rest.substr(0, slash)];
            if (slash == std::string_view::npos) {
                ++entry.holders;
                if (entry.first == nullptr) {
                    entry.first = &value;
                } else if (*entry.first != value) {
                    entry.differs = true;
                }
                return;
            }
            const std::string_view below = rest.substr(slash + 1);
            entry.children.insert(below.substr(0, below.find('/')));
            ++entry.values;
        });
    }

    std::vector<PropertyOutlineEntry> entries;
    entries.reserve(gathered.size());
    for (const auto& [name, entry] : gathered) {
        PropertyOutlineEntry out;
        out.name = std::string(name);
        out.path = prefix + out.name;
        out.holders = entry.holders;
        out.varies = entry.holders > 0 && (entry.differs || entry.holders < maps.size());
        if (entry.holders > 0 && !out.varies) {
            out.value = *entry.first;
        }
        out.children = entry.children.size();
        out.values = entry.values;
        entries.push_back(std::move(out));
    }
    std::ranges::sort(entries, [](const PropertyOutlineEntry& a, const PropertyOutlineEntry& b) {
        return naturalLess(a.name, b.name);
    });
    return entries;
}

bool hasPropertiesUnder(const PropertyMap& properties, std::string_view path)
{
    if (path.empty()) {
        return !properties.empty();
    }
    if (properties.contains(path)) {
        return true;
    }
    const std::string prefix = std::string(path) + '/';
    const auto at = properties.lower_bound(prefix);
    return at != properties.end() && std::string_view(at->first).starts_with(prefix);
}

Result<std::string> propertyTreeReply(const Document& document,
                                      const std::vector<std::string>& words,
                                      const ScopeViewProvider& views)
{
    std::size_t at = 0;
    auto scope = parseScopeWords(words, at);
    if (!scope) {
        return scope.error();
    }
    std::string under;
    std::size_t from = 0;
    std::size_t limit = kPropertyTreePage;
    for (; at < words.size(); ++at) {
        const std::string word = katana::core::lowered(words[at]);
        const bool last = at + 1 >= words.size();
        if (word == "under" || word == "from" || word == "limit") {
            if (last) {
                return makeError(ErrorCode::ParseFailure,
                                 word == "under" ? "UNDER needs a path after it"
                                                 : "FROM and LIMIT need a number after them",
                                 words[at]);
            }
            const std::string& value = words[++at];
            if (word == "under") {
                under = value;
                // "vertex/3/" and "vertex/3" name one branch.
                while (!under.empty() && under.back() == '/') {
                    under.pop_back();
                }
            } else {
                auto number =
                    wholeNumber(value, word == "from" ? "FROM" : "LIMIT", word == "from" ? 0 : 1);
                if (!number) {
                    return number.error();
                }
                (word == "from" ? from : limit) = *number;
            }
            continue;
        }
        return makeError(ErrorCode::ParseFailure,
                         "not a PROP TREE word; PROP TREE [scope] [WHERE k=v ...] [UNDER path] "
                         "[FROM n] [LIMIT n]",
                         words[at]);
    }

    auto match = matchScope(document, *scope, views);
    if (!match) {
        return match.error();
    }
    std::vector<const PropertyMap*> maps;
    maps.reserve(match->matched.size());
    for (const katana::entity::EntityId id : match->matched) {
        if (const katana::entity::Entity* entity = document.model().entities.find(id)) {
            maps.push_back(&entity->properties);
        }
    }
    const std::vector<PropertyOutlineEntry> entries = propertyOutline(maps, under);
    const std::size_t first = std::min(from, entries.size());
    const std::size_t shown = std::min(limit, entries.size() - first);

    std::string reply = scopeRecord(*match) + " under=" + recordValue(under) +
                        " entries=" + std::to_string(entries.size()) +
                        " from=" + std::to_string(first) + " shown=" + std::to_string(shown);
    for (std::size_t i = first; i < first + shown; ++i) {
        const PropertyOutlineEntry& entry = entries[i];
        reply += "\npath=" + recordValue(entry.path);
        if (entry.value) {
            reply += " value=" + recordValue(katana::entity::toString(*entry.value)) +
                     " type=" + std::string(katana::entity::typeName(*entry.value));
        } else if (entry.varies) {
            reply += " varies=yes holders=" + std::to_string(entry.holders);
        }
        if (entry.children > 0) {
            reply += " children=" + std::to_string(entry.children) +
                     " values=" + std::to_string(entry.values);
        }
    }
    return reply;
}

} // namespace katana::cad
