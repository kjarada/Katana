// The view options the sheet editor's properties gained for the VIEW SET
// options only a typed line reached (src/katana_qt/sheet_editor.cpp,
// docs/plotting.md "The sheet editor"): the view's place on the paper typed
// in millimetres, cross sections cut every interval over a chainage range,
// the hidden layers of a section view, a picture chosen for an image view,
// and Choose Paper at a scale with its advice shown first. Each builds the
// VIEW SET or SHEET SUGGESTPAPER line and runs it through the editor
// (SheetEditor::runLine), so each is ONE undoable step, refused as the typed
// line is, and in the window echoed in the log. Driven by object name on the
// offscreen platform.

#include <gtest/gtest.h>

#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

#include <QApplication>
#include <QComboBox>
#include <QDialog>
#include <QDoubleSpinBox>
#include <QFileDialog>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPushButton>
#include <QRadioButton>
#include <QTimer>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/plotting/section_fit.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/plotting/sheet_verbs.hpp"
#include "katana/commands/entity_commands.hpp"
#include "plotting/sheet_arrange.hpp"
#include "sheet_editor.hpp"
#include "widget_harness.hpp"

namespace fs = std::filesystem;
using katana::cad::Document;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::qt::SheetEditor;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;
namespace cmd = katana::commands;

namespace {

SheetEditor::SourceProvider sourceOf(const Document& document)
{
    return [&document] {
        SheetSource source;
        source.plan = katana::qt::planSourceOf(document);
        source.document = &document;
        source.revision = document.modelRevision();
        return source;
    };
}

plotting::Viewport viewportAt(std::string id, plotting::ViewportKind kind, Box2 rect)
{
    plotting::Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = kind;
    viewport.rect = rect;
    return viewport;
}

void addSheetWith(Document& document, std::vector<plotting::Viewport> viewports)
{
    plotting::Sheet sheet;
    sheet.id = "s1";
    sheet.name = "TEST";
    sheet.viewports = std::move(viewports);
    ASSERT_TRUE(plotting::addSheet(document, sheet).ok());
}

const plotting::Viewport& onlyViewport(const Document& document)
{
    return document.sheetSet().sheets.at(0).viewports.at(0);
}

// A road east from (0, 0) to (1000, 0): chainages 0 to 1000.
void road(Document& document)
{
    katana::entity::Alignment alignment;
    alignment.name = "ROAD";
    alignment.horizontal.pis = {{Point2(0.0, 0.0)}, {Point2(1000.0, 0.0)}};
    ASSERT_TRUE(document.execute(cmd::createAlignment(alignment)).ok());
}

void addLayer(Document& document, const std::string& name)
{
    katana::entity::Layer layer;
    layer.name = name;
    ASSERT_TRUE(document.execute(cmd::createLayer(layer)).ok());
}

// An editor shown with vp1 selected, collecting what it says.
struct Shown {
    explicit Shown(Document& document, const char* select = "vp1")
        : editor(document, sourceOf(document))
    {
        editor.onMessage = [this](const QString& text, bool error) {
            messages.push_back(text);
            errors += error ? 1 : 0;
        };
        editor.resize(1400, 900);
        editor.show();
        katana::qt::test::processEvents();
        if (select != nullptr) {
            editor.canvas()->select(select);
            katana::qt::test::processEvents();
        }
    }

    template <typename Widget> Widget* find(const char* name) const
    {
        auto* widget = editor.findChild<Widget*>(QString::fromLatin1(name));
        EXPECT_NE(widget, nullptr) << name;
        return widget;
    }

    // A spin box given a value and left, as a person tabs out of it.
    void type(const char* name, double value)
    {
        auto* box = find<QDoubleSpinBox>(name);
        ASSERT_NE(box, nullptr);
        box->setValue(value);
        emit box->editingFinished();
        katana::qt::test::processEvents();
    }

    [[nodiscard]] bool said(const QString& text) const
    {
        for (const QString& message : messages) {
            if (message == text) {
                return true;
            }
        }
        return false;
    }

    SheetEditor editor;
    std::vector<QString> messages;
    int errors = 0;
};

// A directory of the test's own, removed by name.
struct ScratchDirectory {
    fs::path path;
    explicit ScratchDirectory(const std::string& name)
        : path(fs::temp_directory_path() / ("katana-view-options-" + name))
    {
        fs::remove_all(path);
        fs::create_directories(path);
    }
    ~ScratchDirectory() { fs::remove_all(path); }
};

// The next file dialog answered with `file`, as a person picks it.
// The first file dialog is answered with `file`; one that is still up, or
// another, is cancelled, so that a choice that did not take fails the test
// rather than leaving it waiting.
struct FileAnswer {
    QTimer timer;
    bool seen = false;
    explicit FileAnswer(const QString& file)
    {
        timer.setInterval(10);
        QObject::connect(&timer, &QTimer::timeout, [this, file] {
            auto* dialog = qobject_cast<QFileDialog*>(QApplication::activeModalWidget());
            if (dialog == nullptr) {
                return;
            }
            if (seen) {
                dialog->reject();
                return;
            }
            seen = true;
            katana::qt::test::chooseFile(*dialog, file);
        });
        timer.start();
    }
};

} // namespace

TEST(SheetViewOptions, TheViewsPlaceOnThePaperIsTypedAsViewSetRectInOneStep)
{
    Document document;
    addSheetWith(document, {viewportAt("vp1", plotting::ViewportKind::Notes,
                                       Box2(Point2(30.0, 40.0), Point2(80.0, 80.0)))});
    Shown shown(document);
    ASSERT_NE(shown.find<QDoubleSpinBox>("sheetViewportX"), nullptr);
    EXPECT_EQ(shown.find<QDoubleSpinBox>("sheetViewportX")->value(), 30.0);
    EXPECT_EQ(shown.find<QDoubleSpinBox>("sheetViewportY")->value(), 40.0);
    EXPECT_EQ(shown.find<QDoubleSpinBox>("sheetViewportW")->value(), 50.0);
    EXPECT_EQ(shown.find<QDoubleSpinBox>("sheetViewportH")->value(), 40.0);
    const std::size_t steps = document.history().undoCount();

    // Left as it was: nothing written.
    shown.type("sheetViewportX", 30.0);
    EXPECT_EQ(document.history().undoCount(), steps);
    EXPECT_TRUE(shown.messages.empty());

    // Wider by 50 mm: the right edge moves, the left stays.
    shown.type("sheetViewportW", 100.0);
    EXPECT_EQ(onlyViewport(document).rect, Box2(Point2(30.0, 40.0), Point2(130.0, 80.0)));
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_EQ(document.history().undoName(), "EDIT_VIEWPORT");
    EXPECT_TRUE(shown.said("> VIEW SET vp1 rect=30,40,130,80"));
    EXPECT_EQ(shown.errors, 0);

    // Off the paper: refused as the typed line is, and nothing changes.
    shown.type("sheetViewportX", 5000.0);
    EXPECT_EQ(onlyViewport(document).rect, Box2(Point2(30.0, 40.0), Point2(130.0, 80.0)));
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_EQ(shown.errors, 1);
    // The panel shows the stored place again.
    EXPECT_EQ(shown.find<QDoubleSpinBox>("sheetViewportX")->value(), 30.0);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(onlyViewport(document).rect, Box2(Point2(30.0, 40.0), Point2(80.0, 80.0)));
}

TEST(SheetViewOptions, AnEditIsHandedToTheWindowsExecutorWhenThereIsOne)
{
    Document document;
    addSheetWith(document, {viewportAt("vp1", plotting::ViewportKind::Notes,
                                       Box2(Point2(30.0, 40.0), Point2(80.0, 80.0)))});
    Shown shown(document);
    std::vector<QString> lines;
    shown.editor.setCommandRunner([&lines](const QString& line) {
        lines.push_back(line);
        return katana::qt::VerbOutcome{true, QStringLiteral("view id=vp1"), {}};
    });
    shown.type("sheetViewportH", 60.0);
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_EQ(lines.front(), QStringLiteral("VIEW SET vp1 rect=30,40,80,100"));
    // The runner, not the editor, did the work: here it did none.
    EXPECT_EQ(onlyViewport(document).rect, Box2(Point2(30.0, 40.0), Point2(80.0, 80.0)));
    EXPECT_TRUE(shown.messages.empty()) << "the window logs the line itself";
}

TEST(SheetViewOptions, CrossSectionsAreCutEveryIntervalOverARangeOrAtChainages)
{
    Document document;
    road(document);
    plotting::Viewport sections =
        viewportAt("vp1", plotting::ViewportKind::CrossSections, Box2(Point2(30.0, 40.0), Point2(300.0, 280.0)));
    sections.source.alignment = "ROAD";
    sections.source.stations = {500.0};
    sections.source.sectionHalfWidth = 20.0;
    addSheetWith(document, {sections});
    Shown shown(document);
    auto* atChainages = shown.find<QRadioButton>("sheetSectionsAtChainages");
    auto* every = shown.find<QRadioButton>("sheetSectionsEvery");
    ASSERT_NE(atChainages, nullptr);
    ASSERT_NE(every, nullptr);
    EXPECT_TRUE(atChainages->isChecked());
    EXPECT_FALSE(shown.find<QDoubleSpinBox>("sheetSectionInterval")->isEnabled());
    // The chainage range is offered for cross sections now, as for a long section.
    ASSERT_NE(shown.find<QDoubleSpinBox>("sheetChainageFrom"), nullptr);
    ASSERT_NE(shown.find<QDoubleSpinBox>("sheetChainageTo"), nullptr);
    const std::size_t steps = document.history().undoCount();

    // Every 20 m: the list cleared, and with no range the whole road, 0 to
    // 1000 - 51 sections, one each 20 m from 0 (viewportStations).
    every->click();
    katana::qt::test::processEvents();
    const plotting::ViewportSource& cut = onlyViewport(document).source;
    EXPECT_TRUE(cut.stations.empty());
    EXPECT_EQ(cut.sectionInterval, 20.0);
    EXPECT_EQ(cut.chainageFrom, 0.0);
    EXPECT_EQ(cut.chainageTo, 1000.0);
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_TRUE(shown.said("> VIEW SET vp1 stations=\"\" interval=20 from=0 to=1000"));
    const std::vector<double> stations = plotting::viewportStations(cut);
    ASSERT_FALSE(stations.empty());
    EXPECT_EQ(stations.front(), 0.0);
    EXPECT_EQ(stations[1], 20.0);

    // The rebuilt panel shows the interval chosen; a new one and a range.
    EXPECT_TRUE(shown.find<QRadioButton>("sheetSectionsEvery")->isChecked());
    EXPECT_TRUE(shown.find<QDoubleSpinBox>("sheetSectionInterval")->isEnabled());
    shown.type("sheetSectionInterval", 50.0);
    shown.type("sheetChainageFrom", 100.0);
    shown.type("sheetChainageTo", 300.0);
    EXPECT_EQ(onlyViewport(document).source.sectionInterval, 50.0);
    EXPECT_EQ(plotting::viewportStations(onlyViewport(document).source),
              (std::vector<double>{100.0, 150.0, 200.0, 250.0, 300.0}));
    EXPECT_EQ(document.history().undoCount(), steps + 4);

    // Back to a list: the interval off, the chainages typed.
    shown.find<QRadioButton>("sheetSectionsAtChainages")->click();
    katana::qt::test::processEvents();
    auto* list = shown.find<QLineEdit>("sheetSectionStations");
    ASSERT_NE(list, nullptr);
    EXPECT_TRUE(list->isEnabled());
    list->setText(QStringLiteral("120, 480;760"));
    emit list->editingFinished();
    katana::qt::test::processEvents();
    EXPECT_EQ(onlyViewport(document).source.stations, (std::vector<double>{120.0, 480.0, 760.0}));
    EXPECT_EQ(onlyViewport(document).source.sectionInterval, 0.0);
    EXPECT_TRUE(shown.said("> VIEW SET vp1 interval=0 stations=120,480,760"));

    // Words are not chainages: said, and nothing run.
    const std::size_t before = document.history().undoCount();
    list = shown.find<QLineEdit>("sheetSectionStations");
    list->setText(QStringLiteral("120, soon"));
    emit list->editingFinished();
    katana::qt::test::processEvents();
    EXPECT_EQ(document.history().undoCount(), before);
    EXPECT_GE(shown.errors, 1);
}

TEST(SheetViewOptions, SectionViewsHaveTheirHiddenLayersAsPlansDo)
{
    Document document;
    road(document);
    addLayer(document, "TREES");
    addLayer(document, "FENCES");
    plotting::Viewport profile =
        viewportAt("vp1", plotting::ViewportKind::LongSection, Box2(Point2(30.0, 40.0), Point2(300.0, 150.0)));
    profile.source.alignment = "ROAD";
    plotting::Viewport sections =
        viewportAt("vp2", plotting::ViewportKind::CrossSections, Box2(Point2(30.0, 160.0), Point2(300.0, 280.0)));
    sections.source.alignment = "ROAD";
    addSheetWith(document, {profile, sections});
    Shown shown(document);
    auto* button = shown.find<QPushButton>("sheetViewportHiddenLayers");
    ASSERT_NE(button, nullptr);
    const std::size_t steps = document.history().undoCount();
    button->click();
    katana::qt::test::processEvents();
    auto* dialog = shown.editor.findChild<QDialog*>(QStringLiteral("sheetHiddenLayersDialog"));
    ASSERT_NE(dialog, nullptr);
    auto* list = dialog->findChild<QListWidget*>(QStringLiteral("sheetHiddenLayersList"));
    ASSERT_NE(list, nullptr);
    const auto items = list->findItems(QStringLiteral("TREES"), Qt::MatchExactly);
    ASSERT_EQ(items.size(), 1);
    items.front()->setCheckState(Qt::Unchecked);
    dialog->findChild<QPushButton*>(QStringLiteral("sheetHiddenLayersOk"))->click();
    katana::qt::test::processEvents();
    EXPECT_TRUE(onlyViewport(document).hiddenLayers.hidesDirectly("TREES"));
    EXPECT_FALSE(onlyViewport(document).hiddenLayers.hidesDirectly("FENCES"));
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_TRUE(shown.said("> VIEW SET vp1 hide=\"TREES\""));
    // The button counts them.
    EXPECT_EQ(shown.find<QPushButton>("sheetViewportHiddenLayers")->text(),
              QStringLiteral("Hidden layers (1)..."));

    // The cross sections have the button too; OK with nothing changed writes nothing.
    shown.editor.canvas()->select("vp2");
    katana::qt::test::processEvents();
    shown.find<QPushButton>("sheetViewportHiddenLayers")->click();
    katana::qt::test::processEvents();
    dialog = shown.editor.findChild<QDialog*>(QStringLiteral("sheetHiddenLayersDialog"));
    ASSERT_NE(dialog, nullptr);
    dialog->findChild<QPushButton*>(QStringLiteral("sheetHiddenLayersOk"))->click();
    katana::qt::test::processEvents();
    EXPECT_EQ(document.history().undoCount(), steps + 1);
}

TEST(SheetViewOptions, AnImageViewsPictureIsChosenAndCopiedInOneStep)
{
    const ScratchDirectory scratch("image");
    const fs::path image = scratch.path / "site photo.png";
    {
        // The PNG signature and a little after it: recognised by its content.
        std::ofstream out(image, std::ios::binary);
        out << std::string("\x89PNG\r\n\x1a\n", 8) << "not really pixels";
    }
    Document document;
    ASSERT_TRUE(document.saveAs(scratch.path / "project").ok());
    plotting::Viewport picture =
        viewportAt("vp1", plotting::ViewportKind::Image, Box2(Point2(30.0, 40.0), Point2(130.0, 120.0)));
    picture.text = "old.png";
    addSheetWith(document, {picture});
    Shown shown(document);
    ASSERT_NE(shown.find<QLineEdit>("sheetImageName"), nullptr);
    auto* browse = shown.find<QPushButton>("sheetImageBrowse");
    ASSERT_NE(browse, nullptr);
    const std::size_t steps = document.history().undoCount();
    {
        FileAnswer answer(QString::fromStdWString(image.wstring()));
        browse->click();
        katana::qt::test::processEvents();
        ASSERT_TRUE(answer.seen);
    }
    EXPECT_EQ(onlyViewport(document).text, "site_photo.png");
    EXPECT_TRUE(fs::exists(scratch.path / "project" / "assets" / "site_photo.png"));
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_EQ(shown.errors, 0);

    // A headless session opens no file dialog: it names the verb instead.
    shown.editor.setHeadless(true);
    shown.find<QPushButton>("sheetImageBrowse")->click();
    katana::qt::test::processEvents();
    EXPECT_EQ(shown.errors, 1);
    EXPECT_TRUE(shown.messages.back().contains(QStringLiteral("VIEW SET vp1 file=")));
    EXPECT_EQ(document.history().undoCount(), steps + 1);
}

TEST(SheetViewOptions, ChoosePaperShowsTheAdviceAtAScaleBeforeApplyingIt)
{
    // 500 x 300 m drawn by its four sides, in a plan at 1 : 1000 filling the
    // sheet: 520 x 312 mm with 4% to spare, which A3 cannot hold and A2
    // landscape can (the editor's Arrange test works it out).
    Document document;
    for (const auto& [a, b] : {std::pair{Point2(0.0, 0.0), Point2(500.0, 0.0)},
                               std::pair{Point2(500.0, 0.0), Point2(500.0, 300.0)},
                               std::pair{Point2(500.0, 300.0), Point2(0.0, 300.0)},
                               std::pair{Point2(0.0, 300.0), Point2(0.0, 0.0)}}) {
        ASSERT_TRUE(document.execute(cmd::createLine(a, b)).ok());
    }
    plotting::Viewport plan =
        viewportAt("vp1", plotting::ViewportKind::Plan, plotting::tilingArea(plotting::Sheet{}));
    plan.scale = 1000.0;
    addSheetWith(document, {plan});
    Shown shown(document, nullptr);
    auto* button = shown.find<QPushButton>("sheetChoosePaper");
    ASSERT_NE(button, nullptr);
    const std::size_t steps = document.history().undoCount();
    button->click();
    katana::qt::test::processEvents();
    auto* dialog = shown.editor.findChild<QDialog*>(QStringLiteral("sheetSuggestPaperDialog"));
    ASSERT_NE(dialog, nullptr);
    auto* scale = dialog->findChild<QComboBox*>(QStringLiteral("sheetSuggestPaperScale"));
    auto* advice = dialog->findChild<QLabel*>(QStringLiteral("sheetSuggestPaperAdvice"));
    ASSERT_NE(scale, nullptr);
    ASSERT_NE(advice, nullptr);
    EXPECT_EQ(scale->currentText(), QStringLiteral("As drawn"));
    EXPECT_TRUE(advice->text().startsWith(QStringLiteral("A2 landscape, ")));
    EXPECT_TRUE(advice->text().endsWith(QStringLiteral("full at 1:1000")));

    // At 1 : 2000 the verb's own advice, shown, and nothing changed yet.
    scale->setCurrentText(QStringLiteral("1:2000"));
    const auto reply = plotting::runSheetVerb(document, {"SHEET", "SUGGESTPAPER", "1", "scale=2000"});
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const std::size_t at = reply->find("paper=");
    ASSERT_NE(at, std::string::npos);
    const std::string paper = reply->substr(at + 6, reply->find(' ', at) - at - 6);
    EXPECT_TRUE(advice->text().startsWith(QString::fromStdString(paper + " "))) << advice->text().toStdString();
    EXPECT_EQ(document.history().undoCount(), steps);
    // A scale that cannot be read cannot be applied.
    auto* apply = dialog->findChild<QPushButton*>(QStringLiteral("sheetSuggestPaperApply"));
    ASSERT_NE(apply, nullptr);
    scale->setCurrentText(QStringLiteral("large"));
    EXPECT_FALSE(apply->isEnabled());

    scale->setCurrentText(QStringLiteral("As drawn"));
    apply->click();
    katana::qt::test::processEvents();
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_EQ(document.sheetSet().sheets[0].paper, katana::cad::PaperSize::A2);
    EXPECT_TRUE(document.sheetSet().sheets[0].landscape);
    EXPECT_TRUE(shown.said("> SHEET SUGGESTPAPER 1 scale=auto apply=on"));
}
