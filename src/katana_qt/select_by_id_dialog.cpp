#include "select_by_id_dialog.hpp"

#include <algorithm>
#include <utility>

#include <QCheckBox>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QRegularExpression>
#include <QVBoxLayout>

#include "katana/cad/command_interpreter.hpp"

namespace katana::qt {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::entity::EntityId;

katana::core::Result<std::vector<EntityId>> parseEntityIds(const QString& text)
{
    static const QRegularExpression separators(QStringLiteral("[,\\s]+"));
    std::vector<EntityId> ids;
    for (const QString& word : text.split(separators, Qt::SkipEmptyParts)) {
        // The interpreter's own test of an id, so what is accepted here is
        // what INFO and SELECT accept.
        const std::string plain = word.toStdString();
        if (!katana::cad::CommandInterpreter::isEntityId(plain)) {
            return makeError(ErrorCode::InvalidArgument, "not an entity id", plain);
        }
        const EntityId id = word.mid(word.startsWith('#') ? 1 : 0).toULongLong();
        if (std::ranges::find(ids, id) == ids.end()) {
            ids.push_back(id);
        }
    }
    if (ids.empty()) {
        return makeError(ErrorCode::InvalidArgument, "there is no id to select");
    }
    return ids;
}

QString selectByIdLine(const std::vector<EntityId>& ids, const std::vector<EntityId>& current,
                       bool add)
{
    std::vector<EntityId> wanted;
    if (add) {
        wanted = current;
    }
    for (const EntityId id : ids) {
        if (std::ranges::find(wanted, id) == wanted.end()) {
            wanted.push_back(id);
        }
    }
    QString line = QStringLiteral("SELECT");
    for (const EntityId id : wanted) {
        line += ' ' + QString::number(id);
    }
    return line;
}

SelectByIdDialog::SelectByIdDialog(SelectByIdContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName(QStringLiteral("selectByIdDialog"));
    setWindowTitle(QStringLiteral("Select by ID"));
    auto* layout = new QVBoxLayout(this);
    auto* form = new QFormLayout();

    ids_ = new QLineEdit(this);
    ids_->setObjectName(QStringLiteral("selectByIdIds"));
    ids_->setPlaceholderText(QStringLiteral("12, 15 #40"));
    ids_->setToolTip(QStringLiteral(
        "The entity ids LIST, INFO and AREA print, separated by commas or spaces; #12 is 12"));
    form->addRow(QStringLiteral("&Ids:"), ids_);

    add_ = new QCheckBox(QStringLiteral("&Add to the current selection"), this);
    add_->setObjectName(QStringLiteral("selectByIdAdd"));
    form->addRow(QString(), add_);
    zoom_ = new QCheckBox(QStringLiteral("&Zoom to what is selected"), this);
    zoom_->setObjectName(QStringLiteral("selectByIdZoom"));
    zoom_->setChecked(true);
    form->addRow(QString(), zoom_);
    layout->addLayout(form);

    status_ = new QLabel(this);
    status_->setObjectName(QStringLiteral("selectByIdStatus"));
    status_->setWordWrap(true);
    status_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    layout->addWidget(status_);

    auto* buttons = new QDialogButtonBox(this);
    QPushButton* select = buttons->addButton(QStringLiteral("&Select"),
                                             QDialogButtonBox::AcceptRole);
    select->setObjectName(QStringLiteral("selectByIdSelect"));
    select->setDefault(true);
    QPushButton* close = buttons->addButton(QDialogButtonBox::Close);
    close->setObjectName(QStringLiteral("selectByIdClose"));
    layout->addWidget(buttons);

    // Select keeps the dialog open, for the next id; Close closes it. Enter
    // in the ids presses Select, the default button.
    connect(select, &QPushButton::clicked, this, [this] { (void)selectIds(); });
    connect(close, &QPushButton::clicked, this, &QDialog::close);
}

bool SelectByIdDialog::selectIds()
{
    const auto ids = parseEntityIds(ids_->text());
    if (!ids) {
        status_->setText(QString::fromStdString(ids.error().describe()));
        return false;
    }
    const QString line =
        selectByIdLine(*ids, context_.document->selection().ids(), add_->isChecked());
    const VerbOutcome outcome = context_.run
                                    ? context_.run(line)
                                    : VerbOutcome{false, {}, QStringLiteral("nothing runs it")};
    if (!outcome.ok) {
        status_->setText(outcome.error);
        return false;
    }
    status_->setText(outcome.reply);
    if (context_.onSelected) {
        context_.onSelected(zoom_->isChecked());
    }
    return true;
}

} // namespace katana::qt
