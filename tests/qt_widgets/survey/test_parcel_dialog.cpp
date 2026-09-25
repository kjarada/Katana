// Survey > Coordinate Geometry > Parcel Report (src/katana_qt/survey/
// survey_parcel_dialog.hpp), driven by its object names as a person drives
// it.
//
// The report is cad::parcelReport's and its words formatParcelReport's and
// legalDescription's, which tests/cad/test_parcel.cpp pins on the same
// rectangle by hand; what is pinned here is that the dialog shows exactly
// those, and that Label Courses is the line PARCEL <id> LABEL <height> run
// through the executor - here the real interpreter on the same drawing, where
// the window's command line sends it - as one undo step. The headless
// qt_the_parcel_report_computes_labels_and_describes_a_lot_headless runs it through the window.

#include <gtest/gtest.h>

#include <QApplication>
#include <QClipboard>
#include <QDir>
#include <QFile>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTableWidget>
#include <QTemporaryDir>

#include <memory>
#include <string>
#include <vector>

#include "command_runner.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/parcel.hpp"
#include "katana/commands/entity_commands.hpp"
#include "survey/survey_parcel_dialog.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::qt::SurveyDialogContext;
using katana::qt::SurveyParcelDialog;
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

void click(QWidget& parent, const char* name)
{
    auto* button = child<QPushButton>(parent, name);
    ASSERT_NE(button, nullptr);
    ASSERT_TRUE(button->isEnabled()) << name << " is disabled";
    button->click();
    QApplication::processEvents();
}

QString message(QWidget& dialog) { return child<QLabel>(dialog, "message")->text(); }

// The 100 by 50 rectangle of tests/cad/test_parcel.cpp, drawn
// counter-clockwise from the origin, as the drawing's only entity.
struct Fixture {
    explicit Fixture(bool withRunner = true, bool headless = false) : interpreter(document)
    {
        Polyline2 boundary;
        boundary.vertices = {Point2(0, 0), Point2(100, 0), Point2(100, 50), Point2(0, 50)};
        boundary.closed = true;
        EXPECT_TRUE(document.execute(katana::commands::createPolyline(boundary)).ok());
        id = document.model().entities.ids().back();
        SurveyDialogContext context{&document, nullptr, [this](const QString& text, bool isError) {
                                        logged.push_back((isError ? "! " : "") + text);
                                    }};
        katana::qt::CommandRunner run;
        if (withRunner) {
            run = [this](const QString& line) {
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
        dialog = std::make_unique<SurveyParcelDialog>(context, run,
                                                      [headless] { return headless; }, nullptr);
    }
    [[nodiscard]] katana::cad::ParcelReport report() const
    {
        const auto* boundary =
            std::get_if<Polyline2>(&document.model().entities.find(id)->geometry);
        return *katana::cad::parcelReport(*boundary);
    }
    [[nodiscard]] std::size_t entities() const { return document.model().entities.size(); }

    Document document;
    CommandInterpreter interpreter;
    EntityId id = 0;
    std::vector<QString> lines;
    std::vector<QString> logged;
    std::unique_ptr<SurveyParcelDialog> dialog;
};

} // namespace

TEST(ParcelDialog, UseSelectionTakesTheSelectedClosedPolylineAndRefusesAnythingElse)
{
    Fixture f;
    click(*f.dialog, "useSelection");
    EXPECT_TRUE(message(*f.dialog).contains("select the one closed polyline"))
        << message(*f.dialog).toStdString();
    f.document.selection().set({f.id});
    click(*f.dialog, "useSelection");
    EXPECT_EQ(child<QLineEdit>(*f.dialog, "parcel")->text(), QString::number(f.id));
}

TEST(ParcelDialog, ComputeShowsTheCoursesSummaryAndReportThePARCELVerbPrints)
{
    Fixture f;
    fill(*f.dialog, "parcel", QString::number(f.id));
    click(*f.dialog, "compute");
    const katana::cad::ParcelReport report = f.report();

    auto* courses = child<QTableWidget>(*f.dialog, "parcelCourses");
    ASSERT_EQ(courses->rowCount(), 4);
    EXPECT_EQ(courses->item(0, 0)->text().toStdString(), "1");
    EXPECT_EQ(courses->item(0, 3)->text().toStdString(), report.courses[0].bearing);
    EXPECT_EQ(courses->item(0, 4)->text().toStdString(), "100.000");
    EXPECT_EQ(courses->item(1, 1)->text().toStdString(), "100.000"); // from (100, 0)
    EXPECT_EQ(courses->item(1, 4)->text().toStdString(), "50.000");
    EXPECT_EQ(child<QLabel>(*f.dialog, "parcelSummary")->text().toStdString(),
              katana::cad::formatParcelSummary(report));
    // The pane and the log carry what PARCEL id prints, word for word.
    const auto verb = f.interpreter.run("PARCEL " + std::to_string(f.id));
    ASSERT_TRUE(verb.ok());
    EXPECT_EQ(child<QPlainTextEdit>(*f.dialog, "result")->toPlainText().toStdString(), *verb);
    ASSERT_FALSE(f.logged.empty());
    EXPECT_EQ(f.logged.back().toStdString(), *verb);
}

TEST(ParcelDialog, TheLegalDescriptionIsPARCELLEGALsWithTheNameTyped)
{
    Fixture f;
    fill(*f.dialog, "parcel", "#" + QString::number(f.id));
    click(*f.dialog, "compute");
    const std::string id = std::to_string(f.id);
    auto* legal = child<QPlainTextEdit>(*f.dialog, "parcelLegalText");
    // No name typed: the verb's default, "Parcel <id>".
    EXPECT_EQ(legal->toPlainText().toStdString(), *f.interpreter.run("PARCEL " + id + " LEGAL"));
    fill(*f.dialog, "parcelName", "Lot7");
    EXPECT_EQ(legal->toPlainText().toStdString(),
              *f.interpreter.run("PARCEL " + id + " LEGAL Lot7"));
}

TEST(ParcelDialog, LabelCoursesRunsTheParcelLabelLineAsOneUndoStepAndNamesTheLayer)
{
    Fixture f;
    fill(*f.dialog, "parcel", QString::number(f.id));
    fill(*f.dialog, "labelHeight", "2");
    const std::size_t before = f.entities();
    const std::size_t undo = f.document.history().undoCount();
    click(*f.dialog, "label");
    ASSERT_EQ(f.lines.size(), 1u);
    EXPECT_EQ(f.lines.back().toStdString(), "PARCEL " + std::to_string(f.id) + " LABEL 2");
    // Four courses and the area.
    EXPECT_EQ(f.entities(), before + 5);
    EXPECT_EQ(f.document.history().undoCount(), undo + 1);
    EXPECT_EQ(message(*f.dialog).toStdString(), "5 labels created on layer 0, the current layer");
    ASSERT_TRUE(f.document.undo().ok());
    EXPECT_EQ(f.entities(), before);
}

TEST(ParcelDialog, WhatIsNotAParcelIsRefusedInTheVerbsWords)
{
    Fixture f;
    click(*f.dialog, "compute");
    EXPECT_TRUE(message(*f.dialog).contains("type the id of a closed polyline"))
        << message(*f.dialog).toStdString();
    ASSERT_TRUE(f.interpreter.run("LINE 0,0 10,0").ok());
    fill(*f.dialog, "parcel", QString::number(f.document.model().entities.ids().back()));
    click(*f.dialog, "compute");
    EXPECT_TRUE(message(*f.dialog).contains("a parcel must be a closed polyline"))
        << message(*f.dialog).toStdString();
    fill(*f.dialog, "parcel", "999999");
    click(*f.dialog, "compute");
    EXPECT_TRUE(message(*f.dialog).contains("no entity with that id"))
        << message(*f.dialog).toStdString();
    // A refused label runs its line and says why; nothing is added.
    const std::size_t before = f.entities();
    fill(*f.dialog, "parcel", QString::number(f.id));
    fill(*f.dialog, "labelHeight", "0");
    click(*f.dialog, "label");
    EXPECT_TRUE(message(*f.dialog).contains("height must be positive"))
        << message(*f.dialog).toStdString();
    EXPECT_EQ(f.entities(), before);
}

TEST(ParcelDialog, WithoutAnExecutorItSaysItCannotLabelAndChangesNothing)
{
    Fixture f(false);
    fill(*f.dialog, "parcel", QString::number(f.id));
    const std::size_t before = f.entities();
    click(*f.dialog, "label");
    EXPECT_TRUE(message(*f.dialog).contains("no command line")) << message(*f.dialog).toStdString();
    EXPECT_EQ(f.entities(), before);
}

TEST(ParcelDialog, TheCsvAndTheLegalTextAreSavedAsTheFunctionsWriteThem)
{
    Fixture f;
    QTemporaryDir folder;
    ASSERT_TRUE(folder.isValid());
    // Nothing to save before a report.
    EXPECT_FALSE(f.dialog->saveCoursesCsv(folder.filePath("early.csv")).ok());
    fill(*f.dialog, "parcel", QString::number(f.id));
    click(*f.dialog, "compute");

    const QString csvPath = folder.filePath("courses.csv");
    ASSERT_TRUE(f.dialog->saveCoursesCsv(csvPath).ok());
    QFile csv(csvPath);
    ASSERT_TRUE(csv.open(QIODevice::ReadOnly));
    EXPECT_EQ(csv.readAll().toStdString(), katana::cad::parcelCoursesCsv(f.report()));

    const QString legalPath = folder.filePath("legal.txt");
    ASSERT_TRUE(f.dialog->saveLegalText(legalPath).ok());
    QFile legal(legalPath);
    ASSERT_TRUE(legal.open(QIODevice::ReadOnly));
    EXPECT_EQ(legal.readAll().toStdString(),
              katana::cad::legalDescription(f.report(), "Parcel " + std::to_string(f.id)) + "\n");

    click(*f.dialog, "parcelCopy");
    const QString copied = QApplication::clipboard()->text();
    EXPECT_TRUE(copied.startsWith("Course\tFrom E\tFrom N\tBearing\tDistance\n1\t0.000\t0.000\t"))
        << copied.toStdString();
    EXPECT_EQ(copied.count('\n'), 5);
}

TEST(ParcelDialog, SavingInAHeadlessSessionOpensNoFileDialog)
{
    Fixture f(true, true);
    fill(*f.dialog, "parcel", QString::number(f.id));
    click(*f.dialog, "compute");
    click(*f.dialog, "parcelCsv");
    EXPECT_TRUE(message(*f.dialog).contains("headless")) << message(*f.dialog).toStdString();
    click(*f.dialog, "parcelLegalSave");
    EXPECT_TRUE(message(*f.dialog).contains("headless")) << message(*f.dialog).toStdString();
}
