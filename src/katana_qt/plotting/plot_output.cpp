#include "plot_output.hpp"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <format>
#include <optional>

#include <QDir>
#include <QFileInfo>
#include <QImage>
#include <QImageWriter>
#include <QPageLayout>
#include <QPageSize>
#include <QPainter>
#include <QPdfWriter>
#include <QPrinter>
#include <QPrinterInfo>
#include <QSaveFile>

#include "plan_painter.hpp"
#include "plot_style.hpp"
#include "tiff_writer.hpp"

namespace katana::qt {

namespace plotting = katana::cad::plotting;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

QString sheetsText(std::size_t count)
{
    return QString("%1 sheet%2").arg(count).arg(count == 1 ? "" : "s");
}

// The paint options a request's plot style asks for, at `pixelsPerMillimetre`.
SheetPaintOptions paintOptions(const PlotRequest& request, double pixelsPerMillimetre, double dpi)
{
    SheetPaintOptions options;
    options.medium = PlanMedium::Paper;
    options.pixelsPerMillimetre = pixelsPerMillimetre;
    options.plot.dpi = dpi;
    options.plot.colourMode = request.colourMode;
    options.plot.lineWeightScale = request.lineWeightScale;
    return options;
}

// A PDF page exactly the sheet's paper, landscape or not, in millimetres.
QPageSize pdfPageSize(const plotting::Sheet& sheet)
{
    const katana::cad::PaperDimensions paper =
        katana::cad::paperDimensions(sheet.paper, sheet.landscape);
    return QPageSize(QSizeF(paper.widthMm, paper.heightMm), QPageSize::Millimeter);
}

// The ISO size a printer driver knows by name, so it picks the paper tray.
QPageSize printerPageSize(katana::cad::PaperSize paper)
{
    switch (paper) {
    case katana::cad::PaperSize::A0:
        return QPageSize(QPageSize::A0);
    case katana::cad::PaperSize::A1:
        return QPageSize(QPageSize::A1);
    case katana::cad::PaperSize::A2:
        return QPageSize(QPageSize::A2);
    case katana::cad::PaperSize::A3:
        return QPageSize(QPageSize::A3);
    case katana::cad::PaperSize::A4:
        return QPageSize(QPageSize::A4);
    }
    return QPageSize(QPageSize::A3);
}

void collectProblems(PlotReport& report, const plotting::SheetSet& set, std::size_t index,
                     const SheetPaintStats& stats)
{
    for (const std::string& line : stats.problems) {
        report.problems.push_back(set.sheets[index].name + " - " + line);
    }
}

// Paints sheet `index` onto an active painter whose device has `dpi`, at the
// device's origin, then scaled by `scale` (a PDF's resolution rounding, a
// printer's fit).
SheetPaintStats paintPage(QPainter& painter, const plotting::SheetSet& set, std::size_t index,
                          const SheetSource& source, const SheetPaintOptions& options,
                          SheetPaintCache& cache, QPointF offset, double scale)
{
    painter.save();
    painter.translate(offset);
    if (scale != 1.0) {
        painter.scale(scale, scale);
    }
    const SheetPaintStats stats = paintSheet(painter, set, index, source, options, cache);
    painter.restore();
    return stats;
}

// The pixel size of sheet `sheet` at `dpi`.
QSize rasterSize(const plotting::Sheet& sheet, double dpi)
{
    const katana::cad::PaperDimensions paper =
        katana::cad::paperDimensions(sheet.paper, sheet.landscape);
    return QSize(static_cast<int>(std::lround(paper.widthMm * dpi / 25.4)),
                 static_cast<int>(std::lround(paper.heightMm * dpi / 25.4)));
}

// The request's style and resolution, checked as a page setup is; the
// file-name pattern only when there are files to name.
Status validateStyle(const PlotRequest& request)
{
    plotting::PageSetup setup;
    setup.colourMode = request.colourMode;
    setup.lineWeightScale = request.lineWeightScale;
    setup.dpi = request.dpi;
    if (plotsFilePerSheet(request.format)) {
        setup.fileNamePattern = request.fileNamePattern;
    }
    return plotting::validatePageSetup(setup);
}

// Asks `progress` whether to go on; remembers a no.
class Progress {
  public:
    Progress(const PlotProgress& progress, std::size_t total, PlotReport& report)
        : progress_(progress), total_(total), report_(report)
    {
    }
    [[nodiscard]] bool next(const std::string& name)
    {
        if (progress_ && !progress_(done_, total_, name)) {
            report_.cancelled = true;
            return false;
        }
        return true;
    }
    void done() { ++done_; }
    void finish() const
    {
        if (progress_ && !report_.cancelled) {
            (void)progress_(done_, total_, std::string{});
        }
    }

  private:
    const PlotProgress& progress_;
    std::size_t total_;
    std::size_t done_ = 0;
    PlotReport& report_;
};

// Sheets `pages` of `set` into one PDF at `path`. The file is written
// through a QSaveFile and takes its name only when its last page is done: a
// plot cancelled or failed part of the way leaves no half a set behind, and
// an earlier file of that name as it was.
Status writePdf(const QString& path, const QString& title, const plotting::SheetSet& set,
                const std::vector<std::size_t>& pages, const PlotRequest& request,
                const SheetSourceProvider& source, SheetPaintCache& cache, Progress& progress,
                PlotReport& report)
{
    const PdfResolution resolution = pdfResolutionFor(request.dpi);
    const SheetPaintOptions options =
        paintOptions(request, katana::cad::millimetresToPixels(1.0, request.dpi), request.dpi);
    QSaveFile file(path);
    std::optional<QPdfWriter> writer;
    std::optional<QPainter> painter;
    std::vector<std::size_t> plotted;
    for (const std::size_t index : pages) {
        const plotting::Sheet& sheet = set.sheets[index];
        if (!progress.next(sheet.name)) {
            break;
        }
        if (!writer) {
            if (!file.open(QIODevice::WriteOnly)) {
                return makeError(ErrorCode::FileExportFailure,
                                 "could not open the PDF for writing: " +
                                     file.errorString().toStdString(),
                                 path.toStdString());
            }
            writer.emplace(&file);
            writer->setResolution(resolution.resolution);
            writer->setTitle(title.isEmpty() ? QFileInfo(path).completeBaseName() : title);
            writer->setCreator(QStringLiteral("Katana"));
            writer->setPageSize(pdfPageSize(sheet));
            writer->setPageMargins(QMarginsF(0.0, 0.0, 0.0, 0.0));
            painter.emplace();
            if (!painter->begin(&*writer)) {
                file.cancelWriting();
                return makeError(ErrorCode::FileExportFailure, "could not start the PDF",
                                 path.toStdString());
            }
        } else {
            writer->setPageSize(pdfPageSize(sheet));
            if (!writer->newPage()) {
                file.cancelWriting();
                return makeError(ErrorCode::FileExportFailure, "could not start a page",
                                 path.toStdString());
            }
        }
        const SheetPaintStats stats = paintPage(*painter, set, index, source(), options, cache,
                                                QPointF(0.0, 0.0), resolution.scale);
        collectProblems(report, set, index, stats);
        plotted.push_back(index);
        progress.done();
    }
    report.sheets.insert(report.sheets.end(), plotted.begin(), plotted.end());
    if (!painter) {
        return {}; // cancelled before the first page: nothing was opened
    }
    const bool ended = painter->end();
    painter.reset();
    writer.reset();
    if (report.cancelled) {
        file.cancelWriting(); // half a set is not a set
        return {};
    }
    if (!ended || !file.commit()) {
        return makeError(ErrorCode::FileExportFailure,
                         "could not finish the PDF: " + file.errorString().toStdString(),
                         path.toStdString());
    }
    report.files.push_back(path);
    return {};
}

// Sheet `index` as a raster at the request's resolution, written as `path`:
// RGB in colour, one grey channel otherwise (greyscaleRaster), with the
// resolution recorded so it prints at its scale.
Status writeRaster(const QString& path, const plotting::SheetSet& set, std::size_t index,
                   const PlotRequest& request, const SheetSource& source, SheetPaintCache& cache,
                   PlotReport& report)
{
    const plotting::Sheet& sheet = set.sheets[index];
    const QSize size = rasterSize(sheet, request.dpi);
    QImage image(size, QImage::Format_RGB32);
    if (image.isNull()) {
        return makeError(ErrorCode::FileExportFailure,
                         std::format("not enough memory for a {} x {} raster", size.width(),
                                     size.height()),
                         path.toStdString());
    }
    image.fill(Qt::white);
    {
        QPainter painter(&image);
        const SheetPaintOptions options = paintOptions(request, request.dpi / 25.4, request.dpi);
        const SheetPaintStats stats = paintPage(painter, set, index, source, options, cache,
                                                QPointF(0.0, 0.0), 1.0);
        collectProblems(report, set, index, stats);
    }
    // Every colour is a grey already; a grey image is a third of the size.
    if (request.colourMode != katana::cad::PlotColourMode::Colour) {
        image = greyscaleRaster(image);
    }
    const int dotsPerMetre = static_cast<int>(std::lround(request.dpi / 0.0254));
    image.setDotsPerMeterX(dotsPerMetre);
    image.setDotsPerMeterY(dotsPerMetre);
    if (request.format == PlotFormat::Tiff) {
        return writeTiff(image, path, request.dpi);
    }
    // Through a QSaveFile, as the PDF is: a failed write leaves no broken file.
    QSaveFile file(path);
    QImageWriter writer(&file, "png");
    if (!file.open(QIODevice::WriteOnly) || !writer.write(image) || !file.commit()) {
        const QString why = writer.error() != QImageWriter::UnknownError ? writer.errorString()
                                                                         : file.errorString();
        return makeError(ErrorCode::FileExportFailure,
                         "could not write the PNG file: " + why.toStdString(), path.toStdString());
    }
    return {};
}

} // namespace

std::string_view toString(PlotFormat format)
{
    switch (format) {
    case PlotFormat::Pdf:
        return "pdf";
    case PlotFormat::PdfPerSheet:
        return "pdfs";
    case PlotFormat::Png:
        return "png";
    case PlotFormat::Tiff:
        return "tiff";
    }
    return "pdf";
}

std::optional<PlotFormat> plotFormatFrom(std::string_view name)
{
    std::string lower;
    for (const char c : name) {
        lower.push_back(static_cast<char>(std::tolower(static_cast<unsigned char>(c))));
    }
    if (lower == "pdf") {
        return PlotFormat::Pdf;
    }
    if (lower == "pdfs" || lower == "pdf-per-sheet") {
        return PlotFormat::PdfPerSheet;
    }
    if (lower == "png") {
        return PlotFormat::Png;
    }
    if (lower == "tiff" || lower == "tif") {
        return PlotFormat::Tiff;
    }
    return std::nullopt;
}

QString plotFormatExtension(PlotFormat format)
{
    switch (format) {
    case PlotFormat::Pdf:
    case PlotFormat::PdfPerSheet:
        return QStringLiteral(".pdf");
    case PlotFormat::Png:
        return QStringLiteral(".png");
    case PlotFormat::Tiff:
        return QStringLiteral(".tif");
    }
    return QStringLiteral(".pdf");
}

bool plotsFilePerSheet(PlotFormat format)
{
    return format != PlotFormat::Pdf;
}

PlotRequest plotRequestFor(const plotting::PageSetup& setup, QString destination,
                           std::string sheets)
{
    PlotRequest request;
    request.sheets = std::move(sheets);
    request.format = setup.filePerSheet ? PlotFormat::PdfPerSheet : PlotFormat::Pdf;
    request.colourMode = setup.colourMode;
    request.lineWeightScale = setup.lineWeightScale;
    request.dpi = setup.dpi;
    request.destination = std::move(destination);
    request.fileNamePattern = setup.fileNamePattern;
    return request;
}

plotting::PageSetup pageSetupFor(const PlotRequest& request, const plotting::PageSetup& base)
{
    plotting::PageSetup setup = base;
    setup.colourMode = request.colourMode;
    setup.lineWeightScale = request.lineWeightScale;
    setup.dpi = request.dpi;
    setup.fileNamePattern = request.fileNamePattern;
    if (request.format == PlotFormat::Pdf) {
        setup.filePerSheet = false;
    } else if (request.format == PlotFormat::PdfPerSheet) {
        setup.filePerSheet = true;
    }
    return setup;
}

Status validatePlotRequest(const plotting::SheetSet& set, const PlotRequest& request)
{
    const auto selection = plotting::parseSheetSelection(request.sheets, set);
    if (!selection) {
        return selection.error();
    }
    if (Status status = validateStyle(request); !status) {
        return status;
    }
    if (request.destination.trimmed().isEmpty()) {
        return makeError(ErrorCode::InvalidArgument,
                         plotsFilePerSheet(request.format)
                             ? "choose the folder the plot's files go in"
                             : "choose the PDF file the plot goes in");
    }
    if (request.format == PlotFormat::Png || request.format == PlotFormat::Tiff) {
        for (const std::size_t index : *selection) {
            const plotting::Sheet& sheet = set.sheets[index];
            const QSize size = rasterSize(sheet, request.dpi);
            const double pixels = static_cast<double>(size.width()) * size.height();
            if (pixels > kMaximumRasterPixels) {
                const katana::cad::PaperDimensions paper =
                    katana::cad::paperDimensions(sheet.paper, sheet.landscape);
                const double fits =
                    std::floor(std::sqrt(kMaximumRasterPixels / (paper.widthMm * paper.heightMm)) *
                               25.4);
                return makeError(ErrorCode::InvalidArgument,
                                 std::format("sheet {} ({}) at {:g} dpi is {:.0f} megapixels, more "
                                             "than the {:.0f} a raster may be: plot it at {:g} dpi "
                                             "or less",
                                             index + 1, sheet.name, request.dpi, pixels / 1.0e6,
                                             kMaximumRasterPixels / 1.0e6, fits));
            }
        }
    }
    return {};
}

Result<std::vector<QString>> plannedFiles(const plotting::SheetSet& set, const PlotRequest& request)
{
    if (Status status = validatePlotRequest(set, request); !status) {
        return status.error();
    }
    if (!plotsFilePerSheet(request.format)) {
        return std::vector<QString>{request.destination};
    }
    const auto selection = plotting::parseSheetSelection(request.sheets, set);
    const QDir folder(request.destination);
    const QString extension = plotFormatExtension(request.format);
    std::vector<QString> files;
    for (const std::string& name :
         plotting::sheetFileNames(set, *selection, request.fileNamePattern)) {
        files.push_back(folder.filePath(QString::fromStdString(name) + extension));
    }
    return files;
}

QString PlotReport::summary(const PlotRequest& request) const
{
    if (cancelled) {
        QString text = QString("Cancelled after %1 of %2.").arg(sheets.size()).arg(sheetsText(sheetsAsked));
        if (!files.empty() && plotsFilePerSheet(request.format)) {
            text.chop(1);
            text += QString("; %1 file%2 kept in %3.")
                        .arg(files.size())
                        .arg(files.size() == 1 ? "" : "s")
                        .arg(QDir::toNativeSeparators(request.destination));
        }
        return text;
    }
    if (!printer.isEmpty()) {
        return QString("Printed %1 on %2.").arg(sheetsText(sheets.size()), printer);
    }
    if (!plotsFilePerSheet(request.format)) {
        return QString("Plotted %1 to %2.")
            .arg(sheetsText(sheets.size()), QDir::toNativeSeparators(request.destination));
    }
    const QString kind = request.format == PlotFormat::PdfPerSheet ? QStringLiteral("PDF")
                         : request.format == PlotFormat::Png      ? QStringLiteral("PNG")
                                                                  : QStringLiteral("TIFF");
    return QString("Plotted %1 to %2 %3 file%4 in %5.")
        .arg(sheetsText(sheets.size()))
        .arg(files.size())
        .arg(kind)
        .arg(files.size() == 1 ? "" : "s")
        .arg(QDir::toNativeSeparators(request.destination));
}

Result<PlotReport> plotSheets(const plotting::SheetSet& set, const PlotRequest& request,
                              const SheetSourceProvider& source, SheetPaintCache& cache,
                              const PlotProgress& progress)
{
    const auto files = plannedFiles(set, request);
    if (!files) {
        return files.error();
    }
    const std::vector<std::size_t> selection = *plotting::parseSheetSelection(request.sheets, set);
    PlotReport report;
    report.sheetsAsked = selection.size();
    Progress steps(progress, selection.size(), report);

    if (request.format == PlotFormat::Pdf) {
        const QFileInfo target(request.destination);
        if (!QDir().mkpath(target.absolutePath())) {
            return makeError(ErrorCode::FileExportFailure, "could not make the folder",
                             target.absolutePath().toStdString());
        }
        if (Status status = writePdf(request.destination, request.title, set, selection, request,
                                     source, cache, steps, report);
            !status) {
            return status.error();
        }
        steps.finish();
        return report;
    }

    if (!QDir().mkpath(request.destination)) {
        return makeError(ErrorCode::FileExportFailure, "could not make the folder",
                         request.destination.toStdString());
    }
    for (std::size_t n = 0; n < selection.size(); ++n) {
        const std::size_t index = selection[n];
        const QString& path = (*files)[n];
        if (request.format == PlotFormat::PdfPerSheet) {
            // One page: writePdf asks about the sheet itself.
            const QString title = request.title.isEmpty()
                                      ? QString()
                                      : request.title + QStringLiteral(" - ") +
                                            QString::fromStdString(set.sheets[index].name);
            if (Status status = writePdf(path, title, set, {index}, request, source, cache,
                                         steps, report);
                !status) {
                return status.error();
            }
            if (report.cancelled) {
                break;
            }
            continue;
        }
        if (!steps.next(set.sheets[index].name)) {
            break;
        }
        if (Status status = writeRaster(path, set, index, request, source(), cache, report);
            !status) {
            return status.error();
        }
        report.sheets.push_back(index);
        report.files.push_back(path);
        steps.done();
    }
    steps.finish();
    return report;
}

Result<PlotReport> plotSheets(const plotting::SheetSet& set, const PlotRequest& request,
                              const SheetSource& source, SheetPaintCache& cache,
                              const PlotProgress& progress)
{
    return plotSheets(
        set, request, [&source] { return source; }, cache, progress);
}

Result<PlotReport> printSheets(QPrinter& printer, const plotting::SheetSet& set,
                               const PlotRequest& request, const SheetSourceProvider& source,
                               SheetPaintCache& cache, const PlotProgress& progress)
{
    const auto selection = plotting::parseSheetSelection(request.sheets, set);
    if (!selection) {
        return selection.error();
    }
    if (Status status = validateStyle(request); !status) {
        return status.error();
    }
    PlotReport report;
    report.sheetsAsked = selection->size();
    report.printer = printer.outputFormat() == QPrinter::PdfFormat
                         ? QDir::toNativeSeparators(printer.outputFileName())
                         : printer.printerName();
    if (report.printer.isEmpty()) {
        report.printer = QStringLiteral("the printer");
    }
    Progress steps(progress, selection->size(), report);
    printer.setFullPage(true);
    printer.setDocName(request.title.isEmpty() ? QStringLiteral("Katana sheets") : request.title);
    printer.setCreator(QStringLiteral("Katana"));
    QPainter painter;
    for (const std::size_t index : *selection) {
        const plotting::Sheet& sheet = set.sheets[index];
        if (!steps.next(sheet.name)) {
            break;
        }
        const QPageLayout layout(printerPageSize(sheet.paper),
                                 sheet.landscape ? QPageLayout::Landscape : QPageLayout::Portrait,
                                 QMarginsF(0.0, 0.0, 0.0, 0.0));
        (void)printer.setPageLayout(layout);
        if (!painter.isActive()) {
            if (!painter.begin(&printer)) {
                return makeError(ErrorCode::FileExportFailure, "could not start the printer",
                                 report.printer.toStdString());
            }
        } else if (!printer.newPage()) {
            return makeError(ErrorCode::FileExportFailure, "the printer refused a new page",
                             report.printer.toStdString());
        }
        // What the printer gave: its whole page, in its device pixels. A
        // sheet bigger than that is scaled down to fit and centred; a
        // smaller one is printed at its size in the top-left corner, as the
        // page was asked for at its size.
        const int dpi = printer.resolution();
        const QRectF page = printer.pageLayout().fullRectPixels(dpi);
        const katana::cad::PaperDimensions paper =
            katana::cad::paperDimensions(sheet.paper, sheet.landscape);
        const double ppmm = dpi / 25.4;
        const double sheetW = paper.widthMm * ppmm;
        const double sheetH = paper.heightMm * ppmm;
        const double scale =
            std::min({1.0, page.width() / sheetW, page.height() / sheetH});
        QPointF offset(0.0, 0.0);
        if (scale < 0.999) {
            offset = QPointF(0.5 * (page.width() - sheetW * scale),
                             0.5 * (page.height() - sheetH * scale));
            report.shrunk.emplace_back(index, scale);
        }
        const SheetPaintOptions options = paintOptions(request, ppmm, dpi);
        const SheetPaintStats stats = paintPage(painter, set, index, source(), options, cache,
                                                offset, scale < 0.999 ? scale : 1.0);
        collectProblems(report, set, index, stats);
        report.sheets.push_back(index);
        steps.done();
    }
    if (painter.isActive() && !painter.end()) {
        return makeError(ErrorCode::FileExportFailure, "could not finish printing",
                         report.printer.toStdString());
    }
    steps.finish();
    return report;
}

Result<PlotReport> printSheets(const QString& printerName, const plotting::SheetSet& set,
                               const PlotRequest& request, const SheetSourceProvider& source,
                               SheetPaintCache& cache, const PlotProgress& progress)
{
    const QPrinterInfo info = printerName.isEmpty() ? QPrinterInfo::defaultPrinter()
                                                    : QPrinterInfo::printerInfo(printerName);
    if (info.isNull()) {
        return makeError(ErrorCode::NotFound,
                         printerName.isEmpty() ? "there is no default printer"
                                               : "there is no printer of that name",
                         printerName.toStdString());
    }
    QPrinter printer(info, QPrinter::HighResolution);
    return printSheets(printer, set, request, source, cache, progress);
}

} // namespace katana::qt
