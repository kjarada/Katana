// The Surfaces panel (src/katana_qt/terrain/surfaces_panel.*): the drawing's
// surfaces as a table that follows the document, every change a line through
// the window's one executor - here an interpreter of the test's own, which
// runs the drawing's SURFACE words as the window's command line does.

#include <gtest/gtest.h>

#include <QAction>
#include <QComboBox>
#include <QCoreApplication>
#include <QDockWidget>
#include <QMainWindow>
#include <QMenu>
#include <QTableWidget>
#include <QToolButton>

#include <memory>
#include <optional>
#include <string>
#include <utility>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/terrain/surface_store.hpp"
#include "terrain/surfaces_panel.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::qt::SurfacesPanel;
using katana::qt::VerbOutcome;

namespace {

std::shared_ptr<const katana::terrain::TinSurface> square(double z)
{
    using katana::terrain::Point3;
    auto made = katana::terrain::TinSurface::create(
        {Point3(0, 0, z), Point3(10, 0, z), Point3(10, 10, z), Point3(0, 10, z)},
        {{0, 1, 2}, {0, 2, 3}});
    EXPECT_TRUE(made.ok());
    return std::make_shared<const katana::terrain::TinSurface>(
        made.ok() ? std::move(made).value() : katana::terrain::TinSurface{});
}

// A drawing of two surfaces, the panel on it, and the lines it ran.
struct Rig {
    Document document;
    CommandInterpreter interpreter{document};
    QStringList lines;
    std::optional<katana::geometry::Box2> zoomed;
    int styleEdits = 0;
    std::unique_ptr<SurfacesPanel> panel;

    explicit Rig(bool withSurfaces = true)
    {
        if (withSurfaces) {
            katana::terrain::SurfaceStore store;
            EXPECT_TRUE(store.add({"ground", square(100.0), "raster dem.tif"}).ok());
            EXPECT_TRUE(store.add({"design", square(99.0), "drawing (4 entities)"}).ok());
            EXPECT_TRUE(document.setSurfaces(std::move(store), "SURFACE FROM").ok());
        }
        SurfacesPanel::Services services;
        services.run = [this](const QString& line) {
            lines << line;
            VerbOutcome outcome;
            auto reply = interpreter.run(line.toStdString());
            outcome.ok = reply.ok();
            if (reply.ok()) {
                outcome.reply = QString::fromStdString(*reply);
            } else {
                outcome.error = QString::fromStdString(reply.error().describe());
            }
            return outcome;
        };
        services.zoomTo = [this](const katana::geometry::Box2& box) { zoomed = box; };
        services.editStyles = [this] { ++styleEdits; };
        panel = std::make_unique<SurfacesPanel>(document, std::move(services));
    }

    [[nodiscard]] QTableWidget* table() const
    {
        return panel->findChild<QTableWidget*>("surfacesTable");
    }
    [[nodiscard]] QToolButton* button(const char* name) const
    {
        return panel->findChild<QToolButton*>(name);
    }
    // The handler's own change is read once it has returned.
    static void settle() { QCoreApplication::processEvents(); }
};

} // namespace

TEST(SurfacesPanelWidget, ShowsARowPerSurfaceWithItsStyleSourceAndTriangles)
{
    Rig rig;
    QTableWidget* table = rig.table();
    ASSERT_NE(table, nullptr);
    EXPECT_EQ(rig.panel->objectName(), "surfacesPanel");
    ASSERT_EQ(table->rowCount(), 2);
    EXPECT_EQ(table->item(0, SurfacesPanel::Name)->text(), "ground");
    EXPECT_EQ(table->item(0, SurfacesPanel::Name)->checkState(), Qt::Checked);
    EXPECT_EQ(table->item(0, SurfacesPanel::Source)->text(), "raster dem.tif");
    EXPECT_EQ(table->item(0, SurfacesPanel::Triangles)->text(), "2");
    auto* style = qobject_cast<QComboBox*>(table->cellWidget(0, SurfacesPanel::Style));
    ASSERT_NE(style, nullptr);
    EXPECT_EQ(style->objectName(), "surfaceStyle");
    EXPECT_EQ(style->currentText(), "Default (Standard)");
    // The default, then the seven built-in styles.
    EXPECT_EQ(style->count(), 8);
}

TEST(SurfacesPanelWidget, NothingPickedLeavesTheRowToolsOff)
{
    Rig rig;
    for (const char* name : {"surfacesRename", "surfacesCopy", "surfacesRemove", "surfacesZoomTo"}) {
        ASSERT_NE(rig.button(name), nullptr) << name;
        EXPECT_FALSE(rig.button(name)->isEnabled()) << name;
    }
    ASSERT_NE(rig.button("surfacesEditStyles"), nullptr);
    EXPECT_TRUE(rig.button("surfacesEditStyles")->isEnabled());
    rig.button("surfacesEditStyles")->click();
    EXPECT_EQ(rig.styleEdits, 1);
    ASSERT_TRUE(rig.panel->pickSurface("DESIGN"));
    EXPECT_EQ(rig.panel->pickedSurface(), std::optional<std::string>("design"));
    EXPECT_TRUE(rig.button("surfacesCopy")->isEnabled());
}

TEST(SurfacesPanelWidget, UntickingASurfaceHidesItAsOneUndoStep)
{
    Rig rig;
    const std::size_t steps = rig.document.history().undoCount();
    rig.table()->item(0, SurfacesPanel::Name)->setCheckState(Qt::Unchecked);
    ASSERT_FALSE(rig.lines.isEmpty());
    EXPECT_EQ(rig.lines.back(), "SURFACE HIDE \"ground\"");
    EXPECT_FALSE(rig.document.surfaces().find("ground")->visible);
    EXPECT_EQ(rig.document.history().undoCount(), steps + 1);
    Rig::settle();
    EXPECT_EQ(rig.table()->item(0, SurfacesPanel::Name)->checkState(), Qt::Unchecked);
    rig.table()->item(0, SurfacesPanel::Name)->setCheckState(Qt::Checked);
    EXPECT_EQ(rig.lines.back(), "SURFACE SHOW \"ground\"");
    EXPECT_TRUE(rig.document.surfaces().find("ground")->visible);
}

TEST(SurfacesPanelWidget, TypingANewNameRenamesTheSurface)
{
    Rig rig;
    rig.table()->item(1, SurfacesPanel::Name)->setText("design pad");
    EXPECT_EQ(rig.lines.back(), "SURFACE RENAME \"design\" \"design pad\"");
    EXPECT_NE(rig.document.surfaces().find("design pad"), nullptr);
    Rig::settle();
    EXPECT_EQ(rig.table()->item(1, SurfacesPanel::Name)->text(), "design pad");
    // A name another surface has is refused by the verb; the row shows the
    // drawing's name again.
    rig.table()->item(1, SurfacesPanel::Name)->setText("ground");
    EXPECT_NE(rig.panel->lastReply().indexOf("AlreadyExists"), -1) << rig.panel->lastReply().toStdString();
    Rig::settle();
    EXPECT_EQ(rig.table()->item(1, SurfacesPanel::Name)->text(), "design pad");
}

TEST(SurfacesPanelWidget, ChoosingAStyleGivesItToTheSurface)
{
    Rig rig;
    auto* style = qobject_cast<QComboBox*>(rig.table()->cellWidget(0, SurfacesPanel::Style));
    ASSERT_NE(style, nullptr);
    const int contours = style->findText("Contours");
    ASSERT_GT(contours, 0);
    style->setCurrentIndex(contours);
    emit style->activated(contours); // as a person's choice in the list does
    EXPECT_EQ(rig.lines.back(), "SURFACE STYLE \"ground\" \"Contours\"");
    EXPECT_EQ(rig.document.surfaces().find("ground")->style, "Contours");
    Rig::settle();
    style = qobject_cast<QComboBox*>(rig.table()->cellWidget(0, SurfacesPanel::Style));
    EXPECT_EQ(style->currentText(), "Contours");
    // Back to the drawing's default: the first entry, an empty style.
    emit style->activated(0);
    EXPECT_EQ(rig.lines.back(), "SURFACE STYLE \"ground\" \"\"");
    EXPECT_EQ(rig.document.surfaces().find("ground")->style, "");
}

TEST(SurfacesPanelWidget, CopyRemoveAndZoomToActOnThePickedSurface)
{
    Rig rig;
    ASSERT_TRUE(rig.panel->pickSurface("ground"));
    rig.button("surfacesCopy")->click();
    EXPECT_EQ(rig.lines.back(), "SURFACE COPY \"ground\" \"ground (2)\"");
    EXPECT_NE(rig.document.surfaces().find("ground (2)"), nullptr);
    Rig::settle();
    EXPECT_EQ(rig.table()->rowCount(), 3);
    EXPECT_EQ(rig.panel->pickedSurface(), std::optional<std::string>("ground")) << "still picked";

    rig.button("surfacesZoomTo")->click();
    ASSERT_TRUE(rig.zoomed.has_value());
    EXPECT_EQ(rig.zoomed->min.x, 0.0);
    EXPECT_EQ(rig.zoomed->max.y, 10.0);

    // SURFACE REMOVE is the geoprocessing executor's, which this test's
    // runner does not have: the line is what the panel answers for.
    rig.button("surfacesRemove")->click();
    EXPECT_EQ(rig.lines.back(), "SURFACE REMOVE \"ground\"");
}

TEST(SurfacesPanelWidget, AChangeMadeElsewhereIsShownAtOnce)
{
    Rig rig(false);
    EXPECT_EQ(rig.table()->rowCount(), 0);
    katana::terrain::SurfaceStore store;
    ASSERT_TRUE(store.add({"ground", square(5.0), "test"}).ok());
    ASSERT_TRUE(rig.document.setSurfaces(std::move(store)).ok());
    EXPECT_EQ(rig.table()->rowCount(), 1) << "without waiting for the event loop";
    ASSERT_TRUE(rig.document.undo().ok());
    EXPECT_EQ(rig.table()->rowCount(), 0);
}

TEST(SurfacesPanelWidget, ItsDockIsReachedFromViewPanelsAndTheTerrainMenu)
{
    Document document;
    QMainWindow window;
    QMenu panels("Panels");
    QMenu terrain("Terrain");
    auto* drawing = terrain.addAction("Surface From Drawing...");
    drawing->setObjectName("surfaceFromDrawing");
    terrain.addSection("Sections");
    auto* after = terrain.addAction("Cut Section");

    const katana::qt::SurfacesPanelUi ui = katana::qt::installSurfacesPanel(
        window, document, SurfacesPanel::Services{}, &panels, &terrain, nullptr);
    ASSERT_NE(ui.dock, nullptr);
    EXPECT_EQ(ui.dock->objectName(), "SurfacesDock");
    EXPECT_TRUE(ui.dock->isHidden()) << "until asked for";
    ASSERT_NE(ui.toggle, nullptr);
    EXPECT_EQ(ui.toggle->objectName(), "viewSurfacesPanel");
    EXPECT_TRUE(panels.actions().contains(ui.toggle));
    ASSERT_NE(ui.terrainItem, nullptr);
    EXPECT_EQ(ui.terrainItem->objectName(), "terrainSurfacesPanel");
    EXPECT_FALSE(ui.terrainItem->icon().isNull());
    EXPECT_FALSE(ui.terrainItem->statusTip().isEmpty());
    // Right after Surface From Drawing, before the next section.
    const QList<QAction*> items = terrain.actions();
    const qsizetype at = items.indexOf(ui.terrainItem);
    ASSERT_GT(at, 0);
    EXPECT_EQ(items[at - 1], drawing);
    EXPECT_LT(at, items.indexOf(after));
    ui.terrainItem->trigger();
    EXPECT_FALSE(ui.dock->isHidden());
}
