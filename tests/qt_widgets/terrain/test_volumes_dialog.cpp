// Terrain > Volumes (src/katana_qt/terrain/volumes_dialog.hpp): each page's
// line, built from its fields as a person fills them - found by object name -
// and handed to the executor exactly as shown; and, with the interpreter
// itself as the executor, the records it answers in the table and the mass
// haul diagram, and the cut/fill map made as a surface of the drawing.

#include <gtest/gtest.h>

#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
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
