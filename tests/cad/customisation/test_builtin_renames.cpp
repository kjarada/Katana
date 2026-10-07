// The renames the drawing side knows (customisation_record.hpp) against the
// built-in customisation: the names they give now must be the ones the
// built-in has, or a project or drawing saved before the rename would find
// nothing.
//
// Two built-ins are held to it. A small fixture handed over as a host hands
// one over - always, whatever this build compiled in - and the customisation
// compiled into the program, when there is one; that one is third-party
// material, tracked as resources/customisation/nsw.customisation.json, so the
// half of each test that needs it runs in a build made from a clean checkout
// and SKIPS in a build whose KATANA_BUILTIN_CUSTOMISATION names no file.

#include <gtest/gtest.h>

#include <memory>
#include <string>
#include <vector>

#include "katana/cad/customisation_host.hpp"
#include "katana/cad/customisation_record.hpp"
#include "katana/cad/document.hpp"
#include "katana/entity/customisation.hpp"

namespace {

using katana::cad::BuiltInCustomisation;
using katana::cad::CustomisationHost;
using katana::cad::Document;

// A built-in as a host hands one over: two definitions and a rule, written for
// this test, none of them of the reference customisation.
const std::string kFixture = R"({
  "format": "katana-customisation", "version": 1,
  "name": "TEST Built In",
  "linestyles": [
    {"name": "TEST Fence", "strokes": [["move", 0, 0], ["draw", 2, 0]]}
  ],
  "symbols": [
    {"name": "TEST Peg", "atVertices": true, "strokes": [["move", 0, 0], ["circle", 0.5]]}
  ],
  "codes": [
    {"key": "FE*", "sets": "feature", "layer": "TEST FENCES", "linestyle": "TEST Fence"}
  ]
})";

BuiltInCustomisation fixtureBuiltIn()
{
    auto read = katana::entity::customisationFromJson(kFixture);
    EXPECT_TRUE(read.ok()) << (read.ok() ? "" : read.error().describe());
    BuiltInCustomisation builtIn;
    if (read.ok()) {
        builtIn.customisation =
            std::make_shared<const katana::entity::Customisation>(std::move(*read));
        builtIn.digest = katana::entity::customisationDigest(kFixture);
    }
    return builtIn;
}

} // namespace

// Eight earlier names in two sets of four (customisation_record.hpp), and the
// built-in answers for every one of them with its own NAME - one customisation
// now stands where four files did.
TEST(BuiltInRenames, TheFileRenamesAnswerWithTheNameOfTheBuiltIn)
{
    Document document;
    CustomisationHost host;
    host.builtIn = fixtureBuiltIn();
    ASSERT_NE(host.builtIn.customisation, nullptr);
    const katana::cad::CustomisationStart start = katana::cad::startCustomisation(document, host);
    ASSERT_TRUE(start.problems.empty());

    // What the Document answers an earlier name with is the name its host's
    // built-in declares (CustomisationState::builtIn).
    EXPECT_EQ(document.customisationState().builtIn, "TEST Built In");
    const std::vector<katana::cad::RenamedSource> renames =
        katana::cad::builtinRenames(document.customisationState().builtIn);
    ASSERT_EQ(renames.size(), 8u);
    for (const katana::cad::RenamedSource& rename : renames) {
        EXPECT_EQ(rename.now, "TEST Built In");
    }
}

TEST(BuiltInRenames, TheFileRenamesOfTheCompiledInCustomisationAnswerWithItsName)
{
    const BuiltInCustomisation& compiledIn = katana::cad::compiledInCustomisation();
    if (!compiledIn.customisation) {
        GTEST_SKIP() << "this build has no customisation compiled in";
    }
    const std::vector<katana::cad::RenamedSource> renames = katana::cad::builtinRenames();
    ASSERT_EQ(renames.size(), 8u);
    for (const katana::cad::RenamedSource& rename : renames) {
        EXPECT_EQ(rename.now, compiledIn.customisation->name);
    }
}

// Each definition rename gives a name the built-in library defines, and no
// built-in definition is itself known to the table as an earlier name, so the
// retry after a missed lookup can only ever land on a definition that is there.
TEST(BuiltInRenames, NoDefinitionOfAFixtureBuiltInIsKnownAsAnEarlierName)
{
    const BuiltInCustomisation builtIn = fixtureBuiltIn();
    ASSERT_NE(builtIn.customisation, nullptr);
    std::size_t known = 0;
    builtIn.customisation->library.forEach([&](const katana::entity::LineStyle& style) {
        known += katana::cad::definitionNameNow(style.name).empty() ? 0 : 1;
    });
    EXPECT_EQ(known, 0u);
    // And the table itself: a name it gives is never one it also renames, so
    // a retry cannot be sent on a second time.
    for (const katana::cad::RenamedDefinition& rename : katana::cad::builtinDefinitionRenames()) {
        EXPECT_TRUE(katana::cad::definitionNameNow(rename.now).empty()) << rename.now;
    }
}

TEST(BuiltInRenames, EveryRenamedDefinitionIsInTheCompiledInLibraryUnderTheNameItHasNow)
{
    const BuiltInCustomisation& compiledIn = katana::cad::compiledInCustomisation();
    if (!compiledIn.customisation) {
        GTEST_SKIP() << "this build has no customisation compiled in";
    }
    // 29 definitions carried the publisher's word, which the reference
    // customisation was given without (the census's `carried`).
    ASSERT_EQ(katana::cad::builtinDefinitionRenames().size(), 29u);
    const katana::entity::StyleLibrary& library = compiledIn.customisation->library;
    for (const katana::cad::RenamedDefinition& rename : katana::cad::builtinDefinitionRenames()) {
        EXPECT_NE(library.find(std::string(rename.now)), nullptr) << rename.now;
    }
    std::size_t known = 0;
    library.forEach([&](const katana::entity::LineStyle& style) {
        known += katana::cad::definitionNameNow(style.name).empty() ? 0 : 1;
    });
    EXPECT_EQ(known, 0u);
}
