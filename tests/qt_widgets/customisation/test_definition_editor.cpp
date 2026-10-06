// DefinitionEditorDialog: one linestyle or symbol definition of the session's
// customisation made, changed, copied and deleted, driven by object name as a
// person - or the headless driver - drives it.
//
// Nothing is read from a file. The customisation the tests edit is written
// out here, and every definition a test expects in the library afterwards is
// built by hand beside the assertion:
//
//   library   "TEST Valve"  a symbol, at vertices, paper, group Test/Marks,
//                           from "Site": a 2 x 2 cross through the origin and
//                           a circle of radius 0.5 round it
//             "TEST Kerb"   a linestyle, paper, length 12, group Test/Lines,
//                           from "Site": one dash of 8
//             "TEST Spare"  a symbol, at vertices, made in a session: a dot
//   rules     #0  WM*  feature  linestyle "TEST Kerb"
//             #1  AC*  symbol   symbol    "TEST Valve"
//   styles    Marks    symbol "TEST Valve"
//
// So "TEST Valve" is used by rule #1 and the style Marks, "TEST Kerb" by rule
// #0, and "TEST Spare" by nothing.
//
// The text a definition's strokes are shown as is the Katana customisation
// format's, by the rules of docs/customisation.md ("Layout"): one stroke a
// line, `["move", x, y]` with a blank after each comma, a number as its
// shortest text, and a comma ending every line but the last.
//
// The verb CUSTOMISE REMOVE is another change's and is not in this build. The
// editor only builds its line and hands it to the runner it was given, so
// these tests give it a runner that does what the verb is designed to do -
// refuse a definition something uses until FORCE - and assert on the lines
// and on the runner's own words, which the editor shows as they came. A
// second runner fails every line for a reason that has nothing to do with
// use, which is what the real window answers until the verb is there.

#include <gtest/gtest.h>

#include <cstdint>
#include <functional>
#include <memory>
#include <string>
#include <utility>
#include <vector>

#include <QAction>
#include <QCheckBox>
#include <QComboBox>
#include <QImage>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QMainWindow>
#include <QMenu>
#include <QPlainTextEdit>
#include <QPointer>
#include <QPushButton>
#include <QRegularExpression>
#include <QSpinBox>
#include <QTextBlock>
#include <QTextCursor>
#include <QTextEdit>
#include <QToolBar>

#include "command_runner.hpp"
#include "customisation/customisation_context.hpp"
#include "customisation/customisation_workbench.hpp"
#include "customisation/definition_editor.hpp"
#include "customisation/style_preview.hpp"
#include "customisation/symbol_library.hpp"
#include "katana/cad/definition_users.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/style_manager_rows.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/customisation.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/entity/survey_map.hpp"
#include "katana/entity/tables.hpp"
#include "style_manager.hpp"
#include "theme.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::entity::LineStyle;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::entity::StrokeText;
using katana::entity::StyleLibrary;
using katana::entity::StyleUnits;
using katana::qt::CustomisationContext;
using katana::qt::DefinitionEdit;
using katana::qt::DefinitionEditorDialog;
using katana::qt::VerbOutcome;

namespace {

Stroke move(double x, double y) { return Stroke{.op = StrokeOp::Move, .point = {x, y}}; }
Stroke draw(double x, double y) { return Stroke{.op = StrokeOp::Draw, .point = {x, y}}; }

LineStyle valve()
{
    LineStyle symbol;
    symbol.name = "TEST Valve";
    symbol.group = "Test/Marks";
    symbol.units = StyleUnits::Paper;
    symbol.atVertices = true;
    symbol.symbol = true;
    symbol.source = "Site";
    symbol.strokes = {move(-1.0, 0.0), draw(1.0, 0.0), move(0.0, -1.0), draw(0.0, 1.0),
                      move(0.0, 0.0), Stroke{.op = StrokeOp::Circle, .radius = 0.5}};
    return symbol;
}

LineStyle kerb()
{
    LineStyle linestyle;
    linestyle.name = "TEST Kerb";
    linestyle.group = "Test/Lines";
    linestyle.units = StyleUnits::Paper;
    linestyle.length = 12.0;
    linestyle.source = "Site";
    linestyle.strokes = {move(0.0, 0.0), draw(8.0, 0.0)};
    return linestyle;
}

LineStyle spare()
{
    LineStyle symbol;
    symbol.name = "TEST Spare";
    symbol.atVertices = true;
    symbol.symbol = true;
    symbol.strokes = {move(0.0, 0.0), Stroke{.op = StrokeOp::Dot}};
    return symbol;
}

// CUSTOMISE REMOVE as its design gives it, for the two lines the editor
// builds: a definition something names is refused until FORCE, and one
// removed leaves the library through Document::setStyleLibrary.
VerbOutcome removeAsTheVerbWould(Document& document, const QString& line)
{
    static const QRegularExpression shape(
        QStringLiteral("^CUSTOMISE REMOVE \"([^\"]+)\"( FORCE)?$"));
    const QRegularExpressionMatch match = shape.match(line);
    VerbOutcome outcome;
    if (!match.hasMatch()) {
        outcome.error = QStringLiteral("not a line this runner knows: ") + line;
        return outcome;
    }
    const std::string name = match.captured(1).toStdString();
    if (match.captured(2).isEmpty() && !katana::cad::definitionUsers(document, name).empty()) {
        outcome.error = QStringLiteral("the definition is used");
        return outcome;
    }
    StyleLibrary library = document.styleLibrary();
    if (!library.remove(name)) {
        outcome.error = QStringLiteral("no such definition");
        return outcome;
    }
    document.setStyleLibrary(std::move(library));
    outcome.ok = true;
    return outcome;
}

struct EditorFixture {
    Document document;
    CustomisationContext context;
    std::vector<std::pair<QString, bool>> logged;
    QStringList ran;

    EditorFixture()
    {
        StyleLibrary library;
        for (const LineStyle& each : {valve(), kerb(), spare()}) {
            const auto added = library.add(each);
            EXPECT_TRUE(added.ok()) << (added.ok() ? "" : added.error().describe());
        }
        document.setStyleLibrary(std::move(library));

        katana::entity::SurveyMap map;
        katana::entity::SurveyRule water;
        water.key = "WM*";
        water.linestyle = "TEST Kerb";
        EXPECT_TRUE(map.add(water).ok());
        katana::entity::SurveyRule mark;
        mark.key = "AC*";
        mark.section = katana::entity::SurveySection::VertexSymbol;
        mark.symbol = katana::entity::SurveySymbol{};
        mark.symbol->style = "TEST Valve";
        EXPECT_TRUE(map.add(mark).ok());
        document.setSurveyMap(std::move(map));

        katana::entity::Style marks;
        marks.name = "Marks";
        marks.symbol = "TEST Valve";
        EXPECT_TRUE(document.execute(katana::commands::createStyle(marks)).ok());

        context.document = &document;
        context.log = [this](const QString& message, bool isError) {
            logged.emplace_back(message, isError);
        };
        context.run = [this](const QString& line) {
            ran << line;
            return removeAsTheVerbWould(document, line);
        };
    }

    [[nodiscard]] bool loggedExactly(const QString& message) const
    {
        for (const auto& line : logged) {
            if (line.first == message) {
                return true;
            }
        }
        return false;
    }

    [[nodiscard]] LineStyle inLibrary(const char* name) const
    {
        const LineStyle* found = document.styleLibrary().find(name);
        EXPECT_NE(found, nullptr) << name;
        return found != nullptr ? *found : LineStyle{};
    }
};

template <typename T> T* child(const QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << name;
    return found;
}

void fill(const QWidget& dialog, const char* field, const QString& text)
{
    if (QLineEdit* line = child<QLineEdit>(dialog, field)) {
        line->setText(text);
    }
}

void strokes(const QWidget& dialog, const QString& text)
{
    child<QPlainTextEdit>(dialog, "definitionStrokes")->setPlainText(text);
}

QString issues(const QWidget& dialog)
{
    return child<QPlainTextEdit>(dialog, "definitionIssues")->toPlainText();
}

bool enabled(const QWidget& dialog, const char* name)
{
    return child<QWidget>(dialog, name)->isEnabled();
}

void press(const QWidget& dialog, const char* button)
{
    child<QPushButton>(dialog, button)->click();
}

void choose(const QWidget& dialog, const char* box, const QString& text)
{
    auto* combo = child<QComboBox>(dialog, box);
    const int row = combo->findText(text);
    ASSERT_GE(row, 0) << box << " has no choice " << text.toStdString();
    combo->setCurrentIndex(row);
}

// Whether two grabs of ONE pane differ anywhere: ink against a baseline, and
// never a count of pixels, which a font or a platform's antialiasing moves.
// The two must be of one size - a pane that was resized differs from itself,
// which would pass for ink and is said here instead.
bool differ(const QImage& a, const QImage& b)
{
    EXPECT_EQ(a.size(), b.size()) << "the pane was resized between the two grabs";
    return a.size() == b.size() && a != b;
}

// The lines of the strokes box that are washed as not reading, from 1.
std::vector<int> washedLines(const QWidget& dialog)
{
    std::vector<int> lines;
    for (const QTextEdit::ExtraSelection& mark :
         child<QPlainTextEdit>(dialog, "definitionStrokes")->extraSelections()) {
        lines.push_back(mark.cursor.blockNumber() + 1);
    }
    return lines;
}

// Whether the message area is in the colour of a refusal.
bool saysAnError(const QWidget& dialog)
{
    return child<QPlainTextEdit>(dialog, "definitionIssues")
        ->styleSheet()
        .contains(katana::qt::theme::error().name());
}

// A button the page is offering: shown and pressable ITSELF. A button in a
// hidden row is neither seen nor - by the headless driver, which asks the
// button - refused, so the row's being hidden proves nothing.
bool offered(const QWidget& dialog, const char* name)
{
    const auto* button = child<QPushButton>(dialog, name);
    return !button->isHidden() && button->isEnabled() && button->isVisibleTo(&dialog);
}

bool withdrawn(const QWidget& dialog, const char* name)
{
    const auto* button = child<QPushButton>(dialog, name);
    return button->isHidden() && !button->isEnabled();
}

TEST(DefinitionEditor, ItIsOneNonModalDialogWithANameForEveryControl)
{
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    EXPECT_EQ(dialog.objectName(), QStringLiteral("definitionEditorDialog"));
    EXPECT_FALSE(dialog.isModal());
    for (const char* name :
         {"definitionHeading", "definitionName", "definitionGroup", "definitionKind",
          "definitionUnits", "definitionUnitsNote", "definitionAtVertices", "definitionLength",
          "definitionFactor", "definitionOriginX", "definitionOriginY", "definitionAnchor1X",
          "definitionAnchor1Y", "definitionAnchor2X", "definitionAnchor2Y",
          "definitionStretchMode", "definitionCycleMode", "definitionSource",
          "definitionStrokes", "definitionStrokesHelp", "definitionPreview",
          "definitionLinePreview", "definitionPreviewNote", "definitionPlotScale",
          "definitionIssues", "definitionDeletePanel", "definitionDeleteAnyway",
          "definitionDeleteCancel", "definitionNew",
          "definitionDuplicate", "definitionDelete", "definitionRevert", "definitionSave",
          "definitionClose"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(QString::fromLatin1(name)), nullptr) << name;
    }
    // Every button, field and box a person can reach has one.
    for (const QWidget* each : dialog.findChildren<QPushButton*>()) {
        EXPECT_FALSE(each->objectName().isEmpty());
    }
}

TEST(DefinitionEditor, ASymbolTypedAsStrokesIsSavedIntoTheLibraryAsTheDefinitionWrittenOutByHand)
{
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.newDefinition(true));
    EXPECT_EQ(child<QLabel>(dialog, "definitionHeading")->text(), QStringLiteral("New symbol"));
    // Nothing to save yet: a definition is named.
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_TRUE(issues(dialog).startsWith(QStringLiteral("Type a name")));

    // A blank at either end of a new name is not part of it.
    fill(dialog, "definitionName", QStringLiteral(" TEST Post "));
    fill(dialog, "definitionGroup", QStringLiteral("Test/Marks"));
    choose(dialog, "definitionUnits", QStringLiteral("paper"));
    fill(dialog, "definitionFactor", QStringLiteral("2"));
    fill(dialog, "definitionOriginX", QStringLiteral("0.5"));
    fill(dialog, "definitionOriginY", QStringLiteral("-0.25"));
    // One of each kind of stroke, as a file holds them; the second line is
    // without the comma that would end it there, and a blank line is passed
    // over.
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n"
                                   "[\"draw\", 0, 2.5]\n"
                                   "\n"
                                   "[\"arc\", 1, 0, 180],\n"
                                   "[\"pen\", \"red\"],\n"
                                   "[\"circle\", 0.25],\n"
                                   "[\"dot\", 0],\n"
                                   "[\"text\", {\"text\": \"P\", \"height\": 1.5, "
                                   "\"justify\": \"middle-centre\"}]"));
    ASSERT_TRUE(enabled(dialog, "definitionSave")) << issues(dialog).toStdString();
    const std::uint64_t generation = f.document.libraryGeneration();
    press(dialog, "definitionSave");

    LineStyle expected;
    expected.name = "TEST Post";
    expected.group = "Test/Marks";
    expected.units = StyleUnits::Paper;
    expected.factor = 2.0;
    expected.origin = {0.5, -0.25};
    expected.symbol = true; // New from the symbol library's side: listed as a symbol
    expected.source = "";   // made in this session, of no customisation
    expected.strokes = {
        move(0.0, 0.0),
        draw(0.0, 2.5),
        Stroke{.op = StrokeOp::Arc, .radius = 1.0, .startAngle = 0.0, .endAngle = 180.0},
        Stroke{.op = StrokeOp::Pen, .pen = "red"},
        Stroke{.op = StrokeOp::Circle, .radius = 0.25},
        Stroke{.op = StrokeOp::Dot, .radius = 0.0},
        Stroke{.op = StrokeOp::Text, .text = 0},
    };
    StrokeText letter;
    letter.text = "P";
    letter.height = 1.5;
    letter.justify = "middle-centre";
    expected.texts = {letter};

    ASSERT_EQ(f.document.styleLibrary().size(), 4u);
    EXPECT_TRUE(f.inLibrary("TEST Post") == expected);
    // One commit, and nothing else in the library touched by it.
    EXPECT_EQ(f.document.libraryGeneration(), generation + 1);
    EXPECT_TRUE(f.inLibrary("TEST Valve") == valve());
    EXPECT_TRUE(f.inLibrary("TEST Kerb") == kerb());
    EXPECT_TRUE(f.loggedExactly(
        QStringLiteral("Symbol \"TEST Post\" added to the library: 7 strokes, 1 text.")));

    // It exists now: its name is fixed, and there is nothing left to save.
    EXPECT_EQ(dialog.editedName(), "TEST Post");
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionName")->text(), QStringLiteral("TEST Post"));
    EXPECT_FALSE(enabled(dialog, "definitionName"));
    EXPECT_FALSE(dialog.dirty());
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_EQ(child<QLabel>(dialog, "definitionHeading")->text(),
              QStringLiteral("Symbol \"TEST Post\""));
    EXPECT_EQ(child<QLabel>(dialog, "definitionSource")->text(),
              QStringLiteral("Made in this session"));
}

TEST(DefinitionEditor, AnExistingDefinitionIsShownAsTheFormatWritesItAndItsLengthIsChanged)
{
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.editDefinition("TEST Kerb"));

    EXPECT_EQ(child<QLineEdit>(dialog, "definitionName")->text(), QStringLiteral("TEST Kerb"));
    EXPECT_FALSE(enabled(dialog, "definitionName")) << "a name is fixed once it exists";
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionGroup")->text(), QStringLiteral("Test/Lines"));
    EXPECT_EQ(child<QComboBox>(dialog, "definitionKind")->currentText(),
              QStringLiteral("Linestyle"));
    EXPECT_EQ(child<QComboBox>(dialog, "definitionUnits")->currentText(), QStringLiteral("paper"));
    EXPECT_FALSE(child<QCheckBox>(dialog, "definitionAtVertices")->isChecked());
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionLength")->text(), QStringLiteral("12"));
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionFactor")->text(), QStringLiteral("1"));
    EXPECT_EQ(child<QLabel>(dialog, "definitionSource")->text(), QStringLiteral("Site"));
    EXPECT_EQ(child<QPlainTextEdit>(dialog, "definitionStrokes")->toPlainText(),
              QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 8, 0]"));
    // What names it is said while the form reads: rule #0 is the only one.
    EXPECT_EQ(issues(dialog),
              QStringLiteral("Named by:\n  rule #0 WM* (feature) names it as its linestyle"));

    // Opened and not touched: nothing to save or to go back to.
    EXPECT_FALSE(dialog.dirty());
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_FALSE(enabled(dialog, "definitionRevert"));

    fill(dialog, "definitionLength", QStringLiteral("15"));
    EXPECT_TRUE(dialog.dirty());
    EXPECT_EQ(child<QLabel>(dialog, "definitionHeading")->text(),
              QStringLiteral("Linestyle \"TEST Kerb\" - edited, not saved"));
    ASSERT_TRUE(enabled(dialog, "definitionSave"));
    EXPECT_TRUE(f.inLibrary("TEST Kerb") == kerb()) << "nothing reaches the library before Save";
    press(dialog, "definitionSave");

    LineStyle expected = kerb();
    expected.length = 15.0;
    EXPECT_TRUE(f.inLibrary("TEST Kerb") == expected);
    EXPECT_EQ(f.document.styleLibrary().size(), 3u) << "replaced, not added beside";
    EXPECT_TRUE(f.loggedExactly(
        QStringLiteral("Linestyle \"TEST Kerb\" saved: 2 strokes, 0 texts.")));
    EXPECT_FALSE(dialog.dirty());

    // Revert goes back to what the form was opened with.
    fill(dialog, "definitionLength", QStringLiteral("99"));
    press(dialog, "definitionRevert");
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionLength")->text(), QStringLiteral("15"));
    EXPECT_FALSE(dialog.dirty());
    EXPECT_TRUE(f.inLibrary("TEST Kerb") == expected);
}

TEST(DefinitionEditor, AnUntouchedFormDescribesExactlyTheDefinitionItWasOpenedOn)
{
    // Numbers no decimal field would hold - 0.1 + 0.2, a third - a negative
    // radius, anchors and modes on a definition that is not two-point, and a
    // text with every member: a Save of an untouched form, or of one where
    // only the group was changed, may alter none of them.
    LineStyle awkward;
    awkward.name = "TEST Awkward";
    awkward.group = "Test/Odd";
    awkward.units = StyleUnits::World;
    awkward.length = 0.1 + 0.2;
    awkward.factor = 1.0 / 3.0;
    awkward.origin = {1e-7, -2.5e20};
    awkward.anchor1 = {0.25, 0.5};
    awkward.anchor2 = {-4.0, 8.0};
    awkward.stretchMode = 2;
    awkward.cycleMode = 1;
    awkward.source = "Site";
    StrokeText label;
    label.text = "W \"1\"";
    label.angle = 30.0;
    label.height = 1.5;
    label.justify = "top-left";
    label.font = "Arial";
    label.widthFactor = 0.8;
    label.unnamed = {0.0, -0.3, 0.035};
    awkward.texts = {label};
    awkward.strokes = {
        move(0.1, 0.2),
        Stroke{.op = StrokeOp::Arc, .radius = -1.5, .startAngle = 10.0, .endAngle = 350.0},
        Stroke{.op = StrokeOp::Text, .text = 0},
    };
    EditorFixture f;
    StyleLibrary library = f.document.styleLibrary();
    ASSERT_TRUE(library.add(awkward).ok());
    f.document.setStyleLibrary(std::move(library));

    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.editDefinition("TEST Awkward"));
    const auto read = dialog.definition();
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(*read == awkward);
    // Not two-point: its anchors and modes are shown and cannot be typed into.
    EXPECT_FALSE(enabled(dialog, "definitionAnchor1X"));
    EXPECT_FALSE(enabled(dialog, "definitionCycleMode"));
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionAnchor2Y")->text(), QStringLiteral("8"));
    EXPECT_EQ(child<QSpinBox>(dialog, "definitionStretchMode")->value(), 2);

    fill(dialog, "definitionGroup", QStringLiteral("Test/Other"));
    press(dialog, "definitionSave");
    LineStyle expected = awkward;
    expected.group = "Test/Other";
    EXPECT_TRUE(f.inLibrary("TEST Awkward") == expected);
}

TEST(DefinitionEditor, AStrokeLineWithATypoKeepsSaveDisabledAndNamesTheLine)
{
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.editDefinition("TEST Valve"));

    // A word that is no kind of stroke, on the third line. The reader's own
    // refusal is shown, which names the stroke by its place counted from 0.
    strokes(dialog, QStringLiteral("[\"move\", -1, 0],\n"
                                   "[\"draw\", 1, 0],\n"
                                   "[\"drow\", 0, -1],\n"
                                   "[\"draw\", 0, 1]"));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_EQ(dialog.problemLine(), 3);
    EXPECT_TRUE(issues(dialog).startsWith(
        QStringLiteral("Line 3: definition \"TEST Valve\" strokes[2]: \"drow\" is not a kind "
                       "of stroke")))
        << issues(dialog).toStdString();
    // The line is washed in the box, and the message is in a refusal's colour.
    EXPECT_EQ(washedLines(dialog), (std::vector<int>{3}));
    EXPECT_TRUE(saysAnError(dialog));
    EXPECT_FALSE(dialog.save());
    EXPECT_FALSE(dialog.definition().ok());
    EXPECT_TRUE(f.inLibrary("TEST Valve") == valve());

    // Text that is not JSON at all - a comma missing between two numbers -
    // on the fourth line of the box, a blank line above it: the line is the
    // box's, not the stroke's place.
    strokes(dialog, QStringLiteral("[\"move\", -1, 0],\n"
                                   "\n"
                                   "[\"draw\", 1, 0],\n"
                                   "[\"draw\", 0 1]"));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_EQ(dialog.problemLine(), 4);
    EXPECT_TRUE(issues(dialog).startsWith(QStringLiteral("Line 4 is not a stroke: ")))
        << issues(dialog).toStdString();
    EXPECT_EQ(washedLines(dialog), (std::vector<int>{4}));
    // The reader's own place - a line and column of the text the lines are
    // read in, which nobody typed - is not passed on.
    EXPECT_FALSE(issues(dialog).contains(QStringLiteral("column"))) << issues(dialog).toStdString();

    // A stroke with a value short: an arc takes three.
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n[\"arc\", 1, 90]"));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_EQ(dialog.problemLine(), 2);
    EXPECT_TRUE(issues(dialog).startsWith(QStringLiteral(
        "Line 2: definition \"TEST Valve\" strokes[1]: \"arc\" takes a radius, a start angle "
        "and an end angle (3 after the word) and has 2")))
        << issues(dialog).toStdString();

    // What entity::validate refuses of a stroke - a text below the line - is
    // named by its line as well.
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n[\"text\", {\"text\": \"W\", "
                                   "\"height\": -1}]"));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_EQ(dialog.problemLine(), 2);
    EXPECT_TRUE(issues(dialog).contains(QStringLiteral("linestyle text height is negative")))
        << issues(dialog).toStdString();

    // A line that is nothing but a comma is not a stroke either.
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n,\n[\"draw\", 1, 0]"));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_EQ(dialog.problemLine(), 2);

    // A number no double holds, and a line short of its closing bracket: each
    // said of the line typed, with nothing of the text it is read in - no
    // "line 2, column 14", and no "}" where it is a "]" that is missing.
    strokes(dialog, QStringLiteral("[\"move\", 1e999, 0]"));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_EQ(issues(dialog),
              QStringLiteral("Line 1: definition \"TEST Valve\" strokes[0]: this stroke has a "
                             "number too large to hold - 1e999"));
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 1, 0"));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_EQ(dialog.problemLine(), 2);
    EXPECT_TRUE(issues(dialog).startsWith(QStringLiteral("Line 2 is not a stroke: ")))
        << issues(dialog).toStdString();
    EXPECT_TRUE(issues(dialog).contains(QStringLiteral("expected ']'")))
        << issues(dialog).toStdString();
    EXPECT_FALSE(issues(dialog).contains(QLatin1Char('}'))) << issues(dialog).toStdString();

    // Put right, it saves; the wash and the colour go.
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 1, 0]"));
    EXPECT_EQ(dialog.problemLine(), 0);
    EXPECT_TRUE(washedLines(dialog).empty());
    EXPECT_FALSE(saysAnError(dialog));
    ASSERT_TRUE(enabled(dialog, "definitionSave")) << issues(dialog).toStdString();
    press(dialog, "definitionSave");
    LineStyle expected = valve();
    expected.strokes = {move(0.0, 0.0), draw(1.0, 0.0)};
    EXPECT_TRUE(f.inLibrary("TEST Valve") == expected);
}

TEST(DefinitionEditor, TheStrokesBoxHoldsStrokesAndNothingElseOfTheDefinition)
{
    // A line that ends the list of strokes and goes on to give members: as
    // JSON it reads, and would save a definition at vertices, with anchors,
    // under a form whose box is unticked and whose anchors read 0.
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.editDefinition("TEST Kerb"));
    strokes(dialog, QStringLiteral("[\"move\", 0, 0]], \"atVertices\": true, \"anchors\": "
                                   "[[0, 0], [1, 1]"));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_TRUE(issues(dialog).startsWith(QStringLiteral("The strokes hold more than strokes")))
        << issues(dialog).toStdString();
    EXPECT_FALSE(dialog.save());
    EXPECT_TRUE(f.inLibrary("TEST Kerb") == kerb());
}

TEST(DefinitionEditor, ANumberThatDoesNotReadOrThatADefinitionMayNotHaveKeepsSaveDisabled)
{
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.editDefinition("TEST Kerb"));

    fill(dialog, "definitionLength", QStringLiteral("twelve"));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_EQ(issues(dialog), QStringLiteral("Length: \"twelve\" is not a number."));
    // A decimal comma is not a decimal point.
    fill(dialog, "definitionLength", QStringLiteral("1,5"));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    // Nor is anything that is not finite a length: the words for infinity
    // and for no number, and a number past the largest a double holds.
    for (const char* notFinite : {"inf", "-inf", "nan", "NaN", "Infinity", "1e999"}) {
        fill(dialog, "definitionLength", QString::fromLatin1(notFinite));
        EXPECT_FALSE(enabled(dialog, "definitionSave")) << notFinite;
        EXPECT_EQ(issues(dialog),
                  QStringLiteral("Length: \"%1\" is not a number.").arg(QLatin1String(notFinite)));
        EXPECT_FALSE(dialog.save()) << notFinite;
    }
    EXPECT_TRUE(saysAnError(dialog));

    // entity::validate's two rules for a definition's own numbers, in its words.
    fill(dialog, "definitionLength", QStringLiteral("-1"));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_TRUE(issues(dialog).contains(
        QStringLiteral("linestyle length must be finite and not negative")))
        << issues(dialog).toStdString();
    fill(dialog, "definitionLength", QStringLiteral("12"));
    fill(dialog, "definitionFactor", QStringLiteral("0"));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_TRUE(issues(dialog).contains(
        QStringLiteral("linestyle factor must be finite and greater than zero")))
        << issues(dialog).toStdString();
    EXPECT_FALSE(dialog.save());
    EXPECT_TRUE(f.inLibrary("TEST Kerb") == kerb());

    // An empty number is the value a file's missing member has: no length
    // said, a factor of 1.
    fill(dialog, "definitionLength", QString());
    fill(dialog, "definitionFactor", QString());
    ASSERT_TRUE(enabled(dialog, "definitionSave")) << issues(dialog).toStdString();
    press(dialog, "definitionSave");
    LineStyle expected = kerb();
    expected.length = 0.0;
    expected.factor = 1.0;
    EXPECT_TRUE(f.inLibrary("TEST Kerb") == expected);
}

TEST(DefinitionEditor, ANameTheLibraryHoldsCannotBeMadeASecondTime)
{
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.newDefinition(true));
    fill(dialog, "definitionName", QStringLiteral("TEST Valve"));
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 3, 0]"));

    // It reads as a definition, and is still not saved over the one there is.
    EXPECT_TRUE(dialog.definition().ok());
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_TRUE(issues(dialog).startsWith(
        QStringLiteral("\"TEST Valve\" is already in the library.")))
        << issues(dialog).toStdString();
    EXPECT_FALSE(dialog.save());
    EXPECT_TRUE(f.inLibrary("TEST Valve") == valve());
    EXPECT_EQ(f.document.styleLibrary().size(), 3u);

    // Under a name of its own it is; and that name is then taken too.
    fill(dialog, "definitionName", QStringLiteral("TEST Valve B"));
    ASSERT_TRUE(enabled(dialog, "definitionSave")) << issues(dialog).toStdString();
    press(dialog, "definitionSave");
    EXPECT_EQ(f.document.styleLibrary().size(), 4u);
    ASSERT_TRUE(dialog.newDefinition(true));
    fill(dialog, "definitionName", QStringLiteral("TEST Valve B"));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_EQ(f.document.styleLibrary().size(), 4u);
}

TEST(DefinitionEditor, ANameRulesAlreadyGiveIsSaidWhileItIsBeingMade)
{
    // Rule #1 could as well name a symbol nothing defines yet; making it is
    // how that is put right, and the form says who is waiting for it.
    EditorFixture f;
    katana::entity::SurveyMap map = f.document.surveyMap();
    katana::entity::SurveyRule pit;
    pit.key = "PX*";
    pit.section = katana::entity::SurveySection::VertexSymbol;
    pit.symbol = katana::entity::SurveySymbol{};
    pit.symbol->style = "TEST Pit";
    ASSERT_TRUE(map.add(pit).ok());
    f.document.setSurveyMap(std::move(map));

    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.newDefinition(true));
    fill(dialog, "definitionName", QStringLiteral("TEST Pit"));
    EXPECT_TRUE(issues(dialog).contains(QStringLiteral("rule #2 PX* (symbol) draws it as its "
                                                       "symbol")))
        << issues(dialog).toStdString();
    EXPECT_TRUE(enabled(dialog, "definitionSave"));
}

TEST(DefinitionEditor, DuplicateMakesASecondDefinitionEqualButForItsName)
{
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.duplicateDefinition("TEST Valve"));

    // The first free name, which can still be changed: the copy does not
    // exist until it is saved.
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionName")->text(), QStringLiteral("TEST Valve 2"));
    EXPECT_TRUE(enabled(dialog, "definitionName"));
    EXPECT_EQ(child<QLabel>(dialog, "definitionHeading")->text(),
              QStringLiteral("New symbol, a copy of \"TEST Valve\""));
    EXPECT_EQ(dialog.editedName(), "");
    EXPECT_EQ(f.document.styleLibrary().size(), 3u);
    ASSERT_TRUE(enabled(dialog, "definitionSave"));
    press(dialog, "definitionSave");

    LineStyle expected = valve();
    expected.name = "TEST Valve 2";
    EXPECT_TRUE(f.inLibrary("TEST Valve 2") == expected);
    EXPECT_TRUE(f.inLibrary("TEST Valve") == valve()) << "the original is as it was";
    EXPECT_EQ(f.document.styleLibrary().size(), 4u);

    // A second copy takes the next free name; one given a name of its own
    // keeps everything else.
    ASSERT_TRUE(dialog.duplicateDefinition("TEST Valve"));
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionName")->text(), QStringLiteral("TEST Valve 3"));
    fill(dialog, "definitionName", QStringLiteral("TEST Hydrant"));
    press(dialog, "definitionSave");
    expected.name = "TEST Hydrant";
    EXPECT_TRUE(f.inLibrary("TEST Hydrant") == expected);

    // The editor's own Duplicate copies the definition it is on.
    press(dialog, "definitionDuplicate");
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionName")->text(),
              QStringLiteral("TEST Hydrant 2"));
}

TEST(DefinitionEditor, DeleteOfADefinitionARuleNamesIsRefusedListingTheRuleAndDeleteAnywayRemovesIt)
{
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    dialog.show();
    ASSERT_TRUE(dialog.editDefinition("TEST Valve"));
    EXPECT_TRUE(withdrawn(dialog, "definitionDeleteAnyway"));
    EXPECT_TRUE(withdrawn(dialog, "definitionDeleteCancel"));
    EXPECT_FALSE(dialog.deleteAnywayOffered());

    press(dialog, "definitionDelete");
    // The plain line, as it would be typed; refused, since rule #1 and the
    // style Marks name the definition. What the line answered comes first,
    // word for word; then what names it; then what Delete Anyway would do -
    // a point is all that draws it, so a stand-in mark is all that follows.
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE REMOVE \"TEST Valve\"")}));
    EXPECT_TRUE(f.inLibrary("TEST Valve") == valve());
    EXPECT_EQ(issues(dialog),
              QStringLiteral("\"TEST Valve\" was not deleted: the definition is used\n"
                             "It is named by:\n"
                             "  rule #1 AC* (symbol) draws it as its symbol\n"
                             "  style \"Marks\" draws it as its symbol\n"
                             "Delete Anyway runs the line again with FORCE. A point naming it "
                             "is then drawn as a stand-in mark."));
    EXPECT_TRUE(saysAnError(dialog));
    EXPECT_TRUE(offered(dialog, "definitionDeleteAnyway"));
    EXPECT_TRUE(offered(dialog, "definitionDeleteCancel"));
    EXPECT_TRUE(dialog.deleteAnywayOffered());

    // Keep It takes the offer away, and nothing more is run.
    press(dialog, "definitionDeleteCancel");
    EXPECT_TRUE(withdrawn(dialog, "definitionDeleteAnyway"));
    EXPECT_EQ(f.ran.size(), 1);
    EXPECT_EQ(issues(dialog),
              QStringLiteral("Named by:\n"
                             "  rule #1 AC* (symbol) draws it as its symbol\n"
                             "  style \"Marks\" draws it as its symbol"));

    press(dialog, "definitionDelete");
    ASSERT_TRUE(offered(dialog, "definitionDeleteAnyway"));
    press(dialog, "definitionDeleteAnyway");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE REMOVE \"TEST Valve\""),
                                  QStringLiteral("CUSTOMISE REMOVE \"TEST Valve\""),
                                  QStringLiteral("CUSTOMISE REMOVE \"TEST Valve\" FORCE")}));
    EXPECT_EQ(f.document.styleLibrary().find("TEST Valve"), nullptr);
    EXPECT_EQ(f.document.styleLibrary().size(), 2u);
    EXPECT_TRUE(withdrawn(dialog, "definitionDeleteAnyway"));
    EXPECT_TRUE(issues(dialog).startsWith(QStringLiteral(
        "\"TEST Valve\" was deleted from the library. It is still shown here: Save puts it "
        "back.")))
        << issues(dialog).toStdString();
    EXPECT_EQ(child<QLabel>(dialog, "definitionHeading")->text(),
              QStringLiteral("Symbol \"TEST Valve\" - not in the library"));
    EXPECT_FALSE(enabled(dialog, "definitionDelete"));

    // The library has no undo; the form is the way back.
    ASSERT_TRUE(enabled(dialog, "definitionSave"));
    press(dialog, "definitionSave");
    EXPECT_TRUE(f.inLibrary("TEST Valve") == valve());
    EXPECT_TRUE(f.loggedExactly(
        QStringLiteral("Symbol \"TEST Valve\" added to the library: 6 strokes, 0 texts.")));
}

TEST(DefinitionEditor, DeleteAnywayIsNotCarriedOutUnlessItsRefusalIsOnThePage)
{
    // FORCE deletes what something uses, and the library has no undo. It is
    // offered under a refusal that lists what; asked for with none showing -
    // a script pressing the button, a caller - nothing is run.
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    dialog.show();
    ASSERT_TRUE(dialog.editDefinition("TEST Valve"));

    // The button ITSELF is hidden and disabled, not only the row it is in:
    // the headless driver asks the button, and a click of it does nothing.
    ASSERT_TRUE(withdrawn(dialog, "definitionDeleteAnyway"));
    press(dialog, "definitionDeleteAnyway");
    EXPECT_TRUE(f.ran.isEmpty());
    // Nor does the call behind it run a line.
    EXPECT_FALSE(dialog.remove(true));
    EXPECT_TRUE(f.ran.isEmpty());
    EXPECT_TRUE(f.inLibrary("TEST Valve") == valve());
    EXPECT_TRUE(issues(dialog).startsWith(
        QStringLiteral("Delete Anyway is offered once Delete has been refused")))
        << issues(dialog).toStdString();
    EXPECT_TRUE(withdrawn(dialog, "definitionDeleteAnyway")) << "saying so offers nothing";

    // An edit of the form withdraws an offer that was up: the refusal was of
    // the page as it then stood.
    press(dialog, "definitionDelete");
    ASSERT_TRUE(offered(dialog, "definitionDeleteAnyway"));
    fill(dialog, "definitionGroup", QStringLiteral("Test/Other"));
    EXPECT_TRUE(withdrawn(dialog, "definitionDeleteAnyway"));
    EXPECT_FALSE(dialog.remove(true));
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE REMOVE \"TEST Valve\"")}));
    EXPECT_TRUE(f.inLibrary("TEST Valve") == valve());
}

TEST(DefinitionEditor, ALineThatFailsForAnotherReasonIsShownAsItFailedAndAFailedForceOffersNothing)
{
    // The line fails, and not because the definition is used - here as the
    // window fails it while the verb is not there: REMOVE read as a file's
    // name. The editor shows what it was told, and does not say "is used".
    EditorFixture f;
    const QString reply = QStringLiteral("NotFound: cannot open this file [REMOVE]");
    f.context.run = [&f, reply](const QString& line) {
        f.ran << line;
        VerbOutcome outcome;
        outcome.error = reply;
        return outcome;
    };
    DefinitionEditorDialog dialog(f.context);
    dialog.show();
    ASSERT_TRUE(dialog.editDefinition("TEST Valve"));

    // Something names it, so FORCE is offered - under the line's own words.
    press(dialog, "definitionDelete");
    EXPECT_EQ(issues(dialog),
              QStringLiteral("\"TEST Valve\" was not deleted: NotFound: cannot open this file "
                             "[REMOVE]\n"
                             "It is named by:\n"
                             "  rule #1 AC* (symbol) draws it as its symbol\n"
                             "  style \"Marks\" draws it as its symbol\n"
                             "Delete Anyway runs the line again with FORCE. A point naming it "
                             "is then drawn as a stand-in mark."));
    EXPECT_FALSE(issues(dialog).contains(QStringLiteral("is used")));
    ASSERT_TRUE(offered(dialog, "definitionDeleteAnyway"));

    // The FORCE line fails as well: said, and nothing further is offered.
    press(dialog, "definitionDeleteAnyway");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE REMOVE \"TEST Valve\""),
                                  QStringLiteral("CUSTOMISE REMOVE \"TEST Valve\" FORCE")}));
    EXPECT_EQ(issues(dialog),
              QStringLiteral("\"TEST Valve\" was not deleted: NotFound: cannot open this file "
                             "[REMOVE]\n"
                             "Named by:\n"
                             "  rule #1 AC* (symbol) draws it as its symbol\n"
                             "  style \"Marks\" draws it as its symbol"));
    EXPECT_TRUE(saysAnError(dialog));
    EXPECT_TRUE(withdrawn(dialog, "definitionDeleteAnyway"));
    EXPECT_TRUE(withdrawn(dialog, "definitionDeleteCancel"));
    EXPECT_FALSE(dialog.deleteAnywayOffered());
    EXPECT_TRUE(f.inLibrary("TEST Valve") == valve());

    // Of a definition nothing names there is no FORCE to offer at all.
    ASSERT_TRUE(dialog.editDefinition("TEST Spare"));
    press(dialog, "definitionDelete");
    EXPECT_EQ(issues(dialog),
              QStringLiteral("\"TEST Spare\" was not deleted: NotFound: cannot open this file "
                             "[REMOVE]\n"
                             "No survey code, style or layer names it."));
    EXPECT_TRUE(withdrawn(dialog, "definitionDeleteAnyway"));
    EXPECT_TRUE(f.inLibrary("TEST Spare") == spare());

    // A line refused with nothing said at all is still said to be refused.
    f.context.run = [&f](const QString& line) {
        f.ran << line;
        return VerbOutcome{};
    };
    DefinitionEditorDialog mute(f.context);
    ASSERT_TRUE(mute.editDefinition("TEST Spare"));
    EXPECT_FALSE(mute.remove(false));
    EXPECT_TRUE(issues(mute).startsWith(
        QStringLiteral("\"TEST Spare\" was not deleted: the command was refused.")))
        << issues(mute).toStdString();
}

TEST(DefinitionEditor, ARefusedDeleteListsWhatNamesTheDefinitionWhateverTheFormReadsAs)
{
    // Delete acts on the library's definition, so it can be pressed over a
    // form that does not read; the offer of FORCE then still has its list
    // above it, of the definition the form is ON.
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    dialog.show();
    ASSERT_TRUE(dialog.editDefinition("TEST Valve"));
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n[\"drow\", 1, 0]"));
    ASSERT_FALSE(dialog.definition().ok());

    press(dialog, "definitionDelete");
    ASSERT_TRUE(offered(dialog, "definitionDeleteAnyway"));
    const QString said = issues(dialog);
    EXPECT_TRUE(said.startsWith(
        QStringLiteral("\"TEST Valve\" was not deleted: the definition is used\n"
                       "It is named by:\n"
                       "  rule #1 AC* (symbol) draws it as its symbol\n"
                       "  style \"Marks\" draws it as its symbol\n"
                       "Delete Anyway runs the line again with FORCE. A point naming it is "
                       "then drawn as a stand-in mark.\n"
                       "Line 2: definition \"TEST Valve\" strokes[1]: \"drow\" is not a kind of "
                       "stroke")))
        << said.toStdString();
    EXPECT_EQ(washedLines(dialog), (std::vector<int>{2}));
}

TEST(DefinitionEditor, DeleteOfAFormThatIsNotInTheLibraryRunsNothing)
{
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    // A new definition: there is nothing of it to delete yet.
    ASSERT_TRUE(dialog.newDefinition(true));
    fill(dialog, "definitionName", QStringLiteral("TEST Post"));
    EXPECT_FALSE(enabled(dialog, "definitionDelete"));
    EXPECT_FALSE(dialog.remove(false));
    EXPECT_TRUE(f.ran.isEmpty());
    EXPECT_TRUE(issues(dialog).startsWith(
        QStringLiteral("Delete: the form holds no definition that is in the library.")))
        << issues(dialog).toStdString();
    EXPECT_TRUE(saysAnError(dialog));
    EXPECT_EQ(f.document.styleLibrary().size(), 3u);
}

TEST(DefinitionEditor, ADefinitionNothingNamesIsDeletedByTheOneLine)
{
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    // What the symbol library's Delete asks: shown, then deleted.
    EXPECT_TRUE(dialog.deleteDefinition("TEST Spare"));
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE REMOVE \"TEST Spare\"")}));
    EXPECT_EQ(f.document.styleLibrary().find("TEST Spare"), nullptr);
    EXPECT_EQ(dialog.editedName(), "TEST Spare");
    EXPECT_TRUE(withdrawn(dialog, "definitionDeleteAnyway"));

    // A name of one word is quoted on the line all the same, as the line is
    // documented: it reads the same either way.
    LineStyle lone = spare();
    lone.name = "Lone";
    StyleLibrary withLone = f.document.styleLibrary();
    ASSERT_TRUE(withLone.add(lone).ok());
    f.document.setStyleLibrary(std::move(withLone));
    f.ran.clear();
    DefinitionEditorDialog other(f.context);
    EXPECT_TRUE(other.deleteDefinition("Lone"));
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE REMOVE \"Lone\"")}));
    EXPECT_EQ(f.document.styleLibrary().find("Lone"), nullptr);

    // A refusal that is not about users - here, no command line at all - is
    // said, and offers no Delete Anyway: FORCE would not answer it.
    EditorFixture bare;
    bare.context.run = {};
    DefinitionEditorDialog alone(bare.context);
    EXPECT_FALSE(alone.deleteDefinition("TEST Spare"));
    EXPECT_TRUE(issues(alone).startsWith(
        QStringLiteral("Nothing here can run CUSTOMISE REMOVE")))
        << issues(alone).toStdString();
    EXPECT_TRUE(bare.inLibrary("TEST Spare") == spare());
    EXPECT_TRUE(withdrawn(alone, "definitionDeleteAnyway"));
}

TEST(DefinitionEditor, ANameTheCommandLineCannotCarryIsNotPutOnALine)
{
    // A word of a command line has no escape for a double quote: the line
    // would run on another name, cut short at the quote. And its quotes do
    // not survive the tokenizer, so a definition named CODE is the verb's own
    // word: `CUSTOMISE REMOVE "CODE" FORCE` would remove the survey code
    // FORCE. Such names come out of a file; they are opened and changed, and
    // Delete says why it cannot name them.
    EditorFixture f;
    LineStyle quoted = spare();
    quoted.name = "TEST 6\" Pipe";
    LineStyle keyword = spare();
    keyword.name = "CODE";
    StyleLibrary library = f.document.styleLibrary();
    ASSERT_TRUE(library.add(quoted).ok());
    ASSERT_TRUE(library.add(keyword).ok());
    f.document.setStyleLibrary(std::move(library));

    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.editDefinition(quoted.name));
    EXPECT_FALSE(dialog.remove(false));
    EXPECT_TRUE(f.ran.isEmpty());
    EXPECT_EQ(issues(dialog).section(QLatin1Char('\n'), 0, 0),
              QStringLiteral("Delete: no command line can name \"TEST 6\" Pipe\" - a double quote "
                             "cannot be written on a command line."));
    EXPECT_TRUE(f.inLibrary(quoted.name.c_str()) == quoted);
    // It is edited like any other: its name is its own.
    fill(dialog, "definitionGroup", QStringLiteral("Test/Pipes"));
    ASSERT_TRUE(enabled(dialog, "definitionSave")) << issues(dialog).toStdString();
    press(dialog, "definitionSave");
    LineStyle regrouped = quoted;
    regrouped.group = "Test/Pipes";
    EXPECT_TRUE(f.inLibrary(quoted.name.c_str()) == regrouped);

    ASSERT_TRUE(dialog.editDefinition("CODE"));
    EXPECT_FALSE(dialog.remove(false));
    EXPECT_TRUE(f.ran.isEmpty());
    EXPECT_EQ(issues(dialog).section(QLatin1Char('\n'), 0, 0),
              QStringLiteral("Delete: no command line can name \"CODE\" - the name is a word of "
                             "the CUSTOMISE REMOVE line itself (CODE), which a command line "
                             "cannot tell from the definition."));
    EXPECT_TRUE(f.inLibrary("CODE") == keyword);
}

TEST(DefinitionEditor, ADefinitionIsNotMadeUnderANameNoLineCouldRemoveItBy)
{
    // Saved, it could never be deleted again - not here, and not by the line
    // an agent types.
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.newDefinition(true));
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 0, 2]"));

    fill(dialog, "definitionName", QStringLiteral("6\" Valve"));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_EQ(issues(dialog),
              QStringLiteral("A new definition cannot be named \"6\" Valve\": a double quote "
                             "cannot be written on a command line."));
    EXPECT_FALSE(dialog.save());
    EXPECT_FALSE(dialog.definition().ok());
    for (const char* word : {"Force", "code"}) {
        fill(dialog, "definitionName", QString::fromLatin1(word));
        EXPECT_FALSE(enabled(dialog, "definitionSave")) << word;
        EXPECT_TRUE(issues(dialog).startsWith(
            QStringLiteral("A new definition cannot be named \"%1\": the name is a word of the "
                           "CUSTOMISE REMOVE line itself")
                .arg(QLatin1String(word))))
            << issues(dialog).toStdString();
        EXPECT_FALSE(dialog.save());
    }
    EXPECT_EQ(f.document.styleLibrary().size(), 3u);

    // A name that only begins with such a word is a name like any other.
    fill(dialog, "definitionName", QStringLiteral("Force main"));
    ASSERT_TRUE(enabled(dialog, "definitionSave")) << issues(dialog).toStdString();
    press(dialog, "definitionSave");
    EXPECT_EQ(f.document.styleLibrary().size(), 4u);
    EXPECT_NE(f.document.styleLibrary().find("Force main"), nullptr);
}

TEST(DefinitionEditor, TheCommitHookIsCalledBeforeASaveAndWhatItHandsBackAfterIt)
{
    // The hook is how a session that was kept stays kept: its maker runs
    // CUSTOMISE KEEP in what it hands back, and hands nothing back when the
    // session was not kept. The editor knows neither; it calls, and counts on
    // the order.
    EditorFixture f;
    bool kept = true;
    int begun = 0;
    int finished = 0;
    std::uint64_t generationAtBegin = 0;
    std::uint64_t generationAtFinish = 0;
    f.context.beginCommit = [&]() -> std::function<void()> {
        ++begun;
        generationAtBegin = f.document.libraryGeneration();
        if (!kept) {
            return {};
        }
        return [&] {
            ++finished;
            generationAtFinish = f.document.libraryGeneration();
            f.context.run(QStringLiteral("CUSTOMISE KEEP"));
        };
    };
    // KEEP is not this runner's to carry out; it is only recorded.
    f.context.run = [&f](const QString& line) {
        f.ran << line;
        return line == QStringLiteral("CUSTOMISE KEEP") ? VerbOutcome{true, {}, {}}
                                                         : removeAsTheVerbWould(f.document, line);
    };
    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.editDefinition("TEST Kerb"));

    // A Save that is refused commits nothing, and the hook is not troubled.
    fill(dialog, "definitionLength", QStringLiteral("-1"));
    EXPECT_FALSE(dialog.save());
    EXPECT_EQ(begun, 0);

    const std::uint64_t before = f.document.libraryGeneration();
    fill(dialog, "definitionLength", QStringLiteral("20"));
    press(dialog, "definitionSave");
    EXPECT_EQ(begun, 1);
    EXPECT_EQ(finished, 1);
    EXPECT_EQ(generationAtBegin, before) << "asked before the library was touched";
    EXPECT_EQ(generationAtFinish, before + 1) << "answered once it had been";
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE KEEP")}));

    // Not kept: asked all the same, and nothing is run.
    kept = false;
    fill(dialog, "definitionLength", QStringLiteral("21"));
    press(dialog, "definitionSave");
    EXPECT_EQ(begun, 2);
    EXPECT_EQ(finished, 1);
    EXPECT_EQ(f.ran.size(), 1);
    EXPECT_EQ(f.inLibrary("TEST Kerb").length, 21.0);

    // A Delete is the editor's commit too: after the line that removed the
    // definition, and not after the one that was refused.
    kept = true;
    f.ran.clear();
    ASSERT_TRUE(dialog.editDefinition("TEST Valve"));
    press(dialog, "definitionDelete");
    EXPECT_EQ(begun, 3);
    EXPECT_EQ(finished, 1) << "refused: nothing was committed";
    press(dialog, "definitionDeleteAnyway");
    EXPECT_EQ(begun, 4);
    EXPECT_EQ(finished, 2);
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE REMOVE \"TEST Valve\""),
                                  QStringLiteral("CUSTOMISE REMOVE \"TEST Valve\" FORCE"),
                                  QStringLiteral("CUSTOMISE KEEP")}));
}

TEST(DefinitionEditor, TheUnitsAreChosenByTheWordsAFileHolds)
{
    // docs/customisation.md, "The members": `units` is `world`, `paper` or
    // `twoPoint`, and `world` is what a file that says nothing means. The box
    // offers those three words, and what it gives a definition is what the
    // format's own writer then writes.
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.editDefinition("TEST Kerb"));
    const auto* units = child<QComboBox>(dialog, "definitionUnits");
    ASSERT_EQ(units->count(), 3);
    EXPECT_EQ(units->itemText(0), QStringLiteral("world"));
    EXPECT_EQ(units->itemText(1), QStringLiteral("paper"));
    EXPECT_EQ(units->itemText(2), QStringLiteral("twoPoint"));

    const auto written = [&dialog] {
        const auto read = dialog.definition();
        EXPECT_TRUE(read.ok());
        const auto text = katana::entity::definitionToJson(read.ok() ? *read : LineStyle{});
        EXPECT_TRUE(text.ok());
        return text.ok() ? *text : std::string();
    };
    choose(dialog, "definitionUnits", QStringLiteral("world"));
    EXPECT_EQ(written().find("\"units\""), std::string::npos) << written();
    EXPECT_FALSE(enabled(dialog, "definitionAnchor1X"));
    choose(dialog, "definitionUnits", QStringLiteral("paper"));
    EXPECT_NE(written().find("\"units\": \"paper\""), std::string::npos) << written();
    EXPECT_FALSE(enabled(dialog, "definitionStretchMode"));
    // Two-point: the anchors and the two modes can be typed.
    choose(dialog, "definitionUnits", QStringLiteral("twoPoint"));
    EXPECT_NE(written().find("\"units\": \"twoPoint\""), std::string::npos) << written();
    for (const char* name : {"definitionAnchor1X", "definitionAnchor1Y", "definitionAnchor2X",
                             "definitionAnchor2Y", "definitionStretchMode",
                             "definitionCycleMode"}) {
        EXPECT_TRUE(enabled(dialog, name)) << name;
    }
    fill(dialog, "definitionAnchor2X", QStringLiteral("8"));
    child<QSpinBox>(dialog, "definitionStretchMode")->setValue(1);
    press(dialog, "definitionSave");
    LineStyle expected = kerb();
    expected.units = StyleUnits::TwoPoint;
    expected.anchor2 = {8.0, 0.0};
    expected.stretchMode = 1;
    EXPECT_TRUE(f.inLibrary("TEST Kerb") == expected);

    // The kind is which of a file's two lists holds it.
    choose(dialog, "definitionKind", QStringLiteral("Symbol"));
    press(dialog, "definitionSave");
    expected.symbol = true;
    EXPECT_TRUE(f.inLibrary("TEST Kerb") == expected);
}

TEST(DefinitionEditor, ThePreviewDrawsWhatIsTypedBeforeAnyOfItIsSaved)
{
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    dialog.show();
    katana::qt::test::processEvents();
    ASSERT_TRUE(dialog.newDefinition(false));
    katana::qt::StylePreview* asSymbol = dialog.symbolPreview();
    katana::qt::StylePreview* alongLine = dialog.linePreview();
    const auto* note = child<QLabel>(dialog, "definitionPreviewNote");
    ASSERT_TRUE(alongLine->isVisibleTo(&dialog)) << "a linestyle is shown along a line too";

    // The baseline is a definition with NO strokes, which is what a new form
    // is: each pane already shows what it shows of any definition - its
    // scale bar, and the symbol pane the mark of the insertion point - so
    // what differs from it below is the strokes and nothing else.
    choose(dialog, "definitionUnits", QStringLiteral("paper"));
    fill(dialog, "definitionLength", QStringLiteral("6"));
    const QImage emptySymbol = asSymbol->grab().toImage();
    const QImage emptyLine = alongLine->grab().toImage();
    // Which pane draws it as which: only a symbol has an insertion point.
    EXPECT_TRUE(asSymbol->insertionPoint().has_value()) << "the origin is marked";
    EXPECT_FALSE(alongLine->insertionPoint().has_value());
    EXPECT_GT(asSymbol->scaleBarMetres(), 0.0);
    EXPECT_GT(alongLine->scaleBarMetres(), 0.0);

    // Strokes, and still no name: a definition is pictured from its first
    // stroke, though it cannot be saved until it is named.
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 4, 0]"));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_TRUE(issues(dialog).startsWith(QStringLiteral("Type a name")));
    const QImage dashSymbol = asSymbol->grab().toImage();
    const QImage dashLine = alongLine->grab().toImage();
    EXPECT_TRUE(differ(dashSymbol, emptySymbol));
    EXPECT_TRUE(differ(dashLine, emptyLine));
    // Drawn as what it is: nothing in either pane says it is undefined, or a
    // symbol where a linestyle was meant.
    EXPECT_TRUE(asSymbol->notice().isEmpty()) << asSymbol->notice().toStdString();
    EXPECT_TRUE(alongLine->notice().isEmpty()) << alongLine->notice().toStdString();
    EXPECT_TRUE(note->text().isEmpty());

    // Named, it is the same picture - the name is no part of it - and can be
    // saved; nothing has reached the library.
    fill(dialog, "definitionName", QStringLiteral("TEST Dashes"));
    EXPECT_TRUE(enabled(dialog, "definitionSave"));
    EXPECT_FALSE(differ(asSymbol->grab().toImage(), dashSymbol));
    EXPECT_FALSE(differ(alongLine->grab().toImage(), dashLine));
    EXPECT_EQ(f.document.styleLibrary().find("TEST Dashes"), nullptr);

    // Live: another stroke, another picture in BOTH panes - under the same
    // name, which a cache keyed on names alone would answer with the last.
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 4, 0],\n[\"draw\", 4, 3]"));
    const QImage hookSymbol = asSymbol->grab().toImage();
    const QImage hookLine = alongLine->grab().toImage();
    EXPECT_TRUE(differ(hookSymbol, dashSymbol));
    EXPECT_TRUE(differ(hookLine, dashLine));

    // While a line is half typed the last picture that read stays up, and
    // the note under the panes says it is not of what is typed.
    strokes(dialog,
            QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 4, 0],\n[\"draw\", 4, 3],\n[\"dr"));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_FALSE(differ(asSymbol->grab().toImage(), hookSymbol));
    EXPECT_FALSE(differ(alongLine->grab().toImage(), hookLine));
    EXPECT_EQ(note->text(),
              QStringLiteral("Not what is typed: the picture is of the last text that read."));
    // Finished, the note goes.
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 4, 0],\n[\"draw\", 4, 3],\n"
                                   "[\"draw\", 0, 3]"));
    EXPECT_TRUE(note->text().isEmpty());
    EXPECT_TRUE(differ(asSymbol->grab().toImage(), hookSymbol));

    // At another plot scale both panes are laid out at it.
    const int before = asSymbol->scaleDenominator();
    EXPECT_EQ(alongLine->scaleDenominator(), before);
    const QString other = before == 1000 ? QStringLiteral("1:200") : QStringLiteral("1:1000");
    choose(dialog, "definitionPlotScale", other);
    EXPECT_EQ(asSymbol->scaleDenominator(), before == 1000 ? 200 : 1000);
    EXPECT_EQ(alongLine->scaleDenominator(), before == 1000 ? 200 : 1000);
    choose(dialog, "definitionPlotScale", QStringLiteral("1:%1").arg(before));

    // A symbol has no pattern along a line: that pane goes.
    choose(dialog, "definitionKind", QStringLiteral("Symbol"));
    EXPECT_FALSE(alongLine->isVisibleTo(&dialog));
    // Another definition is not shown under the last one's picture: a new
    // one's panes are those of no strokes again.
    choose(dialog, "definitionKind", QStringLiteral("Linestyle"));
    press(dialog, "definitionRevert");
    ASSERT_TRUE(dialog.newDefinition(false));
    choose(dialog, "definitionUnits", QStringLiteral("paper"));
    fill(dialog, "definitionLength", QStringLiteral("6"));
    EXPECT_FALSE(differ(asSymbol->grab().toImage(), emptySymbol));
    EXPECT_FALSE(differ(alongLine->grab().toImage(), emptyLine));
    EXPECT_TRUE(note->text().isEmpty());
}

TEST(DefinitionEditor, AnotherDefinitionIsNotOpenedOverEditsThatAreNotSaved)
{
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.editDefinition("TEST Kerb"));
    fill(dialog, "definitionLength", QStringLiteral("15"));

    for (const DefinitionEdit what : {DefinitionEdit::NewSymbol, DefinitionEdit::NewLinestyle,
                                      DefinitionEdit::Edit, DefinitionEdit::Duplicate,
                                      DefinitionEdit::Delete}) {
        EXPECT_FALSE(dialog.request(what, "TEST Valve"));
    }
    EXPECT_EQ(dialog.editedName(), "TEST Kerb");
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionLength")->text(), QStringLiteral("15"));
    EXPECT_TRUE(f.ran.isEmpty()) << "and nothing was deleted";
    EXPECT_TRUE(f.loggedExactly(
        QStringLiteral("Edit \"TEST Valve\": the definition editor holds edits to \"TEST Kerb\" "
                       "that are not saved. Save or Revert them first.")));
    // The last of the five asked was the Delete, and the form says so.
    EXPECT_TRUE(issues(dialog).startsWith(QStringLiteral("Delete \"TEST Valve\": ")))
        << issues(dialog).toStdString();

    // Asked for again, the definition it is on is simply still there.
    EXPECT_TRUE(dialog.editDefinition("TEST Kerb"));
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionLength")->text(), QStringLiteral("15"));

    press(dialog, "definitionRevert");
    EXPECT_TRUE(dialog.editDefinition("TEST Valve"));
    EXPECT_EQ(dialog.editedName(), "TEST Valve");
    // A name the library does not define opens nothing.
    EXPECT_FALSE(dialog.editDefinition("TEST Nowhere"));
    EXPECT_FALSE(dialog.duplicateDefinition("TEST Nowhere"));
    EXPECT_EQ(dialog.editedName(), "TEST Valve");
}

TEST(DefinitionEditor, ADefinitionChangedElsewhereIsTakenUpUneditedAndSaidOverEdits)
{
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.editDefinition("TEST Kerb"));
    const auto elsewhere = [&f](double length) {
        StyleLibrary library = f.document.styleLibrary();
        LineStyle changed = kerb();
        changed.length = length;
        ASSERT_TRUE(library.update(changed).ok());
        f.document.setStyleLibrary(std::move(library));
        katana::qt::test::processEvents();
    };

    // Nothing typed: the form shows what the library has now.
    elsewhere(20.0);
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionLength")->text(), QStringLiteral("20"));
    EXPECT_FALSE(dialog.dirty());

    // Something typed: it is kept, the change is said, and Revert shows the
    // library's definition as it is now - not as it was when the form opened.
    fill(dialog, "definitionGroup", QStringLiteral("Test/Mine"));
    elsewhere(30.0);
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionGroup")->text(), QStringLiteral("Test/Mine"));
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionLength")->text(), QStringLiteral("20"));
    EXPECT_TRUE(dialog.dirty());
    EXPECT_TRUE(issues(dialog).startsWith(
        QStringLiteral("\"TEST Kerb\" was changed elsewhere since it was opened here.")))
        << issues(dialog).toStdString();
    press(dialog, "definitionRevert");
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionGroup")->text(), QStringLiteral("Test/Lines"));
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionLength")->text(), QStringLiteral("30"));
    EXPECT_FALSE(dialog.dirty());

    // Removed elsewhere: said, the form kept, and Save puts it back.
    fill(dialog, "definitionLength", QStringLiteral("31"));
    StyleLibrary without = f.document.styleLibrary();
    ASSERT_TRUE(without.remove("TEST Kerb").ok());
    f.document.setStyleLibrary(std::move(without));
    katana::qt::test::processEvents();
    EXPECT_TRUE(issues(dialog).startsWith(
        QStringLiteral("\"TEST Kerb\" was removed from the library elsewhere.")))
        << issues(dialog).toStdString();
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionLength")->text(), QStringLiteral("31"));
    ASSERT_TRUE(enabled(dialog, "definitionSave"));
    press(dialog, "definitionSave");
    LineStyle expected = kerb();
    expected.length = 31.0;
    EXPECT_TRUE(f.inLibrary("TEST Kerb") == expected);
}

TEST(DefinitionEditor, NoButtonIsADefaultAndEnterInAFieldPressesNothing)
{
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(katana::qt::test::showActive(dialog));
    ASSERT_TRUE(dialog.editDefinition("TEST Kerb"));
    fill(dialog, "definitionLength", QStringLiteral("15"));
    for (const QPushButton* button : dialog.findChildren<QPushButton*>()) {
        EXPECT_FALSE(button->isDefault()) << button->objectName().toStdString();
        EXPECT_FALSE(button->autoDefault()) << button->objectName().toStdString();
    }
    // With an edit waiting, Enter in a field would Save it - or, the first
    // button being New, try to throw it away - were any button a default.
    for (const char* name : {"definitionGroup", "definitionLength", "definitionFactor",
                             "definitionOriginX"}) {
        QKeyEvent enter(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier);
        QCoreApplication::sendEvent(child<QLineEdit>(dialog, name), &enter);
    }
    katana::qt::test::processEvents();
    EXPECT_TRUE(f.logged.empty()) << f.logged.front().first.toStdString();
    EXPECT_TRUE(dialog.isVisible());
    EXPECT_TRUE(dialog.dirty());
    EXPECT_TRUE(f.inLibrary("TEST Kerb") == kerb());
}

TEST(DefinitionEditor, ClosingItKeepsWhatWasTypedAndSaysSo)
{
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    dialog.show();
    ASSERT_TRUE(dialog.editDefinition("TEST Kerb"));
    fill(dialog, "definitionLength", QStringLiteral("15"));
    press(dialog, "definitionClose");
    EXPECT_FALSE(dialog.isVisible());
    EXPECT_TRUE(dialog.dirty());
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionLength")->text(), QStringLiteral("15"));
    EXPECT_TRUE(f.loggedExactly(QStringLiteral(
        "The definition editor was closed with edits that are not saved; they are kept in "
        "it.")));
}

TEST(DefinitionEditor, TheDialogOutlivesItsDocumentAndDoesNothingAfterIt)
{
    auto fixture = std::make_unique<EditorFixture>();
    CustomisationContext context = fixture->context;
    // What the fixture's hooks point at goes with it.
    context.log = {};
    context.run = {};
    DefinitionEditorDialog dialog(context);
    ASSERT_TRUE(dialog.editDefinition("TEST Kerb"));
    fill(dialog, "definitionLength", QStringLiteral("15"));

    fixture.reset();
    katana::qt::test::processEvents();

    EXPECT_FALSE(dialog.save());
    EXPECT_FALSE(dialog.remove(false));
    EXPECT_FALSE(dialog.remove(true));
    EXPECT_FALSE(dialog.newDefinition(true));
    EXPECT_FALSE(dialog.editDefinition("TEST Valve"));
    EXPECT_FALSE(dialog.duplicateDefinition("TEST Kerb"));
    dialog.revert();
    // Typing still only reads the form.
    fill(dialog, "definitionGroup", QStringLiteral("Test/After"));
    strokes(dialog, QStringLiteral("[\"move\", 0, 0]"));
    EXPECT_TRUE(issues(dialog).startsWith(
        QStringLiteral("The drawing this editor worked on has closed")))
        << issues(dialog).toStdString();
    for (const char* name : {"definitionSave", "definitionRevert", "definitionNew",
                             "definitionDuplicate", "definitionDelete", "definitionStrokes",
                             "definitionName"}) {
        EXPECT_FALSE(enabled(dialog, name)) << name;
    }
    EXPECT_TRUE(enabled(dialog, "definitionClose"));
    katana::qt::test::paint(dialog);
}

// ---- what the strokes box can and cannot hold ------------------------------------

TEST(DefinitionEditor, ATextHoldingANoBreakSpaceOpensUneditedAndIsSavedCharacterForCharacter)
{
    // A no-break space (U+00A0) and a line separator (U+2028) in a stroke's
    // text are characters of it. A text box asked for its "plain text" hands
    // the first back as a blank and the second as a line break; the form
    // would then differ from what it was opened with before anything was
    // typed - "edited, not saved" for ever, every other definition refused -
    // and Save would write the blank.
    LineStyle label = spare();
    label.name = "TEST Label";
    StrokeText words;
    words.text = "A\xC2\xA0" "B\xE2\x80\xA8" "C";
    words.height = 1.0;
    label.texts = {words};
    label.strokes = {move(0.0, 0.0), Stroke{.op = StrokeOp::Text, .text = 0}};
    EditorFixture f;
    StyleLibrary library = f.document.styleLibrary();
    ASSERT_TRUE(library.add(label).ok());
    f.document.setStyleLibrary(std::move(library));

    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.editDefinition("TEST Label")) << issues(dialog).toStdString();
    EXPECT_FALSE(dialog.dirty());
    EXPECT_EQ(child<QLabel>(dialog, "definitionHeading")->text(),
              QStringLiteral("Symbol \"TEST Label\""));
    EXPECT_FALSE(enabled(dialog, "definitionSave"));
    EXPECT_FALSE(enabled(dialog, "definitionRevert"));
    const auto read = dialog.definition();
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(*read == label);

    // Nothing is unsaved, so another definition opens, and this one again.
    EXPECT_TRUE(dialog.editDefinition("TEST Kerb"));
    EXPECT_TRUE(dialog.editDefinition("TEST Label"));
    EXPECT_FALSE(dialog.dirty());

    // A change elsewhere in the form saves the text as it was.
    fill(dialog, "definitionGroup", QStringLiteral("Test/Labels"));
    press(dialog, "definitionSave");
    LineStyle expected = label;
    expected.group = "Test/Labels";
    EXPECT_TRUE(f.inLibrary("TEST Label") == expected);
    EXPECT_EQ(f.inLibrary("TEST Label").texts.front().text, words.text);

    // Typed into a new definition, the two characters are kept the same way.
    ASSERT_TRUE(dialog.newDefinition(true));
    fill(dialog, "definitionName", QStringLiteral("TEST Typed"));
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n[\"text\", {\"text\": \"A%1B%2C\", "
                                   "\"height\": 1}]")
                        .arg(QChar(0x00A0))
                        .arg(QChar(0x2028)));
    ASSERT_TRUE(enabled(dialog, "definitionSave")) << issues(dialog).toStdString();
    press(dialog, "definitionSave");
    EXPECT_EQ(f.inLibrary("TEST Typed").texts.front().text, words.text);
    EXPECT_FALSE(dialog.dirty());
}

TEST(DefinitionEditor, ADefinitionTheFormCannotShowIsNotOpenedOrCopiedAndIsSaidWhenItArrives)
{
    // Two ways a library can hold what the form cannot show: a stroke the
    // format cannot write - a move that carries a radius - and a text holding
    // a paragraph separator (U+2029), at which the strokes box breaks a line,
    // so the stroke could not be kept on one. Neither is opened as something
    // it is not.
    LineStyle stray = spare();
    stray.name = "TEST Stray";
    stray.strokes = {Stroke{.op = StrokeOp::Move, .point = {0.0, 0.0}, .radius = 2.0}};
    LineStyle broken = spare();
    broken.name = "TEST Broken";
    StrokeText words;
    words.text = "A\xE2\x80\xA9" "B";
    words.height = 1.0;
    broken.texts = {words};
    broken.strokes = {move(0.0, 0.0), Stroke{.op = StrokeOp::Text, .text = 0}};
    EditorFixture f;
    StyleLibrary library = f.document.styleLibrary();
    ASSERT_TRUE(library.add(stray).ok());
    ASSERT_TRUE(library.add(broken).ok());
    f.document.setStyleLibrary(std::move(library));

    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.editDefinition("TEST Kerb"));

    EXPECT_FALSE(dialog.editDefinition("TEST Stray"));
    EXPECT_TRUE(issues(dialog).startsWith(
        QStringLiteral("\"TEST Stray\" cannot be edited here, because the customisation format "
                       "cannot write it: ")))
        << issues(dialog).toStdString();
    EXPECT_TRUE(saysAnError(dialog));
    EXPECT_FALSE(dialog.duplicateDefinition("TEST Stray"));
    EXPECT_TRUE(issues(dialog).startsWith(
        QStringLiteral("\"TEST Stray\" cannot be copied here, because the customisation format "
                       "cannot write it: ")))
        << issues(dialog).toStdString();
    // Delete is asked through Edit, and so is refused the same way: nothing ran.
    EXPECT_FALSE(dialog.deleteDefinition("TEST Stray"));
    EXPECT_TRUE(f.ran.isEmpty());

    EXPECT_FALSE(dialog.editDefinition("TEST Broken"));
    EXPECT_EQ(issues(dialog).section(QLatin1Char('\n'), 0, 0),
              QStringLiteral("\"TEST Broken\" cannot be edited here, because one of its texts "
                             "holds a paragraph separator, which the strokes box breaks a line "
                             "at: its strokes could not be shown one a line"));
    EXPECT_FALSE(dialog.duplicateDefinition("TEST Broken"));

    // Each refusal is in the log as an error, and the form is still on what
    // it was on.
    EXPECT_GE(f.logged.size(), 4u);
    for (const auto& line : f.logged) {
        EXPECT_TRUE(line.second) << line.first.toStdString();
    }
    EXPECT_EQ(dialog.editedName(), "TEST Kerb");
    EXPECT_EQ(child<QPlainTextEdit>(dialog, "definitionStrokes")->toPlainText(),
              QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 8, 0]"));

    // The definition a form is on changed elsewhere into such a thing: said,
    // and what the form shows is kept. (A second editor, opened on it and
    // told nothing yet: the first still shows its last refusal first, as it
    // does until its form is next edited.)
    DefinitionEditorDialog second(f.context);
    ASSERT_TRUE(second.editDefinition("TEST Kerb"));
    LineStyle kerbStray = kerb();
    kerbStray.strokes = stray.strokes;
    StyleLibrary changed = f.document.styleLibrary();
    ASSERT_TRUE(changed.update(kerbStray).ok());
    f.document.setStyleLibrary(std::move(changed));
    katana::qt::test::processEvents();
    EXPECT_TRUE(issues(second).startsWith(
        QStringLiteral("\"TEST Kerb\" was changed elsewhere into something that cannot be shown "
                       "here, because the customisation format cannot write it: ")))
        << issues(second).toStdString();
    EXPECT_TRUE(issues(second).contains(QStringLiteral("What is typed here is kept.")));
    EXPECT_EQ(child<QPlainTextEdit>(second, "definitionStrokes")->toPlainText(),
              QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 8, 0]"));
    // The first editor is told the same, under what it last said.
    EXPECT_TRUE(issues(dialog).contains(
        QStringLiteral("\"TEST Kerb\" was changed elsewhere into something that cannot be shown "
                       "here")))
        << issues(dialog).toStdString();
    EXPECT_EQ(child<QPlainTextEdit>(dialog, "definitionStrokes")->toPlainText(),
              QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 8, 0]"));
    EXPECT_TRUE(f.inLibrary("TEST Kerb") == kerbStray);
}

// ---- a name the drawing itself also answers to -----------------------------------

TEST(DefinitionEditor, ANameTheDrawingAlsoAnswersToIsSaidAsThatAndNotAsNamingNothing)
{
    // The drawing has a linetype "Fence" of its own, and a layer names it: the
    // layer's lines are dashed by that linetype (decision D2,
    // cad/style_resolver.hpp), not drawn plain. A library linestyle made
    // under the same name is drawn in its place once saved. That is allowed -
    // a name in both is reported, never an error - and is said before Save.
    EditorFixture f;
    katana::entity::Linetype fence;
    fence.name = "Fence";
    fence.pattern = {katana::entity::LinetypeElement{2.0}, katana::entity::LinetypeElement{-1.0}};
    ASSERT_TRUE(f.document.execute(katana::commands::createLinetype(fence)).ok());
    katana::entity::Layer fences;
    fences.name = "fences";
    fences.linetype = "Fence";
    ASSERT_TRUE(f.document.execute(katana::commands::createLayer(fences)).ok());

    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.newDefinition(false));
    fill(dialog, "definitionName", QStringLiteral("Fence"));
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 2, 0]"));
    EXPECT_EQ(issues(dialog),
              QStringLiteral("Named already:\n"
                             "  layer \"fences\" names it as its linetype\n"
                             "The drawing has a linetype \"Fence\" of its own, which draws the "
                             "lines that name it now. Saved, this definition is drawn in its "
                             "place, and Styles and Linetypes lists the name under Diagnostics "
                             "as one in both."));
    EXPECT_TRUE(enabled(dialog, "definitionSave"));
    EXPECT_FALSE(saysAnError(dialog));
    // At vertices it is not drawn along a line at all, and takes nothing over.
    child<QCheckBox>(dialog, "definitionAtVertices")->setChecked(true);
    EXPECT_TRUE(issues(dialog).endsWith(
        QStringLiteral("The drawing has a linetype \"Fence\" of its own, which goes on drawing "
                       "the lines that name it: a definition at vertices is not drawn along a "
                       "line.")))
        << issues(dialog).toStdString();
    press(dialog, "definitionRevert");

    // The same of a built-in shape: a style draws its points as "circle".
    katana::entity::Style rings;
    rings.name = "Rings";
    rings.symbol = "circle";
    ASSERT_TRUE(f.document.execute(katana::commands::createStyle(rings)).ok());
    ASSERT_TRUE(dialog.newDefinition(true));
    fill(dialog, "definitionName", QStringLiteral("circle"));
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n[\"circle\", 1]"));
    EXPECT_EQ(issues(dialog),
              QStringLiteral("Named already:\n"
                             "  style \"Rings\" draws it as its symbol\n"
                             "\"circle\" is a built-in shape, which draws the points that name "
                             "it now. Saved, this definition is drawn in its place."));
    press(dialog, "definitionRevert");

    // A name nothing else answers to: until it is saved, what names it IS
    // drawn plain or as a stand-in, and that is what is said.
    katana::entity::SurveyMap map = f.document.surveyMap();
    katana::entity::SurveyRule pipe;
    pipe.key = "PX*";
    pipe.linestyle = "TEST Pipe";
    ASSERT_TRUE(map.add(pipe).ok());
    f.document.setSurveyMap(std::move(map));
    ASSERT_TRUE(dialog.newDefinition(false));
    fill(dialog, "definitionName", QStringLiteral("TEST Pipe"));
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 2, 0]"));
    EXPECT_EQ(issues(dialog),
              QStringLiteral("Named already:\n"
                             "  rule #2 PX* (feature) names it as its linestyle\n"
                             "Until it is saved a line naming it is drawn plain."));
    press(dialog, "definitionRevert");

    // A copy does not begin under a name the drawing's linetypes hold either.
    katana::entity::Linetype second = fence;
    second.name = "TEST Kerb 2";
    ASSERT_TRUE(f.document.execute(katana::commands::createLinetype(second)).ok());
    ASSERT_TRUE(dialog.duplicateDefinition("TEST Kerb"));
    EXPECT_EQ(child<QLineEdit>(dialog, "definitionName")->text(), QStringLiteral("TEST Kerb 3"));
    press(dialog, "definitionRevert");

    // And a library linestyle the drawing also holds: deleted, what names it
    // is handed to the drawing's linetype, not left plain.
    katana::entity::Linetype kerbToo = fence;
    kerbToo.name = "TEST Kerb";
    ASSERT_TRUE(f.document.execute(katana::commands::createLinetype(kerbToo)).ok());
    ASSERT_TRUE(dialog.editDefinition("TEST Kerb"));
    press(dialog, "definitionDelete");
    EXPECT_EQ(issues(dialog),
              QStringLiteral("\"TEST Kerb\" was not deleted: the definition is used\n"
                             "It is named by:\n"
                             "  rule #0 WM* (feature) names it as its linestyle\n"
                             "Delete Anyway runs the line again with FORCE. A line naming it is "
                             "then drawn with the drawing's own linetype of that name."));
}

TEST(DefinitionEditor, ADefinitionWithNoStrokesIsSavedAndSaidToDrawNothing)
{
    // The format holds a definition with no strokes, so the editor saves one;
    // but it draws nothing, which is rarely what was meant, and is said.
    EditorFixture f;
    DefinitionEditorDialog dialog(f.context);
    ASSERT_TRUE(dialog.newDefinition(false));
    fill(dialog, "definitionName", QStringLiteral("TEST Gate"));
    EXPECT_EQ(issues(dialog), QStringLiteral("No strokes yet: a definition is drawn by its "
                                             "strokes, and this one has none."));
    EXPECT_FALSE(saysAnError(dialog));
    ASSERT_TRUE(enabled(dialog, "definitionSave"));
    press(dialog, "definitionSave");
    LineStyle expected;
    expected.name = "TEST Gate";
    EXPECT_TRUE(f.inLibrary("TEST Gate") == expected);
    // Still said of it once it is in the library.
    EXPECT_TRUE(issues(dialog).contains(QStringLiteral("No strokes yet")))
        << issues(dialog).toStdString();
    strokes(dialog, QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 2, 0]"));
    EXPECT_FALSE(issues(dialog).contains(QStringLiteral("No strokes yet")))
        << issues(dialog).toStdString();
}

// ---- where it is opened from -----------------------------------------------------

// A window with a Format menu and the workbench on it, as MainWindow builds
// them, over the customisation of the file comment.
struct Bench {
    EditorFixture f;
    QMainWindow window;
    QMenu* menu = new QMenu("Format", &window);
    QToolBar* bar = new QToolBar("Format", &window);
    bool headless = true;
    std::unique_ptr<katana::qt::CustomisationWorkbench> bench;

    Bench()
    {
        katana::qt::CustomisationServices services;
        services.document = &f.document;
        services.makeAction = [this](katana::qt::Icon icon, const QString& text, const QString&,
                                     const QKeySequence&, const QString& name) {
            auto* action = new QAction(katana::qt::icon(icon), text, &window);
            action->setObjectName(name);
            return action;
        };
        services.log = f.context.log;
        services.headless = [this] { return headless; };
        services.run = f.context.run;
        bench = std::make_unique<katana::qt::CustomisationWorkbench>(window, std::move(services),
                                                                     *menu, *bar);
    }

    void trigger(const char* action)
    {
        QAction* found = window.findChild<QAction*>(action);
        ASSERT_NE(found, nullptr) << action;
        found->trigger();
        katana::qt::test::processEvents();
    }
};

TEST(DefinitionEditor, TheSymbolLibrarysFourButtonsOpenTheOneEditorTheWorkbenchKeeps)
{
    Bench b;
    b.trigger("formatSymbols");
    katana::qt::SymbolLibraryDialog* symbols = b.bench->symbolLibrary();
    ASSERT_NE(symbols, nullptr);
    EXPECT_EQ(b.bench->definitionEditor(), nullptr) << "made when first asked for";

    // New needs no symbol; the other three a definition the library holds,
    // which a built-in shape is not.
    ASSERT_TRUE(symbols->selectSymbol("circle"));
    EXPECT_TRUE(enabled(*symbols, "symbolNew"));
    for (const char* name : {"symbolEdit", "symbolDuplicate", "symbolDelete"}) {
        EXPECT_FALSE(enabled(*symbols, name)) << name;
    }
    ASSERT_TRUE(symbols->selectSymbol("TEST Valve"));
    for (const char* name : {"symbolNew", "symbolEdit", "symbolDuplicate", "symbolDelete"}) {
        EXPECT_TRUE(enabled(*symbols, name)) << name;
    }

    press(*symbols, "symbolEdit");
    DefinitionEditorDialog* editor = b.bench->definitionEditor();
    ASSERT_NE(editor, nullptr);
    EXPECT_TRUE(editor->isVisible());
    EXPECT_FALSE(editor->isModal()) << "beside the library, not over it";
    EXPECT_EQ(editor->parentWidget(), &b.window);
    EXPECT_EQ(editor->editedName(), "TEST Valve");

    // The same editor each time, whichever button asked.
    press(*symbols, "symbolNew");
    EXPECT_EQ(b.bench->definitionEditor(), editor);
    EXPECT_EQ(child<QLabel>(*editor, "definitionHeading")->text(), QStringLiteral("New symbol"));
    press(*symbols, "symbolDuplicate");
    EXPECT_EQ(child<QLineEdit>(*editor, "definitionName")->text(),
              QStringLiteral("TEST Valve 2"));
    EXPECT_EQ(b.window.findChildren<QDialog*>(QStringLiteral("definitionEditorDialog")).size(), 1);

    // Delete: the definition shown, the line run through the window's
    // executor, and - rule #1 and the style Marks naming it - refused there.
    editor->close();
    press(*symbols, "symbolDelete");
    EXPECT_TRUE(editor->isVisible()) << "shown again";
    EXPECT_EQ(editor->editedName(), "TEST Valve");
    EXPECT_EQ(b.f.ran, (QStringList{QStringLiteral("CUSTOMISE REMOVE \"TEST Valve\"")}));
    EXPECT_TRUE(child<QPushButton>(*editor, "definitionDeleteAnyway")->isVisibleTo(editor));
    EXPECT_TRUE(b.f.inLibrary("TEST Valve") == valve());

    // A symbol library built on its own has no editor to ask.
    katana::qt::SymbolLibraryDialog alone(b.f.context);
    ASSERT_TRUE(alone.selectSymbol("TEST Valve"));
    for (const char* name : {"symbolNew", "symbolEdit", "symbolDuplicate", "symbolDelete"}) {
        EXPECT_FALSE(enabled(alone, name)) << name;
    }
}

TEST(DefinitionEditor, TheLinetypesTabOpensTheSameEditorOnALibraryLinestyle)
{
    Bench b;
    b.trigger("formatStyles");
    katana::qt::StyleManagerDialog* styles = b.bench->styleManager();
    ASSERT_NE(styles, nullptr);
    styles->selectLinetype("TEST Kerb", katana::cad::LinetypeOrigin::Library);
    katana::qt::test::processEvents();
    ASSERT_EQ(styles->selectedLinetypes(),
              (std::vector<std::pair<std::string, katana::cad::LinetypeOrigin>>{
                  {"TEST Kerb", katana::cad::LinetypeOrigin::Library}}));

    // The page says where a library linestyle IS edited, under the sentence
    // that it is not edited on the page.
    EXPECT_TRUE(child<QLabel>(*styles, "libraryDetails")
                    ->text()
                    .contains(QStringLiteral("are read-only here. Edit Definition opens this "
                                             "one in the definition editor.")))
        << child<QLabel>(*styles, "libraryDetails")->text().toStdString();
    QPushButton* edit = child<QPushButton>(*styles, "linetypeEditDefinition");
    ASSERT_NE(edit, nullptr);
    ASSERT_TRUE(edit->isEnabled());
    edit->click();
    DefinitionEditorDialog* editor = b.bench->definitionEditor();
    ASSERT_NE(editor, nullptr);
    EXPECT_TRUE(editor->isVisible());
    EXPECT_EQ(editor->editedName(), "TEST Kerb");
    EXPECT_EQ(child<QComboBox>(*editor, "definitionKind")->currentText(),
              QStringLiteral("Linestyle"));

    // And the symbol library's buttons reach that one, not a second.
    b.trigger("formatSymbols");
    ASSERT_TRUE(b.bench->symbolLibrary()->selectSymbol("TEST Valve"));
    press(*b.bench->symbolLibrary(), "symbolEdit");
    EXPECT_EQ(b.bench->definitionEditor(), editor);
    EXPECT_EQ(editor->editedName(), "TEST Valve");

    // A linestyle is BEGUN from this tab too, with no library linestyle to
    // select first: the button sits with the tab's own, not on the page of
    // one that exists.
    QPushButton* fresh = child<QPushButton>(*styles, "linetypeNewDefinition");
    ASSERT_NE(fresh, nullptr);
    ASSERT_TRUE(fresh->isEnabled());
    fresh->click();
    EXPECT_EQ(b.bench->definitionEditor(), editor);
    EXPECT_EQ(child<QLabel>(*editor, "definitionHeading")->text(),
              QStringLiteral("New linestyle"));
    EXPECT_EQ(child<QComboBox>(*editor, "definitionKind")->currentText(),
              QStringLiteral("Linestyle"));
    EXPECT_EQ(editor->editedName(), "");

    // A style manager built on its own has no editor to ask: both doors are
    // shut rather than pressed to no effect.
    katana::qt::StyleManagerDialog alone(b.f.context);
    alone.selectLinetype("TEST Kerb", katana::cad::LinetypeOrigin::Library);
    katana::qt::test::processEvents();
    EXPECT_FALSE(enabled(alone, "linetypeEditDefinition"));
    EXPECT_FALSE(enabled(alone, "linetypeNewDefinition"));
}

TEST(DefinitionEditor, TheEditorGoesWithTheWorkbenchBeforeTheDocument)
{
    Bench b;
    const QPointer<QDialog> editor(&b.bench->showDefinitionEditor());
    ASSERT_FALSE(editor.isNull());
    b.bench.reset();
    EXPECT_TRUE(editor.isNull());
}

TEST(DefinitionEditor, TheWindowIsNotClosedOverAFormThatIsNotSaved)
{
    Bench b;
    EXPECT_TRUE(b.bench->confirmClose());
    ASSERT_TRUE(b.bench->editDefinition(DefinitionEdit::Edit, "TEST Kerb"));
    DefinitionEditorDialog* editor = b.bench->definitionEditor();
    EXPECT_TRUE(b.bench->confirmClose()) << "open and untouched: nothing to lose";
    fill(*editor, "definitionLength", QStringLiteral("15"));

    // Headless: nobody to ask, so refused and said.
    b.f.logged.clear();
    EXPECT_FALSE(b.bench->confirmClose());
    ASSERT_EQ(b.f.logged.size(), 1u);
    EXPECT_TRUE(b.f.logged.front().first.startsWith(QStringLiteral("Unsaved Definition: ")));
    EXPECT_TRUE(b.f.logged.front().second);

    // Interactive: asked. No keeps the window and the edits.
    b.headless = false;
    QString asked;
    int questions = 0;
    bool answer = false;
    b.bench->confirm = [&](const QString& question) {
        asked = question;
        ++questions;
        return answer;
    };
    EXPECT_FALSE(b.bench->confirmClose());
    EXPECT_EQ(asked,
              QStringLiteral("The definition editor has edits that are not saved. Discard them?"));
    EXPECT_EQ(questions, 1);
    EXPECT_TRUE(editor->dirty());

    // Yes lets the close go on - and discards NOTHING yet. The window asks
    // more after this (the code manager's rules, the unsaved drawing), and a
    // Cancel there keeps the window open: what was typed must still be in
    // the form then. The edits go when the window does.
    answer = true;
    EXPECT_TRUE(b.bench->confirmClose());
    EXPECT_EQ(questions, 2);
    EXPECT_TRUE(editor->dirty());
    EXPECT_EQ(child<QLineEdit>(*editor, "definitionLength")->text(), QStringLiteral("15"));
    EXPECT_TRUE(b.f.inLibrary("TEST Kerb") == kerb());

    // The answer is remembered for the form as it stands: the close asked
    // for again after that Cancel does not ask this again.
    EXPECT_TRUE(b.bench->confirmClose());
    EXPECT_EQ(questions, 2);
    // More typed is more to lose, and is asked about.
    fill(*editor, "definitionLength", QStringLiteral("16"));
    answer = false;
    EXPECT_FALSE(b.bench->confirmClose());
    EXPECT_EQ(questions, 3);
    EXPECT_EQ(child<QLineEdit>(*editor, "definitionLength")->text(), QStringLiteral("16"));
    // Saved, there is nothing to ask.
    press(*editor, "definitionSave");
    EXPECT_TRUE(b.bench->confirmClose());
    EXPECT_EQ(questions, 3);
}

} // namespace
