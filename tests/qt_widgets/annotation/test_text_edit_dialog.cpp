// Annotate > Edit Text... (src/katana_qt/annotation/text_edit_dialog.hpp),
// driven by object name and checked by the TEXTEDIT line it runs: only the
// fields the form changed, one line, one undo step. The runner is the
// interpreter itself, so a line is checked by what the verb makes of it.

#include <gtest/gtest.h>

#include <variant>

#include <QComboBox>
#include <QLineEdit>
#include <QMainWindow>
#include <QMenu>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QToolBar>

#include "annotation/annotation_workbench.hpp"
#include "annotation/text_edit_dialog.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/math/numerics.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::entity::TextGeometry;
using katana::entity::TextJustify;
using katana::qt::TextEditDialog;
using katana::qt::VerbOutcome;

namespace {

struct Runner {
    explicit Runner(Document& document) : interpreter(document) {}
    CommandInterpreter interpreter;
    QStringList lines;

    katana::qt::CommandRunner runner()
    {
        return [this](const QString& line) {
            lines << line;
            const auto reply = interpreter.run(line.toStdString());
            if (!reply) {
                return VerbOutcome{false, {}, QString::fromStdString(reply.error().describe())};
            }
            return VerbOutcome{true, QString::fromStdString(*reply), {}};
        };
    }
    void ok(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        ASSERT_TRUE(reply.ok()) << line << ": " << reply.error().describe();
    }
};

template <typename Widget> Widget* child(const QWidget& dialog, const char* name)
{
    auto* widget = dialog.findChild<Widget*>(QString::fromLatin1(name));
    EXPECT_NE(widget, nullptr) << name;
    return widget;
}

void select(Document& document, std::vector<katana::entity::EntityId> ids)
{
    document.selection().set(std::move(ids));
    document.notifySelectionChanged();
}

const TextGeometry& textOf(const Document& document, katana::entity::EntityId id)
{
    return std::get<TextGeometry>(document.model().entities.find(id)->geometry);
}

} // namespace

TEST(TextEditDialog, WithNoTextSelectedItAsksForOneAndRunsNothing)
{
    Document document;
    Runner run(document);
    run.ok("TEXT 0,0 2.5 old");
    run.ok("LINE 0,0 10,0");
    TextEditDialog dialog(document, run.runner());
    EXPECT_EQ(dialog.objectName(), QStringLiteral("textEditDialog"));
    EXPECT_EQ(dialog.textId(), 0u);
    EXPECT_EQ(dialog.status(), QStringLiteral("Select one text in the drawing to edit it."));
    EXPECT_FALSE(child<QPushButton>(dialog, "textEditApply")->isEnabled());
    EXPECT_FALSE(dialog.apply());
    EXPECT_TRUE(run.lines.isEmpty());

    select(document, {2});
    EXPECT_TRUE(dialog.status().contains(QStringLiteral("not a text")))
        << dialog.status().toStdString();
    select(document, {1, 2});
    EXPECT_TRUE(dialog.status().contains(QStringLiteral("2 entities")))
        << dialog.status().toStdString();
    select(document, {1});
    EXPECT_EQ(dialog.textId(), 1u) << "it follows the selection";
    EXPECT_EQ(dialog.status(), QStringLiteral("Text 1"));
    EXPECT_EQ(child<QPlainTextEdit>(dialog, "textEditText")->toPlainText(), QStringLiteral("old"));
}

TEST(TextEditDialog, EveryFieldByNameIsOneLineOfTheChangedKeysAndOneStep)
{
    Document document;
    Runner run(document);
    run.ok("TEXTSTYLE NEW Notes paper=3.5");
    run.ok("TEXT 10,20 2.5 old");
    select(document, {1});
    TextEditDialog dialog(document, run.runner());
    ASSERT_EQ(dialog.textId(), 1u);
    EXPECT_EQ(child<QLineEdit>(dialog, "textEditPosition")->text(), QStringLiteral("10,20"));
    EXPECT_EQ(child<QLineEdit>(dialog, "textEditHeight")->text(), QStringLiteral("2.5"));
    EXPECT_EQ(child<QComboBox>(dialog, "textEditStyle")->currentText(), QStringLiteral("(none)"));
    EXPECT_EQ(child<QComboBox>(dialog, "textEditJustify")->currentText(), QStringLiteral("BL"));

    child<QPlainTextEdit>(dialog, "textEditText")->setPlainText(QStringLiteral("PIT 12\nIL 10.50"));
    auto* style = child<QComboBox>(dialog, "textEditStyle");
    style->setCurrentIndex(style->findText(QStringLiteral("Notes")));
    auto* justify = child<QComboBox>(dialog, "textEditJustify");
    justify->setCurrentIndex(justify->findText(QStringLiteral("MC")));
    child<QLineEdit>(dialog, "textEditRotation")->setText(QStringLiteral("30"));
    child<QLineEdit>(dialog, "textEditPosition")->setText(QStringLiteral("5, 6"));

    const std::size_t before = document.history().undoCount();
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    ASSERT_EQ(run.lines.size(), 1);
    EXPECT_EQ(run.lines.front(), QStringLiteral("TEXTEDIT 1 text=\"PIT 12\\nIL 10.50\" style=Notes "
                                                "justify=MC rotation=30 at=5,6"))
        << "only the changed keys: the height and paper were left alone";
    EXPECT_EQ(document.history().undoCount(), before + 1);
    const TextGeometry& text = textOf(document, 1);
    EXPECT_EQ(text.text, "PIT 12\nIL 10.50");
    EXPECT_EQ(text.style, "Notes");
    EXPECT_EQ(text.justify, TextJustify::MiddleCentre);
    EXPECT_DOUBLE_EQ(text.rotation, 30.0 * katana::math::kDegToRad);
    EXPECT_EQ(text.position, katana::geometry::Point2(5.0, 6.0));
    // Paper-sized now: 3.5 mm at the default 1:1000 is 3.5 m.
    EXPECT_DOUBLE_EQ(text.height, 3.5);
    EXPECT_EQ(child<QLineEdit>(dialog, "textEditHeight")->text(), QStringLiteral("3.5"))
        << "the form shows what is stored now";

    EXPECT_TRUE(dialog.apply()) << "an unchanged form";
    EXPECT_EQ(run.lines.size(), 1) << "runs nothing";

    // The style back to none, and a paper height of its own.
    style->setCurrentIndex(0);
    child<QLineEdit>(dialog, "textEditPaper")->setText(QStringLiteral("5"));
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(run.lines.back(), QStringLiteral("TEXTEDIT 1 style=\"\" paper=5"));
    EXPECT_EQ(textOf(document, 1).style, "");
    EXPECT_EQ(textOf(document, 1).paperHeight, 5.0);

    ASSERT_TRUE(document.undo().ok());
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(textOf(document, 1).text, "old") << "each Apply was one step";
    EXPECT_EQ(child<QPlainTextEdit>(dialog, "textEditText")->toPlainText(), QStringLiteral("old"));
}

TEST(TextEditDialog, AValueNoLineCanCarryIsRefusedBeforeAnythingRuns)
{
    Document document;
    Runner run(document);
    run.ok("TEXT 0,0 2.5 old");
    select(document, {1});
    TextEditDialog dialog(document, run.runner());
    child<QPlainTextEdit>(dialog, "textEditText")->setPlainText(QStringLiteral("5\" pipe"));
    EXPECT_FALSE(dialog.apply());
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("double quote")))
        << dialog.problem().toStdString();
    dialog.revert();
    child<QLineEdit>(dialog, "textEditHeight")->setText(QStringLiteral("0"));
    EXPECT_FALSE(dialog.apply());
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("greater than zero")));
    child<QLineEdit>(dialog, "textEditHeight")->setText(QStringLiteral("2.5"));
    child<QLineEdit>(dialog, "textEditPosition")->setText(QStringLiteral("east"));
    EXPECT_FALSE(dialog.apply());
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("E,N")));
    child<QPlainTextEdit>(dialog, "textEditText")->setPlainText(QStringLiteral("  "));
    EXPECT_FALSE(dialog.apply());
    EXPECT_TRUE(run.lines.isEmpty());
    EXPECT_EQ(textOf(document, 1).text, "old");
}

TEST(TextEditDialog, EditsAreKeptThroughChangesElsewhereAndReplacedWhenTheTextChanges)
{
    Document document;
    Runner run(document);
    run.ok("TEXT 0,0 2.5 old");
    select(document, {1});
    TextEditDialog dialog(document, run.runner());
    auto* words = child<QPlainTextEdit>(dialog, "textEditText");
    words->setPlainText(QStringLiteral("typing"));
    run.ok("LINE 0,0 10,0");
    EXPECT_EQ(words->toPlainText(), QStringLiteral("typing")) << "another entity changed";
    run.ok("TEXTEDIT 1 text=typed");
    EXPECT_EQ(words->toPlainText(), QStringLiteral("typed")) << "this text changed";
    words->setPlainText(QStringLiteral("again"));
    dialog.revert();
    EXPECT_EQ(words->toPlainText(), QStringLiteral("typed"));
}

TEST(TextEditDialog, TheAnnotateMenuOpensItByName)
{
    Document document;
    QMainWindow window;
    QMenu format;
    QMenu annotate;
    QToolBar toolBar;
    katana::qt::AnnotationWorkbench workbench(window, document, format, toolBar);
    Runner run(document);
    workbench.setCommandRunner(run.runner());
    QAction* action = workbench.addEditTextAction(annotate);
    ASSERT_NE(action, nullptr);
    EXPECT_EQ(action->objectName(), QStringLiteral("annotateEditText"));
    EXPECT_EQ(action->data().toString(), QStringLiteral("textEditDialog"))
        << "what the headless --dialog opens it by";
    EXPECT_TRUE(annotate.actions().contains(action));
    run.ok("TEXT 0,0 2.5 old");
    select(document, {1});
    action->trigger();
    TextEditDialog& dialog = workbench.showTextEdit();
    EXPECT_EQ(dialog.textId(), 1u);
    child<QPlainTextEdit>(dialog, "textEditText")->setPlainText(QStringLiteral("new"));
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(run.lines, QStringList{QStringLiteral("TEXTEDIT 1 text=new")});
}
