#include "zoom_to_dialog.hpp"

#include <utility>

#include <QComboBox>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QLabel>
#include <QPushButton>
#include <QVBoxLayout>

#include "customisation/scope_filter_widget.hpp"
#include "view_workspace.hpp"

namespace katana::qt {

ZoomToDialog::ZoomToDialog(ZoomToContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName(QStringLiteral("zoomToDialog"));
    setWindowTitle(QStringLiteral("Zoom To"));
    setModal(false);
    auto* layout = new QVBoxLayout(this);

    scope_ = new ScopeFilterWidget(QStringLiteral("zoomTo"), this);
    if (context_.views != nullptr) {
        ViewWorkspace* views = context_.views;
        scope_->views = [views] { return scopeFilterViews(views->viewSet()); };
    }
    scope_->onChanged = [this] { showLine(); };
    layout->addWidget(scope_);

    auto* form = new QFormLayout();
    view_ = new QComboBox(this);
    view_->setObjectName(QStringLiteral("zoomToView"));
    view_->setToolTip(QStringLiteral(
        "The view to frame what the scope takes in; the views linked with it follow"));
    form->addRow(QStringLiteral("&In:"), view_);
    line_ = new QLabel(this);
    line_->setObjectName(QStringLiteral("zoomToLine"));
    line_->setWordWrap(true);
    line_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    form->addRow(QStringLiteral("Line:"), line_);
    layout->addLayout(form);
    connect(view_, &QComboBox::currentIndexChanged, this, [this] { showLine(); });

    status_ = new QLabel(this);
    status_->setObjectName(QStringLiteral("zoomToStatus"));
    status_->setWordWrap(true);
    status_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    layout->addWidget(status_);

    auto* buttons = new QDialogButtonBox(this);
    QPushButton* run = buttons->addButton(QStringLiteral("&Zoom"), QDialogButtonBox::AcceptRole);
    run->setObjectName(QStringLiteral("zoomToRun"));
    run->setDefault(true);
    QPushButton* close = buttons->addButton(QDialogButtonBox::Close);
    close->setObjectName(QStringLiteral("zoomToClose"));
    layout->addWidget(buttons);
    // Zoom keeps the dialog open for the next scope; Close closes it.
    connect(run, &QPushButton::clicked, this, [this] { (void)zoom(); });
    connect(close, &QPushButton::clicked, this, &QDialog::close);

    refresh();
}

void ZoomToDialog::refresh()
{
    if (context_.document != nullptr) {
        scope_->reload(context_.document->model());
    } else {
        scope_->reloadViews();
    }
    // The choice survives by the view's id; a view closed since falls back
    // to the active view.
    const auto chosen = view_->currentData().toUInt();
    view_->blockSignals(true);
    view_->clear();
    view_->addItem(QStringLiteral("Active view"), 0U);
    if (context_.views != nullptr) {
        for (const katana::cad::ViewState* state : context_.views->viewSet().views()) {
            view_->addItem(QStringLiteral("%1 (view %2)")
                               .arg(QString::fromStdString(katana::cad::ViewSet::title(*state)))
                               .arg(state->id),
                           state->id);
        }
    }
    const int at = view_->findData(chosen);
    view_->setCurrentIndex(at >= 0 ? at : 0);
    view_->blockSignals(false);
    showLine();
}

katana::core::Result<QString> ZoomToDialog::line() const
{
    auto words = scope_->verbWords();
    if (!words) {
        return words.error();
    }
    QString text = QStringLiteral("ZOOM ") + *words;
    // No view= for the active view, so the line reads as a person types it.
    if (const auto id = view_->currentData().toUInt(); id != 0) {
        text += QStringLiteral(" view=%1").arg(id);
    }
    return text;
}

void ZoomToDialog::showLine()
{
    const auto text = line();
    line_->setText(text ? *text : QString::fromStdString(text.error().describe()));
}

bool ZoomToDialog::zoom()
{
    const auto text = line();
    if (!text) {
        status_->setText(QString::fromStdString(text.error().describe()));
        return false;
    }
    const VerbOutcome outcome = context_.run
                                    ? context_.run(*text)
                                    : VerbOutcome{false, {}, QStringLiteral("nothing runs it")};
    status_->setText(outcome.ok ? outcome.reply : outcome.error);
    return outcome.ok;
}

} // namespace katana::qt
