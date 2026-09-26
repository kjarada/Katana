#pragma once

// The small pieces every annotation manager's form is made of
// (annotation_managers.cpp, leader_manager.cpp): a named spin box, check
// box, line edit, button and problem line, an enumeration's names in a
// combo, and a refusal in the command line's own words. One place, so the
// managers' fields are named, sized and worded alike. Internal to
// src/katana_qt/annotation/.

#include <cstddef>

#include <QCheckBox>
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QString>

#include "katana/core/error.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/tables.hpp"

namespace katana::qt::annotation_form {

inline QDoubleSpinBox* spin(QWidget* parent, const char* name, double minimum, double maximum,
                            double step, int decimals, const QString& suffix = {})
{
    auto* box = new QDoubleSpinBox(parent);
    box->setObjectName(QString::fromLatin1(name));
    box->setRange(minimum, maximum);
    box->setSingleStep(step);
    box->setDecimals(decimals);
    box->setSuffix(suffix);
    return box;
}

inline QCheckBox* check(QWidget* parent, const char* name, const QString& text)
{
    auto* box = new QCheckBox(text, parent);
    box->setObjectName(QString::fromLatin1(name));
    return box;
}

inline QLineEdit* edit(QWidget* parent, const char* name, const QString& placeholder = {})
{
    auto* line = new QLineEdit(parent);
    line->setObjectName(QString::fromLatin1(name));
    line->setPlaceholderText(placeholder);
    return line;
}

inline QPushButton* button(QWidget* parent, const char* name, const QString& text)
{
    auto* push = new QPushButton(text, parent);
    push->setObjectName(QString::fromLatin1(name));
    push->setAutoDefault(false);
    return push;
}

inline QLabel* problemLine(QWidget* parent, const char* name)
{
    auto* label = new QLabel(parent);
    label->setObjectName(QString::fromLatin1(name));
    label->setWordWrap(true);
    label->setStyleSheet(QStringLiteral("color: #d9534f"));
    return label;
}

// An enumeration's names in a combo, by value.
template <typename Enum, std::size_t N>
QComboBox* combo(QWidget* parent, const char* name, const Enum (&values)[N])
{
    auto* box = new QComboBox(parent);
    box->setObjectName(QString::fromLatin1(name));
    for (const Enum value : values) {
        box->addItem(QString::fromUtf8(katana::entity::toString(value)), static_cast<int>(value));
    }
    return box;
}

template <typename Enum> Enum chosen(const QComboBox* box)
{
    return static_cast<Enum>(box->currentData().toInt());
}

template <typename Enum> void choose(QComboBox* box, Enum value)
{
    box->setCurrentIndex(box->findData(static_cast<int>(value)));
}

// A refusal as the problem line says it: the message, and what it was about.
inline QString describe(const katana::core::Status& status)
{
    if (status) {
        return {};
    }
    const auto& error = status.error();
    return QString::fromStdString(error.message +
                                  (error.context.empty() ? std::string() : ": " + error.context));
}

} // namespace katana::qt::annotation_form
