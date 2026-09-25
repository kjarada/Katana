#pragma once

// File > Export View as Image, Edit > Copy View as Image and the SNAPSHOT
// verb: a picture of a view, as a file or on the clipboard (docs/desktop.md,
// "Plot to PDF and view images").
//
//   SNAPSHOT <file.png|.jpg|.jpeg|.tif|.tiff> [width=N] [height=N] [scale=F]
//            [bg=theme|white|none] [view=plan|3d]
//   SNAPSHOT CLIPBOARD [the same options]
//
// The size: width and height both given are the image's; one given takes the
// other from the view's proportions; neither is the view's size on screen
// times scale= (1 unless given, 0.25 to 8). A side is 1 to 10000 pixels - an
// A0 sheet's width at 300 dpi is 9933. The plan view is painted afresh at that
// size by the plan painter (ViewportWidget::renderToImage), without the grid,
// on the view's own ground (theme), on white - drawn as a plot draws it, white
// pens black and line weights in millimetres, since the screen's white pens
// would vanish - or on nothing (none: a transparent PNG, or the
// clipboard; a JPEG has no transparency and the TIFF written here is RGB, so
// bg=none with either is refused). A 3D view is grabbed as it is drawn on
// screen and scaled to the size; bg= is the plan view's.
//
// The verb is the window's - there is a view to picture only there - and a
// headless run's --screenshot grabs the whole window. What it did is logged as
// a record: file="path" (or clipboard=yes) view= width= height=.
//
// The dialog is non-modal and runs nothing itself: it writes the SNAPSHOT
// line its fields describe and Export hands it to the window's one executor.
// Object names:
//   viewImageDialog      the dialog (File > Export View as Image,
//                        fileExportViewImage)
//   viewImagePath        the file; its extension is the format
//   viewImageBrowse      choose it with a file dialog (not headless)
//   viewImageFormat      PNG / JPEG / TIFF: sets the path's extension
//   viewImageView        Plan view / 3D view
//   viewImageSize        1x .. 4x the view's size on screen, or Custom size
//   viewImageWidth       pixels, for a custom size
//   viewImageHeight      pixels, for a custom size
//   viewImageBackground  Theme / White / Transparent
//   viewImageCommand     the exact line Export will run (read-only)
//   viewImageExport      run it
//   viewImageStatus      what happened last
//   viewImageClose       close

#include <QDialog>
#include <QImage>
#include <QSize>
#include <QString>

#include <functional>

#include "command_runner.hpp"
#include "katana/core/error.hpp"

class QComboBox;
class QLabel;
class QLineEdit;
class QPushButton;
class QSpinBox;

namespace katana::qt {

struct SnapshotRequest {
    enum class Background { Theme, White, None };
    enum class View { Plan, Model3D };

    QString path; // empty for the clipboard
    bool clipboard = false;
    int width = 0;  // 0: from the view
    int height = 0; // 0: from the view
    double scale = 1.0;
    Background background = Background::Theme;
    View view = View::Plan;
};

// The largest side of a snapshot, in pixels: an A0 sheet's 841 mm at 300 dpi
// is 9933, and an image beyond that costs gigabytes for nothing a screen or a
// report shows.
inline constexpr int kMaximumSnapshotSide = 10000;

// A SNAPSHOT line read by the interpreter's word rules. InvalidArgument naming
// the word, with the usage, for anything the grammar above does not take.
[[nodiscard]] katana::core::Result<SnapshotRequest> parseSnapshot(const QString& line);
// The line parseSnapshot reads back: the path quoted with '/' separators (or
// CLIPBOARD), then only the options that differ from the defaults.
[[nodiscard]] QString snapshotCommandLine(const SnapshotRequest& request);
// The image's size in pixels for a view of `viewSize` (logical pixels), as the
// grammar says: never below 1 or above kMaximumSnapshotSide on a side.
[[nodiscard]] QSize snapshotSize(const SnapshotRequest& request, const QSize& viewSize);
// Writes `image` to `path` in the format its extension names. FileExportFailure
// when Qt cannot write it.
[[nodiscard]] katana::core::Status writeSnapshot(const QImage& image, const QString& path);

struct ViewImageDialogContext {
    CommandRunner run;
    std::function<bool()> headless;
    QString suggestedPath;
};

class ViewImageDialog final : public QDialog {
  public:
    explicit ViewImageDialog(ViewImageDialogContext context, QWidget* parent = nullptr);

    // The request the fields describe, or why there is none.
    [[nodiscard]] katana::core::Result<SnapshotRequest> request() const;
    // What Export does: the SNAPSHOT line handed to the runner and the
    // outcome said in viewImageStatus.
    void exportImage();

  private:
    void refresh();
    void browse();
    void followFormat();

    ViewImageDialogContext context_;
    QLineEdit* path_ = nullptr;
    QComboBox* format_ = nullptr;
    QComboBox* view_ = nullptr;
    QComboBox* size_ = nullptr;
    QSpinBox* width_ = nullptr;
    QSpinBox* height_ = nullptr;
    QComboBox* background_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* export_ = nullptr;
    QLabel* status_ = nullptr;
};

} // namespace katana::qt
