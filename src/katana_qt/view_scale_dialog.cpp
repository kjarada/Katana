#include "view_scale_dialog.hpp"

#include <utility>

#include <QComboBox>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSignalBlocker>
#include <QVBoxLayout>

#include "katana/core/text.hpp"
#include "theme.hpp"

namespace katana::qt {

ViewScaleDialog::ViewScaleDialog(const katana::cad::ViewSet& views, katana::cad::ViewId view,
                                 CommandRunner run, QWidget* parent)
    : QDialog(parent), views_(views), run_(std::move(run))
{
    setObjectName("viewScaleDialog");
    setWindowTitle("Paper Style Scale");
    setModal(false);
    auto* layout = new QVBoxLayout(this);
    auto* explanation = new QLabel(
        "Size paper-based library linestyles and symbols in this plan view.\n"
        "Auto keeps their screen size. Fixed 1:N uses N/1000 metres per paper millimetre; "
        "larger N makes the marks larger. Camera zoom, ground-sized marks, annotation "
        "and plots are unchanged.", this);
    explanation->setObjectName("viewScaleExplanation");
    explanation->setWordWrap(true);
    layout->addWidget(explanation);

    auto* form = new QFormLayout();
    const auto row = [this, form](const char* name, const QString& text, QWidget* field) {
        auto* label = new QLabel(text, this);
        label->setObjectName(name);
        label->setBuddy(field);
        form->addRow(label, field);
    };
    view_ = new QComboBox(this);
    view_->setObjectName("viewScaleView");
    row("viewScaleViewLabel", "&Plan view:", view_);
    mode_ = new QComboBox(this);
    mode_->setObjectName("viewScaleMode");
    mode_->addItem("Auto (fixed screen size)");
    mode_->addItem("Fixed 1:N");
    row("viewScaleModeLabel", "&Sizing:", mode_);
    denominator_ = new QLineEdit(this);
    denominator_->setObjectName("viewScaleDenominator");
    denominator_->setPlaceholderText("For example, 500 or 1:500");
    denominator_->setToolTip("Positive finite N in 1:N; this is not the camera zoom");
    row("viewScaleDenominatorLabel", "&Denominator N:", denominator_);
    line_ = new QLabel(this);
    line_->setObjectName("viewScaleLine");
    line_->setWordWrap(true);
    line_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    row("viewScaleLineLabel", "Command:", line_);
    layout->addLayout(form);

    status_ = new QLabel(this);
    status_->setObjectName("viewScaleStatus");
    status_->setWordWrap(true);
    status_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    layout->addWidget(status_);
    auto* buttons = new QDialogButtonBox(this);
    buttons->setObjectName("viewScaleButtons");
    auto* applyButton = buttons->addButton("&Apply", QDialogButtonBox::ApplyRole);
    applyButton->setObjectName("viewScaleApply");
    applyButton->setDefault(true);
    auto* close = buttons->addButton(QDialogButtonBox::Close);
    close->setObjectName("viewScaleClose");
    layout->addWidget(buttons);
    connect(applyButton, &QPushButton::clicked, this, [this] { (void)apply(); });
    connect(close, &QPushButton::clicked, this, &QDialog::close);
    connect(view_, &QComboBox::currentIndexChanged, this, [this] { readView(); });
    connect(mode_, &QComboBox::currentIndexChanged, this, [this] { showLine(); });
    connect(denominator_, &QLineEdit::textChanged, this, [this] { showLine(); });
    refresh(view);
}

void ViewScaleDialog::refresh(katana::cad::ViewId view)
{
    if (view == katana::cad::kNoView) {
        view = view_->currentData().toUInt();
    }
    const QSignalBlocker quiet(view_);
    view_->clear();
    for (const katana::cad::ViewState* state : views_.views()) {
        if (state->kind == katana::cad::ViewKind::Plan) {
            view_->addItem(QString("%1 (view %2)")
                               .arg(QString::fromStdString(katana::cad::ViewSet::title(*state)))
                               .arg(state->id),
                           state->id);
        }
    }
    view_->setCurrentIndex(view_->findData(view));
    readView();
}

void ViewScaleDialog::readView()
{
    const auto* state = views_.find(view_->currentData().toUInt());
    const std::optional<double> scale = state != nullptr ? state->paperStyleScale : std::nullopt;
    const QSignalBlocker quietMode(mode_);
    const QSignalBlocker quietDenominator(denominator_);
    mode_->setCurrentIndex(scale ? 1 : 0);
    denominator_->setText(scale ? QString::fromStdString(katana::core::formatExactReal(*scale))
                                : QString());
    showLine();
}

katana::core::Result<QString> ViewScaleDialog::line() const
{
    const auto id = view_->currentData().toUInt();
    const auto* state = views_.find(id);
    if (state == nullptr || state->kind != katana::cad::ViewKind::Plan) {
        return katana::core::makeError(katana::core::ErrorCode::NotFound,
                                       "choose an open plan view; VIEWS lists them");
    }
    const QString value = mode_->currentIndex() == 0
                              ? QStringLiteral("AUTO") : denominator_->text().trimmed();
    const auto parsed = katana::cad::parsePaperStyleScale(value.toStdString());
    if (!parsed) {
        return parsed.error();
    }
    return QString("VIEWS SCALE %1 %2").arg(id).arg(
        *parsed ? QString::fromStdString(katana::core::formatExactReal(**parsed)) : "AUTO");
}

void ViewScaleDialog::showLine()
{
    denominator_->setEnabled(mode_->currentIndex() == 1);
    const auto command = line();
    line_->setText(command ? *command : QString::fromStdString(command.error().message));
}

void ViewScaleDialog::setStatus(const QString& text, bool error)
{
    status_->setText(text);
    status_->setStyleSheet(error ? QString("color: %1;").arg(theme::error().name()) : QString());
}

bool ViewScaleDialog::apply()
{
    const auto command = line();
    if (!command) {
        setStatus(QString::fromStdString(command.error().describe()), true);
        return false;
    }
    if (!run_) {
        setStatus("No command executor is available.", true);
        return false;
    }
    const VerbOutcome outcome = run_(*command);
    setStatus(outcome.ok ? outcome.reply : outcome.error, !outcome.ok);
    return outcome.ok;
}

} // namespace katana::qt
