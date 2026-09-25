// The sheet editor's Generate Sheets dialog (src/katana_qt/sheet_editor.cpp,
// docs/plotting.md "The sheet editor"): every layout and every option
// GENERATE takes, each control by its object name. OK builds the GENERATE
// line - shown in the dialog before it is run - and runs it through the
// editor (SheetEditor::runLine), so each choice gives exactly the sheets the
// typed line gives. Each case writes the line out by hand from the choices,
// checks the dialog made that line, and lays the same drawing out again from
// the typed line in a second document, with the window's context
// (sheetVerbContextFor), to compare the sets.

#include <gtest/gtest.h>

#include <functional>
#include <string>
#include <vector>

#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QDoubleSpinBox>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QSpinBox>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/commands/entity_commands.hpp"
#include "sheet_editor.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::qt::SheetEditor;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;
namespace cmd = katana::commands;

namespace {

using Drawing = std::function<void(Document&)>;

SheetEditor::SourceProvider sourceOf(const Document& document)
{
    return [&document] {
        SheetSource source;
        source.plan = katana::qt::planSourceOf(document);
        source.document = &document;
        source.revision = document.modelRevision();
        return source;
    };
}

// A road east from (0, 0) to (1000, 0), and a site beside it.
void roadAndSite(Document& document)
{
    katana::entity::Alignment alignment;
    alignment.name = "ROAD";
    alignment.horizontal.pis = {{Point2(0.0, 0.0)}, {Point2(1000.0, 0.0)}};
    ASSERT_TRUE(document.execute(cmd::createAlignment(alignment)).ok());
    ASSERT_TRUE(document.execute(cmd::createLine(Point2(0.0, 20.0), Point2(400.0, 220.0))).ok());
}

// The same drawing laid out from the typed line, in a document of its own,
// given what the window gives its command line.
plotting::SheetSet typed(const Drawing& draw, const std::string& line)
{
    Document twin;
    draw(twin);
    katana::cad::CommandInterpreter interpreter(twin);
    interpreter.setSheetContext(
        [&twin] { return katana::qt::sheetVerbContextFor(twin, sourceOf(twin)); });
    const auto reply = interpreter.run(line);
    EXPECT_TRUE(reply.ok()) << line << "\n  -> " << (reply.ok() ? "" : reply.error().describe());
    return twin.sheetSet();
}

struct Shown {
    explicit Shown(Document& document) : editor(document, sourceOf(document))
    {
        editor.onMessage = [this](const QString& text, bool error) {
            messages.push_back(text);
            errors += error ? 1 : 0;
        };
        editor.resize(1400, 900);
        editor.show();
        katana::qt::test::processEvents();
    }

    // The dialog, opened as the toolbar opens it.
    QDialog* open()
    {
        dialog = editor.openGenerateDialog();
        katana::qt::test::processEvents();
        return dialog;
    }
    template <typename Widget> Widget* field(const char* name) const
    {
        auto* widget = dialog->findChild<Widget*>(QString::fromLatin1(name));
        EXPECT_NE(widget, nullptr) << name;
        return widget;
    }
    void choose(const char* name, const QString& text) const
    {
        auto* combo = field<QComboBox>(name);
        ASSERT_NE(combo, nullptr);
        if (combo->isEditable()) {
            combo->setCurrentText(text);
            return;
        }
        const int at = combo->findText(text);
        ASSERT_GE(at, 0) << name << " offers no " << text.toStdString();
        combo->setCurrentIndex(at);
    }
    void tick(const char* name, bool on) const { field<QCheckBox>(name)->setChecked(on); }
    [[nodiscard]] QString line() const { return field<QLabel>("sheetGenerateLine")->text(); }
    [[nodiscard]] bool canGenerate() const { return field<QPushButton>("sheetGenerateOk")->isEnabled(); }
    // OK, as a person clicks it; the dialog closes and goes.
    void generate()
    {
        field<QPushButton>("sheetGenerateOk")->click();
        katana::qt::test::processEvents();
        dialog = nullptr;
    }

    SheetEditor editor;
    QDialog* dialog = nullptr;
    std::vector<QString> messages;
    int errors = 0;
};

// Lays `draw` out through the dialog with `choices`, checks the line it made
// is `expected` and was run, and that the typed line gives the same sheets.
void expectSameAsTyped(const Drawing& draw, const std::function<void(Shown&)>& choices,
                       const std::string& expected)
{
    Document document;
    draw(document);
    Shown shown(document);
    ASSERT_NE(shown.open(), nullptr);
    choices(shown);
    EXPECT_EQ(shown.line().toStdString(), expected);
    ASSERT_TRUE(shown.canGenerate());
    const std::size_t steps = document.history().undoCount();
    shown.generate();
    EXPECT_EQ(shown.errors, 0) << (shown.messages.empty() ? "" : shown.messages.back().toStdString());
    ASSERT_FALSE(shown.messages.empty());
    EXPECT_EQ(shown.messages.front().toStdString(), "> " + expected);
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_FALSE(document.sheetSet().sheets.empty());
    EXPECT_EQ(document.sheetSet(), typed(draw, expected));
}

} // namespace

TEST(GenerateDialog, EveryControlHasAnObjectName)
{
    Document document;
    roadAndSite(document);
    Shown shown(document);
    QDialog* dialog = shown.open();
    ASSERT_NE(dialog, nullptr);
    EXPECT_EQ(dialog->objectName(), QStringLiteral("sheetGenerateDialog"));
    for (const char* name :
         {"sheetGenerateLayout", "sheetGeneratePaper", "sheetGenerateOrientation", "sheetGenerateFrame",
          "sheetGenerateArea", "sheetGenerateAreaX0", "sheetGenerateAreaY0", "sheetGenerateAreaX1",
          "sheetGenerateAreaY1", "sheetGenerateScale", "sheetGenerateAlignment", "sheetGenerateFrom",
          "sheetGenerateTo", "sheetGenerateOverlap", "sheetGenerateKeyPlan", "sheetGenerateNoSections",
          "sheetGenerateEvery", "sheetGenerateInterval", "sheetGenerateAtStations",
          "sheetGenerateStations", "sheetGenerateHalfWidth", "sheetGenerateRows",
          "sheetGenerateColumns", "sheetGenerateVe", "sheetGenerateModel3d", "sheetGenerateLegend",
          "sheetGenerateRotate", "sheetGenerateReplace", "sheetGenerateLine", "sheetGenerateOk",
          "sheetGenerateCancel"}) {
        EXPECT_NE(dialog->findChild<QWidget*>(QString::fromLatin1(name)), nullptr) << name;
    }
    // The dialog opens on a fit of the drawing, and says so before it is run.
    EXPECT_EQ(shown.line(), QStringLiteral("GENERATE fit paper=A3 landscape frame=on scale=auto "
                                           "model3d=off legend=on"));
    dialog->reject();
}

TEST(GenerateDialog, StripsAlongARangeOnPortraitPaperAreTheTypedLinesStrips)
{
    expectSameAsTyped(
        roadAndSite,
        [](Shown& shown) {
            shown.choose("sheetGenerateLayout", QStringLiteral("Plan strips along an alignment"));
            // A strip follows its alignment; the first is chosen for it.
            EXPECT_EQ(shown.field<QComboBox>("sheetGenerateAlignment")->currentText(), QStringLiteral("ROAD"));
            // At an automatic scale the road goes on one sheet: no range, no overlap.
            EXPECT_FALSE(shown.field<QLineEdit>("sheetGenerateFrom")->isEnabled());
            EXPECT_FALSE(shown.field<QDoubleSpinBox>("sheetGenerateOverlap")->isEnabled());
            shown.choose("sheetGenerateScale", QStringLiteral("1:500"));
            EXPECT_TRUE(shown.field<QLineEdit>("sheetGenerateFrom")->isEnabled());
            shown.field<QLineEdit>("sheetGenerateFrom")->setText(QStringLiteral("100"));
            shown.field<QLineEdit>("sheetGenerateTo")->setText(QStringLiteral("900"));
            shown.choose("sheetGeneratePaper", QStringLiteral("A1"));
            shown.choose("sheetGenerateOrientation", QStringLiteral("Portrait"));
            shown.tick("sheetGenerateFrame", false);
            shown.tick("sheetGenerateReplace", true);
            EXPECT_FALSE(shown.field<QCheckBox>("sheetGenerateRotate")->isEnabled());
        },
        "GENERATE strips paper=A1 portrait frame=off alignment=\"ROAD\" scale=500 overlap=10 "
        "keyplan=on from=100 to=900 replace=on");
}

TEST(GenerateDialog, AFitOfAWindowWithASnapshotIsTheTypedLinesFit)
{
    expectSameAsTyped(
        roadAndSite,
        [](Shown& shown) {
            shown.choose("sheetGenerateArea", QStringLiteral("A window"));
            EXPECT_TRUE(shown.field<QDoubleSpinBox>("sheetGenerateAreaX0")->isEnabled());
            shown.field<QDoubleSpinBox>("sheetGenerateAreaX0")->setValue(0.0);
            shown.field<QDoubleSpinBox>("sheetGenerateAreaY0")->setValue(0.0);
            shown.field<QDoubleSpinBox>("sheetGenerateAreaX1")->setValue(200.0);
            shown.field<QDoubleSpinBox>("sheetGenerateAreaY1")->setValue(100.0);
            shown.tick("sheetGenerateModel3d", true);
            shown.tick("sheetGenerateLegend", false);
        },
        "GENERATE fit paper=A3 landscape frame=on area=0,0,200,100 scale=auto model3d=on "
        "legend=off");
}

TEST(GenerateDialog, TheCurrentPlanViewIsOfferedOnlyWhenThereIsOne)
{
    Document document;
    roadAndSite(document);
    Shown shown(document);
    shown.open();
    EXPECT_EQ(shown.field<QComboBox>("sheetGenerateArea")->findText(QStringLiteral("The current plan view")), -1);
    shown.dialog->reject();

    shown.editor.planViewArea = [] { return Box2(Point2(-50.0, -25.0), Point2(450.0, 275.5)); };
    shown.open();
    shown.choose("sheetGenerateArea", QStringLiteral("The current plan view"));
    shown.choose("sheetGenerateLayout", QStringLiteral("Tiles over the drawing, with a key plan"));
    shown.choose("sheetGenerateScale", QStringLiteral("1:500"));
    shown.field<QDoubleSpinBox>("sheetGenerateOverlap")->setValue(20.0);
    shown.tick("sheetGenerateKeyPlan", false);
    const std::string expected =
        "GENERATE grid paper=A3 landscape frame=on area=-50,-25,450,275.5 scale=500 overlap=20 "
        "keyplan=off";
    EXPECT_EQ(shown.line().toStdString(), expected);
    shown.generate();
    EXPECT_EQ(shown.errors, 0);
    EXPECT_EQ(document.sheetSet(), typed(roadAndSite, expected));
}

TEST(GenerateDialog, CrossSectionsAtChainagesWithAnExaggerationAreTheTypedLines)
{
    expectSameAsTyped(
        roadAndSite,
        [](Shown& shown) {
            shown.choose("sheetGenerateLayout", QStringLiteral("Cross sections along an alignment"));
            // Sections are cut every interval or at chainages, never none.
            EXPECT_FALSE(shown.field<QRadioButton>("sheetGenerateNoSections")->isEnabled());
            EXPECT_TRUE(shown.field<QRadioButton>("sheetGenerateEvery")->isChecked());
            shown.field<QRadioButton>("sheetGenerateAtStations")->setChecked(true);
            EXPECT_FALSE(shown.field<QDoubleSpinBox>("sheetGenerateInterval")->isEnabled());
            shown.field<QLineEdit>("sheetGenerateStations")->setText(QStringLiteral("100, 500 900"));
            shown.choose("sheetGenerateVe", QStringLiteral("5 x"));
            shown.field<QSpinBox>("sheetGenerateRows")->setValue(2);
            shown.field<QSpinBox>("sheetGenerateColumns")->setValue(1);
        },
        "GENERATE sections paper=A3 landscape frame=on alignment=\"ROAD\" scale=auto "
        "stations=100,500,900 halfwidth=20 rows=2 columns=1 ve=5");
}

TEST(GenerateDialog, APlanAndProfileWithSectionsIsTheTypedLinesPlanAndProfile)
{
    expectSameAsTyped(
        roadAndSite,
        [](Shown& shown) {
            shown.choose("sheetGenerateLayout", QStringLiteral("Plan and profile along an alignment"));
            // A profile's plan may have sections after it, but not at a list.
            EXPECT_FALSE(shown.field<QRadioButton>("sheetGenerateAtStations")->isEnabled());
            shown.field<QRadioButton>("sheetGenerateEvery")->setChecked(true);
            shown.field<QDoubleSpinBox>("sheetGenerateInterval")->setValue(50.0);
            shown.field<QDoubleSpinBox>("sheetGenerateHalfWidth")->setValue(15.0);
        },
        "GENERATE profile paper=A3 landscape frame=on alignment=\"ROAD\" scale=auto interval=50 "
        "halfwidth=15 model3d=off legend=on");
}

TEST(GenerateDialog, TheRegisterAndThePlotFramesTakeOnlyWhatTheirVerbsTake)
{
    Document document;
    roadAndSite(document);
    Shown shown(document);
    shown.open();
    shown.choose("sheetGenerateLayout", QStringLiteral("One sheet per imported plot frame"));
    EXPECT_FALSE(shown.field<QComboBox>("sheetGeneratePaper")->isEnabled());
    EXPECT_FALSE(shown.field<QComboBox>("sheetGenerateScale")->isEnabled());
    EXPECT_EQ(shown.line(), QStringLiteral("GENERATE frames frame=on"));

    shown.choose("sheetGenerateLayout", QStringLiteral("Drawing register (cover sheet)"));
    // A register goes in front of the sheets: it replaces none.
    EXPECT_FALSE(shown.field<QCheckBox>("sheetGenerateReplace")->isEnabled());
    shown.choose("sheetGenerateOrientation", QStringLiteral("Portrait"));
    shown.tick("sheetGenerateFrame", false);
    const std::string expected = "GENERATE register paper=A3 portrait frame=off";
    EXPECT_EQ(shown.line().toStdString(), expected);
    shown.generate();
    EXPECT_EQ(shown.errors, 0);
    ASSERT_EQ(document.sheetSet().sheets.size(), 1u);
    EXPECT_FALSE(document.sheetSet().sheets[0].landscape);
    EXPECT_EQ(document.sheetSet(), typed(roadAndSite, expected));
}

TEST(GenerateDialog, ALineThatCannotBeTypedCannotBeGenerated)
{
    Document document;
    roadAndSite(document);
    Shown shown(document);
    shown.open();
    shown.choose("sheetGenerateLayout", QStringLiteral("Cross sections along an alignment"));
    shown.field<QRadioButton>("sheetGenerateAtStations")->setChecked(true);
    shown.field<QLineEdit>("sheetGenerateStations")->setText(QStringLiteral("100, soon"));
    EXPECT_FALSE(shown.canGenerate());
    EXPECT_EQ(shown.line(), QStringLiteral("list the chainages to cut, separated by commas"));
    shown.field<QLineEdit>("sheetGenerateStations")->setText(QString());
    EXPECT_FALSE(shown.canGenerate());

    // A window with no breadth is no area.
    shown.choose("sheetGenerateLayout", QStringLiteral("Tiles over the drawing, with a key plan"));
    shown.choose("sheetGenerateArea", QStringLiteral("A window"));
    shown.field<QDoubleSpinBox>("sheetGenerateAreaX0")->setValue(10.0);
    shown.field<QDoubleSpinBox>("sheetGenerateAreaX1")->setValue(10.0);
    EXPECT_FALSE(shown.canGenerate());
    EXPECT_EQ(shown.line(), QStringLiteral("the window needs a width and a height"));

    // A strip's range that is not a number.
    shown.choose("sheetGenerateLayout", QStringLiteral("Plan strips along an alignment"));
    shown.choose("sheetGenerateScale", QStringLiteral("1:500"));
    shown.field<QLineEdit>("sheetGenerateFrom")->setText(QStringLiteral("start"));
    EXPECT_FALSE(shown.canGenerate());
    shown.dialog->reject();
    EXPECT_TRUE(document.sheetSet().sheets.empty());
}

TEST(GenerateDialog, RotateIsOfferedOnlyForAPlanOfAnArea)
{
    Document document;
    roadAndSite(document);
    Shown shown(document);
    shown.open();
    auto* rotate = shown.field<QCheckBox>("sheetGenerateRotate");
    EXPECT_TRUE(rotate->isEnabled());
    rotate->setChecked(true);
    EXPECT_TRUE(shown.line().endsWith(QStringLiteral(" rotate=on")));
    // Along the alignment, or with sections after the plan, it is not turned.
    shown.choose("sheetGenerateAlignment", QStringLiteral("ROAD"));
    EXPECT_FALSE(rotate->isEnabled());
    EXPECT_FALSE(shown.line().contains(QStringLiteral("rotate")));
    EXPECT_FALSE(shown.field<QComboBox>("sheetGenerateArea")->isEnabled());
    shown.choose("sheetGenerateAlignment", QStringLiteral("(none)"));
    shown.field<QRadioButton>("sheetGenerateEvery")->setChecked(true);
    EXPECT_FALSE(rotate->isEnabled());
    shown.dialog->reject();
}

TEST(GenerateDialog, TheLineIsHandedToTheWindowsExecutor)
{
    Document document;
    roadAndSite(document);
    Shown shown(document);
    std::vector<QString> lines;
    shown.editor.setCommandRunner([&lines](const QString& line) {
        lines.push_back(line);
        return katana::qt::VerbOutcome{true, QStringLiteral("generated 1 sheet: 1"), {}};
    });
    shown.open();
    shown.choose("sheetGenerateScale", QStringLiteral("1:1000"));
    shown.generate();
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_EQ(lines.front(),
              QStringLiteral("GENERATE fit paper=A3 landscape frame=on scale=1000 model3d=off legend=on"));
    EXPECT_TRUE(document.sheetSet().sheets.empty()) << "the runner did the work, and here did none";
    EXPECT_TRUE(shown.messages.empty()) << "the window logs the line itself";
}
