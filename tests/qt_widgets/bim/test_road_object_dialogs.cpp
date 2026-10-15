// BIM > Road Objects (src/katana_qt/bim/), driven by object names as a
// person drives it by clicking and typing.
//
// The dialogs change nothing themselves: they write the ROADOBJECT line a
// person would type and hand it to the window's executor. So what is tested
// is each line - exactly the one a person would type - that the drawing then
// holds what the line says, and that what the dialogs show is what the
// drawing holds. The executor here runs the line through the real
// CommandInterpreter on the same drawing, which is where the window's
// command line sends a ROADOBJECT line.
//
// The site: alignment Main, straight from (0,0) to (100,0), its design
// profile level at 10.

#include <gtest/gtest.h>

#include <QAbstractButton>
#include <QAction>
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QGroupBox>
#include <QImage>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QMainWindow>
#include <QMenu>
#include <QPainter>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QScrollArea>
#include <QSet>
#include <QStringList>
#include <QTabWidget>
#include <QTableWidget>
#include <QToolBar>

#include <algorithm>
#include <memory>
#include <string>
#include <vector>

#include "bim/road_object_dialog.hpp"
#include "bim/road_object_manager.hpp"
#include "bim/road_object_workbench.hpp"
#include "command_reference_dialog.hpp"
#include "command_runner.hpp"
#include "icons.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/road/road_object_mesh.hpp"
#include "katana/cad/road/road_objects.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::road::RoadObjectKind;
using katana::qt::RoadObjectDialog;
using katana::qt::RoadObjectDialogContext;
using katana::qt::RoadObjectForm;
using katana::qt::RoadObjectManagerContext;
using katana::qt::RoadObjectManagerDialog;
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
    QApplication::processEvents();
}

void click(QWidget& parent, const char* name)
{
    auto* button = child<QPushButton>(parent, name);
    ASSERT_NE(button, nullptr);
    ASSERT_TRUE(button->isEnabled()) << name << " is disabled";
    button->click();
    QApplication::processEvents();
    QApplication::processEvents();
}

QString text(QWidget& parent, const char* name)
{
    if (auto* field = parent.findChild<QLineEdit*>(QString::fromLatin1(name))) {
        return field->text();
    }
    if (auto* label = parent.findChild<QLabel*>(QString::fromLatin1(name))) {
        return label->text();
    }
    if (auto* box = parent.findChild<QPlainTextEdit*>(QString::fromLatin1(name))) {
        return box->toPlainText();
    }
    ADD_FAILURE() << "no text widget " << name;
    return {};
}

// The window's executor, as far as a ROADOBJECT line goes: the interpreter on
// the same drawing. It keeps every line it was handed.
struct Site {
    Site()
    {
        for (const char* line : {"ALIGN NEW Main 0,0 100,0", "ALIGN DESIGN Main 0,10 100,10"}) {
            const auto reply = interpreter.run(line);
            EXPECT_TRUE(reply.ok()) << line;
        }
    }

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

    void run(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << ": " << (reply.ok() ? "" : reply.error().describe());
    }

    Document document;
    CommandInterpreter interpreter{document};
    std::vector<QString> lines;
};

} // namespace

// ---- the line the placing dialog writes ---------------------------------------------------

TEST(RoadObjectDialog, AnAlignmentPlacementWritesItsKeysBeforeItsPlacement)
{
    RoadObjectForm form;
    form.kind = RoadObjectKind::Barrier;
    form.alignment = "Main";
    form.station = "10";
    form.to = "60";
    form.offset = " 3 ";
    form.parameters = {{"type", "fshape"}};
    form.name = "Median east";
    const auto line = katana::qt::roadObjectAddLine(form);
    ASSERT_TRUE(line.ok()) << line.error().describe();
    EXPECT_EQ(*line, "ROADOBJECT ADD barrier name=\"Median east\" type=fshape ALIGNMENT Main "
                     "station=10 to=60 offset=3");
    form.preview = true;
    EXPECT_TRUE(katana::qt::roadObjectAddLine(form)->endsWith(" PREVIEW"));
}

TEST(RoadObjectDialog, AScopeEndsTheLineSoItsWhereTakesNothingOfTheObjects)
{
    RoadObjectForm form;
    form.kind = RoadObjectKind::Fence;
    form.place = RoadObjectForm::Place::Lines;
    form.scope = "LAYERS boundary WHERE TYPE=polyline";
    form.parameters = {{"height", "1.5"}};
    form.layer = "fences";
    const auto line = katana::qt::roadObjectAddLine(form);
    ASSERT_TRUE(line.ok()) << line.error().describe();
    EXPECT_EQ(*line, "ROADOBJECT ADD fence layer=fences height=1.5 ALONG FROM LAYERS boundary "
                     "WHERE TYPE=polyline");
}

TEST(RoadObjectDialog, PointsAreWrittenAsTypedAndRefusedWhenTheyDoNotRead)
{
    RoadObjectForm form;
    form.kind = RoadObjectKind::Sign;
    form.place = RoadObjectForm::Place::Points;
    form.points = "5,5,1  6,7";
    form.parameters = {{"code", "R1-1"}};
    EXPECT_EQ(*katana::qt::roadObjectAddLine(form), "ROADOBJECT ADD sign code=R1-1 AT 5,5,1 6,7");
    form.points = "5,5 six,7";
    EXPECT_FALSE(katana::qt::roadObjectAddLine(form).ok());
    form.points.clear();
    EXPECT_FALSE(katana::qt::roadObjectAddLine(form).ok());
}

TEST(RoadObjectDialog, WhatALineCannotSayRefusesTheWholeLine)
{
    RoadObjectForm form;
    form.kind = RoadObjectKind::Sign;
    form.alignment = "Main";
    // No station.
    EXPECT_TRUE(katana::qt::roadObjectAddLine(form).error().message.contains("station"));
    form.station = "fifty";
    EXPECT_TRUE(katana::qt::roadObjectAddLine(form).error().message.contains("station"));
    form.station = "50";
    form.name = "the \"big\" one";
    EXPECT_TRUE(katana::qt::roadObjectAddLine(form).error().message.contains("double quote"));
    form.name.clear();
    form.parameters = {{"width", "wide"}};
    EXPECT_TRUE(katana::qt::roadObjectAddLine(form).error().message.contains("width"));
    form.parameters.clear();
    form.alignment.clear();
    EXPECT_FALSE(katana::qt::roadObjectAddLine(form).ok());
    form.place = RoadObjectForm::Place::Lines;
    EXPECT_FALSE(katana::qt::roadObjectAddLine(form).ok()) << "no scope";
}

TEST(RoadObjectDialog, ThePreviewMakesEveryKindWithItsDefaultsAsClosedSolids)
{
    for (const RoadObjectKind kind : katana::cad::road::roadObjectKinds()) {
        const auto model = katana::qt::roadObjectPreview(kind, {});
        EXPECT_TRUE(model.problems.empty())
            << kindWord(kind) << ": " << (model.problems.empty() ? "" : model.problems[0].message);
        ASSERT_FALSE(model.objects.empty()) << kindWord(kind);
        for (const auto& object : model.objects) {
            EXPECT_TRUE(katana::cad::road::isClosedMesh(object.mesh)) << object.id;
        }
    }
    // A parameter that does not read is left at its default; one that does
    // is used: a wall 3 m tall.
    const auto wall = katana::qt::roadObjectPreview(RoadObjectKind::Wall, {{"height", "3"}});
    ASSERT_FALSE(wall.objects.empty());
    EXPECT_NEAR(wall.objects.front().mesh.bounds().max.z, 3.0, 1e-9);
}

// ---- the placing dialog, by object name ---------------------------------------------------

TEST(RoadObjectDialog, ASignPlacedFromItsPageIsTheLineAPersonWouldType)
{
    Site site;
    RoadObjectDialogContext context;
    context.document = &site.document;
    context.run = site.runner();
    RoadObjectDialog dialog(std::move(context));
    dialog.showKind(RoadObjectKind::Sign);
    EXPECT_EQ(child<QTabWidget>(dialog, "roadObjectKinds")->currentIndex(),
              static_cast<int>(RoadObjectKind::Sign));
    EXPECT_EQ(child<QComboBox>(dialog, "roadObjectAlignment")->currentText(), "Main");
    fill(dialog, "roadObjectStation", "50");
    fill(dialog, "roadObjectOffset", "4");
    fill(dialog, "roadObject_sign_code", "R1-1");
    EXPECT_EQ(text(dialog, "roadObjectLine"),
              "ROADOBJECT ADD sign code=R1-1 ALIGNMENT Main station=50 offset=4");

    // Preview changes nothing.
    click(dialog, "roadObjectPreviewRun");
    ASSERT_EQ(site.lines.size(), 1u);
    EXPECT_TRUE(site.lines.back().endsWith(" PREVIEW"));
    EXPECT_TRUE(site.document.roadObjects().objects.empty());
    EXPECT_TRUE(text(dialog, "roadObjectReply").startsWith("preview id=sign-1"))
        << text(dialog, "roadObjectReply").toStdString();

    click(dialog, "roadObjectAdd");
    ASSERT_EQ(site.lines.size(), 2u);
    EXPECT_EQ(site.lines.back(),
              "ROADOBJECT ADD sign code=R1-1 ALIGNMENT Main station=50 offset=4");
    ASSERT_EQ(site.document.roadObjects().objects.size(), 1u);
    EXPECT_EQ(site.document.roadObjects().objects.front().id, "sign-1");
    EXPECT_TRUE(text(dialog, "roadObjectStatus").contains("one undo step"));
}

TEST(RoadObjectDialog, AChoiceLeftAtItsDefaultIsLeftOutAndAChosenOneIsWritten)
{
    Site site;
    RoadObjectDialogContext context;
    context.document = &site.document;
    context.run = site.runner();
    RoadObjectDialog dialog(std::move(context));
    dialog.showKind(RoadObjectKind::Barrier);
    fill(dialog, "roadObjectStation", "0");
    fill(dialog, "roadObjectTo", "20");
    EXPECT_FALSE(text(dialog, "roadObjectLine").contains("type="));
    auto* type = child<QComboBox>(dialog, "roadObject_barrier_type");
    ASSERT_NE(type, nullptr);
    EXPECT_EQ(type->currentData().toString(), QString()) << "the first item is the default";
    type->setCurrentIndex(type->findData("singleslope"));
    QApplication::processEvents();
    EXPECT_TRUE(text(dialog, "roadObjectLine").contains(" type=singleslope ALIGNMENT"))
        << text(dialog, "roadObjectLine").toStdString();
    // What cannot be said is not run: Add is off and the status says why.
    fill(dialog, "roadObjectTo", "twenty");
    EXPECT_FALSE(child<QPushButton>(dialog, "roadObjectAdd")->isEnabled());
    EXPECT_TRUE(text(dialog, "roadObjectStatus").contains("to must be a number"))
        << text(dialog, "roadObjectStatus").toStdString();
}

TEST(RoadObjectDialog, ARefusedLineChangesNothingAndSaysWhy)
{
    Site site;
    RoadObjectDialogContext context;
    context.document = &site.document;
    context.run = site.runner();
    RoadObjectDialog dialog(std::move(context));
    dialog.showKind(RoadObjectKind::Sign);
    fill(dialog, "roadObjectStation", "150");
    click(dialog, "roadObjectAdd");
    EXPECT_TRUE(site.document.roadObjects().objects.empty());
    EXPECT_TRUE(text(dialog, "roadObjectStatus").contains("off alignment Main"))
        << text(dialog, "roadObjectStatus").toStdString();
}

// ---- the manager ----------------------------------------------------------------------------

TEST(RoadObjectManager, ItListsTheObjectsAndEachEditIsTheLineAPersonWouldType)
{
    Site site;
    site.run("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
    site.run("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
    RoadObjectManagerContext context;
    context.document = &site.document;
    context.run = site.runner();
    RoadObjectManagerDialog manager(std::move(context));
    auto* table = child<QTableWidget>(manager, "roadObjectTable");
    ASSERT_NE(table, nullptr);
    ASSERT_EQ(table->rowCount(), 2);
    EXPECT_EQ(table->item(0, 0)->text(), "barrier-1");
    EXPECT_EQ(table->item(1, 0)->text(), "sign-2");
    EXPECT_EQ(text(manager, "roadObjectCount"), "2 road objects, 0 problems");

    ASSERT_TRUE(manager.selectObject("barrier-1"));
    QApplication::processEvents();
    EXPECT_TRUE(text(manager, "roadObjectInfo").startsWith("object id=barrier-1"))
        << text(manager, "roadObjectInfo").toStdString();
    fill(manager, "roadObjectKeys", "offset=5");
    click(manager, "roadObjectApply");
    ASSERT_FALSE(site.lines.empty());
    EXPECT_EQ(site.lines.back(), "ROADOBJECT SET barrier-1 offset=5");
    EXPECT_EQ(site.document.roadObjects().find("barrier-1")->placement.offset, 5.0);
    // The table followed the drawing, and kept the row chosen.
    QApplication::processEvents();
    QApplication::processEvents();
    EXPECT_EQ(manager.selectedIds(), QStringList{"barrier-1"});

    click(manager, "roadObjectCopy");
    EXPECT_EQ(site.lines.back(), "ROADOBJECT COPY barrier-1");
    EXPECT_EQ(site.document.roadObjects().objects.size(), 3u);

    ASSERT_TRUE(manager.selectObject("sign-2"));
    QApplication::processEvents();
    click(manager, "roadObjectDelete");
    EXPECT_EQ(site.lines.back(), "ROADOBJECT DELETE sign-2");
    EXPECT_EQ(site.document.roadObjects().find("sign-2"), nullptr);
}

TEST(RoadObjectManager, ZoomFramesTheChosenObjectsWithRoomRoundThem)
{
    // The sign at (50, 4): its solids span x 49.9595 to 50.0375 and y 3.55
    // to 4.45 (test_road_objects.cpp), so it is framed 10 m across - the
    // least a frame is - about its middle, (49.9985, 4).
    Site site;
    site.run("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
    const auto frame = katana::qt::roadObjectsFrame(site.document, {"sign-1"});
    ASSERT_TRUE(frame.has_value());
    EXPECT_NEAR(frame->min.x, 49.9985 - 5.0, 1e-9);
    EXPECT_NEAR(frame->max.x, 49.9985 + 5.0, 1e-9);
    EXPECT_NEAR(frame->min.y, -1.0, 1e-9);
    EXPECT_NEAR(frame->max.y, 9.0, 1e-9);
    EXPECT_FALSE(katana::qt::roadObjectsFrame(site.document, {"sign-9"}).has_value());

    RoadObjectManagerContext context;
    context.document = &site.document;
    context.run = site.runner();
    RoadObjectManagerDialog manager(std::move(context));
    ASSERT_TRUE(manager.selectObject("sign-1"));
    QApplication::processEvents();
    click(manager, "roadObjectZoom");
    ASSERT_FALSE(site.lines.empty());
    EXPECT_TRUE(site.lines.back().startsWith("ZOOM WINDOW ")) << site.lines.back().toStdString();
}

// ---- the menu and the toolbar ----------------------------------------------------------------

TEST(RoadObjectWorkbench, TheBimMenuGainsARoadObjectsSectionWithAnIconAndATipOnEveryItem)
{
    Site site;
    QMainWindow window;
    QMenu menu("&BIM", &window);
    QToolBar bar("BIM", &window);
    katana::qt::RoadObjectServices services;
    services.document = &site.document;
    services.run = site.runner();
    services.makeAction = [&window](katana::qt::Icon icon, const QString& label,
                                    const QString& tip, const QKeySequence& shortcut,
                                    const QString& name) {
        auto* action = new QAction(katana::qt::icon(icon), label, &window);
        action->setObjectName(name);
        action->setStatusTip(tip);
        action->setShortcut(shortcut);
        return action;
    };
    katana::qt::RoadObjectWorkbench workbench(window, std::move(services), menu, bar);

    QStringList names;
    for (QAction* action : menu.actions()) {
        if (action->isSeparator()) {
            EXPECT_EQ(action->text(), "Road Objects") << "the section's title";
            continue;
        }
        names << action->objectName();
        EXPECT_FALSE(action->icon().isNull()) << action->objectName().toStdString();
        EXPECT_FALSE(action->statusTip().isEmpty()) << action->objectName().toStdString();
    }
    EXPECT_EQ(names, (QStringList{"bimRoadBarrier", "bimRoadGuidePosts", "bimRoadSign",
                                  "bimRoadLightPoles", "bimRoadCulvert", "bimRoadWall",
                                  "bimRoadFence", "bimRoadLineMarking", "bimRoadObjects"}));
    // Each underlined letter its own within the section.
    QSet<QChar> letters;
    for (QAction* action : menu.actions()) {
        const qsizetype at = action->text().indexOf('&');
        if (at >= 0 && at + 1 < action->text().size()) {
            EXPECT_FALSE(letters.contains(action->text()[at + 1].toLower()))
                << action->text().toStdString();
            letters.insert(action->text()[at + 1].toLower());
        }
    }
    // The ones placed most on the toolbar, after a separator.
    QStringList onBar;
    for (QAction* action : bar.actions()) {
        if (!action->isSeparator()) {
            onBar << action->objectName();
        }
    }
    EXPECT_EQ(onBar, (QStringList{"bimRoadBarrier", "bimRoadSign", "bimRoadLightPoles",
                                  "bimRoadLineMarking", "bimRoadObjects"}));

    // A menu item opens the one dialog, on its own page, as the window's child.
    workbench.action(RoadObjectKind::Culvert)->trigger();
    QApplication::processEvents();
    // By its object name, as --dialog finds it (no moc, so not by its class).
    auto* dialog =
        dynamic_cast<RoadObjectDialog*>(window.findChild<QDialog*>("roadObjectDialog"));
    ASSERT_NE(dialog, nullptr);
    EXPECT_EQ(dialog->currentKind(), RoadObjectKind::Culvert);
    workbench.managerAction()->trigger();
    QApplication::processEvents();
    EXPECT_NE(dynamic_cast<RoadObjectManagerDialog*>(
                  window.findChild<QDialog*>("roadObjectManagerDialog")),
              nullptr);
    dialog->hide();
}

TEST(RoadObjectWorkbench, EveryRoadObjectIconPaintsSomething)
{
    for (const katana::qt::Icon which :
         {katana::qt::Icon::RoadBarrier, katana::qt::Icon::RoadGuidePosts,
          katana::qt::Icon::RoadSign, katana::qt::Icon::RoadLightPole,
          katana::qt::Icon::RoadCulvert, katana::qt::Icon::RoadWall, katana::qt::Icon::RoadFence,
          katana::qt::Icon::RoadLineMarking, katana::qt::Icon::RoadObjects}) {
        QImage image(48, 48, QImage::Format_ARGB32_Premultiplied);
        image.fill(Qt::transparent);
        QPainter painter(&image);
        katana::qt::paintIcon(painter, which, QRectF(0, 0, 48, 48), Qt::black, Qt::red);
        painter.end();
        int ink = 0;
        for (int y = 0; y < image.height(); ++y) {
            for (int x = 0; x < image.width(); ++x) {
                ink += qAlpha(image.pixel(x, y)) > 0 ? 1 : 0;
            }
        }
        EXPECT_GT(ink, 0) << static_cast<int>(which);
    }
}

// Help > Command Reference reads HELP ROADOBJECT as a section of its own, so
// a word added to the verb's help is there at once.
TEST(RoadObjectWorkbench, TheCommandReferenceHasTheRoadObjectVerbs)
{
    const auto sections = katana::qt::commandReferenceSections();
    const auto found = std::ranges::find(sections, QString("Road objects"),
                                         &katana::qt::ReferenceSection::name);
    ASSERT_NE(found, sections.end());
    QStringList words;
    for (const auto& entry : found->entries) {
        words << entry.text;
    }
    const QString all = words.join('\n');
    for (const char* line : {"ROADOBJECT ADD kind", "ROADOBJECT KINDS", "ROADOBJECT PROBLEMS",
                             "ALONG FROM <scope>"}) {
        EXPECT_TRUE(all.contains(line)) << line;
    }
    EXPECT_FALSE(found->entries.empty());
    EXPECT_EQ(found->entries.front().title, "About");
}

// ---- the findings of the review of 2026-10-15 --------------------------------------------------

TEST(RoadObjectDialog, AddingFromAScopeThatTakesNothingSaysNothingWasAddedAndMakesNoUndoStep)
{
    // The scope is the selection and nothing is selected: the verb reports
    // added=0 and records no step. The status said "Added - one undo step."
    Site site;
    RoadObjectDialogContext context;
    context.document = &site.document;
    context.run = site.runner();
    RoadObjectDialog dialog(std::move(context));
    dialog.showKind(RoadObjectKind::Fence);
    child<QRadioButton>(dialog, "roadObjectPlaceLines")->setChecked(true);
    QApplication::processEvents();
    EXPECT_TRUE(text(dialog, "roadObjectLine").contains("ALONG FROM"))
        << text(dialog, "roadObjectLine").toStdString();
    click(dialog, "roadObjectAdd");
    EXPECT_TRUE(text(dialog, "roadObjectReply").contains("added=0"))
        << text(dialog, "roadObjectReply").toStdString();
    const QString status = text(dialog, "roadObjectStatus");
    EXPECT_TRUE(status.startsWith("Nothing added")) << status.toStdString();
    EXPECT_FALSE(status.contains("undo step")) << status.toStdString();
    EXPECT_TRUE(site.document.roadObjects().objects.empty());
    click(dialog, "roadObjectPreviewRun");
    EXPECT_TRUE(text(dialog, "roadObjectStatus").startsWith("Previewed: the scope took nothing"))
        << text(dialog, "roadObjectStatus").toStdString();
}

TEST(RoadObjectDialog, AddingFromAScopeThatTakesTwoLinesSaysHowManyWereAddedInOneUndoStep)
{
    Site site;
    site.run("LINE 0,20 10,20");
    site.run("LINE 0,30 10,30");
    site.run("SELECT ALL");
    site.run("PROP SET elevation 2.5");
    RoadObjectDialogContext context;
    context.document = &site.document;
    context.run = site.runner();
    RoadObjectDialog dialog(std::move(context));
    dialog.showKind(RoadObjectKind::Fence);
    child<QRadioButton>(dialog, "roadObjectPlaceLines")->setChecked(true);
    child<QRadioButton>(dialog, "roadObjectSourceScopeDrawing")->setChecked(true);
    QApplication::processEvents();
    click(dialog, "roadObjectAdd");
    EXPECT_EQ(site.document.roadObjects().objects.size(), 2u);
    EXPECT_EQ(text(dialog, "roadObjectStatus"), "Added 2 - one undo step.");
}

TEST(RoadObjectDialog, TheScopesOwnRefusalIsShownInPlaceOfAGenericOne)
{
    // A filter colour that does not read: the scope widget says why, and the
    // dialog said "say which part of the drawing" to a person who had.
    Site site;
    RoadObjectDialogContext context;
    context.document = &site.document;
    context.run = site.runner();
    RoadObjectDialog dialog(std::move(context));
    dialog.showKind(RoadObjectKind::Fence);
    child<QRadioButton>(dialog, "roadObjectPlaceLines")->setChecked(true);
    fill(dialog, "roadObjectSourceFilterColour", "red");
    const QString status = text(dialog, "roadObjectStatus");
    EXPECT_TRUE(status.contains("filter's colour")) << status.toStdString();
    EXPECT_FALSE(status.contains("say which part")) << status.toStdString();
    EXPECT_FALSE(child<QPushButton>(dialog, "roadObjectAdd")->isEnabled());
    const auto line = katana::qt::roadObjectAddLine(dialog.form());
    ASSERT_FALSE(line.ok());
    EXPECT_TRUE(QString::fromStdString(line.error().message).contains("filter's colour"));
    // With nothing the scope objects to and nothing said, the old words stand.
    RoadObjectForm form;
    form.kind = RoadObjectKind::Fence;
    form.place = RoadObjectForm::Place::Lines;
    EXPECT_TRUE(QString::fromStdString(katana::qt::roadObjectAddLine(form).error().message)
                    .contains("say which part"));
}

TEST(RoadObjectDialog, EveryButtonOfTheDialogHasItsOwnMnemonic)
{
    // Alt+D was both "At points in the drawing" and "Add": two widgets with
    // one key make it cycle between them and choose neither.
    Site site;
    RoadObjectDialogContext context;
    context.document = &site.document;
    context.run = site.runner();
    RoadObjectDialog dialog(std::move(context));
    QSet<QChar> letters;
    for (const auto* button : dialog.findChildren<QAbstractButton*>()) {
        const qsizetype at = button->text().indexOf('&');
        if (at < 0 || at + 1 >= button->text().size()) {
            continue;
        }
        const QChar letter = button->text()[at + 1].toLower();
        EXPECT_FALSE(letters.contains(letter))
            << "two buttons use Alt+" << letter.toLatin1() << ": " << button->text().toStdString();
        letters.insert(letter);
    }
    EXPECT_TRUE(letters.contains(QChar('d'))) << "Add keeps Alt+D";
}

TEST(RoadObjectDialog, TheCrossfallIsOnlyOfferedWhereThereIsAProfileToCarryOut)
{
    Site site;
    RoadObjectDialogContext context;
    context.document = &site.document;
    context.run = site.runner();
    RoadObjectDialog dialog(std::move(context));
    dialog.showKind(RoadObjectKind::Sign);
    fill(dialog, "roadObjectStation", "50");
    fill(dialog, "roadObjectCrossfall", "-3");
    auto* crossfall = child<QLineEdit>(dialog, "roadObjectCrossfall");
    EXPECT_TRUE(crossfall->isEnabled());
    EXPECT_TRUE(text(dialog, "roadObjectLine").contains("crossfall=-3"));
    // Along lines, or at points, there is no profile: not offered, not written.
    child<QRadioButton>(dialog, "roadObjectPlacePoints")->setChecked(true);
    fill(dialog, "roadObjectPoints", "5,5,1");
    EXPECT_FALSE(crossfall->isEnabled());
    EXPECT_FALSE(text(dialog, "roadObjectLine").contains("crossfall"))
        << text(dialog, "roadObjectLine").toStdString();
    click(dialog, "roadObjectAdd");
    ASSERT_EQ(site.document.roadObjects().objects.size(), 1u);
    // On the heights of a line, or a surface: the same.
    child<QRadioButton>(dialog, "roadObjectPlaceAlignment")->setChecked(true);
    auto* heights = child<QComboBox>(dialog, "roadObjectHeights");
    heights->setCurrentIndex(heights->findData("surface"));
    QApplication::processEvents();
    EXPECT_FALSE(crossfall->isEnabled());
    heights->setCurrentIndex(heights->findData("profile"));
    QApplication::processEvents();
    EXPECT_TRUE(crossfall->isEnabled());
}

TEST(RoadObjectDialog, TheCulvertPreviewShowsTheBarrelThroughItsHeadwalls)
{
    // Seen along the barrel each headwall is a slab as large as the barrel and
    // more, in the barrel's colour; painted solid over it they hid it, and the
    // preview was one plain rectangle whatever the shape and the cells. The
    // headwalls are outlines now and the barrel is drawn over them: the pipe's
    // hole is the background. Found by the culvert colour (158, 154, 146): its
    // bounding box is the barrel's, and the middle of that is the hole.
    Site site;
    RoadObjectDialogContext context;
    context.document = &site.document;
    context.run = site.runner();
    RoadObjectDialog dialog(std::move(context));
    dialog.showKind(RoadObjectKind::Culvert);
    auto* preview = child<QWidget>(dialog, "roadObjectPreview");
    ASSERT_NE(preview, nullptr);
    preview->resize(400, 300);
    const QColor base = preview->palette().base().color();
    for (const char* shape : {"circular", "box"}) {
        auto* choice = child<QComboBox>(dialog, "roadObject_culvert_shape");
        ASSERT_NE(choice, nullptr);
        choice->setCurrentIndex(choice->findData(shape));
        QApplication::processEvents();
        const QImage image = preview->grab().toImage();
        const QRgb fill = qRgb(158, 154, 146);
        int x0 = image.width();
        int x1 = -1;
        int y0 = image.height();
        int y1 = -1;
        for (int y = 0; y < image.height(); ++y) {
            for (int x = 0; x < image.width(); ++x) {
                if (image.pixel(x, y) == fill) {
                    x0 = std::min(x0, x);
                    x1 = std::max(x1, x);
                    y0 = std::min(y0, y);
                    y1 = std::max(y1, y);
                }
            }
        }
        ASSERT_GT(x1, x0) << shape << ": no culvert colour painted";
        ASSERT_GT(y1, y0) << shape;
        EXPECT_EQ(QColor(image.pixel((x0 + x1) / 2, (y0 + y1) / 2)), base)
            << shape << ": the middle of the barrel is not its hole";
    }
}

TEST(RoadObjectDialog, ThePagesScrollSoPreviewAddAndCloseStayInReachOnALaptopScreen)
{
    // Laid out in one piece the dialog was some 1,500 px tall and Add was off
    // the screen of a 1366 x 768 laptop (728 px under its task bar). The
    // pages, placement and heights are in a scroll area; the buttons are not.
    Site site;
    RoadObjectDialogContext context;
    context.document = &site.document;
    context.run = site.runner();
    RoadObjectDialog dialog(std::move(context));
    auto* body = child<QScrollArea>(dialog, "roadObjectBody");
    ASSERT_NE(body, nullptr);
    for (const char* name : {"roadObjectAdd", "roadObjectPreviewRun", "roadObjectClose",
                             "roadObjectLine", "roadObjectReply", "roadObjectStatus"}) {
        EXPECT_EQ(body->findChild<QWidget*>(QString::fromLatin1(name)), nullptr)
            << name << " scrolls with the pages";
    }
    for (const char* name : {"roadObjectKinds", "roadObjectPlacement", "roadObjectHeightsGroup",
                             "roadObjectPreview"}) {
        EXPECT_NE(body->findChild<QWidget*>(QString::fromLatin1(name)), nullptr) << name;
    }
    EXPECT_LE(dialog.minimumSizeHint().height(), 700);
}

// ---- the manager ----------------------------------------------------------------------------

TEST(RoadObjectManager, ASetItCannotReadIsSaidAndNotShownAsZeroObjects)
{
    Site site;
    auto metadata = site.document.metadata();
    metadata.unknownKeys["road_objects"] =
        R"({"format": "katana-road-objects", "version": 2, "next": 2, "objects": []})";
    site.document.setMetadata(metadata);
    RoadObjectManagerContext context;
    context.document = &site.document;
    context.run = site.runner();
    RoadObjectManagerDialog manager(std::move(context));
    const QString count = text(manager, "roadObjectCount");
    EXPECT_FALSE(count.startsWith("0 road objects, 0 problems")) << count.toStdString();
    EXPECT_TRUE(count.startsWith("0 road objects, 1 problems")) << count.toStdString();
    EXPECT_TRUE(count.contains("cannot be read")) << count.toStdString();
    EXPECT_TRUE(count.contains("newer version")) << count.toStdString();
}

TEST(RoadObjectManager, CopyTakesTheKeysFieldSoACopyAtPointsCanBeMovedFromTheWindow)
{
    // station=, dx= and dy= are what move a copy and nothing else in the window
    // says them: Copy sent only the id, and the copy stood on its original.
    Site site;
    site.run("ROADOBJECT ADD sign AT 5,5,1");
    RoadObjectManagerContext context;
    context.document = &site.document;
    context.run = site.runner();
    RoadObjectManagerDialog manager(std::move(context));
    ASSERT_TRUE(manager.selectObject("sign-1"));
    QApplication::processEvents();
    fill(manager, "roadObjectKeys", "dx=10 dy=-2");
    click(manager, "roadObjectCopy");
    EXPECT_EQ(site.lines.back(), "ROADOBJECT COPY sign-1 dx=10 dy=-2");
    const auto* copy = site.document.roadObjects().find("sign-2");
    ASSERT_NE(copy, nullptr);
    ASSERT_EQ(copy->placement.vertices.size(), 1u);
    EXPECT_EQ(copy->placement.vertices.front().position, katana::geometry::Point2(15, 3));
    // The keys were for that copy: spent, not left to move the next.
    EXPECT_EQ(text(manager, "roadObjectKeys"), "");
    click(manager, "roadObjectCopy");
    EXPECT_EQ(site.lines.back(), "ROADOBJECT COPY sign-1");
    // A refused Apply keeps its keys, to be put right.
    fill(manager, "roadObjectKeys", "nosuchkey=1");
    click(manager, "roadObjectApply");
    EXPECT_EQ(text(manager, "roadObjectKeys"), "nosuchkey=1");
    EXPECT_TRUE(text(manager, "roadObjectManagerStatus").startsWith("Refused"))
        << text(manager, "roadObjectManagerStatus").toStdString();
}

// ---- the manager by scope ---------------------------------------------------------------------
//
// The shared "Apply to" and "Only those that match" controls beside the table,
// and the three buttons that put them into ROADOBJECT LIST, QUANTITIES and DELETE
// - each the line a person would type, run by the window's executor.

namespace {

// The manager on a site of three objects, in this order:
//   barrier-1  road/objects/barrier  stations 0 to 20, offset 4
//   sign-2     road/objects/sign     station 50, offset 4
//   light-3    lighting/poles        poles at stations 0, 30 and 60, offset 6
struct ScopedManager {
    ScopedManager()
    {
        site.run("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
        site.run("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
        site.run("ROADOBJECT ADD light ALIGNMENT Main station=0 to=60 spacing=30 offset=6 "
                 "layer=lighting/poles");
        RoadObjectManagerContext context;
        context.document = &site.document;
        context.run = site.runner();
        manager = std::make_unique<RoadObjectManagerDialog>(std::move(context));
        QApplication::processEvents();
    }

    Site site;
    std::unique_ptr<RoadObjectManagerDialog> manager;
};

void chooseScope(QWidget& manager, const char* radio)
{
    auto* button = child<QRadioButton>(manager, radio);
    ASSERT_NE(button, nullptr);
    button->click();
    QApplication::processEvents();
}

} // namespace

TEST(RoadObjectManager, TheSharedScopeControlsStartOnTheWholeDrawingAndGreyWhatARoadObjectHasNoneOf)
{
    ScopedManager scoped;
    RoadObjectManagerDialog& manager = *scoped.manager;
    EXPECT_NE(manager.findChild<QGroupBox*>("roadObjectManagerByScope"), nullptr);
    auto* drawing = child<QRadioButton>(manager, "roadObjectManagerScopeDrawing");
    ASSERT_NE(drawing, nullptr);
    EXPECT_TRUE(drawing->isChecked());
    // An entity's type, style and text: a road object has none, so they are
    // greyed with the reason, where the verb would refuse them by name.
    for (const char* name : {"roadObjectManagerTypePoint", "roadObjectManagerTypeLine",
                             "roadObjectManagerTypeSpline"}) {
        auto* box = child<QCheckBox>(manager, name);
        ASSERT_NE(box, nullptr);
        EXPECT_FALSE(box->isEnabled()) << name;
        EXPECT_TRUE(box->toolTip().contains("not an entity")) << name;
    }
    for (const char* name : {"roadObjectManagerFilterStyle", "roadObjectManagerFilterText"}) {
        auto* edit = child<QLineEdit>(manager, name);
        ASSERT_NE(edit, nullptr);
        EXPECT_FALSE(edit->isEnabled()) << name;
        EXPECT_TRUE(edit->toolTip().contains("not an entity")) << name;
    }
    // What means something for one stays live: a layer pattern, a colour, a
    // property and its value, and what is drawn.
    for (const char* name : {"roadObjectManagerFilterLayer", "roadObjectManagerFilterColour",
                             "roadObjectManagerFilterProperty", "roadObjectManagerFilterValue"}) {
        auto* edit = child<QLineEdit>(manager, name);
        ASSERT_NE(edit, nullptr);
        EXPECT_TRUE(edit->isEnabled()) << name;
    }
    EXPECT_TRUE(child<QCheckBox>(manager, "roadObjectManagerDrawnOnly")->isEnabled());
    // The three buttons, each with a mnemonic of its own beside the others'.
    QSet<QChar> letters;
    for (const auto* button : manager.findChildren<QAbstractButton*>()) {
        const qsizetype at = button->text().indexOf('&');
        if (at < 0 || at + 1 >= button->text().size()) {
            continue;
        }
        const QChar letter = button->text()[at + 1].toLower();
        EXPECT_FALSE(letters.contains(letter))
            << "two buttons use Alt+" << letter.toLatin1() << ": " << button->text().toStdString();
        letters.insert(letter);
    }
    for (const char* name :
         {"roadObjectSelectMatching", "roadObjectQuantities", "roadObjectDeleteMatching"}) {
        EXPECT_NE(manager.findChild<QPushButton*>(name), nullptr) << name;
    }
}

TEST(RoadObjectManager, SelectMatchingRunsListWithTheScopeAndChoosesTheRowsItTook)
{
    ScopedManager scoped;
    RoadObjectManagerDialog& manager = *scoped.manager;
    Site& site = scoped.site;

    // A property and its value on the whole drawing: the kind.
    fill(manager, "roadObjectManagerFilterProperty", "kind");
    fill(manager, "roadObjectManagerFilterValue", "barrier");
    click(manager, "roadObjectSelectMatching");
    EXPECT_EQ(site.lines.back(), "ROADOBJECT LIST DRAWING WHERE PROP=kind:barrier");
    EXPECT_EQ(manager.selectedIds(), QStringList{"barrier-1"});
    EXPECT_TRUE(text(manager, "roadObjectManagerStatus")
                    .startsWith("1 chosen - scope=drawing where=\"PROP=kind:barrier\" matched=1"))
        << text(manager, "roadObjectManagerStatus").toStdString();

    // A layer pattern of the filter: the lights' layer.
    fill(manager, "roadObjectManagerFilterProperty", "");
    fill(manager, "roadObjectManagerFilterValue", "");
    fill(manager, "roadObjectManagerFilterLayer", "lighting/*");
    click(manager, "roadObjectSelectMatching");
    EXPECT_EQ(site.lines.back(), "ROADOBJECT LIST DRAWING WHERE LAYER=lighting/*");
    EXPECT_EQ(manager.selectedIds(), QStringList{"light-3"});

    // The checked layers: the sign's, with the layers beneath it.
    fill(manager, "roadObjectManagerFilterLayer", "");
    chooseScope(manager, "roadObjectManagerScopeLayers");
    auto* layers = child<QListWidget>(manager, "roadObjectManagerLayers");
    ASSERT_NE(layers, nullptr);
    int ticked = 0;
    for (int row = 0; row < layers->count(); ++row) {
        if (layers->item(row)->text() == "road/objects/sign") {
            layers->item(row)->setCheckState(Qt::Checked);
            ++ticked;
        }
    }
    ASSERT_EQ(ticked, 1);
    click(manager, "roadObjectSelectMatching");
    EXPECT_EQ(site.lines.back(), "ROADOBJECT LIST LAYERS road/objects/sign");
    EXPECT_EQ(manager.selectedIds(), QStringList{"sign-2"});

    // A scope that takes nothing is said, not refused, and chooses nothing.
    chooseScope(manager, "roadObjectManagerScopeDrawing");
    fill(manager, "roadObjectManagerFilterProperty", "kind");
    fill(manager, "roadObjectManagerFilterValue", "culvert");
    click(manager, "roadObjectSelectMatching");
    EXPECT_TRUE(manager.selectedIds().isEmpty());
    EXPECT_TRUE(text(manager, "roadObjectManagerStatus")
                    .startsWith("0 chosen - scope=drawing where=\"PROP=kind:culvert\" matched=0"))
        << text(manager, "roadObjectManagerStatus").toStdString();
}

TEST(RoadObjectManager, QuantitiesRunsQuantitiesWithTheScopeAndShowsThemInTheDetails)
{
    ScopedManager scoped;
    RoadObjectManagerDialog& manager = *scoped.manager;
    fill(manager, "roadObjectManagerFilterProperty", "kind");
    fill(manager, "roadObjectManagerFilterValue", "barrier");
    click(manager, "roadObjectQuantities");
    EXPECT_EQ(scoped.site.lines.back(), "ROADOBJECT QUANTITIES DRAWING WHERE PROP=kind:barrier");
    // The one barrier is 20 m long.
    const QString details = text(manager, "roadObjectInfo");
    EXPECT_TRUE(details.startsWith("scope=drawing where=\"PROP=kind:barrier\" matched=1"))
        << details.toStdString();
    EXPECT_TRUE(details.contains("total kind=barrier objects=1 length=20")) << details.toStdString();
    EXPECT_FALSE(details.contains("kind=sign")) << details.toStdString();
}

TEST(RoadObjectManager, DeleteMatchingRunsDeleteWithTheScopeAsOneStepAndTheTableFollows)
{
    ScopedManager scoped;
    RoadObjectManagerDialog& manager = *scoped.manager;
    Site& site = scoped.site;
    fill(manager, "roadObjectManagerFilterLayer", "road/objects/s*");
    click(manager, "roadObjectDeleteMatching");
    EXPECT_EQ(site.lines.back(), "ROADOBJECT DELETE DRAWING WHERE LAYER=road/objects/s*");
    EXPECT_EQ(site.document.roadObjects().find("sign-2"), nullptr);
    EXPECT_NE(site.document.roadObjects().find("barrier-1"), nullptr);
    EXPECT_NE(site.document.roadObjects().find("light-3"), nullptr);
    QApplication::processEvents();
    EXPECT_EQ(child<QTableWidget>(manager, "roadObjectTable")->rowCount(), 2);
    EXPECT_TRUE(text(manager, "roadObjectManagerStatus").contains("deleted count=1 ids=sign-2"))
        << text(manager, "roadObjectManagerStatus").toStdString();
    // One undo brings it back.
    site.run("UNDO");
    QApplication::processEvents();
    EXPECT_NE(site.document.roadObjects().find("sign-2"), nullptr);
    EXPECT_EQ(child<QTableWidget>(manager, "roadObjectTable")->rowCount(), 3);
}

TEST(RoadObjectManager, ControlsThatDoNotReadRefuseWithTheirOwnReasonAndRunNothing)
{
    ScopedManager scoped;
    RoadObjectManagerDialog& manager = *scoped.manager;
    Site& site = scoped.site;
    const std::size_t before = site.lines.size();
    // The layers scope with no layer ticked, and a colour that is not a colour.
    chooseScope(manager, "roadObjectManagerScopeLayers");
    click(manager, "roadObjectDeleteMatching");
    EXPECT_TRUE(text(manager, "roadObjectManagerStatus").startsWith("Refused: "))
        << text(manager, "roadObjectManagerStatus").toStdString();
    chooseScope(manager, "roadObjectManagerScopeDrawing");
    fill(manager, "roadObjectManagerFilterColour", "red");
    click(manager, "roadObjectSelectMatching");
    EXPECT_TRUE(text(manager, "roadObjectManagerStatus").startsWith("Refused: "))
        << text(manager, "roadObjectManagerStatus").toStdString();
    // Nothing was handed to the executor, and nothing was deleted.
    EXPECT_EQ(site.lines.size(), before);
    EXPECT_EQ(site.document.roadObjects().objects.size(), 3u);
}
