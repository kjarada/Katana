// The renames the drawing side knows (customisation_record.hpp) against the
// built-in customisation: the names they give now must be the ones the
// built-in has, or a project or drawing saved before the rename would find
// nothing.
//
// Two built-ins are held to it. A small fixture handed over as a host hands
// one over - always, whatever this build compiled in - and the NSW customisation
// compiled into the program, when there is one: its library for the definition
// renames (the 29 definitions were its own), and its name, which the file
// renames answer with in every build. The NSW one is
// third-party material, tracked as resources/customisation/nsw.customisation.json,
// so the half of a test that needs it runs in a build made from a clean
// checkout and SKIPS in a build whose KATANA_ALTERNATE_CUSTOMISATIONS names no
// such file.

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
// table answers for every one of them with the NAME it is given - one
// customisation now stands where four files did. Which name that is for a
// session is not its host's built-in's: see the next test, and
// CustomisationState.TheEarlierFileNamesAreAnsweredByNswAndByNoOtherBuiltIn.
TEST(BuiltInRenames, TheFileRenamesAnswerWithTheNameTheTableIsGiven)
{
    Document document;
    CustomisationHost host;
    host.builtIn = fixtureBuiltIn();
    ASSERT_NE(host.builtIn.customisation, nullptr);
    const katana::cad::CustomisationStart start = katana::cad::startCustomisation(document, host);
    ASSERT_TRUE(start.problems.empty());

    // The host's built-in is the built-in of the run (CustomisationState::builtIn)...
    EXPECT_EQ(document.customisationState().builtIn, "TEST Built In");
    // ...and a table made for any name answers with it.
    for (const char* name : {"TEST Built In", "Another"}) {
        const std::vector<katana::cad::RenamedSource> renames = katana::cad::builtinRenames(name);
        ASSERT_EQ(renames.size(), 8u);
        for (const katana::cad::RenamedSource& rename : renames) {
            EXPECT_EQ(rename.now, name);
        }
    }
}

// The eight earlier names were the four files of the NSW customisation, so they
// are answered by that one and by no other: the default of a program is Katana
// Standard, which defines none of what they drew, and answering them with it
// would say that a project drawn with NSW's linestyles has its customisation
// loaded when it has not. The form with no name is the Document's table
// (Document::customisationRenames), whatever this build compiled in.
TEST(BuiltInRenames, TheFileRenamesWithNoNameAnswerWithNswWhateverIsCompiledIn)
{
    const std::vector<katana::cad::RenamedSource> renames = katana::cad::builtinRenames();
    ASSERT_EQ(renames.size(), 8u);
    for (const katana::cad::RenamedSource& rename : renames) {
        EXPECT_EQ(rename.now, "NSW");
    }
    EXPECT_EQ(katana::cad::kEarlierFileNamesBelongTo, "NSW");
    // And not with the default's name, which is what they were answered with
    // when the default was NSW.
    const BuiltInCustomisation& compiledIn = katana::cad::compiledInCustomisation();
    if (compiledIn.customisation && compiledIn.customisation->name != "NSW") {
        for (const katana::cad::RenamedSource& rename : renames) {
            EXPECT_NE(rename.now, compiledIn.customisation->name);
        }
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

TEST(BuiltInRenames, EveryRenamedDefinitionIsInTheNswBuiltInLibraryUnderTheNameItHasNow)
{
    // The library of the NSW built-in, found by name among what was compiled
    // in: the default is Katana Standard, which holds none of the 29.
    const katana::cad::BuiltInEntry* nsw = katana::cad::findBuiltIn(katana::cad::builtIns(), "NSW");
    if (nsw == nullptr || !nsw->builtIn.customisation) {
        GTEST_SKIP() << "this build has no NSW customisation compiled in";
    }
    // 29 definitions carried the publisher's word, which the reference
    // customisation was given without (the census's `carried`).
    ASSERT_EQ(katana::cad::builtinDefinitionRenames().size(), 29u);
    const katana::entity::StyleLibrary& library = nsw->builtIn.customisation->library;
    for (const katana::cad::RenamedDefinition& rename : katana::cad::builtinDefinitionRenames()) {
        EXPECT_NE(library.find(std::string(rename.now)), nullptr) << rename.now;
    }
    std::size_t known = 0;
    library.forEach([&](const katana::entity::LineStyle& style) {
        known += katana::cad::definitionNameNow(style.name).empty() ? 0 : 1;
    });
    EXPECT_EQ(known, 0u);
}
