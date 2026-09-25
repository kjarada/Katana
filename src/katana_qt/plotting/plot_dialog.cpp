#include "plot_dialog.hpp"

#include <algorithm>
#include <cmath>
#include <memory>

#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDesktopServices>
#include <QDialogButtonBox>
#include <QDir>
#include <QDoubleSpinBox>
#include <QFileDialog>
#include <QFileInfo>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPrinter>
#include <QPrinterInfo>
#include <QProgressDialog>
#include <QPushButton>
#include <QRadioButton>
#include <QSpinBox>
#include <QStandardPaths>
#include <QUrl>
#include <QVBoxLayout>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/page_setup.hpp"

namespace katana::qt {

namespace plotting = katana::cad::plotting;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

// The format combo's item data for the printer, beside the PlotFormat names.
const QString kPrinterItem = QStringLiteral("printer");

QString qtText(const std::string& text) { return QString::fromStdString(text); }

// A line edit with a Browse button, as one row.
QWidget* browseRow(QLineEdit*& edit, const QString& editName, const QString& buttonName,
                   const std::function<void()>& browse, QWidget* parent)
{
    auto* row = new QWidget(parent);
    auto* layout = new QHBoxLayout(row);
    layout->setContentsMargins(0, 0, 0, 0);
    edit = new QLineEdit(row);
    edit->setObjectName(editName);
    auto* button = new QPushButton(QStringLiteral("Browse..."), row);
    button->setObjectName(buttonName);
    QObject::connect(button, &QPushButton::clicked, row, browse);
    layout->addWidget(edit, 1);
    layout->addWidget(button);
    return row;
}

} // namespace

// ---- the dialog -------------------------------------------------------------------------

PlotDialog::PlotDialog(const plotting::SheetSet& set, std::size_t current, bool allSheets,
                       const QString& suggestedFile, QWidget* parent)
    : QDialog(parent), set_(set), current_(std::min(current, set.sheets.empty() ? std::size_t{0}
                                                                              : set.sheets.size() - 1))
{
    setObjectName(QStringLiteral("plotDialog"));
    setWindowTitle(QStringLiteral("Plot Sheets"));
    setMinimumWidth(520);
    const plotting::PageSetup& setup = set_.pageSetup;
    auto* layout = new QVBoxLayout(this);
    form_ = new QFormLayout();
    layout->addLayout(form_);

    // Which sheets.
    auto* sheets = new QWidget(this);
    auto* sheetsLayout = new QVBoxLayout(sheets);
    sheetsLayout->setContentsMargins(0, 0, 0, 0);
    all_ = new QRadioButton(QString("All %1 sheets").arg(set_.sheets.size()), sheets);
    all_->setObjectName(QStringLiteral("plotSheetsAll"));
    QString currentName = QStringLiteral("Current sheet");
    if (current_ < set_.sheets.size()) {
        currentName += QString(" (%1, %2)")
                           .arg(current_ + 1)
                           .arg(qtText(set_.sheets[current_].name));
    }
    currentOnly_ = new QRadioButton(currentName, sheets);
    currentOnly_->setObjectName(QStringLiteral("plotSheetsCurrent"));
    auto* listRow = new QWidget(sheets);
    auto* listLayout = new QHBoxLayout(listRow);
    listLayout->setContentsMargins(0, 0, 0, 0);
    list_ = new QRadioButton(QStringLiteral("Sheets"), listRow);
    list_->setObjectName(QStringLiteral("plotSheetsList"));
    listText_ = new QLineEdit(listRow);
    listText_->setObjectName(QStringLiteral("plotSheetsText"));
    listText_->setPlaceholderText(QStringLiteral("1,3-5 or sheet ids"));
    listLayout->addWidget(list_);
    listLayout->addWidget(listText_, 1);
    sheetsSummary_ = new QLabel(sheets);
    sheetsSummary_->setObjectName(QStringLiteral("plotSheetsSummary"));
    sheetsLayout->addWidget(all_);
    sheetsLayout->addWidget(currentOnly_);
    sheetsLayout->addWidget(listRow);
    sheetsLayout->addWidget(sheetsSummary_);
    (allSheets ? all_ : currentOnly_)->setChecked(true);
    form_->addRow(QStringLiteral("Sheets"), sheets);

    // What is made.
    format_ = new QComboBox(this);
    format_->setObjectName(QStringLiteral("plotFormat"));
    format_->addItem(QStringLiteral("PDF, one file"), QStringLiteral("pdf"));
    format_->addItem(QStringLiteral("PDF, a file per sheet"), QStringLiteral("pdfs"));
    format_->addItem(QStringLiteral("PNG images, one per sheet"), QStringLiteral("png"));
    format_->addItem(QStringLiteral("TIFF images, one per sheet"), QStringLiteral("tiff"));
    format_->addItem(QStringLiteral("Printer"), kPrinterItem);
    format_->setCurrentIndex(setup.filePerSheet ? 1 : 0);
    form_->addRow(QStringLiteral("Output"), format_);

    colourMode_ = new QComboBox(this);
    colourMode_->setObjectName(QStringLiteral("plotColourMode"));
    colourMode_->addItem(QStringLiteral("Colour"), QStringLiteral("colour"));
    colourMode_->addItem(QStringLiteral("Greyscale"), QStringLiteral("greyscale"));
    colourMode_->addItem(QStringLiteral("Monochrome (all ink black)"), QStringLiteral("monochrome"));
    colourMode_->setCurrentIndex(
        colourMode_->findData(QString::fromUtf8(katana::cad::toString(setup.colourMode))));
    form_->addRow(QStringLiteral("Colours"), colourMode_);

    lineWeightScale_ = new QDoubleSpinBox(this);
    lineWeightScale_->setObjectName(QStringLiteral("plotLineWeightScale"));
    lineWeightScale_->setRange(plotting::kMinimumLineWeightScale, plotting::kMaximumLineWeightScale);
    lineWeightScale_->setSingleStep(0.1);
    lineWeightScale_->setDecimals(2);
    lineWeightScale_->setSuffix(QStringLiteral(" x"));
    lineWeightScale_->setValue(setup.lineWeightScale);
    lineWeightScale_->setToolTip(QStringLiteral("Every line weight is multiplied by this"));
    form_->addRow(QStringLiteral("Line weights"), lineWeightScale_);

    dpi_ = new QSpinBox(this);
    dpi_->setObjectName(QStringLiteral("plotDpi"));
    dpi_->setRange(static_cast<int>(plotting::kMinimumPlotDpi),
                   static_cast<int>(plotting::kMaximumPlotDpi));
    dpi_->setSingleStep(50);
    dpi_->setSuffix(QStringLiteral(" dpi"));
    dpi_->setValue(static_cast<int>(std::lround(setup.dpi)));
    dpi_->setToolTip(QStringLiteral("The resolution of an image, and of a PDF's 3D snapshot"));
    form_->addRow(QStringLiteral("Resolution"), dpi_);

    // Where it goes.
    const QFileInfo suggested(suggestedFile);
    fileRow_ = browseRow(
        file_, QStringLiteral("plotFile"), QStringLiteral("plotBrowseFile"),
        [this] {
            const QString path = QFileDialog::getSaveFileName(this, QStringLiteral("Plot to PDF"),
                                                              file_->text(), QStringLiteral("PDF (*.pdf)"));
            if (!path.isEmpty()) {
                file_->setText(QDir::toNativeSeparators(path));
            }
        },
        this);
    file_->setText(QDir::toNativeSeparators(suggestedFile));
    form_->addRow(QStringLiteral("File"), fileRow_);
    folderRow_ = browseRow(
        folder_, QStringLiteral("plotFolder"), QStringLiteral("plotBrowseFolder"),
        [this] {
            const QString path = QFileDialog::getExistingDirectory(this, QStringLiteral("Plot into Folder"),
                                                                   folder_->text());
            if (!path.isEmpty()) {
                folder_->setText(QDir::toNativeSeparators(path));
            }
        },
        this);
    folder_->setText(suggestedFile.isEmpty() ? QString()
                                             : QDir::toNativeSeparators(suggested.absolutePath()));
    form_->addRow(QStringLiteral("Folder"), folderRow_);
    pattern_ = new QLineEdit(qtText(setup.fileNamePattern), this);
    pattern_->setObjectName(QStringLiteral("plotPattern"));
    pattern_->setToolTip(QStringLiteral(
        "{n} the sheet's position, {n:02} padded to two, {N} the count, {set} the set number, "
        "{number} the sheet number, {name} the sheet's name, {id} its id"));
    form_->addRow(QStringLiteral("File names"), pattern_);
    patternPreview_ = new QLabel(this);
    patternPreview_->setObjectName(QStringLiteral("plotPatternPreview"));
    form_->addRow(QString(), patternPreview_);
    printer_ = new QComboBox(this);
    printer_->setObjectName(QStringLiteral("plotPrinter"));
    printer_->addItems(QPrinterInfo::availablePrinterNames());
    if (const int at = printer_->findText(QPrinterInfo::defaultPrinterName()); at >= 0) {
        printer_->setCurrentIndex(at);
    }
    form_->addRow(QStringLiteral("Printer"), printer_);

    openAfter_ = new QCheckBox(QStringLiteral("Open the result afterwards"), this);
    openAfter_->setObjectName(QStringLiteral("plotOpenAfter"));
    layout->addWidget(openAfter_);
    keepSetup_ = new QCheckBox(QStringLiteral("Keep these settings as the project's page setup"), this);
    keepSetup_->setObjectName(QStringLiteral("plotKeepSetup"));
    keepSetup_->setChecked(true);
    layout->addWidget(keepSetup_);

    problem_ = new QLabel(this);
    problem_->setObjectName(QStringLiteral("plotProblem"));
    problem_->setWordWrap(true);
    problem_->setStyleSheet(QStringLiteral("color: #c0392b;"));
    layout->addWidget(problem_);

    auto* buttons = new QDialogButtonBox(this);
    run_ = buttons->addButton(QStringLiteral("Plot"), QDialogButtonBox::AcceptRole);
    run_->setObjectName(QStringLiteral("plotRun"));
    run_->setDefault(true);
    QPushButton* cancel = buttons->addButton(QDialogButtonBox::Cancel);
    cancel->setObjectName(QStringLiteral("plotCancel"));
    connect(buttons, &QDialogButtonBox::accepted, this, &QDialog::accept);
    connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);
    layout->addWidget(buttons);

    for (QRadioButton* radio : {all_, currentOnly_, list_}) {
        connect(radio, &QRadioButton::toggled, this, [this] { refresh(); });
    }
    connect(listText_, &QLineEdit::textEdited, this, [this] {
        list_->setChecked(true);
        refresh();
    });
    for (QLineEdit* edit : {file_, folder_, pattern_}) {
        connect(edit, &QLineEdit::textChanged, this, [this] { refresh(); });
    }
    for (QComboBox* combo : {format_, colourMode_, printer_}) {
        connect(combo, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    }
    connect(lineWeightScale_, &QDoubleSpinBox::valueChanged, this, [this] { refresh(); });
    connect(dpi_, &QSpinBox::valueChanged, this, [this] { refresh(); });
    refresh();
}

bool PlotDialog::toPrinter() const
{
    return format_->currentData().toString() == kPrinterItem;
}

QString PlotDialog::printerName() const
{
    return printer_->currentText();
}

bool PlotDialog::openAfterwards() const
{
    return openAfter_->isChecked();
}

bool PlotDialog::keepAsPageSetup() const
{
    return keepSetup_->isChecked();
}

PlotRequest PlotDialog::request() const
{
    PlotRequest request;
    if (currentOnly_->isChecked()) {
        request.sheets = std::to_string(current_ + 1);
    } else if (list_->isChecked()) {
        request.sheets = listText_->text().toStdString();
    }
    const QString format = format_->currentData().toString();
    if (const auto parsed = plotFormatFrom(format.toStdString())) {
        request.format = *parsed;
    } else {
        request.format = set_.pageSetup.filePerSheet ? PlotFormat::PdfPerSheet : PlotFormat::Pdf;
    }
    request.colourMode = katana::cad::plotColourModeFrom(colourMode_->currentData().toString().toStdString())
                             .value_or(katana::cad::PlotColourMode::Colour);
    request.lineWeightScale = lineWeightScale_->value();
    request.dpi = dpi_->value();
    request.fileNamePattern = pattern_->text().toStdString();
    if (!toPrinter()) {
        request.destination = QDir::fromNativeSeparators(
            (plotsFilePerSheet(request.format) ? folder_ : file_)->text().trimmed());
    }
    return request;
}

Status PlotDialog::check() const
{
    const PlotRequest asked = request();
    if (toPrinter()) {
        if (printer_->count() == 0) {
            return makeError(ErrorCode::InvalidArgument, "there is no printer to print on");
        }
        // The printer takes no destination; everything else is checked alike.
        PlotRequest checked = asked;
        checked.destination = QStringLiteral("printer");
        return validatePlotRequest(set_, checked);
    }
    return validatePlotRequest(set_, asked);
}

void PlotDialog::refresh()
{
    const PlotRequest asked = request();
    listText_->setEnabled(list_->isChecked());
    // The sheets the choice names, as the set numbers them.
    if (const auto selection = plotting::parseSheetSelection(asked.sheets, set_)) {
        QString text = QString("%1 sheet%2").arg(selection->size()).arg(selection->size() == 1 ? "" : "s");
        if (selection->size() < set_.sheets.size()) {
            text += ": " + qtText(plotting::formatSheetSelection(*selection));
        }
        sheetsSummary_->setText(text);
    } else {
        sheetsSummary_->setText(QString());
    }
    const bool printer = toPrinter();
    const bool perSheet = !printer && plotsFilePerSheet(asked.format);
    form_->setRowVisible(fileRow_, !printer && !perSheet);
    form_->setRowVisible(folderRow_, perSheet);
    form_->setRowVisible(pattern_, perSheet);
    form_->setRowVisible(patternPreview_, perSheet);
    form_->setRowVisible(printer_, printer);
    openAfter_->setEnabled(!printer);
    if (perSheet) {
        const auto files = plannedFiles(set_, asked);
        patternPreview_->setText(files && !files->empty()
                                     ? QStringLiteral("First file: ") +
                                           QFileInfo(files->front()).fileName()
                                     : QString());
    }
    const Status status = check();
    problem_->setText(status ? QString() : qtText(status.error().message));
    run_->setEnabled(static_cast<bool>(status));
}

// ---- running a plot ---------------------------------------------------------------------

QString suggestedPlotFile(const katana::cad::Document& document)
{
    std::string name = plotting::sanitiseFileName(document.metadata().name);
    if (document.metadata().name.empty()) {
        name = "sheets";
    }
    QString folder = QStandardPaths::writableLocation(QStandardPaths::DocumentsLocation);
    if (const auto project = document.projectDirectory()) {
        folder = QString::fromStdWString(project->wstring());
    }
    return QDir(folder).filePath(QString::fromStdString(name) + QStringLiteral(".pdf"));
}

Result<PlotReport> plotWithProgress(QWidget* parent, const plotting::SheetSet& set,
                                    const PlotRequest& request, const SheetSourceProvider& source,
                                    QPrinter* printer)
{
    const plotting::SheetSet snapshot = set;
    QProgressDialog progress(parent);
    progress.setObjectName(QStringLiteral("plotProgress"));
    progress.setWindowTitle(QStringLiteral("Plotting"));
    progress.setLabelText(QStringLiteral("Starting the plot..."));
    progress.setCancelButtonText(QStringLiteral("Cancel"));
    progress.setWindowModality(Qt::ApplicationModal);
    progress.setMinimumDuration(400);
    progress.setAutoClose(false);
    progress.setAutoReset(false);
    const PlotProgress step = [&progress](std::size_t done, std::size_t total,
                                          const std::string& name) {
        progress.setMaximum(static_cast<int>(total));
        if (done < total) {
            progress.setLabelText(QString("Plotting sheet %1 of %2: %3")
                                      .arg(done + 1)
                                      .arg(total)
                                      .arg(QString::fromStdString(name)));
        }
        progress.setValue(static_cast<int>(done));
        // What a person does while a sheet is painted - Cancel, moving the
        // window - is seen between sheets. Until the progress dialog is up
        // (it waits minimumDuration, so a quick plot shows none) there is no
        // modal window to keep a click off the sheets, so only what is not
        // input is let through: the window repaints and timers run, and a
        // click waits for the plot to end.
        QApplication::processEvents(progress.isVisible() ? QEventLoop::AllEvents
                                                         : QEventLoop::ExcludeUserInputEvents);
        return !progress.wasCanceled();
    };
    SheetPaintCache cache;
    Result<PlotReport> result = printer != nullptr
                                    ? printSheets(*printer, snapshot, request, source, cache, step)
                                    : plotSheets(snapshot, request, source, cache, step);
    progress.close();
    return result;
}

void plotInteractively(QWidget* parent, const plotting::SheetSet& sheets, std::size_t current,
                       bool allSheets, const QString& suggestedFile, const QString& title,
                       const SheetSourceProvider& source, katana::cad::Document* document,
                       const std::function<void(const QString&, bool)>& report)
{
    // A copy: `sheets` may be the document's own set, which keeping the page
    // setup replaces.
    const plotting::SheetSet set = sheets;
    // One plot at a time: the progress dialog lets events through, and a
    // second Plot from them would paint over the first.
    static bool busy = false;
    const auto say = [&report](const QString& text, bool error) {
        if (report) {
            report(text, error);
        }
    };
    if (busy) {
        say(QStringLiteral("A plot is already running."), true);
        return;
    }
    if (set.sheets.empty()) {
        say(QStringLiteral("There are no sheets to plot. Generate some first."), true);
        return;
    }
    PlotDialog dialog(set, current, allSheets, suggestedFile, parent);
    if (dialog.exec() != QDialog::Accepted) {
        return;
    }
    PlotRequest request = dialog.request();
    request.title = title;
    if (document != nullptr && dialog.keepAsPageSetup()) {
        const plotting::PageSetup kept = pageSetupFor(request, set.pageSetup);
        if (kept != set.pageSetup) {
            if (Status status = plotting::setPageSetup(*document, kept); !status) {
                say(qtText(status.error().describe()), true);
            } else {
                say(QStringLiteral("Kept as the project's page setup."), false);
            }
        }
    }
    std::unique_ptr<QPrinter> printer;
    if (dialog.toPrinter()) {
        // A printer taken away since the dialog listed it would otherwise
        // quietly become the default one.
        const QPrinterInfo info = QPrinterInfo::printerInfo(dialog.printerName());
        if (info.isNull()) {
            say(QString("There is no printer named %1.").arg(dialog.printerName()), true);
            return;
        }
        printer = std::make_unique<QPrinter>(info, QPrinter::HighResolution);
    }
    busy = true;
    const Result<PlotReport> result = plotWithProgress(parent, set, request, source, printer.get());
    busy = false;
    if (!result) {
        say(qtText(result.error().describe()), true);
        return;
    }
    for (const std::string& problem : result->problems) {
        say(qtText(problem), true);
    }
    for (const auto& [index, scale] : result->shrunk) {
        say(QString("%1 was printed at %2% to fit the printer's paper.")
                .arg(qtText(set.sheets[index].name))
                .arg(std::lround(scale * 100.0)),
            false);
    }
    say(result->summary(request), false);
    if (dialog.openAfterwards() && !result->cancelled && !result->files.empty()) {
        const QString target = plotsFilePerSheet(request.format) ? request.destination
                                                                 : result->files.front();
        QDesktopServices::openUrl(QUrl::fromLocalFile(target));
    }
}

} // namespace katana::qt
