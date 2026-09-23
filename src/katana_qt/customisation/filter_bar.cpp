#include "filter_bar.hpp"

#include <QButtonGroup>
#include <QHBoxLayout>
#include <QLineEdit>
#include <QToolButton>

namespace katana::qt {

FilterBar::FilterBar(const QStringList& chips, QWidget* parent) : QWidget(parent)
{
    auto* layout = new QHBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(4);

    search_ = new QLineEdit(this);
    search_->setObjectName(QStringLiteral("filterText"));
    search_->setPlaceholderText(tr("Search"));
    search_->setClearButtonEnabled(true);
    layout->addWidget(search_, 1);

    group_ = new QButtonGroup(this);
    group_->setExclusive(true);
    for (int index = 0; index < chips.size(); ++index) {
        auto* chip = new QToolButton(this);
        chip->setObjectName(chipObjectName(chips[index]));
        chip->setText(chips[index]);
        chip->setCheckable(true);
        chip->setAutoRaise(true);
        chip->setChecked(index == 0);
        group_->addButton(chip, index);
        layout->addWidget(chip);
        chips_.push_back(chip);
    }

    // Lambdas, not slots: no moc here. Each re-reads the callback when it
    // fires, so a dialog may set them after constructing the bar.
    QObject::connect(search_, &QLineEdit::textChanged, this, [this](const QString& text) {
        if (onTextChanged) {
            onTextChanged(text);
        }
    });
    QObject::connect(group_, &QButtonGroup::idToggled, this, [this](int index, bool checked) {
        // An exclusive group toggles the old chip off and the new one on;
        // only the one turned on is news.
        if (checked && onChipChanged) {
            onChipChanged(index);
        }
    });
}

QString FilterBar::chipObjectName(const QString& label)
{
    QString name = QStringLiteral("filter");
    for (const QChar c : label) {
        if (c.isLetterOrNumber()) {
            name.append(c);
        }
    }
    return name;
}

QString FilterBar::text() const { return search_->text(); }

void FilterBar::setText(const QString& text) { search_->setText(text); }

void FilterBar::setPlaceholderText(const QString& text) { search_->setPlaceholderText(text); }

int FilterBar::chip() const { return group_->checkedId(); }

QString FilterBar::chipLabel() const
{
    const int index = chip();
    return index >= 0 ? chips_[static_cast<std::size_t>(index)]->text() : QString();
}

void FilterBar::setChip(int index)
{
    if (index < 0 || static_cast<std::size_t>(index) >= chips_.size()) {
        return;
    }
    chips_[static_cast<std::size_t>(index)]->setChecked(true);
}

void FilterBar::setChipLabel(int index, const QString& label)
{
    if (index < 0 || static_cast<std::size_t>(index) >= chips_.size()) {
        return;
    }
    chips_[static_cast<std::size_t>(index)]->setText(label);
}

} // namespace katana::qt
