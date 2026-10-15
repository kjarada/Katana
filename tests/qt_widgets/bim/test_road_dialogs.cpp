// The BIM menu's road dialogs (src/katana_qt/bim/), driven by their object
// names as a person drives them by clicking and typing.
//
// None of them changes anything itself: each writes the ROADTEMPLATE or
// CORRIDOR line a person would type and hands it to the window's executor.
// So what is tested is each line - exactly the one a person would type -
// that the drawing then holds what it says, and that what a dialog shows is
// what the drawing holds. The executor here runs the line through the real
// CommandInterpreter on the same drawing, which is where the window's
// command line sends it.

#include <gtest/gtest.h>

#include <QAction>
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDir>
#include <QFile>
#include <QLabel>
#include <QLineEdit>
#include <QMainWindow>
#include <QMenu>
#include <QPushButton>
#include <QTableWidget>
#include <QTemporaryDir>
#include <QToolBar>

#include <memory>
#include <string>
#include <vector>

#include "bim/corridor_dialog.hpp"
#include "bim/corridor_quantities_dialog.hpp"
#include "bim/road_template_manager.hpp"
#include "bim/road_workbench.hpp"
#include "command_runner.hpp"
#include "katana/cad/bim/bim_object.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/road/corridor_model.hpp"
#include "katana/cad/road/road_template.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::qt::VerbOutcome;

namespace {

template <typename T> T* child(QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << "no " << name;
    return found;
}

void fill(QWidget& parent, const char* name, const QString& text)
{
    auto* field = child<QLineEdit>(parent, name);
    ASSERT_NE(field, nullptr);
    field->setText(text);
}

void choose(QWidget& parent, const char* name, const QString& text)
{
    auto* combo = child<QComboBox>(parent, name);
    ASSERT_NE(combo, nullptr);
    const int at = combo->findText(text);
    ASSERT_GE(at, 0) << name << " does not offer " << text.toStdString();
    combo->setCurrentIndex(at);
}

void click(QWidget& parent, const char* name)
{
    auto* button = child<QPushButton>(parent, name);
    ASSERT_NE(button, nullptr);
    ASSERT_TRUE(button->isEnabled()) << name << " is disabled";
    button->click();
    QApplication::processEvents();
}

QString label(QWidget& parent, const char* name)
{
    auto* found = child<QLabel>(parent, name);
    return found != nullptr ? found->text() : QString();
}

// The window's executor, as far as a road line goes: the interpreter on the
// same drawing. It keeps every line it was handed.
struct Executor {
    explicit Executor(Document& document) : interpreter(document) {}

    CommandInterpreter interpreter;
    std::vector<QString> lines;

    katana::qt::CommandRunner runner()
    {
        return [this](const QString& line) {
            lines.push_back(line);
            VerbOutcome outcome;
            const auto reply = interpreter.run(line.toStdString());
            outcome.ok = reply.ok();
            if (reply.ok()) {
                outcome.reply = QString::fromStdString(*reply);
            } else {
                outcome.error = QString::fromStdString(reply.error().describe());
            }
            return outcome;
        };
    }
    [[nodiscard]] std::string last() const
    {
        return lines.empty() ? std::string() : lines.back().toStdString();
    }
    void ok(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        ASSERT_TRUE(reply.ok()) << line << ": " << reply.error().describe();
    }
};

// A 100 m straight along +x with a level design at 10, and the standard
// templates.
void road(Executor& executor)
{
    executor.ok("ALIGN NEW road 0,0 100,0");
    executor.ok("ALIGN DESIGN road 0,10 100,10");
    executor.ok("ROADTEMPLATE DEFAULTS");
}

} // namespace

TEST(RoadTemplateManager, EachButtonRunsItsRoadTemplateLineAsOneStep)
{
    Document document;
    Executor executor(document);
    katana::qt::RoadTemplateManagerDialog dialog({&document, executor.runner()});
    EXPECT_EQ(child<QTableWidget>(dialog, "roadTemplateList")->rowCount(), 0);

    click(dialog, "roadTemplateDefaults");
    EXPECT_EQ(executor.last(), "ROADTEMPLATE DEFAULTS");
    EXPECT_EQ(child<QTableWidget>(dialog, "roadTemplateList")->rowCount(), 3);

    fill(dialog, "roadTemplateName", "my road");
    click(dialog, "roadTemplateNew");
    // A name with a blank is quoted, as a person would have to type it.
    EXPECT_EQ(executor.last(), "ROADTEMPLATE NEW \"my road\"");
    EXPECT_EQ(dialog.currentTemplate().toStdString(), "my road");

    choose(dialog, "roadTemplateAddSide", "both");
    choose(dialog, "roadTemplateAddKind", "lane");
    const std::size_t before = document.history().undoCount();
    click(dialog, "roadTemplateAdd");
    EXPECT_EQ(executor.last(), "ROADTEMPLATE ADD \"my road\" both lane");
    EXPECT_EQ(document.history().undoCount(), before + 1);
    auto* components = child<QTableWidget>(dialog, "roadTemplateComponents");
    ASSERT_EQ(components->rowCount(), 2);
    EXPECT_EQ(components->item(0, 0)->text().toStdString(), "left");
    EXPECT_EQ(components->item(0, 1)->text().toStdString(), "lane");

    // A grid is a buffer: an edit runs nothing until Apply.
    const std::size_t linesBefore = executor.lines.size();
    components->item(0, 3)->setText("3.25");
    QApplication::processEvents();
    EXPECT_EQ(executor.lines.size(), linesBefore);
    click(dialog, "roadTemplateApplyComponents");
    EXPECT_EQ(executor.last(),
              "ROADTEMPLATE SET \"my road\" left:lane width=3.25 crossfall=-3 layer=road/pavement");
    const auto* made = document.roadTemplates().find("my road");
    ASSERT_NE(made, nullptr);
    EXPECT_EQ(made->components[*made->find(katana::cad::road::Side::Left, "lane")].width, 3.25);
    EXPECT_EQ(made->components[*made->find(katana::cad::road::Side::Right, "lane")].width, 3.5);

    // A cell that is not one number is refused naming it, before any line.
    components = child<QTableWidget>(dialog, "roadTemplateComponents");
    components->item(1, 3)->setText("3,5");
    QApplication::processEvents();
    const std::size_t refusedBefore = executor.lines.size();
    click(dialog, "roadTemplateApplyComponents");
    EXPECT_EQ(executor.lines.size(), refusedBefore);
    EXPECT_TRUE(label(dialog, "roadTemplateStatus").contains("right:lane"))
        << label(dialog, "roadTemplateStatus").toStdString();
    click(dialog, "roadTemplateRevertComponents");

    // The chosen component's courses: a lane comes with three.
    components = child<QTableWidget>(dialog, "roadTemplateComponents");
    components->selectRow(0);
    QApplication::processEvents();
    EXPECT_EQ(child<QTableWidget>(dialog, "roadTemplateCourses")->rowCount(), 3);
    fill(dialog, "roadTemplateCourseName", "seal");
    fill(dialog, "roadTemplateCourseThickness", "0.01");
    fill(dialog, "roadTemplateCourseMaterial", "spray seal");
    click(dialog, "roadTemplateCourseAdd");
    EXPECT_EQ(executor.last(), "ROADTEMPLATE COURSE \"my road\" left:lane ADD seal thickness=0.01 "
                               "\"material=spray seal\"");
    EXPECT_EQ(child<QTableWidget>(dialog, "roadTemplateCourses")->rowCount(), 4);

    // Keys typed for the chosen component.
    fill(dialog, "roadTemplateComponentKeys", "superelevated=off");
    click(dialog, "roadTemplateSetKeys");
    EXPECT_EQ(executor.last(), "ROADTEMPLATE SET \"my road\" left:lane superelevated=off");
    made = document.roadTemplates().find("my road");
    EXPECT_FALSE(
        made->components[*made->find(katana::cad::road::Side::Left, "lane")].superelevated);

    click(dialog, "roadTemplateRemove");
    EXPECT_EQ(executor.last(), "ROADTEMPLATE REMOVE \"my road\" left:lane");
    EXPECT_EQ(child<QTableWidget>(dialog, "roadTemplateComponents")->rowCount(), 1);
}

TEST(RoadTemplateManager, ALineTypedElsewhereIsShownAndARefusalIsSaid)
{
    Document document;
    Executor executor(document);
    katana::qt::RoadTemplateManagerDialog dialog({&document, executor.runner()});
    executor.ok("ROADTEMPLATE NEW typed");
    QApplication::processEvents(); // the watcher's delivery
    QApplication::processEvents();
    EXPECT_EQ(child<QTableWidget>(dialog, "roadTemplateList")->rowCount(), 1);
    // Deleting a template a corridor uses is refused; the dialog says why.
    executor.ok("ROADTEMPLATE DEFAULTS");
    executor.ok("ALIGN NEW road 0,0 100,0");
    executor.ok("ALIGN DESIGN road 0,10 100,10");
    executor.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
    QApplication::processEvents();
    QApplication::processEvents();
    ASSERT_TRUE(dialog.selectTemplate("urban street"));
    click(dialog, "roadTemplateDelete");
    EXPECT_TRUE(label(dialog, "roadTemplateStatus").contains("corridor Main"))
        << label(dialog, "roadTemplateStatus").toStdString();
    EXPECT_NE(document.roadTemplates().find("urban street"), nullptr);
}

TEST(CorridorDialog, NewBuildsItsLineFromTheFieldsAndTheTablesFollowTheDrawing)
{
    Document document;
    Executor executor(document);
    road(executor);
    katana::qt::CorridorDialog dialog({&document, executor.runner()});
    fill(dialog, "corridorName", "Main");
    choose(dialog, "corridorAlignment", "road");
    choose(dialog, "corridorTemplate", "urban street");
    choose(dialog, "corridorSurface", "(none)");
    fill(dialog, "corridorInterval", "20");
    click(dialog, "corridorNew");
    EXPECT_EQ(executor.last(),
              "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\" interval=20");
    ASSERT_EQ(document.corridors().corridors.size(), 1u);
    EXPECT_EQ(child<QTableWidget>(dialog, "corridorList")->rowCount(), 1);
    EXPECT_EQ(dialog.currentCorridor().toStdString(), "Main");
    // With no target surface it builds the pavement and says why not more.
    EXPECT_TRUE(label(dialog, "corridorProblems").contains("no target surface"))
        << label(dialog, "corridorProblems").toStdString();

    fill(dialog, "corridorSuperStation", "30");
    fill(dialog, "corridorSuperLeft", "-3");
    fill(dialog, "corridorSuperRight", "-3");
    click(dialog, "corridorSuperAdd");
    EXPECT_EQ(executor.last(), "CORRIDOR SUPERELEVATION Main ADD 30 -3 -3");
    EXPECT_EQ(child<QTableWidget>(dialog, "corridorSuperelevation")->rowCount(), 1);

    fill(dialog, "corridorRegionFrom", "40");
    fill(dialog, "corridorRegionTo", "60");
    choose(dialog, "corridorRegionTemplate", "divided road");
    click(dialog, "corridorRegionAdd");
    EXPECT_EQ(executor.last(), "CORRIDOR REGION Main ADD 40 60 \"divided road\"");
    EXPECT_EQ(child<QTableWidget>(dialog, "corridorRegions")->rowCount(), 1);

    // Apply writes every key, an empty station clearing it.
    fill(dialog, "corridorFrom", "10");
    click(dialog, "corridorApply");
    EXPECT_EQ(executor.last(),
              "CORRIDOR SET Main alignment=road template=\"urban street\" surface= interval=20 "
              "from=10 to=");
    EXPECT_EQ(document.corridors().find("Main")->start, 10.0);

    // A field that is not one number refuses before any line runs.
    fill(dialog, "corridorInterval", "ten");
    const std::size_t before = executor.lines.size();
    click(dialog, "corridorApply");
    EXPECT_EQ(executor.lines.size(), before);
    EXPECT_TRUE(label(dialog, "corridorStatus").contains("interval"));

    click(dialog, "corridorStrings");
    EXPECT_EQ(executor.last(), "CORRIDOR STRINGS Main");
    EXPECT_GT(document.model().entities.size(), 0u);
}

TEST(CorridorQuantities, ListsEveryObjectsQuantitiesAndSavesThemAsCsv)
{
    Document document;
    Executor executor(document);
    road(executor);
    executor.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
    katana::qt::CorridorQuantitiesDialog dialog(
        {&document, executor.runner(), [] { return true; }});
    choose(dialog, "corridorQuantitiesCorridor", "Main");
    click(dialog, "corridorQuantitiesRun");
    EXPECT_EQ(executor.last(), "CORRIDOR QUANTITIES Main");
    const auto model = katana::cad::bim::bimModel(document);
    auto* table = child<QTableWidget>(dialog, "corridorQuantitiesTable");
    EXPECT_EQ(table->rowCount(), static_cast<int>(model->objects.size()));
    // No target surface: nothing was measured, which is not a road with 0.000
    // of cut (CLAUDE.md: absent is not zero).
    EXPECT_TRUE(label(dialog, "corridorQuantitiesTotals").startsWith("Cut and fill not measured"))
        << label(dialog, "corridorQuantitiesTotals").toStdString();

    // Browse opens no file dialog in a headless session; the path is typed.
    click(dialog, "corridorQuantitiesBrowse");
    EXPECT_TRUE(label(dialog, "corridorQuantitiesStatus").contains("headless"));
    QTemporaryDir folder;
    ASSERT_TRUE(folder.isValid());
    const QString path = QDir(folder.path()).filePath("quantities.csv");
    fill(dialog, "corridorQuantitiesCsvPath", path);
    click(dialog, "corridorQuantitiesSaveCsv");
    // The path as the line takes it - quoted only were it to hold a blank.
    EXPECT_TRUE(executor.last().starts_with("CORRIDOR QUANTITIES Main ")) << executor.last();
    EXPECT_NE(executor.last().find("csv="), std::string::npos) << executor.last();
    EXPECT_TRUE(QFile::exists(path));

    // A file already there is kept unless the box says to replace it: the line
    // is the one a person would type, with OVERWRITE only when asked for.
    ASSERT_TRUE(QFile::exists(path));
    QFile existing(path);
    ASSERT_TRUE(existing.open(QIODevice::WriteOnly | QIODevice::Truncate));
    existing.write("keep me\n");
    existing.close();
    click(dialog, "corridorQuantitiesSaveCsv");
    EXPECT_EQ(executor.last().find("OVERWRITE"), std::string::npos) << executor.last();
    EXPECT_TRUE(label(dialog, "corridorQuantitiesStatus").contains("already there"))
        << label(dialog, "corridorQuantitiesStatus").toStdString();
    QFile kept(path);
    ASSERT_TRUE(kept.open(QIODevice::ReadOnly));
    EXPECT_EQ(kept.readAll(), QByteArray("keep me\n"));
    kept.close();
    child<QCheckBox>(dialog, "corridorQuantitiesOverwrite")->setChecked(true);
    click(dialog, "corridorQuantitiesSaveCsv");
    EXPECT_NE(executor.last().find(" OVERWRITE"), std::string::npos) << executor.last();
    ASSERT_TRUE(kept.open(QIODevice::ReadOnly));
    EXPECT_TRUE(kept.readAll().startsWith("id,name,kind,"));
}

TEST(RoadWorkbench, TheBimMenuHasItsSectionsAndTheRoadModelGoesToThe3dViews)
{
    Document document;
    Executor executor(document);
    QMainWindow window;
    QMenu menu;
    std::vector<QString> logged;
    std::vector<std::size_t> handed;
    katana::qt::RoadServices services;
    services.document = &document;
    services.makeAction = [&window](katana::qt::Icon, const QString& text, const QString& tip,
                                    const QKeySequence& shortcut, const QString& name) {
        auto* action = new QAction(text, &window);
        action->setObjectName(name);
        action->setStatusTip(tip);
        action->setShortcut(shortcut);
        return action;
    };
    services.log = [&logged](const QString& text, bool) { logged.push_back(text); };
    services.run = executor.runner();
    services.headless = [] { return true; };
    services.setRoadMeshes = [&handed](std::vector<katana::cad::SceneMesh> meshes) {
        handed.push_back(meshes.size());
    };
    katana::qt::RoadWorkbench workbench(window, std::move(services), menu);
    QStringList names;
    for (QAction* action : menu.actions()) {
        if (!action->isSeparator()) {
            names << action->objectName();
        }
    }
    EXPECT_EQ(names, (QStringList{"bimRoadTemplates", "bimCorridors", "bimShowRoadModel",
                                  "bimCorridorQuantities"}));
    QToolBar bar;
    workbench.fillToolBar(bar);
    EXPECT_TRUE(bar.actions().contains(workbench.corridorsAction()));

    // No alignment with a design profile: the corridor items say where one is
    // made, and open nothing.
    workbench.corridorsAction()->trigger();
    ASSERT_FALSE(logged.empty());
    EXPECT_TRUE(logged.back().startsWith("No alignment has a design profile"));

    // A corridor's objects reach the 3D views, every one on a layer shown.
    road(executor);
    executor.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
    workbench.syncViews();
    ASSERT_FALSE(handed.empty());
    EXPECT_EQ(handed.back(), katana::cad::bim::bimModel(document)->objects.size());
    // Its kerbs' layer switched off: their two meshes are not handed over.
    executor.ok("LAYER HIDE road/kerb");
    workbench.syncViews();
    EXPECT_EQ(handed.back(), katana::cad::bim::bimModel(document)->objects.size() - 2);
    // Show Road Model off: none.
    for (QAction* action : menu.actions()) {
        if (action->objectName() == "bimShowRoadModel") {
            action->setChecked(false);
        }
    }
    EXPECT_EQ(handed.back(), 0u);
    for (QAction* action : menu.actions()) {
        if (action->objectName() == "bimShowRoadModel") {
            action->setChecked(true); // the session's switch, back as it was
        }
    }
}
