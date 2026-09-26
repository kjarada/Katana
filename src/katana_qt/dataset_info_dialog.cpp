#include "dataset_info_dialog.hpp"

#include <QDialogButtonBox>
#include <QFontDatabase>
#include <QPlainTextEdit>
#include <QVBoxLayout>

namespace katana::qt {

// ---- information ----------------------------------------------------------------------------

DatasetInfoDialog::DatasetInfoDialog(const QString& title, const QString& text, QWidget* parent)
    : QDialog(parent)
{
    setWindowTitle(title);
    resize(720, 480);
    auto* layout = new QVBoxLayout(this);
    auto* view = new QPlainTextEdit(text, this);
    view->setReadOnly(true);
    view->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    view->setLineWrapMode(QPlainTextEdit::NoWrap);
    layout->addWidget(view);
    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Close, this);
    connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);
    layout->addWidget(buttons);
}

} // namespace katana::qt
