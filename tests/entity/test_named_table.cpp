// The shared named-table behaviour (include/katana/entity/named_table.hpp).
//
// Linetypes, dimension styles and styles were three hand-written copies of one
// container until they were collapsed onto `NamedTable`. The rules they share -
// duplicate rejection, absence reporting, name ordering - had never been tested
// on the linetype table at all, and the two rules that protect the built-in
// "continuous" entry had no test of any kind: they would have gone on passing
// if the code had been deleted.

#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "katana/entity/tables.hpp"

using namespace katana::entity;
using katana::core::ErrorCode;

namespace {

Linetype dashed(std::string name)
{
    Linetype linetype;
    linetype.name = std::move(name);
    linetype.pattern = {LinetypeElement{0.5}, LinetypeElement{-0.25}};
    return linetype;
}

} // namespace

// ---- the rules that protect a built-in entry -------------------------------------

TEST(NamedTable, TheContinuousLinetypeCannotBeRemoved)
{
    // Every entity resolves to a linetype, and an unset ByLayer chain ends at
    // this one. Removing it would leave that chain pointing at nothing.
    LinetypeDatabase linetypes;
    const auto removed = linetypes.remove(kContinuousLinetype);
    ASSERT_FALSE(removed.ok());
    EXPECT_EQ(removed.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(linetypes.contains(kContinuousLinetype));
}

TEST(NamedTable, TheContinuousLinetypeCannotBeGivenAPattern)
{
    // Giving it a pattern would silently change what every entity that has not
    // chosen a linetype draws as, across the whole document.
    LinetypeDatabase linetypes;
    const auto status = linetypes.update(dashed(std::string(kContinuousLinetype)));
    ASSERT_FALSE(status);
    EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
    ASSERT_NE(linetypes.find(kContinuousLinetype), nullptr);
    EXPECT_TRUE(linetypes.find(kContinuousLinetype)->isContinuous());
}

TEST(NamedTable, TheContinuousLinetypeMayStillBeUpdatedInEveryOtherRespect)
{
    // The rule is about the pattern, not about the entry being frozen - so the
    // test proves the guard is narrow rather than a blanket refusal that would
    // pass the two tests above for the wrong reason.
    LinetypeDatabase linetypes;
    Linetype continuous = *linetypes.find(kContinuousLinetype);
    continuous.description = "Unbroken";
    EXPECT_TRUE(linetypes.update(continuous));
    EXPECT_EQ(linetypes.find(kContinuousLinetype)->description, "Unbroken");
}

TEST(NamedTable, TheDefaultDimensionStyleCannotBeRemoved)
{
    DimensionStyleDatabase styles;
    const auto removed = styles.remove(kDefaultDimensionStyleName);
    ASSERT_FALSE(removed.ok());
    EXPECT_EQ(removed.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(styles.contains(kDefaultDimensionStyleName));
}

TEST(NamedTable, ATableWithNoBuiltInStartsEmptyAndRemovesFreely)
{
    // StylePolicy takes the default `isProtected` and `seed`. Asserting that
    // the defaults actually apply is what stops a future policy from
    // accidentally inheriting a protection it never asked for.
    StyleDatabase styles;
    EXPECT_EQ(styles.size(), 0u);
    Style style;
    style.name = "kerb";
    ASSERT_TRUE(styles.add(style));
    EXPECT_TRUE(styles.remove("kerb").ok());
    EXPECT_EQ(styles.size(), 0u);
}

// ---- the rules every table shares ------------------------------------------------

TEST(NamedTable, AddingANameThatExistsIsRefusedAndChangesNothing)
{
    LinetypeDatabase linetypes;
    ASSERT_TRUE(linetypes.add(dashed("fence")));
    Linetype different = dashed("fence");
    different.description = "a different fence";

    const auto status = linetypes.add(different);
    ASSERT_FALSE(status);
    EXPECT_EQ(status.error().code, ErrorCode::AlreadyExists);
    // The rejected add must not have overwritten the original.
    EXPECT_EQ(linetypes.find("fence")->description, "");
}

TEST(NamedTable, UpdatingSomethingAbsentIsNotFoundRatherThanAnInsert)
{
    // The distinction matters: an update that silently inserted would let a
    // rename lose the original and create a second entry instead.
    LinetypeDatabase linetypes;
    const auto status = linetypes.update(dashed("never added"));
    ASSERT_FALSE(status);
    EXPECT_EQ(status.error().code, ErrorCode::NotFound);
    EXPECT_FALSE(linetypes.contains("never added"));
}

TEST(NamedTable, RemovingSomethingAbsentIsNotFound)
{
    LinetypeDatabase linetypes;
    const auto removed = linetypes.remove("never added");
    ASSERT_FALSE(removed.ok());
    EXPECT_EQ(removed.error().code, ErrorCode::NotFound);
}

TEST(NamedTable, TheErrorNamesTheKindOfTableSoTheMessageReadsAsEnglish)
{
    // Each policy carries its own noun. Without it every table would report
    // "already exists" with no indication of what does.
    LinetypeDatabase linetypes;
    ASSERT_TRUE(linetypes.add(dashed("fence")));
    const std::string linetypeMessage = linetypes.add(dashed("fence")).error().describe();
    EXPECT_NE(linetypeMessage.find("linetype"), std::string::npos) << linetypeMessage;

    DimensionStyleDatabase styles;
    DimensionStyle style;
    style.name = std::string(kDefaultDimensionStyleName);
    const std::string styleMessage = styles.add(style).error().describe();
    EXPECT_NE(styleMessage.find("dimension style"), std::string::npos) << styleMessage;
}

TEST(NamedTable, NamesAndRecordsComeBackInNameOrderWhateverTheInsertionOrder)
{
    // The project store writes tables in this order, and a stable order is what
    // makes a saved file diffable. Inserted deliberately out of order.
    LinetypeDatabase linetypes;
    for (const char* name : {"zigzag", "ditch", "kerb", "aerial"}) {
        ASSERT_TRUE(linetypes.add(dashed(name)));
    }
    const std::vector<std::string> expected{"aerial", "continuous", "ditch", "kerb", "zigzag"};
    EXPECT_EQ(linetypes.names(), expected);

    std::vector<std::string> fromRecords;
    for (const Linetype& linetype : linetypes.all()) {
        fromRecords.push_back(linetype.name);
    }
    EXPECT_EQ(fromRecords, expected);
}

TEST(NamedTable, ResetRestoresTheBuiltInAndNothingElse)
{
    LinetypeDatabase linetypes;
    ASSERT_TRUE(linetypes.add(dashed("fence")));
    ASSERT_TRUE(linetypes.remove(kContinuousLinetype).ok() == false); // still protected
    linetypes.reset();
    EXPECT_EQ(linetypes.size(), 1u);
    EXPECT_TRUE(linetypes.contains(kContinuousLinetype));
    EXPECT_FALSE(linetypes.contains("fence"));
}
