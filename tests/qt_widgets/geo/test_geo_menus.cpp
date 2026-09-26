// The GIS and Terrain items the geoprocessing lanes add (src/katana_qt/geo/
// menu_table.cpp): each one, and each submenu, reached from the keyboard by a
// letter of its own within its menu. Terrain > Analysis and its six items had
// none, so a person could reach them only with the mouse.
// qt_every_shortcut_and_menu_letter_reaches_one_thing_headless checks that no
// letter is shared across the whole window; this checks that none is missing.

#include <gtest/gtest.h>

#include <QAction>
#include <QMainWindow>
#include <QMenu>
#include <QSet>
#include <QString>

#include <filesystem>
#include <memory>

#include "geo/geo_workbench.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

using katana::qt::GeoMenus;
using katana::qt::GeoServices;
using katana::qt::GeoWorkbench;

// The letter after a single '&', upper case; empty for a text with none (the
// rule MainWindow::shortcutClashes reads).
QString letterOf(const QString& text)
{
    for (qsizetype at = text.indexOf('&'); at >= 0 && at + 1 < text.size();
         at = text.indexOf('&', at + 2)) {
        if (text[at + 1] != '&') {
            return text.mid(at + 1, 1).toUpper();
        }
    }
    return {};
}

// Every item of `menu` and of its submenus has a letter, none shared in one
// menu; each failure names the menu path and the item.
void expectLetters(const QMenu& menu, const QString& path)
{
    QSet<QString> taken;
    for (const QAction* item : menu.actions()) {
        if (item->isSeparator()) {
            continue;
        }
        const QString letter = letterOf(item->text());
        EXPECT_FALSE(letter.isEmpty())
            << path.toStdString() << " > " << item->text().toStdString() << " has no letter";
        if (!letter.isEmpty()) {
            EXPECT_FALSE(taken.contains(letter))
                << path.toStdString() << ": " << letter.toStdString() << " is shared";
            taken.insert(letter);
        }
        if (const QMenu* sub = item->menu()) {
            expectLetters(*sub, path + " > " + QString(item->text()).remove('&'));
        }
    }
}

TEST(GeoMenus, EveryItemTheLanesAddHasALetterOfItsOwn)
{
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    QMainWindow main;
    GeoServices services;
    services.document = &document;
    services.interpreter = &interpreter;
    services.reference = &reference;
    services.surfaces = &surfaces;
    services.scratch = std::filesystem::temp_directory_path() / "katana-geo-menus-test";
    services.makeAction = [&main](katana::qt::Icon, const QString& text, const QString& tip,
                                  const QKeySequence& shortcut, const QString& name) {
        auto* action = new QAction(text, &main);
        action->setStatusTip(tip);
        action->setShortcut(shortcut);
        action->setObjectName(name);
        return action;
    };
    GeoWorkbench workbench(main, std::move(services));
    QMenu gis("&GIS");
    QMenu terrain("&Terrain");
    GeoMenus menus(gis, terrain);
    katana::qt::buildGeoMenus(menus, workbench);

    const auto* analysis = terrain.findChild<QMenu*>("terrainAnalysisMenu");
    ASSERT_NE(analysis, nullptr);
    EXPECT_EQ(analysis->actions().size(), 6);
    expectLetters(terrain, "Terrain");
    expectLetters(gis, "GIS");
}

} // namespace
