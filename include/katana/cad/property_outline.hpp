#pragma once

// An entity's properties read as the tree their names describe, one level at
// a time: what the Properties panel shows and what PROP TREE replies.
//
// The model keeps one flat map of properties per entity (entity.hpp); an
// archive import flattens its attribute groups into it with '/' - "Asset/
// Dimensions/Size" - and a string's per-vertex attributes as "vertex/3/Name"
// (docs/desktop.md, "The attribute manager"). A surveyed string of a
// thousand vertices with thirty attributes each is thirty thousand keys:
// shown flat, the panel was a list nobody could read, and built all at once it
// was thirty thousand rows made before anything could be seen. The owner's
// request of 2026-09-26 was a tree "because some lines are very long and
// contain lots of attributes in the vertices, so need a smart way to load all
// these information".
//
// So an outline is asked for ONE LEVEL: the entries directly beneath a path,
// each saying how much lies beneath it, and a front end asks for the next
// level only when a person opens it - or an agent names it (PROP TREE ...
// UNDER path). Nothing is stored: the tree is a reading of the names, as the
// layer tree is a reading of '/'-separated layer paths (layer_path.hpp).
//
// Several entities are read together as the attribute manager reads them: a
// value shows only where every one of them holds the same value at that
// name; one that only some hold, or that they hold differently, VARIES,
// because showing the first entity's value would put words in the others'
// mouths.

#include <cstddef>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"

namespace katana::cad {

// One entry directly beneath a path.
struct PropertyOutlineEntry {
    // The part of the name after the parent's path: "Dimensions", "3".
    std::string name;
    // The whole name up to here: "Asset/Dimensions". A key when a value is
    // stored under it (`holders` > 0), and a prefix when names go on beneath
    // it (`children` > 0); it can be both.
    std::string path;
    // How many of the entities read store a value under exactly `path`.
    std::size_t holders = 0;
    // That value, when every entity read holds one and they all agree.
    std::optional<katana::entity::PropertyValue> value{};
    // Some hold it and some do not, or they hold different values.
    bool varies = false;
    // The distinct names directly beneath `path`, and every value beneath it
    // at any depth (counted once per entity that holds it).
    std::size_t children = 0;
    std::size_t values = 0;

    friend bool operator==(const PropertyOutlineEntry&, const PropertyOutlineEntry&) = default;
};

// The order entries are listed in: digits compare as whole numbers, so
// "vertex/2" comes before "vertex/10" - an archive numbers vertices from 1 and
// the map's own order would put 10 to 19 between 1 and 2 - and letters
// without regard to case, a tie then broken by case, so the order is total.
[[nodiscard]] bool naturalLess(std::string_view a, std::string_view b);

// The entries directly beneath `path` in `properties`, in naturalLess order.
// `path` is "" for the top, or a name without a trailing '/'. A path nothing
// is stored beneath gives no entries; asking costs the keys beneath `path`,
// never the whole map.
[[nodiscard]] std::vector<PropertyOutlineEntry>
propertyOutline(const katana::entity::PropertyMap& properties, std::string_view path);

// The same over several maps read as one (see the top of this file for what
// varies). Empty `maps`: no entries.
[[nodiscard]] std::vector<PropertyOutlineEntry>
propertyOutline(std::span<const katana::entity::PropertyMap* const> maps, std::string_view path);

// Whether anything is stored under `path` or beneath it: what tells a front
// end to draw an expander before it reads the level. O(log n).
[[nodiscard]] bool hasPropertiesUnder(const katana::entity::PropertyMap& properties,
                                      std::string_view path);

// ---- PROP TREE --------------------------------------------------------------------------

// The most entries one PROP TREE reply lists unless LIMIT says otherwise: a
// page an agent reads at once, as the panel shows a page at a time.
inline constexpr std::size_t kPropertyTreePage = 200;

// PROP TREE [scope] [WHERE k=v ...] [UNDER path] [FROM n] [LIMIT n] - `words`
// are those after TREE. The scope is the shared grammar (scope_verbs.hpp),
// the selection when none is given; `views` answers VIEW. The reply is one
// record for the level, then one per entry:
//
//   scope=selection matched=1 under="" entries=3 from=0 shown=3
//   path=Asset children=2 values=5
//   path=Name value=KERB type=text
//   path=vertex children=1250 values=37500
//
// An entry holding a value says value= and type=, or varies=yes with how
// many hold it (holders=); one with names beneath it says children= and
// values=. A scope that takes nothing is reported (matched=0 entries=0), not
// refused. ParseFailure for a word that is none of these, FROM or LIMIT that
// is not a whole number (LIMIT at least 1), or UNDER without a path.
[[nodiscard]] katana::core::Result<std::string>
propertyTreeReply(const Document& document, const std::vector<std::string>& words,
                  const ScopeViewProvider& views);

} // namespace katana::cad
