// The Plot dialog and the plot's progress (src/katana_qt/plotting/
// plot_dialog): what the controls make of a request, what they refuse, the
// progress dialog's Cancel, the page setup kept as one step, and the sheet
// editor's Plot buttons opening the dialog. Driven by object names on the
// offscreen platform, as a person or a scripted session drives it.

#include <gtest/gtest.h>

#include <functional>
#include <string>
#include <utility>
#include <vector>

#include <QAction>
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDir>
#include <QDoubleSpinBox>
#include <QLabel>
#include <QLineEdit>
#include <QProgressDialog>
#include <QPushButton>
#include <QRadioButton>
#include <QSpinBox>
#include <QTemporaryDir>
#include <QTimer>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/page_setup.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "plotting/plot_dialog.hpp"
#include "sheet_editor.hpp"

using katana::cad::Document;
using katana::cad::PlotColourMode;
using katana::qt::PlotDialog;
using katana::qt::PlotFormat;
using katana::qt::PlotRequest;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;

namespace {

// Three A3 sheets, ONE, TWO and THREE, with nothing on them.
plotting::SheetSet threeSheets()
{
    plotting::SheetSet set;
    const char* names[] = {"ONE", "TWO", "THREE"};
    for (int i = 0; i < 3; ++i) {
        plotting::Sheet sheet;
        sheet.id = "s" + std::to_string(i + 1);
        sheet.name = names[i];
        set.sheets.push_back(sheet);
    }
    return set;
}

template <typename Widget>
Widget& child(QWidget& parent, const char* name)
{
    auto* found = parent.findChild<Widget*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << name;
    if (found == nullptr) {
        throw std::runtime_error(name);
    }
    return *found;
}

void chooseData(QComboBox& combo, const QString& data)
{
    const int at = combo.findData(data);
    ASSERT_GE(at, 0) << data.toStdString();
    combo.setCurrentIndex(at);
}

// The first shown top-level widget named `name`.
QWidget* shownWindow(const QString& name)
{
    for (QWidget* widget : QApplication::topLevelWidgets()) {
        if (widget->objectName() == name && widget->isVisible()) {
            return widget;
        }
    }
    return nullptr;
}

// Runs `act` on the Plot dialog once it is open (from the event loop its
// exec() runs), then closes it with `accept` or reject if `act` did not.
void whenTheDialogOpens(std::function<void(PlotDialog&)> act, bool* seen)
{
    QTimer::singleShot(0, [act = std::move(act), seen] {
        auto* dialog = dynamic_cast<PlotDialog*>(shownWindow(QStringLiteral("plotDialog")));
        ASSERT_NE(dialog, nullptr);
        *seen = true;
        act(*dialog);
        if (dialog->isVisible()) {
            dialog->reject();
        }
    });
}

SheetSource emptySource(const Document& document)
{
    SheetSource source;
    source.plan = katana::qt::planSourceOf(document);
    return source;
}

} // namespace

TEST(PlotDialog, EveryControlHasItsObjectName)
{
    PlotDialog dialog(threeSheets(), 0, true, QStringLiteral("C:/plots/set.pdf"));
    EXPECT_EQ(dialog.objectName(), "plotDialog");
    for (const char* name : {"plotSheetsAll", "plotSheetsCurrent", "plotSheetsList"}) {
        (void)child<QRadioButton>(dialog, name);
    }
    for (const char* name : {"plotSheetsText", "plotFile", "plotFolder", "plotPattern"}) {
        (void)child<QLineEdit>(dialog, name);
    }
    for (const char* name : {"plotFormat", "plotColourMode", "plotPrinter"}) {
        (void)child<QComboBox>(dialog, name);
    }
    for (const char* name : {"plotSheetsSummary", "plotPatternPreview", "plotProblem"}) {
        (void)child<QLabel>(dialog, name);
    }
    for (const char* name : {"plotBrowseFile", "plotBrowseFolder", "plotRun", "plotCancel"}) {
        (void)child<QPushButton>(dialog, name);
    }
    (void)child<QDoubleSpinBox>(dialog, "plotLineWeightScale");
    (void)child<QSpinBox>(dialog, "plotDpi");
    (void)child<QCheckBox>(dialog, "plotOpenAfter");
    (void)child<QCheckBox>(dialog, "plotKeepSetup");
}

TEST(PlotDialog, ItStartsFromThePageSetup)
{
    plotting::SheetSet set = threeSheets();
    set.pageSetup.colourMode = PlotColourMode::Greyscale;
    set.pageSetup.lineWeightScale = 0.7;
    set.pageSetup.dpi = 150.0;
    set.pageSetup.fileNamePattern = "{number} {name}";
    set.pageSetup.filePerSheet = true;
    PlotDialog dialog(set, 0, true, QStringLiteral("C:/plots/set.pdf"));
    EXPECT_EQ(child<QComboBox>(dialog, "plotFormat").currentData().toString(), "pdfs");
    EXPECT_EQ(child<QComboBox>(dialog, "plotColourMode").currentData().toString(), "greyscale");
    EXPECT_DOUBLE_EQ(child<QDoubleSpinBox>(dialog, "plotLineWeightScale").value(), 0.7);
    EXPECT_EQ(child<QSpinBox>(dialog, "plotDpi").value(), 150);

    const PlotRequest request = dialog.request();
    EXPECT_EQ(request.format, PlotFormat::PdfPerSheet);
    EXPECT_EQ(request.colourMode, PlotColourMode::Greyscale);
    EXPECT_DOUBLE_EQ(request.lineWeightScale, 0.7);
    EXPECT_DOUBLE_EQ(request.dpi, 150.0);
    EXPECT_EQ(request.fileNamePattern, "{number} {name}");
    EXPECT_EQ(request.sheets, "");
    // A file a sheet goes in the suggested file's folder.
    EXPECT_EQ(request.destination, "C:/plots");
    EXPECT_FALSE(dialog.toPrinter());
    EXPECT_TRUE(dialog.keepAsPageSetup());
    EXPECT_FALSE(dialog.openAfterwards());
    EXPECT_TRUE(dialog.check().ok());
    EXPECT_TRUE(child<QPushButton>(dialog, "plotRun").isEnabled());
    // Kept as it is, it is the page setup it came from.
    EXPECT_EQ(katana::qt::pageSetupFor(request, set.pageSetup), set.pageSetup);
}

TEST(PlotDialog, TheSheetsChosenBecomeTheSelection)
{
    PlotDialog dialog(threeSheets(), 1, false, QStringLiteral("C:/plots/set.pdf"));
    // Opened from Plot Sheet: the current sheet.
    EXPECT_TRUE(child<QRadioButton>(dialog, "plotSheetsCurrent").isChecked());
    EXPECT_EQ(dialog.request().sheets, "2");
    EXPECT_EQ(child<QLabel>(dialog, "plotSheetsSummary").text(), "1 sheet: 2");
    EXPECT_EQ(dialog.request().destination, "C:/plots/set.pdf");

    child<QRadioButton>(dialog, "plotSheetsAll").setChecked(true);
    EXPECT_EQ(dialog.request().sheets, "");
    EXPECT_EQ(child<QLabel>(dialog, "plotSheetsSummary").text(), "3 sheets");

    // Typing a list chooses it.
    QLineEdit& list = child<QLineEdit>(dialog, "plotSheetsText");
    list.setText(QStringLiteral("3, 1"));
    emit list.textEdited(list.text());
    EXPECT_TRUE(child<QRadioButton>(dialog, "plotSheetsList").isChecked());
    EXPECT_EQ(dialog.request().sheets, "3, 1");
    EXPECT_EQ(child<QLabel>(dialog, "plotSheetsSummary").text(), "2 sheets: 3,1");

    // A sheet that is not there: said, and Plot is off.
    list.setText(QStringLiteral("9"));
    emit list.textEdited(list.text());
    EXPECT_EQ(child<QLabel>(dialog, "plotProblem").text(), "there is no sheet 9: the set has 3 sheets");
    EXPECT_FALSE(child<QPushButton>(dialog, "plotRun").isEnabled());
    EXPECT_FALSE(dialog.check().ok());
}

TEST(PlotDialog, AFilePerSheetShowsTheFolderAndTheFirstFilesName)
{
    plotting::SheetSet set = threeSheets();
    set.defaults.setNumber = "C";
    PlotDialog dialog(set, 0, true, QStringLiteral("C:/plots/set.pdf"));
    dialog.show();
    EXPECT_TRUE(child<QLineEdit>(dialog, "plotFile").isVisibleTo(&dialog));
    EXPECT_FALSE(child<QLineEdit>(dialog, "plotFolder").isVisibleTo(&dialog));

    chooseData(child<QComboBox>(dialog, "plotFormat"), QStringLiteral("png"));
    EXPECT_FALSE(child<QLineEdit>(dialog, "plotFile").isVisibleTo(&dialog));
    EXPECT_TRUE(child<QLineEdit>(dialog, "plotFolder").isVisibleTo(&dialog));
    EXPECT_TRUE(child<QLineEdit>(dialog, "plotPattern").isVisibleTo(&dialog));
    EXPECT_EQ(child<QLabel>(dialog, "plotPatternPreview").text(), "First file: C01 ONE.png");

    child<QLineEdit>(dialog, "plotPattern").setText(QStringLiteral("{name}-{n}"));
    EXPECT_EQ(child<QLabel>(dialog, "plotPatternPreview").text(), "First file: ONE-1.png");
    child<QLineEdit>(dialog, "plotPattern").setText(QStringLiteral("{nope}"));
    EXPECT_FALSE(child<QPushButton>(dialog, "plotRun").isEnabled());
    EXPECT_NE(child<QLabel>(dialog, "plotProblem").text().indexOf("{nope}"), -1);

    // A raster too large to make is refused before it is tried.
    child<QLineEdit>(dialog, "plotPattern").setText(QStringLiteral("{n}"));
    child<QSpinBox>(dialog, "plotDpi").setValue(1200);
    EXPECT_FALSE(child<QPushButton>(dialog, "plotRun").isEnabled());
    EXPECT_NE(child<QLabel>(dialog, "plotProblem").text().indexOf("megapixels"), -1);
    child<QSpinBox>(dialog, "plotDpi").setValue(300);
    EXPECT_TRUE(child<QPushButton>(dialog, "plotRun").isEnabled());
    EXPECT_TRUE(child<QLabel>(dialog, "plotProblem").text().isEmpty());
}

TEST(PlotDialog, APrinterNeedsNoFileButMustBeThere)
{
    PlotDialog dialog(threeSheets(), 0, true, QStringLiteral("C:/plots/set.pdf"));
    dialog.show();
    chooseData(child<QComboBox>(dialog, "plotFormat"), QStringLiteral("printer"));
    EXPECT_TRUE(dialog.toPrinter());
    EXPECT_TRUE(dialog.request().destination.isEmpty());
    EXPECT_FALSE(child<QLineEdit>(dialog, "plotFile").isVisibleTo(&dialog));
    EXPECT_TRUE(child<QComboBox>(dialog, "plotPrinter").isVisibleTo(&dialog));
    EXPECT_FALSE(child<QCheckBox>(dialog, "plotOpenAfter").isEnabled());
    if (child<QComboBox>(dialog, "plotPrinter").count() == 0) {
        EXPECT_EQ(child<QLabel>(dialog, "plotProblem").text(), "there is no printer to print on");
        EXPECT_FALSE(child<QPushButton>(dialog, "plotRun").isEnabled());
    } else {
        EXPECT_TRUE(dialog.check().ok());
    }
}

TEST(PlotDialog, TheProgressDialogsCancelStopsThePlotBeforeTheNextSheet)
{
    Document document;
    const plotting::SheetSet set = threeSheets();
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    PlotRequest request;
    request.format = PlotFormat::Png;
    request.dpi = 50.0;
    request.destination = dir.filePath("png");

    // Uninterrupted: every sheet.
    const auto whole = katana::qt::plotWithProgress(nullptr, set, request,
                                                    [&document] { return emptySource(document); });
    ASSERT_TRUE(whole.ok()) << whole.error().describe();
    EXPECT_EQ(whole->files.size(), 3u);
    EXPECT_FALSE(whole->cancelled);

    // Cancel pressed while the first sheet is painted: the source is asked
    // for as each sheet starts, and that is when the progress dialog is
    // found and its Cancel pressed.
    request.destination = dir.filePath("cancelled");
    int sheetsStarted = 0;
    bool pressed = false;
    const auto cancelled = katana::qt::plotWithProgress(nullptr, set, request, [&] {
        if (++sheetsStarted == 1) {
            for (QWidget* widget : QApplication::topLevelWidgets()) {
                if (auto* progress = qobject_cast<QProgressDialog*>(widget);
                    progress != nullptr && progress->objectName() == "plotProgress") {
                    progress->cancel();
                    pressed = true;
                }
            }
        }
        return emptySource(document);
    });
    ASSERT_TRUE(pressed);
    ASSERT_TRUE(cancelled.ok()) << cancelled.error().describe();
    EXPECT_TRUE(cancelled->cancelled);
    EXPECT_EQ(cancelled->sheets.size(), 1u);
    EXPECT_EQ(QDir(dir.filePath("cancelled")).entryList(QDir::Files).size(), 1);
    // The progress dialog is gone with the plot.
    EXPECT_EQ(shownWindow(QStringLiteral("plotProgress")), nullptr);
}

TEST(PlotDialog, PlottingKeepsTheChoiceAsThePageSetupInOneStep)
{
    Document document;
    for (const plotting::Sheet& sheet : threeSheets().sheets) {
        ASSERT_TRUE(plotting::addSheet(document, sheet).ok());
    }
    QTemporaryDir dir;
    ASSERT_TRUE(dir.isValid());
    const QString folder = dir.filePath("mono");
    const std::size_t stepsBefore = document.history().undoCount();
    std::vector<std::pair<QString, bool>> messages;
    bool seen = false;
    whenTheDialogOpens(
        [&folder](PlotDialog& dialog) {
            chooseData(child<QComboBox>(dialog, "plotFormat"), QStringLiteral("png"));
            chooseData(child<QComboBox>(dialog, "plotColourMode"), QStringLiteral("monochrome"));
            child<QSpinBox>(dialog, "plotDpi").setValue(50);
            child<QLineEdit>(dialog, "plotFolder").setText(folder);
            child<QPushButton>(dialog, "plotRun").click();
        },
        &seen);
    katana::qt::plotInteractively(
        nullptr, document.sheetSet(), 0, true, dir.filePath("set.pdf"), QStringLiteral("TEST"),
        [&document] { return emptySource(document); }, &document,
        [&messages](const QString& text, bool error) { messages.emplace_back(text, error); });
    ASSERT_TRUE(seen);
    EXPECT_EQ(QDir(folder).entryList(QDir::Files).size(), 3);
    // The choice is the page setup now, as one step.
    const plotting::PageSetup& kept = document.sheetSet().pageSetup;
    EXPECT_EQ(kept.colourMode, PlotColourMode::Monochrome);
    EXPECT_EQ(kept.dpi, 50.0);
    EXPECT_FALSE(kept.filePerSheet); // a raster is not a page setup's format
    EXPECT_EQ(document.history().undoCount(), stepsBefore + 1);
    EXPECT_EQ(document.history().undoName(), "PAGE_SETUP");
    ASSERT_FALSE(messages.empty());
    EXPECT_EQ(messages.back().first,
              QString("Plotted 3 sheets to 3 PNG files in %1.").arg(QDir::toNativeSeparators(folder)));
    EXPECT_FALSE(messages.back().second);

    // Cancelled, the dialog does nothing at all.
    messages.clear();
    seen = false;
    whenTheDialogOpens([](PlotDialog& dialog) { dialog.reject(); }, &seen);
    katana::qt::plotInteractively(
        nullptr, document.sheetSet(), 0, true, dir.filePath("set.pdf"), QStringLiteral("TEST"),
        [&document] { return emptySource(document); }, &document,
        [&messages](const QString& text, bool error) { messages.emplace_back(text, error); });
    EXPECT_TRUE(seen);
    EXPECT_TRUE(messages.empty());
    EXPECT_EQ(document.history().undoCount(), stepsBefore + 1);
}

TEST(PlotDialog, TheEditorsPlotButtonsOpenIt)
{
    Document document;
    for (const plotting::Sheet& sheet : threeSheets().sheets) {
        ASSERT_TRUE(plotting::addSheet(document, sheet).ok());
    }
    katana::qt::SheetEditor editor(document, [&document] { return emptySource(document); });
    editor.setCurrentSheet(2);
    for (const auto& [action, allChosen] :
         {std::pair{"sheetPlotAll", "plotSheetsAll"}, std::pair{"sheetPlotSheet", "plotSheetsCurrent"}}) {
        auto* button = editor.findChild<QAction*>(QString::fromLatin1(action));
        ASSERT_NE(button, nullptr) << action;
        bool seen = false;
        std::string sheets;
        bool chosen = false;
        whenTheDialogOpens(
            [&](PlotDialog& dialog) {
                chosen = child<QRadioButton>(dialog, allChosen).isChecked();
                sheets = dialog.request().sheets;
            },
            &seen);
        button->trigger();
        EXPECT_TRUE(seen) << action;
        EXPECT_TRUE(chosen) << action;
        EXPECT_EQ(sheets, std::string(action) == "sheetPlotAll" ? "" : "3");
    }
}

TEST(PlotDialog, TheSuggestedFileIsNamedAfterTheProject)
{
    Document document;
    EXPECT_TRUE(katana::qt::suggestedPlotFile(document).endsWith("/sheets.pdf"));
    auto metadata = document.metadata();
    metadata.name = "Main Street: Stage 2";
    document.setMetadata(metadata);
    EXPECT_TRUE(katana::qt::suggestedPlotFile(document).endsWith("/Main Street- Stage 2.pdf"));
}
