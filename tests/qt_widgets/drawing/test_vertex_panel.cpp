// The Vertices panel (src/katana_qt/drawing/vertex_panel.*): it follows the
// selection, shows the polyline's vertices, and each edit typed into it is
// one undoable step.

#include <gtest/gtest.h>

#include <QDockWidget>
#include <QMainWindow>
#include <QPushButton>
#include <QMenu>
#include <QTableWidget>
#include <QToolBar>

#include "drawing/drawing_ui.hpp"
#include "drawing/vertex_panel.hpp"
#include "katana/commands/entity_commands.hpp"

using katana::cad::Document;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::qt::drawing::VertexPanel;

namespace {

katana::entity::EntityId addTraverse(Document& document)
{
    EXPECT_TRUE(document
                    .execute(katana::commands::createPolyline(
                        Polyline2{{Point2(0, 0), Point2(10, 0), Point2(10, 10)}, false}))
                    .ok());
    return document.lastCreatedEntities().front();
}

} // namespace

TEST(VertexPanelWidget, FollowsTheSelection)
{
    Document document;
    const auto id = addTraverse(document);
    VertexPanel panel(document);
    auto* table = panel.findChild<QTableWidget*>("vertexTable");
    ASSERT_NE(table, nullptr);
    EXPECT_FALSE(panel.shown().has_value());
    EXPECT_EQ(table->rowCount(), 0);
    document.selection().add(id);
    document.notifySelectionChanged();
    EXPECT_EQ(panel.shown(), id);
    EXPECT_EQ(table->rowCount(), 3);
    EXPECT_EQ(table->item(1, 1)->text(), "10.000");
    document.selection().clear();
    document.notifySelectionChanged();
    EXPECT_EQ(table->rowCount(), 0);
}

TEST(VertexPanelWidget, AnEditTypedIntoTheTableIsOneUndoStep)
{
    Document document;
    const auto id = addTraverse(document);
    document.selection().add(id);
    document.notifySelectionChanged();
    VertexPanel panel(document);
    auto* table = panel.findChild<QTableWidget*>("vertexTable");
    const std::size_t before = document.history().undoCount();
    // Typed as a user types it: the cell's text changes and the panel reacts.
    table->item(2, 3)->setText("104.5"); // vertex 2's height
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(document.history().undoName(), "VERTEX_SET");
    const auto& entity = *document.model().entities.find(id);
    EXPECT_EQ(katana::entity::heightsOf(entity.properties, 3)[2], 104.5);
    EXPECT_EQ(table->item(2, 3)->text(), "104.500") << "shown as the drawing now has it";
    // A value that does not parse changes nothing and puts the cell back.
    table->item(1, 1)->setText("east");
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(table->item(1, 1)->text(), "10.000");
    ASSERT_TRUE(document.undo().ok());
    EXPECT_FALSE(katana::entity::heightsOf(document.model().entities.find(id)->properties, 3)[2]
                     .has_value());
}

TEST(VertexPanelWidget, InsertAndDeleteButtons)
{
    Document document;
    const auto id = addTraverse(document);
    document.selection().add(id);
    document.notifySelectionChanged();
    VertexPanel panel(document);
    auto* table = panel.findChild<QTableWidget*>("vertexTable");
    table->setCurrentCell(0, 1);
    panel.findChild<QPushButton*>("vertexInsertButton")->click();
    EXPECT_EQ(table->rowCount(), 4);
    EXPECT_EQ(table->item(1, 1)->text(), "5.000");
    table->setCurrentCell(1, 1);
    panel.findChild<QPushButton*>("vertexDeleteButton")->click();
    EXPECT_EQ(table->rowCount(), 3);
}

TEST(DrawingUiInstall, TheDockAndTheDraftingTogglesFollowTheDocument)
{
    Document document;
    QMainWindow window;
    auto* panels = new QMenu("Panels", &window);
    const auto ui = katana::qt::drawing::installDrawingUi(window, document, panels, nullptr);
    ASSERT_NE(ui.verticesDock, nullptr);
    EXPECT_EQ(ui.verticesDock->objectName(), "VerticesDock");
    auto* ortho = window.findChild<QAction*>("actionOrtho");
    ASSERT_NE(ortho, nullptr);
    ortho->trigger();
    EXPECT_TRUE(document.drafting().ortho);
    // A verb (or anything else) that changes the setting checks the button.
    document.drafting().ortho = false;
    document.notifyDraftingChanged();
    EXPECT_FALSE(ortho->isChecked());
    auto* quadrant = window.findChild<QAction*>("actionSnapQuadrant");
    ASSERT_NE(quadrant, nullptr);
    quadrant->trigger();
    EXPECT_TRUE(katana::cad::hasMode(document.drafting().snapModes, katana::cad::SnapMode::Quadrant));
    EXPECT_NE(window.findChild<QAction*>("actionVerticesPanel"), nullptr);
}
