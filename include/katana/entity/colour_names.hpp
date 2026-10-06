#pragma once

// Colour NAMES, and the colours they mean.
//
// A survey code rule, the symbol it gives and a pen inside a linestyle
// definition all NAME their colour - "red", "dark grey", "sui water potable" -
// and so does every string of a .12da archive. Two things turn a name into a
// colour: the standard names, which mean the same in every drawing, and a
// customisation's own table for the names only it uses.
//
// They are in the entity layer because three layers resolve a name and may not
// all see each other (tools/check_layering.cmake): `cad` codes a survey,
// `archive12d` imports an archive, `qt` paints a definition. The standard names
// began as a table private to archive12d, out of cad's reach, so a front end
// had to hand cad a function to ask.
//
// ONE FOLD. A name is compared after foldColourName, and nothing compares a
// colour name any other way: were the standard names and a customisation's
// table folded differently ("gray" grey to one and unknown to the other), one
// name would have two colours.

#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"

namespace katana::entity {

// The form a colour name is compared in: ASCII letters in lower case, no
// blanks at either end, `_` and `-` read as a blank, and "gray" as "grey"
// where it is the whole name or its last word ("Dark_Gray" is "dark grey").
// Blanks inside a name are kept as they are: "dark  red", with two, is not
// "dark red".
[[nodiscard]] std::string foldColourName(std::string_view name);

// The colour of one of the 27 standard names, compared by foldColourName.
// nullopt for any other name: a colour outside the standard ones is defined by
// a customisation (ColourTable below) or by nothing Katana has, and such a
// name ("pen 025") leaves a colour alone rather than being guessed at.
[[nodiscard]] std::optional<Color> standardColour(std::string_view name);
// The 27 names, lower case, in the table's order: the plain names first, then
// the dark and the light shades. For a colour field to list, so that it offers
// nothing standardColour would not draw and keeps no copy of the table.
[[nodiscard]] std::vector<std::string> standardColourNames();
// The standard name nearest to `colour` by distance in RGB - what an archive
// export writes for a colour that never had a name.
[[nodiscard]] std::string nearestStandardColour(const Color& colour);

// A customisation's own colour names: "sui electricity" is #FF7F00. A value
// type, compared whole.
class ColourTable {
  public:
    struct Entry {
        std::string name{}; // as written; it is found by its fold
        Color colour{};

        friend bool operator==(const Entry&, const Entry&) = default;
    };

    // Fails with InvalidArgument for a name that is empty or nothing but blanks
    // and separators, or is not valid UTF-8; for a name whose fold another
    // entry already has ("SUI Gas" beside "sui_gas": a lookup could not say
    // which was meant); and for a name whose fold is a standard name. That
    // last one is the rule that keeps one name one colour: an archive import
    // colours its strings from the standard names alone, having no
    // customisation to ask, so a table that redefined "red" would draw two
    // reds in one drawing.
    [[nodiscard]] katana::core::Status add(std::string name, const Color& colour);

    // The colour of `name`, compared by foldColourName; nullopt when the table
    // does not have it.
    [[nodiscard]] std::optional<Color> find(std::string_view name) const;

    // Every entry, in the order of the folded names - the order a file lists
    // them in, so that writing a table twice gives the same text.
    [[nodiscard]] std::vector<Entry> entries() const;

    [[nodiscard]] std::size_t size() const { return entries_.size(); }
    [[nodiscard]] bool empty() const { return entries_.empty(); }

    friend bool operator==(const ColourTable&, const ColourTable&) = default;

  private:
    // Keyed by the fold, which is the identity; the name as written is kept so
    // that it can be shown and written back as its author spelled it.
    std::map<std::string, Entry, std::less<>> entries_{};
};

// THE resolver of a colour name: the customisation's table, then the standard
// names. nullopt when neither knows the name, and the caller then leaves the
// colour alone. (The table cannot hold a standard name, so the order decides
// nothing today; it is stated so that it stays decided.)
[[nodiscard]] std::optional<Color> resolveColour(const ColourTable& table, std::string_view name);

} // namespace katana::entity
