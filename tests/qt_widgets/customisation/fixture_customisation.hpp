#pragma once

// The committed, hand-written customisation fixture in the Katana format
// (tests/data/customisation), and the ONE place the widget tests read it
// from: a test names the fixtures it wants and gets a Document that holds
// them, loaded as a person's would be - read by the format's reader, merged
// in the order named, installed.
//
// The three files, counted by hand from their text:
//   test_linestyles  3 linestyles - TEST Dashed Kerb (paper, Test/Lines),
//                    TEST Gate (two-point, Test/Lines), TEST Water Main
//                    (world, Test/Services, one pen "blue"); none at vertices
//   test_symbols     4 symbols - TEST Survey Mark, TEST U Turn, TEST Valve
//                    (at vertices, Test/Marks; the valve carries one text) and
//                    TEST Tree (NOT at vertices, Test/Vegetation, one pen
//                    "green"), which only the list it sits in makes a symbol
//   test_survey      11 rules over 8 keys - 7 feature (WM*, KB*, AC*, TR*, 1*,
//                    2*, PX*), 3 symbol (AC*, TR*, PX*), 1 attributes (*)
// A definition's source is its customisation's name ("test_symbols"), and a
// session that had no name takes the first loaded customisation's.
//
// Those three say nothing of who wrote them. A second session, written here
// as text, is four customisations that each carry an author's NOTICE, for the
// tests of what travels with a customisation's data (installNoticedSession).
//
// Header-only, so the globbed widget target needs no line for it.

#include <gtest/gtest.h>

#include <filesystem>
#include <span>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/cad/customisation_merge.hpp"
#include "katana/cad/customisation_state.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/path_text.hpp"
#include "katana/entity/customisation.hpp"

namespace katana::qt::test {

// tests/data/customisation, found from this file's own place in the tree
// (tests/qt_widgets/customisation) rather than through a compile definition.
inline std::filesystem::path customisationFixtureDirectory()
{
    return std::filesystem::path(__FILE__).parent_path().parent_path().parent_path() / "data" /
           "customisation";
}

// The file of the fixture `name`: "test_symbols" is
// tests/data/customisation/test_symbols.customisation.json.
inline std::filesystem::path customisationFixtureFile(std::string_view name)
{
    return customisationFixtureDirectory() / (std::string(name) + ".customisation.json");
}

// `text` read by the format's own reader. A failure is the test's, said once
// here, and gives an empty customisation.
inline katana::entity::Customisation customisationFromText(std::string_view text)
{
    auto read = katana::entity::customisationFromJson(text);
    EXPECT_TRUE(read.ok()) << (read.ok() ? std::string() : read.error().describe());
    return read.ok() ? std::move(*read) : katana::entity::Customisation{};
}

// The fixture `name`, read by the format's own reader.
inline katana::entity::Customisation readCustomisationFixture(std::string_view name)
{
    const auto bytes = katana::core::readFileBytes(customisationFixtureFile(name));
    EXPECT_TRUE(bytes.ok()) << (bytes.ok() ? std::string() : bytes.error().describe());
    return bytes.ok() ? customisationFromText(*bytes) : katana::entity::Customisation{};
}

// Loads `loaded` into `document`, merged in the order given on top of
// whatever it holds: mergeCustomisation, then installCustomisation, which is
// what a load is everywhere (cad/customisation_merge.hpp).
inline void installCustomisations(katana::cad::Document& document,
                                  const std::vector<katana::entity::Customisation>& loaded)
{
    katana::cad::CustomisationMerge merged = katana::cad::mergeCustomisation(
        document.customisation(), std::span<const katana::entity::Customisation>(loaded),
        katana::cad::LoadMode::Merge);
    ASSERT_TRUE(merged.ok()) << (merged.problems.empty() ? std::string()
                                                         : merged.problems.front());
    const auto installed = document.installCustomisation(
        std::move(merged.merged), katana::cad::CustomisationOrigin::Loaded);
    ASSERT_TRUE(installed.ok()) << (installed.ok() ? std::string()
                                                   : installed.error().describe());
}

// Loads the named fixtures into `document`, in the order given.
inline void installCustomisationFixtures(
    katana::cad::Document& document,
    const std::vector<std::string>& names = {"test_linestyles", "test_survey", "test_symbols"})
{
    std::vector<katana::entity::Customisation> loaded;
    for (const std::string& name : names) {
        loaded.push_back(readCustomisationFixture(name));
    }
    installCustomisations(document, loaded);
}

// ---- a session whose parts each carry a notice --------------------------------------
//
// Four customisations, loaded in this order into an empty Document:
//
//   base    description "The base set.", notice "Base: all rights reserved.";
//           one symbol, BASE Peg, whose pen is "tint teal"; one rule, PG*
//           feature, layer BASE PEGS, colour "tint teal", drawn as a point
//   client  notice "Client codes, for this job only."; one rule, FN* feature,
//           layer CLIENT FENCES - rules alone, and no colour named
//   marks   notice "Marks drawn by hand."; one symbol, MARK Cross, with no
//           pen - definitions alone
//   tints   notice "Tints: free to use."; the colours "tint teal" #008080 and
//           "tint rose" #FF007F - neither definitions nor rules
//
// What the session then is, by cad/customisation_merge.hpp: it had no name, so
// it is named "base" and takes base's description and notice; its sources, in
// load order, are base (definitions and rules; no notice of its own, that
// having become the session's), client (rules, its notice), marks
// (definitions, its notice) and tints (neither, its notice). Its rules are
// PG* then FN*; its colours both of tints'.
inline constexpr const char* kNoticedBase = R"({
  "format": "katana-customisation",
  "version": 1,
  "name": "base",
  "description": "The base set.",
  "notice": [
    "Base: all rights reserved."
  ],
  "symbols": [
    {"name": "BASE Peg", "atVertices": true, "strokes": [
      ["pen", "tint teal"],
      ["circle", 0.5]
    ]}
  ],
  "codes": [
    {"key": "PG*", "sets": "feature", "layer": "BASE PEGS", "colour": "tint teal", "draw": "point"}
  ]
}
)";

inline constexpr const char* kNoticedClient = R"({
  "format": "katana-customisation",
  "version": 1,
  "name": "client",
  "notice": [
    "Client codes, for this job only."
  ],
  "codes": [
    {"key": "FN*", "sets": "feature", "layer": "CLIENT FENCES"}
  ]
}
)";

inline constexpr const char* kNoticedMarks = R"({
  "format": "katana-customisation",
  "version": 1,
  "name": "marks",
  "notice": [
    "Marks drawn by hand."
  ],
  "symbols": [
    {"name": "MARK Cross", "atVertices": true, "strokes": [
      ["move", -0.5, 0],
      ["draw", 0.5, 0]
    ]}
  ]
}
)";

inline constexpr const char* kNoticedTints = R"({
  "format": "katana-customisation",
  "version": 1,
  "name": "tints",
  "notice": [
    "Tints: free to use."
  ],
  "colours": {
    "tint rose": "#FF007F",
    "tint teal": "#008080"
  }
}
)";

inline void installNoticedSession(katana::cad::Document& document)
{
    installCustomisations(document,
                          {customisationFromText(kNoticedBase),
                           customisationFromText(kNoticedClient),
                           customisationFromText(kNoticedMarks),
                           customisationFromText(kNoticedTints)});
}

} // namespace katana::qt::test
