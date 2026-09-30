#include "zoom_to_dialog.hpp"

#include <utility>

#include <QComboBox>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QLabel>
#include <QPushButton>
#include <QRegularExpression>
#include <QVBoxLayout>

#include "customisation/scope_filter_widget.hpp"
#include "theme.hpp"
#include "view_workspace.hpp"

namespace katana::qt {

QString ZoomToDialog::spokenReply(const QString& reply)
{
    // The records a ZOOM on a scope answers (cad/view_verbs.hpp): the
    // scope's, then the view it framed - with shown=, how many of them it
    // shows - then each view that followed it; or, when the view shows none
    // of them, that view's record with moved=no.
    static const QRegularExpression matchedField(QStringLiteral("\\bmatched=(\\d+)"));
    static const QRegularExpression shownField(QStringLiteral("\\bshown=(\\d+)"));
    static const QRegularExpression viewField(QStringLiteral("^view=(\\d+)"));
    long long matched = -1;
    long long shown = -1;
    QString framed;
    QString unmoved;
    QStringList followed;
    for (const QString& line : reply.split('\n', Qt::SkipEmptyParts)) {
        if (line.startsWith(QStringLiteral("scope="))) {
            if (const auto found = matchedField.match(line); found.hasMatch()) {
                matched = found.captured(1).toLongLong();
            }
            continue;
        }
        const auto view = viewField.match(line);
        if (!view.hasMatch()) {
            continue;
        }
        if (line.contains(QStringLiteral(" followed="))) {
            followed.append(view.captured(1));
            continue;
        }
        if (line.contains(QStringLiteral(" moved=no"))) {
            unmoved = view.captured(1);
        } else {
            framed = view.captured(1);
        }
        if (const auto found = shownField.match(line); found.hasMatch()) {
            shown = found.captured(1).toLongLong();
        }
    }
    if (matched == 0) {
        return QStringLiteral("Nothing matched, so no view moved.");
    }
    const QString took = matched > 0 ? QStringLiteral("%1 matched").arg(matched)
                                     : QStringLiteral("ZOOM answered");
    if (!unmoved.isEmpty() || framed.isEmpty()) {
        // It said "framed in view 2" of a view that showed none of it.
        return took + (unmoved.isEmpty()
                           ? QStringLiteral(", and no view moved.")
                           : QStringLiteral(", but view %1 shows none of them: nothing moved.")
                                 .arg(unmoved));
    }
    QString text = took + (shown >= 0 && shown < matched
                               ? QStringLiteral(", %1 of them shown and framed in view %2")
                                     .arg(shown)
                                     .arg(framed)
                               : QStringLiteral(", framed in view %1").arg(framed));
    if (!followed.isEmpty()) {
        const QString last = followed.takeLast();
        text += QStringLiteral("; view%1 %2 followed")
                    .arg(followed.isEmpty() ? QString() : QStringLiteral("s"))
                    .arg(followed.isEmpty() ? last : followed.join(", ") + " and " + last);
    }
    return text + '.';
}

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
            // Only the views that frame a scope (cad::zoomTakes): ZOOM would
            // refuse the rest by their kind.
            if (!katana::cad::zoomTakes(state->kind, katana::cad::ZoomRequest::Kind::Scope)) {
                continue;
            }
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
        setStatus(QString::fromStdString(text.error().describe()), true);
        return false;
    }
    const VerbOutcome outcome = context_.run
                                    ? context_.run(*text)
                                    : VerbOutcome{false, {}, QStringLiteral("nothing runs it")};
    // A sentence, and a refusal in the error colour, as Online Data words its
    // status: the raw records - long reals among them - wrapped over five
    // lines, and the dialog, grown only to its least height, pressed the
    // type grid above until its captions lost their descenders. The records
    // are in the command log the runner writes.
    setStatus(outcome.ok ? spokenReply(outcome.reply) : outcome.error, !outcome.ok);
    return outcome.ok;
}

void ZoomToDialog::setStatus(const QString& text, bool isError)
{
    status_->setText(text);
    status_->setStyleSheet(isError ? QStringLiteral("color: %1;").arg(theme::error().name())
                                   : QString());
}

} // namespace katana::qt
