// What a person reads about styles, symbols and survey codes is in general
// words: no text in the Format menu's actions or in its three managers names
// another program. Every text a widget can show is walked - titles, tips,
// button and label texts, placeholders, a combo's items and a view's cells
// and headers, an action's text and tips - over the committed hand-written
// fixture customisation (tests/data/customisation, read through
// fixture_customisation.hpp), with a style on a library linestyle and one on
// the plain line selected, so the texts that depend on a selection are shown
// too. The file dialogs the managers open are caught as they open, read and
// cancelled.
//
// Each test also asserts a text it expects in the new words, so a walk that
// found nothing cannot pass for one that found nothing wrong.
//
// The three managers are held to one thing more since they read and write
// Katana customisation files: none of their texts names a file of the older
// formats - a `.4d` or `.mapfile` suffix, a "survey code file", a "style
// library", a "symbol file". The Format menu's own items are not yet: the
// window's loading of a customisation is other work's.

#include <gtest/gtest.h>

#include <filesystem>
#include <memory>
#include <string>
#include <vector>

#include <QAbstractButton>
#include <QAbstractItemModel>
#include <QAbstractItemView>
#include <QAction>
#include <QApplication>
#include <QComboBox>
#include <QDialog>
#include <QDoubleSpinBox>
#include <QFileDialog>
#include <QFileInfo>
#include <QGroupBox>
#include <QLabel>
#include <QLineEdit>
#include <QMainWindow>
#include <QMenu>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QSpinBox>
#include <QTabWidget>
#include <QTextEdit>
#include <QTimer>
#include <QToolBar>

#include "customisation/code_manager.hpp"
#include "customisation/customisation_workbench.hpp"
#include "customisation/fixture_customisation.hpp"
#include "customisation/symbol_library.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/style_catalogue.hpp"
#include "katana/commands/entity_commands.hpp"
#include "style_manager.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::qt::CustomisationServices;
using katana::qt::CustomisationWorkbench;

namespace {

// One text a person can read, and where it was found.
struct Seen {
    QString where;
    QString text;
};

void add(std::vector<Seen>& seen, const QString& where, const QString& text)
{
    if (!text.isEmpty()) {
        seen.push_back({where, text});
    }
}

void addAction(std::vector<Seen>& seen, const QString& where, const QAction& action)
{
    const QString name = where + " action " + action.objectName();
    add(seen, name + " text", action.text());
    add(seen, name + " tooltip", action.toolTip());
    add(seen, name + " status tip", action.statusTip());
    add(seen, name + " what's this", action.whatsThis());
    add(seen, name + " icon text", action.iconText());
}

// Every cell below `parent`, in every role a person reads, and the headers.
void addModel(std::vector<Seen>& seen, const QString& where, const QAbstractItemModel& model,
              const QModelIndex& parent = {})
{
    if (!parent.isValid()) {
        for (int column = 0; column < model.columnCount(); ++column) {
            for (const int role : {int(Qt::DisplayRole), int(Qt::ToolTipRole)}) {
                add(seen, where + " header",
                    model.headerData(column, Qt::Horizontal, role).toString());
            }
        }
    }
    for (int row = 0; row < model.rowCount(parent); ++row) {
        for (int column = 0; column < model.columnCount(parent); ++column) {
            const QModelIndex index = model.index(row, column, parent);
            for (const int role : {int(Qt::DisplayRole), int(Qt::ToolTipRole),
                                   int(Qt::StatusTipRole), int(Qt::WhatsThisRole)}) {
                add(seen, where + " cell", index.data(role).toString());
            }
            if (column == 0 && model.hasChildren(index)) {
                addModel(seen, where, model, index);
            }
        }
    }
}

// Every text `root` and the widgets and actions under it can show.
std::vector<Seen> textsIn(QWidget& root)
{
    std::vector<Seen> seen;
    QList<QWidget*> widgets = root.findChildren<QWidget*>();
    widgets.prepend(&root);
    for (QWidget* widget : widgets) {
        const QString where = QString::fromLatin1(widget->metaObject()->className()) + " " +
                              widget->objectName();
        add(seen, where + " window title", widget->windowTitle());
        add(seen, where + " tooltip", widget->toolTip());
        add(seen, where + " status tip", widget->statusTip());
        add(seen, where + " what's this", widget->whatsThis());
        add(seen, where + " accessible name", widget->accessibleName());
        add(seen, where + " accessible description", widget->accessibleDescription());
        for (const QAction* action : widget->actions()) {
            addAction(seen, where, *action);
        }
        if (const auto* button = qobject_cast<QAbstractButton*>(widget)) {
            add(seen, where + " text", button->text());
        }
        if (const auto* label = qobject_cast<QLabel*>(widget)) {
            add(seen, where + " text", label->text());
        }
        if (const auto* line = qobject_cast<QLineEdit*>(widget)) {
            add(seen, where + " text", line->text());
            add(seen, where + " placeholder", line->placeholderText());
        }
        if (const auto* plain = qobject_cast<QPlainTextEdit*>(widget)) {
            add(seen, where + " text", plain->toPlainText());
            add(seen, where + " placeholder", plain->placeholderText());
        }
        if (const auto* rich = qobject_cast<QTextEdit*>(widget)) {
            add(seen, where + " text", rich->toPlainText());
            add(seen, where + " placeholder", rich->placeholderText());
        }
        if (const auto* group = qobject_cast<QGroupBox*>(widget)) {
            add(seen, where + " title", group->title());
        }
        if (const auto* spin = qobject_cast<QSpinBox*>(widget)) {
            add(seen, where + " prefix", spin->prefix());
            add(seen, where + " suffix", spin->suffix());
            add(seen, where + " special value", spin->specialValueText());
        }
        if (const auto* spin = qobject_cast<QDoubleSpinBox*>(widget)) {
            add(seen, where + " prefix", spin->prefix());
            add(seen, where + " suffix", spin->suffix());
            add(seen, where + " special value", spin->specialValueText());
        }
        if (const auto* tabs = qobject_cast<QTabWidget*>(widget)) {
            for (int tab = 0; tab < tabs->count(); ++tab) {
                add(seen, where + " tab", tabs->tabText(tab));
                add(seen, where + " tab tooltip", tabs->tabToolTip(tab));
            }
        }
        if (const auto* menu = qobject_cast<QMenu*>(widget)) {
            add(seen, where + " title", menu->title());
        }
        if (const auto* combo = qobject_cast<QComboBox*>(widget); combo && combo->model()) {
            add(seen, where + " placeholder", combo->placeholderText());
            addModel(seen, where + " item", *combo->model());
        }
        if (const auto* view = qobject_cast<QAbstractItemView*>(widget); view && view->model()) {
            addModel(seen, where, *view->model());
        }
    }
    for (const QAction* action : root.findChildren<QAction*>()) {
        addAction(seen, QStringLiteral("owned"), *action);
    }
    return seen;
}

// The property keys an archive import stores its string names, styles and
// breaklines under, which projects already hold: renaming them is a storage
// migration (decision D6), not a change of words, so a chooser that offers
// the properties an entity may carry offers these as they are. Only the key
// ALONE is let through; any sentence around it is still checked.
const QStringList kPersistedKeys{QStringLiteral("12d.name"), QStringLiteral("12d.style"),
                                 QStringLiteral("12d.breakline")};

// Fails once for each text naming another program, saying where it is.
void expectNoneNamesAnotherProgram(const std::vector<Seen>& seen)
{
    for (const Seen& each : seen) {
        if (kPersistedKeys.contains(each.text)) {
            continue;
        }
        EXPECT_FALSE(each.text.contains(QStringLiteral("1" "2d"), Qt::CaseInsensitive))
            << each.where.toStdString() << ": " << each.text.toStdString();
    }
}

// Fails once for each text that names a file of the formats the managers no
// longer read or write, in any letter case. What they read and write now is
// a "Katana customisation (*.customisation.json *.json)". "mapfile" is
// looked for bare, with no dot before it: "the mapfile" in a tip names that
// format as surely as its suffix does.
void expectNoneNamesAnOlderFormat(const std::vector<Seen>& seen)
{
    for (const Seen& each : seen) {
        for (const char* word :
             {".4d", "mapfile", "survey code file", "style library", "symbol file"}) {
            EXPECT_FALSE(each.text.contains(QString::fromLatin1(word), Qt::CaseInsensitive))
                << each.where.toStdString() << " says \"" << word
                << "\": " << each.text.toStdString();
        }
    }
}

bool anyContains(const std::vector<Seen>& seen, const QString& part)
{
    for (const Seen& each : seen) {
        if (each.text.contains(part)) {
            return true;
        }
    }
    return false;
}

// The next file dialog a click opens, caught as it opens: its title and
// filters read, then cancelled, so the click returns having chosen nothing.
struct FileDialogPeek {
    QTimer timer;
    bool seen = false;
    QString title;
    QStringList filters;
    QString file;

    FileDialogPeek()
    {
        timer.setInterval(10);
        QObject::connect(&timer, &QTimer::timeout, [this] {
            auto* dialog = qobject_cast<QFileDialog*>(QApplication::activeModalWidget());
            if (dialog == nullptr) {
                return;
            }
            seen = true;
            title = dialog->windowTitle();
            filters = dialog->nameFilters();
            const QStringList chosen = dialog->selectedFiles();
            file = chosen.isEmpty() ? QString() : QFileInfo(chosen.front()).fileName();
            timer.stop();
            dialog->reject();
        });
        timer.start();
    }

    [[nodiscard]] std::vector<Seen> texts(const QString& where) const
    {
        std::vector<Seen> out{{where + " title", title}, {where + " file", file}};
        for (const QString& filter : filters) {
            out.push_back({where + " filter", filter});
        }
        return out;
    }
};

// The Format menu and toolbar as MainWindow builds them: the workbench's
// actions made with their tips as the window's action factory makes them
// (status tip, and a tooltip naming the action), the window's own Layers
// action beside them. The fixture customisation is
// loaded, and the drawing has a style on a library linestyle, a style on
// the plain line ("1", as an archive import names it) and a point in each.
struct Bench {
    Document document;
    QMainWindow window;
    QMenu* menu = new QMenu("Format", &window);
    QToolBar* bar = new QToolBar("Format", &window);
    QAction* layers = new QAction("&Layers...", &window);
    std::unique_ptr<CustomisationWorkbench> bench;

    Bench()
    {
        katana::qt::test::installCustomisationFixtures(document);
        for (const auto& [name, linetype] : std::vector<std::pair<const char*, const char*>>{
                 {"Kerb", "TEST Dashed Kerb"}, {"Plain", "1"}}) {
            katana::entity::Style style;
            style.name = name;
            style.linetype = linetype;
            style.symbol = "TEST Survey Mark";
            must(katana::commands::createStyle(style));
            must(katana::commands::createPoint({0.0, 0.0}, {.layer = "0", .style = name}));
        }

        layers->setObjectName("formatLayers");
        CustomisationServices services;
        services.document = &document;
        services.makeAction = [this](katana::qt::Icon icon, const QString& text,
                                     const QString& tip, const QKeySequence&,
                                     const QString& name) {
            auto* action = new QAction(katana::qt::icon(icon), text, &window);
            action->setObjectName(name);
            action->setStatusTip(tip);
            action->setToolTip(QString("<b>%1</b><br>%2").arg(QString(text).remove('&'), tip));
            return action;
        };
        services.log = [](const QString&, bool) {};
        services.headless = [] { return true; };
        services.layers = layers;
        bench = std::make_unique<CustomisationWorkbench>(window, std::move(services), *menu, *bar);
    }

    void must(katana::commands::CommandPtr command)
    {
        const auto status = document.execute(std::move(command));
        EXPECT_TRUE(status.ok()) << status.error().describe();
    }
};

template <typename T> T* child(QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << name;
    return found;
}

} // namespace

TEST(GeneralTexts, NoFormatMenuItemOrToolbarButtonNamesAnotherProgramInItsTextOrTips)
{
    Bench bench;
    std::vector<Seen> seen;
    add(seen, "menu title", bench.menu->title());
    for (const QAction* action : bench.menu->actions()) {
        addAction(seen, "menu", *action);
    }
    for (const QAction* action : bench.bar->actions()) {
        addAction(seen, "toolbar", *action);
    }
    // The workbench's five, each with its tips. (The window's Load and
    // Replace Loaded Customisation stood beside them until the files they
    // loaded stopped being read; no menu item loads a file here now.)
    for (const char* name : {"formatStyles", "formatSymbols", "formatSurveyCodes", "formatPurge",
                             "formatGlobalModify"}) {
        EXPECT_NE(bench.window.findChild<QAction*>(name), nullptr) << name;
    }
    for (const char* name : {"loadCustomisation", "replaceCustomisation"}) {
        EXPECT_EQ(bench.window.findChild<QAction*>(name), nullptr) << name;
    }
    expectNoneNamesAnotherProgram(seen);
    // Nor a file of a format the window no longer reads: the Survey Code
    // Manager's tip said "the loaded survey code files" while it loaded them.
    expectNoneNamesAnOlderFormat(seen);
    EXPECT_TRUE(anyContains(seen, "the loaded library linestyles"));
    EXPECT_TRUE(anyContains(seen, "The survey codes of the session's customisation"));
}

TEST(GeneralTexts, NoTextTheStyleManagerShowsNamesAnotherProgram)
{
    Bench bench;
    bench.window.findChild<QAction*>("formatStyles")->trigger();
    katana::qt::StyleManagerDialog* dialog = bench.bench->styleManager();
    ASSERT_NE(dialog, nullptr);
    katana::qt::test::processEvents();

    // Each selection shows texts of its own: a style on the plain line (the
    // picker's plain-line tip), and a library linestyle (its kind explained).
    dialog->selectStyles({"Plain"});
    katana::qt::test::processEvents();
    std::vector<Seen> seen = textsIn(*dialog);
    dialog->selectLinetype("TEST Dashed Kerb", katana::cad::LinetypeOrigin::Library);
    katana::qt::test::processEvents();
    for (Seen& each : textsIn(*dialog)) {
        seen.push_back(std::move(each));
    }

    expectNoneNamesAnotherProgram(seen);
    expectNoneNamesAnOlderFormat(seen);
    EXPECT_GT(seen.size(), 100u) << "the walk found the dialog's texts";
    EXPECT_TRUE(anyContains(seen, "Paper linestyle: millimetres on the plot"));
    EXPECT_TRUE(anyContains(seen, "Library linestyles belong to the session's customisation"));
    // Where a library linestyle came from is a customisation, by its name:
    // TEST Dashed Kerb is test_linestyles'. It used to be a file.
    EXPECT_TRUE(anyContains(seen, "Customisation: test_linestyles"));
    EXPECT_FALSE(child<QLabel>(*dialog, "libraryDetails")->text().contains("File:"));
}

TEST(GeneralTexts, NoTextTheSymbolLibraryOrItsFileDialogsShowNamesAnotherProgram)
{
    Bench bench;
    bench.window.findChild<QAction*>("formatSymbols")->trigger();
    katana::qt::SymbolLibraryDialog* dialog = bench.bench->symbolLibrary();
    ASSERT_NE(dialog, nullptr);
    katana::qt::test::processEvents();
    ASSERT_TRUE(dialog->selectSymbol("TEST Survey Mark"));
    katana::qt::test::processEvents();
    std::vector<Seen> seen = textsIn(*dialog);

    // The file dialogs a person gets from Import and Export, opened for real.
    dialog->setHeadless(false);
    for (const char* name : {"importDefinitions", "exportSelected"}) {
        auto* button = child<QPushButton>(*dialog, name);
        ASSERT_NE(button, nullptr);
        ASSERT_TRUE(button->isEnabled()) << name;
        FileDialogPeek peek;
        button->click();
        ASSERT_TRUE(peek.seen) << name << " opened no file dialog";
        for (Seen& each : peek.texts(QString::fromLatin1(name))) {
            seen.push_back(std::move(each));
        }
        EXPECT_TRUE(peek.filters.contains(
            QStringLiteral("Katana customisation (*.customisation.json *.json)")))
            << name << ": " << peek.filters.join(" ;; ").toStdString();
    }

    expectNoneNamesAnotherProgram(seen);
    expectNoneNamesAnOlderFormat(seen);
    EXPECT_GT(seen.size(), 50u) << "the walk found the dialog's texts";
    EXPECT_TRUE(anyContains(
        seen, "Merge the linestyle and symbol definitions of a Katana customisation file"));
    EXPECT_TRUE(anyContains(seen, "Import Definitions from a Katana Customisation"));
    EXPECT_TRUE(anyContains(seen, "symbols.customisation.json"));
}

TEST(GeneralTexts, NoTextTheSurveyCodeManagerOrItsFileDialogsShowNamesAnotherProgram)
{
    Bench bench;
    bench.window.findChild<QAction*>("formatSurveyCodes")->trigger();
    katana::qt::SurveyCodeManagerDialog* dialog = bench.bench->codeManager();
    ASSERT_NE(dialog, nullptr);
    katana::qt::test::processEvents();
    // AC01 meets AC* (rule #2), whose linestyle "0" is the plain line: the
    // explanation says what that is.
    auto* testCode = child<QLineEdit>(*dialog, "testCode");
    ASSERT_NE(testCode, nullptr);
    testCode->setText(QStringLiteral("AC01"));
    katana::qt::test::processEvents();
    std::vector<Seen> seen = textsIn(*dialog);

    dialog->setInteractive(true);
    for (const char* name : {"importCodes", "exportCodes", "exportCodeList"}) {
        auto* button = child<QPushButton>(*dialog, name);
        ASSERT_NE(button, nullptr);
        FileDialogPeek peek;
        button->click();
        ASSERT_TRUE(peek.seen) << name << " opened no file dialog";
        for (Seen& each : peek.texts(QString::fromLatin1(name))) {
            seen.push_back(std::move(each));
        }
    }
    dialog->setInteractive(false);

    expectNoneNamesAnotherProgram(seen);
    expectNoneNamesAnOlderFormat(seen);
    EXPECT_GT(seen.size(), 100u) << "the walk found the dialog's texts";
    EXPECT_TRUE(anyContains(seen, "the plain continuous line"));
    EXPECT_TRUE(anyContains(seen, "Katana customisation (*.customisation.json *.json)"));
    EXPECT_TRUE(anyContains(seen, "Import Survey Codes"));
    EXPECT_TRUE(anyContains(seen, "Export Survey Codes"));
}
