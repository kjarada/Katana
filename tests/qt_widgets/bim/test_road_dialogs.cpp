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
#include <QDialog>
#include <QDir>
#include <QFile>
#include <QHeaderView>
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
#include "katana/geometry/mesh.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "view_workspace.hpp"
#include "widget_harness.hpp"

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

// The drawing with surfaces of these names, each a tilted square that covers
// the 100 m road.
void addSurfaces(Document& document, const std::vector<std::string>& names)
{
    auto surface = katana::terrain::TinSurface::create(
        {{-10, -50, 1}, {110, -50, 2}, {110, 50, 3}, {-10, 50, 2}}, {{0, 1, 2}, {0, 2, 3}});
    ASSERT_TRUE(surface.ok());
    const auto shared = std::make_shared<const katana::terrain::TinSurface>(*surface);
    katana::terrain::SurfaceStore store = document.surfaces();
    for (const std::string& name : names) {
        ASSERT_TRUE(store.add({name, shared, "test"}).ok());
    }
    ASSERT_TRUE(document.setSurfaces(std::move(store), "TEST").ok());
}

// The watcher delivers from the event loop, one turn after the change.
void settle()
{
    QApplication::processEvents();
    QApplication::processEvents();
}

QString textOf(QWidget& parent, const char* name)
{
    auto* field = child<QLineEdit>(parent, name);
    return field != nullptr ? field->text() : QString();
}

QString shown(QWidget& parent, const char* name)
{
    auto* combo = child<QComboBox>(parent, name);
    return combo != nullptr ? combo->currentText() : QString();
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

namespace {

// The row of the components grid for side:name; -1 when it has none.
int componentRow(QTableWidget& grid, const char* side, const char* name)
{
    for (int row = 0; row < grid.rowCount(); ++row) {
        if (grid.item(row, 0)->text() == side && grid.item(row, 1)->text() == name) {
            return row;
        }
    }
    return -1;
}

constexpr int kWidthColumn = 3;     // the components grid: Width (m)
constexpr int kThicknessColumn = 1; // the courses grid: Thickness (m)

double laneWidth(const Document& document, const char* name, katana::cad::road::Side side)
{
    const auto* roadTemplate = document.roadTemplates().find(name);
    return roadTemplate->components[*roadTemplate->find(side, "lane")].width;
}

double courseThickness(const Document& document, const char* name, const char* course)
{
    const auto* roadTemplate = document.roadTemplates().find(name);
    const auto& lane =
        roadTemplate->components[*roadTemplate->find(katana::cad::road::Side::Left, "lane")];
    for (const auto& each : lane.courses) {
        if (each.name == course) {
            return each.thickness;
        }
    }
    return -1.0;
}

} // namespace

// One user edit is one undo step (docs/desktop.md): an Apply over several
// rows is ONE line, ROADTEMPLATE SET with a group for each row.
TEST(RoadTemplateManager, AnApplyOfSeveralRowsIsOneLineAndOneUndoStep)
{
    using katana::cad::road::Side;
    Document document;
    Executor executor(document);
    katana::qt::RoadTemplateManagerDialog dialog({&document, executor.runner()});
    click(dialog, "roadTemplateDefaults");
    ASSERT_TRUE(dialog.selectTemplate("rural two-lane"));
    auto* components = child<QTableWidget>(dialog, "roadTemplateComponents");
    const int left = componentRow(*components, "left", "lane");
    const int right = componentRow(*components, "right", "lane");
    const int shoulder = componentRow(*components, "right", "shoulder");
    ASSERT_GE(left, 0);
    ASSERT_GE(right, 0);
    ASSERT_GE(shoulder, 0);
    // The standard rural two-lane: 3.5 m lanes and 2 m shoulders.
    ASSERT_EQ(laneWidth(document, "rural two-lane", Side::Left), 3.5);

    components->item(left, kWidthColumn)->setText("3.25");
    components->item(right, kWidthColumn)->setText("3.75");
    components->item(shoulder, kWidthColumn)->setText("2.5");
    QApplication::processEvents();
    const std::size_t lines = executor.lines.size();
    const std::size_t steps = document.history().undoCount();
    click(dialog, "roadTemplateApplyComponents");
    ASSERT_EQ(executor.lines.size(), lines + 1) << "one line for the grid, not one a row";
    const std::string line = executor.last();
    EXPECT_TRUE(line.starts_with("ROADTEMPLATE SET \"rural two-lane\" left:lane width=3.25 "))
        << line;
    EXPECT_NE(line.find(" right:lane width=3.75 "), std::string::npos) << line;
    EXPECT_NE(line.find(" right:shoulder width=2.5 "), std::string::npos) << line;
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_EQ(laneWidth(document, "rural two-lane", Side::Left), 3.25);
    EXPECT_EQ(laneWidth(document, "rural two-lane", Side::Right), 3.75);

    // One Ctrl+Z takes all three back.
    executor.ok("UNDO");
    EXPECT_EQ(laneWidth(document, "rural two-lane", Side::Left), 3.5);
    EXPECT_EQ(laneWidth(document, "rural two-lane", Side::Right), 3.5);
    EXPECT_EQ(document.history().undoCount(), steps);
}

TEST(RoadTemplateManager, ARefusedApplyChangesNothingAndKeepsTheEditsToCorrect)
{
    using katana::cad::road::Side;
    Document document;
    Executor executor(document);
    katana::qt::RoadTemplateManagerDialog dialog({&document, executor.runner()});
    click(dialog, "roadTemplateDefaults");
    ASSERT_TRUE(dialog.selectTemplate("rural two-lane"));
    auto* components = child<QTableWidget>(dialog, "roadTemplateComponents");
    const int left = componentRow(*components, "left", "lane");
    const int right = componentRow(*components, "right", "lane");
    ASSERT_GE(left, 0);
    ASSERT_GE(right, 0);
    const std::size_t steps = document.history().undoCount();

    // A width of 0 is a number, so the dialog lets it through; the template
    // refuses it.
    components->item(left, kWidthColumn)->setText("3.25");
    components->item(right, kWidthColumn)->setText("0");
    QApplication::processEvents();
    click(dialog, "roadTemplateApplyComponents");
    EXPECT_TRUE(label(dialog, "roadTemplateStatus").contains("positive"))
        << label(dialog, "roadTemplateStatus").toStdString();
    EXPECT_EQ(laneWidth(document, "rural two-lane", Side::Left), 3.5)
        << "the row before the refused one was applied";
    EXPECT_EQ(laneWidth(document, "rural two-lane", Side::Right), 3.5);
    EXPECT_EQ(document.history().undoCount(), steps);
    // The person's edits are still in the grid to correct.
    components = child<QTableWidget>(dialog, "roadTemplateComponents");
    EXPECT_EQ(components->item(left, kWidthColumn)->text().toStdString(), "3.25");
    EXPECT_EQ(components->item(right, kWidthColumn)->text().toStdString(), "0");

    components->item(right, kWidthColumn)->setText("3.75");
    QApplication::processEvents();
    click(dialog, "roadTemplateApplyComponents");
    EXPECT_EQ(laneWidth(document, "rural two-lane", Side::Left), 3.25);
    EXPECT_EQ(laneWidth(document, "rural two-lane", Side::Right), 3.75);
    EXPECT_EQ(document.history().undoCount(), steps + 1);
}

TEST(RoadTemplateManager, AnApplyOfSeveralCoursesIsOneLineAndARefusedThicknessChangesNone)
{
    Document document;
    Executor executor(document);
    katana::qt::RoadTemplateManagerDialog dialog({&document, executor.runner()});
    click(dialog, "roadTemplateDefaults");
    ASSERT_TRUE(dialog.selectTemplate("rural two-lane"));
    auto* components = child<QTableWidget>(dialog, "roadTemplateComponents");
    components->selectRow(componentRow(*components, "left", "lane"));
    QApplication::processEvents();
    auto* courses = child<QTableWidget>(dialog, "roadTemplateCourses");
    // A lane's pavement: 50 mm of AC14, 200 mm of base, 250 mm of subbase.
    ASSERT_EQ(courses->rowCount(), 3);
    ASSERT_EQ(courses->item(0, 0)->text().toStdString(), "AC14");
    ASSERT_EQ(courses->item(1, 0)->text().toStdString(), "base");
    const std::size_t steps = document.history().undoCount();

    courses->item(0, kThicknessColumn)->setText("0.06");
    courses->item(1, kThicknessColumn)->setText("0");
    QApplication::processEvents();
    const std::size_t lines = executor.lines.size();
    click(dialog, "roadTemplateApplyCourses");
    ASSERT_EQ(executor.lines.size(), lines + 1) << "one line for the grid, not one a row";
    EXPECT_TRUE(label(dialog, "roadTemplateStatus").contains("positive"))
        << label(dialog, "roadTemplateStatus").toStdString();
    EXPECT_EQ(courseThickness(document, "rural two-lane", "AC14"), 0.05)
        << "the course before the refused one was applied";
    EXPECT_EQ(document.history().undoCount(), steps);
    courses = child<QTableWidget>(dialog, "roadTemplateCourses");
    EXPECT_EQ(courses->item(0, kThicknessColumn)->text().toStdString(), "0.06");
    EXPECT_EQ(courses->item(1, kThicknessColumn)->text().toStdString(), "0");

    courses->item(1, kThicknessColumn)->setText("0.3");
    QApplication::processEvents();
    click(dialog, "roadTemplateApplyCourses");
    EXPECT_EQ(executor.last(),
              "ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET AC14 thickness=0.06 "
              "material=asphalt ifc=PAVEMENT base thickness=0.3 \"material=crushed rock\" "
              "ifc=PAVEMENT");
    EXPECT_EQ(courseThickness(document, "rural two-lane", "AC14"), 0.06);
    EXPECT_EQ(courseThickness(document, "rural two-lane", "base"), 0.3);
    EXPECT_EQ(document.history().undoCount(), steps + 1);
}

TEST(RoadTemplateManager, PendingCourseEditsAreDroppedWhenAnotherTemplateIsChosen)
{
    Document document;
    Executor executor(document);
    katana::qt::RoadTemplateManagerDialog dialog({&document, executor.runner()});
    click(dialog, "roadTemplateDefaults");
    ASSERT_TRUE(dialog.selectTemplate("rural two-lane"));
    auto* components = child<QTableWidget>(dialog, "roadTemplateComponents");
    components->selectRow(componentRow(*components, "left", "lane"));
    QApplication::processEvents();
    auto* courses = child<QTableWidget>(dialog, "roadTemplateCourses");
    ASSERT_EQ(courses->item(0, 0)->text().toStdString(), "AC14");
    ASSERT_EQ(courses->item(0, kThicknessColumn)->text().toStdString(), "0.05");
    courses->item(0, kThicknessColumn)->setText("0.1");
    QApplication::processEvents();

    // The urban street has a left:lane with the same courses.
    ASSERT_TRUE(dialog.selectTemplate("urban street"));
    QApplication::processEvents();
    courses = child<QTableWidget>(dialog, "roadTemplateCourses");
    ASSERT_GT(courses->rowCount(), 0);
    EXPECT_EQ(courses->item(0, kThicknessColumn)->text().toStdString(), "0.05")
        << "the grid shows rural two-lane's edit under urban street";
    const std::size_t lines = executor.lines.size();
    click(dialog, "roadTemplateApplyCourses");
    EXPECT_EQ(executor.lines.size(), lines) << "there is nothing edited in urban street";
    EXPECT_TRUE(label(dialog, "roadTemplateStatus").contains("Nothing is edited"))
        << label(dialog, "roadTemplateStatus").toStdString();
    EXPECT_EQ(courseThickness(document, "urban street", "AC14"), 0.05);
    EXPECT_EQ(courseThickness(document, "rural two-lane", "AC14"), 0.05);
}

TEST(RoadTemplateManager, CourseEditsSurviveAnUnrelatedLineButNotAChangeToTheComponent)
{
    Document document;
    Executor executor(document);
    katana::qt::RoadTemplateManagerDialog dialog({&document, executor.runner()});
    click(dialog, "roadTemplateDefaults");
    ASSERT_TRUE(dialog.selectTemplate("rural two-lane"));
    auto* components = child<QTableWidget>(dialog, "roadTemplateComponents");
    components->selectRow(componentRow(*components, "left", "lane"));
    QApplication::processEvents();
    auto* courses = child<QTableWidget>(dialog, "roadTemplateCourses");
    courses->item(0, kThicknessColumn)->setText("0.1");
    QApplication::processEvents();

    // A line that touches no template: the edit stays, and Apply sends it.
    executor.ok("LINE 0,0 10,10");
    settle();
    courses = child<QTableWidget>(dialog, "roadTemplateCourses");
    EXPECT_EQ(courses->item(0, kThicknessColumn)->text().toStdString(), "0.1");
    click(dialog, "roadTemplateApplyCourses");
    EXPECT_EQ(executor.last(),
              "ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET AC14 thickness=0.1 "
              "material=asphalt ifc=PAVEMENT");
    EXPECT_EQ(courseThickness(document, "rural two-lane", "AC14"), 0.1);

    // A line that changes the component's courses: the grid shows what the
    // drawing holds, and the edit that was pending is gone.
    courses = child<QTableWidget>(dialog, "roadTemplateCourses");
    courses->item(0, kThicknessColumn)->setText("0.2");
    QApplication::processEvents();
    executor.ok("ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET base thickness=0.3");
    settle();
    courses = child<QTableWidget>(dialog, "roadTemplateCourses");
    EXPECT_EQ(courses->item(0, kThicknessColumn)->text().toStdString(), "0.1");
    EXPECT_EQ(courses->item(1, kThicknessColumn)->text().toStdString(), "0.3");
    const std::size_t lines = executor.lines.size();
    click(dialog, "roadTemplateApplyCourses");
    EXPECT_EQ(executor.lines.size(), lines) << "nothing is edited now";
}

TEST(RoadTemplateManager, BothGridsKeepRoomForSeveralRowsAtTheOpeningSize)
{
    Document document;
    Executor executor(document);
    katana::qt::RoadTemplateManagerDialog dialog({&document, executor.runner()});
    click(dialog, "roadTemplateDefaults");
    ASSERT_TRUE(dialog.selectTemplate("urban street"));
    dialog.show();
    QApplication::processEvents();
    for (const char* name : {"roadTemplateComponents", "roadTemplateCourses"}) {
        auto* grid = child<QTableWidget>(dialog, name);
        ASSERT_NE(grid, nullptr);
        // Four rows below the header, as the finding saw one: the viewport is
        // the area the rows are drawn in.
        EXPECT_GE(grid->viewport()->height(), 4 * grid->verticalHeader()->defaultSectionSize())
            << name << " is " << grid->viewport()->height() << " px tall";
    }
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

// The verbs find a template and a surface ignoring case and store the name as
// typed (docs/road_design.md), so a corridor may hold "URBAN STREET" and
// GROUND while the drawing has "urban street" and "ground".
TEST(CorridorDialog, ApplyKeepsTheTemplateAndSurfaceACorridorNamesInAnotherCase)
{
    Document document;
    Executor executor(document);
    road(executor);
    addSurfaces(document, {"ground"});
    executor.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"URBAN STREET\" surface=GROUND");
    ASSERT_EQ(document.corridors().find("Main")->templateName, "URBAN STREET");
    ASSERT_EQ(document.corridors().find("Main")->surface, "GROUND");
    ASSERT_EQ(document.corridors().find("Main")->interval, 10.0);

    katana::qt::CorridorDialog dialog({&document, executor.runner()});
    ASSERT_EQ(dialog.currentCorridor().toStdString(), "Main");
    // The combos show the drawing's own item for it, not the first template
    // (rural two-lane) and not (none), which is what a non-editable combo
    // falls back to when setCurrentText finds no exact match.
    EXPECT_EQ(shown(dialog, "corridorTemplate").toStdString(), "urban street");
    EXPECT_EQ(shown(dialog, "corridorSurface").toStdString(), "ground");

    fill(dialog, "corridorInterval", "20");
    click(dialog, "corridorApply");
    // Written as the corridor holds them: the same names, in the same case.
    EXPECT_EQ(executor.last(),
              "CORRIDOR SET Main alignment=road template=\"URBAN STREET\" surface=GROUND "
              "interval=20 from= to=");
    const auto* main = document.corridors().find("Main");
    ASSERT_NE(main, nullptr);
    EXPECT_EQ(main->templateName, "URBAN STREET");
    EXPECT_EQ(main->surface, "GROUND");
    EXPECT_EQ(main->interval, 20.0);
    // The target surface is still found, so the earthworks are still built.
    EXPECT_FALSE(label(dialog, "corridorProblems").contains("no target surface"))
        << label(dialog, "corridorProblems").toStdString();
}

TEST(CorridorDialog, AChosenCorridorNamesWhatTheDrawingLacksSoTheComboShowsThatAndNotTheLastOne)
{
    Document document;
    Executor executor(document);
    road(executor);
    addSurfaces(document, {"ground", "old"});
    executor.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\" surface=ground");
    executor.ok("CORRIDOR NEW Second ALIGNMENT road TEMPLATE \"rural two-lane\" surface=old");
    katana::terrain::SurfaceStore store = document.surfaces();
    ASSERT_TRUE(store.remove("old"));
    ASSERT_TRUE(document.setSurfaces(std::move(store), "TEST").ok());

    katana::qt::CorridorDialog dialog({&document, executor.runner()});
    ASSERT_TRUE(dialog.selectCorridor("Main"));
    EXPECT_EQ(shown(dialog, "corridorSurface").toStdString(), "ground");
    ASSERT_TRUE(dialog.selectCorridor("Second"));
    // Not Main's ground, which was showing: the surface Second names.
    EXPECT_EQ(shown(dialog, "corridorSurface").toStdString(), "old");
    EXPECT_EQ(shown(dialog, "corridorTemplate").toStdString(), "rural two-lane");

    // Apply writes what the corridor holds, which the drawing refuses to keep
    // as it has no such surface: said, and nothing re-pointed.
    click(dialog, "corridorApply");
    EXPECT_EQ(executor.last(),
              "CORRIDOR SET Second alignment=road template=\"rural two-lane\" surface=old "
              "interval=10 from= to=");
    EXPECT_TRUE(label(dialog, "corridorStatus").contains("no surface old"))
        << label(dialog, "corridorStatus").toStdString();
    EXPECT_EQ(document.corridors().find("Second")->surface, "old");
    EXPECT_EQ(document.corridors().find("Main")->surface, "ground");

    // The person can say what it should be: (none) clears the target.
    choose(dialog, "corridorSurface", "(none)");
    click(dialog, "corridorApply");
    EXPECT_EQ(document.corridors().find("Second")->surface, "");
    EXPECT_EQ(document.corridors().find("Main")->surface, "ground");
}

TEST(CorridorDialog, NewLeavesTheNewCorridorChosenAndItsButtonsActOnIt)
{
    Document document;
    Executor executor(document);
    road(executor);
    executor.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
    katana::qt::CorridorDialog dialog({&document, executor.runner()});
    // Main is chosen when the dialog opens: the case the first test of this
    // dialog (with no corridor yet) never reached.
    ASSERT_EQ(dialog.currentCorridor().toStdString(), "Main");

    fill(dialog, "corridorName", "Second");
    choose(dialog, "corridorTemplate", "rural two-lane");
    fill(dialog, "corridorInterval", "20");
    click(dialog, "corridorNew");
    EXPECT_EQ(executor.last(),
              "CORRIDOR NEW Second ALIGNMENT road TEMPLATE \"rural two-lane\" interval=20");
    ASSERT_NE(document.corridors().find("Second"), nullptr);
    EXPECT_EQ(dialog.currentCorridor().toStdString(), "Second");
    EXPECT_EQ(textOf(dialog, "corridorName").toStdString(), "Second");
    EXPECT_EQ(textOf(dialog, "corridorInterval").toStdString(), "20");
    EXPECT_EQ(shown(dialog, "corridorTemplate").toStdString(), "rural two-lane");

    click(dialog, "corridorBuild");
    EXPECT_EQ(executor.last(), "CORRIDOR BUILD Second");
    click(dialog, "corridorDelete");
    EXPECT_EQ(executor.last(), "CORRIDOR DELETE Second");
    EXPECT_EQ(document.corridors().find("Second"), nullptr);
    EXPECT_NE(document.corridors().find("Main"), nullptr);
}

TEST(CorridorDialog, RenameLeavesTheRenamedCorridorChosen)
{
    Document document;
    Executor executor(document);
    road(executor);
    executor.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
    executor.ok("CORRIDOR NEW Other ALIGNMENT road TEMPLATE \"rural two-lane\"");
    katana::qt::CorridorDialog dialog({&document, executor.runner()});
    ASSERT_TRUE(dialog.selectCorridor("Main"));
    fill(dialog, "corridorName", "Renamed");
    click(dialog, "corridorRename");
    EXPECT_EQ(executor.last(), "CORRIDOR RENAME Main Renamed");
    EXPECT_EQ(dialog.currentCorridor().toStdString(), "Renamed");
    EXPECT_EQ(shown(dialog, "corridorTemplate").toStdString(), "urban street");
    click(dialog, "corridorBuild");
    EXPECT_EQ(executor.last(), "CORRIDOR BUILD Renamed");
}

TEST(CorridorDialog, TypedEditsSurviveAnUnrelatedCommandAndAChangeToTheCorridorIsShown)
{
    Document document;
    Executor executor(document);
    road(executor);
    executor.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
    katana::qt::CorridorDialog dialog({&document, executor.runner()});
    ASSERT_EQ(dialog.currentCorridor().toStdString(), "Main");

    fill(dialog, "corridorInterval", "25");
    fill(dialog, "corridorFrom", "5");
    choose(dialog, "corridorTemplate", "divided road");
    // Any command that changes the model reloads the dialog.
    executor.ok("LINE 0,0 10,10");
    settle();
    EXPECT_EQ(textOf(dialog, "corridorInterval").toStdString(), "25");
    EXPECT_EQ(textOf(dialog, "corridorFrom").toStdString(), "5");
    EXPECT_EQ(shown(dialog, "corridorTemplate").toStdString(), "divided road");
    click(dialog, "corridorApply");
    EXPECT_EQ(executor.last(),
              "CORRIDOR SET Main alignment=road template=\"divided road\" surface= interval=25 "
              "from=5 to=");
    EXPECT_EQ(document.corridors().find("Main")->interval, 25.0);

    // A change to the chosen corridor itself - here a typed line, as an undo
    // or a redo is - shows what the drawing holds, whatever was typed.
    fill(dialog, "corridorInterval", "33");
    executor.ok("CORRIDOR SET Main interval=40");
    settle();
    EXPECT_EQ(textOf(dialog, "corridorInterval").toStdString(), "40");
    executor.ok("UNDO");
    settle();
    EXPECT_EQ(textOf(dialog, "corridorInterval").toStdString(), "25");
    EXPECT_EQ(textOf(dialog, "corridorFrom").toStdString(), "5");
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

// "Shows the DOCUMENT's state after an edit, an undo, a redo, New and Open"
// (docs/desktop.md): a listing of a road model the drawing no longer has is
// not left on the screen, nor handed out by Copy.
TEST(CorridorQuantities, TheListingIsClearedWhenTheRoadModelChangesUnderIt)
{
    Document document;
    Executor executor(document);
    road(executor);
    executor.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
    katana::qt::CorridorQuantitiesDialog dialog(
        {&document, executor.runner(), [] { return true; }});
    auto* table = child<QTableWidget>(dialog, "corridorQuantitiesTable");
    const auto listed = [&] {
        choose(dialog, "corridorQuantitiesCorridor", "Main");
        click(dialog, "corridorQuantitiesRun");
    };

    // Nothing listed yet: a change has nothing to clear and says nothing.
    executor.ok("LINE 0,50 10,50");
    settle();
    EXPECT_TRUE(label(dialog, "corridorQuantitiesStatus").isEmpty());

    listed();
    ASSERT_GT(table->rowCount(), 0);
    ASSERT_FALSE(label(dialog, "corridorQuantitiesTotals").isEmpty());
    EXPECT_TRUE(label(dialog, "corridorQuantitiesTotals").contains("asphalt"))
        << label(dialog, "corridorQuantitiesTotals").toStdString();

    // A line drawn leaves the road model as it was: the listing stays.
    executor.ok("LINE 0,60 10,60");
    settle();
    EXPECT_GT(table->rowCount(), 0);
    EXPECT_FALSE(label(dialog, "corridorQuantitiesTotals").isEmpty());

    // A template edit changes it: the listing is cleared, and the status says
    // to list again.
    executor.ok("ROADTEMPLATE COURSE \"urban street\" left:lane SET AC14 thickness=0.5");
    settle();
    EXPECT_EQ(table->rowCount(), 0);
    EXPECT_TRUE(label(dialog, "corridorQuantitiesTotals").isEmpty());
    EXPECT_TRUE(label(dialog, "corridorQuantitiesStatus").contains("List Quantities again"))
        << label(dialog, "corridorQuantitiesStatus").toStdString();

    // Listed again it shows the drawing as it is now; an undo is a change too,
    // and Copy - pressed before the watcher has delivered - hands out nothing.
    listed();
    ASSERT_GT(table->rowCount(), 0);
    executor.ok("UNDO");
    click(dialog, "corridorQuantitiesCopy");
    EXPECT_TRUE(label(dialog, "corridorQuantitiesStatus").contains("List Quantities again"))
        << label(dialog, "corridorQuantitiesStatus").toStdString();
    EXPECT_EQ(table->rowCount(), 0);

    // A new drawing has no corridor: the listing of the old one goes.
    listed();
    ASSERT_GT(table->rowCount(), 0);
    executor.ok("NEW");
    settle();
    EXPECT_EQ(table->rowCount(), 0);
    EXPECT_TRUE(label(dialog, "corridorQuantitiesTotals").isEmpty());
    EXPECT_TRUE(label(dialog, "corridorQuantitiesStatus").contains("List Quantities again"));
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

// "A view that hides a layer of its own hides its objects there" (docs/
// road_design.md): the plan view's painter does, and the 3D views have to as
// well, though the window hands them all one list of meshes.
TEST(RoadWorkbench, AThreeDViewThatHidesALayerOfItsOwnDoesNotDrawTheRoadObjectsOnIt)
{
    using katana::cad::SceneMesh;
    using katana::cad::ViewKind;
    Document document;
    QMainWindow window;
    auto* views = new katana::qt::ViewWorkspace(document, &window);
    window.setCentralWidget(views);
    window.resize(900, 700);
    window.show();
    katana::qt::test::processEvents();

    // Two objects of the road model, a triangle each and the same shape, on
    // two layers: the pavement's and the kerb's.
    katana::geometry::TriangleMesh pavement;
    pavement.vertices = {{0, 0, 0}, {10, 0, 0}, {0, 10, 0}};
    pavement.faces = {{0, 1, 2}};
    katana::geometry::TriangleMesh kerb;
    kerb.vertices = {{20, 0, 0}, {30, 0, 0}, {20, 10, 0}};
    kerb.faces = {{0, 1, 2}};
    std::vector<SceneMesh> meshes(2);
    meshes[0].name = "pavement";
    meshes[0].mesh = &pavement;
    meshes[0].layer = "road/pavement";
    meshes[1].name = "kerb";
    meshes[1].mesh = &kerb;
    meshes[1].layer = "road/kerb";
    views->setMeshes(&meshes);

    const auto first = views->openView(ViewKind::Model3D).id;
    const auto second = views->openView(ViewKind::Model3D).id;
    katana::qt::test::processEvents();
    katana::qt::RenderViewWidget* hiding = views->renderView(first);
    katana::qt::RenderViewWidget* other = views->renderView(second);
    ASSERT_NE(hiding, nullptr);
    ASSERT_NE(other, nullptr);
    katana::qt::test::paint(*hiding);
    katana::qt::test::paint(*other);
    const std::size_t both = other->sceneLayers().terrain.triangles.size();
    ASSERT_GT(both, 0u) << "the objects were never drawn";
    ASSERT_EQ(both % 2, 0u) << "two meshes of one triangle each draw the same number";
    ASSERT_EQ(hiding->sceneLayers().terrain.triangles.size(), both);

    // The kerb's layer hidden in the first view only (VIEWS HIDE, or its
    // Layers popup): that view draws the pavement alone, the other both.
    hiding->state().layers.hide("road/kerb");
    views->viewSettingsChanged(first);
    katana::qt::test::paint(*hiding);
    katana::qt::test::paint(*other);
    EXPECT_EQ(hiding->sceneLayers().terrain.triangles.size(), both / 2);
    EXPECT_EQ(other->sceneLayers().terrain.triangles.size(), both);

    // A parent hides what is under it, as in plan; and showing it again
    // brings both back.
    hiding->state().layers.clear();
    hiding->state().layers.hide("road");
    views->viewSettingsChanged(first);
    katana::qt::test::paint(*hiding);
    EXPECT_EQ(hiding->sceneLayers().terrain.triangles.size(), 0u);
    hiding->state().layers.clear();
    views->viewSettingsChanged(first);
    katana::qt::test::paint(*hiding);
    EXPECT_EQ(hiding->sceneLayers().terrain.triangles.size(), both);
}

// The dialogs' precondition - an alignment with a design profile - is for
// MAKING a corridor. A drawing that has one still has something to show.
TEST(RoadWorkbench, TheCorridorDialogsOpenForCorridorsWhoseAlignmentLostItsProfile)
{
    Document document;
    Executor executor(document);
    QMainWindow window;
    QMenu menu;
    std::vector<QString> logged;
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
    services.setRoadMeshes = [](std::vector<katana::cad::SceneMesh>) {};
    katana::qt::RoadWorkbench workbench(window, std::move(services), menu);

    road(executor);
    executor.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
    executor.ok("ALIGN CLEARPROFILE road");
    ASSERT_FALSE(document.model().alignments.find("road")->vertical.has_value());
    logged.clear();

    workbench.corridorsAction()->trigger();
    QApplication::processEvents();
    auto* corridors = window.findChild<QDialog*>("corridorDialog");
    ASSERT_NE(corridors, nullptr) << "the Corridors dialog did not open";
    EXPECT_TRUE(corridors->isVisible());
    // It shows the corridor and what is wrong with it.
    EXPECT_EQ(child<QTableWidget>(*corridors, "corridorList")->rowCount(), 1);
    EXPECT_TRUE(label(*corridors, "corridorProblems").contains("no design profile"))
        << label(*corridors, "corridorProblems").toStdString();
    // ... and the item itself said nothing about where a profile is made.
    for (const QString& text : logged) {
        EXPECT_FALSE(text.startsWith("No alignment has a design profile")) << text.toStdString();
    }

    workbench.quantitiesAction()->trigger();
    QApplication::processEvents();
    EXPECT_NE(window.findChild<QDialog*>("corridorQuantitiesDialog"), nullptr);

    // No corridor and no profile: as before, nothing opens and it says why.
    executor.ok("CORRIDOR DELETE Main");
    logged.clear();
    workbench.corridorsAction()->trigger();
    ASSERT_FALSE(logged.empty());
    EXPECT_TRUE(logged.back().startsWith("No alignment has a design profile"));
}

// The curve tolerance (docs/road_design.md, "Building a corridor") is a field of
// the definition, in the buffer with the others: said in the line only when it
// is not what the corridor has, so the lines stay the short ones they were.
TEST(CorridorDialog, TheCurveToleranceIsInTheLineOnlyWhenItIsNotWhatTheCorridorHolds)
{
    Document document;
    Executor executor(document);
    road(executor);
    katana::qt::CorridorDialog dialog({&document, executor.runner()});
    EXPECT_EQ(textOf(dialog, "corridorCurveTolerance").toStdString(), "0.001");

    fill(dialog, "corridorName", "Main");
    choose(dialog, "corridorAlignment", "road");
    choose(dialog, "corridorTemplate", "urban street");
    choose(dialog, "corridorSurface", "(none)");
    click(dialog, "corridorNew");
    EXPECT_EQ(executor.last(), "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\" "
                               "interval=10")
        << "the default is not said";
    EXPECT_EQ(document.corridors().find("Main")->curveTolerance, 0.001);

    // A new one with another tolerance says it, after the interval.
    fill(dialog, "corridorName", "Second");
    fill(dialog, "corridorCurveTolerance", "0.005");
    click(dialog, "corridorNew");
    EXPECT_EQ(executor.last(), "CORRIDOR NEW Second ALIGNMENT road TEMPLATE \"urban street\" "
                               "interval=10 curvetol=0.005");
    ASSERT_NE(document.corridors().find("Second"), nullptr);
    EXPECT_EQ(document.corridors().find("Second")->curveTolerance, 0.005);
    ASSERT_EQ(dialog.currentCorridor().toStdString(), "Second");
    // The field shows what the chosen corridor holds, and so does the list.
    EXPECT_EQ(textOf(dialog, "corridorCurveTolerance").toStdString(), "0.005");
    auto* list = child<QTableWidget>(dialog, "corridorList");
    ASSERT_EQ(list->columnCount(), 10);
    EXPECT_EQ(list->horizontalHeaderItem(9)->text().toStdString(), "Curve tolerance");
    EXPECT_EQ(list->item(1, 9)->text().toStdString(), "0.005");
    EXPECT_EQ(list->item(0, 9)->text().toStdString(), "0.001");

    // Apply with the field as it was filled says nothing of it; with it edited,
    // it does - and 0 is how the verb is told to add no sections.
    click(dialog, "corridorApply");
    EXPECT_EQ(executor.last(),
              "CORRIDOR SET Second alignment=road template=\"urban street\" surface= interval=10 "
              "from= to=");
    fill(dialog, "corridorCurveTolerance", "0");
    click(dialog, "corridorApply");
    EXPECT_EQ(executor.last(),
              "CORRIDOR SET Second alignment=road template=\"urban street\" surface= interval=10 "
              "from= to= curvetol=0");
    EXPECT_EQ(document.corridors().find("Second")->curveTolerance, 0.0);

    // Not one number: said on the status line, and no line runs. One the
    // corridor refuses (over a tenth) is the corridor's refusal, shown, and the
    // field keeps what was typed for the person to correct.
    const std::size_t lines = executor.lines.size();
    fill(dialog, "corridorCurveTolerance", "tiny");
    click(dialog, "corridorApply");
    EXPECT_EQ(executor.lines.size(), lines);
    EXPECT_TRUE(label(dialog, "corridorStatus").contains("curve tolerance"))
        << label(dialog, "corridorStatus").toStdString();
    fill(dialog, "corridorCurveTolerance", "0.5");
    click(dialog, "corridorApply");
    EXPECT_EQ(executor.lines.size(), lines + 1);
    EXPECT_TRUE(label(dialog, "corridorStatus").contains("curve tolerance"))
        << label(dialog, "corridorStatus").toStdString();
    EXPECT_EQ(document.corridors().find("Second")->curveTolerance, 0.0);
    EXPECT_EQ(textOf(dialog, "corridorCurveTolerance").toStdString(), "0.5");
}
