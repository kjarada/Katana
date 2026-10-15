// Terrain > Volumes (src/katana_qt/terrain/volumes_dialog.hpp): each page's
// line, built from its fields as a person fills them - found by object name -
// and handed to the executor exactly as shown; and, with the interpreter
// itself as the executor, the records it answers in the table and the mass
// haul diagram, and the cut/fill map made as a surface of the drawing.

#include <gtest/gtest.h>

#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QCoreApplication>
#include <QDoubleSpinBox>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QScreen>
#include <QScrollArea>
#include <QStringList>
#include <QTableWidget>
#include <QTabWidget>
#include <QGuiApplication>

#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <memory>
#include <string>
#include <utility>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_builder.hpp"
#include "screen_fit.hpp"
#include "terrain/volumes_dialog.hpp"

namespace {

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::geometry::Point3;
using katana::qt::VerbOutcome;
using katana::qt::VolumeMethod;
using katana::qt::VolumesDialog;
using katana::qt::VolumesDialogContext;
using katana::terrain::TinInput;
using katana::terrain::TinSurface;

template <class T>
T* child(QWidget& dialog, const char* name)
{
    auto* found = dialog.findChild<T*>(name);
    EXPECT_NE(found, nullptr) << name;
    return found;
}

TinSurface surfaceOf(std::vector<Point3> points)
{
    TinInput input;
    input.points = std::move(points);
    auto built = katana::terrain::buildTin(input);
    EXPECT_TRUE(built.ok()) << built.error().describe();
    return built.ok() ? std::move(built->surface) : TinSurface{};
}

void addSurface(Document& document, const std::string& name, TinSurface surface)
{
    katana::terrain::SurfaceStore store = document.surfaces();
    katana::terrain::NamedSurface named;
    named.name = name;
    named.surface = std::make_shared<const TinSurface>(std::move(surface));
    named.source = "test";
    ASSERT_TRUE(store.add(std::move(named)).ok());
    ASSERT_TRUE(document.setSurfaces(std::move(store), "TEST").ok());
}

// The wedge of docs/volumes.md: the ground z = 0 under the triangle (0,0),
// (100,-10), (100,10) of z = 0.1 x, and the alignment cl along it. A section
// at s holds s^2 / 50: 6700 by end areas every 10 m.
void addWedge(Document& document, CommandInterpreter& interpreter)
{
    addSurface(document, "ground",
               surfaceOf({Point3(-20, -60, 0), Point3(120, -60, 0), Point3(120, 80, 0),
                          Point3(-20, 80, 0)}));
    addSurface(document, "design",
               surfaceOf({Point3(0, 0, 0), Point3(100, -10, 10), Point3(100, 10, 10)}));
    ASSERT_TRUE(interpreter.run("ALIGN NEW cl 0,0 100,0").ok());
}

// The executor a test hands the dialog: a record of the lines, answered by
// the interpreter on the test's document - the verbs the window runs.
struct Runner {
    Document& document;
    CommandInterpreter& interpreter;
    QStringList lines;

    VolumesDialogContext context()
    {
        VolumesDialogContext made;
        made.document = &document;
        made.headless = [] { return true; };
        made.run = [this](const QString& line) {
            lines << line;
            const auto reply = interpreter.run(line.toStdString());
            if (!reply.ok()) {
                return VerbOutcome{false, {}, QString::fromStdString(reply.error().describe())};
            }
            return VerbOutcome{true, QString::fromStdString(*reply), {}};
        };
        return made;
    }
};

} // namespace

TEST(VolumesDialog, EachPageSaysTheLineItsFieldsMake)
{
    Document document;
    CommandInterpreter interpreter(document);
    addWedge(document, interpreter);
    Runner runner{document, interpreter, {}};
    VolumesDialog dialog(runner.context());
    auto* command = child<QLineEdit>(dialog, "volumesCommand");

    // Surface to Surface: the base and the top, and the top starts on the
    // second surface, so the two are not one surface against itself.
    dialog.setMethod(VolumeMethod::Surfaces);
    EXPECT_EQ(command->text(), "VOLUME SURFACES ground design");
    child<QRadioButton>(dialog, "volumesBoundaryCorners")->click();
    child<QLineEdit>(dialog, "volumesCorners")->setText("0,0 10,0 10,10");
    child<QCheckBox>(dialog, "volumesEach")->setChecked(true);
    EXPECT_EQ(command->text(), "VOLUME SURFACES ground design INSIDE 0,0 10,0 10,10 EACH");
    child<QRadioButton>(dialog, "volumesBoundaryScope")->click();
    child<QRadioButton>(dialog, "volumesScopeDrawing")->click();
    EXPECT_EQ(command->text(), "VOLUME SURFACES ground design INSIDE FROM DRAWING EACH");
    child<QDoubleSpinBox>(dialog, "volumesBulk")->setValue(1.25);
    EXPECT_EQ(command->text(), "VOLUME SURFACES ground design INSIDE FROM DRAWING EACH bulk=1.25");
    child<QDoubleSpinBox>(dialog, "volumesBulk")->setValue(1.0);
    child<QRadioButton>(dialog, "volumesBoundaryNone")->click();

    dialog.setMethod(VolumeMethod::Level);
    child<QDoubleSpinBox>(dialog, "volumesLevel")->setValue(12.5);
    EXPECT_EQ(command->text(), "VOLUME DATUM ground 12.5");

    dialog.setMethod(VolumeMethod::Stockpile);
    child<QRadioButton>(dialog, "volumesBoundaryCorners")->click();
    child<QCheckBox>(dialog, "volumesEach")->setChecked(false);
    auto* base = child<QComboBox>(dialog, "volumesStockpileBase");
    base->setCurrentIndex(base->findData(QStringLiteral("lowest")));
    EXPECT_EQ(command->text(), "VOLUME STOCKPILE ground INSIDE 0,0 10,0 10,10 base=lowest");
    base->setCurrentIndex(base->findData(QStringLiteral("level")));
    child<QDoubleSpinBox>(dialog, "volumesStockpileLevel")->setValue(-2.0);
    EXPECT_EQ(command->text(), "VOLUME STOCKPILE ground INSIDE 0,0 10,0 10,10 level=-2");

    dialog.setMethod(VolumeMethod::Sections);
    EXPECT_EQ(command->text(), "VOLUME SECTIONS ground design ALIGNMENT cl interval=10");
    child<QLineEdit>(dialog, "volumesLeft")->setText("20");
    child<QLineEdit>(dialog, "volumesRight")->setText("20");
    auto* method = child<QComboBox>(dialog, "volumesSectionMethod");
    method->setCurrentIndex(method->findData(QStringLiteral("both")));
    EXPECT_EQ(command->text(),
              "VOLUME SECTIONS ground design ALIGNMENT cl interval=10 left=20 right=20 method=both");

    dialog.setMethod(VolumeMethod::Grid);
    child<QDoubleSpinBox>(dialog, "volumesCell")->setValue(2.0);
    EXPECT_EQ(command->text(), "VOLUME GRID ground design cell=2 INSIDE 0,0 10,0 10,10");
    EXPECT_TRUE(runner.lines.isEmpty()); // building a line runs nothing
}

TEST(VolumesDialog, TheCurvatureBoxIsOnByDefaultSaysNothingThenAndCurvatureOffWhenUnchecked)
{
    // The correction is the verb's default, so the line only names the plain
    // figure; the box is on the Along Alignment page alone, and the line it makes
    // is the one handed to the executor.
    Document document;
    CommandInterpreter interpreter(document);
    addWedge(document, interpreter);
    Runner runner{document, interpreter, {}};
    VolumesDialog dialog(runner.context());
    auto* command = child<QLineEdit>(dialog, "volumesCommand");
    auto* curvature = child<QCheckBox>(dialog, "volumesCurvature");
    dialog.setMethod(VolumeMethod::Sections);
    EXPECT_TRUE(curvature->isChecked());
    EXPECT_EQ(command->text(), "VOLUME SECTIONS ground design ALIGNMENT cl interval=10");
    curvature->setChecked(false);
    EXPECT_EQ(command->text(), "VOLUME SECTIONS ground design ALIGNMENT cl interval=10 curvature=off");
    auto* method = child<QComboBox>(dialog, "volumesSectionMethod");
    method->setCurrentIndex(method->findData(QStringLiteral("both")));
    EXPECT_EQ(command->text(),
              "VOLUME SECTIONS ground design ALIGNMENT cl interval=10 method=both curvature=off");
    method->setCurrentIndex(method->findData(QStringLiteral("endarea")));
    // The other pages take no curvature=, and say nothing of it.
    dialog.setMethod(VolumeMethod::Surfaces);
    EXPECT_EQ(command->text(), "VOLUME SURFACES ground design");
    dialog.setMethod(VolumeMethod::Sections);

    // Measure runs the line as shown, and the reply's volume record says the
    // figure is the plain one.
    child<QLineEdit>(dialog, "volumesLeft")->setText("20");
    child<QLineEdit>(dialog, "volumesRight")->setText("20");
    child<QPushButton>(dialog, "volumesRun")->click();
    ASSERT_EQ(runner.lines.size(), 1);
    EXPECT_EQ(runner.lines.front(),
              "VOLUME SECTIONS ground design ALIGNMENT cl interval=10 left=20 right=20 "
              "curvature=off");
    EXPECT_TRUE(dialog.resultsText().contains(QStringLiteral("\toff\t")));
    // Checked again, the word goes.
    curvature->setChecked(true);
    EXPECT_EQ(command->text(),
              "VOLUME SECTIONS ground design ALIGNMENT cl interval=10 left=20 right=20");
}

TEST(VolumesDialog, AStockpileWithNoBoundaryIsRefusedNamingWhatIsMissingAndRunsNothing)
{
    Document document;
    CommandInterpreter interpreter(document);
    addWedge(document, interpreter);
    Runner runner{document, interpreter, {}};
    VolumesDialog dialog(runner.context());
    dialog.setMethod(VolumeMethod::Stockpile);
    child<QRadioButton>(dialog, "volumesBoundaryNone")->click();
    EXPECT_TRUE(child<QLineEdit>(dialog, "volumesCommand")->text().isEmpty());
    child<QPushButton>(dialog, "volumesRun")->click();
    EXPECT_TRUE(runner.lines.isEmpty());
    EXPECT_TRUE(child<QLabel>(dialog, "volumesStatus")->text().contains("boundary"));
}

TEST(VolumesDialog, MeasuringAlongAnAlignmentFillsTheTableAndTheMassHaulDiagram)
{
    Document document;
    CommandInterpreter interpreter(document);
    addWedge(document, interpreter);
    Runner runner{document, interpreter, {}};
    VolumesDialog dialog(runner.context());
    dialog.setMethod(VolumeMethod::Sections);
    child<QLineEdit>(dialog, "volumesLeft")->setText("20");
    child<QLineEdit>(dialog, "volumesRight")->setText("20");
    child<QPushButton>(dialog, "volumesRun")->click();
    ASSERT_EQ(runner.lines.size(), 1);
    EXPECT_EQ(runner.lines.front(), child<QLineEdit>(dialog, "volumesCommand")->text());

    // Eleven sections and the end-area total.
    auto* table = child<QTableWidget>(dialog, "volumesResults");
    EXPECT_EQ(table->rowCount(), 12);
    // The diagram: every section's ordinate, falling to -6700 (all fill).
    const auto& points = dialog.chart().points();
    ASSERT_EQ(points.size(), 11u);
    EXPECT_DOUBLE_EQ(points.front().x(), 0.0);
    EXPECT_DOUBLE_EQ(points.back().x(), 100.0);
    EXPECT_DOUBLE_EQ(points.back().y(), -6700.0);
    EXPECT_TRUE(child<QLabel>(dialog, "volumesStatus")->text().contains("6700.000"));
    // Copy's text is the table, its header first.
    EXPECT_TRUE(dialog.resultsText().startsWith("record\tstation"));
}

TEST(VolumesDialog, MakeCutFillMapAddsTheDifferenceAndOffersItAtOnce)
{
    Document document;
    CommandInterpreter interpreter(document);
    addWedge(document, interpreter);
    Runner runner{document, interpreter, {}};
    VolumesDialog dialog(runner.context());
    dialog.setMethod(VolumeMethod::Surfaces);
    child<QLineEdit>(dialog, "volumesMapName")->setText("cut fill");
    child<QPushButton>(dialog, "volumesCutFillMap")->click();
    ASSERT_EQ(runner.lines.size(), 1);
    EXPECT_EQ(runner.lines.front(), "SURFACE DIFFERENCE ground design NAME \"cut fill\"");
    ASSERT_NE(document.surfaces().find("cut fill"), nullptr);
    EXPECT_GE(child<QComboBox>(dialog, "volumesBase")->findText("cut fill"), 0);
}

TEST(VolumesDialog, MakeEnvelopeRunsSurfaceEnvelopeOfThePagesTwoSurfacesAndOffersTheResultAtOnce)
{
    Document document;
    CommandInterpreter interpreter(document);
    addWedge(document, interpreter);
    Runner runner{document, interpreter, {}};
    VolumesDialog dialog(runner.context());
    auto* side = child<QComboBox>(dialog, "volumesEnvelopeSide");
    auto* name = child<QLineEdit>(dialog, "volumesEnvelopeName");
    auto* button = child<QPushButton>(dialog, "volumesEnvelope");
    ASSERT_NE(side, nullptr);

    // The line the fields make: the page's two surfaces, MAX by default.
    dialog.setMethod(VolumeMethod::Surfaces);
    const auto upper = dialog.envelopeLine();
    ASSERT_TRUE(upper.ok()) << upper.error().describe();
    EXPECT_EQ(*upper, "SURFACE ENVELOPE ground design MAX");
    side->setCurrentIndex(side->findData(QStringLiteral("MIN")));
    name->setText("lower level");
    const auto lower = dialog.envelopeLine();
    ASSERT_TRUE(lower.ok());
    EXPECT_EQ(*lower, "SURFACE ENVELOPE ground design MIN NAME \"lower level\"");
    // The other pages use their own two surfaces, as the cut/fill map does.
    dialog.setMethod(VolumeMethod::Grid);
    EXPECT_EQ(*dialog.envelopeLine(), "SURFACE ENVELOPE ground design MIN NAME \"lower level\"");
    dialog.setMethod(VolumeMethod::Sections);
    EXPECT_EQ(*dialog.envelopeLine(), "SURFACE ENVELOPE ground design MIN NAME \"lower level\"");
    dialog.setMethod(VolumeMethod::Surfaces);
    EXPECT_TRUE(runner.lines.isEmpty()); // building a line runs nothing

    // The button hands exactly that line to the executor; the surface is in the
    // drawing in the default style and the lists offer it at once.
    button->click();
    ASSERT_EQ(runner.lines.size(), 1);
    EXPECT_EQ(runner.lines.front(), "SURFACE ENVELOPE ground design MIN NAME \"lower level\"");
    const katana::terrain::NamedSurface* made = document.surfaces().find("lower level");
    ASSERT_NE(made, nullptr);
    EXPECT_TRUE(made->style.empty());
    EXPECT_GE(child<QComboBox>(dialog, "volumesBase")->findText("lower level"), 0);
    EXPECT_TRUE(child<QLabel>(dialog, "volumesStatus")->text().contains("Made the envelope"));
    EXPECT_TRUE(child<QLabel>(dialog, "volumesStatus")->text().contains("lower"));

    // Without a name the verb's own: ground-design-max.
    side->setCurrentIndex(side->findData(QStringLiteral("MAX")));
    name->clear();
    button->click();
    ASSERT_EQ(runner.lines.size(), 2);
    EXPECT_EQ(runner.lines.back(), "SURFACE ENVELOPE ground design MAX");
    EXPECT_NE(document.surfaces().find("ground-design-max"), nullptr);
}

TEST(VolumesDialog, MakeEnvelopeWithNoSurfaceOrARefusedNameSaysWhyAndChangesNothing)
{
    // No surfaces: a line cannot be made, naming what is missing, and nothing runs.
    {
        Document document;
        CommandInterpreter interpreter(document);
        Runner runner{document, interpreter, {}};
        VolumesDialog dialog(runner.context());
        const auto none = dialog.envelopeLine();
        ASSERT_FALSE(none.ok());
        EXPECT_NE(none.error().message.find("no surface"), std::string::npos);
        child<QPushButton>(dialog, "volumesEnvelope")->click();
        EXPECT_TRUE(runner.lines.isEmpty());
        EXPECT_TRUE(child<QLabel>(dialog, "volumesStatus")->text().contains("no surface"));
    }
    // The verb's refusal reaches the status and the drawing is as it was: a name
    // the drawing already has.
    Document document;
    CommandInterpreter interpreter(document);
    addWedge(document, interpreter);
    Runner runner{document, interpreter, {}};
    VolumesDialog dialog(runner.context());
    child<QLineEdit>(dialog, "volumesEnvelopeName")->setText("ground");
    child<QPushButton>(dialog, "volumesEnvelope")->click();
    ASSERT_EQ(runner.lines.size(), 1);
    EXPECT_TRUE(child<QLabel>(dialog, "volumesStatus")->text().contains("already has"));
    EXPECT_EQ(document.surfaces().all().size(), 2u);
}

TEST(VolumesDialog, TheListsFollowTheDrawingWhileTheDialogStaysOpen)
{
    // The dialog is non-modal and kept. A surface made by a typed line, or one
    // an UNDO removes, used to be missed until the dialog was shown again: the
    // line it built named a surface the drawing no longer had (NotFound), and
    // one made meanwhile could not be chosen. Its own status said "UNDO removes
    // it" and the removed map was still offered.
    Document document;
    CommandInterpreter interpreter(document);
    addWedge(document, interpreter);
    Runner runner{document, interpreter, {}};
    VolumesDialog dialog(runner.context());
    auto* base = child<QComboBox>(dialog, "volumesBase");
    auto* top = child<QComboBox>(dialog, "volumesTop");
    auto* alignment = child<QComboBox>(dialog, "volumesAlignment");
    auto* command = child<QLineEdit>(dialog, "volumesCommand");
    EXPECT_EQ(command->text(), "VOLUME SURFACES ground design");
    EXPECT_EQ(alignment->count(), 1);

    // A surface made by a typed line is offered after a turn of the event loop.
    ASSERT_TRUE(interpreter.run("SURFACE DIFFERENCE ground design NAME raised").ok());
    QCoreApplication::processEvents();
    EXPECT_GE(top->findText("raised"), 0);
    EXPECT_GE(base->findText("raised"), 0);
    top->setCurrentIndex(top->findText("raised"));
    EXPECT_EQ(command->text(), "VOLUME SURFACES ground raised");

    // UNDO removes it: not offered, and the line no longer names it.
    ASSERT_TRUE(document.undo().ok());
    QCoreApplication::processEvents();
    EXPECT_LT(top->findText("raised"), 0);
    EXPECT_FALSE(command->text().contains("raised"));

    // The alignment, and the surface the page had chosen, go the same way.
    ASSERT_TRUE(document.undo().ok()); // ALIGN NEW cl
    QCoreApplication::processEvents();
    EXPECT_EQ(alignment->count(), 0);
    ASSERT_TRUE(document.undo().ok()); // the design
    QCoreApplication::processEvents();
    EXPECT_LT(top->findText("design"), 0);
    EXPECT_FALSE(command->text().contains("design"));
    EXPECT_TRUE(runner.lines.isEmpty()); // following the drawing runs nothing
}

TEST(VolumesDialog, SaveCsvRunsTheSameLineWritingTheFileItNames)
{
    Document document;
    CommandInterpreter interpreter(document);
    addWedge(document, interpreter);
    QStringList lines;
    VolumesDialogContext context;
    context.document = &document;
    context.headless = [] { return true; };
    context.run = [&lines](const QString& line) {
        lines << line;
        return VerbOutcome{true, QStringLiteral("volume method=exact cut=0.000"), {}};
    };
    VolumesDialog dialog(std::move(context));
    dialog.setMethod(VolumeMethod::Surfaces);
    // Headless, with no file named, nothing runs and the field is named.
    child<QPushButton>(dialog, "volumesSaveCsv")->click();
    EXPECT_TRUE(lines.isEmpty());
    child<QLineEdit>(dialog, "volumesCsvPath")->setText("C:/site/volumes.csv");
    child<QPushButton>(dialog, "volumesSaveCsv")->click();
    ASSERT_EQ(lines.size(), 1);
    EXPECT_EQ(lines.front(), "VOLUME SURFACES ground design csv=C:/site/volumes.csv OVERWRITE");
}

TEST(VolumesDialog, SaveCsvOfAScopeThatTakesNoPolygonSaysNothingWasSavedAndLeavesTheFile)
{
    // "The polygons below" on the default scope (the selection) with nothing
    // selected: the verb runs nothing and writes nothing, and the status used to
    // say "Saved the table to ..." over a file that still held what an earlier
    // save left there.
    Document document;
    CommandInterpreter interpreter(document);
    addWedge(document, interpreter);
    Runner runner{document, interpreter, {}};
    VolumesDialog dialog(runner.context());
    dialog.setMethod(VolumeMethod::Surfaces);
    const std::filesystem::path file =
        std::filesystem::temp_directory_path() /
        ("katana_volumes_dialog_" +
         std::to_string(reinterpret_cast<std::uintptr_t>(&dialog)) + ".csv");
    {
        std::ofstream out(file, std::ios::binary | std::ios::trunc);
        out << "OLD TABLE FROM AN EARLIER RUN\r\n";
    }
    const QString path = QString::fromStdString(file.generic_string());
    child<QLineEdit>(dialog, "volumesCsvPath")->setText(path);
    child<QRadioButton>(dialog, "volumesBoundaryScope")->click();
    auto* status = child<QLabel>(dialog, "volumesStatus");

    child<QPushButton>(dialog, "volumesSaveCsv")->click();
    ASSERT_EQ(runner.lines.size(), 1);
    EXPECT_FALSE(status->text().contains("Saved the table")) << status->text().toStdString();
    EXPECT_TRUE(status->text().contains("was not written")) << status->text().toStdString();
    EXPECT_TRUE(dialog.resultsText().contains("written")); // the csv record is in the table
    {
        std::ifstream in(file, std::ios::binary);
        std::string content((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
        EXPECT_EQ(content, "OLD TABLE FROM AN EARLIER RUN\r\n");
    }

    // The same save over typed corners that measure something writes the file
    // and says so.
    child<QRadioButton>(dialog, "volumesBoundaryCorners")->click();
    child<QLineEdit>(dialog, "volumesCorners")->setText("10,0 50,0 50,4");
    child<QPushButton>(dialog, "volumesSaveCsv")->click();
    ASSERT_EQ(runner.lines.size(), 2);
    EXPECT_TRUE(status->text().contains("Saved the table to")) << status->text().toStdString();
    {
        std::ifstream in(file, std::ios::binary);
        std::string first;
        std::getline(in, first);
        EXPECT_EQ(first.rfind("record,polygon,entity,polygon_area,area,cut,fill,net", 0), 0u)
            << first;
    }
    std::filesystem::remove(file);
}

TEST(VolumeResultTable, EveryRecordIsARowAndEveryKeyAColumnInTheOrderFirstMet)
{
    const auto table = katana::qt::volumeResultTable(
        "polygon index=1 cut=1.000 fill=2.000\n"
        "polygon index=2 cut=3.000 fill=4.000 chorded=yes\n"
        "volume method=exact cut=4.000 fill=6.000\n");
    EXPECT_EQ(table.columns,
              (QStringList{"record", "index", "cut", "fill", "chorded", "method"}));
    ASSERT_EQ(table.rows.size(), 3u);
    EXPECT_EQ(table.rows[1], (QStringList{"polygon", "2", "3.000", "4.000", "yes", ""}));
    EXPECT_EQ(table.rows[2], (QStringList{"volume", "", "4.000", "6.000", "", "exact"}));
}

// The tallest dialog the window has: every page's fields, the scope and filter
// controls, a table and a diagram, a thousand pixels and more once shown. A laptop's
// screen has less room than that, so it scrolls (src/katana_qt/screen_fit.hpp) and its
// Close button is still reached.
TEST(VolumesDialog, IsNoTallerThanTheScreenAndStillReachesItsCloseButton)
{
    Document document;
    CommandInterpreter interpreter(document);
    Runner runner{document, interpreter, {}};
    VolumesDialog dialog(runner.context());
    const QRect available = QGuiApplication::primaryScreen()->availableGeometry();
    ASSERT_GT(dialog.minimumSizeHint().height(), available.height() - 100)
        << "the premise: the dialog needs more room than this screen has";
    EXPECT_TRUE(katana::qt::fitDialogToScreen(dialog));
    dialog.show();
    QApplication::processEvents();
    EXPECT_LE(dialog.height(), available.height());
    auto* scroll = dialog.findChild<QScrollArea*>(katana::qt::kScreenFitScroll);
    ASSERT_NE(scroll, nullptr);
    auto* close = child<QPushButton>(dialog, "volumesClose");
    scroll->ensureWidgetVisible(close, 0, 0);
    QApplication::processEvents();
    const QRect where(close->mapTo(scroll->viewport(), QPoint(0, 0)), close->size());
    EXPECT_TRUE(scroll->viewport()->rect().contains(where)) << "Close cannot be scrolled into view";
}
