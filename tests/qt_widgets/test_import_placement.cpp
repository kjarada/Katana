// Where an import lands, in the window (src/katana_qt/import_placement,
// gis_dialogs): the Placement group driven by its object names, File >
// Import's step and the IMPORT line it makes, GIS > Import Vector Data's
// dialog with the group in it, and the one place the window decides where
// data it has read goes, in a headless session where nobody is asked.
//
// The coordinates are those of samples/gis/parcels.geojson, which spans
// (180, 0) to (365, 165): what the typed and katana_cli tests import.

#include <gtest/gtest.h>

#include <QCoreApplication>
#include <QDoubleSpinBox>
#include <QLabel>
#include <QPushButton>
#include <QRadioButton>
#include <QSettings>

#include <string>
#include <utility>
#include <vector>

#include "gis_dialogs.hpp"
#include "import_placement.hpp"
#include "katana/cad/command_interpreter.hpp"

using katana::cad::ImportPlacement;
using katana::cad::ImportPlacementMode;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Vec2;
using katana::qt::ImportPlacementBox;
using katana::qt::ImportPlacementDialog;

namespace {

const Box2 kParcels(Point2(180.0, 0.0), Point2(365.0, 165.0));

template <typename T> T* child(QWidget& parent, const char* name)
{
    auto* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << name;
    return found;
}

// The application's settings under a name of the test's own, emptied before
// and after, so a remembered choice neither leaks in from a person's
// sessions nor out to them. The widget suite's application has no name, and
// QSettings then keeps nothing - which is what every other test here relies
// on.
struct OwnSettings {
    OwnSettings()
    {
        QCoreApplication::setOrganizationName("KatanaWidgetTests");
        QCoreApplication::setApplicationName("ImportPlacement");
        QSettings().clear();
    }
    ~OwnSettings()
    {
        QSettings().clear();
        QCoreApplication::setOrganizationName(QString());
        QCoreApplication::setApplicationName(QString());
    }
};

} // namespace

TEST(ImportPlacementBox, EachChoiceIsItsPlacementAndSaysWhatItWillDoAndTheLineThatDoesIt)
{
    const OwnSettings settings;
    const Box2 drawing(Point2(1000.0, 2000.0), Point2(1010.0, 2005.0));
    ImportPlacementBox box(drawing, "C:/Survey Data/site.dxf");
    auto* keep = child<QRadioButton>(box, "importPlacementKeep");
    auto* local = child<QRadioButton>(box, "importPlacementLocal");
    auto* alongside = child<QRadioButton>(box, "importPlacementAlongside");
    auto* offset = child<QRadioButton>(box, "importPlacementOffset");
    auto* east = child<QDoubleSpinBox>(box, "importOffsetE");
    auto* north = child<QDoubleSpinBox>(box, "importOffsetN");
    ASSERT_TRUE(keep && local && alongside && offset && east && north);
    ASSERT_NE(child<QLabel>(box, "importPlacementShift"), nullptr);

    // Nothing remembered: its own coordinates, and the question if far away.
    EXPECT_TRUE(keep->isChecked());
    EXPECT_EQ(box.placement(), ImportPlacement{});
    EXPECT_TRUE(box.said().contains("keeps its own coordinates")) << box.said().toStdString();
    EXPECT_TRUE(box.said().endsWith("Typed: IMPORT \"C:/Survey Data/site.dxf\""))
        << box.said().toStdString();
    EXPECT_FALSE(east->isEnabled());

    local->click();
    EXPECT_EQ(box.placement().mode, ImportPlacementMode::Local);
    EXPECT_TRUE(box.said().contains("lower-left corner sits at 0,0")) << box.said().toStdString();
    EXPECT_TRUE(box.said().endsWith("IMPORT \"C:/Survey Data/site.dxf\" LOCAL"));

    alongside->click();
    EXPECT_EQ(box.placement().mode, ImportPlacementMode::Alongside);
    EXPECT_TRUE(box.said().contains("sits on the drawing's, at 1000.000,2000.000"))
        << box.said().toStdString();
    EXPECT_TRUE(box.said().endsWith("IMPORT \"C:/Survey Data/site.dxf\" ALONGSIDE"));

    offset->click();
    EXPECT_TRUE(east->isEnabled());
    east->setValue(10.0);
    north->setValue(-20.5);
    EXPECT_EQ(box.placement(), (ImportPlacement{ImportPlacementMode::Offset, Vec2(10.0, -20.5)}));
    EXPECT_TRUE(box.said().contains("moved as one piece by 10.000,-20.500"))
        << box.said().toStdString();
    EXPECT_TRUE(box.said().endsWith("IMPORT \"C:/Survey Data/site.dxf\" OFFSET=10,-20.5"))
        << box.said().toStdString();
}

TEST(ImportPlacementBox, AlongsideAnEmptyDrawingSaysThereIsNothingToSitBeside)
{
    const OwnSettings settings;
    ImportPlacementBox box(Box2{}, QString());
    child<QRadioButton>(box, "importPlacementAlongside")->click();
    EXPECT_TRUE(box.said().contains("The drawing is empty")) << box.said().toStdString();
    EXPECT_FALSE(box.said().contains("Typed:")) << "no file, no line";
}

TEST(ImportPlacementBox, TheChoiceIsRememberedOnlyWhenAnImportIsAcceptedAndOfferedNextTime)
{
    const OwnSettings settings;
    {
        ImportPlacementBox box(Box2{}, QString());
        box.setPlacement(ImportPlacement{ImportPlacementMode::Offset, Vec2(-5.25, 7.0)});
        // Not remembered: a click is no import.
    }
    EXPECT_EQ(ImportPlacementBox(Box2{}, QString()).placement(), ImportPlacement{});
    {
        ImportPlacementDialog dialog("C:/data/site.12da", Box2{});
        dialog.box().setPlacement(ImportPlacement{ImportPlacementMode::Offset, Vec2(-5.25, 7.0)});
        child<QPushButton>(dialog, "importPlacementImport")->click();
        EXPECT_EQ(dialog.result(), QDialog::Accepted);
    }
    EXPECT_EQ(ImportPlacementBox(Box2{}, QString()).placement(),
              (ImportPlacement{ImportPlacementMode::Offset, Vec2(-5.25, 7.0)}));
    {
        ImportPlacementDialog dialog("C:/data/site.12da", Box2{});
        dialog.box().setPlacement(ImportPlacement{ImportPlacementMode::Local, {}});
        child<QPushButton>(dialog, "importPlacementCancel")->click();
    }
    EXPECT_EQ(ImportPlacementBox(Box2{}, QString()).placement().mode, ImportPlacementMode::Offset)
        << "a cancelled import remembers nothing";
}

TEST(ImportPlacementDialog, ItsLineReadsBackAsThePathAndThePlacementItWasMadeFrom)
{
    // File > Import runs this line through the window's executor, which reads
    // it with CommandInterpreter::importArgument: it must mean what was
    // chosen, blanks in the path and all.
    for (const ImportPlacement& chosen :
         {ImportPlacement{}, ImportPlacement{ImportPlacementMode::Local, {}},
          ImportPlacement{ImportPlacementMode::Alongside, {}},
          ImportPlacement{ImportPlacementMode::Offset, Vec2(0.1, -1234567.125)}}) {
        ImportPlacementDialog dialog("C:/Survey Data/job 7.12da", Box2{});
        EXPECT_EQ(dialog.objectName(), "importPlacementDialog");
        dialog.box().setPlacement(chosen);
        const QString line = dialog.line();
        ASSERT_TRUE(line.startsWith("IMPORT ")) << line.toStdString();
        const auto read =
            katana::cad::CommandInterpreter::importArgument(line.mid(6).toStdString());
        ASSERT_TRUE(read.ok()) << line.toStdString();
        EXPECT_EQ(read->path, "C:/Survey Data/job 7.12da") << line.toStdString();
        EXPECT_EQ(read->placement, chosen) << line.toStdString();
    }
}

TEST(VectorImportDialog, TheGisImportHasThePlacementGroupAndNamesItsFields)
{
    const OwnSettings settings;
    katana::interop::SourceDescription source;
    source.path = "parcels.geojson";
    source.kind = katana::interop::SourceKind::Vector;
    source.driver = "GeoJSON";
    source.vectorLayers.push_back({"parcels", 4, "Polygon", ""});
    katana::qt::VectorImportDialog dialog(source, Box2{});
    ASSERT_NE(child<QWidget>(dialog, "importSourceLayer"), nullptr);
    ASSERT_NE(child<QWidget>(dialog, "importTargetLayer"), nullptr);
    ASSERT_NE(child<QWidget>(dialog, "importAttributes"), nullptr);
    EXPECT_EQ(dialog.placementBox().placement(), ImportPlacement{});
    child<QRadioButton>(dialog, "importPlacementLocal")->click();
    EXPECT_EQ(dialog.placementBox().placement().mode, ImportPlacementMode::Local);
}

TEST(DecideImportPlacement, AHeadlessImportFarFromTheDrawingKeepsItsCoordinatesAndSaysWhy)
{
    std::vector<std::pair<QString, bool>> logged;
    const auto log = [&](const QString& text, bool isError) { logged.emplace_back(text, isError); };
    // A 100 x 50 drawing at the origin and the parcels' shape 18 km east: at
    // a zoom that shows both (diagonal 18 186) the drawing's diagonal, 112,
    // is under 1% of it, yet over 2% of the parcels' 248 - lost by where it
    // was put (interop::advisePlacement).
    const Box2 drawing(Point2(0.0, 0.0), Point2(100.0, 50.0));
    const auto kept = katana::qt::decideImportPlacement(nullptr, true, ImportPlacement{}, drawing,
                                                        Box2(Point2(18000.0, 0.0),
                                                             Point2(18185.0, 165.0)),
                                                        log);
    EXPECT_FALSE(kept.cancelled);
    EXPECT_FALSE(kept.shift.has_value());
    ASSERT_EQ(logged.size(), 1u);
    EXPECT_TRUE(logged[0].first.endsWith("(kept: no one to ask).")) << logged[0].first.toStdString();
    EXPECT_TRUE(logged[0].second);

    // Near the drawing: nothing to ask, nothing said.
    logged.clear();
    const auto near = katana::qt::decideImportPlacement(nullptr, true, ImportPlacement{},
                                                        Box2(Point2(0, 0), Point2(200, 200)),
                                                        kParcels, log);
    EXPECT_FALSE(near.shift.has_value());
    EXPECT_TRUE(logged.empty());
}

TEST(DecideImportPlacement, AChosenPlacementIsNeverAskedAndSaysTheMoveItMakes)
{
    std::vector<std::pair<QString, bool>> logged;
    const auto log = [&](const QString& text, bool isError) { logged.emplace_back(text, isError); };
    // Not headless: a chosen placement opens no box, so this returns at once.
    const auto local = katana::qt::decideImportPlacement(
        nullptr, false, ImportPlacement{ImportPlacementMode::Local, {}},
        Box2(Point2(0.0, 0.0), Point2(1.0, 0.5)), kParcels, log);
    ASSERT_TRUE(local.shift.has_value());
    EXPECT_EQ(*local.shift, Vec2(180.0, 0.0));
    ASSERT_EQ(logged.size(), 1u);
    EXPECT_EQ(logged[0].first,
              "LOCAL: moved as one piece by -180.000,0.000, so its lower-left corner sits at 0,0.");
    EXPECT_FALSE(logged[0].second);
}
