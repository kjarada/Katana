#include "plotting/plot_drawing_dialog.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <optional>
#include <string>
#include <string_view>

#include <QCheckBox>
#include <QComboBox>
#include <QDir>
#include <QDoubleSpinBox>
#include <QFileDialog>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QVBoxLayout>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/plotting/page_setup.hpp"

namespace katana::qt {

namespace {

using katana::cad::PaperSize;
using katana::cad::PlotColourMode;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

constexpr const char* kUsage =
    "usage: PLOT <file.pdf> [paper=A0|A1|A2|A3|A4] [landscape|portrait] [fit|scale=N] [dpi=N] "
    "[style=colour|grey|mono] [lineweight=F] [margin=MM]";

// The dialog's ranges, which the verb keeps too, so the two take the same
// plots: 72 dpi is a screen's and 1200 a plotter's finest; a margin wider than
// 50 mm leaves an A4 sheet a postcard.
constexpr double kMinimumDpi = 72.0;
constexpr double kMaximumDpi = 1200.0;
constexpr double kMaximumMarginMm = 50.0;

// In the order the dialog lists them.
constexpr std::array<std::pair<PaperSize, const char*>, 5> kPapers{
    {{PaperSize::A4, "A4"}, {PaperSize::A3, "A3"}, {PaperSize::A2, "A2"}, {PaperSize::A1, "A1"},
     {PaperSize::A0, "A0"}}};
constexpr std::array<PlotColourMode, 3> kModes{PlotColourMode::Colour, PlotColourMode::Greyscale,
                                               PlotColourMode::Monochrome};

const char* paperName(PaperSize paper)
{
    for (const auto& [size, name] : kPapers) {
        if (size == paper) {
            return name;
        }
    }
    return "A3";
}

QString numberText(double value)
{
    return QString::number(value, 'g', 10);
}

katana::core::Error refused(const std::string& why, const std::string& word)
{
    return makeError(ErrorCode::InvalidArgument, why + "; " + kUsage, word);
}

// A number in [low, high], or nullopt.
std::optional<double> numberIn(const QString& text, double low, double high)
{
    bool ok = false;
    const double value = text.toDouble(&ok);
    if (!ok || !std::isfinite(value) || value < low || value > high) {
        return std::nullopt;
    }
    return value;
}

} // namespace

Result<PlotDrawingRequest> parsePlotDrawing(const QString& line)
{
    const auto tokens = katana::cad::CommandInterpreter::tokenize(line.toStdString());
    if (!tokens) {
        return tokens.error();
    }
    if (tokens->size() < 2 || (*tokens)[1].empty()) {
        return makeError(ErrorCode::InvalidArgument, kUsage);
    }
    PlotDrawingRequest request;
    request.path = QString::fromStdString((*tokens)[1]);
    for (std::size_t at = 2; at < tokens->size(); ++at) {
        const std::string& raw = (*tokens)[at];
        const QString word = QString::fromStdString(raw);
        const QString upper = word.toUpper();
        if (upper == "FIT") {
            request.fit = true;
            continue;
        }
        if (upper == "LANDSCAPE" || upper == "PORTRAIT") {
            request.settings.landscape = upper == "LANDSCAPE";
            continue;
        }
        const qsizetype equals = word.indexOf('=');
        if (equals <= 0) {
            return refused("not a PLOT option", raw);
        }
        const QString key = word.left(equals).toLower();
        QString value = word.mid(equals + 1);
        if (key == "paper") {
            const auto found = std::ranges::find_if(kPapers, [&value](const auto& paper) {
                return value.compare(paper.second, Qt::CaseInsensitive) == 0;
            });
            if (found == kPapers.end()) {
                return refused("paper= is A0, A1, A2, A3 or A4", raw);
            }
            request.settings.paper = found->first;
        } else if (key == "scale") {
            // 1:500 as a scale rule writes it, or 500.
            if (value.startsWith("1:")) {
                value = value.mid(2);
            }
            bool ok = false;
            const double denominator = value.toDouble(&ok);
            if (!ok || !std::isfinite(denominator) || denominator <= 0.0) {
                return refused("scale= is a positive number, 500 or 1:500", raw);
            }
            request.settings.scaleDenominator = denominator;
            request.fit = false;
        } else if (key == "dpi") {
            const auto dpi = numberIn(value, kMinimumDpi, kMaximumDpi);
            if (!dpi) {
                return refused("dpi= is from 72 to 1200", raw);
            }
            request.settings.dpi = *dpi;
        } else if (key == "style") {
            const auto mode = katana::cad::plotColourModeFrom(value.toStdString());
            if (!mode) {
                return refused("style= is colour, grey or mono", raw);
            }
            request.settings.colourMode = *mode;
        } else if (key == "lineweight") {
            const auto factor = numberIn(value, katana::cad::plotting::kMinimumLineWeightScale,
                                         katana::cad::plotting::kMaximumLineWeightScale);
            if (!factor) {
                return refused("lineweight= is from 0.1 to 5", raw);
            }
            request.settings.lineWeightScale = *factor;
        } else if (key == "margin") {
            const auto margin = numberIn(value, 0.0, kMaximumMarginMm);
            if (!margin) {
                return refused("margin= is from 0 to 50 mm", raw);
            }
            request.settings.marginMm = *margin;
        } else {
            return refused("not a PLOT option", raw);
        }
    }
    return request;
}

QString plotDrawingCommandLine(const PlotDrawingRequest& request)
{
    const katana::cad::PlotSettings& settings = request.settings;
    return QString("PLOT \"%1\" paper=%2 %3 %4 dpi=%5 style=%6 lineweight=%7 margin=%8")
        .arg(QDir::fromNativeSeparators(request.path), paperName(settings.paper),
             settings.landscape ? "landscape" : "portrait",
             request.fit ? QString("fit") : "scale=" + numberText(settings.scaleDenominator),
             numberText(settings.dpi),
             QString::fromUtf8(katana::cad::toString(settings.colourMode).data(),
                               static_cast<qsizetype>(katana::cad::toString(settings.colourMode).size())),
             numberText(settings.lineWeightScale), numberText(settings.marginMm));
}

// ---- the dialog -----------------------------------------------------------------------------

PlotDrawingDialog::PlotDrawingDialog(PlotDrawingDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("plotDrawingDialog");
    setWindowTitle("Plot to PDF");
    setModal(false);
    const katana::cad::PlotSettings defaults;

    path_ = new QLineEdit(context_.suggestedPath, this);
    path_->setObjectName("plotDrawingPath");
    path_->setPlaceholderText("the PDF to write");
    auto* browseButton = new QPushButton("Browse...", this);
    browseButton->setObjectName("plotDrawingBrowse");
    auto* pathRow = new QHBoxLayout;
    pathRow->addWidget(path_, 1);
    pathRow->addWidget(browseButton);

    paper_ = new QComboBox(this);
    paper_->setObjectName("plotDrawingPaper");
    for (const auto& [size, name] : kPapers) {
        paper_->addItem(name);
    }
    paper_->setCurrentText(paperName(defaults.paper));
    orientation_ = new QComboBox(this);
    orientation_->setObjectName("plotDrawingOrientation");
    orientation_->addItems({"Landscape", "Portrait"});
    orientation_->setCurrentIndex(defaults.landscape ? 0 : 1);
    // Fitting is the default because it is what a first plot of any drawing
    // wants, and it picks a scale a scale rule carries.
    fit_ = new QCheckBox("Fit the drawing to the sheet at a standard scale", this);
    fit_->setObjectName("plotDrawingFit");
    fit_->setChecked(true);
    scale_ = new QDoubleSpinBox(this);
    scale_->setObjectName("plotDrawingScale");
    scale_->setRange(1.0, 1000000.0);
    scale_->setDecimals(0);
    scale_->setValue(defaults.scaleDenominator);
    scale_->setPrefix("1 : ");
    dpi_ = new QDoubleSpinBox(this);
    dpi_->setObjectName("plotDrawingDpi");
    dpi_->setRange(kMinimumDpi, kMaximumDpi);
    dpi_->setDecimals(0);
    dpi_->setValue(defaults.dpi);
    margin_ = new QDoubleSpinBox(this);
    margin_->setObjectName("plotDrawingMargin");
    margin_->setRange(0.0, kMaximumMarginMm);
    margin_->setDecimals(1);
    margin_->setValue(defaults.marginMm);
    margin_->setSuffix(" mm");
    colourMode_ = new QComboBox(this);
    colourMode_->setObjectName("plotDrawingColourMode");
    colourMode_->addItems({"Colour", "Greyscale", "Monochrome"});
    colourMode_->setToolTip("Colour as drawn (white printing black), every colour as its grey, "
                            "or black ink alone");
    lineWeight_ = new QDoubleSpinBox(this);
    lineWeight_->setObjectName("plotDrawingLineWeightScale");
    lineWeight_->setRange(katana::cad::plotting::kMinimumLineWeightScale,
                          katana::cad::plotting::kMaximumLineWeightScale);
    lineWeight_->setDecimals(2);
    lineWeight_->setSingleStep(0.1);
    lineWeight_->setValue(defaults.lineWeightScale);
    lineWeight_->setSuffix(" x");
    lineWeight_->setToolTip("Every line weight times this: 0.7 for a check plot, 1.4 for a bold "
                            "one; text, dashes and symbols keep their size");

    command_ = new QLineEdit(this);
    command_->setObjectName("plotDrawingCommand");
    command_->setReadOnly(true);
    command_->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    command_->setToolTip("The line Plot hands to the command line - type it there and it does "
                         "the same");

    auto* form = new QFormLayout;
    form->addRow("PDF:", pathRow);
    form->addRow("Paper:", paper_);
    form->addRow("Orientation:", orientation_);
    form->addRow(QString(), fit_);
    form->addRow("Scale, when not fitting:", scale_);
    form->addRow("Resolution (dpi):", dpi_);
    form->addRow("Margin:", margin_);
    form->addRow("Colours:", colourMode_);
    form->addRow("Line weights:", lineWeight_);
    form->addRow("Command:", command_);

    plot_ = new QPushButton("Plot", this);
    plot_->setObjectName("plotDrawingPlot");
    plot_->setDefault(true);
    status_ = new QLabel(this);
    status_->setObjectName("plotDrawingStatus");
    status_->setWordWrap(true);
    status_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    auto* close = new QPushButton("Close", this);
    close->setObjectName("plotDrawingClose");
    auto* buttons = new QHBoxLayout;
    buttons->addWidget(status_, 1);
    buttons->addWidget(plot_);
    buttons->addWidget(close);

    auto* layout = new QVBoxLayout(this);
    layout->addLayout(form);
    layout->addLayout(buttons);

    connect(path_, &QLineEdit::textChanged, this, [this] { refresh(); });
    for (QComboBox* box : {paper_, orientation_, colourMode_}) {
        connect(box, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    }
    for (QDoubleSpinBox* spin : {scale_, dpi_, margin_, lineWeight_}) {
        connect(spin, &QDoubleSpinBox::valueChanged, this, [this] { refresh(); });
    }
    connect(fit_, &QCheckBox::toggled, this, [this] { refresh(); });
    connect(browseButton, &QPushButton::clicked, this, [this] { browse(); });
    connect(plot_, &QPushButton::clicked, this, [this] { plot(); });
    connect(close, &QPushButton::clicked, this, [this] { hide(); });
    refresh();
}

Result<PlotDrawingRequest> PlotDrawingDialog::request() const
{
    const QString path = path_->text().trimmed();
    if (path.isEmpty()) {
        return makeError(ErrorCode::InvalidArgument, "no PDF is named");
    }
    PlotDrawingRequest request;
    request.path = path;
    request.fit = fit_->isChecked();
    katana::cad::PlotSettings& settings = request.settings;
    settings.paper = kPapers[static_cast<std::size_t>(paper_->currentIndex())].first;
    settings.landscape = orientation_->currentIndex() == 0;
    settings.scaleDenominator = scale_->value();
    settings.dpi = dpi_->value();
    settings.marginMm = margin_->value();
    settings.colourMode = kModes[static_cast<std::size_t>(colourMode_->currentIndex())];
    settings.lineWeightScale = lineWeight_->value();
    return request;
}

void PlotDrawingDialog::refresh()
{
    scale_->setEnabled(!fit_->isChecked());
    const auto made = request();
    command_->setText(made ? plotDrawingCommandLine(*made) : QString());
    plot_->setEnabled(made.ok());
}

void PlotDrawingDialog::plot()
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
    const VerbOutcome outcome = context_.run(plotDrawingCommandLine(*made));
    status_->setText(outcome.ok ? "Plotted: " + outcome.reply.section('\n', -1)
                                : "Not plotted: " + outcome.error.section('\n', -1));
}

void PlotDrawingDialog::browse()
{
    if (context_.headless && context_.headless()) {
        status_->setText("A headless session opens no file dialog: fill plotDrawingPath with the "
                         "path instead.");
        return;
    }
    QString path = QFileDialog::getSaveFileName(this, "Plot to PDF", path_->text().trimmed(),
                                                "PDF (*.pdf)");
    if (path.isEmpty()) {
        return;
    }
    if (!path.endsWith(".pdf", Qt::CaseInsensitive)) {
        path += ".pdf";
    }
    path_->setText(QDir::toNativeSeparators(path));
}

} // namespace katana::qt
