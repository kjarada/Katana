#pragma once

// What the road model's dialogs (road_template_manager.hpp, corridor_dialog.hpp,
// corridor_quantities_dialog.hpp) are built from: their buttons, fields and
// tables made the one way, and the one way they run a line and say what it
// answered. Every dialog here changes nothing itself - it builds the line a
// person would type and hands it to the window's one executor
// (command_runner.hpp) - so this is also where that rule lives for them.

#include <QAbstractItemView>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QString>
#include <QStringList>
#include <QTableWidget>

#include <optional>
#include <string>

#include "command_runner.hpp"
#include "command_word.hpp"
#include "katana/core/text.hpp"
#include "theme.hpp"

namespace katana::qt::road_ui {

inline QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

inline QPushButton* button(const QString& text, const QString& objectName, const QString& tip,
                           QWidget* parent)
{
    auto* made = new QPushButton(text, parent);
    made->setObjectName(objectName);
    made->setToolTip(tip);
    // Enter in a field belongs to that field, never to whichever button Qt
    // would make the default.
    made->setAutoDefault(false);
    return made;
}

inline QLineEdit* field(const QString& objectName, const QString& placeholder, const QString& tip,
                        QWidget* parent)
{
    auto* made = new QLineEdit(parent);
    made->setObjectName(objectName);
    made->setPlaceholderText(placeholder);
    made->setToolTip(tip);
    return made;
}

inline QTableWidget* table(const QString& objectName, const QStringList& headers, QWidget* parent)
{
    auto* made = new QTableWidget(0, static_cast<int>(headers.size()), parent);
    made->setObjectName(objectName);
    made->setHorizontalHeaderLabels(headers);
    made->verticalHeader()->setVisible(false);
    made->setSelectionBehavior(QAbstractItemView::SelectRows);
    made->setSelectionMode(QAbstractItemView::SingleSelection);
    made->setEditTriggers(QAbstractItemView::NoEditTriggers);
    made->horizontalHeader()->setStretchLastSection(true);
    return made;
}

// A cell, editable or not: a grid is a buffer, applied by one line for the grid.
inline QTableWidgetItem* cell(const QString& text, bool editable = false)
{
    auto* item = new QTableWidgetItem(text);
    const Qt::ItemFlags flags = item->flags();
    item->setFlags(editable ? (flags | Qt::ItemIsEditable) : (flags & ~Qt::ItemIsEditable));
    return item;
}

inline QLabel* muted(const QString& text, QWidget* parent)
{
    auto* label = new QLabel(text, parent);
    label->setWordWrap(true);
    label->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    return label;
}

inline void setStatus(QLabel* status, const QString& text, bool isError)
{
    status->setStyleSheet(
        QString("color: %1").arg((isError ? theme::error() : theme::textMuted()).name()));
    status->setText(text);
}

// A name as the command line reads it (command_word.hpp), or nullopt with
// the refusal on the status line.
inline std::optional<QString> word(QLabel* status, const QString& text, const QString& what)
{
    const auto quoted = commandWord(text, what);
    if (!quoted) {
        setStatus(status, qs(quoted.error().message), true);
        return std::nullopt;
    }
    return *quoted;
}

// key=value with the value quoted when it needs to be: "material=crushed rock".
inline std::optional<QString> option(QLabel* status, const QString& key, const QString& value)
{
    if (value.isEmpty()) {
        return key + "=";
    }
    return word(status, key + "=" + value, key);
}

// Runs `line` through the executor and shows what it said: the last line of
// its reply, or why it was refused. True when it was carried out.
inline bool run(const CommandRunner& runner, QLabel* status, const QString& line)
{
    if (!runner) {
        setStatus(status, "Nothing here can run " + line + ": the dialog has no command line.",
                  true);
        return false;
    }
    const VerbOutcome outcome = runner(line);
    if (outcome.ok) {
        const QStringList lines = outcome.reply.split('\n', Qt::SkipEmptyParts);
        setStatus(status, lines.isEmpty() ? line : lines.front(), false);
    } else {
        setStatus(status, outcome.error.isEmpty() ? line + " was refused." : outcome.error, true);
    }
    return outcome.ok;
}

// A number a cell holds, read as the command line reads one; nullopt for
// anything else.
inline std::optional<double> number(const QString& text)
{
    return katana::core::parseFiniteDouble(text.trimmed().toStdString());
}

} // namespace katana::qt::road_ui
