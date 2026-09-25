// File > Export View as Image, Edit > Copy View as Image and the SNAPSHOT verb
// (plotting/view_image_export.hpp): the grammar, the size an image is given,
// the files written, the dialog's line, and the plan view painted into an
// image (ViewportWidget::renderToImage) without touching the view.

#include <gtest/gtest.h>

#include <QComboBox>
#include <QFile>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSpinBox>
#include <QTemporaryDir>

#include <algorithm>
#include <vector>

#include "katana/cad/view_set.hpp"
#include "katana/commands/entity_commands.hpp"
#include "plotting/view_image_export.hpp"
#include "widget_harness.hpp"

namespace {

using katana::geometry::Point2;
using katana::qt::SnapshotRequest;
using katana::qt::VerbOutcome;
using katana::qt::ViewImageDialog;
using katana::qt::ViewImageDialogContext;
using Background = SnapshotRequest::Background;
using View = SnapshotRequest::View;

template <typename T> T* child(QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << "no " << name;
    return found;
}

TEST(Snapshot, AFileTakesTheDefaultsAndItsExtensionIsItsFormat)
{
    const auto request = katana::qt::parseSnapshot("SNAPSHOT \"C:/out/site view.png\"");
    ASSERT_TRUE(request.ok()) << request.error().describe();
    EXPECT_EQ(request->path, "C:/out/site view.png");
    EXPECT_FALSE(request->clipboard);
    EXPECT_EQ(request->width, 0);
    EXPECT_EQ(request->height, 0);
    EXPECT_EQ(request->scale, 1.0);
    EXPECT_EQ(request->background, Background::Theme);
    EXPECT_EQ(request->view, View::Plan);
    for (const char* line : {"SNAPSHOT a.JPG", "SNAPSHOT a.jpeg", "SNAPSHOT a.tif", "SNAPSHOT a.TIFF"}) {
        EXPECT_TRUE(katana::qt::parseSnapshot(line).ok()) << line;
    }
}

TEST(Snapshot, TheClipboardAndEveryOptionAreRead)
{
    const auto request =
        katana::qt::parseSnapshot("snapshot clipboard WIDTH=640 height=480 scale=2 bg=none view=PLAN");
    ASSERT_TRUE(request.ok()) << request.error().describe();
    EXPECT_TRUE(request->clipboard);
    EXPECT_TRUE(request->path.isEmpty());
    EXPECT_EQ(request->width, 640);
    EXPECT_EQ(request->height, 480);
    EXPECT_EQ(request->scale, 2.0);
    EXPECT_EQ(request->background, Background::None);
    EXPECT_EQ(request->view, View::Plan);
    const auto model = katana::qt::parseSnapshot("snapshot clipboard view=3D bg=theme");
    ASSERT_TRUE(model.ok()) << model.error().describe();
    EXPECT_EQ(model->view, View::Model3D);
}

// The 3D view is grabbed on its own ground, so a ground asked of it is
// refused, in either order, rather than dropped.
TEST(Snapshot, AGroundAskedOfThe3DViewIsRefusedRatherThanDropped)
{
    for (const char* line : {"SNAPSHOT a.png view=3d bg=none", "SNAPSHOT a.png bg=white view=3d",
                             "SNAPSHOT CLIPBOARD bg=none view=3d"}) {
        const auto refused = katana::qt::parseSnapshot(line);
        ASSERT_FALSE(refused.ok()) << line;
        EXPECT_NE(refused.error().describe().find("bg= is the plan view's"), std::string::npos)
            << refused.error().describe();
    }
}

TEST(Snapshot, WhatTheGrammarDoesNotTakeIsRefused)
{
    for (const char* line :
         {"SNAPSHOT", "SNAPSHOT a.bmp", "SNAPSHOT a", "SNAPSHOT a.png width=0",
          "SNAPSHOT a.png width=10001", "SNAPSHOT a.png height=tall", "SNAPSHOT a.png scale=0.1",
          "SNAPSHOT a.png scale=9", "SNAPSHOT a.png bg=red", "SNAPSHOT a.png view=side",
          "SNAPSHOT a.png size=2", "SNAPSHOT a.png big", "SNAPSHOT a.jpg bg=none",
          "SNAPSHOT a.tif bg=none"}) {
        EXPECT_FALSE(katana::qt::parseSnapshot(line).ok()) << line;
    }
    const auto jpeg = katana::qt::parseSnapshot("SNAPSHOT a.jpg bg=none");
    EXPECT_NE(jpeg.error().describe().find("bg=none needs a .png"), std::string::npos);
    // The clipboard keeps transparency, as a PNG does.
    EXPECT_TRUE(katana::qt::parseSnapshot("SNAPSHOT CLIPBOARD bg=none").ok());
}

TEST(Snapshot, TheLineItWritesIsTheLineItReads)
{
    SnapshotRequest request;
    request.path = "C:\\out\\view.png";
    request.width = 300;
    request.height = 150;
    request.background = Background::White;
    const QString line = katana::qt::snapshotCommandLine(request);
    EXPECT_EQ(line, "SNAPSHOT \"C:/out/view.png\" width=300 height=150 bg=white");
    const auto read = katana::qt::parseSnapshot(line);
    ASSERT_TRUE(read.ok());
    EXPECT_EQ(read->width, 300);
    EXPECT_EQ(read->background, Background::White);
    EXPECT_EQ(read->view, View::Plan);
    SnapshotRequest model;
    model.path = "view.png";
    model.view = View::Model3D;
    EXPECT_EQ(katana::qt::snapshotCommandLine(model), "SNAPSHOT \"view.png\" view=3d");
    EXPECT_EQ(katana::qt::parseSnapshot(katana::qt::snapshotCommandLine(model))->view,
              View::Model3D);
    SnapshotRequest clipboard;
    clipboard.clipboard = true;
    clipboard.scale = 2.5;
    EXPECT_EQ(katana::qt::snapshotCommandLine(clipboard), "SNAPSHOT CLIPBOARD scale=2.5");
}

TEST(Snapshot, TheSizeIsGivenOrTakenFromTheViewsProportions)
{
    const QSize view(800, 600);
    SnapshotRequest both;
    both.width = 300;
    both.height = 100;
    EXPECT_EQ(katana::qt::snapshotSize(both, view), QSize(300, 100));
    SnapshotRequest wide;
    wide.width = 400; // 400 x 400 * 600 / 800
    EXPECT_EQ(katana::qt::snapshotSize(wide, view), QSize(400, 300));
    SnapshotRequest tall;
    tall.height = 150; // 150 * 800 / 600 x 150
    EXPECT_EQ(katana::qt::snapshotSize(tall, view), QSize(200, 150));
    SnapshotRequest twice;
    twice.scale = 2.0;
    EXPECT_EQ(katana::qt::snapshotSize(twice, view), QSize(1600, 1200));
    // Never past the largest side, never below one pixel.
    SnapshotRequest huge;
    huge.scale = 8.0;
    EXPECT_EQ(katana::qt::snapshotSize(huge, QSize(4000, 10)), QSize(10000, 80));
    EXPECT_EQ(katana::qt::snapshotSize(SnapshotRequest{}, QSize(0, 0)), QSize(1, 1));
}

TEST(Snapshot, EachFormatIsWrittenAndReadsBack)
{
    QTemporaryDir folder;
    ASSERT_TRUE(folder.isValid());
    QImage image(40, 20, QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::blue);
    for (const char* name : {"view.png", "view.jpg"}) {
        const QString path = folder.filePath(name);
        ASSERT_TRUE(katana::qt::writeSnapshot(image, path).ok()) << name;
        const QImage read(path);
        EXPECT_EQ(read.size(), QSize(40, 20)) << name;
    }
    // A TIFF through the project's own writer (tiff_writer.hpp): little-endian.
    const QString tiff = folder.filePath("view.tif");
    ASSERT_TRUE(katana::qt::writeSnapshot(image, tiff).ok());
    QFile file(tiff);
    ASSERT_TRUE(file.open(QIODevice::ReadOnly));
    EXPECT_EQ(file.read(4), QByteArray("II*\0", 4));
    EXPECT_FALSE(katana::qt::writeSnapshot(image, folder.filePath("missing/folder/view.png")).ok());
}

TEST(ViewImageDialog, TheFieldsWriteTheLineAndExportRunsIt)
{
    std::vector<QString> ran;
    ViewImageDialogContext context;
    context.run = [&ran](const QString& line) {
        ran.push_back(line);
        return VerbOutcome{true, "file=\"out.png\" view=plan width=300 height=150", {}};
    };
    ViewImageDialog dialog(std::move(context));
    EXPECT_EQ(dialog.objectName(), "viewImageDialog");
    EXPECT_FALSE(child<QPushButton>(dialog, "viewImageExport")->isEnabled());
    child<QLineEdit>(dialog, "viewImagePath")->setText("out.png");
    EXPECT_EQ(child<QLineEdit>(dialog, "viewImageCommand")->text(), "SNAPSHOT \"out.png\"");
    child<QComboBox>(dialog, "viewImageSize")->setCurrentText("2x the view");
    EXPECT_EQ(child<QLineEdit>(dialog, "viewImageCommand")->text(), "SNAPSHOT \"out.png\" scale=2");
    EXPECT_FALSE(child<QSpinBox>(dialog, "viewImageWidth")->isEnabled());
    child<QComboBox>(dialog, "viewImageSize")->setCurrentText("Custom size");
    child<QSpinBox>(dialog, "viewImageWidth")->setValue(300);
    child<QSpinBox>(dialog, "viewImageHeight")->setValue(150);
    child<QComboBox>(dialog, "viewImageBackground")->setCurrentText("White");
    EXPECT_EQ(child<QLineEdit>(dialog, "viewImageCommand")->text(),
              "SNAPSHOT \"out.png\" width=300 height=150 bg=white");
    // The ground is the plan view's: for the 3D view it is greyed out and
    // left off the line, which the verb would refuse.
    child<QComboBox>(dialog, "viewImageView")->setCurrentText("3D view");
    EXPECT_FALSE(child<QComboBox>(dialog, "viewImageBackground")->isEnabled());
    const QString expected = "SNAPSHOT \"out.png\" width=300 height=150 view=3d";
    EXPECT_EQ(child<QLineEdit>(dialog, "viewImageCommand")->text(), expected);
    child<QPushButton>(dialog, "viewImageExport")->click();
    EXPECT_EQ(ran, std::vector<QString>{expected});
    EXPECT_EQ(child<QLabel>(dialog, "viewImageStatus")->text(),
              "Exported: file=\"out.png\" view=plan width=300 height=150");
}

TEST(ViewImageDialog, TheFormatSetsTheExtensionAndATransparentJpegIsRefused)
{
    ViewImageDialog dialog(ViewImageDialogContext{});
    child<QLineEdit>(dialog, "viewImagePath")->setText("C:/out/site.png");
    child<QComboBox>(dialog, "viewImageFormat")->setCurrentText("JPEG");
    EXPECT_EQ(child<QLineEdit>(dialog, "viewImagePath")->text(), "C:/out/site.jpg");
    child<QComboBox>(dialog, "viewImageBackground")->setCurrentText("Transparent");
    EXPECT_FALSE(child<QPushButton>(dialog, "viewImageExport")->isEnabled());
    EXPECT_TRUE(child<QLineEdit>(dialog, "viewImageCommand")->text().isEmpty());
    EXPECT_TRUE(child<QLineEdit>(dialog, "viewImageCommand")
                    ->placeholderText()
                    .contains("bg=none needs a .png"));
    child<QComboBox>(dialog, "viewImageFormat")->setCurrentText("PNG");
    EXPECT_TRUE(child<QPushButton>(dialog, "viewImageExport")->isEnabled());
}

// The file follows the drawing (suggestPath), in the format chosen, until
// one is typed; and a file that is there is written over only when the
// person says so.
TEST(ViewImageDialog, TheSuggestedFileFollowsTheDrawingUntilOneIsTypedAndAReplaceIsAsked)
{
    QTemporaryDir folder;
    ASSERT_TRUE(folder.isValid());
    const QString there = folder.filePath("there.png");
    QFile file(there);
    ASSERT_TRUE(file.open(QIODevice::WriteOnly));
    file.close();

    std::vector<QString> ran;
    std::vector<QString> asked;
    bool replace = false;
    ViewImageDialogContext context;
    context.run = [&ran](const QString& line) {
        ran.push_back(line);
        return VerbOutcome{true, "file=\"there.png\"", {}};
    };
    context.suggestedPath = "C:/a/a.png";
    context.confirmReplace = [&](const QString& path) {
        asked.push_back(path);
        return replace;
    };
    ViewImageDialog dialog(std::move(context));
    auto* path = child<QLineEdit>(dialog, "viewImagePath");
    EXPECT_EQ(path->text(), "C:/a/a.png");
    child<QComboBox>(dialog, "viewImageFormat")->setCurrentText("JPEG");
    dialog.suggestPath("C:/b/b.png");
    EXPECT_EQ(path->text(), "C:/b/b.jpg") << "another drawing's, in the format chosen";

    path->setText(there);
    child<QComboBox>(dialog, "viewImageFormat")->setCurrentText("PNG");
    dialog.suggestPath("C:/c/c.png");
    EXPECT_EQ(path->text(), there) << "a file typed is kept";

    dialog.exportImage();
    EXPECT_EQ(asked, std::vector<QString>{there});
    EXPECT_TRUE(ran.empty()) << "kept when the person says no";
    EXPECT_TRUE(child<QLabel>(dialog, "viewImageStatus")->text().startsWith("Not exported:"));
    replace = true;
    dialog.exportImage();
    EXPECT_EQ(ran.size(), 1u);
}

// The line (0, 0)-(100, 50) framed in a 400 x 300 view: its midpoint is the
// view's centre, and so the centre of any image of the view.
struct FramedLine {
    katana::cad::Document document;
    katana::cad::ViewSet views;
    katana::cad::ViewState* state = nullptr;
    std::unique_ptr<katana::qt::ViewportWidget> view;

    FramedLine()
    {
        EXPECT_TRUE(document
                        .execute(katana::commands::createLine(Point2(0.0, 0.0),
                                                              Point2(100.0, 50.0)))
                        .ok());
        state = &views.add(katana::cad::ViewKind::Plan);
        view = std::make_unique<katana::qt::ViewportWidget>(document, *state);
        view->resize(400, 300);
        view->setGridVisible(true);
        katana::qt::test::paint(*view);
    }
};

// Whether any pixel within `reach` of `at` is not `ground`.
bool inkNear(const QImage& image, QPoint at, int reach, QColor ground)
{
    for (int y = at.y() - reach; y <= at.y() + reach; ++y) {
        for (int x = at.x() - reach; x <= at.x() + reach; ++x) {
            if (image.pixelColor(x, y) != ground) {
                return true;
            }
        }
    }
    return false;
}

TEST(ViewImage, ThePlanViewIsPaintedAtTheSizeAskedWithoutItsGridOrTouchingTheView)
{
    FramedLine framed;
    ASSERT_TRUE(framed.state->planFramed);
    const std::size_t paints = framed.view->drawingPaintCount();
    const katana::cad::ViewTransform before = framed.state->plan;
    const QImage image = framed.view->renderToImage(QSize(800, 600), Qt::black);
    EXPECT_EQ(image.size(), QSize(800, 600));
    EXPECT_TRUE(inkNear(image, QPoint(400, 300), 3, Qt::black)) << "the line's midpoint";
    // The grid is on in the view and not in the picture: away from the line
    // every pixel is the ground.
    for (const QPoint corner : {QPoint(5, 5), QPoint(795, 5), QPoint(5, 595), QPoint(795, 595)}) {
        EXPECT_FALSE(inkNear(image, corner, 4, Qt::black)) << corner.x() << "," << corner.y();
    }
    EXPECT_EQ(framed.view->drawingPaintCount(), paints) << "the view repainted its own drawing";
    EXPECT_EQ(framed.state->plan.scale, before.scale);
    EXPECT_EQ(framed.state->plan.widthPixels, before.widthPixels);
}

TEST(ViewImage, OnWhiteTheDrawingIsPaintedAsAPlotWithItsWhitePensBlack)
{
    FramedLine framed;
    // Layer 0 draws white, which on white would vanish.
    const QImage screen = framed.view->renderToImage(QSize(400, 300), Qt::white);
    const QImage plotted = framed.view->renderToImage(QSize(400, 300), Qt::white, true);
    const auto darkest = [](const QImage& image) {
        int least = 255;
        for (int y = 290 / 2; y <= 310 / 2; ++y) {
            for (int x = 390 / 2; x <= 410 / 2; ++x) {
                const QColor pixel = image.pixelColor(x, y);
                least = std::min({least, pixel.red(), pixel.green(), pixel.blue()});
            }
        }
        return least;
    };
    EXPECT_GT(darkest(screen), 200) << "a white pen on white";
    EXPECT_LT(darkest(plotted), 128) << "printed black";
}

} // namespace
