// The drawing register and the revision table in the sheet editor
// (src/katana_qt/sheet_editor.cpp): the Generate Sheets choice that puts a
// register cover first, the two views on the Add View menu, and the revision
// table's "newest only" setting - each ONE undoable step, each reachable by
// its object name. Driven on the offscreen platform.

#include <gtest/gtest.h>

#include <vector>

#include <QAction>
#include <QApplication>
#include <QComboBox>
#include <QDialog>
#include <QSpinBox>
#include <QTimer>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/plotting/tables.hpp"
#include "sheet_editor.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::qt::SheetEditor;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;

namespace {

SheetEditor::SourceProvider sourceOf(const Document& document)
{
    return [&document] {
        SheetSource source;
        source.plan = katana::qt::planSourceOf(document);
        source.revision = document.modelRevision();
        return source;
    };
}

void addNamedSheet(Document& document, std::string name)
{
    plotting::Sheet sheet;
    sheet.name = std::move(name);
    ASSERT_TRUE(plotting::addSheet(document, sheet).ok());
}

struct Shown {
    explicit Shown(Document& document) : editor(document, sourceOf(document))
    {
        editor.resize(1400, 900);
        editor.onMessage = [this](const QString& text, bool error) {
            messages.push_back({text, error});
        };
        editor.show();
        katana::qt::test::processEvents();
    }
    SheetEditor editor;
    std::vector<std::pair<QString, bool>> messages;
};

// Answers the next Generate Sheets dialog with the layout at `choice`,
// caught as it opens.
struct GenerateAnswer {
    QTimer timer;
    bool seen = false;
    bool replaceEnabled = true;

    explicit GenerateAnswer(int choice)
    {
        timer.setInterval(10);
        QObject::connect(&timer, &QTimer::timeout, [this, choice] {
            auto* dialog = qobject_cast<QDialog*>(QApplication::activeModalWidget());
            if (dialog == nullptr) {
                return;
            }
            auto* layout = dialog->findChild<QComboBox*>(QStringLiteral("sheetGenerateLayout"));
            if (layout == nullptr) {
                dialog->reject();
                return;
            }
            seen = true;
            layout->setCurrentIndex(choice);
            // The register goes in front of the sheets there are: "replace"
            // makes no sense for it and is turned off.
            for (QWidget* child : dialog->findChildren<QWidget*>()) {
                if (child->property("text").toString().startsWith(QStringLiteral("Replace"))) {
                    replaceEnabled = child->isEnabled();
                }
            }
            timer.stop();
            dialog->accept();
        });
        timer.start();
    }
};

constexpr int kRegisterChoice = 6;

} // namespace

TEST(SheetRegisterEditor, GenerateSheetsPutsTheRegisterCoverFirstInOneStep)
{
    Document document;
    addNamedSheet(document, "PLAN 1");
    addNamedSheet(document, "PLAN 2");
    const plotting::SheetSet before = document.sheetSet();
    Shown shown(document);
    shown.editor.setCurrentSheet(1);
    {
        GenerateAnswer answer(kRegisterChoice);
        shown.editor.generateSheets();
        ASSERT_TRUE(answer.seen);
        EXPECT_FALSE(answer.replaceEnabled);
    }
    const plotting::SheetSet& set = document.sheetSet();
    ASSERT_EQ(set.sheets.size(), 3u);
    EXPECT_EQ(set.sheets[0].name, "DRAWING REGISTER");
    EXPECT_EQ(set.sheets[0].viewports[0].kind, plotting::ViewportKind::SheetIndex);
    EXPECT_EQ(set.sheets[1].name, "PLAN 1");
    // The editor shows the new cover.
    EXPECT_EQ(shown.editor.currentSheet(), 0u);
    ASSERT_FALSE(shown.messages.empty());
    EXPECT_FALSE(shown.messages.back().second);

    // A second register is refused, said so, and changes nothing.
    {
        GenerateAnswer answer(kRegisterChoice);
        shown.editor.generateSheets();
        ASSERT_TRUE(answer.seen);
    }
    EXPECT_EQ(document.sheetSet().sheets.size(), 3u);
    ASSERT_FALSE(shown.messages.empty());
    EXPECT_TRUE(shown.messages.back().second);
    EXPECT_TRUE(shown.messages.back().first.contains(QStringLiteral("already has a drawing register")));

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.sheetSet(), before);
}

TEST(SheetRegisterEditor, AddViewOffersTheRegisterAndTheRevisionTableEachInOneStep)
{
    Document document;
    addNamedSheet(document, "COVER");
    Shown shown(document);
    auto* addRegister = shown.editor.findChild<QAction*>(QStringLiteral("sheetAddView_sheet_index"));
    auto* addRevisions = shown.editor.findChild<QAction*>(QStringLiteral("sheetAddView_revisions"));
    ASSERT_NE(addRegister, nullptr);
    ASSERT_NE(addRevisions, nullptr);
    EXPECT_EQ(addRegister->text(), QStringLiteral("Drawing register"));
    EXPECT_EQ(addRevisions->text(), QStringLiteral("Revision table"));

    const std::size_t steps = document.history().undoCount();
    addRegister->trigger();
    addRevisions->trigger();
    EXPECT_EQ(document.history().undoCount(), steps + 2);
    const plotting::Sheet& sheet = document.sheetSet().sheets[0];
    ASSERT_EQ(sheet.viewports.size(), 2u);
    EXPECT_EQ(sheet.viewports[0].kind, plotting::ViewportKind::SheetIndex);
    EXPECT_EQ(sheet.viewports[1].kind, plotting::ViewportKind::Revisions);
    // Placed on the paper, inside the drawing area, not over each other.
    const Box2 area = plotting::drawingArea(sheet);
    for (const plotting::Viewport& view : sheet.viewports) {
        EXPECT_FALSE(view.rect.empty());
        EXPECT_TRUE(area.inflated(1e-9).contains(view.rect));
    }
    // The canvas paints both without a complaint.
    katana::qt::test::paint(*shown.editor.canvas());
    EXPECT_TRUE(shown.editor.canvas()->lastStats().problems.empty());
    EXPECT_EQ(shown.editor.canvas()->lastStats().viewportsDrawn, 2u);
}

TEST(SheetRegisterEditor, TheNewestRevisionsSettingIsOneStep)
{
    Document document;
    ASSERT_TRUE(plotting::addRegisterSheet(document).ok());
    Shown shown(document);
    const std::string id = document.sheetSet().sheets[0].viewports[1].id;
    shown.editor.canvas()->select(id);
    auto* limit = shown.editor.findChild<QSpinBox*>(QStringLiteral("sheetRevisionLimit"));
    ASSERT_NE(limit, nullptr);
    EXPECT_EQ(limit->value(), 0);
    EXPECT_EQ(limit->text(), QStringLiteral("All"));

    const std::size_t steps = document.history().undoCount();
    limit->setValue(3);
    katana::qt::test::processEvents();
    EXPECT_EQ(document.sheetSet().sheets[0].viewports[1].revisionLimit, 3u);
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    // The panel is rebuilt showing the stored value.
    auto* rebuilt = shown.editor.findChild<QSpinBox*>(QStringLiteral("sheetRevisionLimit"));
    ASSERT_NE(rebuilt, nullptr);
    EXPECT_EQ(rebuilt->value(), 3);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.sheetSet().sheets[0].viewports[1].revisionLimit, 0u);
}
