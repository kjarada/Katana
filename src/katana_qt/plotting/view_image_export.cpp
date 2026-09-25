#include "plotting/view_image_export.hpp"

#include <algorithm>
#include <cmath>
#include <optional>
#include <string>

#include <QComboBox>
#include <QDir>
#include <QFileDialog>
#include <QFileInfo>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QImageWriter>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSpinBox>
#include <QVBoxLayout>

#include "katana/cad/command_interpreter.hpp"
#include "plotting/tiff_writer.hpp"

namespace katana::qt {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using Background = SnapshotRequest::Background;
using View = SnapshotRequest::View;

constexpr const char* kUsage =
    "usage: SNAPSHOT <file.png|.jpg|.tif> | CLIPBOARD [width=N] [height=N] [scale=F] "
    "[bg=theme|white|none] [view=plan|3d]";

// A picture of a view is a small multiple of it or a large one at most: under
// a quarter it is a thumbnail nobody can read, over eight it passes the side
// limit on any screen.
constexpr double kMinimumScale = 0.25;
constexpr double kMaximumScale = 8.0;

// A TIFF carries the resolution it is to print at; a view picture has none,
// so it is marked at the screen's nominal 96 dpi, as Windows' logical inch.
constexpr double kScreenDpi = 96.0;

// The format a path's extension names, lower case; empty for none we write.
QString formatOf(const QString& path)
{
    const QString suffix = QFileInfo(path).suffix().toLower();
    if (suffix == "png") {
        return "png";
    }
    if (suffix == "jpg" || suffix == "jpeg") {
        return "jpeg";
    }
    if (suffix == "tif" || suffix == "tiff") {
        return "tiff";
    }
    return {};
}

katana::core::Error refused(const std::string& why, const std::string& word)
{
    return makeError(ErrorCode::InvalidArgument, why + "; " + kUsage, word);
}

std::optional<int> side(const QString& text)
{
    bool ok = false;
    const int value = text.toInt(&ok);
    if (!ok || value < 1 || value > kMaximumSnapshotSide) {
        return std::nullopt;
    }
    return value;
}

QString numberText(double value)
{
    return QString::number(value, 'g', 10);
}

} // namespace

Result<SnapshotRequest> parseSnapshot(const QString& line)
{
    const auto tokens = katana::cad::CommandInterpreter::tokenize(line.toStdString());
    if (!tokens) {
        return tokens.error();
    }
    if (tokens->size() < 2 || (*tokens)[1].empty()) {
        return makeError(ErrorCode::InvalidArgument, kUsage);
    }
    SnapshotRequest request;
    const QString target = QString::fromStdString((*tokens)[1]);
    if (target.compare("CLIPBOARD", Qt::CaseInsensitive) == 0) {
        request.clipboard = true;
    } else if (formatOf(target).isEmpty()) {
        return refused("the file's extension names its format: .png, .jpg, .jpeg, .tif or .tiff",
                       (*tokens)[1]);
    } else {
        request.path = target;
    }
    for (std::size_t at = 2; at < tokens->size(); ++at) {
        const std::string& raw = (*tokens)[at];
        const QString word = QString::fromStdString(raw);
        const qsizetype equals = word.indexOf('=');
        if (equals <= 0) {
            return refused("not a SNAPSHOT option", raw);
        }
        const QString key = word.left(equals).toLower();
        const QString value = word.mid(equals + 1).toLower();
        if (key == "width" || key == "height") {
            const auto pixels = side(value);
            if (!pixels) {
                return refused(key.toStdString() + "= is a whole number of pixels from 1 to 10000",
                               raw);
            }
            (key == "width" ? request.width : request.height) = *pixels;
        } else if (key == "scale") {
            bool ok = false;
            const double scale = value.toDouble(&ok);
            if (!ok || !std::isfinite(scale) || scale < kMinimumScale || scale > kMaximumScale) {
                return refused("scale= is the view's size times 0.25 to 8", raw);
            }
            request.scale = scale;
        } else if (key == "bg") {
            if (value == "theme") {
                request.background = Background::Theme;
            } else if (value == "white") {
                request.background = Background::White;
            } else if (value == "none" || value == "transparent") {
                request.background = Background::None;
            } else {
                return refused("bg= is theme, white or none", raw);
            }
        } else if (key == "view") {
            if (value == "plan") {
                request.view = View::Plan;
            } else if (value == "3d") {
                request.view = View::Model3D;
            } else {
                return refused("view= is plan or 3d", raw);
            }
        } else {
            return refused("not a SNAPSHOT option", raw);
        }
    }
    // Only a PNG, or the clipboard, keeps transparency: a JPEG has none, and
    // the TIFF written here (tiff_writer.hpp) is RGB.
    if (request.background == Background::None && !request.clipboard &&
        formatOf(request.path) != "png") {
        return refused("bg=none needs a .png: a JPEG or TIFF has no transparency",
                       request.path.toStdString());
    }
    return request;
}

QString snapshotCommandLine(const SnapshotRequest& request)
{
    QString line = request.clipboard
                       ? QString("SNAPSHOT CLIPBOARD")
                       : QString("SNAPSHOT \"%1\"").arg(QDir::fromNativeSeparators(request.path));
    if (request.width > 0) {
        line += QString(" width=%1").arg(request.width);
    }
    if (request.height > 0) {
        line += QString(" height=%1").arg(request.height);
    }
    if (request.scale != 1.0) {
        line += " scale=" + numberText(request.scale);
    }
    if (request.background == Background::White) {
        line += " bg=white";
    } else if (request.background == Background::None) {
        line += " bg=none";
    }
    if (request.view == View::Model3D) {
        line += " view=3d";
    }
    return line;
}

QSize snapshotSize(const SnapshotRequest& request, const QSize& viewSize)
{
    const double w = std::max(1, viewSize.width());
    const double h = std::max(1, viewSize.height());
    double width = 0.0;
    double height = 0.0;
    if (request.width > 0 && request.height > 0) {
        width = request.width;
        height = request.height;
    } else if (request.width > 0) {
        width = request.width;
        height = request.width * h / w;
    } else if (request.height > 0) {
        height = request.height;
        width = request.height * w / h;
    } else {
        width = w * request.scale;
        height = h * request.scale;
    }
    const auto clamp = [](double pixels) {
        return std::clamp(static_cast<int>(std::lround(pixels)), 1, kMaximumSnapshotSide);
    };
    return {clamp(width), clamp(height)};
}

katana::core::Status writeSnapshot(const QImage& image, const QString& path)
{
    const QString format = formatOf(path);
    if (format == "tiff") {
        return writeTiff(image, path, kScreenDpi);
    }
    QImageWriter writer(path, format.toLatin1());
    // A JPEG has no alpha: laid on the image's own ground, which the
    // snapshot filled opaque, it loses nothing.
    const QImage written = format == "jpeg" ? image.convertToFormat(QImage::Format_RGB32) : image;
    if (!writer.write(written)) {
        return makeError(ErrorCode::FileExportFailure, "the image could not be written",
                         path.toStdString() + ": " + writer.errorString().toStdString());
    }
    return {};
}

// ---- the dialog -----------------------------------------------------------------------------

ViewImageDialog::ViewImageDialog(ViewImageDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("viewImageDialog");
    setWindowTitle("Export View as Image");
    setModal(false);

    path_ = new QLineEdit(context_.suggestedPath, this);
    path_->setObjectName("viewImagePath");
    path_->setPlaceholderText("the image to write: .png, .jpg or .tif");
    auto* browseButton = new QPushButton("Browse...", this);
    browseButton->setObjectName("viewImageBrowse");
    auto* pathRow = new QHBoxLayout;
    pathRow->addWidget(path_, 1);
    pathRow->addWidget(browseButton);

    format_ = new QComboBox(this);
    format_->setObjectName("viewImageFormat");
    format_->addItems({"PNG", "JPEG", "TIFF"});
    view_ = new QComboBox(this);
    view_->setObjectName("viewImageView");
    view_->addItems({"Plan view", "3D view"});
    size_ = new QComboBox(this);
    size_->setObjectName("viewImageSize");
    size_->addItems({"1x the view", "2x the view", "3x the view", "4x the view", "Custom size"});
    width_ = new QSpinBox(this);
    width_->setObjectName("viewImageWidth");
    width_->setRange(1, kMaximumSnapshotSide);
    width_->setValue(1920);
    width_->setSuffix(" px");
    height_ = new QSpinBox(this);
    height_->setObjectName("viewImageHeight");
    height_->setRange(1, kMaximumSnapshotSide);
    height_->setValue(1080);
    height_->setSuffix(" px");
    auto* customRow = new QHBoxLayout;
    customRow->addWidget(width_);
    customRow->addWidget(new QLabel("by", this));
    customRow->addWidget(height_);
    customRow->addStretch(1);
    background_ = new QComboBox(this);
    background_->setObjectName("viewImageBackground");
    background_->addItems({"Theme", "White", "Transparent"});
    background_->setToolTip("The plan view's ground: as on screen, white, or none (a PNG only)");

    command_ = new QLineEdit(this);
    command_->setObjectName("viewImageCommand");
    command_->setReadOnly(true);
    command_->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    command_->setToolTip("The line Export hands to the command line - type it there and it does "
                         "the same");

    auto* form = new QFormLayout;
    form->addRow("Image:", pathRow);
    form->addRow("Format:", format_);
    form->addRow("View:", view_);
    form->addRow("Size:", size_);
    form->addRow("Custom size:", customRow);
    form->addRow("Background:", background_);
    form->addRow("Command:", command_);

    export_ = new QPushButton("Export", this);
    export_->setObjectName("viewImageExport");
    export_->setDefault(true);
    status_ = new QLabel(this);
    status_->setObjectName("viewImageStatus");
    status_->setWordWrap(true);
    status_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    auto* close = new QPushButton("Close", this);
    close->setObjectName("viewImageClose");
    auto* buttons = new QHBoxLayout;
    buttons->addWidget(status_, 1);
    buttons->addWidget(export_);
    buttons->addWidget(close);

    auto* layout = new QVBoxLayout(this);
    layout->addLayout(form);
    layout->addLayout(buttons);

    connect(path_, &QLineEdit::textChanged, this, [this] { refresh(); });
    connect(format_, &QComboBox::currentIndexChanged, this, [this] { followFormat(); });
    for (QComboBox* box : {view_, size_, background_}) {
        connect(box, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    }
    for (QSpinBox* spin : {width_, height_}) {
        connect(spin, &QSpinBox::valueChanged, this, [this] { refresh(); });
    }
    connect(browseButton, &QPushButton::clicked, this, [this] { browse(); });
    connect(export_, &QPushButton::clicked, this, [this] { exportImage(); });
    connect(close, &QPushButton::clicked, this, [this] { hide(); });
    refresh();
}

Result<SnapshotRequest> ViewImageDialog::request() const
{
    const QString path = path_->text().trimmed();
    if (path.isEmpty()) {
        return makeError(ErrorCode::InvalidArgument, "no image file is named");
    }
    SnapshotRequest request;
    request.path = path;
    request.view = view_->currentIndex() == 1 ? View::Model3D : View::Plan;
    if (size_->currentIndex() == size_->count() - 1) {
        request.width = width_->value();
        request.height = height_->value();
    } else {
        request.scale = size_->currentIndex() + 1;
    }
    request.background = static_cast<Background>(background_->currentIndex());
    // The line's own reader is the judge, so the dialog refuses what the verb
    // would: an extension it does not write, a transparent JPEG.
    const auto checked = parseSnapshot(snapshotCommandLine(request));
    if (!checked) {
        return checked.error();
    }
    return request;
}

void ViewImageDialog::refresh()
{
    const bool custom = size_->currentIndex() == size_->count() - 1;
    width_->setEnabled(custom);
    height_->setEnabled(custom);
    const auto made = request();
    command_->setText(made ? snapshotCommandLine(*made) : QString());
    command_->setPlaceholderText(made ? QString()
                                      : "nothing to run yet: " +
                                            QString::fromStdString(made.error().message));
    export_->setEnabled(made.ok());
}

void ViewImageDialog::followFormat()
{
    static const char* const kExtensions[] = {"png", "jpg", "tif"};
    const QString path = path_->text().trimmed();
    if (!path.isEmpty()) {
        const QFileInfo file(path);
        const QString stem = file.suffix().isEmpty() ? path : path.chopped(file.suffix().size() + 1);
        path_->setText(stem + "." + kExtensions[format_->currentIndex()]);
    }
    refresh();
}

void ViewImageDialog::exportImage()
{
    const auto made = request();
    if (!made) {
        status_->setText(QString::fromStdString(made.error().message));
        return;
    }
    if (!context_.run) {
        status_->setText("nothing here can run a command");
        return;
    }
    const VerbOutcome outcome = context_.run(snapshotCommandLine(*made));
    status_->setText(outcome.ok ? "Exported: " + outcome.reply.section('\n', -1)
                                : "Not exported: " + outcome.error.section('\n', -1));
}

void ViewImageDialog::browse()
{
    if (context_.headless && context_.headless()) {
        status_->setText("A headless session opens no file dialog: fill viewImagePath with the "
                         "path instead.");
        return;
    }
    const QString path =
        QFileDialog::getSaveFileName(this, "Export View as Image", path_->text().trimmed(),
                                     "PNG (*.png);;JPEG (*.jpg *.jpeg);;TIFF (*.tif *.tiff)");
    if (!path.isEmpty()) {
        path_->setText(QDir::toNativeSeparators(path));
    }
}

} // namespace katana::qt
