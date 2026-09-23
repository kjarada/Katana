// NamePicker: the linetype and symbol combo every manager uses, and the rule
// it exists for - a picker never drops or rewrites the name it was given
// (decision D3, audit QT-02).

#include <gtest/gtest.h>

#include <string>
#include <vector>

#include <QAbstractItemView>
#include <QCompleter>
#include <QCoreApplication>
#include <QKeyEvent>
#include <QLineEdit>

#include "customisation/customisation_context.hpp"
#include "customisation/definition_thumbnails.hpp"
#include "customisation/name_picker.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"

using katana::cad::Document;
using katana::cad::NameRole;
using katana::qt::CustomisationContext;
using katana::qt::DefinitionThumbnails;
using katana::qt::NamePicker;

namespace {

// A library of two definitions written for this test: a paper linestyle
// (not `mode vertex`, so a linestyle - D2) and a vertex symbol (a symbol -
// D3). Their strokes are irrelevant here; they only have to be valid.
katana::entity::StyleLibrary testLibrary()
{
    using katana::entity::Stroke;
    using katana::entity::StrokeOp;
    katana::entity::StyleLibrary library;
    katana::entity::LineStyle kerb;
    kerb.name = "TEST Dashed Kerb";
    kerb.units = katana::entity::StyleUnits::Paper;
    kerb.length = 4.0;
    kerb.strokes = {Stroke{.op = StrokeOp::Move, .point = {0.0, 0.0}},
                    Stroke{.op = StrokeOp::Draw, .point = {1.5, 0.0}}};
    EXPECT_TRUE(library.add(kerb).ok());
    katana::entity::LineStyle manhole;
    manhole.name = "TEST Manhole";
    manhole.atVertices = true;
    manhole.strokes = {Stroke{.op = StrokeOp::Circle, .radius = 0.5}};
    EXPECT_TRUE(library.add(manhole).ok());
    return library;
}

struct PickerFixture {
    Document document;
    DefinitionThumbnails thumbnails;
    CustomisationContext context;

    PickerFixture()
    {
        document.setStyleLibrary(testLibrary());
        katana::entity::Linetype dashed;
        dashed.name = "DASHED";
        dashed.pattern = {{1.0}, {-0.5}};
        EXPECT_TRUE(document.execute(katana::commands::createLinetype(dashed)).ok());
        context.document = &document;
        context.thumbnails = &thumbnails;
    }
};

void typeText(QWidget* target, const QString& text)
{
    for (const QChar c : text) {
        QKeyEvent press(QEvent::KeyPress, 0, Qt::NoModifier, QString(c));
        QCoreApplication::sendEvent(target, &press);
        QKeyEvent release(QEvent::KeyRelease, 0, Qt::NoModifier, QString(c));
        QCoreApplication::sendEvent(target, &release);
    }
}

void pressKey(QWidget* target, Qt::Key key)
{
    QKeyEvent press(QEvent::KeyPress, key, Qt::NoModifier);
    QCoreApplication::sendEvent(target, &press);
    QKeyEvent release(QEvent::KeyRelease, key, Qt::NoModifier);
    QCoreApplication::sendEvent(target, &release);
}

// The row showing `text`, or -1.
int rowOf(const NamePicker& picker, const QString& text) { return picker.findText(text); }

} // namespace

TEST(NamePicker, ANameDefinedNowhereSurvivesSetAndCurrentNameByteForByte)
{
    PickerFixture fixture;
    NamePicker picker(fixture.context, NameRole::Symbol, false);
    // A 12d name as a library in another office might spell it: two spaces,
    // a trailing space, UTF-8 "e acute", and a byte (0xFF) that is not UTF-8
    // at all - which a round trip through QString would turn into U+FFFD.
    const std::string name = std::string("Pit  Grated \xC3\xA9 (SW)\xFF ");
    picker.setCurrentName(name);
    EXPECT_EQ(picker.currentName(), name);
    EXPECT_FALSE(picker.currentIsDefined());
    // Kept first in the list, not dropped.
    ASSERT_FALSE(picker.names().empty());
    EXPECT_EQ(picker.names().front(), name);
    // Its tooltip says what it draws as meanwhile: entity::builtInSymbolFor
    // finds "pit" in the name, so a manhole.
    EXPECT_TRUE(picker.itemData(0, Qt::ToolTipRole).toString().contains(QStringLiteral("manhole")));
    EXPECT_EQ(picker.itemData(0, Qt::ForegroundRole).value<QBrush>().color(),
              katana::qt::kUndefinedNameColour);
}

TEST(NamePicker, TheDecoratedLabelIsWhatIsShownButNeverWhatCurrentNameReturns)
{
    PickerFixture fixture;
    NamePicker picker(fixture.context, NameRole::Linetype, true);
    picker.setCurrentName("Old Kerb");
    EXPECT_EQ(picker.currentText(), QStringLiteral("Old Kerb (not defined)"));
    EXPECT_EQ(picker.currentName(), "Old Kerb");

    // A name that IS listed carries no decoration and no mark.
    picker.setCurrentName("TEST Dashed Kerb");
    EXPECT_EQ(picker.currentText(), QStringLiteral("TEST Dashed Kerb"));
    EXPECT_TRUE(picker.currentIsDefined());
    // And the previous kept name is gone: it belonged to the other value.
    EXPECT_EQ(rowOf(picker, QStringLiteral("Old Kerb (not defined)")), -1);

    // Names are case-sensitive: a different case is a different, undefined
    // name, kept as given rather than "corrected" to the listed one.
    picker.setCurrentName("test dashed kerb");
    EXPECT_EQ(picker.currentName(), "test dashed kerb");
    EXPECT_FALSE(picker.currentIsDefined());
}

TEST(NamePicker, TheListIsInSectionsAndOnlyAStylesLinetypeOffersByLayer)
{
    PickerFixture fixture;
    NamePicker style(fixture.context, NameRole::Linetype, true);
    ASSERT_GT(style.count(), 0);
    EXPECT_EQ(style.itemText(0), QStringLiteral("ByLayer"));
    const int drawing = rowOf(style, QStringLiteral("Drawing linetypes"));
    const int library = rowOf(style, QStringLiteral("Library linestyles"));
    ASSERT_GE(drawing, 0);
    ASSERT_GE(library, 0);
    EXPECT_LT(drawing, rowOf(style, QStringLiteral("DASHED")));
    EXPECT_LT(rowOf(style, QStringLiteral("DASHED")), library);
    EXPECT_LT(library, rowOf(style, QStringLiteral("TEST Dashed Kerb")));
    // A header cannot be chosen.
    EXPECT_FALSE(style.itemData(drawing, NamePicker::kNameRole).isValid());
    // A vertex symbol is not a linestyle (D2).
    EXPECT_EQ(rowOf(style, QStringLiteral("TEST Manhole")), -1);

    NamePicker layer(fixture.context, NameRole::Linetype, false);
    EXPECT_EQ(rowOf(layer, QStringLiteral("ByLayer")), -1);

    NamePicker symbol(fixture.context, NameRole::Symbol, false);
    const int builtIn = rowOf(symbol, QStringLiteral("Built-in symbols"));
    const int librarySymbols = rowOf(symbol, QStringLiteral("Library symbols"));
    ASSERT_GE(builtIn, 0);
    ASSERT_GT(librarySymbols, builtIn);
    EXPECT_GT(rowOf(symbol, QStringLiteral("TEST Manhole")), librarySymbols);
    EXPECT_GT(rowOf(symbol, QStringLiteral("circle")), builtIn);
    EXPECT_LT(rowOf(symbol, QStringLiteral("circle")), librarySymbols);
    // Every entry has a picture, the shared cache's.
    EXPECT_FALSE(symbol.itemIcon(rowOf(symbol, QStringLiteral("TEST Manhole"))).isNull());
    EXPECT_FALSE(style.itemIcon(rowOf(style, QStringLiteral("TEST Dashed Kerb"))).isNull());
    EXPECT_FALSE(style.itemIcon(rowOf(style, QStringLiteral("DASHED"))).isNull());
}

TEST(NamePicker, TypingAFragmentInAnyCaseCompletesAndChoosingItReportsTheExactName)
{
    PickerFixture fixture;
    NamePicker picker(fixture.context, NameRole::Linetype, true);
    std::vector<std::string> chosen;
    picker.onNameChosen = [&chosen](const std::string& name) { chosen.push_back(name); };
    picker.resize(300, 30);
    picker.show();

    // "dASHED k": the middle of "TEST Dashed Kerb", in neither of its cases.
    // "DASHED" contains "dashed" but not "dashed k", so one completion.
    picker.lineEdit()->clear();
    typeText(picker.lineEdit(), QStringLiteral("dASHED k"));
    QCompleter* completer = picker.nameCompleter();
    ASSERT_EQ(completer->completionCount(), 1);
    EXPECT_EQ(completer->completionModel()->index(0, 0).data().toString(),
              QStringLiteral("TEST Dashed Kerb"));

    // Chosen from the popup as a person would: the arrow onto it, then Enter.
    QAbstractItemView* popup = completer->popup();
    ASSERT_TRUE(popup->isVisible());
    popup->setCurrentIndex(completer->completionModel()->index(0, 0));
    pressKey(popup, Qt::Key_Return);
    EXPECT_EQ(picker.currentName(), "TEST Dashed Kerb");
    EXPECT_EQ(chosen, std::vector<std::string>{"TEST Dashed Kerb"});

    // "dash" alone is in both names.
    picker.lineEdit()->clear();
    typeText(picker.lineEdit(), QStringLiteral("dash"));
    EXPECT_EQ(completer->completionCount(), 2);
}

TEST(NamePicker, SettingAndRefreshingNeverFireOnNameChosenAndTypingDoes)
{
    PickerFixture fixture;
    NamePicker picker(fixture.context, NameRole::Linetype, true);
    int fired = 0;
    std::string last;
    picker.onNameChosen = [&](const std::string& name) {
        ++fired;
        last = name;
    };
    picker.setCurrentName("DASHED");
    picker.setCurrentName("Unknown 12d");
    picker.refresh();
    fixture.document.setStyleLibrary({});
    picker.refresh();
    EXPECT_EQ(fired, 0);

    // A person typing a name and pressing Enter is a choice.
    picker.lineEdit()->selectAll();
    typeText(picker.lineEdit(), QStringLiteral("DASHED"));
    pressKey(picker.lineEdit(), Qt::Key_Return);
    EXPECT_EQ(fired, 1);
    EXPECT_EQ(last, "DASHED");
    // Enter again with nothing changed is not another choice.
    pressKey(picker.lineEdit(), Qt::Key_Return);
    EXPECT_EQ(fired, 1);
}

TEST(NamePicker, ALibraryReloadKeepsTheCurrentNameMarkedWhileItIsUndefined)
{
    PickerFixture fixture;
    NamePicker picker(fixture.context, NameRole::Linetype, true);
    picker.setCurrentName("TEST Dashed Kerb");
    ASSERT_TRUE(picker.currentIsDefined());

    // The library goes: the name stays, marked.
    fixture.document.setStyleLibrary({});
    picker.refresh();
    EXPECT_EQ(picker.currentName(), "TEST Dashed Kerb");
    EXPECT_FALSE(picker.currentIsDefined());
    EXPECT_EQ(picker.currentText(), QStringLiteral("TEST Dashed Kerb (not defined)"));

    // And comes back: defined again, listed once.
    fixture.document.setStyleLibrary(testLibrary());
    picker.refresh();
    EXPECT_EQ(picker.currentName(), "TEST Dashed Kerb");
    EXPECT_TRUE(picker.currentIsDefined());
    const std::vector<std::string> names = picker.names();
    EXPECT_EQ(std::count(names.begin(), names.end(), "TEST Dashed Kerb"), 1);
}
