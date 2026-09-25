#pragma once

// A name as the command line reads it, for a dialog that builds the verb line
// a person would type (command_runner.hpp): double-quoted when it holds a
// blank - or is empty - which the interpreter's tokenizer would otherwise
// split or drop. A double quote or a line break cannot be carried at all,
// since the tokenizer's quoted words have no escape, so a name holding one is
// refused naming `what` rather than cut short at the quote.
//
// The rule itself is katana_cad's (cad::annotation::commandWord), the one the
// annotation dialogs use: this only carries a QString to it and names the
// field in the refusal, so every dialog writes a word the same way.

#include <QString>

#include "katana/cad/annotation/command_words.hpp"
#include "katana/core/error.hpp"

namespace katana::qt {

[[nodiscard]] inline katana::core::Result<QString> commandWord(const QString& text,
                                                               const QString& what)
{
    auto word = katana::cad::annotation::commandWord(text.toStdString());
    if (!word) {
        return katana::core::makeError(word.error().code,
                                       what.toStdString() + ": " + word.error().message,
                                       text.toStdString());
    }
    return QString::fromStdString(*word);
}

} // namespace katana::qt
