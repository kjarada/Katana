// File > Project Coordinate System (src/katana_qt/project_crs_dialog): driven
// by its controls' object names as a person uses them, each change one undo
// step through Document::setCoordinateSystem.

#include <gtest/gtest.h>

#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QTreeWidget>

#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "project_crs_dialog.hpp"

using katana::cad::Document;
using katana::qt::ProjectCrsDialog;

namespace {

template <typename T>
T* child(QWidget& parent, const char* name)
{
    auto* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << name;
    return found;
}

// Every selectable item's code, in the order the list shows them.
QStringList codes(QTreeWidget& list)
{
    QStringList out;
    for (int i = 0; i < list.topLevelItemCount(); ++i) {
        QTreeWidgetItem* top = list.topLevelItem(i);
        for (int j = 0; j < top->childCount(); ++j) {
            out << top->child(j)->text(1);
        }
    }
    return out;
}

} // namespace

TEST(ProjectCrsDialog, TypingACodeChecksItAndSetSetsItAsOneStep)
{
    Document document;
    ProjectCrsDialog dialog(document, std::nullopt);
    auto* text = child<QLineEdit>(dialog, "projectCrsText");
    auto* check = child<QLabel>(dialog, "projectCrsCheck");
    ASSERT_TRUE(text && check);

    text->setText("7856");
    EXPECT_TRUE(check->text().contains("GDA2020 / MGA zone 56")) << check->text().toStdString();
    text->setText("no such system");
    EXPECT_TRUE(check->text().startsWith("Not a coordinate system")) << check->text().toStdString();
    EXPECT_FALSE(dialog.apply());
    EXPECT_TRUE(document.metadata().coordinateSystem.empty());

    const std::size_t before = document.history().undoCount();
    text->setText("epsg:7856");
    ASSERT_TRUE(dialog.apply());
    EXPECT_EQ(document.metadata().coordinateSystem, "EPSG:7856");
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(katana::qt::projectCrsLabel(document).toStdString(), "EPSG:7856  GDA2020 / MGA zone 56");
}

TEST(ProjectCrsDialog, APlaceIsOfferedItsZoneFirstAndSearchFilters)
{
    Document document;
    // Sydney.
    ProjectCrsDialog dialog(document, std::pair{151.21, -33.87});
    auto* list = child<QTreeWidget>(dialog, "projectCrsList");
    auto* search = child<QLineEdit>(dialog, "projectCrsSearch");
    ASSERT_TRUE(list && search);
    ASSERT_GT(list->topLevelItemCount(), 1);
    EXPECT_EQ(list->topLevelItem(0)->text(0).toStdString(), "Suggested for this place");
    EXPECT_EQ(list->topLevelItem(0)->child(0)->text(1).toStdString(), "EPSG:7856");

    search->setText("mga 55");
    EXPECT_EQ(codes(*list), (QStringList{"EPSG:7855", "EPSG:28355"}));

    // Choosing an item fills the text box, and Set sets it.
    list->setCurrentItem(list->topLevelItem(0)->child(0));
    auto* text = child<QLineEdit>(dialog, "projectCrsText");
    ASSERT_NE(text, nullptr);
    EXPECT_EQ(text->text().toStdString(), "EPSG:7855");
    ASSERT_TRUE(dialog.apply());
    EXPECT_EQ(document.metadata().coordinateSystem, "EPSG:7855");
}

TEST(ProjectCrsDialog, APlaceTypedIsSuggestedItsZoneFirst)
{
    // File > Project Coordinate System opens with no place: one is typed.
    // Perth is at 115.86 E, in UTM zone floor((115.86 + 180) / 6) + 1 = 50:
    // GDA2020 / MGA zone 50 is EPSG:7850.
    Document document;
    ProjectCrsDialog dialog(document, std::nullopt);
    auto* list = child<QTreeWidget>(dialog, "projectCrsList");
    auto* place = child<QLineEdit>(dialog, "projectCrsPlace");
    auto* suggest = child<QPushButton>(dialog, "projectCrsSuggest");
    ASSERT_TRUE(list && place && suggest);
    EXPECT_TRUE(place->text().isEmpty()) << "no system and no drawing: no place to give";
    ASSERT_GT(list->topLevelItemCount(), 0);
    EXPECT_NE(list->topLevelItem(0)->text(0).toStdString(), "Suggested for this place");

    child<QLineEdit>(dialog, "projectCrsSearch")->setText("utm");
    place->setText("115.86 -31.95");
    suggest->click();
    EXPECT_TRUE(child<QLineEdit>(dialog, "projectCrsSearch")->text().isEmpty());
    EXPECT_EQ(list->topLevelItem(0)->text(0).toStdString(), "Suggested for this place");
    EXPECT_EQ(list->topLevelItem(0)->child(0)->text(1).toStdString(), "EPSG:7850");
}

TEST(ProjectCrsDialog, APlaceThatIsNotOneIsSaidAndSuggestsNothing)
{
    Document document;
    ProjectCrsDialog dialog(document, std::nullopt);
    auto* place = child<QLineEdit>(dialog, "projectCrsPlace");
    auto* check = child<QLabel>(dialog, "projectCrsCheck");
    ASSERT_TRUE(place && check);
    for (const char* text : {"north of here", "151.21", "151.21, -33.87, 4", ""}) {
        place->setText(text);
        EXPECT_FALSE(dialog.suggest()) << text;
        EXPECT_TRUE(check->text().startsWith("Not a place")) << text;
    }
    // Numbers, but no longitude: suggestCoordinateSystems says which.
    place->setText("200, 10");
    EXPECT_FALSE(dialog.suggest());
    EXPECT_TRUE(check->text().contains("longitude")) << check->text().toStdString();
}

TEST(ProjectCrsDialog, TheDrawingsCentreIsFilledInAsALongitudeAndALatitude)
{
    // In WGS 84 / UTM zone 56S (EPSG:32756) the central meridian, 153 E, is
    // at the false easting 500 000, and the equator at the southern false
    // northing 10 000 000 (the UTM definition): a point there is 153, 0
    // exactly, with no datum between the two systems to shift it.
    Document document;
    ASSERT_TRUE(document.setCoordinateSystem("EPSG:32756").ok());
    ASSERT_TRUE(
        document.execute(katana::commands::createPoint({500000.0, 10000000.0})).ok());
    ProjectCrsDialog dialog(document, std::nullopt);
    const QStringList parts = child<QLineEdit>(dialog, "projectCrsPlace")->text().split(", ");
    ASSERT_EQ(parts.size(), 2) << parts.join('|').toStdString();
    // Printed to 6 decimals, a tenth of a metre.
    EXPECT_NEAR(parts[0].toDouble(), 153.0, 1e-6);
    EXPECT_NEAR(parts[1].toDouble(), 0.0, 1e-6);
    // Filled in, not yet suggested: that is Suggest's.
    auto* list = child<QTreeWidget>(dialog, "projectCrsList");
    EXPECT_NE(list->topLevelItem(0)->text(0).toStdString(), "Suggested for this place");
}

TEST(ProjectCrsDialog, LocalCoordinatesClearsIt)
{
    Document document;
    ASSERT_TRUE(document.setCoordinateSystem("EPSG:28356").ok());
    ProjectCrsDialog dialog(document, std::nullopt);
    auto* local = child<QPushButton>(dialog, "projectCrsClear");
    ASSERT_NE(local, nullptr);
    local->click();
    EXPECT_TRUE(document.metadata().coordinateSystem.empty());
    EXPECT_EQ(katana::qt::projectCrsLabel(document).toStdString(), "no coordinate system");
}
