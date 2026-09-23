// FilterBar: the search field and exclusive chips above every manager's
// table, found by objectName as a test or the headless driver finds them.

#include <gtest/gtest.h>

#include <vector>

#include <QLineEdit>
#include <QToolButton>

#include "customisation/filter_bar.hpp"

using katana::qt::FilterBar;

TEST(FilterBar, TheSearchFieldAndEachChipAreFoundByTheirObjectNames)
{
    FilterBar bar({QStringLiteral("All"), QStringLiteral("Used"), QStringLiteral("Unused"),
                   QStringLiteral("Missing")});
    EXPECT_NE(bar.findChild<QLineEdit*>(QStringLiteral("filterText")), nullptr);
    for (const char* name : {"filterAll", "filterUsed", "filterUnused", "filterMissing"}) {
        EXPECT_NE(bar.findChild<QToolButton*>(QString::fromLatin1(name)), nullptr) << name;
    }
    // Spaces and punctuation are not part of a name.
    EXPECT_EQ(FilterBar::chipObjectName(QStringLiteral("Not used (3)")),
              QStringLiteral("filterNotused3"));
}

TEST(FilterBar, TheChipsAreExclusiveTheFirstStartsCheckedAndAClickReportsItsIndex)
{
    FilterBar bar({QStringLiteral("All"), QStringLiteral("Used"), QStringLiteral("Unused")});
    std::vector<int> reported;
    bar.onChipChanged = [&reported](int index) { reported.push_back(index); };
    EXPECT_EQ(bar.chip(), 0);

    auto* unused = bar.findChild<QToolButton*>(QStringLiteral("filterUnused"));
    ASSERT_NE(unused, nullptr);
    unused->click();
    EXPECT_EQ(bar.chip(), 2);
    EXPECT_EQ(bar.chipLabel(), QStringLiteral("Unused"));
    EXPECT_FALSE(bar.findChild<QToolButton*>(QStringLiteral("filterAll"))->isChecked());
    // Once, for the chip turned on - not again for the one turned off.
    EXPECT_EQ(reported, std::vector<int>{2});

    // Clicking the checked chip leaves it checked: there is always a filter.
    unused->click();
    EXPECT_EQ(bar.chip(), 2);
    EXPECT_EQ(reported, std::vector<int>{2});

    bar.setChip(1);
    EXPECT_EQ(reported, (std::vector<int>{2, 1}));
    bar.setChip(9); // out of range: ignored
    EXPECT_EQ(bar.chip(), 1);
}

TEST(FilterBar, TypingReportsTheTextAsTypedWithItsCaseKept)
{
    FilterBar bar({QStringLiteral("All")});
    QString reported;
    bar.onTextChanged = [&reported](const QString& text) { reported = text; };
    auto* field = bar.findChild<QLineEdit*>(QStringLiteral("filterText"));
    ASSERT_NE(field, nullptr);
    field->insert(QStringLiteral("KeRb"));
    EXPECT_EQ(reported, QStringLiteral("KeRb"));
    EXPECT_EQ(bar.text(), QStringLiteral("KeRb"));
}

TEST(FilterBar, ARelabelledChipKeepsTheObjectNameOfItsFirstLabel)
{
    FilterBar bar({QStringLiteral("All"), QStringLiteral("Missing")});
    bar.setChipLabel(1, QStringLiteral("Missing (3)"));
    auto* missing = bar.findChild<QToolButton*>(QStringLiteral("filterMissing"));
    ASSERT_NE(missing, nullptr);
    EXPECT_EQ(missing->text(), QStringLiteral("Missing (3)"));
}
