// File > Drawing Summary (customisation/drawing_summary_dialog.hpp): what it
// shows of a drawing laid out by hand in each test, that it follows the
// drawing as commands run, and that its buttons run the line and call the
// window's hooks they name. The runner and the hooks are the test's own; the
// headless qt_drawing_summary_* check drives the real window.
//
// The customisation pane is what a bare CUSTOMISE replies (docs/customisation.md,
// "The verbs"): its expected text is written from that chapter's records and
// from the fixture tests/data/customisation/test_survey.customisation.json -
// 11 rules over 8 keys, counted by hand - not from a run.

#include <gtest/gtest.h>

#include <filesystem>
#include <memory>
#include <string>
#include <vector>

#include <QApplication>
#include <QClipboard>
#include <QLabel>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QScrollBar>

#include "customisation/drawing_summary_dialog.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/entity/style_library.hpp"
#include "widget_harness.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::qt::DrawingSummaryContext;
using katana::qt::DrawingSummaryDialog;
using katana::qt::VerbOutcome;

namespace {

template <typename T> T* child(QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << "no " << name;
    return found;
}

QString label(QWidget& parent, const char* name)
{
    const auto* found = child<QLabel>(parent, name);
    return found != nullptr ? found->text() : QString();
}

TEST(DrawingSummaryDialog, ItSaysWhatTheDrawingHoldsAndWhatIsCurrent)
{
    Document document;
    CommandInterpreter interpreter(document);
    ASSERT_TRUE(interpreter.run("RECT 0,0 10,5").ok());
    ASSERT_TRUE(interpreter.run("CIRCLE 5,5 2").ok());
    ASSERT_TRUE(interpreter.run("LAYER NEW roads").ok());
    ASSERT_TRUE(interpreter.run("UNDO").ok());
    ASSERT_TRUE(interpreter.run("SELECT 2").ok());
    DrawingSummaryContext context;
    context.document = &document;
    DrawingSummaryDialog dialog(std::move(context));
    EXPECT_EQ(dialog.objectName(), "drawingSummaryDialog");
    EXPECT_EQ(label(dialog, "drawingSummaryProject"),
              "none - not saved to a project yet, with unsaved changes");
    EXPECT_EQ(label(dialog, "drawingSummaryDrawing"), "2 entities, 1 layer, 0 alignments, 0 sheets");
    EXPECT_TRUE(label(dialog, "drawingSummaryCurrent").startsWith("layer 0, style ByLayer, annotation scale 1:"));
    EXPECT_EQ(label(dialog, "drawingSummarySelection"), "1 entity selected");
    EXPECT_EQ(label(dialog, "drawingSummaryHistory"),
              "2 steps to undo (next: " + QString::fromStdString(std::string(document.history().undoName())) +
                  "), 1 step to redo (next: " +
                  QString::fromStdString(std::string(document.history().redoName())) + ")");
    // Nothing was installed: no name, origin none, not kept, and the
    // settings a session starts with - both automation switches on, the
    // seven linework control codes at their defaults.
    EXPECT_EQ(child<QPlainTextEdit>(dialog, "drawingSummaryCustomisation")->toPlainText(),
              "No customisation is loaded.\n"
              "  CUSTOMISE <file> [<file>...]  loads Katana customisation files\n"
              "customisation name=\"\" origin=none kept=no definitions=0 codes=0 rules=0 "
              "colours=0\n"
              "automation auto.codes=on auto.linework=on\n"
              "linework linework.start=ST linework.end=END linework.close=CL "
              "linework.arcstart=BC linework.arcend=EC linework.join=JPN linework.rectangle=RECT");
}

TEST(DrawingSummaryDialog, TheCustomisationPaneIsTheCustomiseLinesReplyAndFollowsALoad)
{
    // The fixture's 11 rules over 8 keys loaded into an empty session, which
    // takes the file's name and has it as its one source - of rules alone.
    // The pane reads the Document, so it needs no list from the window.
    const std::filesystem::path fixture =
        std::filesystem::path(__FILE__).parent_path().parent_path().parent_path() / "data" /
        "customisation" / "test_survey.customisation.json";
    Document document;
    CommandInterpreter interpreter(document);
    DrawingSummaryContext context;
    context.document = &document;
    DrawingSummaryDialog dialog(std::move(context));
    const auto loaded = interpreter.run("CUSTOMISE \"" + fixture.generic_string() + "\"");
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
    katana::qt::test::processEvents();
    const QString pane =
        child<QPlainTextEdit>(dialog, "drawingSummaryCustomisation")->toPlainText();
    EXPECT_TRUE(pane.contains("11 survey code rules over 8 distinct codes\n"
                              "customisation name=test_survey origin=loaded kept=no definitions=0 "
                              "codes=8 rules=11 colours=0\n"
                              "source name=test_survey definitions=no rules=yes\n"
                              "automation auto.codes=on auto.linework=on\n"))
        << pane.toStdString();
    EXPECT_FALSE(pane.contains("Loaded files, in load order")) << "the older prose";
}

TEST(DrawingSummaryDialog, NoRecordOfTheCustomisationPaneIsCutAtThePanesEdge)
{
    // The linework record is in every reply and is 143 characters long (the
    // word, then its seven key=value pairs: 8 + 18 + 17 + 18 + 21 + 19 + 18 +
    // 24) - wider than the pane in any fixed-width face at the dialog's own
    // 640 pixels. Nothing of it may lie past the edge: the pane wraps, so
    // there is nothing to scroll sideways. Left unwrapped, the record's last
    // codes were out of sight behind a scroll bar.
    Document document;
    DrawingSummaryContext context;
    context.document = &document;
    DrawingSummaryDialog dialog(std::move(context));
    dialog.show();
    katana::qt::test::processEvents();
    auto* pane = child<QPlainTextEdit>(dialog, "drawingSummaryCustomisation");
    ASSERT_NE(pane, nullptr);
    ASSERT_TRUE(pane->toPlainText().endsWith("linework.join=JPN linework.rectangle=RECT"));
    EXPECT_EQ(pane->horizontalScrollBar()->maximum(), 0)
        << "a record runs past the pane's edge";
}

TEST(DrawingSummaryDialog, ItFollowsTheDrawingAsCommandsRun)
{
    Document document;
    CommandInterpreter interpreter(document);
    DrawingSummaryContext context;
    context.document = &document;
    DrawingSummaryDialog dialog(std::move(context));
    EXPECT_EQ(label(dialog, "drawingSummaryDrawing"), "0 entities, 1 layer, 0 alignments, 0 sheets");
    ASSERT_TRUE(interpreter.run("POINT 1,1").ok());
    ASSERT_TRUE(interpreter.run("POINT 2,2").ok());
    // Nothing yet: the dialog refreshes on the event loop, once.
    EXPECT_EQ(label(dialog, "drawingSummaryDrawing"), "0 entities, 1 layer, 0 alignments, 0 sheets");
    katana::qt::test::processEvents();
    EXPECT_EQ(label(dialog, "drawingSummaryDrawing"), "2 entities, 1 layer, 0 alignments, 0 sheets");
}

TEST(DrawingSummaryDialog, ANameNoLoadedLibraryDefinesIsListedAndOpensTheStyleManager)
{
    Document document;
    katana::entity::StyleLibrary library;
    katana::entity::LineStyle fence;
    fence.name = "TEST Fence";
    fence.group = "FENCES";
    ASSERT_TRUE(katana::entity::addOrReplace(library, fence).ok());
    document.setStyleLibrary(std::move(library));
    CommandInterpreter interpreter(document);
    ASSERT_TRUE(interpreter.run("STYLE NEW fences").ok());
    ASSERT_TRUE(interpreter.run("STYLE SET fences linetype \"TEST Fence\"").ok());
    // The library the style was drawn with, replaced by one without it.
    document.setStyleLibrary({});

    QStringList shown;
    DrawingSummaryContext context;
    context.document = &document;
    context.showMissing = [&shown](const QString& name) { shown << name; };
    DrawingSummaryDialog dialog(std::move(context));
    auto* list = child<QListWidget>(dialog, "drawingSummaryUnresolved");
    ASSERT_EQ(list->count(), 1);
    EXPECT_EQ(list->item(0)->text(), "TEST Fence");
    auto* show = child<QPushButton>(dialog, "drawingSummaryShowMissing");
    EXPECT_FALSE(show->isEnabled()) << "nothing is chosen yet";
    list->setCurrentRow(0);
    ASSERT_TRUE(show->isEnabled());
    show->click();
    EXPECT_EQ(shown, QStringList{"TEST Fence"});
    emit list->itemActivated(list->item(0));
    EXPECT_EQ(shown, (QStringList{"TEST Fence", "TEST Fence"}));
}

TEST(DrawingSummaryDialog, CopyAsJsonRunsStatusJsonAndCopiesItsReply)
{
    Document document;
    std::vector<QString> ran;
    DrawingSummaryContext context;
    context.document = &document;
    context.run = [&ran](const QString& line) {
        ran.push_back(line);
        return VerbOutcome{true, "{\n  \"entities\": 0\n}", {}};
    };
    DrawingSummaryDialog dialog(std::move(context));
    child<QPushButton>(dialog, "drawingSummaryCopyJson")->click();
    EXPECT_EQ(ran, std::vector<QString>{"STATUS JSON"});
    EXPECT_EQ(QApplication::clipboard()->text(), "{\n  \"entities\": 0\n}");
    EXPECT_EQ(label(dialog, "drawingSummaryStatus"),
              "Copied the drawing's state as JSON (STATUS JSON).");
}

TEST(DrawingSummaryDialog, ARefusedStatusCopiesNothingAndSaysWhy)
{
    Document document;
    DrawingSummaryContext context;
    context.document = &document;
    context.run = [](const QString&) { return VerbOutcome{false, {}, "no"}; };
    DrawingSummaryDialog dialog(std::move(context));
    QApplication::clipboard()->setText("before");
    EXPECT_TRUE(dialog.copyJson().isEmpty());
    EXPECT_EQ(QApplication::clipboard()->text(), "before");
    EXPECT_EQ(label(dialog, "drawingSummaryStatus"), "STATUS JSON was refused: no");
}

TEST(DrawingSummaryDialog, ItOffersNoButtonForTheRemovedLoadCustomisationItem)
{
    // The summary had a Load Customisation button that triggered the Format
    // menu's item of that name. The item is gone - a customisation file is
    // loaded by the CUSTOMISE line - and a button left behind would trigger
    // a name that no longer exists, and fail without a word.
    Document document;
    DrawingSummaryContext context;
    context.document = &document;
    DrawingSummaryDialog dialog(std::move(context));
    EXPECT_EQ(dialog.findChild<QWidget*>("drawingSummaryLoad"), nullptr);
    for (const QPushButton* each : dialog.findChildren<QPushButton*>()) {
        EXPECT_FALSE(each->text().contains("Load Customisation")) << each->text().toStdString();
    }
}

TEST(DrawingSummaryDialog, ADocumentThatGoesFirstLeavesTheDialogSafe)
{
    auto document = std::make_unique<Document>();
    DrawingSummaryContext context;
    context.document = document.get();
    DrawingSummaryDialog dialog(std::move(context));
    ASSERT_TRUE(CommandInterpreter(*document).run("POINT 1,1").ok());
    document.reset(); // a refresh is queued, and must deliver nothing
    katana::qt::test::processEvents();
    dialog.refresh();
    EXPECT_EQ(label(dialog, "drawingSummaryDrawing"), "0 entities, 1 layer, 0 alignments, 0 sheets");
}

} // namespace
