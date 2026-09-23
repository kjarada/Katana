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
#include "katana/cad/style_catalogue.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"
#include "widget_harness.hpp"

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

// A library of paper linestyles with these names and nothing else: the
// case tests need names that differ only in case, which testLibrary has not.
katana::entity::StyleLibrary linestylesNamed(const std::vector<std::string>& names)
{
    using katana::entity::Stroke;
    using katana::entity::StrokeOp;
    katana::entity::StyleLibrary library;
    for (const std::string& name : names) {
        katana::entity::LineStyle style;
        style.name = name;
        style.units = katana::entity::StyleUnits::Paper;
        style.length = 4.0;
        style.strokes = {Stroke{.op = StrokeOp::Move, .point = {0.0, 0.0}},
                         Stroke{.op = StrokeOp::Draw, .point = {1.5, 0.0}}};
        EXPECT_TRUE(library.add(style).ok());
    }
    return library;
}

// Enter as a person's keyboard delivers it: to the completion popup while it
// is open (it holds the keyboard, and its completer hands the key on to the
// field before acting on it), to the field otherwise.
void pressEnter(NamePicker& picker)
{
    QAbstractItemView* popup = picker.nameCompleter()->popup();
    pressKey(popup != nullptr && popup->isVisible() ? static_cast<QWidget*>(popup)
                                                    : picker.lineEdit(),
             Qt::Key_Return);
}

// Escape closes the completion popup, as a person does before pressing Enter
// on what they typed.
void closeCompletions(NamePicker& picker)
{
    QAbstractItemView* popup = picker.nameCompleter()->popup();
    ASSERT_TRUE(popup != nullptr && popup->isVisible());
    pressKey(popup, Qt::Key_Escape);
    ASSERT_FALSE(popup->isVisible());
}

// A window holding a picker and another field to click into, active, with
// the picker focused - leaving a field needs focus to leave.
struct FocusWindow {
    QWidget window;
    NamePicker* picker;
    QLineEdit* other;

    FocusWindow(const CustomisationContext& context, NameRole role, bool offerByLayer)
        : picker(new NamePicker(context, role, offerByLayer, &window)),
          other(new QLineEdit(&window))
    {
        window.resize(320, 80);
        other->move(0, 40);
    }

    // False when the platform would not give focus; the test asserts it, as
    // a focus test that never had focus would pass for nothing.
    [[nodiscard]] bool focusPicker()
    {
        if (!katana::qt::test::showActive(window)) {
            return false;
        }
        picker->setFocus(Qt::MouseFocusReason);
        katana::qt::test::processEvents();
        return picker->hasFocus();
    }

    // A click into the other field: the click outside closes an open
    // completion popup, then focus moves.
    void leavePicker()
    {
        if (QAbstractItemView* popup = picker->nameCompleter()->popup();
            popup != nullptr && popup->isVisible()) {
            popup->hide();
        }
        other->setFocus(Qt::MouseFocusReason);
        katana::qt::test::processEvents();
    }
};

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

// Review of 2026-09-24: QComboBox finishes an edit with its own lookup,
// findText(text, matchFlags()), before the picker hears of it, and its flags
// drop Qt::MatchCaseSensitive whenever the line edit's completer is
// case-insensitive. The picker's search folds case, so the combo jumped to
// the first listed name in ANY case and reported it as chosen - the
// rewriting of a name D3 forbids.

TEST(NamePicker, EnterOnAnExactlyTypedNameChoosesThatNameNotAnotherCaseOfIt)
{
    PickerFixture fixture;
    fixture.document.setStyleLibrary(linestylesNamed({"Kerb", "KERB"}));
    NamePicker picker(fixture.context, NameRole::Linetype, true);
    // Sorted with case folded, ties by bytes: "KERB" (0x45) before "Kerb"
    // (0x65), so a case-folded search meets "KERB" first.
    ASSERT_LT(rowOf(picker, QStringLiteral("KERB")), rowOf(picker, QStringLiteral("Kerb")));
    picker.setCurrentName("ByLayer");
    std::vector<std::string> chosen;
    picker.onNameChosen = [&chosen](const std::string& name) { chosen.push_back(name); };
    picker.resize(300, 30);
    picker.show();

    picker.lineEdit()->clear();
    typeText(picker.lineEdit(), QStringLiteral("Kerb"));
    // Both names contain "kerb" folded, so the popup offers both, and
    // nothing in it is highlighted: Enter finishes what was typed.
    ASSERT_EQ(picker.nameCompleter()->completionCount(), 2);
    pressEnter(picker);
    EXPECT_EQ(chosen, std::vector<std::string>{"Kerb"});
    EXPECT_EQ(picker.currentName(), "Kerb");
    EXPECT_TRUE(picker.currentIsDefined());
}

TEST(NamePicker, EnterOnANameListedOnlyInAnotherCaseKeepsTheTypedCase)
{
    PickerFixture fixture;
    fixture.document.setStyleLibrary(linestylesNamed({"Kerb"}));
    NamePicker picker(fixture.context, NameRole::Linetype, true);
    picker.setCurrentName("ByLayer");
    std::vector<std::string> chosen;
    picker.onNameChosen = [&chosen](const std::string& name) { chosen.push_back(name); };
    picker.resize(300, 30);
    picker.show();

    // "KERB": a name from a library that is not loaded. The popup offers
    // "Kerb"; the person closes it and presses Enter on what they typed.
    picker.lineEdit()->clear();
    typeText(picker.lineEdit(), QStringLiteral("KERB"));
    closeCompletions(picker);
    pressEnter(picker);
    EXPECT_EQ(chosen, std::vector<std::string>{"KERB"});
    EXPECT_EQ(picker.currentName(), "KERB");
    EXPECT_EQ(picker.lineEdit()->text(), QStringLiteral("KERB"));
    // Names are case-sensitive: "KERB" is not the listed "Kerb".
    EXPECT_FALSE(picker.currentIsDefined());
}

TEST(NamePicker, LeavingTheFieldKeepsTheTypedCaseOfANameListedInAnother)
{
    PickerFixture fixture;
    fixture.document.setStyleLibrary(linestylesNamed({"Kerb"}));
    FocusWindow window(fixture.context, NameRole::Linetype, true);
    NamePicker& picker = *window.picker;
    picker.setCurrentName("ByLayer");
    std::vector<std::string> chosen;
    picker.onNameChosen = [&chosen](const std::string& name) { chosen.push_back(name); };
    ASSERT_TRUE(window.focusPicker());

    picker.lineEdit()->clear();
    typeText(picker.lineEdit(), QStringLiteral("kerb"));
    window.leavePicker();
    ASSERT_FALSE(picker.hasFocus());
    EXPECT_EQ(chosen, std::vector<std::string>{"kerb"});
    EXPECT_EQ(picker.lineEdit()->text(), QStringLiteral("kerb"));
    EXPECT_EQ(picker.currentName(), "kerb");
}

// The same review: the picker ignored the field's editingFinished whenever
// the completion popup was open, trusting the completer to report the
// choice - but the completer activates only a HIGHLIGHTED completion, and
// the field does not finish again on leaving when nothing has changed since.
// A new name that is part of a listed one was dropped without a word.
TEST(NamePicker, EnterOnATypedNameWithNothingHighlightedInThePopupChoosesIt)
{
    PickerFixture fixture;
    FocusWindow window(fixture.context, NameRole::Linetype, true);
    NamePicker& picker = *window.picker;
    picker.setCurrentName("ByLayer");
    std::vector<std::string> chosen;
    picker.onNameChosen = [&chosen](const std::string& name) { chosen.push_back(name); };
    ASSERT_TRUE(window.focusPicker());

    // "Kerb", a new 12d name, is part of "TEST Dashed Kerb" and of nothing
    // else listed (ByLayer, DASHED), so the popup opens on that one name.
    picker.lineEdit()->clear();
    typeText(picker.lineEdit(), QStringLiteral("Kerb"));
    QAbstractItemView* popup = picker.nameCompleter()->popup();
    ASSERT_TRUE(popup->isVisible());
    ASSERT_EQ(picker.nameCompleter()->completionCount(), 1);
    ASSERT_FALSE(popup->selectionModel()->isSelected(popup->currentIndex()));

    pressEnter(picker);
    EXPECT_EQ(chosen, std::vector<std::string>{"Kerb"});
    EXPECT_EQ(picker.currentName(), "Kerb");
    // Leaving the field afterwards is not a second choice.
    window.leavePicker();
    EXPECT_EQ(chosen, std::vector<std::string>{"Kerb"});
}

// The same review: the picker marked every linetype name it did not list
// as "(not defined)", including the names cad::missingNames deliberately
// never reports - 12d's and DXF's plain line ("1", "0", "continuous" in any
// case; D4) and ByLayer in another spelling (entity::isByLayer folds case).
// A 12da import names "1" for every plain string, so the style manager
// would have marked every one of them in amber while its Missing chip,
// counted by missingNames, said none was missing.
TEST(NamePicker, PlainLineNamesAreNeitherMarkedNorMissingAndAMissingNameIsBoth)
{
    PickerFixture fixture;
    // Styles naming each plain spelling, one naming a genuinely missing 12d
    // linestyle, and one naming the fixture's listed library linestyle.
    const std::vector<std::pair<std::string, std::string>> styles = {
        {"plain 12d", "1"},        {"plain zero", "0"},    {"dxf", "Continuous"},
        {"shouted", "CONTINUOUS"}, {"inherit", "BYLAYER"}, {"old", "Old 12d Kerb"},
        {"kerb", "TEST Dashed Kerb"}};
    for (const auto& [name, linetype] : styles) {
        katana::entity::Style style;
        style.name = name;
        style.linetype = linetype;
        ASSERT_TRUE(fixture.document.execute(katana::commands::createStyle(style)).ok());
    }
    // The cad side, worked out from missingNames' rule (style_catalogue.hpp):
    // of these only "Old 12d Kerb" is in no table and no library and is not a
    // plain-line name.
    const std::vector<katana::cad::MissingName> missing =
        katana::cad::missingNames(fixture.document);
    ASSERT_EQ(missing.size(), 1U);
    EXPECT_EQ(missing.front().name, "Old 12d Kerb");

    NamePicker picker(fixture.context, NameRole::Linetype, true);
    for (const auto& [styleName, linetype] : styles) {
        SCOPED_TRACE(linetype);
        picker.setCurrentName(linetype);
        const bool reported = linetype == "Old 12d Kerb";
        EXPECT_EQ(picker.currentIsDefined(), !reported);
        // Kept byte for byte either way: "BYLAYER" is not rewritten to the
        // listed "ByLayer", nor "Continuous" to anything.
        EXPECT_EQ(picker.currentName(), linetype);
        const int row = picker.currentIndex();
        ASSERT_GE(row, 0);
        const bool marked = picker.itemData(row, NamePicker::kSourceRole).toInt() ==
                            static_cast<int>(katana::cad::DefinitionSource::Undefined);
        EXPECT_EQ(marked, reported);
        EXPECT_EQ(picker.currentText().endsWith(QStringLiteral(" (not defined)")), reported);
        EXPECT_EQ(picker.lineEdit()->palette().color(QPalette::Text) ==
                      katana::qt::kUndefinedNameColour,
                  reported);
    }

    // And each tooltip says what is drawn: a plain line, or - for ByLayer in
    // any spelling on a Style - the layer's linetype, not a solid line.
    picker.setCurrentName("1");
    EXPECT_TRUE(picker.toolTip().contains(QStringLiteral("plain line")));
    picker.setCurrentName("BYLAYER");
    EXPECT_TRUE(picker.toolTip().contains(QStringLiteral("layer's linetype")));
    EXPECT_FALSE(picker.toolTip().contains(QStringLiteral("solid line")));
}
