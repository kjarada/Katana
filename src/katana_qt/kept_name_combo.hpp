#pragma once

// A plain QComboBox of names - hatch patterns, dimension styles - that keeps a
// name it cannot list, as NamePicker does for linetypes and symbols (audit
// QT-02, decision D3). Header-only: the style manager and the layer manager
// share it, and neither is a library.
//
// The rule: a form shows what is STORED, and writes back what is shown. A
// combo whose setCurrentText silently fails on a name it lacks shows the
// previous row's name instead, and Save then writes that - which is how an
// imported hatch or dimension style name was lost. So an unknown name is
// put in the list, first, marked "(not defined)" in NamePicker's amber, and
// read back as its exact bytes.
//
// A form over several items whose values differ shows a "<varies>" row,
// which reads back as nullopt: "leave each item's own value".

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include <QBrush>
#include <QByteArray>
#include <QComboBox>
#include <QSignalBlocker>
#include <QString>

#include "customisation/name_picker.hpp"

namespace katana::qt::kept {

// The exact name, as bytes; invalid on the <varies> row.
inline constexpr int kNameRole = Qt::UserRole + 1;
// True on a row this helper inserted (a kept name, <varies>), which the next
// show() removes again.
inline constexpr int kInsertedRole = Qt::UserRole + 2;

inline const QString kVaries = QStringLiteral("<varies>");

inline QByteArray bytes(std::string_view name)
{
    return QByteArray(name.data(), static_cast<qsizetype>(name.size()));
}

// One choice: what the list says, and the name it stands for ("(ByLayer)"
// for "", say).
struct Choice {
    QString label{};
    std::string name{};
};

// Replaces the list. What was shown is NOT kept: call show() after.
inline void fill(QComboBox* box, const std::vector<Choice>& choices)
{
    const QSignalBlocker quiet(box);
    box->clear();
    for (const Choice& choice : choices) {
        box->addItem(choice.label);
        box->setItemData(box->count() - 1, bytes(choice.name), kNameRole);
    }
}

inline void removeInserted(QComboBox* box)
{
    for (int row = box->count() - 1; row >= 0; --row) {
        if (box->itemData(row, kInsertedRole).toBool()) {
            box->removeItem(row);
        }
    }
}

// Shows `name`, inserting it marked when no row holds it exactly.
inline void show(QComboBox* box, std::string_view name)
{
    const QSignalBlocker quiet(box);
    removeInserted(box);
    const QByteArray wanted = bytes(name);
    for (int row = 0; row < box->count(); ++row) {
        const QVariant stored = box->itemData(row, kNameRole);
        if (stored.isValid() && stored.toByteArray() == wanted) {
            box->setCurrentIndex(row);
            return;
        }
    }
    const QString text = QString::fromUtf8(wanted);
    box->insertItem(0, text + QStringLiteral(" (not defined)"));
    box->setItemData(0, wanted, kNameRole);
    box->setItemData(0, true, kInsertedRole);
    box->setItemData(0, QBrush(kUndefinedNameColour), Qt::ForegroundRole);
    box->setItemData(0,
                     QStringLiteral("Nothing in this drawing defines \"%1\"; it is kept as it "
                                    "is until you choose another")
                         .arg(text),
                     Qt::ToolTipRole);
    box->setCurrentIndex(0);
}

// Shows "<varies>": the items being edited do not agree.
inline void showVaries(QComboBox* box)
{
    const QSignalBlocker quiet(box);
    removeInserted(box);
    box->insertItem(0, kVaries);
    box->setItemData(0, true, kInsertedRole);
    box->setCurrentIndex(0);
}

// The name shown, exactly; nullopt on the <varies> row (or an empty list).
inline std::optional<std::string> current(const QComboBox* box)
{
    const QVariant stored = box->currentData(kNameRole);
    if (!stored.isValid()) {
        return std::nullopt;
    }
    const QByteArray name = stored.toByteArray();
    return std::string(name.constData(), static_cast<std::size_t>(name.size()));
}

} // namespace katana::qt::kept
